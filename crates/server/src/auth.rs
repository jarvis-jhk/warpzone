//! Keypair login: challenge → signature → bearer token. No passwords, no
//! e-mail; a device is its Ed25519 key.

use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, Session, principal_account, principal_key};
use axum::Json;
use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use ed25519_dalek::{Signature, Verifier};
use rusqlite::OptionalExtension;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use warpzone_protocol::api::*;
use warpzone_protocol::ticket::parse_key;

const TOKEN_TTL: Duration = Duration::from_secs(24 * 3600);
const CHALLENGE_TTL: Duration = Duration::from_secs(120);

/// A request from a logged-in device.
#[derive(Clone, Debug)]
pub struct Auth {
    pub key: String,
    pub account: Option<Account>,
    pub roles: Vec<String>,
}

impl Auth {
    /// Event principal: the account if there is one, else the bare key.
    pub fn principal(&self) -> String {
        match &self.account {
            Some(a) => principal_account(a.id),
            None => principal_key(&self.key),
        }
    }

    pub fn account(&self) -> Result<&Account, ApiError> {
        self.account.as_ref().ok_or_else(ApiError::needs_account)
    }

    pub fn is_moderator(&self) -> bool {
        self.roles.iter().any(|r| r == "moderator")
    }

    pub fn is_staff(&self) -> bool {
        !self.roles.is_empty()
    }
}

/// Look up the account a key belongs to.
pub fn account_for_key(db: &rusqlite::Connection, key: &str) -> rusqlite::Result<Option<Account>> {
    db.query_row("SELECT a.id, a.username, a.created FROM keys k JOIN accounts a ON a.id = k.account WHERE k.key = ?1", [key], |r| {
        Ok(Account { id: r.get(0)?, username: r.get(1)?, created: r.get(2)? })
    })
    .optional()
}

fn bearer(parts: &Parts) -> Option<String> {
    let h = parts.headers.get("authorization")?.to_str().ok()?;
    h.strip_prefix("Bearer ").map(|t| t.trim().to_string())
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = bearer(parts).ok_or_else(ApiError::unauthorized)?;
        let session = {
            let mut s = state.sessions.lock().unwrap();
            match s.get(&token) {
                Some(sess) if sess.expires > Instant::now() => sess.clone(),
                Some(_) => {
                    s.remove(&token);
                    return Err(ApiError::unauthorized());
                }
                None => return Err(ApiError::unauthorized()),
            }
        };
        let account = account_for_key(&state.db(), &session.key)?;
        Ok(Auth { roles: state.roles(&session.key), key: session.key, account })
    }
}

/// `Option<Auth>` that does not fail without a token (public routes that show
/// more to owners and staff).
pub struct MaybeAuth(pub Option<Auth>);

impl FromRequestParts<AppState> for MaybeAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        if bearer(parts).is_none() {
            return Ok(MaybeAuth(None));
        }
        Ok(MaybeAuth(Some(Auth::from_request_parts(parts, state).await?)))
    }
}

/// The client's IP: `X-Forwarded-For` behind a trusted proxy, else the socket.
pub fn client_ip(state: &AppState, headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    if state.cfg.trust_forwarded_for
        && let Some(ip) =
            headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).and_then(|v| v.split(',').next_back()).and_then(|v| v.trim().parse().ok())
    {
        return ip;
    }
    peer.ip()
}

pub async fn challenge(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<ChallengeRequest>,
) -> ApiResult<ChallengeResponse> {
    let ip = client_ip(&state, &headers, peer);
    state.limiter.check(&format!("login:{ip}"), state.cfg.limits.logins_per_ip_per_minute, Duration::from_secs(60))?;
    if parse_key(&req.key).is_none() {
        return Err(ApiError::bad("key must be a hex Ed25519 public key"));
    }
    let nonce = hex::encode(rand::random::<[u8; 32]>());
    let mut c = state.challenges.lock().unwrap();
    c.retain(|_, (_, t)| t.elapsed() < CHALLENGE_TTL);
    if c.len() > 100_000 {
        return Err(ApiError::limited());
    }
    c.insert(nonce.clone(), (req.key, Instant::now()));
    Ok(Json(ChallengeResponse { nonce, expires_in: CHALLENGE_TTL.as_secs() }))
}

pub async fn login(State(state): State<AppState>, Json(req): Json<LoginRequest>) -> ApiResult<LoginResponse> {
    let (key, at) = state
        .challenges
        .lock()
        .unwrap()
        .remove(&req.nonce)
        .ok_or_else(|| ApiError::new(axum::http::StatusCode::UNAUTHORIZED, "bad_challenge", "unknown or used challenge"))?;
    if key != req.key || at.elapsed() > CHALLENGE_TTL {
        return Err(ApiError::new(axum::http::StatusCode::UNAUTHORIZED, "bad_challenge", "challenge expired"));
    }
    let vk = parse_key(&key).ok_or_else(|| ApiError::bad("bad key"))?;
    let sig: [u8; 64] = hex::decode(&req.signature).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| ApiError::bad("signature must be 64 hex bytes"))?;
    vk.verify(&login_message(&state.server_key_hex, &req.nonce), &Signature::from_bytes(&sig))
        .map_err(|_| ApiError::new(axum::http::StatusCode::UNAUTHORIZED, "bad_signature", "signature does not verify"))?;
    let token = hex::encode(rand::random::<[u8; 32]>());
    {
        let mut s = state.sessions.lock().unwrap();
        s.insert(token.clone(), Session { key: key.clone(), expires: Instant::now() + TOKEN_TTL });
    }
    let account = account_for_key(&state.db(), &key)?;
    Ok(Json(LoginResponse { token, expires_in: TOKEN_TTL.as_secs(), account, roles: state.roles(&key) }))
}

pub fn sweep(state: &AppState) {
    let now = Instant::now();
    state.sessions.lock().unwrap().retain(|_, s| s.expires > now);
    state.challenges.lock().unwrap().retain(|_, (_, t)| t.elapsed() < CHALLENGE_TTL);
    state.link_codes.lock().unwrap().retain(|_, (_, t)| t.elapsed() < Duration::from_secs(600));
}
