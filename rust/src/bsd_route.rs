//! Command-backed `0.0.0.0/1` and `128.0.0.0/1` routes.
//! Callers pass a runner. Tests never construct `SystemRunner`.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::net::Ipv4Addr;
use std::path::Path;
use std::process::Command;

use crate::netcfg::{rollback, AddOutcome, Route, RouteStore, Session};

pub const PID_FILE: &str = "/tmp/tetherd-routeproof.pid";
pub const ROUTE_FILE: &str = "/tmp/tetherd-routeproof.routes";

const HALF_MASK: Ipv4Addr = Ipv4Addr::new(128, 0, 0, 0);
const LOW_DEST: Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);
const HIGH_DEST: Ipv4Addr = Ipv4Addr::new(128, 0, 0, 0);

#[derive(Debug)]
pub enum ProofError {
    Io(io::Error),
    NotAdded(AddOutcome),
    Rollback,
}

impl From<io::Error> for ProofError {
    fn from(err: io::Error) -> Self {
        ProofError::Io(err)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait CommandRunner {
    fn run(&self, argv: &[String]) -> io::Result<CommandOutput>;
}

/// Runs `route` and `netstat`. Unit tests do not construct this.
#[derive(Debug, Default)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, argv: &[String]) -> io::Result<CommandOutput> {
        let Some(program) = argv.first() else {
            return Err(io::Error::new(io::ErrorKind::Other, "empty command"));
        };
        let output = Command::new(program).args(&argv[1..]).output()?;
        Ok(CommandOutput {
            code: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

pub struct CommandRoutes<R, F> {
    runner: R,
    ifindex: F,
}

impl<R, F> CommandRoutes<R, F> {
    pub fn new(runner: R, ifindex: F) -> Self {
        Self { runner, ifindex }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteTable {
    pub routes: Vec<Route>,
    pub default_present: bool,
}

impl<R, F> CommandRoutes<R, F>
where
    R: CommandRunner,
    F: Fn(&str) -> Option<u32>,
{
    pub fn read_table(&self) -> io::Result<RouteTable> {
        let output = self.runner.run(&list_command())?;
        if output.code != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("netstat exited {}", output.code),
            ));
        }
        Ok(RouteTable {
            routes: parse_half_routes(&output.stdout, &self.ifindex),
            default_present: default_present(&output.stdout),
        })
    }
}

impl<R, F> RouteStore for CommandRoutes<R, F>
where
    R: CommandRunner,
    F: Fn(&str) -> Option<u32>,
{
    fn add(&mut self, route: &Route) -> AddOutcome {
        let Some(argv) = route_command("add", route) else {
            return AddOutcome::Failed;
        };
        match self.runner.run(&argv) {
            Ok(output) if output.code == 0 => AddOutcome::Succeeded,
            Ok(output) if output.stderr.contains("File exists") => AddOutcome::Eexist,
            _ => AddOutcome::Failed,
        }
    }

    fn current(&self) -> Vec<Route> {
        self.read_table()
            .map(|table| table.routes)
            .unwrap_or_default()
    }

    fn delete(&mut self, route: &Route) -> bool {
        let Some(argv) = route_command("delete", route) else {
            return false;
        };
        matches!(self.runner.run(&argv), Ok(output) if output.code == 0)
    }
}

pub fn command_store() -> CommandRoutes<SystemRunner, fn(&str) -> Option<u32>> {
    CommandRoutes::new(SystemRunner, interface_index)
}

pub fn interface_index(name: &str) -> Option<u32> {
    let c_name = std::ffi::CString::new(name).ok()?;
    let index = unsafe { libc::if_nametoindex(c_name.as_ptr()) };
    if index == 0 {
        None
    } else {
        Some(index)
    }
}

/// `kill(pid, 0)` only checks for the process. It does not terminate it.
/// Pid 0 is treated as dead so we never signal the whole process group.
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Replace the record at `path`. Each line is `destination mask gateway ifindex`.
pub fn save_routes(path: &Path, routes: &[Route]) -> io::Result<()> {
    let mut text = String::new();
    for route in routes {
        text.push_str(&format!(
            "{} {} {} {}\n",
            route.destination, route.mask, route.gateway, route.interface_index
        ));
    }
    write_synced(path, &text)
}

pub fn load_routes(path: &Path) -> io::Result<Vec<Route>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    parse_record(&text)
}

pub fn save_pid(path: &Path, pid: u32) -> io::Result<()> {
    write_synced(path, &format!("{pid}\n"))
}

pub fn load_pid(path: &Path) -> io::Result<Option<u32>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let text = text.trim();
    if text.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty pid file"));
    }
    let pid = text
        .parse::<u32>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad pid file"))?;
    Ok(Some(pid))
}

/// Write each planned route, flush it, then add it.
/// The first failure removes that line, rolls back what succeeded, and returns.
/// The pid file is written after the first success and before the next add.
pub fn install_half_routes(
    store: &mut impl RouteStore,
    session: &mut Session,
    routes_path: &Path,
    pid_path: &Path,
    gateway: Ipv4Addr,
    interface_index: u32,
    pid: u32,
) -> Result<(), ProofError> {
    save_routes(routes_path, &[])?;
    let mut pid_written = false;
    for route in crate::netcfg::planned_routes(gateway, interface_index) {
        let outcome = remember_and_add(store, session, routes_path, route)?;
        if outcome == AddOutcome::Succeeded && !pid_written {
            if let Err(err) = save_pid(pid_path, pid) {
                rollback(store, session);
                return Err(ProofError::Io(err));
            }
            pid_written = true;
        }
        if outcome != AddOutcome::Succeeded {
            rollback(store, session);
            return Err(ProofError::NotAdded(outcome));
        }
    }
    Ok(())
}

/// Roll back recorded routes. Delete the pid and record files only when those
/// routes are gone from the table. A failed read counts as a failed rollback.
pub fn finish_proof<R, F>(
    store: &mut CommandRoutes<R, F>,
    session: &Session,
    pid_path: &Path,
    routes_path: &Path,
) -> Result<bool, ProofError>
where
    R: CommandRunner,
    F: Fn(&str) -> Option<u32>,
{
    // A failed table read must still delete the routes this session recorded.
    // `current()` hides that failure, so rollback would otherwise delete nothing.
    let visible = match store.read_table() {
        Ok(table) => session.cleanup(&table.routes),
        Err(_) => session.recorded().to_vec(),
    };
    for route in visible {
        store.delete(&route);
    }
    let table = store.read_table().map_err(|_| ProofError::Rollback)?;
    if !session.cleanup(&table.routes).is_empty() {
        return Err(ProofError::Rollback);
    }
    remove_if_present(routes_path)?;
    remove_if_present(pid_path)?;
    Ok(table.default_present)
}

fn remember_and_add(
    store: &mut impl RouteStore,
    session: &mut Session,
    routes_path: &Path,
    route: Route,
) -> Result<AddOutcome, ProofError> {
    let mut routes = load_routes(routes_path)?;
    routes.push(route);
    save_routes(routes_path, &routes)?;
    let outcome = store.add(&route);
    session.record_added(route, outcome);
    if outcome != AddOutcome::Succeeded {
        routes.pop();
        save_routes(routes_path, &routes)?;
    }
    Ok(outcome)
}

fn remove_if_present(path: &Path) -> Result<(), ProofError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ProofError::Rollback),
    }
}

fn write_synced(path: &Path, text: &str) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(text.as_bytes())?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

fn parse_record(text: &str) -> io::Result<Vec<Route>> {
    let mut routes = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let destination = record_ip(parts.next(), number)?;
        let mask = record_ip(parts.next(), number)?;
        let gateway = record_ip(parts.next(), number)?;
        let index = parts.next().ok_or_else(|| record_error(number))?;
        let interface_index = index.parse::<u32>().map_err(|_| record_error(number))?;
        if parts.next().is_some() {
            return Err(record_error(number));
        }
        routes.push(Route {
            destination,
            mask,
            gateway,
            interface_index,
        });
    }
    Ok(routes)
}

fn record_ip(text: Option<&str>, number: usize) -> io::Result<Ipv4Addr> {
    text.and_then(|text| text.parse().ok())
        .ok_or_else(|| record_error(number))
}

fn record_error(number: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("bad route record line {}", number + 1),
    )
}

fn list_command() -> Vec<String> {
    ["netstat", "-rn", "-f", "inet"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// `None` means this route must not become a command.
/// Only `0.0.0.0/1` and `128.0.0.0/1` are allowed, and the gateway is required.
fn route_command(action: &str, route: &Route) -> Option<Vec<String>> {
    let destination = half_destination(route)?;
    Some(vec![
        "route".to_string(),
        action.to_string(),
        "-net".to_string(),
        destination.to_string(),
        route.gateway.to_string(),
    ])
}

fn half_destination(route: &Route) -> Option<&'static str> {
    if route.mask != HALF_MASK {
        return None;
    }
    if route.destination == LOW_DEST {
        return Some("0.0.0.0/1");
    }
    if route.destination == HIGH_DEST {
        return Some("128.0.0.0/1");
    }
    None
}

fn default_present(text: &str) -> bool {
    text.lines()
        .any(|line| line.split_whitespace().next() == Some("default"))
}

fn parse_half_routes(text: &str, ifindex: impl Fn(&str) -> Option<u32>) -> Vec<Route> {
    let mut routes = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        let Some((destination, mask)) = parse_half_destination(fields[0]) else {
            continue;
        };
        let Ok(gateway) = fields[1].parse::<Ipv4Addr>() else {
            continue;
        };
        let Some(interface_index) = ifindex(fields[3]) else {
            continue;
        };
        routes.push(Route {
            destination,
            mask,
            gateway,
            interface_index,
        });
    }
    routes
}

fn parse_half_destination(field: &str) -> Option<(Ipv4Addr, Ipv4Addr)> {
    let (address, prefix) = field.split_once('/')?;
    if prefix != "1" {
        return None;
    }
    let destination = expand_ipv4(address)?;
    if destination != LOW_DEST && destination != HIGH_DEST {
        return None;
    }
    Some((destination, HALF_MASK))
}

fn expand_ipv4(address: &str) -> Option<Ipv4Addr> {
    let parts: Vec<&str> = address.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let mut octets = [0u8; 4];
    for (index, part) in parts.iter().enumerate() {
        octets[index] = part.parse().ok()?;
    }
    Some(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU64, Ordering};

    const FIXTURE: &str = "\
Routing tables

Internet:
Destination        Gateway            Flags           Netif Expire
default            192.168.1.1        UGScg             en0
0/1                10.210.24.153      UGSc            utun6
128.0/1            10.210.24.153      UGSc            utun6
10.210.24.4        10.210.24.153      UH              utun6
127                127.0.0.1          UCS               lo0
";

    const FIXTURE_DOTTED: &str = "\
Routing tables

Internet:
Destination        Gateway            Flags           Netif Expire
default            192.168.1.1        UGScg             en0
0.0.0.0/1          10.210.24.153      UGSc            utun6
128.0.0.0/1        10.210.24.153      UGSc            utun6
10.210.24.4        10.210.24.153      UH              utun6
127                127.0.0.1          UCS               lo0
";

    fn utun6(name: &str) -> Option<u32> {
        if name == "utun6" {
            Some(12)
        } else {
            None
        }
    }

    fn low() -> Route {
        Route {
            destination: Ipv4Addr::new(0, 0, 0, 0),
            mask: HALF_MASK,
            gateway: Ipv4Addr::new(10, 210, 24, 153),
            interface_index: 12,
        }
    }

    fn high() -> Route {
        Route {
            destination: Ipv4Addr::new(128, 0, 0, 0),
            mask: HALF_MASK,
            gateway: Ipv4Addr::new(10, 210, 24, 153),
            interface_index: 12,
        }
    }

    fn default_route() -> Route {
        Route {
            destination: Ipv4Addr::new(0, 0, 0, 0),
            mask: Ipv4Addr::new(0, 0, 0, 0),
            gateway: Ipv4Addr::new(10, 210, 24, 153),
            interface_index: 12,
        }
    }

    fn cmd(action: &str, destination: &str) -> Vec<String> {
        ["route", action, "-net", destination, "10.210.24.153"]
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    struct Reply {
        code: i32,
        stderr: String,
    }

    fn reply(code: i32, stderr: &str) -> Reply {
        Reply {
            code,
            stderr: stderr.to_string(),
        }
    }

    struct Fake {
        calls: RefCell<Vec<Vec<String>>>,
        add_replies: RefCell<VecDeque<Reply>>,
        delete_code: Cell<i32>,
        netstat_failures: Cell<i32>,
        present: RefCell<Vec<Route>>,
    }

    impl Fake {
        fn new(replies: Vec<Reply>) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                add_replies: RefCell::new(replies.into()),
                delete_code: Cell::new(0),
                netstat_failures: Cell::new(0),
                present: RefCell::new(Vec::new()),
            }
        }

        fn seed(&self, route: Route) {
            self.present.borrow_mut().push(route);
        }

        fn route_from_argv(&self, argv: &[String]) -> Option<Route> {
            let destination = match argv.get(3).map(String::as_str)? {
                "0.0.0.0/1" => LOW_DEST,
                "128.0.0.0/1" => HIGH_DEST,
                _ => return None,
            };
            let gateway = argv.get(4)?.parse().ok()?;
            Some(Route {
                destination,
                mask: HALF_MASK,
                gateway,
                interface_index: 12,
            })
        }

        fn render(&self) -> String {
            let mut text = String::from(
                "Routing tables\n\nInternet:\nDestination Gateway Flags Netif\ndefault 192.168.1.1 UGSc en0\n",
            );
            for route in self.present.borrow().iter() {
                let destination = if route.destination == LOW_DEST {
                    "0/1"
                } else if route.destination == HIGH_DEST {
                    "128.0/1"
                } else {
                    continue;
                };
                text.push_str(&format!("{destination} {} UGSc utun6\n", route.gateway));
            }
            text
        }
    }

    impl CommandRunner for Fake {
        fn run(&self, argv: &[String]) -> io::Result<CommandOutput> {
            self.calls.borrow_mut().push(argv.to_vec());
            if argv.first().map(String::as_str) == Some("netstat") {
                let failures = self.netstat_failures.get();
                if failures > 0 {
                    self.netstat_failures.set(failures - 1);
                    return Ok(CommandOutput {
                        code: 1,
                        stdout: String::new(),
                        stderr: "netstat failed".into(),
                    });
                }
                return Ok(CommandOutput {
                    code: 0,
                    stdout: self.render(),
                    stderr: String::new(),
                });
            }
            if argv.first().map(String::as_str) == Some("route") {
                let action = argv.get(1).map(String::as_str).unwrap_or("");
                if action == "add" {
                    let reply = self.add_replies.borrow_mut().pop_front().unwrap_or(Reply {
                        code: 1,
                        stderr: "no scripted add".into(),
                    });
                    if reply.code == 0 {
                        if let Some(route) = self.route_from_argv(argv) {
                            let mut present = self.present.borrow_mut();
                            if !present.contains(&route) {
                                present.push(route);
                            }
                        }
                    }
                    return Ok(CommandOutput {
                        code: reply.code,
                        stdout: String::new(),
                        stderr: reply.stderr,
                    });
                }
                if action == "delete" {
                    let code = self.delete_code.get();
                    if code == 0 {
                        if let Some(route) = self.route_from_argv(argv) {
                            self.present
                                .borrow_mut()
                                .retain(|existing| existing != &route);
                        }
                    }
                    return Ok(CommandOutput {
                        code,
                        stdout: String::new(),
                        stderr: String::new(),
                    });
                }
            }
            Ok(CommandOutput {
                code: 1,
                stdout: String::new(),
                stderr: "unexpected command".into(),
            })
        }
    }

    fn route_commands(fake: &Fake) -> Vec<Vec<String>> {
        fake.calls
            .borrow()
            .iter()
            .filter(|argv| argv.first().map(String::as_str) == Some("route"))
            .cloned()
            .collect()
    }

    fn assert_no_default(fake: &Fake) {
        for argv in fake.calls.borrow().iter() {
            assert!(argv.iter().all(|arg| arg != "default"));
        }
    }

    struct Scratch {
        dir: std::path::PathBuf,
    }

    impl Scratch {
        fn new() -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("tether-bsd-route-{}-{n}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            Self { dir }
        }

        fn routes(&self) -> std::path::PathBuf {
            self.dir.join("routes")
        }

        fn pid(&self) -> std::path::PathBuf {
            self.dir.join("pid")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn fixture_parse_returns_the_two_half_routes() {
        let routes = parse_half_routes(FIXTURE, utun6);
        assert_eq!(routes, vec![low(), high()]);
        assert!(default_present(FIXTURE));

        let dotted = parse_half_routes(FIXTURE_DOTTED, utun6);
        assert_eq!(dotted, vec![low(), high()]);

        let without_default = FIXTURE
            .lines()
            .filter(|line| line.split_whitespace().next() != Some("default"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!default_present(&without_default));
        assert_eq!(
            parse_half_routes(&without_default, utun6),
            vec![low(), high()]
        );
    }

    #[test]
    fn add_and_delete_argv_for_both_half_routes() {
        let mut routes = CommandRoutes::new(Fake::new(vec![reply(0, ""), reply(0, "")]), utun6);
        assert_eq!(routes.add(&low()), AddOutcome::Succeeded);
        assert_eq!(routes.add(&high()), AddOutcome::Succeeded);
        assert!(routes.delete(&low()));
        assert!(routes.delete(&high()));
        assert_eq!(
            route_commands(&routes.runner),
            vec![
                cmd("add", "0.0.0.0/1"),
                cmd("add", "128.0.0.0/1"),
                cmd("delete", "0.0.0.0/1"),
                cmd("delete", "128.0.0.0/1"),
            ]
        );
        assert_no_default(&routes.runner);

        let before = routes.runner.calls.borrow().len();
        let _ = routes.current();
        let list = routes.runner.calls.borrow()[before].clone();
        assert_eq!(
            list,
            vec![
                "netstat".to_string(),
                "-rn".to_string(),
                "-f".to_string(),
                "inet".to_string(),
            ]
        );
    }

    #[test]
    fn default_route_builds_no_argv() {
        let mut routes = CommandRoutes::new(Fake::new(vec![]), utun6);
        assert_eq!(routes.add(&default_route()), AddOutcome::Failed);
        assert!(!routes.delete(&default_route()));
        assert!(routes.runner.calls.borrow().is_empty());
    }

    #[test]
    fn file_exists_is_eexist_and_rollback_does_not_delete_it() {
        let scratch = Scratch::new();
        let fake = Fake::new(vec![reply(
            1,
            "route: writing to routing socket: File exists\n",
        )]);
        fake.seed(low());
        let mut store = CommandRoutes::new(fake, utun6);
        let mut session = Session::new();

        let err = install_half_routes(
            &mut store,
            &mut session,
            &scratch.routes(),
            &scratch.pid(),
            low().gateway,
            12,
            4242,
        )
        .unwrap_err();
        assert!(matches!(err, ProofError::NotAdded(AddOutcome::Eexist)));
        assert!(session.recorded().is_empty());
        assert_eq!(load_routes(&scratch.routes()).unwrap(), Vec::<Route>::new());
        assert!(load_pid(&scratch.pid()).unwrap().is_none());
        assert_eq!(route_commands(&store.runner), vec![cmd("add", "0.0.0.0/1")]);
        assert_eq!(store.current(), vec![low()]);

        assert!(finish_proof(&mut store, &session, &scratch.pid(), &scratch.routes()).unwrap());
        assert_eq!(route_commands(&store.runner), vec![cmd("add", "0.0.0.0/1")]);
        assert_eq!(store.current(), vec![low()]);
        assert_no_default(&store.runner);
    }

    #[test]
    fn failed_second_add_rolls_back_only_the_first() {
        let scratch = Scratch::new();
        let mut store = CommandRoutes::new(
            Fake::new(vec![reply(0, ""), reply(1, "network is unreachable")]),
            utun6,
        );
        let mut session = Session::new();

        let err = install_half_routes(
            &mut store,
            &mut session,
            &scratch.routes(),
            &scratch.pid(),
            low().gateway,
            12,
            4242,
        )
        .unwrap_err();
        assert!(matches!(err, ProofError::NotAdded(AddOutcome::Failed)));
        assert_eq!(session.recorded(), &[low()]);
        assert_eq!(load_routes(&scratch.routes()).unwrap(), vec![low()]);
        assert_eq!(load_pid(&scratch.pid()).unwrap(), Some(4242));
        assert_eq!(
            route_commands(&store.runner),
            vec![
                cmd("add", "0.0.0.0/1"),
                cmd("add", "128.0.0.0/1"),
                cmd("delete", "0.0.0.0/1"),
            ]
        );
        assert!(store.current().is_empty());

        assert!(finish_proof(&mut store, &session, &scratch.pid(), &scratch.routes()).unwrap());
        assert_eq!(
            route_commands(&store.runner),
            vec![
                cmd("add", "0.0.0.0/1"),
                cmd("add", "128.0.0.0/1"),
                cmd("delete", "0.0.0.0/1"),
            ]
        );
        assert!(!scratch.routes().exists());
        assert!(!scratch.pid().exists());
        assert_no_default(&store.runner);
    }

    #[test]
    fn finish_reports_a_route_that_is_still_present() {
        let scratch = Scratch::new();
        let fake = Fake::new(vec![]);
        fake.seed(low());
        fake.delete_code.set(1);
        let mut store = CommandRoutes::new(fake, utun6);
        let mut session = Session::new();
        session.record_added(low(), AddOutcome::Succeeded);
        save_routes(&scratch.routes(), &[low()]).unwrap();
        save_pid(&scratch.pid(), 4242).unwrap();

        let err =
            finish_proof(&mut store, &session, &scratch.pid(), &scratch.routes()).unwrap_err();
        assert!(matches!(err, ProofError::Rollback));
        assert_eq!(store.current(), vec![low()]);
        assert_eq!(
            route_commands(&store.runner),
            vec![cmd("delete", "0.0.0.0/1")]
        );
        assert_eq!(load_routes(&scratch.routes()).unwrap(), vec![low()]);
        assert_eq!(load_pid(&scratch.pid()).unwrap(), Some(4242));
        assert_no_default(&store.runner);
    }

    #[test]
    fn unreadable_table_still_deletes_recorded_routes() {
        let scratch = Scratch::new();
        let fake = Fake::new(vec![]);
        fake.seed(low());
        fake.netstat_failures.set(1);
        let mut store = CommandRoutes::new(fake, utun6);
        let mut session = Session::new();
        session.record_added(low(), AddOutcome::Succeeded);
        save_routes(&scratch.routes(), &[low()]).unwrap();
        save_pid(&scratch.pid(), 4242).unwrap();

        assert!(finish_proof(&mut store, &session, &scratch.pid(), &scratch.routes()).unwrap());
        assert_eq!(
            route_commands(&store.runner),
            vec![cmd("delete", "0.0.0.0/1")]
        );
        assert!(store.current().is_empty());
        assert!(!scratch.routes().exists());
        assert!(!scratch.pid().exists());
        assert_no_default(&store.runner);
    }

    #[test]
    fn both_adds_stay_recorded_until_finish() {
        let scratch = Scratch::new();
        let mut store = CommandRoutes::new(Fake::new(vec![reply(0, ""), reply(0, "")]), utun6);
        let mut session = Session::new();

        install_half_routes(
            &mut store,
            &mut session,
            &scratch.routes(),
            &scratch.pid(),
            low().gateway,
            12,
            4242,
        )
        .unwrap();
        assert_eq!(session.recorded(), &[low(), high()]);
        assert_eq!(load_routes(&scratch.routes()).unwrap(), vec![low(), high()]);
        assert_eq!(load_pid(&scratch.pid()).unwrap(), Some(4242));
        assert_eq!(
            route_commands(&store.runner),
            vec![cmd("add", "0.0.0.0/1"), cmd("add", "128.0.0.0/1")]
        );

        assert!(finish_proof(&mut store, &session, &scratch.pid(), &scratch.routes()).unwrap());
        assert_eq!(
            route_commands(&store.runner),
            vec![
                cmd("add", "0.0.0.0/1"),
                cmd("add", "128.0.0.0/1"),
                cmd("delete", "0.0.0.0/1"),
                cmd("delete", "128.0.0.0/1"),
            ]
        );
        assert!(store.current().is_empty());
        assert!(!scratch.routes().exists());
        assert!(!scratch.pid().exists());
        assert_no_default(&store.runner);
    }

    #[test]
    fn record_file_round_trip_uses_the_given_path() {
        let scratch = Scratch::new();
        let path = scratch.routes();
        let routes = vec![low(), high()];
        save_routes(&path, &routes).unwrap();
        assert_eq!(load_routes(&path).unwrap(), routes);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "0.0.0.0 128.0.0.0 10.210.24.153 12\n128.0.0.0 128.0.0.0 10.210.24.153 12\n"
        );
        assert!(path.starts_with(std::env::temp_dir()));
        assert_ne!(path, Path::new(ROUTE_FILE));
        assert_eq!(PID_FILE, "/tmp/tetherd-routeproof.pid");
        assert_eq!(ROUTE_FILE, "/tmp/tetherd-routeproof.routes");
    }
}
