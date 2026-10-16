//! Launcher-staged rooms (integration tier 0). The launcher runs the lobby
//! for a game that knows nothing about Warpzone: players gather, the host
//! picks the descriptor's settings and presses Start; every launcher writes
//! the game's launch files from the descriptor's templates, starts the game
//! and tunnels its UDP traffic over Warpzone until the game exits.
//!
//! The engine runs on its own thread (it owns the session and the tunnel),
//! so the game keeps its connection while the launcher window is hidden.

use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use warpzone_core::client::{Client, Config, Event, HostOptions};
use warpzone_core::install::{self, Installed};
use warpzone_core::tunnel::{LOBBY_CHANNEL, LobbyMsg, LobbyPlayer, TUNNEL_CHANNEL, Tunnel, start_context};
use warpzone_protocol::api::{Audience, Entry, RoomInfo, Visibility};
use warpzone_protocol::descriptor::GameDescriptor;

pub enum Cmd {
    Setting(String, String),
    Start,
    Leave,
}

#[derive(Clone, Default)]
pub struct View {
    pub room: Option<RoomInfo>,
    pub code: Option<String>,
    pub invite: Option<String>,
    pub host: bool,
    pub players: Vec<LobbyPlayer>,
    pub settings: BTreeMap<String, String>,
    pub connections: Vec<(String, bool)>,
    pub status: String,
    pub running: bool,
    pub ended: bool,
    pub error: Option<String>,
}

pub struct Lobby {
    pub game: String,
    tx: Sender<Cmd>,
    pub view: Arc<Mutex<View>>,
}

pub enum Mode {
    Host { visibility: Visibility, entry: Entry, audience: Audience, max_players: u32 },
    Join { room: String, code: Option<String>, secret: Option<String> },
}

impl Lobby {
    pub fn start(cfg: Config, desc: GameDescriptor, inst: Installed, mode: Mode, wake: eframe::egui::Context) -> Lobby {
        let (tx, rx) = channel();
        let view = Arc::new(Mutex::new(View { status: "Connecting…".into(), ..Default::default() }));
        let v = view.clone();
        let game = desc.id.clone();
        std::thread::spawn(move || {
            if let Err(e) = engine(cfg, desc, inst, mode, rx, v.clone(), &wake) {
                let mut g = v.lock().unwrap();
                g.error = Some(e);
                g.ended = true;
            }
            wake.request_repaint();
        });
        Lobby { game, tx, view }
    }

    pub fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }

    pub fn snapshot(&self) -> View {
        self.view.lock().unwrap().clone()
    }
}

fn engine(
    cfg: Config,
    desc: GameDescriptor,
    inst: Installed,
    mode: Mode,
    rx: Receiver<Cmd>,
    view: Arc<Mutex<View>>,
    wake: &eframe::egui::Context,
) -> Result<(), String> {
    let t0 = desc.tier0.clone().ok_or("this game has no launcher lobby")?;
    let mut client = Client::new(cfg).map_err(|e| e.to_string())?;
    let defaults: BTreeMap<String, String> =
        t0.settings.iter().map(|s| (s.key.clone(), s.default.clone().or_else(|| s.options.first().cloned()).unwrap_or_default())).collect();
    let mut settings = defaults;
    let mut players: Vec<LobbyPlayer> = Vec::new();
    let mut started = false;
    let mut child: Option<std::process::Child> = None;
    let mut tunnel: Option<Tunnel> = None;
    // After the process exits (relaunch_linger > 0): when the game port last
    // sent, and the frame count then.
    let mut linger: Option<(Instant, u64)> = None;
    let mut host = matches!(mode, Mode::Host { .. });
    let mut me = 0i64;
    let mut sent_request = false;
    let set = |f: &dyn Fn(&mut View)| {
        f(&mut view.lock().unwrap());
        wake.request_repaint();
    };
    let mut last_state = Instant::now();
    loop {
        client.poll();
        if !sent_request && !client.net_candidates().is_empty() {
            sent_request = true;
            me = client.account().map(|a| a.id).ok_or("choose a username in Settings first")?;
            match &mode {
                Mode::Host { visibility, entry, audience, max_players } => client.host(HostOptions {
                    max_players: *max_players,
                    visibility: *visibility,
                    entry: *entry,
                    audience: audience.clone(),
                    preset: desc.rooms.as_ref().and_then(|r| r.presets.first()).map(|p| p.id.clone()),
                    staged: true,
                    ..Default::default()
                }),
                Mode::Join { room, code, secret } => client.join(room, code.as_deref(), secret.as_deref()),
            }
        }
        while let Some(e) = client.next_event() {
            match e {
                Event::RoomHosted { code, invite, .. } => set(&|v| {
                    v.code = code.clone();
                    v.invite = Some(invite.clone());
                }),
                Event::RoomJoined { room, host: h } => {
                    host = h;
                    if h {
                        let name = client.account().map(|a| a.username).unwrap_or_default();
                        players = vec![LobbyPlayer { account: me, name, seat: 0 }];
                    }
                    let (p, s) = (players.clone(), settings.clone());
                    set(&|v| {
                        v.room = Some(room.clone());
                        v.host = h;
                        v.players = p.clone();
                        v.settings = s.clone();
                        v.status = if h { "Waiting for players — share the code or invite link".into() } else { "Waiting for the host to start".into() };
                    });
                }
                Event::Knocking { .. } => set(&|v| v.status = "Waiting for the host to let you in…".into()),
                Event::KnockDenied { .. } => return Err("the host did not let you in".into()),
                Event::Knock { knock, .. } => client.answer_knock(&knock, true),
                Event::PeerJoined { account, username, .. } if host && !started => {
                    let seat = (0..).find(|s| !players.iter().any(|p| p.seat == *s)).unwrap_or(0);
                    players.push(LobbyPlayer { account, name: username, seat });
                    last_state = Instant::now() - Duration::from_secs(5);
                }
                Event::PeerLeft { account, .. } => {
                    if host {
                        players.retain(|p| p.account != account);
                        last_state = Instant::now() - Duration::from_secs(5);
                    }
                }
                Event::PeerData { peer, channel: LOBBY_CHANNEL, data, .. } => match LobbyMsg::decode(&data) {
                    Some(LobbyMsg::State { players: p, settings: s }) if !host => {
                        players = p;
                        settings = s;
                        let (p, s) = (players.clone(), settings.clone());
                        set(&|v| {
                            v.players = p.clone();
                            v.settings = s.clone();
                        });
                    }
                    Some(LobbyMsg::Start { players: p, settings: s, seed }) if !host && !started => {
                        started = true;
                        let route: HashMap<i64, u32> = p.iter().filter(|x| x.account != me).map(|x| (x.account, peer)).collect();
                        let host_account = client.peer_account(peer).unwrap_or(0);
                        let (t, c) = launch(&desc, &inst, &t0, me, host_account, &p, &s, seed, route)?;
                        tunnel = Some(t);
                        child = Some(c);
                        set(&|v| {
                            v.running = true;
                            v.status = "Game running".into();
                        });
                    }
                    _ => {}
                },
                Event::PeerData { channel: TUNNEL_CHANNEL, data, .. } => {
                    if let Some(t) = tunnel.as_mut() {
                        t.deliver(&mut client, &data);
                    }
                }
                Event::RoomClosed { .. } if tunnel.is_none() => return Err("the room was closed".into()),
                Event::Kicked { .. } => return Err("the host removed you".into()),
                Event::Error { op, message, .. } => set(&|v| v.error = Some(format!("{op}: {message}"))),
                _ => {}
            }
        }
        // Host: share the lobby state.
        if host && !started && last_state.elapsed() > Duration::from_secs(2) {
            last_state = Instant::now();
            let msg = LobbyMsg::State { players: players.clone(), settings: settings.clone() }.encode();
            client.broadcast(LOBBY_CHANNEL, &msg, true);
            let (p, s) = (players.clone(), settings.clone());
            set(&|v| {
                v.players = p.clone();
                v.settings = s.clone();
            });
        }
        let conns: Vec<(String, bool)> = client.peers().iter().filter_map(|p| Some((client.peer_name(*p)?, client.peer_relayed(*p)?))).collect();
        {
            let mut g = view.lock().unwrap();
            if g.connections != conns {
                g.connections = conns;
                wake.request_repaint();
            }
        }
        while let Ok(c) = rx.try_recv() {
            match c {
                Cmd::Setting(k, v) if host && !started => {
                    settings.insert(k, v);
                    last_state = Instant::now() - Duration::from_secs(5);
                }
                Cmd::Start if host && !started => {
                    started = true;
                    client.set_closed(true);
                    // 1..=2^31-1: games read the seed into a signed int.
                    let seed: u32 = ((warpzone_protocol::now() as u32).wrapping_mul(2654435761) & 0x7fff_ffff) | 1;
                    let msg = LobbyMsg::Start { players: players.clone(), settings: settings.clone(), seed }.encode();
                    client.broadcast(LOBBY_CHANNEL, &msg, true);
                    let route: HashMap<i64, u32> = client.peers().iter().filter_map(|p| Some((client.peer_account(*p)?, *p))).collect();
                    let (t, c) = launch(&desc, &inst, &t0, me, me, &players, &settings, seed, route)?;
                    tunnel = Some(t);
                    child = Some(c);
                    set(&|v| {
                        v.running = true;
                        v.status = "Game running".into();
                    });
                }
                Cmd::Leave => {
                    client.leave();
                    client.poll();
                    set(&|v| v.ended = true);
                    return Ok(());
                }
                _ => {}
            }
        }
        if let Some(t) = tunnel.as_mut() {
            t.pump(&mut client);
        }
        let mut ended = false;
        if let Some(c) = child.as_mut()
            && let Ok(Some(_)) = c.try_wait()
        {
            child = None;
            if t0.relaunch_linger > 0 {
                // The game may have started its successor (same port, same
                // launch files): keep the tunnel until the game port goes quiet.
                linger = tunnel.as_ref().map(|t| (Instant::now(), t.frames_out));
            } else {
                tunnel = None;
                ended = true;
            }
        }
        if let (Some((since, seen)), Some(t)) = (linger.as_mut(), tunnel.as_ref())
            && t.frames_out != *seen
        {
            *since = Instant::now();
            *seen = t.frames_out;
        }
        if linger.is_some_and(|(since, _)| since.elapsed() > Duration::from_secs(t0.relaunch_linger.into())) {
            linger = None;
            tunnel = None;
            ended = true;
        }
        if ended {
            client.broadcast(LOBBY_CHANNEL, &LobbyMsg::Ended.encode(), true);
            set(&|v| {
                v.running = false;
                v.status = "The game ended".into();
            });
            if host {
                started = false;
                client.set_closed(false);
            }
        }
        std::thread::sleep(Duration::from_millis(if tunnel.is_some() { 1 } else { 10 }));
    }
}

#[allow(clippy::too_many_arguments)]
fn launch(
    desc: &GameDescriptor,
    inst: &Installed,
    t0: &warpzone_protocol::descriptor::Tier0,
    me: i64,
    host: i64,
    players: &[LobbyPlayer],
    settings: &BTreeMap<String, String>,
    seed: u32,
    route: HashMap<i64, u32>,
) -> Result<(Tunnel, std::process::Child), String> {
    let others: Vec<i64> = players.iter().map(|p| p.account).filter(|a| *a != me).collect();
    let tunnel = Tunnel::new(me, me == host, t0.game_port, &others, route).map_err(|e| e.to_string())?;
    let ctx = start_context(me, host, players, settings, seed, t0.game_port, &tunnel);
    let exe = install::executable(desc, inst).ok_or("game executable not found")?;
    let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or(inst.dir.clone());
    for (path, template) in &t0.files {
        let text = warpzone_core::template::render(template, &ctx)?;
        let p = dir.join(path);
        if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(format!("unsafe launch file path {path}"));
        }
        std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    let child = install::launch(desc, inst, &t0.args).map_err(|e| e.to_string())?;
    Ok((tunnel, child))
}
