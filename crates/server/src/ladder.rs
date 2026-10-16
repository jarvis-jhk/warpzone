//! Ranked play: OpenSkill (Weng-Lin, Plackett-Luce) ratings, calendar seasons,
//! matchmaking and result reports. See `docs/ladder.md`.

use crate::auth::Auth;
use crate::error::{ApiError, ApiResult};
use crate::rooms::{Member, NewRoom};
use crate::state::{AppState, State as Srv, principal_account};
use axum::Json;
use axum::extract::{Path, Query, State};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};
use warpzone_protocol::api::*;
use warpzone_protocol::descriptor::GameDescriptor;
use warpzone_protocol::now;

// ---------------------------------------------------------------- OpenSkill

pub const MU: f64 = 25.0;
pub const SIGMA: f64 = 25.0 / 3.0;
const BETA: f64 = SIGMA / 2.0;
const KAPPA: f64 = 0.0001;
const TAU: f64 = MU / 300.0;
/// Matches before a player appears in the standings.
pub const PLACEMENT_GAMES: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rating {
    pub mu: f64,
    pub sigma: f64,
}

impl Default for Rating {
    fn default() -> Self {
        Rating { mu: MU, sigma: SIGMA }
    }
}

/// The number players see: `1500 + 20 * (mu - 3 sigma)`. It starts at 1500 and
/// rises for most players while the system becomes sure about them; ranks
/// use the same conservative value.
pub fn display(mu: f64, sigma: f64) -> i32 {
    (1500.0 + 20.0 * (mu - 3.0 * sigma)).round() as i32
}

/// Plackett-Luce update. `ranks[i]` is team `i`'s place (0 = best; ties share
/// a value). Returns the new ratings in the same shape.
pub fn rate(teams: &[Vec<Rating>], ranks: &[usize]) -> Vec<Vec<Rating>> {
    let teams: Vec<Vec<Rating>> = teams
        .iter()
        .map(|t| t.iter().map(|r| Rating { mu: r.mu, sigma: (r.sigma * r.sigma + TAU * TAU).sqrt() }).collect())
        .collect();
    let n = teams.len();
    let mu: Vec<f64> = teams.iter().map(|t| t.iter().map(|r| r.mu).sum()).collect();
    let s2: Vec<f64> = teams.iter().map(|t| t.iter().map(|r| r.sigma * r.sigma).sum()).collect();
    let c = s2.iter().map(|s| s + BETA * BETA).sum::<f64>().sqrt();
    let sum_q: Vec<f64> = (0..n).map(|q| (0..n).filter(|i| ranks[*i] >= ranks[q]).map(|i| (mu[i] / c).exp()).sum()).collect();
    let a: Vec<f64> = (0..n).map(|i| (0..n).filter(|q| ranks[*q] == ranks[i]).count() as f64).collect();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let mut omega = 0.0;
        let mut delta = 0.0;
        let e_i = (mu[i] / c).exp();
        for q in 0..n {
            if ranks[q] > ranks[i] {
                continue;
            }
            let quotient = e_i / sum_q[q];
            if q == i {
                omega += (1.0 - quotient) / a[q];
            } else {
                omega -= quotient / a[q];
            }
            delta += quotient * (1.0 - quotient) / a[q];
        }
        let gamma = s2[i].sqrt() / c;
        omega *= s2[i] / c;
        delta *= gamma * s2[i] / (c * c);
        out.push(
            teams[i]
                .iter()
                .map(|r| {
                    let share = r.sigma * r.sigma / s2[i];
                    Rating { mu: r.mu + share * omega, sigma: r.sigma * (1.0 - share * delta).max(KAPPA).sqrt() }
                })
                .collect(),
        );
    }
    out
}

// ---------------------------------------------------------------- seasons

/// Season id for a Unix time: `2026-Q4` (quarterly), `2026-10` (monthly) or
/// `2026` (yearly). All seasons start at 00:00 UTC.
pub fn season_id(t: i64, length: &str) -> String {
    let (y, m) = year_month(t);
    match length {
        "monthly" => format!("{y}-{m:02}"),
        "yearly" => format!("{y}"),
        _ => format!("{y}-Q{}", (m - 1) / 3 + 1),
    }
}

/// Civil year and month (1–12) of a Unix time, UTC.
fn year_month(t: i64) -> (i64, u32) {
    // Howard Hinnant's days-to-civil.
    let z = t.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m)
}

/// Soft reset into a new season: halfway back to the default.
pub fn soft_reset(r: Rating) -> Rating {
    Rating { mu: MU + (r.mu - MU) / 2.0, sigma: (r.sigma + (SIGMA - r.sigma) / 2.0).min(SIGMA) }
}

/// Current rating of `account`, carrying the last season over with a soft
/// reset when this season has no row yet.
pub fn load_rating(db: &Connection, game: &str, queue: &str, season: &str, account: i64) -> rusqlite::Result<(Rating, u32, u32)> {
    if let Some(row) = db
        .query_row(
            "SELECT mu, sigma, games, wins FROM ratings WHERE game=?1 AND queue=?2 AND season=?3 AND account=?4",
            params![game, queue, season, account],
            |r| Ok((Rating { mu: r.get(0)?, sigma: r.get(1)? }, r.get(2)?, r.get(3)?)),
        )
        .optional()?
    {
        return Ok(row);
    }
    let prev: Option<Rating> = db
        .query_row(
            "SELECT mu, sigma FROM ratings WHERE game=?1 AND queue=?2 AND account=?3 AND season < ?4 ORDER BY season DESC LIMIT 1",
            params![game, queue, account, season],
            |r| Ok(Rating { mu: r.get(0)?, sigma: r.get(1)? }),
        )
        .optional()?;
    Ok((prev.map(soft_reset).unwrap_or_default(), 0, 0))
}

// ---------------------------------------------------------------- matchmaking

struct QueueEntry {
    account: i64,
    username: String,
    key: String,
    candidates: Vec<String>,
    rating: i32,
    since: Instant,
    since_unix: i64,
}

/// Bucket key: game, queue, game version, ruleset.
type Bucket = (String, String, String, String);

#[derive(Default)]
pub struct Matchmaker {
    buckets: HashMap<Bucket, Vec<QueueEntry>>,
}

impl Matchmaker {
    fn remove(&mut self, account: i64) {
        for v in self.buckets.values_mut() {
            v.retain(|e| e.account != account);
        }
        self.buckets.retain(|_, v| !v.is_empty());
    }

    fn find(&self, account: i64) -> Option<(&Bucket, &QueueEntry, usize)> {
        self.buckets.iter().find_map(|(b, v)| v.iter().find(|e| e.account == account).map(|e| (b, e, v.len())))
    }
}

/// Allowed rating spread for a group whose oldest member waited `secs`.
fn window(secs: u64) -> i32 {
    (150 + 15 * secs.min(90) as i32).min(1500)
}

/// Snake draft: strongest to team 0, next to team 1, ..., then backwards.
fn snake(group: &[usize], teams: usize) -> Vec<Vec<usize>> {
    let mut out = vec![Vec::new(); teams];
    for (i, p) in group.iter().enumerate() {
        let round = i / teams;
        let pos = i % teams;
        let t = if round % 2 == 0 { pos } else { teams - 1 - pos };
        out[t].push(*p);
    }
    out
}

/// One matchmaking pass: forms every match it can and creates its room.
pub fn tick(state: &AppState) {
    let mut made = Vec::new();
    {
        let games = state.games.read().unwrap();
        let mut mm = state.matchmaker.lock().unwrap();
        for (bucket, entries) in mm.buckets.iter_mut() {
            let Some(q) = games.by_id.get(&bucket.0).and_then(|g| g.ladder.iter().find(|q| q.id == bucket.1)) else { continue };
            let need = (q.teams * q.team_size) as usize;
            entries.sort_by_key(|e| std::cmp::Reverse(e.rating));
            let mut i = 0;
            while entries.len() >= need && i + need <= entries.len() {
                let group = &entries[i..i + need];
                let oldest = group.iter().map(|e| e.since.elapsed().as_secs()).max().unwrap_or(0);
                if group[0].rating - group[need - 1].rating <= window(oldest) {
                    let taken: Vec<QueueEntry> = entries.drain(i..i + need).collect();
                    made.push((bucket.clone(), q.teams as usize, taken));
                } else {
                    i += 1;
                }
            }
        }
        mm.buckets.retain(|_, v| !v.is_empty());
    }
    for (bucket, teams, players) in made {
        if let Err(e) = start_match(state, &bucket, teams, players) {
            log::error!("could not start match: {}", e.message);
        }
    }
}

fn start_match(state: &AppState, bucket: &Bucket, n_teams: usize, players: Vec<QueueEntry>) -> Result<(), ApiError> {
    let (game_id, queue, version, ruleset) = bucket;
    let game = state.game(game_id)?;
    let idx: Vec<usize> = (0..players.len()).collect();
    let teams = snake(&idx, n_teams);
    let host = &players[0];
    let match_id = hex::encode(rand::random::<[u8; 8]>());
    let room_id = {
        let mut rooms = state.rooms.lock().unwrap();
        let title = game.ladder.iter().find(|q| &q.id == queue).map(|q| q.title.clone()).unwrap_or_default();
        let room = rooms.create(NewRoom {
            game: game_id.clone(),
            game_version: version.clone(),
            max_players: players.len() as u32,
            visibility: Visibility::Unlisted,
            entry: Entry::Approve,
            audience: Audience::default(),
            preset: None,
            name: format!("Ranked {title}"),
            metadata: BTreeMap::from([("queue".to_string(), queue.clone()), ("match".to_string(), match_id.clone())]),
            lockfile: (ruleset != "stock").then(|| ruleset.clone()),
            staged: false,
            ranked: true,
            host: Member {
                account: host.account,
                username: host.username.clone(),
                key: host.key.clone(),
                candidates: host.candidates.clone(),
                relay: None,
            },
        });
        let id = room.id.clone();
        let room = rooms.by_id.get_mut(&id).unwrap();
        for p in &players {
            room.approved.insert(p.account);
        }
        room.closed_to_joins = true;
        id
    };
    let team_accounts: Vec<Vec<i64>> = teams.iter().map(|t| t.iter().map(|i| players[*i].account).collect()).collect();
    let team_names: Vec<Vec<String>> = teams.iter().map(|t| t.iter().map(|i| players[*i].username.clone()).collect()).collect();
    let season = season_id(now(), &game.season);
    {
        let db = state.db();
        db.execute(
            "INSERT INTO matches (id, game, queue, season, room, ruleset, teams, state, created) VALUES (?1,?2,?3,?4,?5,?6,?7,'open',?8)",
            params![match_id, game_id, queue, season, room_id, ruleset, serde_json::to_string(&team_accounts).unwrap(), now()],
        )?;
        for p in &players {
            db.execute("INSERT INTO match_ratings (match, account, before) VALUES (?1, ?2, ?3)", params![match_id, p.account, p.rating])?;
        }
    }
    for p in &players {
        state.events.push(
            &principal_account(p.account),
            ServerEvent::MatchFound {
                match_id: match_id.clone(),
                game: game_id.clone(),
                queue: queue.clone(),
                room: room_id.clone(),
                host: p.account == host.account,
                teams: team_names.clone(),
            },
        );
    }
    log::info!("match {match_id} ({game_id}/{queue}) with {} players", players.len());
    Ok(())
}

pub async fn queue_join(State(state): State<AppState>, auth: Auth, Json(req): Json<QueueJoin>) -> ApiResult<QueueStatus> {
    let acc = auth.account()?.clone();
    let game = state.game(&req.game)?;
    let q = game.ladder.iter().find(|q| q.id == req.queue).ok_or_else(|| ApiError::not_found("no such queue"))?;
    let _ = q;
    if req.ruleset != "stock" && (req.ruleset.len() != 64 || !req.ruleset.bytes().all(|b| b.is_ascii_hexdigit())) {
        return Err(ApiError::bad("ruleset is `stock` or a lockfile hash"));
    }
    let season = season_id(now(), &game.season);
    let (r, _, _) = load_rating(&state.db(), &req.game, &req.queue, &season, acc.id)?;
    let mut mm = state.matchmaker.lock().unwrap();
    mm.remove(acc.id);
    let bucket = (req.game.clone(), req.queue.clone(), req.game_version.clone(), req.ruleset.clone());
    let v = mm.buckets.entry(bucket).or_default();
    v.push(QueueEntry {
        account: acc.id,
        username: acc.username,
        key: auth.key.clone(),
        candidates: req.candidates,
        rating: display(r.mu, r.sigma),
        since: Instant::now(),
        since_unix: now(),
    });
    let waiting = v.len() as u32;
    Ok(Json(QueueStatus { queued: true, game: Some(req.game), queue: Some(req.queue), since: Some(now()), waiting }))
}

pub async fn queue_status(State(state): State<AppState>, auth: Auth) -> ApiResult<QueueStatus> {
    let acc = auth.account()?;
    let mm = state.matchmaker.lock().unwrap();
    Ok(Json(match mm.find(acc.id) {
        Some((b, e, n)) => QueueStatus { queued: true, game: Some(b.0.clone()), queue: Some(b.1.clone()), since: Some(e.since_unix), waiting: n as u32 },
        None => QueueStatus { queued: false, game: None, queue: None, since: None, waiting: 0 },
    }))
}

pub async fn queue_leave(State(state): State<AppState>, auth: Auth) -> ApiResult<QueueStatus> {
    let acc = auth.account()?;
    state.matchmaker.lock().unwrap().remove(acc.id);
    Ok(Json(QueueStatus { queued: false, game: None, queue: None, since: None, waiting: 0 }))
}

// ---------------------------------------------------------------- results

fn load_match(db: &Connection, id: &str) -> Result<(String, String, String, String, Vec<Vec<i64>>, i64), ApiError> {
    db.query_row("SELECT game, queue, season, state, teams, created FROM matches WHERE id = ?1", [id], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, String>(4)?, r.get(5)?))
    })
    .optional()?
    .map(|(g, q, s, st, t, c)| (g, q, s, st, serde_json::from_str(&t).unwrap_or_default(), c))
    .ok_or_else(|| ApiError::not_found("no such match"))
}

/// Ranks per team from a report, or `None` if the report does not fit the teams.
fn team_ranks(teams: &[Vec<i64>], report: &MatchReport) -> Option<Vec<usize>> {
    let mut place: HashMap<i64, usize> = HashMap::new();
    for (i, group) in report.placements.iter().enumerate() {
        for a in group {
            if place.insert(*a, i).is_some() {
                return None;
            }
        }
    }
    let all: usize = teams.iter().map(|t| t.len()).sum();
    if place.len() != all {
        return None;
    }
    teams
        .iter()
        .map(|t| {
            let p = place.get(t.first()?)?;
            t.iter().all(|a| place.get(a) == Some(p)).then_some(*p)
        })
        .collect()
}

pub async fn report(State(state): State<AppState>, auth: Auth, Path(id): Path<String>, Json(req): Json<MatchReport>) -> ApiResult<MatchInfo> {
    let acc = auth.account()?.clone();
    {
        let db = state.db();
        let (_, _, _, st, teams, _) = load_match(&db, &id)?;
        if !teams.iter().flatten().any(|a| *a == acc.id) {
            return Err(ApiError::forbidden("you did not play in this match"));
        }
        if st != "open" {
            return Err(ApiError::conflict(format!("match is already {st}")));
        }
        if team_ranks(&teams, &req).is_none() {
            return Err(ApiError::bad("placements must list every player once, team members together"));
        }
        db.execute(
            "INSERT OR REPLACE INTO match_reports (match, account, report, created) VALUES (?1, ?2, ?3, ?4)",
            params![id, acc.id, serde_json::to_string(&req).unwrap(), now()],
        )?;
        let reports: i64 = db.query_row("SELECT COUNT(*) FROM match_reports WHERE match = ?1", [&id], |r| r.get(0))?;
        if reports as usize == teams.iter().map(|t| t.len()).sum::<usize>() {
            finalize(&state, &db, &id)?;
        }
    }
    match_info_inner(&state, &id).map(Json)
}

/// Rate a match from its reports, or mark it disputed.
fn finalize(state: &Srv, db: &Connection, id: &str) -> Result<(), ApiError> {
    let (game, queue, season, st, teams, _) = load_match(db, id)?;
    if st != "open" {
        return Ok(());
    }
    let mut st_ = db.prepare("SELECT report FROM match_reports WHERE match = ?1")?;
    let reports: Vec<MatchReport> =
        st_.query_map([id], |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).filter_map(|s| serde_json::from_str(&s).ok()).collect();
    drop(st_);
    let ranks: Vec<Option<Vec<usize>>> = reports.iter().map(|r| team_ranks(&teams, r)).collect();
    let hashes: Vec<&Option<String>> = reports.iter().map(|r| &r.state_hash).collect();
    let agree = !ranks.is_empty()
        && ranks.iter().all(|r| r.is_some() && *r == ranks[0])
        && hashes.iter().filter(|h| h.is_some()).all(|h| *h == hashes.iter().find(|h| h.is_some()).copied().unwrap_or(&None));
    let state_name = if agree { "rated" } else { "disputed" };
    if agree {
        let ranks = ranks[0].clone().unwrap();
        let mut current = Vec::new();
        for t in &teams {
            let mut v = Vec::new();
            for a in t {
                v.push(load_rating(db, &game, &queue, &season, *a)?);
            }
            current.push(v);
        }
        let new = rate(&current.iter().map(|t| t.iter().map(|x| x.0).collect()).collect::<Vec<_>>(), &ranks);
        for (ti, t) in teams.iter().enumerate() {
            for (pi, a) in t.iter().enumerate() {
                let (_, games, wins) = current[ti][pi];
                let r = new[ti][pi];
                let won = ranks[ti] == 0;
                db.execute(
                    "INSERT OR REPLACE INTO ratings (game, queue, season, account, mu, sigma, games, wins, updated) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![game, queue, season, a, r.mu, r.sigma, games + 1, wins + won as u32, now()],
                )?;
                db.execute("UPDATE match_ratings SET after = ?1 WHERE match = ?2 AND account = ?3", params![display(r.mu, r.sigma), id, a])?;
            }
        }
    }
    db.execute("UPDATE matches SET state = ?1, finished = ?2 WHERE id = ?3", params![state_name, now(), id])?;
    for a in teams.iter().flatten() {
        state.events.push(&principal_account(*a), ServerEvent::MatchFinished { match_id: id.into(), state: state_name.into() });
    }
    log::info!("match {id} {state_name}");
    Ok(())
}

/// Background: rate matches where the remaining players never reported
/// (10 minutes after the first report) and abandon silent ones (3 hours).
pub fn sweep(state: &AppState) {
    let db = state.db();
    let t = now();
    let due: Vec<String> = db
        .prepare(
            "SELECT m.id FROM matches m WHERE m.state = 'open' AND
               (SELECT MIN(created) FROM match_reports r WHERE r.match = m.id) < ?1",
        )
        .and_then(|mut s| s.query_map([t - 600], |r| r.get(0))?.collect())
        .unwrap_or_default();
    for id in due {
        if let Err(e) = finalize(state, &db, &id) {
            log::error!("finalize {id}: {}", e.message);
        }
    }
    let _ = db.execute(
        "UPDATE matches SET state = 'abandoned', finished = ?1 WHERE state = 'open' AND created < ?2
           AND NOT EXISTS (SELECT 1 FROM match_reports r WHERE r.match = matches.id)",
        params![t, t - 3 * 3600],
    );
}

fn match_info_inner(state: &AppState, id: &str) -> Result<MatchInfo, ApiError> {
    let db = state.db();
    let (game, queue, season, st, teams, created) = load_match(&db, id)?;
    let room: String = db.query_row("SELECT room FROM matches WHERE id = ?1", [id], |r| r.get(0))?;
    let mut out = Vec::new();
    for t in &teams {
        let mut v = Vec::new();
        for a in t {
            let (username, before, after): (String, i32, Option<i32>) = db
                .query_row(
                    "SELECT COALESCE((SELECT username FROM accounts WHERE id = ?2), '(deleted)'), before, after FROM match_ratings WHERE match = ?1 AND account = ?2",
                    params![id, a],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap_or(("(deleted)".into(), 0, None));
            v.push(MatchPlayer { account: *a, username, rating_before: before, rating_after: after });
        }
        out.push(v);
    }
    Ok(MatchInfo { id: id.into(), game, queue, season, state: st, room, teams: out, created })
}

pub async fn match_info(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<MatchInfo> {
    match_info_inner(&state, &id).map(Json)
}

#[derive(serde::Deserialize)]
pub struct SeasonQuery {
    pub season: Option<String>,
}

pub fn standings_inner(state: &AppState, game: &GameDescriptor, queue: &str, season: Option<String>, limit: u32) -> Result<StandingsResponse, ApiError> {
    if !game.ladder.iter().any(|q| q.id == queue) {
        return Err(ApiError::not_found("no such queue"));
    }
    let current = season_id(now(), &game.season);
    let season = season.unwrap_or_else(|| current.clone());
    let db = state.db();
    let mut seasons: Vec<String> = db
        .prepare("SELECT DISTINCT season FROM ratings WHERE game = ?1 AND queue = ?2 ORDER BY season DESC")?
        .query_map(params![game.id, queue], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    if !seasons.contains(&current) {
        seasons.insert(0, current.clone());
    }
    let mut st = db.prepare(
        "SELECT a.username, r.mu, r.sigma, r.games, r.wins FROM ratings r JOIN accounts a ON a.id = r.account
         WHERE r.game = ?1 AND r.queue = ?2 AND r.season = ?3 AND r.games >= ?4
         ORDER BY (r.mu - 3 * r.sigma) DESC LIMIT ?5",
    )?;
    let standings = st
        .query_map(params![game.id, queue, season, PLACEMENT_GAMES, limit], |r| {
            let (mu, sigma): (f64, f64) = (r.get(1)?, r.get(2)?);
            Ok(Standing { rank: 0, username: r.get(0)?, rating: display(mu, sigma), mu, sigma, games: r.get(3)?, wins: r.get(4)? })
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .enumerate()
        .map(|(i, mut s)| {
            s.rank = i as u32 + 1;
            s
        })
        .collect();
    let placing: u32 = db.query_row(
        "SELECT COUNT(*) FROM ratings WHERE game = ?1 AND queue = ?2 AND season = ?3 AND games < ?4",
        params![game.id, queue, season, PLACEMENT_GAMES],
        |r| r.get(0),
    )?;
    Ok(StandingsResponse { game: game.id.clone(), queue: queue.into(), season, current_season: current, seasons, standings, placing })
}

pub async fn standings(
    State(state): State<AppState>,
    Path((game, queue)): Path<(String, String)>,
    Query(q): Query<SeasonQuery>,
) -> ApiResult<StandingsResponse> {
    let g = state.game(&game)?;
    standings_inner(&state, &g, &queue, q.season, 200).map(Json)
}

/// Keep the matchmaker and match sweeps running.
pub async fn run(state: AppState) {
    let mut every = tokio::time::interval(Duration::from_secs(1));
    let mut n = 0u64;
    loop {
        every.tick().await;
        tick(&state);
        n += 1;
        if n % 30 == 0 {
            sweep(&state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winner_gains_loser_loses() {
        let r = rate(&[vec![Rating::default()], vec![Rating::default()]], &[0, 1]);
        assert!(r[0][0].mu > MU && r[1][0].mu < MU);
        assert!(r[0][0].sigma < SIGMA && r[1][0].sigma < SIGMA);
        assert!((r[0][0].mu - MU + (r[1][0].mu - MU)).abs() < 1e-9, "zero-sum for equal players");
    }

    #[test]
    fn ffa_order_and_ties() {
        let d = Rating::default();
        let r = rate(&[vec![d], vec![d], vec![d], vec![d]], &[0, 1, 2, 3]);
        assert!(r[0][0].mu > r[1][0].mu && r[1][0].mu > r[2][0].mu && r[2][0].mu > r[3][0].mu);
        let t = rate(&[vec![d], vec![d]], &[0, 0]);
        assert!((t[0][0].mu - MU).abs() < 1e-9);
    }

    #[test]
    fn teams_share() {
        let d = Rating::default();
        let r = rate(&[vec![d, d], vec![d, d]], &[0, 1]);
        assert!((r[0][0].mu - r[0][1].mu).abs() < 1e-9);
        assert!(r[0][0].mu > MU);
    }

    #[test]
    fn seasons() {
        // 2026-10-16 12:00 UTC
        let t = 1_792_152_000;
        assert_eq!(season_id(t, "quarterly"), "2026-Q4");
        assert_eq!(season_id(t, "monthly"), "2026-10");
        assert_eq!(season_id(t, "yearly"), "2026");
        assert_eq!(season_id(0, "quarterly"), "1970-Q1");
        // 2024-02-29 is a leap day, still Q1.
        assert_eq!(season_id(1_709_208_000, "monthly"), "2024-02");
        let s = soft_reset(Rating { mu: 35.0, sigma: 2.0 });
        assert_eq!(s.mu, 30.0);
        assert!(s.sigma > 2.0 && s.sigma < SIGMA);
    }

    #[test]
    fn snake_draft() {
        assert_eq!(snake(&[0, 1, 2, 3], 2), vec![vec![0, 3], vec![1, 2]]);
        assert_eq!(snake(&[0, 1, 2, 3], 4), vec![vec![0], vec![1], vec![2], vec![3]]);
    }

    #[test]
    fn report_shape() {
        let teams = vec![vec![1, 2], vec![3, 4]];
        let ok = MatchReport { placements: vec![vec![3, 4], vec![1, 2]], state_hash: None };
        assert_eq!(team_ranks(&teams, &ok), Some(vec![1, 0]));
        let split = MatchReport { placements: vec![vec![1, 3], vec![2, 4]], state_hash: None };
        assert_eq!(team_ranks(&teams, &split), None);
        let missing = MatchReport { placements: vec![vec![1, 2]], state_hash: None };
        assert_eq!(team_ranks(&teams, &missing), None);
    }

    #[test]
    fn display_scale() {
        assert_eq!(display(MU, SIGMA), 1500);
    }
}
