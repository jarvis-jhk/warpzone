//! Peer-to-peer transport over one UDP socket.
//!
//! * **Candidates.** Our host addresses (the local IPv4/IPv6 address the OS
//!   would use towards the server) and server-reflexive addresses from STUN.
//!   The server passes them between members of a room.
//! * **Punching.** Both sides send `HELLO` to every candidate of the other at
//!   the same time; the first packet that gets through opens the path. IPv6
//!   candidates come first.
//! * **Relay.** Each peer pair has a relay channel on the server. It is bound
//!   at once and used when no direct path works after `relay_after`; direct
//!   probing continues and upgrades the path when it succeeds.
//! * **Handshake.** `HELLO` carries the sender's server-signed ticket, an
//!   ephemeral X25519 key and an Ed25519 signature by the ticket's device key.
//!   Peers accept only tickets for the right room, account and key, so the
//!   relay and anyone else on the path see only ciphertext.
//! * **Packets.** ChaCha20-Poly1305 with per-direction keys from HKDF, a 64-bit
//!   counter nonce and a replay window. Inside: unreliable datagrams, a
//!   reliable ordered message stream with selective acks, and keepalives.
//!
//! Nothing here blocks or spawns threads: call [`Net::poll`] regularly
//! (every frame) and drain [`Net::next_event`].

use crate::identity::Identity;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use hkdf::Hkdf;
use sha2::Sha256;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};
use warpzone_protocol::api::{RelayGrant, Role};
use warpzone_protocol::ticket::{Ticket, parse_key};
use warpzone_protocol::{MAX_DATAGRAM, now, wire};

pub type PeerId = u32;

const HELLO: u8 = 0xF1;
const SEALED: u8 = 0xF2;
const K_DATA: u8 = 1;
const K_REL: u8 = 2;
const K_ACK: u8 = 3;
const K_PING: u8 = 4;
const K_PONG: u8 = 5;
const K_BYE: u8 = 6;
const WINDOW: usize = 256;
const KEEPALIVE: Duration = Duration::from_secs(2);
const TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_GIVE_UP: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq)]
pub enum NetEvent {
    /// First authenticated packet from the peer arrived.
    Connected {
        peer: PeerId,
        relayed: bool,
    },
    /// The path changed (relay → direct, usually).
    PathChanged {
        peer: PeerId,
        relayed: bool,
    },
    Data {
        peer: PeerId,
        channel: u8,
        data: Vec<u8>,
        reliable: bool,
    },
    /// No packet for 15 s (or never connected within 30 s).
    Lost {
        peer: PeerId,
    },
}

/// Everything needed to reach one peer.
#[derive(Debug, Clone)]
pub struct PeerSpec {
    pub room: String,
    pub account: i64,
    pub username: String,
    /// The peer's device key (hex).
    pub key: String,
    pub candidates: Vec<String>,
    pub relay: RelayGrant,
    /// My own ticket for this room and my role in it.
    pub my_ticket: String,
    pub my_role: Role,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Path {
    Direct(SocketAddr),
    Relay,
}

struct Crypto {
    send: ChaCha20Poly1305,
    recv: ChaCha20Poly1305,
    ctr: u64,
    highest: u64,
    seen: u64,
}

impl Crypto {
    fn new(shared: &[u8; 32], room: &str, role: Role) -> Crypto {
        let hk = Hkdf::<Sha256>::new(Some(room.as_bytes()), shared);
        let mut h2g = [0u8; 32];
        let mut g2h = [0u8; 32];
        hk.expand(b"warpzone-v1 host->guest", &mut h2g).expect("hkdf");
        hk.expand(b"warpzone-v1 guest->host", &mut g2h).expect("hkdf");
        let (s, r) = if role == Role::Host { (h2g, g2h) } else { (g2h, h2g) };
        Crypto { send: ChaCha20Poly1305::new(Key::from_slice(&s)), recv: ChaCha20Poly1305::new(Key::from_slice(&r)), ctr: 1, highest: 0, seen: 0 }
    }

    fn nonce(ctr: u64) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[4..].copy_from_slice(&ctr.to_be_bytes());
        n
    }

    fn seal(&mut self, plain: &[u8]) -> Vec<u8> {
        let ctr = self.ctr;
        self.ctr += 1;
        let ct = self.send.encrypt(Nonce::from_slice(&Self::nonce(ctr)), plain).expect("encrypt");
        let mut out = Vec::with_capacity(9 + ct.len());
        out.push(SEALED);
        out.extend_from_slice(&ctr.to_be_bytes());
        out.extend_from_slice(&ct);
        out
    }

    fn open(&mut self, pkt: &[u8]) -> Option<Vec<u8>> {
        if pkt.len() < 9 + 16 || pkt[0] != SEALED {
            return None;
        }
        let ctr = u64::from_be_bytes(pkt[1..9].try_into().ok()?);
        if ctr == 0 || (ctr <= self.highest && (self.highest - ctr >= 64 || self.seen & (1 << (self.highest - ctr)) != 0)) {
            return None;
        }
        let plain = self.recv.decrypt(Nonce::from_slice(&Self::nonce(ctr)), &pkt[9..]).ok()?;
        if ctr > self.highest {
            let shift = ctr - self.highest;
            self.seen = if shift >= 64 { 0 } else { self.seen << shift };
            self.seen |= 1;
            self.highest = ctr;
        } else {
            self.seen |= 1 << (self.highest - ctr);
        }
        Some(plain)
    }
}

#[derive(Default)]
struct Reliable {
    next: u32,
    unacked: BTreeMap<u32, (u8, Vec<u8>, Instant, u32)>,
    queue: VecDeque<(u8, Vec<u8>)>,
    expected: u32,
    buffer: BTreeMap<u32, (u8, Vec<u8>)>,
    ack_due: bool,
}

impl Reliable {
    fn new() -> Reliable {
        Reliable { next: 1, expected: 1, ..Default::default() }
    }
}

struct Peer {
    spec: PeerSpec,
    key: VerifyingKey,
    candidates: Vec<SocketAddr>,
    eph_secret: x25519_dalek::StaticSecret,
    hello: Vec<u8>,
    peer_eph: Option<[u8; 32]>,
    crypto: Option<Crypto>,
    path: Option<Path>,
    started: Instant,
    last_probe: Option<Instant>,
    last_recv: Instant,
    last_send: Instant,
    relay_bound: bool,
    last_bind: Option<Instant>,
    lost: bool,
    rel: Reliable,
    rtt: Duration,
    rtt_measured: bool,
    last_ping: Instant,
    hello_replied: HashMap<SocketAddr, Instant>,
}

pub struct Net {
    sock: UdpSocket,
    v6: bool,
    servers: Vec<SocketAddr>,
    server_key: VerifyingKey,
    identity: Arc<Identity>,
    stun_tx: HashMap<[u8; 12], SocketAddr>,
    reflexive: Vec<SocketAddr>,
    local: Vec<SocketAddr>,
    last_stun: Option<Instant>,
    peers: HashMap<PeerId, Peer>,
    next_id: PeerId,
    events: VecDeque<NetEvent>,
    /// Start using the relay after this long without a direct path.
    pub relay_after: Duration,
    /// Testing: never use direct paths (forces the relay).
    pub relay_only: bool,
}

fn bind_dual() -> std::io::Result<(UdpSocket, bool)> {
    use socket2::{Domain, Protocol, Socket, Type};
    if let Ok(s) = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))
        && s.set_only_v6(false).is_ok()
        && s.bind(&"[::]:0".parse::<SocketAddr>().unwrap().into()).is_ok()
    {
        s.set_nonblocking(true)?;
        return Ok((s.into(), true));
    }
    let s = UdpSocket::bind("0.0.0.0:0")?;
    s.set_nonblocking(true)?;
    Ok((s, false))
}

/// The local address the OS would use towards `to` (no packet is sent).
fn local_towards(to: SocketAddr) -> Option<IpAddr> {
    let bind = if to.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
    let s = UdpSocket::bind(bind).ok()?;
    s.connect(to).ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

impl Net {
    /// `udp`: the server's UDP endpoints (`ServerInfo::udp`), `server_key`: hex.
    pub fn new(identity: Arc<Identity>, udp: &[String], server_key: &str) -> std::io::Result<Net> {
        let (sock, v6) = bind_dual()?;
        let mut servers = Vec::new();
        for u in udp {
            if let Ok(addrs) = u.to_socket_addrs() {
                for a in addrs {
                    if (a.is_ipv4() || v6) && !servers.contains(&a) {
                        servers.push(a);
                    }
                }
            }
        }
        let server_key = parse_key(server_key).ok_or_else(|| std::io::Error::new(ErrorKind::InvalidInput, "bad server key"))?;
        let port = sock.local_addr()?.port();
        let mut local = Vec::new();
        for s in &servers {
            if let Some(ip) = local_towards(*s) {
                let a = SocketAddr::new(ip, port);
                if !local.contains(&a) {
                    local.push(a);
                }
            }
        }
        Ok(Net {
            sock,
            v6,
            servers,
            server_key,
            identity,
            stun_tx: HashMap::new(),
            reflexive: Vec::new(),
            local,
            last_stun: None,
            peers: HashMap::new(),
            next_id: 1,
            events: VecDeque::new(),
            relay_after: Duration::from_millis(1500),
            relay_only: false,
        })
    }

    pub fn local_port(&self) -> u16 {
        self.sock.local_addr().map(|a| a.port()).unwrap_or(0)
    }

    /// Our candidates, IPv6 first. Changes once STUN answers arrive.
    pub fn candidates(&self) -> Vec<String> {
        let mut v: Vec<SocketAddr> = self.reflexive.iter().chain(self.local.iter()).copied().collect();
        v.sort_by_key(|a| (!a.is_ipv6(), a.ip().is_loopback()));
        v.dedup();
        v.iter().map(|a| a.to_string()).collect()
    }

    /// Whether every server family has answered STUN.
    pub fn stun_done(&self) -> bool {
        !self.reflexive.is_empty() && (self.servers.iter().filter(|s| s.is_ipv6()).count() == 0 || self.reflexive.iter().any(|a| a.is_ipv6()))
    }

    fn to_sock(&self, a: SocketAddr) -> SocketAddr {
        match (a, self.v6) {
            (SocketAddr::V4(v4), true) => SocketAddr::new(IpAddr::V6(v4.ip().to_ipv6_mapped()), v4.port()),
            _ => a,
        }
    }

    fn send_to(&self, data: &[u8], to: SocketAddr) {
        let _ = self.sock.send_to(data, self.to_sock(to));
    }

    fn relay_server(&self) -> Option<SocketAddr> {
        self.servers.iter().find(|s| s.is_ipv4()).or(self.servers.first()).copied()
    }

    pub fn add_peer(&mut self, spec: PeerSpec) -> std::io::Result<PeerId> {
        let key = parse_key(&spec.key).ok_or_else(|| std::io::Error::new(ErrorKind::InvalidInput, "bad peer key"))?;
        // Replace an existing entry for the same account in the same room.
        if let Some(old) = self.peers.iter().find(|(_, p)| p.spec.account == spec.account && p.spec.room == spec.room).map(|(id, _)| *id) {
            self.peers.remove(&old);
        }
        let eph_secret = x25519_dalek::StaticSecret::from(rand::random::<[u8; 32]>());
        let eph_pub = x25519_dalek::PublicKey::from(&eph_secret).to_bytes();
        let mut signed = b"warpzone-hello-v1".to_vec();
        signed.extend_from_slice(&eph_pub);
        signed.extend_from_slice(spec.my_ticket.as_bytes());
        let sig = self.identity.sign(&signed);
        let t = spec.my_ticket.as_bytes();
        let mut hello = Vec::with_capacity(3 + t.len() + 96);
        hello.push(HELLO);
        hello.extend_from_slice(&(t.len() as u16).to_be_bytes());
        hello.extend_from_slice(t);
        hello.extend_from_slice(&eph_pub);
        hello.extend_from_slice(&sig);
        let id = self.next_id;
        self.next_id += 1;
        let now = Instant::now();
        let candidates = parse_candidates(&spec.candidates);
        self.peers.insert(
            id,
            Peer {
                spec,
                key,
                candidates,
                eph_secret,
                hello,
                peer_eph: None,
                crypto: None,
                path: None,
                started: now,
                last_probe: None,
                last_recv: now,
                last_send: now,
                relay_bound: false,
                last_bind: None,
                lost: false,
                rel: Reliable::new(),
                rtt: Duration::from_millis(100),
                rtt_measured: false,
                last_ping: now,
                hello_replied: HashMap::new(),
            },
        );
        Ok(id)
    }

    pub fn update_candidates(&mut self, peer: PeerId, candidates: &[String]) {
        if let Some(p) = self.peers.get_mut(&peer) {
            p.candidates = parse_candidates(candidates);
        }
    }

    /// Say goodbye and forget the peer.
    pub fn remove_peer(&mut self, peer: PeerId) {
        if let Some(mut p) = self.peers.remove(&peer)
            && let (Some(c), Some(path)) = (p.crypto.as_mut(), p.path)
        {
            let pkt = c.seal(&[K_BYE]);
            self.send_path(&p.spec.relay, path, &pkt);
        }
    }

    pub fn clear(&mut self) {
        let ids: Vec<PeerId> = self.peers.keys().copied().collect();
        for id in ids {
            self.remove_peer(id);
        }
    }

    pub fn peer_account(&self, peer: PeerId) -> Option<i64> {
        self.peers.get(&peer).map(|p| p.spec.account)
    }

    pub fn peer_by_account(&self, account: i64) -> Option<PeerId> {
        self.peers.iter().find(|(_, p)| p.spec.account == account).map(|(id, _)| *id)
    }

    pub fn peer_name(&self, peer: PeerId) -> Option<&str> {
        self.peers.get(&peer).map(|p| p.spec.username.as_str())
    }

    /// Connected peers (a path exists and the peer is alive).
    pub fn connected(&self) -> Vec<PeerId> {
        self.peers.iter().filter(|(_, p)| p.path.is_some() && !p.lost).map(|(id, _)| *id).collect()
    }

    pub fn is_relayed(&self, peer: PeerId) -> Option<bool> {
        self.peers.get(&peer).and_then(|p| p.path).map(|p| p == Path::Relay)
    }

    pub fn rtt(&self, peer: PeerId) -> Option<Duration> {
        self.peers.get(&peer).filter(|p| p.path.is_some()).map(|p| p.rtt)
    }

    pub fn next_event(&mut self) -> Option<NetEvent> {
        self.events.pop_front()
    }

    fn send_path(&self, relay: &RelayGrant, path: Path, pkt: &[u8]) {
        match path {
            Path::Direct(a) => self.send_to(pkt, a),
            Path::Relay => {
                if let Some(s) = self.relay_server() {
                    self.send_to(&wire::relay_data(relay.channel, pkt), s);
                }
            }
        }
    }

    fn send_plain(&mut self, peer: PeerId, plain: &[u8]) -> bool {
        let Some(p) = self.peers.get_mut(&peer) else { return false };
        let (Some(c), Some(path)) = (p.crypto.as_mut(), p.path) else { return false };
        let pkt = c.seal(plain);
        p.last_send = Instant::now();
        let relay = p.spec.relay;
        self.send_path(&relay, path, &pkt);
        true
    }

    /// Unreliable datagram (dropped if not connected). `data` ≤ 1100 bytes.
    pub fn send(&mut self, peer: PeerId, channel: u8, data: &[u8]) -> bool {
        if data.len() > MAX_DATAGRAM {
            return false;
        }
        let mut plain = Vec::with_capacity(2 + data.len());
        plain.push(K_DATA);
        plain.push(channel);
        plain.extend_from_slice(data);
        self.send_plain(peer, &plain)
    }

    /// Reliable, ordered message; queued until connected. `data` ≤ 1100 bytes.
    pub fn send_reliable(&mut self, peer: PeerId, channel: u8, data: &[u8]) -> bool {
        if data.len() > MAX_DATAGRAM {
            return false;
        }
        let Some(p) = self.peers.get_mut(&peer) else { return false };
        p.rel.queue.push_back((channel, data.to_vec()));
        self.flush_reliable(peer);
        true
    }

    fn flush_reliable(&mut self, peer: PeerId) {
        let mut out = Vec::new();
        {
            let Some(p) = self.peers.get_mut(&peer) else { return };
            if p.crypto.is_none() || p.path.is_none() {
                return;
            }
            let now = Instant::now();
            let rto = (p.rtt * 2).max(Duration::from_millis(150));
            for (seq, (ch, data, at, tries)) in p.rel.unacked.iter_mut() {
                if now.duration_since(*at) >= rto {
                    *at = now;
                    *tries += 1;
                    out.push(rel_packet(*seq, *ch, data));
                }
            }
            while p.rel.unacked.len() < WINDOW
                && let Some((ch, data)) = p.rel.queue.pop_front()
            {
                let seq = p.rel.next;
                p.rel.next += 1;
                out.push(rel_packet(seq, ch, &data));
                p.rel.unacked.insert(seq, (ch, data, now, 0));
            }
        }
        for plain in out {
            self.send_plain(peer, &plain);
        }
    }

    fn handle_hello(&mut self, pkt: &[u8], from: Option<SocketAddr>, via_relay_peer: Option<PeerId>) {
        if pkt.len() < 3 {
            return;
        }
        let tl = u16::from_be_bytes([pkt[1], pkt[2]]) as usize;
        if pkt.len() != 3 + tl + 32 + 64 {
            return;
        }
        let Ok(ticket_s) = std::str::from_utf8(&pkt[3..3 + tl]) else { return };
        let eph: [u8; 32] = pkt[3 + tl..3 + tl + 32].try_into().unwrap();
        let sig: [u8; 64] = pkt[3 + tl + 32..].try_into().unwrap();
        let Ok(t) = Ticket::verify(ticket_s, &self.server_key, now()) else { return };
        let id = match via_relay_peer {
            Some(id) => id,
            None => match self.peers.iter().find(|(_, p)| p.spec.account == t.account && p.spec.room == t.room) {
                Some((id, _)) => *id,
                None => return,
            },
        };
        let Some(p) = self.peers.get_mut(&id) else { return };
        if t.account != p.spec.account || t.room != p.spec.room || t.key != p.spec.key {
            return;
        }
        let mut signed = b"warpzone-hello-v1".to_vec();
        signed.extend_from_slice(&eph);
        signed.extend_from_slice(ticket_s.as_bytes());
        if p.key.verify(&signed, &Signature::from_bytes(&sig)).is_err() {
            return;
        }
        if p.peer_eph != Some(eph) {
            // New handshake (first, or the peer restarted): new keys.
            let shared = p.eph_secret.diffie_hellman(&x25519_dalek::PublicKey::from(eph));
            p.crypto = Some(Crypto::new(shared.as_bytes(), &p.spec.room, p.spec.my_role));
            p.peer_eph = Some(eph);
            if p.path.is_some() {
                p.rel = Reliable::new();
            }
        }
        // Answer on the same path, then prove our keys with a sealed ping.
        let path = match from {
            Some(a) => Path::Direct(a),
            None => Path::Relay,
        };
        let key = from.unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], 0)));
        let reply = p.hello_replied.get(&key).is_none_or(|t| t.elapsed() > Duration::from_millis(100));
        if reply {
            p.hello_replied.insert(key, Instant::now());
        }
        let hello = p.hello.clone();
        let relay = p.spec.relay;
        let ping = {
            let mut v = vec![K_PING];
            v.extend_from_slice(&(now_ms()).to_be_bytes());
            p.crypto.as_mut().unwrap().seal(&v)
        };
        if reply {
            self.send_path(&relay, path, &hello);
        }
        self.send_path(&relay, path, &ping);
    }

    fn handle_sealed(&mut self, pkt: &[u8], from: Option<SocketAddr>, via_relay_peer: Option<PeerId>) {
        let candidates: Vec<PeerId> = match via_relay_peer {
            Some(id) => vec![id],
            None => {
                let a = from.unwrap();
                let mut v: Vec<PeerId> =
                    self.peers.iter().filter(|(_, p)| p.path == Some(Path::Direct(a)) || p.candidates.contains(&a)).map(|(id, _)| *id).collect();
                let rest: Vec<PeerId> = self.peers.keys().copied().filter(|id| !v.contains(id)).collect();
                v.extend(rest);
                v
            }
        };
        for id in candidates {
            let Some(p) = self.peers.get_mut(&id) else { continue };
            let Some(c) = p.crypto.as_mut() else { continue };
            let Some(plain) = c.open(pkt) else { continue };
            let this = match from {
                Some(a) => Path::Direct(a),
                None => Path::Relay,
            };
            if this != Path::Relay && self.relay_only {
                return;
            }
            p.last_recv = Instant::now();
            if p.lost {
                p.lost = false;
            }
            match p.path {
                None => {
                    p.path = Some(this);
                    self.events.push_back(NetEvent::Connected { peer: id, relayed: this == Path::Relay });
                }
                Some(Path::Relay) if this != Path::Relay => {
                    p.path = Some(this);
                    self.events.push_back(NetEvent::PathChanged { peer: id, relayed: false });
                }
                Some(Path::Direct(old)) if this != Path::Relay && this != Path::Direct(old) => {
                    p.path = Some(this);
                }
                _ => {}
            }
            self.handle_plain(id, &plain);
            self.flush_reliable(id);
            return;
        }
    }

    fn handle_plain(&mut self, id: PeerId, plain: &[u8]) {
        let Some(&kind) = plain.first() else { return };
        match kind {
            K_DATA if plain.len() >= 2 => {
                self.events.push_back(NetEvent::Data { peer: id, channel: plain[1], data: plain[2..].to_vec(), reliable: false });
            }
            K_REL if plain.len() >= 6 => {
                let seq = u32::from_be_bytes(plain[1..5].try_into().unwrap());
                let ch = plain[5];
                let data = plain[6..].to_vec();
                let mut deliver = Vec::new();
                if let Some(p) = self.peers.get_mut(&id) {
                    p.rel.ack_due = true;
                    if seq == p.rel.expected {
                        deliver.push((ch, data));
                        p.rel.expected += 1;
                        while let Some(x) = p.rel.buffer.remove(&p.rel.expected) {
                            deliver.push(x);
                            p.rel.expected += 1;
                        }
                    } else if seq > p.rel.expected && seq < p.rel.expected + 4096 {
                        p.rel.buffer.insert(seq, (ch, data));
                    }
                }
                for (ch, data) in deliver {
                    self.events.push_back(NetEvent::Data { peer: id, channel: ch, data, reliable: true });
                }
            }
            K_ACK if plain.len() >= 9 => {
                let cum = u32::from_be_bytes(plain[1..5].try_into().unwrap());
                let mask = u32::from_be_bytes(plain[5..9].try_into().unwrap());
                if let Some(p) = self.peers.get_mut(&id) {
                    p.rel.unacked.retain(|s, _| *s > cum);
                    for i in 0..32 {
                        if mask & (1 << i) != 0 {
                            p.rel.unacked.remove(&(cum + 2 + i));
                        }
                    }
                }
            }
            K_PING if plain.len() >= 9 => {
                let mut v = vec![K_PONG];
                v.extend_from_slice(&plain[1..9]);
                self.send_plain(id, &v);
            }
            K_PONG if plain.len() >= 9 => {
                let t = u64::from_be_bytes(plain[1..9].try_into().unwrap());
                let rtt = now_ms().saturating_sub(t);
                if let Some(p) = self.peers.get_mut(&id) {
                    let sample = Duration::from_millis(rtt.min(5000));
                    p.rtt = if p.rtt_measured { (p.rtt * 7 + sample) / 8 } else { sample };
                    p.rtt_measured = true;
                }
            }
            K_BYE => {
                if let Some(p) = self.peers.get_mut(&id)
                    && !p.lost
                {
                    p.lost = true;
                    self.events.push_back(NetEvent::Lost { peer: id });
                }
            }
            _ => {}
        }
    }

    /// Receive everything pending, then run timers. Call every frame.
    pub fn poll(&mut self) {
        let mut buf = [0u8; 2048];
        loop {
            let (n, from) = match self.sock.recv_from(&mut buf) {
                Ok(x) => x,
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::ConnectionReset => continue,
                Err(_) => break,
            };
            let from = wire::canonical(from);
            let pkt = &buf[..n];
            if self.servers.iter().any(|s| wire::canonical(*s) == from) {
                if let Some((ch, inner)) = wire::parse_relay_data(pkt) {
                    let Some(id) = self.peers.iter().find(|(_, p)| p.spec.relay.channel == ch).map(|(id, _)| *id) else { continue };
                    let inner = inner.to_vec();
                    match inner.first() {
                        Some(&HELLO) => self.handle_hello(&inner, None, Some(id)),
                        Some(&SEALED) => self.handle_sealed(&inner, None, Some(id)),
                        _ => {}
                    }
                } else if let Some(ch) = wire::parse_relay_bound(pkt) {
                    for p in self.peers.values_mut().filter(|p| p.spec.relay.channel == ch) {
                        p.relay_bound = true;
                    }
                } else if pkt.len() >= 20 {
                    let tx: [u8; 12] = pkt[8..20].try_into().unwrap();
                    if self.stun_tx.contains_key(&tx)
                        && let Some(addr) = wire::parse_stun_response(pkt, &tx)
                    {
                        self.stun_tx.remove(&tx);
                        if !self.reflexive.contains(&addr) {
                            self.reflexive.push(addr);
                        }
                    }
                }
                continue;
            }
            match pkt.first() {
                Some(&HELLO) if !self.relay_only => self.handle_hello(pkt, Some(from), None),
                Some(&SEALED) => self.handle_sealed(pkt, Some(from), None),
                _ => {}
            }
        }
        self.timers();
    }

    fn timers(&mut self) {
        let now = Instant::now();
        if self.last_stun.is_none_or(|t| now.duration_since(t) > Duration::from_secs(if self.stun_done() { 25 } else { 1 })) {
            self.last_stun = Some(now);
            self.stun_tx.clear();
            for s in self.servers.clone() {
                let tx: [u8; 12] = rand::random();
                self.stun_tx.insert(tx, s);
                self.send_to(&wire::stun_request(tx, wire::STUN_RESPONSE_MAX), s);
            }
        }
        let relay_server = self.relay_server();
        let ids: Vec<PeerId> = self.peers.keys().copied().collect();
        for id in ids {
            let mut sends: Vec<(Vec<u8>, SocketAddr)> = Vec::new();
            let mut relayed: Vec<Vec<u8>> = Vec::new();
            let mut ping = false;
            let mut ack = None;
            {
                let p = self.peers.get_mut(&id).unwrap();
                if p.lost {
                    continue;
                }
                // Keep the relay binding (and our NAT mapping to the server) fresh.
                let bind_every = if p.relay_bound { Duration::from_secs(20) } else { Duration::from_millis(700) };
                if p.last_bind.is_none_or(|t| now.duration_since(t) > bind_every)
                    && let Some(s) = relay_server
                {
                    p.last_bind = Some(now);
                    sends.push((wire::relay_bind(p.spec.relay.channel, p.spec.relay.side, &p.spec.relay.token), s));
                }
                let probing = p.path.is_none() || p.path == Some(Path::Relay);
                let age = now.duration_since(p.started);
                let every = if age < Duration::from_secs(10) { Duration::from_millis(200) } else { Duration::from_secs(1) };
                if probing && age < Duration::from_secs(60) && p.last_probe.is_none_or(|t| now.duration_since(t) >= every) {
                    p.last_probe = Some(now);
                    if !self.relay_only {
                        for c in &p.candidates {
                            sends.push((p.hello.clone(), *c));
                        }
                    }
                    if p.path.is_none() && (age >= self.relay_after || self.relay_only) && p.relay_bound {
                        relayed.push(p.hello.clone());
                    }
                }
                // Keepalive when idle, and an RTT probe every second regardless.
                if p.crypto.is_some() && p.path.is_some() && (now.duration_since(p.last_send) > KEEPALIVE || now.duration_since(p.last_ping) > Duration::from_secs(1)) {
                    p.last_ping = now;
                    ping = true;
                }
                if p.rel.ack_due {
                    p.rel.ack_due = false;
                    let cum = p.rel.expected - 1;
                    let mut mask = 0u32;
                    for i in 0..32 {
                        if p.rel.buffer.contains_key(&(p.rel.expected + 1 + i)) {
                            mask |= 1 << i;
                        }
                    }
                    let mut v = vec![K_ACK];
                    v.extend_from_slice(&cum.to_be_bytes());
                    v.extend_from_slice(&mask.to_be_bytes());
                    ack = Some(v);
                }
                let silent = now.duration_since(p.last_recv);
                if (p.path.is_some() && silent > TIMEOUT) || (p.path.is_none() && age > CONNECT_GIVE_UP) {
                    p.lost = true;
                    self.events.push_back(NetEvent::Lost { peer: id });
                    continue;
                }
            }
            for (pkt, to) in sends {
                self.send_to(&pkt, to);
            }
            if let Some(s) = relay_server {
                let ch = self.peers[&id].spec.relay.channel;
                for pkt in relayed {
                    self.send_to(&wire::relay_data(ch, &pkt), s);
                }
            }
            if ping {
                let mut v = vec![K_PING];
                v.extend_from_slice(&now_ms().to_be_bytes());
                self.send_plain(id, &v);
            }
            if let Some(a) = ack {
                self.send_plain(id, &a);
            }
            self.flush_reliable(id);
        }
    }
}

fn rel_packet(seq: u32, ch: u8, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(6 + data.len());
    v.push(K_REL);
    v.extend_from_slice(&seq.to_be_bytes());
    v.push(ch);
    v.extend_from_slice(data);
    v
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn parse_candidates(c: &[String]) -> Vec<SocketAddr> {
    let mut v: Vec<SocketAddr> = c.iter().filter_map(|s| s.parse().ok()).map(wire::canonical).collect();
    v.sort_by_key(|a| !a.is_ipv6());
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_roundtrip_and_replay() {
        let shared = [5u8; 32];
        let mut h = Crypto::new(&shared, "room", Role::Host);
        let mut g = Crypto::new(&shared, "room", Role::Guest);
        let a = h.seal(b"one");
        let b = h.seal(b"two");
        assert_eq!(g.open(&b).unwrap(), b"two");
        assert_eq!(g.open(&a).unwrap(), b"one", "reordered within the window is fine");
        assert!(g.open(&a).is_none(), "replay is refused");
        let back = g.seal(b"x");
        assert_eq!(h.open(&back).unwrap(), b"x");
        let mut other = Crypto::new(&shared, "other-room", Role::Guest);
        assert!(other.open(&h.seal(b"y")).is_none(), "keys are bound to the room");
    }
}
