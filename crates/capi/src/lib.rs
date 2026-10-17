//! C ABI over `warpzone-core`. The contract is `include/warpzone.h`; keep the
//! two in step (the C example in `examples/c` is compiled against both in CI).

#![allow(clippy::missing_safety_doc)]

use serde_json::{Value, json};
use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_int};
use std::sync::Arc;
use warpzone_core::client::{Client, Config, Event, HostOptions};
use warpzone_core::identity::Identity;
use warpzone_protocol::api::{MatchReport, ScoreSubmit};

pub const ABI: u32 = 1;

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_error(m: impl Into<String>) {
    let s = CString::new(m.into().replace('\0', " ")).unwrap_or_default();
    LAST_ERROR.with(|e| *e.borrow_mut() = s);
}

pub struct WzClient {
    inner: Client,
    /// Keeps the current event's data and JSON alive for the caller.
    json: CString,
    data: Vec<u8>,
}

unsafe fn s<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() { None } else { unsafe { CStr::from_ptr(p) }.to_str().ok() }
}

fn copy_out(text: &str, buf: *mut c_char, cap: usize) -> usize {
    if buf.is_null() || cap == 0 {
        return text.len();
    }
    let n = text.len().min(cap - 1);
    unsafe {
        std::ptr::copy_nonoverlapping(text.as_ptr(), buf as *mut u8, n);
        *buf.add(n) = 0;
    }
    text.len()
}

#[unsafe(no_mangle)]
pub extern "C" fn wz_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

#[unsafe(no_mangle)]
pub extern "C" fn wz_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

#[repr(C)]
pub struct WzConfig {
    pub abi: u32,
    pub server: *const c_char,
    pub game: *const c_char,
    pub game_version: *const c_char,
    pub home: *const c_char,
    pub auto_join_matches: c_int,
}

#[repr(C)]
pub struct WzEvent {
    pub kind: c_int,
    pub peer: u32,
    pub channel: u8,
    pub reliable: u8,
    pub data: *const u8,
    pub len: usize,
    pub json: *const c_char,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_create(cfg: *const WzConfig) -> *mut WzClient {
    let Some(cfg) = (unsafe { cfg.as_ref() }) else {
        set_error("config is NULL");
        return std::ptr::null_mut();
    };
    if cfg.abi != ABI {
        set_error(format!("ABI mismatch: header {} vs library {ABI}", cfg.abi));
        return std::ptr::null_mut();
    }
    let (Some(game), Some(version)) = (unsafe { s(cfg.game) }, unsafe { s(cfg.game_version) }) else {
        set_error("game and game_version are required");
        return std::ptr::null_mut();
    };
    let mut c = Config::new(game, version);
    if let Some(server) = unsafe { s(cfg.server) } {
        c.server = server.into();
    }
    if let Some(home) = unsafe { s(cfg.home) } {
        match Identity::load_or_create(&std::path::Path::new(home).join("identity.key")) {
            Ok(i) => c.identity = Some(Arc::new(i)),
            Err(e) => {
                set_error(e.to_string());
                return std::ptr::null_mut();
            }
        }
        // SAFETY: games call wz_create before starting their own threads.
        unsafe { std::env::set_var("WARPZONE_HOME", home) };
    }
    c.auto_join_matches = cfg.auto_join_matches != 0;
    match Client::new(c) {
        Ok(inner) => Box::into_raw(Box::new(WzClient { inner, json: CString::default(), data: Vec::new() })),
        Err(e) => {
            set_error(e.to_string());
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_destroy(c: *mut WzClient) {
    if !c.is_null() {
        drop(unsafe { Box::from_raw(c) });
    }
}

macro_rules! client {
    ($c:expr) => {
        match unsafe { $c.as_mut() } {
            Some(c) => c,
            None => return Default::default(),
        }
    };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_poll(c: *mut WzClient) {
    let c = client!(c);
    c.inner.poll();
}

fn event_parts(e: Event) -> (c_int, u32, u8, u8, Vec<u8>, Value) {
    use Event::*;
    match e {
        LoggedIn { account, roles } => (1, 0, 0, 0, vec![], json!({ "account": account, "roles": roles })),
        Registered(a) => (2, 0, 0, 0, vec![], json!(a)),
        RoomHosted { room, code, invite } => (3, 0, 0, 0, vec![], json!({ "room": room, "code": code, "invite": invite })),
        RoomJoined { room, host } => (4, 0, 0, 0, vec![], json!({ "room": room, "host": host })),
        Knocking { room } => (5, 0, 0, 0, vec![], json!({ "room": room })),
        KnockDenied { room } => (6, 0, 0, 0, vec![], json!({ "room": room })),
        Knock { knock, account, username } => (7, 0, 0, 0, vec![], json!({ "knock": knock, "account": account, "username": username })),
        PeerJoined { peer, account, username } => (8, peer, 0, 0, vec![], json!({ "account": account, "username": username })),
        PeerConnected { peer, relayed } => (9, peer, 0, 0, vec![], json!({ "relayed": relayed })),
        PeerPathChanged { peer, relayed } => (10, peer, 0, 0, vec![], json!({ "relayed": relayed })),
        PeerData { peer, channel, data, reliable } => (11, peer, channel, reliable as u8, data, json!({})),
        PeerLeft { peer, account } => (12, peer, 0, 0, vec![], json!({ "account": account })),
        RoomClosed { room } => (13, 0, 0, 0, vec![], json!({ "room": room })),
        Kicked { room, banned } => (14, 0, 0, 0, vec![], json!({ "room": room, "banned": banned })),
        RoomList(r) => (15, 0, 0, 0, vec![], json!(r)),
        Queued(q) => (16, 0, 0, 0, vec![], json!(q)),
        MatchFound { match_id, queue, room, host, teams } => {
            (17, 0, 0, 0, vec![], json!({ "match_id": match_id, "queue": queue, "room": room, "host": host, "teams": teams }))
        }
        MatchFinished { match_id, state } => (18, 0, 0, 0, vec![], json!({ "match_id": match_id, "state": state })),
        MatchReported(m) => (19, 0, 0, 0, vec![], json!(m)),
        ScoreSubmitted { board, result } => (20, 0, 0, 0, vec![], json!({ "board": board, "result": result })),
        Standings(s) => (21, 0, 0, 0, vec![], json!(s)),
        Board(b) => (22, 0, 0, 0, vec![], json!(b)),
        Update(u) => (23, 0, 0, 0, vec![], json!(u)),
        ModsProgress { done, total } => (24, 0, 0, 0, vec![], json!({ "done": done, "total": total })),
        ModsSynced { lockfile, paths } => (25, 0, 0, 0, vec![], json!({ "lockfile": lockfile, "paths": paths })),
        Error { op, code, message } => (26, 0, 0, 0, vec![], json!({ "op": op, "code": code, "message": message })),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_next_event(c: *mut WzClient, out: *mut WzEvent) -> c_int {
    let c = client!(c);
    let Some(out) = (unsafe { out.as_mut() }) else { return 0 };
    let Some(e) = c.inner.next_event() else { return 0 };
    let (kind, peer, channel, reliable, data, j) = event_parts(e);
    c.data = data;
    c.json = CString::new(j.to_string()).unwrap_or_default();
    *out = WzEvent {
        kind,
        peer,
        channel,
        reliable,
        data: if c.data.is_empty() { std::ptr::null() } else { c.data.as_ptr() },
        len: c.data.len(),
        json: c.json.as_ptr(),
    };
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_username(c: *mut WzClient, buf: *mut c_char, cap: usize) -> usize {
    let c = client!(c);
    match c.inner.account() {
        Some(a) => copy_out(&a.username, buf, cap),
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_register(c: *mut WzClient, username: *const c_char) {
    let c = client!(c);
    if let Some(u) = unsafe { s(username) } {
        c.inner.register(u);
    }
}

fn host_options(v: &Value) -> Result<HostOptions, String> {
    let mut o = HostOptions { max_players: v["max_players"].as_u64().unwrap_or(2) as u32, ..Default::default() };
    if let Some(x) = v.get("visibility") {
        o.visibility = serde_json::from_value(x.clone()).map_err(|e| format!("visibility: {e}"))?;
    }
    if let Some(x) = v.get("entry") {
        o.entry = serde_json::from_value(x.clone()).map_err(|e| format!("entry: {e}"))?;
    }
    if let Some(x) = v.get("audience") {
        o.audience = serde_json::from_value(x.clone()).map_err(|e| format!("audience: {e}"))?;
    }
    if let Some(x) = v.get("metadata") {
        o.metadata = serde_json::from_value(x.clone()).map_err(|e| format!("metadata: {e}"))?;
    }
    o.preset = v["preset"].as_str().map(String::from);
    o.lockfile = v["lockfile"].as_str().map(String::from);
    o.staged = v["staged"].as_bool().unwrap_or(false);
    Ok(o)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_host(c: *mut WzClient, options_json: *const c_char) {
    let c = client!(c);
    let v: Value = unsafe { s(options_json) }.and_then(|t| serde_json::from_str(t).ok()).unwrap_or(json!({}));
    match host_options(&v) {
        Ok(o) => c.inner.host(o),
        Err(e) => set_error(e),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_join(c: *mut WzClient, room: *const c_char, code: *const c_char, secret: *const c_char) {
    let c = client!(c);
    if let Some(r) = unsafe { s(room) } {
        c.inner.join(r, unsafe { s(code) }, unsafe { s(secret) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_join_code(c: *mut WzClient, code: *const c_char) {
    let c = client!(c);
    if let Some(code) = unsafe { s(code) } {
        c.inner.join_code(code);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_handle_launch(c: *mut WzClient, argc: c_int, argv: *const *const c_char) -> c_int {
    let c = client!(c);
    let mut args = Vec::new();
    for i in 0..argc.max(0) as usize {
        if let Some(a) = unsafe { s(*argv.add(i)) } {
            args.push(a.to_string());
        }
    }
    let intent = warpzone_core::launch::from_args(&args);
    if let Some(room) = intent.join {
        c.inner.join(&room, None, intent.secret.as_deref());
        return 1;
    }
    if intent.host { 2 } else { 0 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_answer_knock(c: *mut WzClient, knock: *const c_char, accept: c_int) {
    let c = client!(c);
    if let Some(k) = unsafe { s(knock) } {
        c.inner.answer_knock(k, accept != 0);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_set_metadata(c: *mut WzClient, json_object: *const c_char) {
    let c = client!(c);
    match unsafe { s(json_object) }.map(serde_json::from_str) {
        Some(Ok(m)) => c.inner.set_metadata(m),
        _ => set_error("metadata must be a JSON object of strings"),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_set_closed(c: *mut WzClient, closed: c_int) {
    let c = client!(c);
    c.inner.set_closed(closed != 0);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_kick(c: *mut WzClient, account: i64, ban: c_int) {
    let c = client!(c);
    c.inner.kick(account, ban != 0);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_leave(c: *mut WzClient) {
    let c = client!(c);
    c.inner.leave();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_list_rooms(c: *mut WzClient, only_with_space: c_int) {
    let c = client!(c);
    c.inner.list_rooms(only_with_space != 0);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_is_host(c: *mut WzClient) -> c_int {
    let c = client!(c);
    c.inner.is_host() as c_int
}

unsafe fn bytes<'a>(data: *const std::ffi::c_void, len: usize) -> &'a [u8] {
    if data.is_null() || len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(data as *const u8, len) } }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_send(c: *mut WzClient, peer: u32, channel: u8, data: *const std::ffi::c_void, len: usize, reliable: c_int) -> c_int {
    let c = client!(c);
    c.inner.send(peer, channel, unsafe { bytes(data, len) }, reliable != 0) as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_broadcast(c: *mut WzClient, channel: u8, data: *const std::ffi::c_void, len: usize, reliable: c_int) -> c_int {
    let c = client!(c);
    c.inner.broadcast(channel, unsafe { bytes(data, len) }, reliable != 0) as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_peer_name(c: *mut WzClient, peer: u32, buf: *mut c_char, cap: usize) -> usize {
    let c = client!(c);
    c.inner.peer_name(peer).map(|n| copy_out(&n, buf, cap)).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_peer_account(c: *mut WzClient, peer: u32) -> i64 {
    match unsafe { c.as_mut() } {
        Some(c) => c.inner.peer_account(peer).unwrap_or(-1),
        None => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_peer_rtt_ms(c: *mut WzClient, peer: u32) -> c_int {
    match unsafe { c.as_mut() } {
        Some(c) => c.inner.peer_rtt(peer).map(|d| d.as_millis() as c_int).unwrap_or(-1),
        None => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_peer_relayed(c: *mut WzClient, peer: u32) -> c_int {
    match unsafe { c.as_mut() } {
        Some(c) => c.inner.peer_relayed(peer).map(|r| r as c_int).unwrap_or(-1),
        None => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_queue(c: *mut WzClient, queue: *const c_char, ruleset: *const c_char) {
    let c = client!(c);
    if let Some(q) = unsafe { s(queue) } {
        c.inner.queue(q, unsafe { s(ruleset) }.unwrap_or("stock"));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_leave_queue(c: *mut WzClient) {
    let c = client!(c);
    c.inner.leave_queue();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_report_match(c: *mut WzClient, match_id: *const c_char, report_json: *const c_char) {
    let c = client!(c);
    let (Some(id), Some(r)) = (unsafe { s(match_id) }, unsafe { s(report_json) }) else { return };
    match serde_json::from_str::<MatchReport>(r) {
        Ok(r) => c.inner.report_match(id, r),
        Err(e) => set_error(format!("report: {e}")),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_submit_score(c: *mut WzClient, board: *const c_char, value: i64, ruleset: *const c_char, replay_hash: *const c_char) {
    let c = client!(c);
    if let Some(b) = unsafe { s(board) } {
        c.inner.submit_score(
            b,
            ScoreSubmit {
                value,
                ruleset: unsafe { s(ruleset) }.unwrap_or("stock").into(),
                replay_hash: unsafe { s(replay_hash) }.map(String::from),
                replay: None,
            },
        );
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_request_standings(c: *mut WzClient, queue: *const c_char, season: *const c_char) {
    let c = client!(c);
    if let Some(q) = unsafe { s(queue) } {
        c.inner.request_standings(q, unsafe { s(season) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_request_board(c: *mut WzClient, board: *const c_char, ruleset: *const c_char, season: *const c_char) {
    let c = client!(c);
    if let Some(b) = unsafe { s(board) } {
        c.inner.request_board(b, unsafe { s(ruleset) }.unwrap_or("stock"), unsafe { s(season) }.unwrap_or("all"));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_check_update(c: *mut WzClient) {
    let c = client!(c);
    c.inner.check_update();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_sync_mods(c: *mut WzClient, lockfile_hash: *const c_char) {
    let c = client!(c);
    if let Some(h) = unsafe { s(lockfile_hash) } {
        c.inner.sync_mods(h);
    }
}

fn client_name() -> String {
    warpzone_core::feedback::client_name("libwarpzone", env!("CARGO_PKG_VERSION"))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_report(c: *mut WzClient, kind: *const c_char, title: *const c_char, detail: *const c_char) {
    let c = client!(c);
    let (Some(kind), Some(title)) = (unsafe { s(kind) }, unsafe { s(title) }) else { return };
    let req = warpzone_core::feedback::request(c.inner.game(), kind, title, unsafe { s(detail) }.unwrap_or(""), Some(c.inner.game_version()), &client_name());
    warpzone_core::feedback::send_in_background(c.inner.api(), req);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wz_report_problem(
    server: *const c_char,
    game: *const c_char,
    game_version: *const c_char,
    kind: *const c_char,
    title: *const c_char,
    detail: *const c_char,
) -> c_int {
    let (Some(game), Some(kind), Some(title)) = (unsafe { s(game) }, unsafe { s(kind) }, unsafe { s(title) }) else {
        set_error("game, kind and title are required");
        return -1;
    };
    let server = unsafe { s(server) }.map(String::from).unwrap_or_else(warpzone_core::server_url);
    // No login needed: a throwaway identity is enough.
    let api = warpzone_core::api::Api::new(&server, Arc::new(Identity::generate()));
    let req = warpzone_core::feedback::request(game, kind, title, unsafe { s(detail) }.unwrap_or(""), unsafe { s(game_version) }, &client_name());
    match warpzone_core::feedback::send(&api, &req) {
        Ok(_) => 0,
        Err(e) => {
            set_error(e.to_string());
            -1
        }
    }
}
