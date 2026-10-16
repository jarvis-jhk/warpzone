//! Integration tier 0: games that know nothing about Warpzone.
//!
//! The launcher stages the lobby itself (who plays, which settings), then on
//! **Start** every machine writes the game's launch files from the
//! descriptor's templates and starts the game. The game talks plain UDP to
//! `127.0.0.1:<port>` for each other player; the [`Tunnel`] carries that
//! traffic over the Warpzone peer channel.
//!
//! Rooms are a star around the host, but games like OpenTS expect every
//! machine to reach every other one, so tunnel frames carry source and
//! destination accounts and the host forwards guest-to-guest frames.
//!
//! Reserved peer channels: [`LOBBY_CHANNEL`] (reliable JSON lobby messages)
//! and [`TUNNEL_CHANNEL`] (unreliable tunnel frames).

use crate::client::Client;
use crate::net::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::net::{SocketAddr, UdpSocket};

pub const TUNNEL_CHANNEL: u8 = 250;
pub const LOBBY_CHANNEL: u8 = 251;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LobbyPlayer {
    pub account: i64,
    pub name: String,
    pub seat: u32,
}

/// Messages on [`LOBBY_CHANNEL`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum LobbyMsg {
    /// Host → all: who is in, and the chosen settings.
    State { players: Vec<LobbyPlayer>, settings: BTreeMap<String, String> },
    /// Host → all: write the files and start the game now.
    Start { players: Vec<LobbyPlayer>, settings: BTreeMap<String, String>, seed: u32 },
    /// Guest → host: I am ready / not ready.
    Ready { ready: bool },
    /// Anyone: my game exited.
    Ended,
}

impl LobbyMsg {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
    pub fn decode(b: &[u8]) -> Option<LobbyMsg> {
        serde_json::from_slice(b).ok()
    }
}

/// Local UDP endpoints for every other player plus the routing between them.
pub struct Tunnel {
    pub me: i64,
    pub host: bool,
    pub game_port: u16,
    sockets: HashMap<i64, UdpSocket>,
    /// Who carries frames for an account: the host for guests; for the host,
    /// that account's own peer.
    route: HashMap<i64, PeerId>,
    pub frames_in: u64,
    pub frames_out: u64,
}

impl Tunnel {
    /// `others`: every other player's account. `route`: account → peer that
    /// reaches it.
    pub fn new(me: i64, host: bool, game_port: u16, others: &[i64], route: HashMap<i64, PeerId>) -> std::io::Result<Tunnel> {
        let mut sockets = HashMap::new();
        for a in others {
            let s = UdpSocket::bind("127.0.0.1:0")?;
            s.set_nonblocking(true)?;
            sockets.insert(*a, s);
        }
        Ok(Tunnel { me, host, game_port, sockets, route, frames_in: 0, frames_out: 0 })
    }

    /// The local port the game must use to reach `account`.
    pub fn port_for(&self, account: i64) -> Option<u16> {
        self.sockets.get(&account).and_then(|s| s.local_addr().ok()).map(|a| a.port())
    }

    fn frame(dst: i64, src: i64, payload: &[u8]) -> Vec<u8> {
        let mut f = Vec::with_capacity(16 + payload.len());
        f.extend_from_slice(&dst.to_be_bytes());
        f.extend_from_slice(&src.to_be_bytes());
        f.extend_from_slice(payload);
        f
    }

    /// Game → network. Call every frame after `Client::poll`.
    pub fn pump(&mut self, client: &mut Client) {
        let mut buf = [0u8; 1500];
        let mut out = Vec::new();
        for (account, s) in &self.sockets {
            while let Ok((n, from)) = s.recv_from(&mut buf) {
                if !from.ip().is_loopback() {
                    continue;
                }
                out.push((*account, Self::frame(*account, self.me, &buf[..n])));
            }
        }
        for (dst, f) in out {
            if let Some(peer) = self.route.get(&dst)
                && f.len() <= warpzone_protocol::MAX_DATAGRAM
            {
                client.send(*peer, TUNNEL_CHANNEL, &f, false);
                self.frames_out += 1;
            }
        }
    }

    /// Network → game (or onwards, on the host).
    pub fn deliver(&mut self, client: &mut Client, frame: &[u8]) {
        if frame.len() < 16 {
            return;
        }
        let dst = i64::from_be_bytes(frame[..8].try_into().unwrap());
        let src = i64::from_be_bytes(frame[8..16].try_into().unwrap());
        if dst == self.me {
            if let Some(s) = self.sockets.get(&src) {
                let to = SocketAddr::from(([127, 0, 0, 1], self.game_port));
                let _ = s.send_to(&frame[16..], to);
                self.frames_in += 1;
            }
        } else if self.host
            && let Some(peer) = self.route.get(&dst)
        {
            client.send(*peer, TUNNEL_CHANNEL, frame, false);
        }
    }
}

/// Template context for one machine, from the host's `Start` message.
pub fn start_context(
    me: i64,
    host_account: i64,
    players: &[LobbyPlayer],
    settings: &BTreeMap<String, String>,
    seed: u32,
    game_port: u16,
    tunnel: &Tunnel,
) -> crate::template::Context {
    let mut ctx = crate::template::Context::default();
    let mine = players.iter().find(|p| p.account == me);
    ctx.values.insert("self.name".into(), mine.map(|p| p.name.clone()).unwrap_or_default());
    ctx.values.insert("self.seat".into(), mine.map(|p| p.seat.to_string()).unwrap_or_default());
    ctx.values.insert("self.host".into(), (me == host_account).to_string());
    ctx.values.insert("self.host_yesno".into(), if me == host_account { "yes" } else { "no" }.into());
    ctx.values.insert("game_port".into(), game_port.to_string());
    ctx.values.insert("seed".into(), seed.to_string());
    ctx.values.insert("players".into(), players.len().to_string());
    for (k, v) in settings {
        ctx.values.insert(format!("setting.{k}"), v.clone());
    }
    for p in players.iter().filter(|p| p.account != me) {
        let mut m = BTreeMap::new();
        m.insert("name".into(), p.name.clone());
        m.insert("seat".into(), p.seat.to_string());
        m.insert("port".into(), tunnel.port_for(p.account).map(|x| x.to_string()).unwrap_or_default());
        m.insert("ip".into(), "127.0.0.1".into());
        ctx.others.push(m);
    }
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lobby_messages_roundtrip() {
        let m = LobbyMsg::Start { players: vec![LobbyPlayer { account: 1, name: "a".into(), seat: 0 }], settings: BTreeMap::new(), seed: 7 };
        assert_eq!(LobbyMsg::decode(&m.encode()), Some(m));
    }
}
