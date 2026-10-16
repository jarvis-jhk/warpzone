//! Leaderboards for non-PvP records (speedrun times, high scores). A board is
//! (game, board, ruleset); each player keeps their best entry, all-time and —
//! for seasonal boards — per season.

use crate::auth::Auth;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::Response;
use base64::Engine;
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};
use warpzone_protocol::api::*;
use warpzone_protocol::descriptor::{GameDescriptor, Leaderboard};
use warpzone_protocol::now;

fn board<'a>(g: &'a GameDescriptor, id: &str) -> Result<&'a Leaderboard, ApiError> {
    g.leaderboards.iter().find(|b| b.id == id).ok_or_else(|| ApiError::not_found("no such leaderboard"))
}

fn check_ruleset(r: &str) -> Result<(), ApiError> {
    if r == "stock" || (r.len() == 64 && r.bytes().all(|b| b.is_ascii_hexdigit())) {
        Ok(())
    } else {
        Err(ApiError::bad("ruleset is `stock` or a lockfile hash"))
    }
}

pub async fn submit(
    State(state): State<AppState>,
    auth: Auth,
    Path((game, board_id)): Path<(String, String)>,
    Json(req): Json<ScoreSubmit>,
) -> ApiResult<ScoreSubmitted> {
    let acc = auth.account()?.clone();
    let g = state.game(&game)?;
    let b = board(&g, &board_id)?.clone();
    check_ruleset(&req.ruleset)?;
    state.limiter.check(&format!("score:{}", acc.id), 120, std::time::Duration::from_secs(3600))?;
    if b.kind == "time" && req.value <= 0 {
        return Err(ApiError::bad("times must be positive milliseconds"));
    }
    if let Some(data) = &req.replay {
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(|_| ApiError::bad("replay is not base64"))?;
        if bytes.len() as u64 > state.cfg.limits.max_replay_kb * 1024 {
            return Err(ApiError::too_large("replay too large"));
        }
        let hash = hex::encode(Sha256::digest(&bytes));
        if req.replay_hash.as_deref() != Some(hash.as_str()) {
            return Err(ApiError::bad("replay_hash must be the sha256 of the replay"));
        }
        let dir = state.cfg.data_dir.join("replays");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(&hash);
        if !path.exists() {
            std::fs::write(&path, &bytes)?;
        }
    }
    if let Some(h) = &req.replay_hash
        && (h.len() != 64 || !h.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err(ApiError::bad("replay_hash must be sha256 hex"));
    }
    let lower_wins = b.kind == "time";
    let mut seasons = vec!["all".to_string()];
    if b.seasonal {
        seasons.push(crate::ladder::season_id(now(), &g.season));
    }
    let db = state.db();
    let mut improved = false;
    for s in &seasons {
        let old: Option<i64> = db
            .query_row(
                "SELECT value FROM scores WHERE game=?1 AND board=?2 AND ruleset=?3 AND season=?4 AND account=?5",
                params![game, board_id, req.ruleset, s, acc.id],
                |r| r.get(0),
            )
            .optional()?;
        let better = old.is_none_or(|o| if lower_wins { req.value < o } else { req.value > o });
        if better {
            db.execute(
                "INSERT OR REPLACE INTO scores (game, board, ruleset, season, account, value, replay_hash, submitted) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![game, board_id, req.ruleset, s, acc.id, req.value, req.replay_hash, now()],
            )?;
            if s == "all" {
                improved = true;
            }
        }
    }
    let best: i64 = db.query_row(
        "SELECT value FROM scores WHERE game=?1 AND board=?2 AND ruleset=?3 AND season='all' AND account=?4",
        params![game, board_id, req.ruleset, acc.id],
        |r| r.get(0),
    )?;
    let cmp = if lower_wins { "<" } else { ">" };
    let better: u32 = db.query_row(
        &format!("SELECT COUNT(*) FROM scores WHERE game=?1 AND board=?2 AND ruleset=?3 AND season='all' AND value {cmp} ?4"),
        params![game, board_id, req.ruleset, best],
        |r| r.get(0),
    )?;
    Ok(Json(ScoreSubmitted { best, rank: better + 1, improved }))
}

#[derive(serde::Deserialize)]
pub struct BoardQuery {
    pub ruleset: Option<String>,
    pub season: Option<String>,
}

pub fn top(state: &AppState, g: &GameDescriptor, board_id: &str, ruleset: &str, season: &str, limit: u32) -> Result<BoardResponse, ApiError> {
    let b = board(g, board_id)?;
    check_ruleset(ruleset)?;
    let order = if b.kind == "time" { "ASC" } else { "DESC" };
    let db = state.db();
    let mut st = db.prepare(&format!(
        "SELECT a.username, s.value, s.replay_hash, s.submitted FROM scores s JOIN accounts a ON a.id = s.account
         WHERE s.game=?1 AND s.board=?2 AND s.ruleset=?3 AND s.season=?4 ORDER BY s.value {order}, s.submitted ASC LIMIT ?5"
    ))?;
    let entries = st
        .query_map(params![g.id, board_id, ruleset, season, limit], |r| {
            Ok(BoardEntry { rank: 0, username: r.get(0)?, value: r.get(1)?, replay_hash: r.get(2)?, submitted: r.get(3)? })
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .enumerate()
        .map(|(i, mut e)| {
            e.rank = i as u32 + 1;
            e
        })
        .collect();
    Ok(BoardResponse {
        game: g.id.clone(),
        board: board_id.into(),
        title: b.title.clone(),
        kind: b.kind.clone(),
        ruleset: ruleset.into(),
        season: season.into(),
        entries,
    })
}

pub async fn get(State(state): State<AppState>, Path((game, board_id)): Path<(String, String)>, Query(q): Query<BoardQuery>) -> ApiResult<BoardResponse> {
    let g = state.game(&game)?;
    let ruleset = q.ruleset.unwrap_or_else(|| "stock".into());
    let season = q.season.unwrap_or_else(|| "all".into());
    top(&state, &g, &board_id, &ruleset, &season, 100).map(Json)
}

pub async fn replay(State(state): State<AppState>, Path(hash): Path<String>) -> Result<Response, ApiError> {
    if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::not_found("no such replay"));
    }
    let bytes = tokio::fs::read(state.cfg.data_dir.join("replays").join(&hash)).await.map_err(|_| ApiError::not_found("no such replay"))?;
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{hash}.replay\""))
        .body(Body::from(bytes))
        .unwrap())
}
