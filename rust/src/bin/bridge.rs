//! One blocking RNDIS session: DHCP, a utun address, gateway ARP, then exit.
//! `--routes` adds the two /1 routes for that session and removes them on the way out.
//! Without `--routes` this binary does not add routes or change DNS.

use std::io::ErrorKind;
use std::net::Ipv4Addr;
use std::os::unix::io::RawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tether::arp;
use tether::bridge::{self, OutDecision};
use tether::bsd_route::{self, CommandRoutes, SystemRunner};
use tether::dhcp::{self, ServerMessage};
use tether::frame;
use tether::netcfg;
use tether::rndis::{
    self, parse_init_cmplt, parse_query_cmplt, parse_set_cmplt, Message, OUR_MAX_TRANSFER_SIZE,
};
use tether::usb::{self, ClaimedDevice, ListedDevice, UsbError};

const OID_802_3_PERMANENT_ADDRESS: u32 = 0x0101_0101;
const OID_GEN_CURRENT_PACKET_FILTER: u32 = 0x0001_010E;
const PACKET_FILTER: u32 = 0x0000_000F;
const DHCP_WAIT: Duration = Duration::from_secs(3);
const ARP_WAIT: Duration = Duration::from_secs(1);
const ARP_TRIES: u32 = 3;
const BULK_OUT_TIMEOUT: Duration = Duration::from_secs(1);
const READ_SLICE: Duration = Duration::from_millis(100);
const OPT_SUBNET_MASK: u8 = 1;
const OPT_SERVER_ID: u8 = 54;

static STOP: AtomicBool = AtomicBool::new(false);
static ROLLBACK_FAILED: AtomicBool = AtomicBool::new(false);

type LiveStore = CommandRoutes<SystemRunner, fn(&str) -> Option<u32>>;

struct Proof {
    store: LiveStore,
    session: netcfg::Session,
    routes_path: PathBuf,
    pid_path: PathBuf,
    done: bool,
}

impl Proof {
    fn finish(&mut self) -> Result<bool, RunError> {
        self.done = true;
        bsd_route::finish_proof(
            &mut self.store,
            &self.session,
            &self.pid_path,
            &self.routes_path,
        )
        .map_err(|_| RunError::Failed("route rollback failed".into()))
    }
}

impl Drop for Proof {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        if bsd_route::finish_proof(
            &mut self.store,
            &self.session,
            &self.pid_path,
            &self.routes_path,
        )
        .is_err()
        {
            ROLLBACK_FAILED.store(true, Ordering::Relaxed);
        }
    }
}

struct Session {
    device: ClaimedDevice,
    name: String,
    mac: [u8; 6],
    phone_max: u32,
    tun: Option<RawFd>,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(fd) = self.tun.take() {
            unsafe { libc::close(fd) };
        }
    }
}

struct Assigned {
    address: Ipv4Addr,
    gateway: Ipv4Addr,
    mask: Ipv4Addr,
}

struct Offer {
    address: Ipv4Addr,
    server: Ipv4Addr,
    router: Option<Ipv4Addr>,
    mask: Option<Ipv4Addr>,
}

struct Counts {
    rx: u64,
    tx: u64,
    drops: u64,
    keepalives: u64,
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(msg) => {
            println!("{msg}");
            return ExitCode::from(1);
        }
    };
    if args.routes {
        if unsafe { libc::geteuid() } != 0 {
            println!("route proof needs root");
            return ExitCode::from(1);
        }
        if !install_stop_handler() {
            println!("could not arm SIGTERM");
            return ExitCode::from(1);
        }
        if let Err(err) = sweep_stale_proof() {
            return report(Err(err));
        }
    }
    report(run(args.seconds, args.routes))
}

fn report(result: Result<String, RunError>) -> ExitCode {
    match result {
        Ok(line) => {
            println!("{line}");
            ExitCode::from(0)
        }
        Err(RunError::NoRndis(devices)) => {
            println!("no RNDIS interface");
            if devices.is_empty() {
                println!("no USB devices read");
            }
            for device in devices {
                print_device(&device);
            }
            ExitCode::from(2)
        }
        Err(RunError::Access(msg)) => {
            println!("{msg}");
            ExitCode::from(3)
        }
        Err(RunError::Failed(msg)) => {
            println!("{msg}");
            ExitCode::from(1)
        }
        Err(RunError::Disconnected) => {
            println!("phone disconnected and did not come back");
            ExitCode::from(1)
        }
    }
}

fn run(seconds: u64, routes: bool) -> Result<String, RunError> {
    let first = run_once(seconds, true, routes);
    if ROLLBACK_FAILED.swap(false, Ordering::Relaxed) {
        return Err(RunError::Failed("route rollback failed".into()));
    }
    match first {
        Err(RunError::Disconnected) => {
            thread::sleep(Duration::from_millis(1000));
            let second = run_once(seconds, false, routes);
            if ROLLBACK_FAILED.swap(false, Ordering::Relaxed) {
                return Err(RunError::Failed("route rollback failed".into()));
            }
            second
        }
        other => other,
    }
}

fn run_once(seconds: u64, activate_alt: bool, routes: bool) -> Result<String, RunError> {
    let mut session = claim_phone(activate_alt)?;
    let tun = open_utun()?;
    session.name = tun.name;
    session.tun = Some(tun.fd);

    let assigned = dhcp(&session)?;
    assign_address(&session.name, &assigned)?;
    let gateway_mac = resolve_gateway(&session, assigned.address, assigned.gateway)?;
    if !routes {
        let counts = forward(&session, seconds, assigned.address, gateway_mac, false)?;
        return Ok(format!(
            "utun={} address={} gateway={} rx={} tx={} drops={} keepalives={}",
            session.name,
            assigned.address,
            assigned.gateway,
            counts.rx,
            counts.tx,
            counts.drops,
            counts.keepalives
        ));
    }

    if STOP.load(Ordering::Relaxed) {
        return Err(RunError::Failed("stopped before routes".into()));
    }
    let mut proof = install_routes(&session.name, assigned.gateway)?;
    let counts = forward(&session, seconds, assigned.address, gateway_mac, true)?;
    let added = proof.session.recorded().len();
    let default_kept = proof.finish()?;
    if !default_kept {
        return Err(RunError::Failed("default route is absent".into()));
    }
    Ok(format!(
        "routes_added={added} default_kept=yes\nutun={} address={} gateway={} rx={} tx={} drops={} keepalives={}",
        session.name,
        assigned.address,
        assigned.gateway,
        counts.rx,
        counts.tx,
        counts.drops,
        counts.keepalives
    ))
}

fn claim_phone(activate_alt: bool) -> Result<Session, RunError> {
    let scan = usb::list_devices()?;
    let Some(found) = scan
        .devices
        .iter()
        .find(|device| device.rndis.is_some())
        .cloned()
    else {
        let saw_interface = scan
            .devices
            .iter()
            .any(|device| !device.interfaces.is_empty());
        let saw_access =
            scan.hidden_by_access || scan.devices.iter().any(|device| device.access_denied);
        if !saw_interface && saw_access {
            return Err(RunError::Access(
                "Access denied (insufficient permissions)".into(),
            ));
        }
        return Err(RunError::NoRndis(scan.devices));
    };
    let rndis = found.rndis.as_ref().expect("rndis match");
    let device = usb::claim(
        found.bus,
        found.address,
        found.vid,
        found.pid,
        rndis,
        activate_alt,
    )?;
    if device.bulk_in == 0 || device.bulk_out == 0 {
        return Err(RunError::Failed("RNDIS data endpoints missing".into()));
    }

    device.send_command(&rndis::build_init(1))?;
    thread::sleep(Duration::from_millis(100));
    let phone_max = match parse_init_cmplt(&device.read_response()?)? {
        Message::InitComplete {
            max_transfer_size, ..
        } => max_transfer_size,
        other => return Err(RunError::Failed(format!("INIT_CMPLT: {other:?}"))),
    };

    device.send_command(&rndis::build_query(2, OID_802_3_PERMANENT_ADDRESS, &[]))?;
    thread::sleep(Duration::from_millis(50));
    let mac = match parse_query_cmplt(&device.read_response()?)? {
        Message::QueryComplete { data, .. } => data,
        other => return Err(RunError::Failed(format!("QUERY_CMPLT: {other:?}"))),
    };
    if mac.len() != 6 {
        return Err(RunError::Failed(format!("MAC is {} bytes", mac.len())));
    }
    let mut mac_bytes = [0u8; 6];
    mac_bytes.copy_from_slice(&mac);

    device.send_command(&rndis::build_set(
        3,
        OID_GEN_CURRENT_PACKET_FILTER,
        &PACKET_FILTER.to_le_bytes(),
    ))?;
    thread::sleep(Duration::from_millis(50));
    match parse_set_cmplt(&device.read_response()?)? {
        Message::SetComplete { .. } => {}
        other => return Err(RunError::Failed(format!("SET_CMPLT: {other:?}"))),
    }

    Ok(Session {
        device,
        name: String::new(),
        mac: mac_bytes,
        phone_max,
        tun: None,
    })
}

fn dhcp(session: &Session) -> Result<Assigned, RunError> {
    let xid = dhcp_xid();
    let discover = bridge::wrap_dhcp(session.mac, &dhcp::build_discover(session.mac, xid));
    must_send(session, &discover)?;
    let offer = wait_offer(session, xid)?;

    let request = bridge::wrap_dhcp(
        session.mac,
        &dhcp::build_request(session.mac, xid, offer.address, offer.server),
    );
    must_send(session, &request)?;
    wait_ack(session, xid, &offer)
}

fn wait_offer(session: &Session, xid: u32) -> Result<Offer, RunError> {
    let deadline = Instant::now() + DHCP_WAIT;
    while Instant::now() < deadline {
        let Some(messages) = read_in(session, deadline)? else {
            continue;
        };
        for message in messages {
            let Message::Data { frame } = message else {
                continue;
            };
            let Some(bootp) = bootp_for(&frame, xid) else {
                continue;
            };
            match dhcp::parse_server_message(bootp) {
                Ok(ServerMessage::Offer(lease)) if !lease.address.is_unspecified() => {
                    return Ok(Offer {
                        address: lease.address,
                        server: option_ipv4(bootp, OPT_SERVER_ID).unwrap_or(Ipv4Addr::UNSPECIFIED),
                        router: lease.router,
                        mask: option_ipv4(bootp, OPT_SUBNET_MASK),
                    });
                }
                Ok(ServerMessage::Nak) => return Err(RunError::Failed("DHCP NAK".into())),
                _ => {}
            }
        }
    }
    Err(RunError::Failed("DHCP offer timed out".into()))
}

fn wait_ack(session: &Session, xid: u32, offer: &Offer) -> Result<Assigned, RunError> {
    let deadline = Instant::now() + DHCP_WAIT;
    while Instant::now() < deadline {
        let Some(messages) = read_in(session, deadline)? else {
            continue;
        };
        for message in messages {
            let Message::Data { frame } = message else {
                continue;
            };
            let Some(bootp) = bootp_for(&frame, xid) else {
                continue;
            };
            match dhcp::parse_server_message(bootp) {
                Ok(ServerMessage::Ack(lease)) if !lease.address.is_unspecified() => {
                    let gateway = lease
                        .router
                        .or(offer.router)
                        .ok_or_else(|| RunError::Failed("DHCP ACK has no gateway".into()))?;
                    let mask = option_ipv4(bootp, OPT_SUBNET_MASK)
                        .or(offer.mask)
                        .ok_or_else(|| RunError::Failed("DHCP ACK has no subnet mask".into()))?;
                    return Ok(Assigned {
                        address: lease.address,
                        gateway,
                        mask,
                    });
                }
                Ok(ServerMessage::Nak) => return Err(RunError::Failed("DHCP NAK".into())),
                _ => {}
            }
        }
    }
    Err(RunError::Failed("DHCP ACK timed out".into()))
}

fn resolve_gateway(
    session: &Session,
    our_ip: Ipv4Addr,
    gateway: Ipv4Addr,
) -> Result<[u8; 6], RunError> {
    for _ in 0..ARP_TRIES {
        let request = arp::build_request(session.mac, our_ip, gateway);
        must_send(session, &request)?;
        let deadline = Instant::now() + ARP_WAIT;
        while Instant::now() < deadline {
            let Some(messages) = read_in(session, deadline)? else {
                continue;
            };
            for message in messages {
                let Message::Data { frame } = message else {
                    continue;
                };
                if let Some(gateway_mac) = arp::accept_gateway_reply(&frame, gateway) {
                    return Ok(gateway_mac);
                }
                if let Some(reply) = arp::reply_if_ours(&frame, session.mac, our_ip) {
                    let _ = try_send(session, &reply);
                }
            }
        }
    }
    Err(RunError::Failed("gateway ARP timed out".into()))
}

fn forward(
    session: &Session,
    seconds: u64,
    our_ip: Ipv4Addr,
    gateway_mac: [u8; 6],
    watch_stop: bool,
) -> Result<Counts, RunError> {
    let fd = session
        .tun
        .ok_or_else(|| RunError::Failed("utun is closed".into()))?;
    let mut counts = Counts {
        rx: 0,
        tx: 0,
        drops: 0,
        keepalives: 0,
    };
    let mut inbound = vec![0u8; OUR_MAX_TRANSFER_SIZE as usize];
    let mut outbound = vec![0u8; 2048];
    let mut keepalive_id = 4u32;
    let mut keepalive_sent = 0u32;
    let started = Instant::now();
    let end = started + Duration::from_secs(seconds);
    while Instant::now() < end {
        if watch_stop && STOP.load(Ordering::Relaxed) {
            break;
        }
        if keepalive_due(started.elapsed(), keepalive_sent) {
            send_keepalive(session, keepalive_id)?;
            keepalive_id = keepalive_id.wrapping_add(1);
            keepalive_sent += 1;
            counts.keepalives += 1;
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let wait = left.min(READ_SLICE);
        match session.device.read_bulk(&mut inbound, wait) {
            Ok(0) | Err(UsbError::Timeout) => {}
            Err(UsbError::Disconnected) => return Err(RunError::Disconnected),
            Ok(len) => {
                for message in messages_of(&inbound[..len]) {
                    let Message::Data { frame } = message else {
                        continue;
                    };
                    if let Some(reply) = arp::reply_if_ours(&frame, session.mac, our_ip) {
                        submit_out(session, &reply, &mut counts)?;
                    } else if let Ok(packet) = frame::eth_to_utun(&frame) {
                        let written = write_utun(fd, &packet);
                        bridge::note_rx(&mut counts.rx, written);
                    }
                }
            }
            Err(err) => return Err(RunError::Failed(format!("bulk IN: {err}"))),
        }
        while Instant::now() < end {
            let Some(len) = read_utun(fd, &mut outbound) else {
                break;
            };
            match frame::utun_to_eth(&outbound[..len], session.mac, gateway_mac) {
                Ok(frame) => submit_out(session, &frame, &mut counts)?,
                Err(_) => counts.drops += 1,
            }
        }
    }
    Ok(counts)
}

/// Send the first keepalive immediately, then one every 30 seconds.
fn keepalive_due(elapsed: Duration, sent: u32) -> bool {
    elapsed >= Duration::from_secs(30) * sent
}

fn send_keepalive(session: &Session, request_id: u32) -> Result<(), RunError> {
    session
        .device
        .send_command(&rndis::build_keepalive(request_id))?;
    thread::sleep(Duration::from_millis(10));
    session.device.read_response()?;
    Ok(())
}

fn submit_out(session: &Session, frame: &[u8], counts: &mut Counts) -> Result<(), RunError> {
    match try_send(session, frame) {
        Ok(()) => counts.tx += 1,
        Err(SendError::Usb(UsbError::Disconnected)) => return Err(RunError::Disconnected),
        Err(SendError::Drop) | Err(SendError::Usb(_)) => counts.drops += 1,
    }
    Ok(())
}

fn must_send(session: &Session, frame: &[u8]) -> Result<(), RunError> {
    try_send(session, frame).map_err(|err| match err {
        SendError::Drop => {
            RunError::Failed("RNDIS packet larger than phone max transfer size".into())
        }
        SendError::Usb(UsbError::Disconnected) => RunError::Disconnected,
        SendError::Usb(err) => RunError::Failed(format!("bulk OUT: {err}")),
    })
}

fn try_send(session: &Session, frame: &[u8]) -> Result<(), SendError> {
    match bridge::decide_out(frame, session.phone_max) {
        OutDecision::Drop => Err(SendError::Drop),
        OutDecision::Send(packet) => session
            .device
            .write_bulk(&packet, BULK_OUT_TIMEOUT)
            .map_err(SendError::Usb),
    }
}

fn read_in(session: &Session, deadline: Instant) -> Result<Option<Vec<Message>>, RunError> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Ok(None);
    }
    let mut buf = vec![0u8; OUR_MAX_TRANSFER_SIZE as usize];
    match session.device.read_bulk(&mut buf, left) {
        Ok(0) | Err(UsbError::Timeout) => Ok(None),
        Err(UsbError::Disconnected) => return Err(RunError::Disconnected),
        Ok(len) => Ok(Some(messages_of(&buf[..len]))),
        Err(err) => Err(RunError::Failed(format!("bulk IN: {err}"))),
    }
}

fn messages_of(buf: &[u8]) -> Vec<Message> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset + 8 <= buf.len() {
        let mut len_bytes = [0u8; 4];
        len_bytes.copy_from_slice(&buf[offset + 4..offset + 8]);
        let msg_len = u32::from_le_bytes(len_bytes) as usize;
        if msg_len < 8 || offset + msg_len > buf.len() {
            break;
        }
        match rndis::unwrap_messages(&buf[offset..offset + msg_len]) {
            Ok(messages) => out.extend(messages),
            Err(_) => break,
        }
        offset += msg_len;
    }
    out
}

fn bootp_for(frame: &[u8], xid: u32) -> Option<&[u8]> {
    if frame.len() < 14 + 20 + 8 {
        return None;
    }
    if u16::from_be_bytes([frame[12], frame[13]]) != 0x0800 {
        return None;
    }
    let ihl = (frame[14] & 0x0f) as usize * 4;
    if frame[14] >> 4 != 4 || ihl < 20 || frame.len() < 14 + ihl + 8 {
        return None;
    }
    if frame[14 + 9] != 17 {
        return None;
    }
    let udp = 14 + ihl;
    let dest = u16::from_be_bytes([frame[udp + 2], frame[udp + 3]]);
    if dest != 68 {
        return None;
    }
    let udp_len = u16::from_be_bytes([frame[udp + 4], frame[udp + 5]]) as usize;
    if udp_len < 8 {
        return None;
    }
    let end = udp + udp_len;
    if end > frame.len() {
        return None;
    }
    let bootp = &frame[udp + 8..end];
    if bootp.len() < 240 || bootp[4..8] != xid.to_be_bytes() {
        return None;
    }
    Some(bootp)
}

fn option_ipv4(bootp: &[u8], code: u8) -> Option<Ipv4Addr> {
    let mut index = 240;
    while index < bootp.len() {
        let option = bootp[index];
        if option == 255 {
            break;
        }
        if option == 0 {
            index += 1;
            continue;
        }
        if index + 1 >= bootp.len() {
            break;
        }
        let len = bootp[index + 1] as usize;
        let start = index + 2;
        let end = start.checked_add(len)?;
        if end > bootp.len() {
            break;
        }
        if option == code && len >= 4 {
            return Some(Ipv4Addr::new(
                bootp[start],
                bootp[start + 1],
                bootp[start + 2],
                bootp[start + 3],
            ));
        }
        index = end;
    }
    None
}

fn assign_address(name: &str, assigned: &Assigned) -> Result<(), RunError> {
    if !is_utun_name(name) {
        return Err(RunError::Failed(format!("refusing ifconfig on {name}")));
    }
    ifconfig(&[
        name,
        "inet",
        &assigned.address.to_string(),
        &assigned.gateway.to_string(),
        "netmask",
        &assigned.mask.to_string(),
        "up",
    ])?;
    ifconfig(&[name, "mtu", "1500"])
}

fn ifconfig(args: &[&str]) -> Result<(), RunError> {
    let output = Command::new("/sbin/ifconfig")
        .args(args)
        .output()
        .map_err(|err| RunError::Failed(format!("ifconfig: {err}")))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr);
    Err(RunError::Failed(format!(
        "ifconfig {} failed: {}",
        args.join(" "),
        detail.trim()
    )))
}

fn is_utun_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("utun") else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
}

struct Tun {
    fd: RawFd,
    name: String,
}

fn open_utun() -> Result<Tun, RunError> {
    let fd = unsafe { libc::socket(libc::PF_SYSTEM, libc::SOCK_DGRAM, libc::SYSPROTO_CONTROL) };
    if fd < 0 {
        return Err(os_err("utun socket"));
    }
    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    match finish_utun(fd) {
        Ok(tun) => Ok(tun),
        Err(err) => {
            unsafe { libc::close(fd) };
            Err(err)
        }
    }
}

fn finish_utun(fd: RawFd) -> Result<Tun, RunError> {
    let mut info: libc::ctl_info = unsafe { std::mem::zeroed() };
    let control = b"com.apple.net.utun_control";
    for (index, byte) in control.iter().enumerate() {
        info.ctl_name[index] = *byte as libc::c_char;
    }
    if unsafe { libc::ioctl(fd, libc::CTLIOCGINFO, &mut info) } < 0 {
        return Err(os_err("utun control"));
    }

    let mut connected = false;
    for unit in 0..256 {
        let mut addr: libc::sockaddr_ctl = unsafe { std::mem::zeroed() };
        addr.sc_len = std::mem::size_of::<libc::sockaddr_ctl>() as libc::c_uchar;
        addr.sc_family = libc::AF_SYSTEM as libc::c_uchar;
        addr.ss_sysaddr = libc::AF_SYS_CONTROL as u16;
        addr.sc_id = info.ctl_id;
        addr.sc_unit = unit + 1;
        let rc = unsafe {
            libc::connect(
                fd,
                &addr as *const libc::sockaddr_ctl as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_ctl>() as libc::socklen_t,
            )
        };
        if rc == 0 {
            connected = true;
            break;
        }
    }
    if !connected {
        return Err(os_err("utun connect"));
    }

    let mut name_buf = [0u8; libc::IFNAMSIZ];
    let mut name_len = name_buf.len() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SYSPROTO_CONTROL,
            libc::UTUN_OPT_IFNAME,
            name_buf.as_mut_ptr() as *mut libc::c_void,
            &mut name_len,
        )
    };
    if rc < 0 {
        return Err(os_err("utun name"));
    }
    let end = name_buf
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(name_buf.len());
    let name = String::from_utf8_lossy(&name_buf[..end]).into_owned();
    if name.is_empty() {
        return Err(RunError::Failed("utun name is empty".into()));
    }

    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(os_err("utun nonblock"));
    }
    let bufsize: libc::c_int = 4 * 1024 * 1024;
    unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            &bufsize as *const libc::c_int as *const libc::c_void,
            std::mem::size_of_val(&bufsize) as libc::socklen_t,
        );
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            &bufsize as *const libc::c_int as *const libc::c_void,
            std::mem::size_of_val(&bufsize) as libc::socklen_t,
        );
    }
    Ok(Tun { fd, name })
}

fn read_utun(fd: RawFd, buf: &mut [u8]) -> Option<usize> {
    loop {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n > 0 {
            return Some(n as usize);
        }
        if n == 0 || std::io::Error::last_os_error().kind() != ErrorKind::Interrupted {
            return None;
        }
    }
}

fn write_utun(fd: RawFd, buf: &[u8]) -> usize {
    if buf.is_empty() {
        return 0;
    }
    loop {
        let n = unsafe { libc::write(fd, buf.as_ptr() as *const libc::c_void, buf.len()) };
        if n > 0 {
            return n as usize;
        }
        if n == 0 || std::io::Error::last_os_error().kind() != ErrorKind::Interrupted {
            return 0;
        }
    }
}

fn dhcp_xid() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or(0x1234_5678)
}

fn os_err(what: &str) -> RunError {
    RunError::Failed(format!("{what}: {}", std::io::Error::last_os_error()))
}

#[derive(Debug)]
struct Args {
    seconds: u64,
    routes: bool,
}

fn parse_args() -> Result<Args, String> {
    parse_args_from(std::env::args().skip(1))
}

fn parse_args_from<I, S>(args: I) -> Result<Args, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut seconds = 5u64;
    let mut routes = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_ref() {
            "--seconds" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--seconds needs a number".to_string())?;
                let value = value.as_ref();
                seconds = value
                    .parse()
                    .map_err(|_| format!("bad --seconds {value}"))?;
            }
            "--routes" => routes = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Args { seconds, routes })
}

fn install_routes(name: &str, gateway: Ipv4Addr) -> Result<Proof, RunError> {
    let store = bsd_route::command_store();
    let table = store
        .read_table()
        .map_err(|_| RunError::Failed("netstat failed".into()))?;
    if !table.default_present {
        return Err(RunError::Failed("default route is absent".into()));
    }
    let index = bsd_route::interface_index(name)
        .ok_or_else(|| RunError::Failed(format!("no interface index for {name}")))?;
    let mut proof = Proof {
        store,
        session: netcfg::Session::new(),
        routes_path: PathBuf::from(bsd_route::ROUTE_FILE),
        pid_path: PathBuf::from(bsd_route::PID_FILE),
        done: false,
    };
    bsd_route::install_half_routes(
        &mut proof.store,
        &mut proof.session,
        &proof.routes_path,
        &proof.pid_path,
        gateway,
        index,
        std::process::id(),
    )
    .map_err(|err| match err {
        bsd_route::ProofError::NotAdded(_) => RunError::Failed("route add did not succeed".into()),
        bsd_route::ProofError::Io(err) => RunError::Failed(format!("route record: {err}")),
        bsd_route::ProofError::Rollback => RunError::Failed("route rollback failed".into()),
    })?;
    Ok(proof)
}

fn sweep_stale_proof() -> Result<(), RunError> {
    let pid_path = Path::new(bsd_route::PID_FILE);
    let routes_path = Path::new(bsd_route::ROUTE_FILE);
    let Some(pid) = bsd_route::load_pid(pid_path)
        .map_err(|err| RunError::Failed(format!("pid file: {err}")))?
    else {
        return Ok(());
    };
    if bsd_route::pid_alive(pid) {
        return Err(RunError::Failed("route proof already running".into()));
    }
    let saved = bsd_route::load_routes(routes_path)
        .map_err(|err| RunError::Failed(format!("route record: {err}")))?;
    let mut store = bsd_route::command_store();
    netcfg::startup_sweep(&mut store, &netcfg::Pidfile { pid }, false, Some(&saved));
    let table = store
        .read_table()
        .map_err(|_| RunError::Failed("route sweep failed".into()))?;
    if saved.iter().any(|route| table.routes.contains(route)) {
        return Err(RunError::Failed("route sweep failed".into()));
    }
    Ok(())
}

fn install_stop_handler() -> bool {
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_stop as *const () as libc::sighandler_t;
        if libc::sigemptyset(&mut action.sa_mask) != 0 {
            return false;
        }
        action.sa_flags = libc::SA_RESTART;
        libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) == 0
            && libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut()) == 0
    }
}

extern "C" fn on_stop(_signal: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

fn print_device(device: &ListedDevice) {
    println!("vid={:04x} pid={:04x}", device.vid, device.pid);
    if device.access_denied {
        println!("  access denied");
        return;
    }
    if device.interfaces.is_empty() {
        println!("  no interfaces read");
        return;
    }
    for iface in &device.interfaces {
        println!(
            "  interface {} class={:02x} subclass={:02x} protocol={:02x}",
            iface.number, iface.class_code, iface.subclass, iface.protocol
        );
    }
}

enum SendError {
    Drop,
    Usb(UsbError),
}

enum RunError {
    NoRndis(Vec<ListedDevice>),
    Access(String),
    Disconnected,
    Failed(String),
}

impl From<UsbError> for RunError {
    fn from(err: UsbError) -> Self {
        match err {
            UsbError::Access(msg) => RunError::Access(msg),
            UsbError::Timeout => RunError::Failed("timeout".into()),
            UsbError::Disconnected => RunError::Disconnected,
            UsbError::Other(msg) => RunError::Failed(msg),
        }
    }
}

impl From<rndis::RndisError> for RunError {
    fn from(err: rndis::RndisError) -> Self {
        RunError::Failed(format!("{err:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_args_from;

    #[test]
    fn no_args_means_five_seconds_and_routes_off() {
        let args = parse_args_from(std::iter::empty::<&str>()).unwrap();
        assert_eq!(args.seconds, 5);
        assert!(!args.routes);
    }

    #[test]
    fn seconds_and_routes_flag() {
        let args = parse_args_from(["--seconds", "45", "--routes"]).unwrap();
        assert_eq!(args.seconds, 45);
        assert!(args.routes);
    }

    #[test]
    fn unknown_argument_is_an_error() {
        let err = parse_args_from(["--dns"]).unwrap_err();
        assert_eq!(err, "unknown argument --dns");
    }

    #[test]
    fn keepalive_is_due_immediately_and_then_every_30_seconds() {
        use std::time::Duration;
        assert!(super::keepalive_due(Duration::from_secs(0), 0));
        assert!(!super::keepalive_due(Duration::from_secs(29), 1));
        assert!(super::keepalive_due(Duration::from_secs(30), 1));
        assert!(!super::keepalive_due(Duration::from_secs(59), 2));
        assert!(super::keepalive_due(Duration::from_secs(60), 2));
    }
}
