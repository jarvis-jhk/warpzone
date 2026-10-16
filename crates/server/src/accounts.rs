//! Usernames and devices. An account is a username plus one or more device
//! keys; nothing personal is stored.

use crate::auth::{Auth, client_ip};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::HeaderMap;
use rusqlite::{OptionalExtension, params};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use warpzone_protocol::api::*;
use warpzone_protocol::names::{self, LINK_CODE_LEN};
use warpzone_protocol::now;

pub fn check_username(state: &AppState, name: &str) -> Result<(), ApiError> {
    if !names::valid_username(name) {
        return Err(ApiError::bad("usernames are 3-16 letters, digits, _ or -, starting with a letter or digit"));
    }
    if names::reserved_username(name) {
        return Err(ApiError::bad("that username is reserved"));
    }
    let lower = name.to_ascii_lowercase();
    let staff = state.staff.read().unwrap();
    if staff.staff.blocked_words.iter().any(|w| !w.is_empty() && lower.contains(&w.to_ascii_lowercase())) {
        return Err(ApiError::bad("please choose another username"));
    }
    Ok(())
}

pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Auth,
    Json(req): Json<RegisterRequest>,
) -> ApiResult<Account> {
    if auth.account.is_some() {
        return Err(ApiError::conflict("this device already has an account"));
    }
    check_username(&state, &req.username)?;
    let ip = client_ip(&state, &headers, peer);
    state.limiter.check(&format!("register:{ip}"), state.cfg.limits.registrations_per_ip_per_day, Duration::from_secs(86_400))?;
    let db = state.db();
    let t = now();
    let taken: Option<i64> = db
        .query_row("SELECT id FROM accounts WHERE username_lc = ?1", [req.username.to_ascii_lowercase()], |r| r.get(0))
        .optional()?;
    if taken.is_some() {
        return Err(ApiError::conflict("that username is taken"));
    }
    db.execute(
        "INSERT INTO accounts (username, username_lc, created) VALUES (?1, ?2, ?3)",
        params![req.username, req.username.to_ascii_lowercase(), t],
    )?;
    let id = db.last_insert_rowid();
    db.execute("INSERT INTO keys (key, account, added) VALUES (?1, ?2, ?3)", params![auth.key, id, t])?;
    log::info!("account {id} registered");
    Ok(Json(Account { id, username: req.username, created: t }))
}

pub async fn me(auth: Auth) -> ApiResult<serde_json::Value> {
    Ok(Json(serde_json::json!({ "key": auth.key, "account": auth.account, "roles": auth.roles })))
}

pub async fn profile(State(state): State<AppState>, Path(username): Path<String>) -> ApiResult<Profile> {
    let db = state.db();
    let acc: Option<(i64, String, i64)> = db
        .query_row("SELECT id, username, created FROM accounts WHERE username_lc = ?1", [username.to_ascii_lowercase()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let (id, username, created) = acc.ok_or_else(|| ApiError::not_found("no such player"))?;
    let mut st = db.prepare("SELECT game, queue, season, mu, sigma, games, wins FROM ratings WHERE account = ?1 ORDER BY season DESC")?;
    let ratings = st
        .query_map([id], |r| {
            let (mu, sigma): (f64, f64) = (r.get(3)?, r.get(4)?);
            let games: u32 = r.get(5)?;
            Ok(ProfileRating {
                game: r.get(0)?,
                queue: r.get(1)?,
                season: r.get(2)?,
                rating: crate::ladder::display(mu, sigma),
                games,
                wins: r.get(6)?,
                placing: games < crate::ladder::PLACEMENT_GAMES,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(Profile { username, created, ratings }))
}

pub async fn link_code(State(state): State<AppState>, auth: Auth) -> ApiResult<LinkCodeResponse> {
    let acc = auth.account()?;
    state.limiter.check(&format!("linkcode:{}", acc.id), 10, Duration::from_secs(3600))?;
    let code = names::generate_code(LINK_CODE_LEN);
    state.link_codes.lock().unwrap().insert(code.clone(), (acc.id, Instant::now()));
    Ok(Json(LinkCodeResponse { code, expires_in: 600 }))
}

pub async fn link(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Auth,
    Json(req): Json<LinkRequest>,
) -> ApiResult<Account> {
    let ip = client_ip(&state, &headers, peer);
    state.limiter.check(&format!("guess:{ip}"), state.cfg.limits.code_guesses_per_ip_per_minute, Duration::from_secs(60))?;
    if auth.account.is_some() {
        return Err(ApiError::conflict("this device already has an account"));
    }
    let code = names::normalize_code(&req.code);
    let (account, at) = state.link_codes.lock().unwrap().remove(&code).ok_or_else(|| ApiError::not_found("unknown or expired code"))?;
    if at.elapsed() > Duration::from_secs(600) {
        return Err(ApiError::not_found("unknown or expired code"));
    }
    let db = state.db();
    let n: i64 = db.query_row("SELECT COUNT(*) FROM keys WHERE account = ?1", [account], |r| r.get(0))?;
    if n >= 10 {
        return Err(ApiError::conflict("an account can have at most 10 devices"));
    }
    db.execute("INSERT INTO keys (key, account, added) VALUES (?1, ?2, ?3)", params![auth.key, account, now()])?;
    let acc = crate::auth::account_for_key(&db, &auth.key)?.ok_or_else(|| ApiError::internal("link lost"))?;
    Ok(Json(acc))
}

pub async fn keys(State(state): State<AppState>, auth: Auth) -> ApiResult<Vec<DeviceKey>> {
    let acc = auth.account()?;
    let db = state.db();
    let mut st = db.prepare("SELECT key, added FROM keys WHERE account = ?1 ORDER BY added")?;
    let v = st.query_map([acc.id], |r| Ok(DeviceKey { key: r.get(0)?, added: r.get(1)? }))?.collect::<Result<Vec<_>, _>>()?;
    Ok(Json(v))
}

pub async fn remove_key(State(state): State<AppState>, auth: Auth, Path(key): Path<String>) -> ApiResult<serde_json::Value> {
    let acc = auth.account()?;
    if key == auth.key {
        return Err(ApiError::bad("a device cannot remove itself; delete the account or use another device"));
    }
    let n = state.db().execute("DELETE FROM keys WHERE key = ?1 AND account = ?2", params![key, acc.id])?;
    if n == 0 {
        return Err(ApiError::not_found("no such device on this account"));
    }
    Ok(Json(serde_json::json!({ "removed": key })))
}

/// Delete the account, its devices, ratings and scores. Mods it published stay
/// (other mods may depend on them) but lose their owner.
pub async fn delete(State(state): State<AppState>, auth: Auth) -> ApiResult<serde_json::Value> {
    let acc = auth.account()?.clone();
    state.rooms.lock().unwrap().remove_account(&state, acc.id);
    state.db().execute("DELETE FROM accounts WHERE id = ?1", [acc.id])?;
    log::info!("account {} deleted on request", acc.id);
    Ok(Json(serde_json::json!({ "deleted": acc.username })))
}

#[derive(serde::Deserialize)]
pub struct Rename {
    pub username: String,
}

/// Moderator: force a rename (offensive or impersonating names).
pub async fn force_rename(
    State(state): State<AppState>,
    auth: Auth,
    Path(old): Path<String>,
    Json(req): Json<Rename>,
) -> ApiResult<serde_json::Value> {
    if !auth.is_moderator() {
        return Err(ApiError::forbidden("moderators only"));
    }
    check_username(&state, &req.username)?;
    let db = state.db();
    let n = db
        .execute(
            "UPDATE accounts SET username = ?1, username_lc = ?2 WHERE username_lc = ?3",
            params![req.username, req.username.to_ascii_lowercase(), old.to_ascii_lowercase()],
        )
        .map_err(|_| ApiError::conflict("that username is taken"))?;
    if n == 0 {
        return Err(ApiError::not_found("no such player"));
    }
    Ok(Json(serde_json::json!({ "renamed": old, "to": req.username })))
}
