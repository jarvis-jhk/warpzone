//! Shared server state, the rate limiter and the event hub.

use crate::config::{Config, Staff};
use crate::error::ApiError;
use crate::relay::Relay;
use crate::rooms::Rooms;
use ed25519_dalek::SigningKey;
use rusqlite::Connection;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::watch;
use warpzone_protocol::api::{EventEnvelope, EventsResponse, ServerEvent};
use warpzone_protocol::descriptor::{GameDescriptor, glob_match};

pub type AppState = Arc<State>;

pub struct State {
    pub cfg: Config,
    db: Mutex<Connection>,
    pub server_key: SigningKey,
    pub server_key_hex: String,
    pub games: RwLock<Games>,
    pub staff: RwLock<StaffFile>,
    pub sessions: Mutex<HashMap<String, Session>>,
    pub challenges: Mutex<HashMap<String, (String, Instant)>>,
    pub link_codes: Mutex<HashMap<String, (i64, Instant)>>,
    pub rooms: Mutex<Rooms>,
    pub events: EventHub,
    pub relay: Arc<Relay>,
    pub limiter: Limiter,
    pub releases: RwLock<BTreeMap<String, warpzone_protocol::api::ReleaseInfo>>,
    pub changelogs: RwLock<BTreeMap<String, Vec<warpzone_protocol::api::ChangelogEntry>>>,
    pub matchmaker: Mutex<crate::ladder::Matchmaker>,
    pub started: Instant,
}

/// A logged-in device.
#[derive(Clone, Debug)]
pub struct Session {
    pub key: String,
    pub expires: Instant,
}

pub struct Games {
    pub by_id: BTreeMap<String, GameDescriptor>,
    stamp: Option<SystemTime>,
}

pub struct StaffFile {
    pub staff: Staff,
    stamp: Option<SystemTime>,
}

impl State {
    pub fn new(cfg: Config, db: Connection, server_key: SigningKey) -> State {
        let relay = Arc::new(Relay::new(cfg.limits.relay_kbps, cfg.limits.relay_total_mbps));
        let state = State {
            server_key_hex: hex::encode(server_key.verifying_key().to_bytes()),
            server_key,
            db: Mutex::new(db),
            games: RwLock::new(Games { by_id: BTreeMap::new(), stamp: None }),
            staff: RwLock::new(StaffFile { staff: Staff::default(), stamp: None }),
            sessions: Mutex::new(HashMap::new()),
            challenges: Mutex::new(HashMap::new()),
            link_codes: Mutex::new(HashMap::new()),
            rooms: Mutex::new(Rooms::default()),
            events: EventHub::default(),
            relay,
            limiter: Limiter::default(),
            releases: RwLock::new(BTreeMap::new()),
            changelogs: RwLock::new(BTreeMap::new()),
            matchmaker: Mutex::new(crate::ladder::Matchmaker::default()),
            started: Instant::now(),
            cfg,
        };
        state.reload_games();
        state.reload_staff();
        state
    }

    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Re-read `games_dir` if any file in it changed.
    pub fn reload_games(&self) {
        let stamp = dir_stamp(&self.cfg.games_dir);
        if self.games.read().unwrap().stamp == stamp && stamp.is_some() {
            return;
        }
        let mut by_id = BTreeMap::new();
        if let Ok(rd) = std::fs::read_dir(&self.cfg.games_dir) {
            let mut paths: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
            paths.sort();
            for p in paths {
                let parsed = std::fs::read_to_string(&p)
                    .map_err(|e| e.to_string())
                    .and_then(|t| toml::from_str::<GameDescriptor>(&t).map_err(|e| e.to_string()))
                    .and_then(|d| d.validate().map(|_| d).map_err(|e| e.to_string()));
                match parsed {
                    Ok(d) if p.file_stem().is_some_and(|s| s == d.id.as_str()) => {
                        by_id.insert(d.id.clone(), d);
                    }
                    Ok(d) => log::warn!("{}: id {} does not match the file name, skipped", p.display(), d.id),
                    Err(e) => log::warn!("{}: {e}, skipped", p.display()),
                }
            }
        }
        log::info!("{} game descriptors loaded", by_id.len());
        *self.games.write().unwrap() = Games { by_id, stamp };
    }

    pub fn reload_staff(&self) {
        let stamp = std::fs::metadata(&self.cfg.staff_file).and_then(|m| m.modified()).ok();
        if self.staff.read().unwrap().stamp == stamp && stamp.is_some() {
            return;
        }
        let staff = match std::fs::read_to_string(&self.cfg.staff_file) {
            Ok(t) => toml::from_str(&t).unwrap_or_else(|e| {
                log::error!("{}: {e}; no staff until fixed", self.cfg.staff_file.display());
                Staff::default()
            }),
            Err(_) => Staff::default(),
        };
        log::info!("staff: {} moderator(s), {} reviewer(s)", staff.moderators.len(), staff.reviewers.len());
        *self.staff.write().unwrap() = StaffFile { staff, stamp };
    }

    pub fn game(&self, id: &str) -> Result<GameDescriptor, ApiError> {
        self.games.read().unwrap().by_id.get(id).cloned().ok_or_else(|| ApiError::not_found(format!("no game {id}")))
    }

    pub fn roles(&self, key: &str) -> Vec<String> {
        let s = &self.staff.read().unwrap().staff;
        let mut r = Vec::new();
        if s.moderators.iter().any(|k| k == key) {
            r.push("moderator".to_string());
        }
        if s.reviewers.iter().any(|k| k == key) {
            r.push("reviewer".to_string());
        }
        r
    }

    /// Event principals of all staff (moderators and reviewers).
    pub fn staff_principals(&self) -> Vec<String> {
        let keys: Vec<String> = {
            let s = &self.staff.read().unwrap().staff;
            s.moderators.iter().chain(s.reviewers.iter()).cloned().collect()
        };
        let db = self.db();
        keys.iter()
            .map(|k| {
                db.query_row("SELECT account FROM keys WHERE key = ?1", [k], |r| r.get::<_, i64>(0)).map(principal_account).unwrap_or_else(|_| principal_key(k))
            })
            .collect()
    }

    /// Fire-and-forget plain-text notification to `notify_url`.
    pub fn notify(&self, text: String) {
        let Some(url) = self.cfg.notify_url.clone() else { return };
        tokio::task::spawn_blocking(move || {
            if let Err(e) = ureq::Agent::new_with_config(crate::http::http_config().build()).post(&url).header("Title", "Warpzone").send(text.as_bytes()) {
                log::warn!("notify failed: {e}");
            }
        });
    }

    /// Whether `name` (an asset in a release) is what `pattern` asks for.
    pub fn asset_matches(pattern: &str, name: &str) -> bool {
        glob_match(pattern, name)
    }
}

fn dir_stamp(dir: &Path) -> Option<SystemTime> {
    let mut newest = std::fs::metadata(dir).and_then(|m| m.modified()).ok()?;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        if let Ok(t) = e.metadata().and_then(|m| m.modified()) {
            newest = newest.max(t);
        }
    }
    Some(newest)
}

pub fn principal_account(id: i64) -> String {
    format!("a:{id}")
}

pub fn principal_key(key: &str) -> String {
    format!("k:{key}")
}

// ---------------------------------------------------------------- rate limits

/// Sliding-window counters keyed by strings like `login:203.0.113.7`.
#[derive(Default)]
pub struct Limiter {
    hits: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl Limiter {
    /// Record a hit; `false` if `key` already had `max` hits within `window`.
    pub fn allow(&self, key: &str, max: u32, window: Duration) -> bool {
        let now = Instant::now();
        let mut map = self.hits.lock().unwrap();
        let q = map.entry(key.to_string()).or_default();
        while q.front().is_some_and(|t| now.duration_since(*t) > window) {
            q.pop_front();
        }
        if q.len() >= max as usize {
            return false;
        }
        q.push_back(now);
        true
    }

    pub fn check(&self, key: &str, max: u32, window: Duration) -> Result<(), ApiError> {
        if self.allow(key, max, window) { Ok(()) } else { Err(ApiError::limited()) }
    }

    /// Like [`Limiter::check`] without recording a hit: for limits that count
    /// only what succeeded ([`Limiter::record`] afterwards).
    pub fn peek(&self, key: &str, max: u32, window: Duration) -> Result<(), ApiError> {
        let now = Instant::now();
        let map = self.hits.lock().unwrap();
        let used = map.get(key).map_or(0, |q| q.iter().filter(|t| now.duration_since(**t) <= window).count());
        if used < max as usize { Ok(()) } else { Err(ApiError::limited()) }
    }

    pub fn record(&self, key: &str) {
        self.hits.lock().unwrap().entry(key.to_string()).or_default().push_back(Instant::now());
    }

    /// Forget keys without hits in the last day.
    pub fn sweep(&self) {
        let now = Instant::now();
        self.hits.lock().unwrap().retain(|_, q| q.back().is_some_and(|t| now.duration_since(*t) < Duration::from_secs(86_400)));
    }
}

// ---------------------------------------------------------------- events

const QUEUE_MAX: usize = 256;

struct Queue {
    next: u64,
    events: VecDeque<EventEnvelope>,
    tx: watch::Sender<u64>,
    touched: Instant,
}

impl Queue {
    fn new() -> Queue {
        Queue { next: 1, events: VecDeque::new(), tx: watch::channel(0).0, touched: Instant::now() }
    }
}

/// Per-principal event queues behind `GET /api/v1/events` (long poll).
#[derive(Default)]
pub struct EventHub {
    queues: Mutex<HashMap<String, Queue>>,
}

impl EventHub {
    pub fn push(&self, principal: &str, event: ServerEvent) {
        let mut map = self.queues.lock().unwrap();
        let q = map.entry(principal.to_string()).or_insert_with(Queue::new);
        let seq = q.next;
        q.next += 1;
        q.events.push_back(EventEnvelope { seq, event });
        while q.events.len() > QUEUE_MAX {
            q.events.pop_front();
        }
        let _ = q.tx.send(seq);
    }

    pub async fn poll(&self, principal: &str, after: u64, timeout: Duration) -> EventsResponse {
        let mut rx = {
            let mut map = self.queues.lock().unwrap();
            let q = map.entry(principal.to_string()).or_insert_with(Queue::new);
            q.touched = Instant::now();
            // `after` from a previous server run can be ahead of us: start over.
            let after = if after >= q.next { 0 } else { after };
            let ready: Vec<EventEnvelope> = q.events.iter().filter(|e| e.seq > after).cloned().collect();
            if !ready.is_empty() {
                return EventsResponse { next: ready.last().map(|e| e.seq).unwrap_or(after), events: ready };
            }
            q.tx.subscribe()
        };
        let _ = tokio::time::timeout(timeout, rx.changed()).await;
        let mut map = self.queues.lock().unwrap();
        let q = map.entry(principal.to_string()).or_insert_with(Queue::new);
        q.touched = Instant::now();
        let after = if after >= q.next { 0 } else { after };
        let ready: Vec<EventEnvelope> = q.events.iter().filter(|e| e.seq > after).cloned().collect();
        EventsResponse { next: ready.last().map(|e| e.seq).unwrap_or(after), events: ready }
    }

    /// Drop queues nobody polled for ten minutes.
    pub fn sweep(&self) {
        self.queues.lock().unwrap().retain(|_, q| q.touched.elapsed() < Duration::from_secs(600) || q.tx.receiver_count() > 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn long_poll_wakes() {
        let hub = Arc::new(EventHub::default());
        let h = hub.clone();
        let t = tokio::spawn(async move { h.poll("a:1", 0, Duration::from_secs(5)).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        hub.push("a:1", ServerEvent::RoomClosed { room: "r".into() });
        let r = t.await.unwrap();
        assert_eq!(r.events.len(), 1);
        assert_eq!(r.next, 1);
        let r = hub.poll("a:1", 1, Duration::from_millis(10)).await;
        assert!(r.events.is_empty());
    }

    #[test]
    fn limiter() {
        let l = Limiter::default();
        assert!(l.allow("x", 2, Duration::from_secs(60)));
        assert!(l.allow("x", 2, Duration::from_secs(60)));
        assert!(!l.allow("x", 2, Duration::from_secs(60)));
    }
}
