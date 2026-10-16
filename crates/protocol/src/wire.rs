//! UDP datagram formats on the server's UDP port.
//!
//! One socket serves three things, told apart by their first bytes:
//!
//! | Datagram | Layout | Answer |
//! |---|---|---|
//! | STUN Binding request (RFC 8489 subset) | 20-byte header + attributes | Binding success with XOR-MAPPED-ADDRESS, **only if the request is at least as long as the answer** |
//! | Relay bind | `"WZB1" channel:u32be side:u8 token:[16]` | `"WZb1" channel:u32be` |
//! | Relay data | `"WZD1" channel:u32be payload` | forwarded unchanged to the other side |
//!
//! The server never sends more bytes than it received, so the port cannot be
//! used to amplify a spoofed-source attack.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub const STUN_MAGIC: u32 = 0x2112_A442;
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;
const ATTR_SOFTWARE: u16 = 0x8022;

pub const RELAY_BIND: &[u8; 4] = b"WZB1";
pub const RELAY_BOUND: &[u8; 4] = b"WZb1";
pub const RELAY_DATA: &[u8; 4] = b"WZD1";

/// Bytes of a relay data header.
pub const RELAY_HEADER: usize = 8;

/// Length of the largest binding response we send (IPv6 mapped address).
pub const STUN_RESPONSE_MAX: usize = 20 + 4 + 20;

/// A STUN Binding request padded (with a SOFTWARE attribute) to `min_len`
/// bytes, so that the answer is never larger than the question.
pub fn stun_request(txid: [u8; 12], min_len: usize) -> Vec<u8> {
    let mut software = b"warpzone".to_vec();
    while 20 + 4 + software.len() < min_len {
        software.push(b' ');
    }
    while !software.len().is_multiple_of(4) {
        software.push(b' ');
    }
    let mut out = Vec::with_capacity(24 + software.len());
    out.extend_from_slice(&BINDING_REQUEST.to_be_bytes());
    out.extend_from_slice(&((4 + software.len()) as u16).to_be_bytes());
    out.extend_from_slice(&STUN_MAGIC.to_be_bytes());
    out.extend_from_slice(&txid);
    out.extend_from_slice(&ATTR_SOFTWARE.to_be_bytes());
    out.extend_from_slice(&(software.len() as u16).to_be_bytes());
    out.extend_from_slice(&software);
    out
}

/// If `buf` is a STUN Binding request, its transaction id.
pub fn parse_stun_request(buf: &[u8]) -> Option<[u8; 12]> {
    if buf.len() < 20 || buf[0] & 0xC0 != 0 {
        return None;
    }
    let ty = u16::from_be_bytes([buf[0], buf[1]]);
    let len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    let magic = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    if ty != BINDING_REQUEST || magic != STUN_MAGIC || len + 20 != buf.len() || !len.is_multiple_of(4) {
        return None;
    }
    buf[8..20].try_into().ok()
}

/// Binding success response carrying `addr` as XOR-MAPPED-ADDRESS.
pub fn stun_response(txid: [u8; 12], addr: SocketAddr) -> Vec<u8> {
    let addr = canonical(addr);
    let mut attr = Vec::with_capacity(20);
    attr.push(0);
    let port = addr.port() ^ (STUN_MAGIC >> 16) as u16;
    match addr.ip() {
        IpAddr::V4(ip) => {
            attr.push(0x01);
            attr.extend_from_slice(&port.to_be_bytes());
            let x = u32::from(ip) ^ STUN_MAGIC;
            attr.extend_from_slice(&x.to_be_bytes());
        }
        IpAddr::V6(ip) => {
            attr.push(0x02);
            attr.extend_from_slice(&port.to_be_bytes());
            let mut key = [0u8; 16];
            key[..4].copy_from_slice(&STUN_MAGIC.to_be_bytes());
            key[4..].copy_from_slice(&txid);
            let o = ip.octets();
            for i in 0..16 {
                attr.push(o[i] ^ key[i]);
            }
        }
    }
    let mut out = Vec::with_capacity(24 + attr.len());
    out.extend_from_slice(&BINDING_SUCCESS.to_be_bytes());
    out.extend_from_slice(&((4 + attr.len()) as u16).to_be_bytes());
    out.extend_from_slice(&STUN_MAGIC.to_be_bytes());
    out.extend_from_slice(&txid);
    out.extend_from_slice(&ATTR_XOR_MAPPED_ADDRESS.to_be_bytes());
    out.extend_from_slice(&(attr.len() as u16).to_be_bytes());
    out.extend_from_slice(&attr);
    out
}

/// If `buf` answers the request `txid`, the mapped (reflexive) address.
pub fn parse_stun_response(buf: &[u8], txid: &[u8; 12]) -> Option<SocketAddr> {
    if buf.len() < 20 {
        return None;
    }
    let ty = u16::from_be_bytes([buf[0], buf[1]]);
    let magic = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    if ty != BINDING_SUCCESS || magic != STUN_MAGIC || &buf[8..20] != txid {
        return None;
    }
    let mut i = 20;
    while i + 4 <= buf.len() {
        let at = u16::from_be_bytes([buf[i], buf[i + 1]]);
        let al = u16::from_be_bytes([buf[i + 2], buf[i + 3]]) as usize;
        let v = buf.get(i + 4..i + 4 + al)?;
        if at == ATTR_XOR_MAPPED_ADDRESS && al >= 8 {
            let port = u16::from_be_bytes([v[2], v[3]]) ^ (STUN_MAGIC >> 16) as u16;
            return match v[1] {
                0x01 => {
                    let x = u32::from_be_bytes([v[4], v[5], v[6], v[7]]) ^ STUN_MAGIC;
                    Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(x)), port))
                }
                0x02 if al >= 20 => {
                    let mut key = [0u8; 16];
                    key[..4].copy_from_slice(&STUN_MAGIC.to_be_bytes());
                    key[4..].copy_from_slice(txid);
                    let mut o = [0u8; 16];
                    for k in 0..16 {
                        o[k] = v[4 + k] ^ key[k];
                    }
                    Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(o)), port))
                }
                _ => None,
            };
        }
        i += 4 + al.div_ceil(4) * 4;
    }
    None
}

/// `WZB1 channel side token`
pub fn relay_bind(channel: u32, side: u8, token: &[u8; 16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(25);
    out.extend_from_slice(RELAY_BIND);
    out.extend_from_slice(&channel.to_be_bytes());
    out.push(side);
    out.extend_from_slice(token);
    out
}

pub fn parse_relay_bind(buf: &[u8]) -> Option<(u32, u8, [u8; 16])> {
    if buf.len() != 25 || &buf[..4] != RELAY_BIND {
        return None;
    }
    let ch = u32::from_be_bytes(buf[4..8].try_into().ok()?);
    Some((ch, buf[8], buf[9..25].try_into().ok()?))
}

pub fn relay_bound(channel: u32) -> Vec<u8> {
    let mut out = RELAY_BOUND.to_vec();
    out.extend_from_slice(&channel.to_be_bytes());
    out
}

pub fn parse_relay_bound(buf: &[u8]) -> Option<u32> {
    if buf.len() != 8 || &buf[..4] != RELAY_BOUND {
        return None;
    }
    Some(u32::from_be_bytes(buf[4..8].try_into().ok()?))
}

/// `WZD1 channel payload`
pub fn relay_data(channel: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(RELAY_HEADER + payload.len());
    out.extend_from_slice(RELAY_DATA);
    out.extend_from_slice(&channel.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn parse_relay_data(buf: &[u8]) -> Option<(u32, &[u8])> {
    if buf.len() < RELAY_HEADER || &buf[..4] != RELAY_DATA {
        return None;
    }
    Some((u32::from_be_bytes(buf[4..8].try_into().ok()?), &buf[8..]))
}

/// IPv4-mapped IPv6 addresses (`::ffff:1.2.3.4`, what a dual-stack socket
/// reports for IPv4 peers) as plain IPv4.
pub fn canonical(addr: SocketAddr) -> SocketAddr {
    match addr.ip() {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => SocketAddr::new(IpAddr::V4(v4), addr.port()),
            None => addr,
        },
        IpAddr::V4(_) => addr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stun_v4_and_v6() {
        let tx = [9u8; 12];
        let req = stun_request(tx, STUN_RESPONSE_MAX);
        assert!(req.len() >= STUN_RESPONSE_MAX);
        assert_eq!(parse_stun_request(&req), Some(tx));
        for a in ["203.0.113.7:40000", "[2001:db8::1]:3478", "[::ffff:198.51.100.2]:5"] {
            let addr: SocketAddr = a.parse().unwrap();
            let resp = stun_response(tx, addr);
            assert!(resp.len() <= STUN_RESPONSE_MAX);
            assert_eq!(parse_stun_response(&resp, &tx), Some(canonical(addr)));
        }
    }

    #[test]
    fn relay_frames() {
        let b = relay_bind(5, 1, &[3; 16]);
        assert_eq!(parse_relay_bind(&b), Some((5, 1, [3; 16])));
        assert_eq!(parse_relay_bound(&relay_bound(5)), Some(5));
        let d = relay_data(5, b"hi");
        assert_eq!(parse_relay_data(&d), Some((5, &b"hi"[..])));
        assert_eq!(parse_stun_request(&d), None);
    }
}
