//! Blocking HTTP client for the Warpzone API. Logs in with the device key on
//! first use and again whenever the server forgets the session (restart,
//! expiry). Every method maps to one route in `docs/api.md`.

use crate::identity::Identity;
use crate::{Error, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use warpzone_protocol::api::*;
use warpzone_protocol::descriptor::GameDescriptor;
use warpzone_protocol::mods::{Lockfile, Manifest};

pub struct Api {
    base: String,
    agent: ureq::Agent,
    identity: Arc<Identity>,
    token: Mutex<Option<String>>,
    info: Mutex<Option<ServerInfo>>,
    login: Mutex<Option<LoginResponse>>,
}

fn net(e: ureq::Error) -> Error {
    Error::Network(e.to_string())
}

/// Percent-encode a query value.
pub fn qs(v: &str) -> String {
    let mut o = String::new();
    for b in v.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

impl Api {
    pub fn new(base: &str, identity: Arc<Identity>) -> Api {
        let agent: ureq::Agent = crate::http_config()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(60)))
            .http_status_as_error(false)
            .user_agent(format!("warpzone-core/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Api { base: base.trim_end_matches('/').to_string(), agent, identity, token: Mutex::new(None), info: Mutex::new(None), login: Mutex::new(None) }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}{}", self.base, warpzone_protocol::API_PREFIX, path)
    }

    fn send(&self, method: &str, path: &str, body: Option<(&[u8], &str)>, auth: bool) -> Result<(u16, Vec<u8>)> {
        let url = self.url(path);
        let token = if auth { Some(self.token()?) } else { None };
        let run = |token: &Option<String>| -> Result<(u16, Vec<u8>)> {
            let mut resp = match method {
                "GET" | "DELETE" => {
                    let mut r = if method == "GET" { self.agent.get(&url) } else { self.agent.delete(&url) };
                    if let Some(t) = token {
                        r = r.header("Authorization", format!("Bearer {t}"));
                    }
                    r.call().map_err(net)?
                }
                _ => {
                    let mut r = self.agent.post(&url);
                    if let Some(t) = token {
                        r = r.header("Authorization", format!("Bearer {t}"));
                    }
                    match body {
                        Some((b, ct)) => r.header("Content-Type", ct).send(b).map_err(net)?,
                        None => r.send_empty().map_err(net)?,
                    }
                }
            };
            let status = resp.status().as_u16();
            let bytes = resp.body_mut().with_config().limit(64 * 1024 * 1024).read_to_vec().map_err(net)?;
            Ok((status, bytes))
        };
        let (status, bytes) = run(&token)?;
        if status == 401 && auth {
            // Session gone (server restart): log in again once.
            *self.token.lock().unwrap() = None;
            let token = Some(self.token()?);
            return run(&token);
        }
        Ok((status, bytes))
    }

    fn decode<T: DeserializeOwned>(status: u16, bytes: &[u8]) -> Result<T> {
        if (200..300).contains(&status) {
            serde_json::from_slice(bytes).map_err(|e| Error::Other(format!("bad response: {e}")))
        } else {
            match serde_json::from_slice::<ErrorBody>(bytes) {
                Ok(e) => Err(Error::Api { status, code: e.error, message: e.message }),
                Err(_) => Err(Error::Api { status, code: "http".into(), message: format!("HTTP {status}") }),
            }
        }
    }

    /// Unauthenticated GET.
    pub fn get_public<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let (s, b) = self.send("GET", path, None, false)?;
        Self::decode(s, &b)
    }

    pub fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let (s, b) = self.send("GET", path, None, true)?;
        Self::decode(s, &b)
    }

    pub fn post<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T> {
        let json = serde_json::to_vec(body).expect("serialisable");
        let (s, b) = self.send("POST", path, Some((&json, "application/json")), true)?;
        Self::decode(s, &b)
    }

    fn post_public<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T> {
        let json = serde_json::to_vec(body).expect("serialisable");
        let (s, b) = self.send("POST", path, Some((&json, "application/json")), false)?;
        Self::decode(s, &b)
    }

    pub fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let (s, b) = self.send("DELETE", path, None, true)?;
        Self::decode(s, &b)
    }

    // ------------------------------------------------------------ session

    pub fn info(&self) -> Result<ServerInfo> {
        if let Some(i) = self.info.lock().unwrap().clone() {
            return Ok(i);
        }
        let i: ServerInfo = self.get_public("/info")?;
        *self.info.lock().unwrap() = Some(i.clone());
        Ok(i)
    }

    /// Log in (challenge → signature → token).
    pub fn login(&self) -> Result<LoginResponse> {
        let info = self.info()?;
        let key = self.identity.public_hex();
        let ch: ChallengeResponse = self.post_public("/auth/challenge", &ChallengeRequest { key: key.clone() })?;
        let sig = self.identity.sign(&login_message(&info.server_key, &ch.nonce));
        let resp: LoginResponse = self.post_public("/auth/login", &LoginRequest { key, nonce: ch.nonce, signature: hex::encode(sig) })?;
        *self.token.lock().unwrap() = Some(resp.token.clone());
        *self.login.lock().unwrap() = Some(resp.clone());
        Ok(resp)
    }

    fn token(&self) -> Result<String> {
        if let Some(t) = self.token.lock().unwrap().clone() {
            return Ok(t);
        }
        Ok(self.login()?.token)
    }

    /// The account of this device as of the last login (`None` = no username yet).
    pub fn account(&self) -> Option<Account> {
        self.login.lock().unwrap().as_ref().and_then(|l| l.account.clone())
    }

    pub fn roles(&self) -> Vec<String> {
        self.login.lock().unwrap().as_ref().map(|l| l.roles.clone()).unwrap_or_default()
    }

    pub fn register(&self, username: &str) -> Result<Account> {
        let a: Account = self.post("/accounts", &RegisterRequest { username: username.into() })?;
        if let Some(l) = self.login.lock().unwrap().as_mut() {
            l.account = Some(a.clone());
        }
        Ok(a)
    }

    pub fn link_code(&self) -> Result<LinkCodeResponse> {
        self.post("/accounts/me/link-code", &serde_json::json!({}))
    }

    pub fn link(&self, code: &str) -> Result<Account> {
        let a: Account = self.post("/accounts/link", &LinkRequest { code: code.into() })?;
        if let Some(l) = self.login.lock().unwrap().as_mut() {
            l.account = Some(a.clone());
        }
        Ok(a)
    }

    pub fn delete_account(&self) -> Result<serde_json::Value> {
        self.delete("/accounts/me")
    }

    pub fn profile(&self, username: &str) -> Result<Profile> {
        self.get_public(&format!("/accounts/{}", qs(username)))
    }

    // ------------------------------------------------------------ catalog

    pub fn games(&self) -> Result<Vec<GameDescriptor>> {
        self.get_public("/games")
    }

    pub fn game(&self, id: &str) -> Result<GameDescriptor> {
        self.get_public(&format!("/games/{}", qs(id)))
    }

    /// `id = "launcher"` for the launcher itself.
    pub fn release(&self, id: &str) -> Result<ReleaseInfo> {
        self.get_public(&format!("/games/{}/release", qs(id)))
    }

    pub fn changelog(&self, id: &str) -> Result<Vec<ChangelogEntry>> {
        self.get_public(&format!("/games/{}/changelog", qs(id)))
    }

    /// What changed since `current`, and whether an update exists.
    pub fn updates(&self, id: &str, current: &str) -> Result<UpdateInfo> {
        self.get_public(&format!("/games/{}/updates?current={}", qs(id), qs(current)))
    }

    // ------------------------------------------------------------ rooms

    pub fn rooms(&self, game: Option<&str>, has_space: bool) -> Result<Vec<RoomInfo>> {
        let mut q = String::from("/rooms?");
        if let Some(g) = game {
            q.push_str(&format!("game={}&", qs(g)));
        }
        if has_space {
            q.push_str("has_space=true");
        }
        self.get_public(&q)
    }

    pub fn room(&self, id: &str, secret: Option<&str>) -> Result<RoomInfo> {
        match secret {
            Some(s) => self.get(&format!("/rooms/{}?s={}", qs(id), qs(s))),
            None => self.get(&format!("/rooms/{}", qs(id))),
        }
    }

    pub fn create_room(&self, req: &CreateRoom) -> Result<CreatedRoom> {
        self.post("/rooms", req)
    }

    pub fn join_room(&self, id: &str, req: &JoinRequest) -> Result<JoinResponse> {
        self.post(&format!("/rooms/{}/join", qs(id)), req)
    }

    pub fn join_code(&self, code: &str, candidates: Vec<String>) -> Result<JoinResponse> {
        self.post("/join", &JoinByCode { code: code.into(), candidates })
    }

    pub fn decide_knock(&self, room: &str, knock: &str, accept: bool) -> Result<serde_json::Value> {
        self.post(&format!("/rooms/{}/knocks/{}", qs(room), qs(knock)), &KnockDecision { accept })
    }

    pub fn heartbeat(&self, room: &str, hb: &Heartbeat) -> Result<RoomInfo> {
        self.post(&format!("/rooms/{}/heartbeat", qs(room)), hb)
    }

    pub fn update_candidates(&self, room: &str, candidates: Vec<String>) -> Result<serde_json::Value> {
        self.post(&format!("/rooms/{}/candidates", qs(room)), &Candidates { candidates })
    }

    pub fn leave_room(&self, room: &str) -> Result<serde_json::Value> {
        self.post(&format!("/rooms/{}/leave", qs(room)), &serde_json::json!({}))
    }

    pub fn close_room(&self, room: &str) -> Result<serde_json::Value> {
        self.delete(&format!("/rooms/{}", qs(room)))
    }

    pub fn kick(&self, room: &str, account: i64, ban: bool) -> Result<serde_json::Value> {
        self.post(&format!("/rooms/{}/kick", qs(room)), &Kick { account, ban })
    }

    /// Long poll; returns after an event or `timeout_s` seconds.
    pub fn events(&self, after: u64, timeout_s: u64) -> Result<EventsResponse> {
        self.get(&format!("/events?after={after}&timeout={timeout_s}"))
    }

    // ------------------------------------------------------------ ladder & boards

    pub fn queue_join(&self, req: &QueueJoin) -> Result<QueueStatus> {
        self.post("/ladder/queue", req)
    }

    pub fn queue_status(&self) -> Result<QueueStatus> {
        self.get("/ladder/queue")
    }

    pub fn queue_leave(&self) -> Result<QueueStatus> {
        self.delete("/ladder/queue")
    }

    pub fn report_match(&self, id: &str, r: &MatchReport) -> Result<MatchInfo> {
        self.post(&format!("/matches/{}/report", qs(id)), r)
    }

    pub fn match_info(&self, id: &str) -> Result<MatchInfo> {
        self.get_public(&format!("/matches/{}", qs(id)))
    }

    pub fn standings(&self, game: &str, queue: &str, season: Option<&str>) -> Result<StandingsResponse> {
        let s = season.map(|s| format!("?season={}", qs(s))).unwrap_or_default();
        self.get_public(&format!("/games/{}/ladder/{}{s}", qs(game), qs(queue)))
    }

    pub fn submit_score(&self, game: &str, board: &str, s: &ScoreSubmit) -> Result<ScoreSubmitted> {
        self.post(&format!("/games/{}/boards/{}/scores", qs(game), qs(board)), s)
    }

    pub fn board(&self, game: &str, board: &str, ruleset: &str, season: &str) -> Result<BoardResponse> {
        self.get_public(&format!("/games/{}/boards/{}?ruleset={}&season={}", qs(game), qs(board), qs(ruleset), qs(season)))
    }

    // ------------------------------------------------------------ mods

    pub fn mods(&self, game: &str, category: Option<&str>, query: Option<&str>) -> Result<Vec<ModSummary>> {
        let mut q = format!("/games/{}/mods?", qs(game));
        if let Some(c) = category {
            q.push_str(&format!("category={}&", qs(c)));
        }
        if let Some(s) = query {
            q.push_str(&format!("q={}", qs(s)));
        }
        self.get_public(&q)
    }

    /// Mod details; authenticated, so authors and staff also see pending versions.
    pub fn mod_detail(&self, game: &str, name: &str) -> Result<ModDetail> {
        self.get(&format!("/games/{}/mods/{}", qs(game), qs(name)))
    }

    pub fn resolve(&self, game: &str, game_version: &str, mods: Vec<String>) -> Result<Lockfile> {
        self.post_public(&format!("/games/{}/mods/resolve", qs(game)), &ResolveRequest { game_version: game_version.into(), mods })
    }

    pub fn lockfile(&self, game: &str, hash: &str) -> Result<Lockfile> {
        self.get_public(&format!("/games/{}/lockfiles/{}", qs(game), qs(hash)))
    }

    pub fn put_lockfile(&self, lock: &Lockfile) -> Result<Lockfile> {
        self.post(&format!("/games/{}/lockfiles", qs(&lock.game)), lock)
    }

    /// Upload a mod version (goes to the moderation queue).
    pub fn upload_mod(&self, game: &str, manifest: &Manifest, archive: &Path) -> Result<Uploaded> {
        let data = std::fs::read(archive)?;
        let boundary = format!("warpzone{}", hex::encode(rand::random::<[u8; 12]>()));
        let filename: String =
            archive.file_name().and_then(|f| f.to_str()).unwrap_or("mod.zip").chars().filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).collect();
        let mut body = Vec::with_capacity(data.len() + 1024);
        write!(body, "--{boundary}\r\nContent-Disposition: form-data; name=\"manifest\"\r\nContent-Type: application/json\r\n\r\n")?;
        body.extend_from_slice(&serde_json::to_vec(manifest).unwrap());
        write!(
            body,
            "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"archive\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )?;
        body.extend_from_slice(&data);
        write!(body, "\r\n--{boundary}--\r\n")?;
        let ct = format!("multipart/form-data; boundary={boundary}");
        let (s, b) = self.send("POST", &format!("/games/{}/mods", qs(game)), Some((&body, &ct)), true)?;
        Self::decode(s, &b)
    }

    /// Download a mod archive to `dest`, verifying its SHA-256. Returns its size.
    pub fn download_mod(&self, game: &str, name: &str, version: &str, sha256: &str, dest: &Path) -> Result<u64> {
        let url = self.url(&format!("/games/{}/mods/{}/{}/download", qs(game), qs(name), qs(version)));
        let mut req = self.agent.get(&url);
        if let Some(t) = self.token.lock().unwrap().clone() {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        let mut resp = req.call().map_err(net)?;
        let status = resp.status().as_u16();
        if status != 200 {
            let b = resp.body_mut().read_to_vec().unwrap_or_default();
            return Self::decode::<serde_json::Value>(status, &b).map(|_| 0);
        }
        download_verified(resp.body_mut().with_config().limit(u64::MAX).reader(), sha256, dest)
    }

    pub fn report(&self, r: &ReportRequest) -> Result<serde_json::Value> {
        self.post("/reports", r)
    }

    /// Needs no account: a crashed game may never have logged in.
    pub fn feedback(&self, r: &FeedbackRequest) -> Result<FeedbackResponse> {
        self.post_public("/feedback", r)
    }

    pub fn feedback_list(&self, game: Option<&str>) -> Result<Vec<FeedbackEntry>> {
        self.get(&game.map(|g| format!("/moderation/feedback?game={}", qs(g))).unwrap_or_else(|| "/moderation/feedback".into()))
    }

    // ------------------------------------------------------------ moderation

    pub fn moderation_queue(&self) -> Result<ModerationQueue> {
        self.get("/moderation/queue")
    }

    pub fn decide(&self, game: &str, name: &str, version: &str, decision: &str, reason: Option<&str>) -> Result<serde_json::Value> {
        self.post(
            &format!("/moderation/mods/{}/{}/{}", qs(game), qs(name), qs(version)),
            &Decision { decision: decision.into(), reason: reason.map(String::from) },
        )
    }

    pub fn note(&self, game: &str, name: &str, version: &str, text: &str) -> Result<serde_json::Value> {
        self.post("/moderation/notes", &NoteRequest { game: game.into(), name: name.into(), version: version.into(), text: text.into() })
    }

    pub fn close_report(&self, id: i64) -> Result<serde_json::Value> {
        self.post(&format!("/moderation/reports/{id}/close"), &serde_json::json!({}))
    }

    // ------------------------------------------------------------ plain downloads

    /// GET any URL (release assets, checksum files) into memory, up to `limit` bytes.
    pub fn fetch(&self, url: &str, limit: u64) -> Result<Vec<u8>> {
        let mut resp = self.agent.get(url).call().map_err(net)?;
        if resp.status().as_u16() != 200 {
            return Err(Error::Network(format!("{url}: HTTP {}", resp.status().as_u16())));
        }
        resp.body_mut().with_config().limit(limit).read_to_vec().map_err(net)
    }

    /// GET any URL into `dest`, verifying `sha256` if given; returns the hash.
    pub fn fetch_to(&self, url: &str, sha256: Option<&str>, dest: &Path, progress: &mut dyn FnMut(u64)) -> Result<String> {
        let mut resp = self.agent.get(url).call().map_err(net)?;
        if resp.status().as_u16() != 200 {
            return Err(Error::Network(format!("{url}: HTTP {}", resp.status().as_u16())));
        }
        let mut r = resp.body_mut().with_config().limit(u64::MAX).reader();
        let (hash, _) = copy_hashing(&mut r, dest, progress)?;
        if let Some(want) = sha256
            && !hash.eq_ignore_ascii_case(want)
        {
            let _ = std::fs::remove_file(dest);
            return Err(Error::Other(format!("checksum mismatch for {url}")));
        }
        Ok(hash)
    }
}

impl Api {
    /// GET `url` and hand the body to `f` as a stream (no temporary file).
    pub fn stream_url<T>(&self, url: &str, f: &mut dyn FnMut(&mut dyn Read) -> Result<T>) -> Result<T> {
        let mut resp = self.agent.get(url).call().map_err(net)?;
        if resp.status().as_u16() != 200 {
            return Err(Error::Network(format!("{url}: HTTP {}", resp.status().as_u16())));
        }
        let mut r = resp.body_mut().with_config().limit(u64::MAX).reader();
        f(&mut r)
    }
}

fn copy_hashing(r: &mut dyn Read, dest: &Path, progress: &mut dyn FnMut(u64)) -> Result<(String, u64)> {
    use sha2::{Digest, Sha256};
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut f = std::fs::File::create(dest)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        f.write_all(&buf[..n])?;
        total += n as u64;
        progress(total);
    }
    f.sync_all()?;
    Ok((hex::encode(h.finalize()), total))
}

fn download_verified(mut r: impl Read, sha256: &str, dest: &Path) -> Result<u64> {
    let (hash, size) = copy_hashing(&mut r, dest, &mut |_| {})?;
    if !hash.eq_ignore_ascii_case(sha256) {
        let _ = std::fs::remove_file(dest);
        return Err(Error::Other(format!("checksum mismatch for {}", dest.display())));
    }
    Ok(size)
}
