//! `warpzone-server` — one small daemon for every game: rooms, STUN, NAT
//! punching and relay, ranked ladders, leaderboards, the mod archive and the
//! website.
//!
//! ```text
//! warpzone-server [--config /config/server.toml]
//! warpzone-server check-games <dir>      validate game descriptors and exit
//! warpzone-server --version
//! ```

mod accounts;
mod auth;
mod boards;
mod config;
mod db;
mod error;
mod ladder;
mod modstore;
mod relay;
mod releases;
mod rooms;
mod state;
mod web;

use axum::Json;
use axum::Router;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::routing::{delete, get, post};
use ed25519_dalek::SigningKey;
use error::{ApiError, ApiResult};
use state::{AppState, State as Srv};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use warpzone_protocol::api::*;
use warpzone_protocol::descriptor::GameDescriptor;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Logger(log::LevelFilter);

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= self.0 && (m.target().starts_with("warpzone") || m.level() <= log::Level::Warn)
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("{} {:<5} {}", warpzone_protocol::now(), r.level(), r.args());
        }
    }
    fn flush(&self) {}
}

fn load_key(path: &std::path::Path) -> std::io::Result<SigningKey> {
    if let Ok(text) = std::fs::read_to_string(path)
        && let Ok(seed) = hex::decode(text.trim())
        && let Ok(seed) = <[u8; 32]>::try_from(seed.as_slice())
    {
        return Ok(SigningKey::from_bytes(&seed));
    }
    let seed: [u8; 32] = rand::random();
    std::fs::write(path, hex::encode(seed))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    log::info!("generated a new server key in {}", path.display());
    Ok(SigningKey::from_bytes(&seed))
}

fn udp_public(cfg: &config::Config) -> Vec<String> {
    if !cfg.udp_public.is_empty() {
        return cfg.udp_public.clone();
    }
    let port = cfg.udp_listen.rsplit(':').next().unwrap_or("3478");
    let host = cfg.public_url.split("://").nth(1).unwrap_or(&cfg.public_url).split(['/', ':']).next().unwrap_or("");
    vec![format!("{host}:{port}")]
}

async fn info(State(s): State<AppState>) -> Json<ServerInfo> {
    Json(ServerInfo {
        name: s.cfg.name.clone(),
        version: VERSION.into(),
        server_key: s.server_key_hex.clone(),
        public_url: s.cfg.public_url.clone(),
        udp: udp_public(&s.cfg),
        features: vec!["rooms".into(), "relay".into(), "ladder".into(), "leaderboards".into(), "mods".into()],
        limits: Limits {
            max_room_players: s.cfg.limits.max_room_players,
            max_metadata_bytes: s.cfg.limits.max_metadata_bytes,
            max_datagram: warpzone_protocol::MAX_DATAGRAM as u32,
            max_mod_mb: s.games.read().unwrap().by_id.values().filter_map(|g| g.mods.as_ref()).map(|m| m.max_size_mb).max().unwrap_or(0),
        },
    })
}

async fn endpoints(State(s): State<AppState>) -> Json<Endpoints> {
    Json(Endpoints { api: s.cfg.public_url.clone(), udp: udp_public(&s.cfg) })
}

async fn games(State(s): State<AppState>) -> Json<Vec<GameDescriptor>> {
    s.reload_games();
    Json(s.games.read().unwrap().by_id.values().filter(|g| !g.hidden).cloned().collect())
}

async fn game(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<GameDescriptor> {
    Ok(Json(s.game(&id)?))
}

async fn release(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<ReleaseInfo> {
    let key = release_key(&s, &id)?;
    s.releases.read().unwrap().get(&key).cloned().map(Json).ok_or_else(|| ApiError::not_found("no release known yet"))
}

fn release_key(s: &AppState, id: &str) -> Result<String, ApiError> {
    if id == "launcher" { Ok(releases::LAUNCHER.to_string()) } else { Ok(s.game(id)?.id) }
}

async fn changelog(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Vec<ChangelogEntry>> {
    let key = release_key(&s, &id)?;
    Ok(Json(s.changelogs.read().unwrap().get(&key).cloned().unwrap_or_default()))
}

#[derive(serde::Deserialize)]
struct UpdatesQuery {
    #[serde(default)]
    current: String,
}

async fn updates(State(s): State<AppState>, Path(id): Path<String>, Query(q): Query<UpdatesQuery>) -> ApiResult<UpdateInfo> {
    let key = release_key(&s, &id)?;
    if q.current.len() > 40 {
        return Err(ApiError::bad("current is a version"));
    }
    Ok(Json(releases::updates(&s, &key, q.current.trim_start_matches('v'))))
}

#[derive(serde::Deserialize)]
struct EventsQuery {
    #[serde(default)]
    after: u64,
    #[serde(default)]
    timeout: Option<u64>,
}

async fn events(State(s): State<AppState>, a: auth::Auth, Query(q): Query<EventsQuery>) -> Json<EventsResponse> {
    let t = Duration::from_secs(q.timeout.unwrap_or(25).min(30));
    Json(s.events.poll(&a.principal(), q.after, t).await)
}

async fn stats(State(s): State<AppState>) -> Json<serde_json::Value> {
    let (rooms, players) = {
        let r = s.rooms.lock().unwrap();
        (r.by_id.len(), r.by_id.values().map(|x| x.members.len()).sum::<usize>())
    };
    let relay = s.relay.stats.lock().unwrap().clone();
    Json(serde_json::json!({
        "version": VERSION,
        "uptime": s.started.elapsed().as_secs(),
        "rooms": rooms,
        "players_in_rooms": players,
        "relay_channels": s.relay.channel_count(),
        "relay_bytes": relay.forwarded_bytes,
        "relay_dropped": relay.dropped,
        "stun_requests": relay.stun,
    }))
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/info", get(info))
        .route("/stats", get(stats))
        .route("/games", get(games))
        .route("/games/{game}", get(game))
        .route("/games/{game}/release", get(release))
        .route("/games/{game}/changelog", get(changelog))
        .route("/games/{game}/updates", get(updates))
        .route("/auth/challenge", post(auth::challenge))
        .route("/auth/login", post(auth::login))
        .route("/accounts", post(accounts::register))
        .route("/accounts/me", get(accounts::me).delete(accounts::delete))
        .route("/accounts/me/link-code", post(accounts::link_code))
        .route("/accounts/link", post(accounts::link))
        .route("/accounts/me/keys", get(accounts::keys))
        .route("/accounts/me/keys/{key}", delete(accounts::remove_key))
        .route("/accounts/{username}", get(accounts::profile))
        .route("/rooms", post(rooms::create).get(rooms::list))
        .route("/rooms/{id}", get(rooms::get).delete(rooms::close))
        .route("/rooms/{id}/join", post(rooms::join))
        .route("/rooms/{id}/knocks/{knock}", post(rooms::decide_knock))
        .route("/rooms/{id}/heartbeat", post(rooms::heartbeat))
        .route("/rooms/{id}/candidates", post(rooms::candidates))
        .route("/rooms/{id}/leave", post(rooms::leave))
        .route("/rooms/{id}/kick", post(rooms::kick))
        .route("/join", post(rooms::join_by_code))
        .route("/events", get(events))
        .route("/ladder/queue", post(ladder::queue_join).get(ladder::queue_status).delete(ladder::queue_leave))
        .route("/matches/{id}", get(ladder::match_info))
        .route("/matches/{id}/report", post(ladder::report))
        .route("/games/{game}/ladder/{queue}", get(ladder::standings))
        .route("/games/{game}/boards/{board}", get(boards::get))
        .route("/games/{game}/boards/{board}/scores", post(boards::submit))
        .route("/replays/{hash}", get(boards::replay))
        .route("/games/{game}/mods", get(modstore::list).post(modstore::upload).layer(DefaultBodyLimit::max(1024 * 1024 * 1024)))
        .route("/games/{game}/mods/resolve", post(modstore::resolve))
        .route("/games/{game}/mods/{name}", get(modstore::detail))
        .route("/games/{game}/mods/{name}/{version}/download", get(modstore::download))
        .route("/games/{game}/mods/{name}/{version}/yank", post(modstore::yank))
        .route("/games/{game}/lockfiles", post(modstore::put_lockfile))
        .route("/games/{game}/lockfiles/{hash}", get(modstore::get_lockfile))
        .route("/reports", post(modstore::report))
        .route("/moderation/queue", get(modstore::queue))
        .route("/moderation/mods/{game}/{name}/{version}", post(modstore::decide))
        .route("/moderation/notes", post(modstore::add_note))
        .route("/moderation/reports/{id}/close", post(modstore::close_report))
        .route("/moderation/accounts/{username}/rename", post(accounts::force_rename))
        .fallback(|| async { ApiError::not_found("no such API route") });
    Router::new()
        .nest(warpzone_protocol::API_PREFIX, api)
        .route("/", get(web::index))
        .route("/games/{id}", get(web::game_page))
        .route("/games/{id}/mods/{name}", get(web::mod_page))
        .route("/join/{id}", get(web::join_page))
        .route("/static/{file}", get(web::static_file))
        .route("/endpoints.json", get(endpoints))
        .route("/healthz", get(|| async { "ok" }))
        .fallback(web::fallback)
        .with_state(state)
}

/// Periodic housekeeping.
async fn sweeper(state: AppState) {
    let mut every = tokio::time::interval(Duration::from_secs(10));
    let mut n = 0u64;
    loop {
        every.tick().await;
        state.rooms.lock().unwrap().sweep(&state);
        state.relay.sweep(Duration::from_secs(180));
        auth::sweep(&state);
        n += 1;
        if n.is_multiple_of(6) {
            state.events.sweep();
            state.limiter.sweep();
            state.reload_staff();
            state.reload_games();
        }
    }
}

fn check_games(dir: &str) -> i32 {
    let mut bad = 0;
    let mut n = 0;
    let Ok(rd) = std::fs::read_dir(dir) else {
        eprintln!("{dir}: cannot read");
        return 2;
    };
    let mut paths: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
    paths.sort();
    for p in paths {
        n += 1;
        let r = std::fs::read_to_string(&p)
            .map_err(|e| e.to_string())
            .and_then(|t| toml::from_str::<GameDescriptor>(&t).map_err(|e| e.to_string()))
            .and_then(|d| d.validate().map(|_| d).map_err(|e| e.to_string()))
            .and_then(|d| if p.file_stem().is_some_and(|s| s == d.id.as_str()) { Ok(d) } else { Err("id does not match file name".into()) });
        match r {
            Ok(d) => println!("ok   {} ({})", d.id, d.features().join(", ")),
            Err(e) => {
                bad += 1;
                println!("FAIL {}: {e}", p.display());
            }
        }
    }
    println!("{n} descriptors, {bad} invalid");
    if bad > 0 { 1 } else { 0 }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--version") {
        println!("warpzone-server {VERSION}");
        return;
    }
    if args.get(1).map(String::as_str) == Some("check-games") {
        std::process::exit(check_games(args.get(2).map(String::as_str).unwrap_or("games")));
    }
    let level = match std::env::var("WARPZONE_LOG").as_deref() {
        Ok("debug") => log::LevelFilter::Debug,
        Ok("warn") => log::LevelFilter::Warn,
        _ => log::LevelFilter::Info,
    };
    let _ = log::set_logger(Box::leak(Box::new(Logger(level))));
    log::set_max_level(level);
    let path = args
        .iter()
        .position(|a| a == "--config")
        .and_then(|i| args.get(i + 1).cloned())
        .or_else(|| std::env::var("WARPZONE_CONFIG").ok())
        .unwrap_or_else(|| "/config/server.toml".into());
    let cfg = match config::Config::load(std::path::Path::new(&path)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = std::fs::create_dir_all(&cfg.data_dir) {
        eprintln!("{}: {e}", cfg.data_dir.display());
        std::process::exit(2);
    }
    let key = load_key(&cfg.data_dir.join("server.key")).expect("server key");
    let conn = db::open(&cfg.data_dir.join("warpzone.db")).expect("database");
    let udp = relay::bind_udp(&cfg.udp_listen).unwrap_or_else(|e| {
        eprintln!("udp {}: {e}", cfg.udp_listen);
        std::process::exit(2);
    });
    let http: SocketAddr = cfg.http_listen.parse().expect("http_listen must be ip:port");
    let state: AppState = Arc::new(Srv::new(cfg, conn, key));
    log::info!("warpzone-server {VERSION}: http {http}, udp {}, key {}", state.cfg.udp_listen, state.server_key_hex);
    tokio::spawn(relay::run(state.relay.clone(), udp, state.clone()));
    tokio::spawn(sweeper(state.clone()));
    tokio::spawn(ladder::run(state.clone()));
    tokio::spawn(releases::run(state.clone()));
    let listener = tokio::net::TcpListener::bind(http).await.expect("http listener");
    let app = router(state).into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("server");
}
