//! The game-facing session.
//!
//! ```no_run
//! use warpzone_core::client::{Client, Config, Event};
//! let mut c = Client::new(Config::new("funny-furries", "0.3.0")).unwrap();
//! c.host(Default::default());
//! loop {
//!     c.poll();
//!     while let Some(e) = c.next_event() {
//!         match e {
//!             Event::PeerData { peer, data, .. } => { /* game input */ let _ = (peer, data); }
//!             _ => {}
//!         }
//!     }
//!     # break;
//! }
//! ```
//!
//! Everything slow (HTTP) runs on two service threads; `poll` only moves
//! results into the event queue and drives the UDP transport. Requests return
//! immediately; their outcome arrives as an [`Event`].

use crate::api::Api;
use crate::identity::Identity;
use crate::modstore::ModStore;
use crate::net::{Net, NetEvent, PeerId, PeerSpec};
use crate::{Error, Result};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use warpzone_protocol::api::*;
use warpzone_protocol::mods::Lockfile;

#[derive(Clone)]
pub struct Config {
    pub server: String,
    pub game: String,
    pub game_version: String,
    /// Defaults to the user's shared identity.
    pub identity: Option<Arc<Identity>>,
    /// Join match rooms automatically when matchmaking finds a match.
    pub auto_join_matches: bool,
}

impl Config {
    pub fn new(game: &str, game_version: &str) -> Config {
        Config { server: crate::server_url(), game: game.into(), game_version: game_version.into(), identity: None, auto_join_matches: true }
    }
}

/// Options for hosting.
#[derive(Clone, Debug, Default)]
pub struct HostOptions {
    pub max_players: u32,
    pub visibility: Visibility,
    pub entry: Entry,
    pub audience: Audience,
    pub preset: Option<String>,
    pub metadata: std::collections::BTreeMap<String, String>,
    pub lockfile: Option<String>,
    pub staged: bool,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// Logged in. `account` is `None` until a username is registered.
    LoggedIn {
        account: Option<Account>,
        roles: Vec<String>,
    },
    Registered(Account),
    RoomHosted {
        room: RoomInfo,
        code: Option<String>,
        invite: String,
    },
    /// We are in a room (as host after `RoomHosted`, or as guest).
    RoomJoined {
        room: RoomInfo,
        host: bool,
    },
    /// Waiting for the host to accept our knock.
    Knocking {
        room: RoomInfo,
    },
    KnockDenied {
        room: String,
    },
    /// Host: somebody knocks; answer with [`Client::answer_knock`].
    Knock {
        knock: String,
        account: i64,
        username: String,
    },
    PeerJoined {
        peer: PeerId,
        account: i64,
        username: String,
    },
    PeerConnected {
        peer: PeerId,
        relayed: bool,
    },
    PeerPathChanged {
        peer: PeerId,
        relayed: bool,
    },
    PeerData {
        peer: PeerId,
        channel: u8,
        data: Vec<u8>,
        reliable: bool,
    },
    PeerLeft {
        peer: PeerId,
        account: i64,
    },
    RoomClosed {
        room: String,
    },
    Kicked {
        room: String,
        banned: bool,
    },
    RoomList(Vec<RoomInfo>),
    Queued(QueueStatus),
    MatchFound {
        match_id: String,
        queue: String,
        room: String,
        host: bool,
        teams: Vec<Vec<String>>,
    },
    MatchFinished {
        match_id: String,
        state: String,
    },
    MatchReported(MatchInfo),
    ScoreSubmitted {
        board: String,
        result: ScoreSubmitted,
    },
    Standings(StandingsResponse),
    Board(BoardResponse),
    Update(UpdateInfo),
    ModsProgress {
        done: u64,
        total: u64,
    },
    ModsSynced {
        lockfile: Lockfile,
        paths: Vec<PathBuf>,
    },
    /// A request failed. `op` names it (`host`, `join`, ...).
    Error {
        op: String,
        code: String,
        message: String,
    },
}

enum Cmd {
    Login,
    Register(String),
    Host(CreateRoom),
    Join { room: String, req: JoinRequest },
    JoinCode { code: String, candidates: Vec<String> },
    Knock { room: String, knock: String, accept: bool },
    Heartbeat { room: String, hb: Heartbeat },
    Candidates { room: String, candidates: Vec<String> },
    Leave(String),
    Close(String),
    Kick { room: String, account: i64, ban: bool },
    List { has_space: bool },
    QueueJoin(QueueJoin),
    QueueLeave,
    Report { match_id: String, report: MatchReport },
    Score { board: String, submit: ScoreSubmit },
    Standings { queue: String, season: Option<String> },
    Board { board: String, ruleset: String, season: String },
    Updates { current: String },
    SyncMods { hash: Option<String>, lockfile: Option<Lockfile> },
}

enum Msg {
    Event(Event),
    Joined { joined: Joined, host: bool },
    Hosted(CreatedRoom),
    Server(ServerEvent),
    Ready { info: ServerInfo },
}

struct RoomState {
    id: String,
    host: bool,
    session: PeerSession,
    peers: HashMap<i64, PeerId>,
    last_beat: Instant,
    sent_candidates: Vec<String>,
}

pub struct Client {
    cfg: Config,
    api: Arc<Api>,
    tx: Sender<Cmd>,
    rx: Receiver<Msg>,
    net: Option<Net>,
    events: VecDeque<Event>,
    room: Option<RoomState>,
    stop: Arc<AtomicBool>,
    identity: Arc<Identity>,
    pending_net: Vec<Msg>,
}

impl Client {
    pub fn new(cfg: Config) -> Result<Client> {
        let identity = match &cfg.identity {
            Some(i) => i.clone(),
            None => Arc::new(Identity::shared()?),
        };
        let api = Arc::new(Api::new(&cfg.server, identity.clone()));
        let (tx, cmd_rx) = channel::<Cmd>();
        let (msg_tx, rx) = channel::<Msg>();
        let stop = Arc::new(AtomicBool::new(false));
        spawn_service(api.clone(), cfg.clone(), cmd_rx, msg_tx, stop.clone());
        tx.send(Cmd::Login).ok();
        Ok(Client { cfg, api, tx, rx, net: None, events: VecDeque::new(), room: None, stop, identity, pending_net: Vec::new() })
    }

    /// The HTTP API, for anything this session type does not wrap. Its calls
    /// block: use them from another thread, or only outside a frame loop.
    pub fn api(&self) -> Arc<Api> {
        self.api.clone()
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn account(&self) -> Option<Account> {
        self.api.account()
    }

    pub fn game(&self) -> &str {
        &self.cfg.game
    }

    pub fn game_version(&self) -> &str {
        &self.cfg.game_version
    }

    fn cmd(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }

    pub fn register(&self, username: &str) {
        self.cmd(Cmd::Register(username.into()));
    }

    fn candidates(&self) -> Vec<String> {
        self.net.as_ref().map(|n| n.candidates()).unwrap_or_default()
    }

    /// Host a room. Result: `RoomHosted` then `RoomJoined`, or `Error`.
    pub fn host(&mut self, o: HostOptions) {
        self.leave_current();
        let req = CreateRoom {
            game: self.cfg.game.clone(),
            game_version: self.cfg.game_version.clone(),
            lockfile: o.lockfile,
            max_players: if o.max_players == 0 { 2 } else { o.max_players },
            visibility: o.visibility,
            entry: o.entry,
            audience: o.audience,
            preset: o.preset,
            metadata: o.metadata,
            candidates: self.candidates(),
            staged: o.staged,
        };
        self.cmd(Cmd::Host(req));
    }

    /// Join a listed room, or one from an invite link (`secret` = the part
    /// after `#`), or a room we are already approved for (launcher hand-off).
    pub fn join(&mut self, room: &str, code: Option<&str>, secret: Option<&str>) {
        self.leave_current();
        let req = JoinRequest { code: code.map(String::from), invite: secret.map(String::from), candidates: self.candidates() };
        self.cmd(Cmd::Join { room: room.into(), req });
    }

    pub fn join_code(&mut self, code: &str) {
        self.leave_current();
        let candidates = self.candidates();
        self.cmd(Cmd::JoinCode { code: code.into(), candidates });
    }

    pub fn answer_knock(&self, knock: &str, accept: bool) {
        if let Some(r) = &self.room {
            self.cmd(Cmd::Knock { room: r.id.clone(), knock: knock.into(), accept });
        }
    }

    /// Host: update the metadata the browser shows (map, mode, ...).
    pub fn set_metadata(&mut self, metadata: std::collections::BTreeMap<String, String>) {
        if let Some(r) = &self.room
            && r.host
        {
            self.cmd(Cmd::Heartbeat { room: r.id.clone(), hb: Heartbeat { metadata: Some(metadata), ..Default::default() } });
        }
    }

    /// Host: stop (or allow) new joiners, e.g. once the game started.
    pub fn set_closed(&mut self, closed: bool) {
        if let Some(r) = &self.room
            && r.host
        {
            self.cmd(Cmd::Heartbeat { room: r.id.clone(), hb: Heartbeat { closed_to_joins: Some(closed), ..Default::default() } });
        }
    }

    pub fn kick(&self, account: i64, ban: bool) {
        if let Some(r) = &self.room {
            self.cmd(Cmd::Kick { room: r.id.clone(), account, ban });
        }
    }

    fn leave_current(&mut self) {
        if let Some(r) = self.room.take() {
            if r.host {
                self.cmd(Cmd::Close(r.id));
            } else {
                self.cmd(Cmd::Leave(r.id));
            }
            if let Some(n) = self.net.as_mut() {
                n.clear();
            }
        }
    }

    /// Leave (or, as host, close) the current room.
    pub fn leave(&mut self) {
        self.leave_current();
    }

    pub fn room_id(&self) -> Option<&str> {
        self.room.as_ref().map(|r| r.id.as_str())
    }

    pub fn is_host(&self) -> bool {
        self.room.as_ref().is_some_and(|r| r.host)
    }

    pub fn list_rooms(&self, has_space: bool) {
        self.cmd(Cmd::List { has_space });
    }

    pub fn queue(&self, queue: &str, ruleset: &str) {
        let q = QueueJoin {
            game: self.cfg.game.clone(),
            queue: queue.into(),
            game_version: self.cfg.game_version.clone(),
            ruleset: ruleset.into(),
            candidates: self.candidates(),
        };
        self.cmd(Cmd::QueueJoin(q));
    }

    pub fn leave_queue(&self) {
        self.cmd(Cmd::QueueLeave);
    }

    pub fn report_match(&self, match_id: &str, report: MatchReport) {
        self.cmd(Cmd::Report { match_id: match_id.into(), report });
    }

    pub fn submit_score(&self, board: &str, submit: ScoreSubmit) {
        self.cmd(Cmd::Score { board: board.into(), submit });
    }

    pub fn request_standings(&self, queue: &str, season: Option<&str>) {
        self.cmd(Cmd::Standings { queue: queue.into(), season: season.map(String::from) });
    }

    pub fn request_board(&self, board: &str, ruleset: &str, season: &str) {
        self.cmd(Cmd::Board { board: board.into(), ruleset: ruleset.into(), season: season.into() });
    }

    /// Ask whether there is a newer release and what changed since ours.
    pub fn check_update(&self) {
        self.cmd(Cmd::Updates { current: self.cfg.game_version.clone() });
    }

    /// Install (if needed) and activate the mod set `hash` (a room's lockfile).
    pub fn sync_mods(&self, hash: &str) {
        self.cmd(Cmd::SyncMods { hash: Some(hash.into()), lockfile: None });
    }

    pub fn sync_lockfile(&self, lock: Lockfile) {
        self.cmd(Cmd::SyncMods { hash: None, lockfile: Some(lock) });
    }

    // ------------------------------------------------------------ peers

    pub fn send(&mut self, peer: PeerId, channel: u8, data: &[u8], reliable: bool) -> bool {
        match self.net.as_mut() {
            Some(n) if reliable => n.send_reliable(peer, channel, data),
            Some(n) => n.send(peer, channel, data),
            None => false,
        }
    }

    pub fn broadcast(&mut self, channel: u8, data: &[u8], reliable: bool) -> usize {
        let peers: Vec<PeerId> = self.room.as_ref().map(|r| r.peers.values().copied().collect()).unwrap_or_default();
        peers.into_iter().filter(|p| self.send(*p, channel, data, reliable)).count()
    }

    pub fn peers(&self) -> Vec<PeerId> {
        self.room.as_ref().map(|r| r.peers.values().copied().collect()).unwrap_or_default()
    }

    pub fn peer_name(&self, peer: PeerId) -> Option<String> {
        self.net.as_ref()?.peer_name(peer).map(String::from)
    }

    pub fn peer_account(&self, peer: PeerId) -> Option<i64> {
        self.net.as_ref()?.peer_account(peer)
    }

    pub fn peer_rtt(&self, peer: PeerId) -> Option<Duration> {
        self.net.as_ref()?.rtt(peer)
    }

    pub fn peer_relayed(&self, peer: PeerId) -> Option<bool> {
        self.net.as_ref()?.is_relayed(peer)
    }

    /// Testing: route all peer traffic through the relay.
    pub fn force_relay(&mut self, on: bool) {
        if let Some(n) = self.net.as_mut() {
            n.relay_only = on;
        }
    }

    pub fn net_candidates(&self) -> Vec<String> {
        self.candidates()
    }

    pub fn next_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    fn add_peer(&mut self, p: &PeerInfo) {
        let Some(room) = self.room.as_mut() else { return };
        let Some(net) = self.net.as_mut() else { return };
        let spec = PeerSpec {
            room: room.id.clone(),
            account: p.account,
            username: p.username.clone(),
            key: p.key.clone(),
            candidates: p.candidates.clone(),
            relay: p.relay,
            my_ticket: room.session.ticket.clone(),
            my_role: room.session.role,
        };
        if let Ok(id) = net.add_peer(spec) {
            room.peers.insert(p.account, id);
            self.events.push_back(Event::PeerJoined { peer: id, account: p.account, username: p.username.clone() });
        }
    }

    fn on_joined(&mut self, j: Joined, host: bool) {
        if let Some(n) = self.net.as_mut() {
            n.clear();
        }
        self.room = Some(RoomState {
            id: j.room.id.clone(),
            host,
            session: j.session.clone(),
            peers: HashMap::new(),
            last_beat: Instant::now(),
            sent_candidates: self.candidates(),
        });
        self.events.push_back(Event::RoomJoined { room: j.room.clone(), host });
        for p in &j.peers {
            self.add_peer(p);
        }
    }

    fn drop_peer(&mut self, account: i64) {
        let Some(room) = self.room.as_mut() else { return };
        if let Some(id) = room.peers.remove(&account) {
            if let Some(n) = self.net.as_mut() {
                n.remove_peer(id);
            }
            self.events.push_back(Event::PeerLeft { peer: id, account });
        }
    }

    fn on_server(&mut self, e: ServerEvent) {
        let in_room = |r: &str, s: &Option<RoomState>| s.as_ref().is_some_and(|x| x.id == r);
        match e {
            ServerEvent::Knock { room, knock, account, username } if in_room(&room, &self.room) => {
                self.events.push_back(Event::Knock { knock, account, username });
            }
            ServerEvent::KnockResult { room, accepted, joined } => {
                if accepted && let Some(j) = joined {
                    self.on_joined(*j, false);
                } else {
                    self.events.push_back(Event::KnockDenied { room });
                }
            }
            ServerEvent::PeerJoined { room, peer } if in_room(&room, &self.room) => self.add_peer(&peer),
            ServerEvent::PeerLeft { room, account } if in_room(&room, &self.room) => self.drop_peer(account),
            ServerEvent::PeerCandidates { room, account, candidates } if in_room(&room, &self.room) => {
                if let (Some(r), Some(n)) = (self.room.as_ref(), self.net.as_mut())
                    && let Some(id) = r.peers.get(&account)
                {
                    n.update_candidates(*id, &candidates);
                }
            }
            ServerEvent::RoomClosed { room } if in_room(&room, &self.room) => {
                self.room = None;
                if let Some(n) = self.net.as_mut() {
                    n.clear();
                }
                self.events.push_back(Event::RoomClosed { room });
            }
            ServerEvent::Kicked { room, banned } if in_room(&room, &self.room) => {
                self.room = None;
                if let Some(n) = self.net.as_mut() {
                    n.clear();
                }
                self.events.push_back(Event::Kicked { room, banned });
            }
            ServerEvent::MatchFound { match_id, game, queue, room, host, teams } if game == self.cfg.game => {
                if self.cfg.auto_join_matches {
                    self.join(&room, None, None);
                }
                self.events.push_back(Event::MatchFound { match_id, queue, room, host, teams });
            }
            ServerEvent::MatchFinished { match_id, state } => self.events.push_back(Event::MatchFinished { match_id, state }),
            _ => {}
        }
    }

    /// Process results, events and peer traffic. Call every frame.
    pub fn poll(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Ready { info } => {
                    match Net::new(self.identity.clone(), &info.udp, &info.server_key) {
                        Ok(n) => self.net = Some(n),
                        Err(e) => self.events.push_back(Event::Error { op: "udp".into(), code: "io".into(), message: e.to_string() }),
                    }
                    let held: Vec<Msg> = std::mem::take(&mut self.pending_net);
                    for h in held {
                        self.handle_msg(h);
                    }
                }
                // Only room traffic needs the UDP side; plain events (the
                // offline login error above all) go out right away.
                Msg::Event(e) => self.events.push_back(e),
                m if self.net.is_none() => self.pending_net.push(m),
                m => self.handle_msg(m),
            }
        }
        let Some(net) = self.net.as_mut() else { return };
        net.poll();
        while let Some(e) = net.next_event() {
            match e {
                NetEvent::Connected { peer, relayed } => self.events.push_back(Event::PeerConnected { peer, relayed }),
                NetEvent::PathChanged { peer, relayed } => self.events.push_back(Event::PeerPathChanged { peer, relayed }),
                NetEvent::Data { peer, channel, data, reliable } => self.events.push_back(Event::PeerData { peer, channel, data, reliable }),
                NetEvent::Lost { peer } => {
                    let account = net.peer_account(peer).unwrap_or(0);
                    if let Some(r) = self.room.as_mut() {
                        r.peers.retain(|_, p| *p != peer);
                    }
                    net.remove_peer(peer);
                    self.events.push_back(Event::PeerLeft { peer, account });
                }
            }
        }
        // Heartbeats (host) and candidate updates (STUN finished later).
        let cands = net.candidates();
        if let Some(r) = self.room.as_mut() {
            let changed = cands != r.sent_candidates && !cands.is_empty();
            if r.host && (r.last_beat.elapsed() > Duration::from_secs(15) || changed) {
                // Peers we still know (connected or connecting); the server
                // drops members missing from this list after a grace period.
                let report = (r.last_beat.elapsed() > Duration::from_secs(15)).then(|| r.peers.keys().copied().collect::<Vec<i64>>());
                r.last_beat = Instant::now();
                let hb = Heartbeat { candidates: changed.then(|| cands.clone()), connected: report, ..Default::default() };
                r.sent_candidates = cands.clone();
                let _ = self.tx.send(Cmd::Heartbeat { room: r.id.clone(), hb });
            } else if !r.host && changed {
                r.sent_candidates = cands.clone();
                let _ = self.tx.send(Cmd::Candidates { room: r.id.clone(), candidates: cands });
            }
        }
    }

    fn handle_msg(&mut self, m: Msg) {
        match m {
            Msg::Event(e) => self.events.push_back(e),
            Msg::Hosted(c) => {
                self.events.push_back(Event::RoomHosted { room: c.room.clone(), code: c.code.clone(), invite: c.invite.clone() });
                self.on_joined(Joined { room: c.room, session: c.session, peers: Vec::new() }, true);
            }
            Msg::Joined { joined, host } => self.on_joined(joined, host),
            Msg::Server(e) => self.on_server(e),
            Msg::Ready { .. } => {}
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.leave_current();
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn err_event(op: &str, e: &Error) -> Event {
    let message = e.to_string();
    Event::Error { op: op.into(), code: e.code().into(), message }
}

fn spawn_service(api: Arc<Api>, cfg: Config, rx: Receiver<Cmd>, tx: Sender<Msg>, stop: Arc<AtomicBool>) {
    let events_started = Arc::new(Mutex::new(false));
    std::thread::Builder::new()
        .name("warpzone-api".into())
        .spawn(move || {
            while let Ok(cmd) = rx.recv() {
                if stop.load(Ordering::SeqCst) {
                    // Finish the goodbye commands queued by Drop, then stop.
                    match cmd {
                        Cmd::Leave(r) => {
                            let _ = api.leave_room(&r);
                        }
                        Cmd::Close(r) => {
                            let _ = api.close_room(&r);
                        }
                        _ => {}
                    }
                    continue;
                }
                let send = |m: Msg| {
                    let _ = tx.send(m);
                };
                let ev = |e: Event| {
                    let _ = tx.send(Msg::Event(e));
                };
                match cmd {
                    Cmd::Login => match api.info().and_then(|info| api.login().map(|l| (info, l))) {
                        Ok((info, l)) => {
                            if l.account.is_some() {
                                start_events(&api, &tx, &stop, &events_started);
                            }
                            send(Msg::Ready { info });
                            ev(Event::LoggedIn { account: l.account.clone(), roles: l.roles.clone() });
                        }
                        Err(e) => {
                            ev(err_event("login", &e));
                            // Offline: try again in a while.
                            let api2 = api.clone();
                            let tx2 = tx.clone();
                            let stop2 = stop.clone();
                            let started = events_started.clone();
                            std::thread::spawn(move || {
                                for _ in 0..30 {
                                    std::thread::sleep(Duration::from_secs(10));
                                    if stop2.load(Ordering::SeqCst) {
                                        return;
                                    }
                                    if let Ok((info, l)) = api2.info().and_then(|info| api2.login().map(|l| (info, l))) {
                                        if l.account.is_some() {
                                            start_events(&api2, &tx2, &stop2, &started);
                                        }
                                        let _ = tx2.send(Msg::Ready { info });
                                        let _ = tx2.send(Msg::Event(Event::LoggedIn { account: l.account.clone(), roles: l.roles }));
                                        return;
                                    }
                                }
                            });
                        }
                    },
                    Cmd::Register(name) => match api.register(&name) {
                        Ok(a) => {
                            ev(Event::Registered(a));
                            start_events(&api, &tx, &stop, &events_started);
                        }
                        Err(e) => ev(err_event("register", &e)),
                    },
                    Cmd::Host(req) => match api.create_room(&req) {
                        Ok(c) => send(Msg::Hosted(c)),
                        Err(e) => ev(err_event("host", &e)),
                    },
                    Cmd::Join { room, req } => handle_join(&tx, api.join_room(&room, &req)),
                    Cmd::JoinCode { code, candidates } => handle_join(&tx, api.join_code(&code, candidates)),
                    Cmd::Knock { room, knock, accept } => {
                        if let Err(e) = api.decide_knock(&room, &knock, accept) {
                            ev(err_event("knock", &e));
                        }
                    }
                    Cmd::Heartbeat { room, hb } => {
                        if let Err(e) = api.heartbeat(&room, &hb)
                            && e.code() == "not_found"
                        {
                            send(Msg::Server(ServerEvent::RoomClosed { room }));
                        }
                    }
                    Cmd::Candidates { room, candidates } => {
                        let _ = api.update_candidates(&room, candidates);
                    }
                    Cmd::Leave(room) => {
                        let _ = api.leave_room(&room);
                    }
                    Cmd::Close(room) => {
                        let _ = api.close_room(&room);
                    }
                    Cmd::Kick { room, account, ban } => {
                        if let Err(e) = api.kick(&room, account, ban) {
                            ev(err_event("kick", &e));
                        }
                    }
                    Cmd::List { has_space } => match api.rooms(Some(&cfg.game), has_space) {
                        Ok(r) => ev(Event::RoomList(r)),
                        Err(e) => ev(err_event("list", &e)),
                    },
                    Cmd::QueueJoin(q) => match api.queue_join(&q) {
                        Ok(s) => ev(Event::Queued(s)),
                        Err(e) => ev(err_event("queue", &e)),
                    },
                    Cmd::QueueLeave => match api.queue_leave() {
                        Ok(s) => ev(Event::Queued(s)),
                        Err(e) => ev(err_event("queue", &e)),
                    },
                    Cmd::Report { match_id, report } => match api.report_match(&match_id, &report) {
                        Ok(m) => ev(Event::MatchReported(m)),
                        Err(e) => ev(err_event("report", &e)),
                    },
                    Cmd::Score { board, submit } => match api.submit_score(&cfg.game, &board, &submit) {
                        Ok(r) => ev(Event::ScoreSubmitted { board, result: r }),
                        Err(e) => ev(err_event("score", &e)),
                    },
                    Cmd::Standings { queue, season } => match api.standings(&cfg.game, &queue, season.as_deref()) {
                        Ok(s) => ev(Event::Standings(s)),
                        Err(e) => ev(err_event("standings", &e)),
                    },
                    Cmd::Board { board, ruleset, season } => match api.board(&cfg.game, &board, &ruleset, &season) {
                        Ok(b) => ev(Event::Board(b)),
                        Err(e) => ev(err_event("board", &e)),
                    },
                    Cmd::Updates { current } => match api.updates(&cfg.game, &current) {
                        Ok(u) => ev(Event::Update(u)),
                        Err(e) => ev(err_event("update", &e)),
                    },
                    Cmd::SyncMods { hash, lockfile } => {
                        let result = (|| -> Result<(Lockfile, Vec<PathBuf>)> {
                            let lock = match (lockfile, hash) {
                                (Some(l), _) => l,
                                (None, Some(h)) => api.lockfile(&cfg.game, &h)?,
                                (None, None) => return Err(Error::Other("no lockfile".into())),
                            };
                            let extract = api.game(&cfg.game).ok().and_then(|g| g.mods).is_some_and(|m| m.install == "extract");
                            let store = ModStore::new(&cfg.game, extract);
                            let tx2 = tx.clone();
                            let paths = store.sync(&api, &lock, &mut |done, total| {
                                let _ = tx2.send(Msg::Event(Event::ModsProgress { done, total }));
                            })?;
                            Ok((lock, paths))
                        })();
                        match result {
                            Ok((lockfile, paths)) => ev(Event::ModsSynced { lockfile, paths }),
                            Err(e) => ev(err_event("mods", &e)),
                        }
                    }
                }
            }
        })
        .expect("spawn warpzone-api thread");
}

fn handle_join(tx: &Sender<Msg>, r: Result<JoinResponse>) {
    let _ = match r {
        Ok(JoinResponse::Joined(j)) => {
            let host = j.session.role == Role::Host;
            tx.send(Msg::Joined { joined: j, host })
        }
        Ok(JoinResponse::Knocking { room, .. }) => tx.send(Msg::Event(Event::Knocking { room })),
        Err(e) => tx.send(Msg::Event(err_event("join", &e))),
    };
}

/// The account's event queue is shared by all its clients and keeps older events
/// (matches found and finished before this client existed). Start at its current
/// end, so a new client never acts on them.
fn events_head(api: &Api) -> Option<u64> {
    api.events(0, 0).ok().map(|r| r.next)
}

/// Called on the API thread before `LoggedIn` goes out, so the cursor is set
/// before the game can queue or host.
fn start_events(api: &Arc<Api>, tx: &Sender<Msg>, stop: &Arc<AtomicBool>, started: &Arc<Mutex<bool>>) {
    {
        let mut s = started.lock().unwrap();
        if *s {
            return;
        }
        *s = true;
    }
    let head = events_head(api);
    let api = api.clone();
    let tx = tx.clone();
    let stop = stop.clone();
    std::thread::Builder::new()
        .name("warpzone-events".into())
        .spawn(move || {
            let mut after = head;
            while !stop.load(Ordering::SeqCst) {
                let Some(a) = after else {
                    after = events_head(&api);
                    if after.is_none() {
                        std::thread::sleep(Duration::from_secs(3));
                    }
                    continue;
                };
                match api.events(a, 25) {
                    Ok(r) => {
                        after = Some(r.next);
                        for e in r.events {
                            if tx.send(Msg::Server(e.event)).is_err() {
                                return;
                            }
                        }
                    }
                    Err(_) => std::thread::sleep(Duration::from_secs(3)),
                }
            }
        })
        .expect("spawn warpzone-events thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_login_error_is_delivered() {
        let mut cfg = Config::new("test", "0");
        cfg.server = "http://127.0.0.1:9".into();
        cfg.identity = Some(Arc::new(Identity::generate()));
        let mut c = Client::new(cfg).unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while Instant::now() < until {
            c.poll();
            if let Some(e) = c.next_event() {
                assert!(matches!(e, Event::Error { ref op, .. } if op == "login"), "{e:?}");
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("no event while offline");
    }
}
