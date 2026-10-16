//! Rooms: in memory only. A room is a star around its host: every guest gets a
//! ticket, the host's candidates and one relay channel to the host.

use crate::auth::{Auth, MaybeAuth, client_ip};
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, State as Srv, principal_account};
use axum::Json;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::HeaderMap;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use warpzone_protocol::api::*;
use warpzone_protocol::names::{self, ROOM_CODE_LEN};
use warpzone_protocol::now;
use warpzone_protocol::ticket::Ticket;

/// A room dies this long after the host's last heartbeat.
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(45);
/// Tickets are valid for a long session.
const TICKET_TTL: i64 = 6 * 3600;
const KNOCK_TTL: Duration = Duration::from_secs(300);

pub struct Member {
    pub account: i64,
    pub username: String,
    pub key: String,
    pub candidates: Vec<String>,
    /// Relay channel to the host (guests only).
    pub relay: Option<(u32, [[u8; 16]; 2])>,
    pub joined: Instant,
}

pub struct Knock {
    account: i64,
    username: String,
    key: String,
    candidates: Vec<String>,
    at: Instant,
}

pub struct Room {
    pub id: String,
    pub game: String,
    pub game_version: String,
    pub host: i64,
    pub max_players: u32,
    pub visibility: Visibility,
    pub entry: Entry,
    pub audience: Audience,
    pub preset: Option<String>,
    pub name: String,
    pub metadata: BTreeMap<String, String>,
    pub lockfile: Option<String>,
    pub code: Option<String>,
    pub secret: String,
    pub staged: bool,
    pub ranked: bool,
    pub created: i64,
    pub beat: Instant,
    pub closed_to_joins: bool,
    pub members: BTreeMap<i64, Member>,
    pub approved: HashSet<i64>,
    pub banned: HashSet<i64>,
    knocks: HashMap<String, Knock>,
}

#[derive(Default)]
pub struct Rooms {
    pub by_id: HashMap<String, Room>,
    by_code: HashMap<String, String>,
}

impl Room {
    pub fn info(&self) -> RoomInfo {
        let host = self.members.get(&self.host).map(|m| m.username.clone()).unwrap_or_default();
        RoomInfo {
            id: self.id.clone(),
            game: self.game.clone(),
            game_version: self.game_version.clone(),
            name: self.name.clone(),
            host,
            players: self.members.len() as u32,
            max_players: self.max_players,
            visibility: self.visibility,
            entry: self.entry,
            audience: self.audience.clone(),
            ranked: self.ranked,
            staged: self.staged,
            lockfile: self.lockfile.clone(),
            preset: self.preset.clone(),
            metadata: self.metadata.clone(),
            members: self.members.values().map(|m| m.username.clone()).collect(),
            created: self.created,
        }
    }

    fn ticket(&self, srv: &Srv, m: &Member, role: Role) -> String {
        let t = now();
        Ticket {
            v: 1,
            room: self.id.clone(),
            game: self.game.clone(),
            account: m.account,
            username: m.username.clone(),
            key: m.key.clone(),
            role,
            issued: t,
            expires: t + TICKET_TTL,
        }
        .sign(&srv.server_key)
    }

    /// The host as a guest sees it.
    fn host_peer(&self, guest: &Member) -> PeerInfo {
        let h = &self.members[&self.host];
        let (ch, tokens) = guest.relay.expect("guests have a relay channel");
        PeerInfo {
            account: h.account,
            username: h.username.clone(),
            key: h.key.clone(),
            role: Role::Host,
            candidates: h.candidates.clone(),
            relay: RelayGrant { channel: ch, side: 1, token: tokens[1] },
        }
    }

    /// A guest as the host sees it.
    fn guest_peer(guest: &Member) -> PeerInfo {
        let (ch, tokens) = guest.relay.expect("guests have a relay channel");
        PeerInfo {
            account: guest.account,
            username: guest.username.clone(),
            key: guest.key.clone(),
            role: Role::Guest,
            candidates: guest.candidates.clone(),
            relay: RelayGrant { channel: ch, side: 0, token: tokens[0] },
        }
    }

    fn joined_as_host(&self, srv: &Srv) -> Joined {
        let h = &self.members[&self.host];
        Joined {
            room: self.info(),
            session: PeerSession { ticket: self.ticket(srv, h, Role::Host), role: Role::Host },
            peers: self.members.values().filter(|m| m.account != self.host).map(Room::guest_peer).collect(),
        }
    }

    fn joined_as_guest(&self, srv: &Srv, account: i64) -> Joined {
        let g = &self.members[&account];
        Joined { room: self.info(), session: PeerSession { ticket: self.ticket(srv, g, Role::Guest), role: Role::Guest }, peers: vec![self.host_peer(g)] }
    }
}

pub struct NewRoom {
    pub game: String,
    pub game_version: String,
    pub max_players: u32,
    pub visibility: Visibility,
    pub entry: Entry,
    pub audience: Audience,
    pub preset: Option<String>,
    pub name: String,
    pub metadata: BTreeMap<String, String>,
    pub lockfile: Option<String>,
    pub staged: bool,
    pub ranked: bool,
    pub host: Member,
}

impl Rooms {
    fn unique_code(&self) -> String {
        loop {
            let c = names::generate_code(ROOM_CODE_LEN);
            if !self.by_code.contains_key(&c) {
                return c;
            }
        }
    }

    pub fn create(&mut self, r: NewRoom) -> &Room {
        let id = loop {
            let id = names::generate_code(10).to_ascii_lowercase();
            if !self.by_id.contains_key(&id) {
                break id;
            }
        };
        let code = (r.entry == Entry::Code || r.visibility == Visibility::Unlisted).then(|| self.unique_code());
        if let Some(c) = &code {
            self.by_code.insert(c.clone(), id.clone());
        }
        let host = r.host.account;
        let mut members = BTreeMap::new();
        members.insert(host, r.host);
        let room = Room {
            id: id.clone(),
            game: r.game,
            game_version: r.game_version,
            host,
            max_players: r.max_players,
            visibility: r.visibility,
            entry: r.entry,
            audience: r.audience,
            preset: r.preset,
            name: r.name,
            metadata: r.metadata,
            lockfile: r.lockfile,
            code,
            secret: hex::encode(rand::random::<[u8; 16]>()),
            staged: r.staged,
            ranked: r.ranked,
            created: now(),
            beat: Instant::now(),
            closed_to_joins: false,
            members,
            approved: HashSet::new(),
            banned: HashSet::new(),
            knocks: HashMap::new(),
        };
        self.by_id.insert(id.clone(), room);
        &self.by_id[&id]
    }

    /// Add a guest; tells the host. Returns what the guest needs.
    pub fn add_member(&mut self, srv: &Srv, room_id: &str, account: i64, username: String, key: String, candidates: Vec<String>) -> Joined {
        let room = self.by_id.get_mut(room_id).expect("room exists");
        if let Some(old) = room.members.remove(&account)
            && let Some((ch, _)) = old.relay
        {
            srv.relay.close(ch);
        }
        let relay = srv.relay.open(room_id);
        let m = Member { account, username, key, candidates, relay: Some(relay), joined: Instant::now() };
        srv.events.push(&principal_account(room.host), ServerEvent::PeerJoined { room: room_id.into(), peer: Room::guest_peer(&m) });
        room.members.insert(account, m);
        room.joined_as_guest(srv, account)
    }

    pub fn remove_member(&mut self, srv: &Srv, room_id: &str, account: i64) {
        let Some(room) = self.by_id.get_mut(room_id) else { return };
        if account == room.host {
            self.close(srv, room_id);
            return;
        }
        if let Some(m) = room.members.remove(&account) {
            if let Some((ch, _)) = m.relay {
                srv.relay.close(ch);
            }
            for other in room.members.keys() {
                srv.events.push(&principal_account(*other), ServerEvent::PeerLeft { room: room_id.into(), account });
            }
        }
    }

    pub fn close(&mut self, srv: &Srv, room_id: &str) {
        if let Some(room) = self.by_id.remove(room_id) {
            if let Some(c) = &room.code {
                self.by_code.remove(c);
            }
            srv.relay.close_room(room_id);
            for m in room.members.keys() {
                srv.events.push(&principal_account(*m), ServerEvent::RoomClosed { room: room_id.into() });
            }
            for k in room.knocks.values() {
                srv.events.push(&principal_account(k.account), ServerEvent::KnockResult { room: room_id.into(), accepted: false, joined: None });
            }
        }
    }

    /// Leave every room (account deletion).
    pub fn remove_account(&mut self, srv: &Srv, account: i64) {
        let ids: Vec<String> = self.by_id.values().filter(|r| r.members.contains_key(&account)).map(|r| r.id.clone()).collect();
        for id in ids {
            self.remove_member(srv, &id, account);
        }
    }

    pub fn hosted_by(&self, account: i64) -> usize {
        self.by_id.values().filter(|r| r.host == account && !r.ranked).count()
    }

    /// Close rooms whose host stopped sending heartbeats; expire knocks.
    pub fn sweep(&mut self, srv: &Srv) {
        let dead: Vec<String> = self.by_id.values().filter(|r| r.beat.elapsed() > HEARTBEAT_TIMEOUT).map(|r| r.id.clone()).collect();
        for id in dead {
            log::debug!("room {id} timed out");
            self.close(srv, &id);
        }
        for room in self.by_id.values_mut() {
            let id = room.id.clone();
            room.knocks.retain(|_, k| {
                let keep = k.at.elapsed() < KNOCK_TTL;
                if !keep {
                    srv.events.push(&principal_account(k.account), ServerEvent::KnockResult { room: id.clone(), accepted: false, joined: None });
                }
                keep
            });
        }
    }
}

fn valid_candidates(c: &[String]) -> Result<Vec<String>, ApiError> {
    if c.len() > 16 {
        return Err(ApiError::bad("at most 16 candidates"));
    }
    for a in c {
        a.parse::<SocketAddr>().map_err(|_| ApiError::bad(format!("bad candidate {a}")))?;
    }
    Ok(c.to_vec())
}

fn check_metadata(state: &AppState, m: &BTreeMap<String, String>) -> Result<(), ApiError> {
    let size: usize = m.iter().map(|(k, v)| k.len() + v.len()).sum();
    if size > state.cfg.limits.max_metadata_bytes as usize {
        return Err(ApiError::bad("room metadata too large"));
    }
    if m.keys().any(|k| !names::valid_slug(k)) {
        return Err(ApiError::bad("metadata keys are 1-32 characters of a-z 0-9 _ - ."));
    }
    Ok(())
}

pub async fn create(State(state): State<AppState>, auth: Auth, Json(req): Json<CreateRoom>) -> ApiResult<CreatedRoom> {
    let acc = auth.account()?.clone();
    let game = state.game(&req.game)?;
    let rooms_cfg = game.rooms.as_ref().ok_or_else(|| ApiError::bad(format!("{} has no online rooms", game.name)))?;
    if req.staged && game.tier0.is_none() {
        return Err(ApiError::bad("this game has no launcher-staged mode"));
    }
    let max = rooms_cfg.max_players.min(state.cfg.limits.max_room_players);
    if req.max_players < 2 || req.max_players > max {
        return Err(ApiError::bad(format!("max_players must be 2..={max}")));
    }
    if req.game_version.is_empty() || req.game_version.len() > 40 {
        return Err(ApiError::bad("game_version is required"));
    }
    check_metadata(&state, &req.metadata)?;
    if req.audience.languages.len() > 5 || req.audience.languages.iter().any(|l| !names::valid_language(l)) {
        return Err(ApiError::bad("up to 5 ISO 639-1 language codes"));
    }
    if let Some(l) = &req.lockfile
        && (l.len() != 64 || !l.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(ApiError::bad("lockfile must be a sha256 hex hash"));
    }
    let label = match &req.preset {
        Some(p) => game.preset_label(p).ok_or_else(|| ApiError::bad(format!("unknown preset {p}")))?.to_string(),
        None => "game".to_string(),
    };
    let candidates = valid_candidates(&req.candidates)?;
    let mut rooms = state.rooms.lock().unwrap();
    if rooms.hosted_by(acc.id) >= state.cfg.limits.rooms_per_account as usize {
        return Err(ApiError::conflict("you host too many rooms already; close one first"));
    }
    let room = rooms.create(NewRoom {
        game: req.game,
        game_version: req.game_version,
        max_players: req.max_players,
        visibility: req.visibility,
        entry: req.entry,
        audience: req.audience,
        preset: req.preset,
        name: format!("{}'s {}", acc.username, label),
        metadata: req.metadata,
        lockfile: req.lockfile,
        staged: req.staged,
        ranked: false,
        host: Member { account: acc.id, username: acc.username.clone(), key: auth.key.clone(), candidates, relay: None, joined: Instant::now() },
    });
    let joined = room.joined_as_host(&state);
    Ok(Json(CreatedRoom {
        code: room.code.clone(),
        invite: format!("warpzone://join/{}#{}", room.id, room.secret),
        room: joined.room,
        session: joined.session,
    }))
}

#[derive(serde::Deserialize, Default)]
pub struct RoomQuery {
    pub game: Option<String>,
    pub language: Option<String>,
    pub skill: Option<Skill>,
    pub style: Option<Style>,
    #[serde(default)]
    pub has_space: bool,
}

pub fn list_public(state: &AppState, q: &RoomQuery) -> Vec<RoomInfo> {
    let rooms = state.rooms.lock().unwrap();
    let mut v: Vec<RoomInfo> = rooms
        .by_id
        .values()
        .filter(|r| r.visibility == Visibility::Public && !r.ranked)
        .filter(|r| q.game.as_ref().is_none_or(|g| &r.game == g))
        .filter(|r| q.language.as_ref().is_none_or(|l| r.audience.languages.is_empty() || r.audience.languages.contains(l)))
        .filter(|r| q.skill.is_none_or(|s| r.audience.skill == s))
        .filter(|r| q.style.is_none_or(|s| r.audience.style == s))
        .filter(|r| !q.has_space || ((r.members.len() as u32) < r.max_players && !r.closed_to_joins))
        .map(|r| r.info())
        .collect();
    v.sort_by_key(|r| std::cmp::Reverse(r.created));
    v
}

pub async fn list(State(state): State<AppState>, Query(q): Query<RoomQuery>) -> ApiResult<Vec<RoomInfo>> {
    Ok(Json(list_public(&state, &q)))
}

#[derive(serde::Deserialize)]
pub struct SecretQuery {
    pub s: Option<String>,
}

pub async fn get(State(state): State<AppState>, MaybeAuth(auth): MaybeAuth, Path(id): Path<String>, Query(q): Query<SecretQuery>) -> ApiResult<RoomInfo> {
    let rooms = state.rooms.lock().unwrap();
    let room = rooms.by_id.get(&id).ok_or_else(|| ApiError::not_found("no such room"))?;
    let member = auth.as_ref().and_then(|a| a.account.as_ref()).is_some_and(|a| room.members.contains_key(&a.id));
    if room.visibility == Visibility::Public || member || q.s.as_deref() == Some(room.secret.as_str()) {
        Ok(Json(room.info()))
    } else {
        Err(ApiError::not_found("no such room"))
    }
}

/// Shared by `join` and `join_by_code`.
fn do_join(state: &AppState, auth: &Auth, room_id: &str, req: &JoinRequest) -> Result<JoinResponse, ApiError> {
    let acc = auth.account()?.clone();
    let candidates = valid_candidates(&req.candidates)?;
    let mut rooms = state.rooms.lock().unwrap();
    let room = rooms.by_id.get_mut(room_id).ok_or_else(|| ApiError::not_found("no such room"))?;
    if room.banned.contains(&acc.id) {
        return Err(ApiError::forbidden("the host removed you from this room"));
    }
    if acc.id == room.host {
        if let Some(h) = room.members.get_mut(&acc.id) {
            h.key = auth.key.clone();
            if !candidates.is_empty() {
                h.candidates = candidates;
            }
        }
        return Ok(JoinResponse::Joined(room.joined_as_host(state)));
    }
    let code_ok = req.code.as_deref().map(names::normalize_code).is_some_and(|c| room.code.as_deref() == Some(c.as_str()));
    let invite_ok = req.invite.as_deref() == Some(room.secret.as_str());
    let member = room.members.contains_key(&acc.id);
    let approved = room.approved.contains(&acc.id) || member;
    if room.visibility == Visibility::Unlisted && !(code_ok || invite_ok || approved) {
        return Err(ApiError::not_found("no such room"));
    }
    if !member {
        if room.closed_to_joins && !approved {
            return Err(ApiError::forbidden("this game has already started"));
        }
        if room.members.len() as u32 >= room.max_players {
            return Err(ApiError::conflict("the room is full"));
        }
    }
    let allowed = invite_ok
        || approved
        || match room.entry {
            Entry::Open => true,
            Entry::Code => code_ok,
            Entry::Approve => false,
        };
    if !allowed {
        if room.entry == Entry::Code {
            return Err(ApiError::forbidden("wrong join code"));
        }
        // Knock: the host decides.
        let knock = hex::encode(rand::random::<[u8; 8]>());
        state.events.push(
            &principal_account(room.host),
            ServerEvent::Knock { room: room.id.clone(), knock: knock.clone(), account: acc.id, username: acc.username.clone() },
        );
        room.knocks.insert(knock.clone(), Knock { account: acc.id, username: acc.username, key: auth.key.clone(), candidates, at: Instant::now() });
        return Ok(JoinResponse::Knocking { room: room.info(), knock });
    }
    let joined = rooms.add_member(state, room_id, acc.id, acc.username, auth.key.clone(), candidates);
    Ok(JoinResponse::Joined(joined))
}

pub async fn join(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Auth,
    Path(id): Path<String>,
    Json(req): Json<JoinRequest>,
) -> ApiResult<JoinResponse> {
    if req.code.is_some() || req.invite.is_some() {
        let ip = client_ip(&state, &headers, peer);
        state.limiter.check(&format!("guess:{ip}"), state.cfg.limits.code_guesses_per_ip_per_minute, Duration::from_secs(60))?;
    }
    Ok(Json(do_join(&state, &auth, &id, &req)?))
}

pub async fn join_by_code(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Auth,
    Json(req): Json<JoinByCode>,
) -> ApiResult<JoinResponse> {
    let ip = client_ip(&state, &headers, peer);
    state.limiter.check(&format!("guess:{ip}"), state.cfg.limits.code_guesses_per_ip_per_minute, Duration::from_secs(60))?;
    let code = names::normalize_code(&req.code);
    let id = state.rooms.lock().unwrap().by_code.get(&code).cloned().ok_or_else(|| ApiError::not_found("no room with that code"))?;
    let jr = JoinRequest { code: Some(code), invite: None, candidates: req.candidates };
    Ok(Json(do_join(&state, &auth, &id, &jr)?))
}

pub async fn decide_knock(
    State(state): State<AppState>,
    auth: Auth,
    Path((id, knock)): Path<(String, String)>,
    Json(req): Json<KnockDecision>,
) -> ApiResult<serde_json::Value> {
    let acc = auth.account()?.clone();
    let mut rooms = state.rooms.lock().unwrap();
    let room = rooms.by_id.get_mut(&id).ok_or_else(|| ApiError::not_found("no such room"))?;
    if room.host != acc.id {
        return Err(ApiError::forbidden("only the host decides"));
    }
    let k = room.knocks.remove(&knock).ok_or_else(|| ApiError::not_found("no such knock"))?;
    if !req.accept || room.members.len() as u32 >= room.max_players {
        state.events.push(&principal_account(k.account), ServerEvent::KnockResult { room: id.clone(), accepted: false, joined: None });
        return Ok(Json(serde_json::json!({ "accepted": false })));
    }
    room.approved.insert(k.account);
    let joined = rooms.add_member(&state, &id, k.account, k.username, k.key, k.candidates);
    state.events.push(&principal_account(k.account), ServerEvent::KnockResult { room: id.clone(), accepted: true, joined: Some(Box::new(joined)) });
    Ok(Json(serde_json::json!({ "accepted": true })))
}

pub async fn heartbeat(State(state): State<AppState>, auth: Auth, Path(id): Path<String>, Json(req): Json<Heartbeat>) -> ApiResult<RoomInfo> {
    let acc = auth.account()?.clone();
    let mut rooms = state.rooms.lock().unwrap();
    let room = rooms.by_id.get_mut(&id).ok_or_else(|| ApiError::not_found("no such room"))?;
    if room.host != acc.id {
        return Err(ApiError::forbidden("only the host sends heartbeats"));
    }
    room.beat = Instant::now();
    if let Some(m) = req.metadata {
        check_metadata(&state, &m)?;
        room.metadata = m;
    }
    if let Some(c) = req.candidates {
        let c = valid_candidates(&c)?;
        if let Some(h) = room.members.get_mut(&acc.id) {
            h.candidates = c.clone();
        }
        for g in room.members.keys().filter(|a| **a != acc.id) {
            state.events.push(&principal_account(*g), ServerEvent::PeerCandidates { room: id.clone(), account: acc.id, candidates: c.clone() });
        }
    }
    if let Some(c) = req.closed_to_joins {
        room.closed_to_joins = c;
    }
    let gone: Vec<i64> = match &req.connected {
        // A guest that joined moments ago may not have reached the host yet.
        Some(list) => room
            .members
            .values()
            .filter(|m| m.account != acc.id && !list.contains(&m.account) && m.joined.elapsed() > Duration::from_secs(30))
            .map(|m| m.account)
            .collect(),
        None => Vec::new(),
    };
    for a in gone {
        rooms.remove_member(&state, &id, a);
    }
    Ok(Json(rooms.by_id[&id].info()))
}

pub async fn candidates(State(state): State<AppState>, auth: Auth, Path(id): Path<String>, Json(req): Json<Candidates>) -> ApiResult<serde_json::Value> {
    let acc = auth.account()?.clone();
    let c = valid_candidates(&req.candidates)?;
    let mut rooms = state.rooms.lock().unwrap();
    let room = rooms.by_id.get_mut(&id).ok_or_else(|| ApiError::not_found("no such room"))?;
    let m = room.members.get_mut(&acc.id).ok_or_else(|| ApiError::forbidden("not a member"))?;
    m.candidates = c.clone();
    let targets: Vec<i64> = if acc.id == room.host { room.members.keys().copied().filter(|a| *a != acc.id).collect() } else { vec![room.host] };
    for t in targets {
        state.events.push(&principal_account(t), ServerEvent::PeerCandidates { room: id.clone(), account: acc.id, candidates: c.clone() });
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn leave(State(state): State<AppState>, auth: Auth, Path(id): Path<String>) -> ApiResult<serde_json::Value> {
    let acc = auth.account()?.clone();
    let mut rooms = state.rooms.lock().unwrap();
    if !rooms.by_id.get(&id).is_some_and(|r| r.members.contains_key(&acc.id)) {
        return Err(ApiError::not_found("not in that room"));
    }
    rooms.remove_member(&state, &id, acc.id);
    Ok(Json(serde_json::json!({ "left": id })))
}

pub async fn close(State(state): State<AppState>, auth: Auth, Path(id): Path<String>) -> ApiResult<serde_json::Value> {
    let acc = auth.account()?.clone();
    let mut rooms = state.rooms.lock().unwrap();
    let room = rooms.by_id.get(&id).ok_or_else(|| ApiError::not_found("no such room"))?;
    if room.host != acc.id {
        return Err(ApiError::forbidden("only the host closes a room"));
    }
    rooms.close(&state, &id);
    Ok(Json(serde_json::json!({ "closed": id })))
}

pub async fn kick(State(state): State<AppState>, auth: Auth, Path(id): Path<String>, Json(req): Json<Kick>) -> ApiResult<serde_json::Value> {
    let acc = auth.account()?.clone();
    let mut rooms = state.rooms.lock().unwrap();
    let room = rooms.by_id.get_mut(&id).ok_or_else(|| ApiError::not_found("no such room"))?;
    if room.host != acc.id {
        return Err(ApiError::forbidden("only the host kicks"));
    }
    if req.account == acc.id {
        return Err(ApiError::bad("you cannot kick yourself"));
    }
    if req.ban {
        room.banned.insert(req.account);
    }
    room.approved.remove(&req.account);
    rooms.remove_member(&state, &id, req.account);
    state.events.push(&principal_account(req.account), ServerEvent::Kicked { room: id.clone(), banned: req.ban });
    Ok(Json(serde_json::json!({ "kicked": req.account })))
}
