//! One Ethernet frame per RNDIS data message, and utun rx accounting.
//!
//! A frame that does not fit in the phone's max transfer size is dropped.
//! It is not shortened. `cap_out_packet` still cuts; the bridge does not use it.

use crate::rndis::{self, DATA_HEADER_LEN};

const MIN_BOOTP_LEN: usize = 258;
const ETH_HEADER_LEN: usize = 14;
const IPV4_HEADER_LEN: usize = 20;
const UDP_HEADER_LEN: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutDecision {
    Send(Vec<u8>),
    Drop,
}

/// Wrap `frame` when the RNDIS message fits in `max_transfer_size`.
/// The header counts toward that size. A frame that does not fit is a drop.
pub fn decide_out(frame: &[u8], max_transfer_size: u32) -> OutDecision {
    let Some(packet_len) = DATA_HEADER_LEN.checked_add(frame.len()) else {
        return OutDecision::Drop;
    };
    if packet_len > max_transfer_size as usize {
        return OutDecision::Drop;
    }
    OutDecision::Send(rndis::build_data(frame))
}

/// Bytes a utun write accepted. Zero is not an rx packet.
pub fn note_rx(rx: &mut u64, written: usize) {
    if written > 0 {
        *rx += 1;
    }
}

/// Ethernet broadcast of a BOOTP payload from `build_discover` or `build_request`.
/// UDP is client port 68 to server port 67. The IPv4 checksum is filled in.
pub fn wrap_dhcp(mac: [u8; 6], bootp: &[u8]) -> Vec<u8> {
    let mut payload = bootp.to_vec();
    if payload.len() < MIN_BOOTP_LEN {
        payload.resize(MIN_BOOTP_LEN, 0);
    }
    let udp_len = UDP_HEADER_LEN + payload.len();
    let ip_len = IPV4_HEADER_LEN + udp_len;
    let mut frame = vec![0u8; ETH_HEADER_LEN + ip_len];
    frame[0..6].fill(0xff);
    frame[6..12].copy_from_slice(&mac);
    frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());

    let ip = ETH_HEADER_LEN;
    frame[ip] = 0x45;
    frame[ip + 2..ip + 4].copy_from_slice(&(ip_len as u16).to_be_bytes());
    frame[ip + 8] = 64;
    frame[ip + 9] = 17;
    frame[ip + 16..ip + 20].fill(0xff);
    let sum = internet_checksum(&frame[ip..ip + IPV4_HEADER_LEN]);
    frame[ip + 10..ip + 12].copy_from_slice(&sum.to_be_bytes());

    let udp = ip + IPV4_HEADER_LEN;
    frame[udp..udp + 2].copy_from_slice(&68u16.to_be_bytes());
    frame[udp + 2..udp + 4].copy_from_slice(&67u16.to_be_bytes());
    frame[udp + 4..udp + 6].copy_from_slice(&(udp_len as u16).to_be_bytes());
    frame[udp + UDP_HEADER_LEN..].copy_from_slice(&payload);
    frame
}

fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut index = 0;
    while index + 1 < data.len() {
        sum += u16::from_be_bytes([data[index], data[index + 1]]) as u32;
        index += 2;
    }
    if index < data.len() {
        sum += (data[index] as u32) << 8;
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dhcp;

    #[test]
    fn oversized_frame_is_a_drop_not_a_cut() {
        let max = 80u32;
        let room = max as usize - DATA_HEADER_LEN;
        let exact = vec![0x5au8; room];
        match decide_out(&exact, max) {
            OutDecision::Send(packet) => {
                assert_eq!(packet.len(), max as usize);
                assert_eq!(&packet[DATA_HEADER_LEN..], exact.as_slice());
            }
            OutDecision::Drop => panic!("frame at the cap was dropped"),
        }

        let over = vec![0x5au8; room + 1];
        assert_eq!(decide_out(&over, max), OutDecision::Drop);
        let cut = rndis::cap_out_packet(&over, max).unwrap();
        assert_eq!(cut.len(), max as usize);
        assert_ne!(&cut[DATA_HEADER_LEN..], over.as_slice());

        assert_eq!(decide_out(&[1, 2, 3, 4], 8), OutDecision::Drop);
    }

    #[test]
    fn utun_write_of_zero_bytes_does_not_increment_rx() {
        let mut rx = 0u64;
        note_rx(&mut rx, 0);
        assert_eq!(rx, 0);
        note_rx(&mut rx, 64);
        assert_eq!(rx, 1);
        note_rx(&mut rx, 0);
        assert_eq!(rx, 1);
    }

    #[test]
    fn dhcp_discover_is_an_ethernet_broadcast() {
        let mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x11];
        let xid = 0x1111_2222u32;
        let bootp = dhcp::build_discover(mac, xid);
        let frame = wrap_dhcp(mac, &bootp);

        assert_eq!(&frame[0..6], &[0xff; 6]);
        assert_eq!(&frame[6..12], &mac);
        assert_eq!(&frame[12..14], &[0x08, 0x00]);
        assert_eq!(frame[14] >> 4, 4);
        assert_eq!(frame[23], 17);
        assert_eq!(&frame[34..36], &68u16.to_be_bytes());
        assert_eq!(&frame[36..38], &67u16.to_be_bytes());
        assert_eq!(&frame[46..50], &xid.to_be_bytes());
        assert_eq!(&frame[42 + 236..42 + 240], &dhcp::MAGIC_COOKIE);

        let mut sum = 0u32;
        for word in frame[14..34].chunks_exact(2) {
            sum += u16::from_be_bytes([word[0], word[1]]) as u32;
        }
        while sum >> 16 != 0 {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        assert_eq!(sum, 0xffff);
    }
}
