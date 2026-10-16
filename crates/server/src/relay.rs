//! The UDP port: STUN reflection and the relay.
//!
//! Relay channels are created when the server issues a join ticket: one per
//! (room, guest), with a random 16-byte token for each side. A side binds by
//! sending `WZB1 channel side token` from the socket it will use; afterwards
//! `WZD1 channel payload` datagrams from that address are forwarded to the
//! other side's bound address. Nothing else is ever forwarded, nothing larger
//! than the request is ever sent (no amplification), and every channel and the
//! whole server have bandwidth caps.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use warpzone_protocol::wire;

/// Token bucket in bytes.
struct Bucket {
    rate: f64,
    burst: f64,
    level: f64,
    at: Instant,
}

impl Bucket {
    fn new(bytes_per_sec: f64) -> Bucket {
        let burst = (bytes_per_sec * 0.5).max(64_000.0);
        Bucket { rate: bytes_per_sec, burst, level: burst, at: Instant::now() }
    }

    fn take(&mut self, n: usize) -> bool {
        let now = Instant::now();
        self.level = (self.level + now.duration_since(self.at).as_secs_f64() * self.rate).min(self.burst);
        self.at = now;
        if self.level >= n as f64 {
            self.level -= n as f64;
            true
        } else {
            false
        }
    }
}

struct Channel {
    room: String,
    tokens: [[u8; 16]; 2],
    addrs: [Option<SocketAddr>; 2],
    buckets: [Bucket; 2],
    last: Instant,
}

pub struct Relay {
    channels: Mutex<HashMap<u32, Channel>>,
    total: Mutex<Bucket>,
    per_channel: f64,
    pub stats: Mutex<RelayStats>,
}

#[derive(Default, Clone, Debug)]
pub struct RelayStats {
    pub forwarded_bytes: u64,
    pub dropped: u64,
    pub stun: u64,
}

impl Relay {
    pub fn new(kbps: u64, total_mbps: u64) -> Relay {
        Relay {
            channels: Mutex::new(HashMap::new()),
            total: Mutex::new(Bucket::new(total_mbps as f64 * 125_000.0)),
            per_channel: kbps as f64 * 125.0,
            stats: Mutex::new(RelayStats::default()),
        }
    }

    /// New channel for `room`; returns its id and both sides' tokens.
    pub fn open(&self, room: &str) -> (u32, [[u8; 16]; 2]) {
        let mut map = self.channels.lock().unwrap();
        loop {
            let id: u32 = rand::random();
            if id == 0 || map.contains_key(&id) {
                continue;
            }
            let tokens: [[u8; 16]; 2] = [rand::random(), rand::random()];
            map.insert(
                id,
                Channel {
                    room: room.to_string(),
                    tokens,
                    addrs: [None, None],
                    buckets: [Bucket::new(self.per_channel), Bucket::new(self.per_channel)],
                    last: Instant::now(),
                },
            );
            return (id, tokens);
        }
    }

    pub fn close(&self, id: u32) {
        self.channels.lock().unwrap().remove(&id);
    }

    pub fn close_room(&self, room: &str) {
        self.channels.lock().unwrap().retain(|_, c| c.room != room);
    }

    pub fn channel_count(&self) -> usize {
        self.channels.lock().unwrap().len()
    }

    /// Handle one datagram; returns what to send where (if anything).
    pub fn handle(&self, buf: &[u8], from: SocketAddr) -> Option<(Vec<u8>, SocketAddr)> {
        let from_c = wire::canonical(from);
        if let Some(tx) = wire::parse_stun_request(buf) {
            let resp = wire::stun_response(tx, from_c);
            self.stats.lock().unwrap().stun += 1;
            // Never answer with more bytes than we were sent.
            return (resp.len() <= buf.len()).then_some((resp, from));
        }
        if let Some((ch, side, token)) = wire::parse_relay_bind(buf) {
            let mut map = self.channels.lock().unwrap();
            let c = map.get_mut(&ch)?;
            if side > 1 || c.tokens[side as usize] != token {
                return None;
            }
            c.addrs[side as usize] = Some(from);
            c.last = Instant::now();
            return Some((wire::relay_bound(ch), from));
        }
        if let Some((ch, _)) = wire::parse_relay_data(buf) {
            let mut map = self.channels.lock().unwrap();
            let c = map.get_mut(&ch)?;
            let side = if c.addrs[0] == Some(from) {
                0
            } else if c.addrs[1] == Some(from) {
                1
            } else {
                return None;
            };
            let to = c.addrs[1 - side]?;
            let ok = c.buckets[side].take(buf.len()) && self.total.lock().unwrap().take(buf.len());
            let mut stats = self.stats.lock().unwrap();
            if !ok {
                stats.dropped += 1;
                return None;
            }
            c.last = Instant::now();
            stats.forwarded_bytes += buf.len() as u64;
            return Some((buf.to_vec(), to));
        }
        None
    }

    /// Drop channels idle for longer than `idle`.
    pub fn sweep(&self, idle: Duration) {
        self.channels.lock().unwrap().retain(|_, c| c.last.elapsed() < idle);
    }
}

/// Bind the UDP socket: dual-stack when the address is `[::]`.
pub fn bind_udp(addr: &str) -> std::io::Result<std::net::UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let addr: SocketAddr = addr.parse().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let domain = if addr.is_ipv6() { Domain::IPV6 } else { Domain::IPV4 };
    let s = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;
    if addr.is_ipv6() {
        let _ = s.set_only_v6(false);
    }
    let _ = s.set_recv_buffer_size(1 << 20);
    let _ = s.set_send_buffer_size(1 << 20);
    s.bind(&addr.into())?;
    s.set_nonblocking(true)?;
    Ok(s.into())
}

/// Serve the UDP port until the process ends.
pub async fn run(relay: std::sync::Arc<Relay>, socket: std::net::UdpSocket, limiter: std::sync::Arc<crate::state::State>) {
    let sock = match tokio::net::UdpSocket::from_std(socket) {
        Ok(s) => s,
        Err(e) => {
            log::error!("udp: {e}");
            return;
        }
    };
    let mut buf = vec![0u8; 2048];
    loop {
        let (n, from) = match sock.recv_from(&mut buf).await {
            Ok(x) => x,
            Err(e) => {
                log::debug!("udp recv: {e}");
                continue;
            }
        };
        let pkt = &buf[..n];
        // STUN and binds are cheap but unauthenticated: cap them per source IP.
        if !pkt.starts_with(wire::RELAY_DATA) && !limiter.limiter.allow(&format!("udp:{}", wire::canonical(from).ip()), 50, Duration::from_secs(1)) {
            continue;
        }
        if let Some((out, to)) = relay.handle(pkt, from) {
            let _ = sock.send_to(&out, to).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_and_forward() {
        let r = Relay::new(1000, 10);
        let (ch, t) = r.open("room");
        let a: SocketAddr = "198.51.100.1:1000".parse().unwrap();
        let b: SocketAddr = "203.0.113.9:2000".parse().unwrap();
        assert!(r.handle(&wire::relay_bind(ch, 0, &[0; 16]), a).is_none(), "wrong token");
        assert_eq!(r.handle(&wire::relay_bind(ch, 0, &t[0]), a).unwrap().1, a);
        assert!(r.handle(&wire::relay_data(ch, b"x"), a).is_none(), "other side not bound yet");
        r.handle(&wire::relay_bind(ch, 1, &t[1]), b).unwrap();
        let (out, to) = r.handle(&wire::relay_data(ch, b"hello"), a).unwrap();
        assert_eq!(to, b);
        assert_eq!(out, wire::relay_data(ch, b"hello"));
        let stranger: SocketAddr = "192.0.2.5:9".parse().unwrap();
        assert!(r.handle(&wire::relay_data(ch, b"spoof"), stranger).is_none());
    }

    #[test]
    fn stun_never_amplifies() {
        let r = Relay::new(1000, 10);
        let from: SocketAddr = "[2001:db8::7]:4000".parse().unwrap();
        let tiny = wire::stun_request([1; 12], 0);
        assert!(r.handle(&tiny, from).is_none());
        let padded = wire::stun_request([1; 12], wire::STUN_RESPONSE_MAX);
        let (resp, to) = r.handle(&padded, from).unwrap();
        assert!(resp.len() <= padded.len());
        assert_eq!(to, from);
        assert_eq!(wire::parse_stun_response(&resp, &[1; 12]), Some(from));
    }
}
