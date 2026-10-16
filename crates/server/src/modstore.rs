//! The mod archive: uploads, the moderation queue, listings, downloads,
//! dependency resolution and lockfiles. See `docs/mods.md` and
//! `docs/moderation.md`.
//!
//! Every uploaded version starts `pending` and is visible only to its author
//! and to staff until a moderator approves it. The server never opens an
//! archive: it stores bytes and hashes and reads only the manifest submitted
//! next to it.

use crate::auth::{Auth, MaybeAuth};
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, principal_account};
use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::header;
use axum::response::Response;
use rusqlite::{OptionalExtension, params};
use semver::{Version, VersionReq};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use warpzone_protocol::api::*;
use warpzone_protocol::mods::{self, Candidate, Dependency, Lockfile, Manifest};
use warpzone_protocol::now;

pub fn blob_path(state: &AppState, sha: &str) -> PathBuf {
    state.cfg.data_dir.join("blobs").join(&sha[..2]).join(sha)
}

fn bad_name(game: &str, name: &str) -> Result<(), ApiError> {
    if !warpzone_protocol::names::valid_game_id(game) || !warpzone_protocol::names::valid_mod_name(name) {
        return Err(ApiError::not_found("no such mod"));
    }
    Ok(())
}

// ---------------------------------------------------------------- upload

pub async fn upload(State(state): State<AppState>, auth: Auth, Path(game): Path<String>, mut form: Multipart) -> ApiResult<Uploaded> {
    let acc = auth.account()?.clone();
    let g = state.game(&game)?;
    let mcfg = g.mods.clone().ok_or_else(|| ApiError::bad(format!("{} does not take mods", g.name)))?;
    state.limiter.check(&format!("upload:{}", acc.id), state.cfg.limits.uploads_per_account_per_day, std::time::Duration::from_secs(86_400))?;
    let max = mcfg.max_size_mb.min(state.cfg.limits.mod_store_gb as u32 * 1024) as u64 * 1024 * 1024;
    let tmp_dir = state.cfg.data_dir.join("blobs").join("tmp");
    tokio::fs::create_dir_all(&tmp_dir).await?;
    let tmp = tmp_dir.join(hex::encode(rand::random::<[u8; 8]>()));
    let mut manifest: Option<Manifest> = None;
    let mut archive: Option<(String, u64, String)> = None;
    let result: Result<(), ApiError> = async {
        while let Some(mut field) = form.next_field().await.map_err(|e| ApiError::bad(format!("bad form: {e}")))? {
            match field.name() {
                Some("manifest") => {
                    let text = field.text().await.map_err(|e| ApiError::bad(e.to_string()))?;
                    if text.len() > 16 * 1024 {
                        return Err(ApiError::too_large("manifest larger than 16 KiB"));
                    }
                    manifest = Some(serde_json::from_str(&text).map_err(|e| ApiError::bad(format!("manifest: {e}")))?);
                }
                Some("archive") => {
                    let filename = field.file_name().unwrap_or("mod.zip").to_string();
                    let mut f = tokio::fs::File::create(&tmp).await?;
                    let mut h = Sha256::new();
                    let mut size = 0u64;
                    while let Some(chunk) = field.chunk().await.map_err(|e| ApiError::bad(e.to_string()))? {
                        size += chunk.len() as u64;
                        if size > max {
                            return Err(ApiError::too_large(format!("archives are limited to {} MiB for this game", max / 1024 / 1024)));
                        }
                        h.update(&chunk);
                        f.write_all(&chunk).await?;
                    }
                    f.flush().await?;
                    archive = Some((hex::encode(h.finalize()), size, filename));
                }
                _ => {}
            }
        }
        Ok(())
    }
    .await;
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    let (Some(m), Some((sha, size, filename))) = (manifest, archive) else {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(ApiError::bad("send multipart fields `manifest` (JSON) and `archive` (file)"));
    };
    let outcome = store_upload(&state, &acc, &game, &mcfg, m, &sha, size, &filename, &tmp);
    if outcome.is_err() {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    outcome.map(Json)
}

#[allow(clippy::too_many_arguments)]
fn store_upload(
    state: &AppState,
    acc: &Account,
    game: &str,
    mcfg: &warpzone_protocol::descriptor::Mods,
    m: Manifest,
    sha: &str,
    size: u64,
    filename: &str,
    tmp: &std::path::Path,
) -> Result<Uploaded, ApiError> {
    let (version, _, deps) = m.check().map_err(|e| ApiError::bad(e.to_string()))?;
    if !mcfg.categories.iter().any(|c| c.id == m.category) {
        let ids: Vec<&str> = mcfg.categories.iter().map(|c| c.id.as_str()).collect();
        return Err(ApiError::bad(format!("category must be one of: {}", ids.join(", "))));
    }
    if size == 0 {
        return Err(ApiError::bad("empty archive"));
    }
    let safe_name: String = filename.chars().filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).take(80).collect();
    let db = state.db();
    let used: i64 = db.query_row("SELECT COALESCE(SUM(size), 0) FROM mod_versions WHERE state != 'rejected'", [], |r| r.get(0))?;
    if (used as u64 + size) > state.cfg.limits.mod_store_gb * 1024 * 1024 * 1024 {
        return Err(ApiError::new(axum::http::StatusCode::INSUFFICIENT_STORAGE, "store_full", "the mod archive is full; try again later"));
    }
    let owner: Option<Option<i64>> = db.query_row("SELECT owner FROM mods WHERE game=?1 AND name=?2", params![game, m.name], |r| r.get(0)).optional()?;
    match owner {
        Some(Some(o)) if o != acc.id => return Err(ApiError::forbidden(format!("the mod name {} belongs to someone else", m.name))),
        Some(None) => return Err(ApiError::forbidden(format!("the mod name {} is retired", m.name))),
        _ => {}
    }
    let exists: bool = db
        .query_row("SELECT 1 FROM mod_versions WHERE game=?1 AND name=?2 AND version=?3", params![game, m.name, version.to_string()], |_| Ok(()))
        .optional()?
        .is_some();
    if exists {
        return Err(ApiError::conflict("this version exists already; versions are immutable, publish a new one"));
    }
    let dest = blob_path(state, sha);
    std::fs::create_dir_all(dest.parent().unwrap())?;
    if dest.exists() {
        std::fs::remove_file(tmp)?;
    } else {
        std::fs::rename(tmp, &dest)?;
    }
    let t = now();
    if owner.is_none() {
        db.execute(
            "INSERT INTO mods (game, name, owner, title, category, description, tags, license, homepage, created, updated)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
            params![game, m.name, acc.id, m.title, m.category, m.description, serde_json::to_string(&m.tags).unwrap(), m.license, m.homepage, t],
        )?;
    }
    let dep_strings: Vec<String> = deps.iter().map(|d| d.to_string()).collect();
    db.execute(
        "INSERT INTO mod_versions (game, name, version, manifest, game_version, deps, sha256, size, filename, state, submitted)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'pending',?10)",
        params![
            game,
            m.name,
            version.to_string(),
            serde_json::to_string(&m).unwrap(),
            m.game_version,
            serde_json::to_string(&dep_strings).unwrap(),
            sha,
            size as i64,
            if safe_name.is_empty() { "mod.zip".to_string() } else { safe_name },
            t
        ],
    )?;
    drop(db);
    for p in state.staff_principals() {
        state
            .events
            .push(&p, ServerEvent::ModSubmitted { game: game.into(), name: m.name.clone(), version: version.to_string(), author: acc.username.clone() });
    }
    state.notify(format!("Mod waiting for review: {game}/{} {} by {} ({} KiB)", m.name, version, acc.username, size / 1024));
    log::info!("mod upload {game}/{} {version} by account {}", m.name, acc.id);
    Ok(Uploaded { name: m.name, version: version.to_string(), sha256: sha.into(), state: "pending".into() })
}

// ---------------------------------------------------------------- listing

#[derive(serde::Deserialize, Default)]
pub struct ModQuery {
    pub category: Option<String>,
    pub q: Option<String>,
    /// `downloads` (default), `updated` or `name`.
    pub sort: Option<String>,
}

pub fn list_inner(state: &AppState, game: &str, q: &ModQuery) -> Result<Vec<ModSummary>, ApiError> {
    let db = state.db();
    let order = match q.sort.as_deref() {
        Some("updated") => "m.updated DESC",
        Some("name") => "m.name ASC",
        _ => "m.downloads DESC, m.updated DESC",
    };
    let mut st = db.prepare(&format!(
        "SELECT m.name, m.title, m.category, COALESCE(a.username, '(deleted)'), m.description, m.downloads, m.updated, m.tags
         FROM mods m LEFT JOIN accounts a ON a.id = m.owner
         WHERE m.game = ?1 AND EXISTS (SELECT 1 FROM mod_versions v WHERE v.game = m.game AND v.name = m.name AND v.state = 'approved' AND v.yanked = 0)
           AND (?2 IS NULL OR m.category = ?2)
           AND (?3 IS NULL OR m.name LIKE '%' || ?3 || '%' OR m.title LIKE '%' || ?3 || '%' OR m.tags LIKE '%' || ?3 || '%')
         ORDER BY {order} LIMIT 500"
    ))?;
    let rows: Vec<ModSummary> = st
        .query_map(params![game, q.category, q.q], |r| {
            Ok(ModSummary {
                game: game.into(),
                name: r.get(0)?,
                title: r.get(1)?,
                category: r.get(2)?,
                author: r.get(3)?,
                latest: String::new(),
                description: r.get(4)?,
                downloads: r.get::<_, i64>(5)? as u64,
                updated: r.get(6)?,
                tags: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
            })
        })?
        .collect::<Result<_, _>>()?;
    drop(st);
    let mut out = Vec::with_capacity(rows.len());
    for mut m in rows {
        m.latest = latest_version(&db, game, &m.name)?.unwrap_or_default();
        out.push(m);
    }
    Ok(out)
}

fn latest_version(db: &rusqlite::Connection, game: &str, name: &str) -> rusqlite::Result<Option<String>> {
    let mut st = db.prepare("SELECT version FROM mod_versions WHERE game=?1 AND name=?2 AND state='approved' AND yanked=0")?;
    let mut v: Vec<Version> = st.query_map(params![game, name], |r| r.get::<_, String>(0))?.filter_map(|s| s.ok()?.parse().ok()).collect();
    v.sort();
    Ok(v.last().map(|v| v.to_string()))
}

pub async fn list(State(state): State<AppState>, Path(game): Path<String>, Query(q): Query<ModQuery>) -> ApiResult<Vec<ModSummary>> {
    state.game(&game)?;
    list_inner(&state, &game, &q).map(Json)
}

pub fn detail_inner(state: &AppState, game: &str, name: &str, viewer: Option<&Auth>) -> Result<ModDetail, ApiError> {
    bad_name(game, name)?;
    let g = state.game(game)?;
    let db = state.db();
    let row = db
        .query_row(
            "SELECT m.title, m.category, COALESCE(a.username, '(deleted)'), m.description, m.downloads, m.updated, m.tags, m.license, m.homepage, m.owner
             FROM mods m LEFT JOIN accounts a ON a.id = m.owner WHERE m.game=?1 AND m.name=?2",
            params![game, name],
            |r| {
                Ok((
                    ModSummary {
                        game: game.into(),
                        name: name.into(),
                        title: r.get(0)?,
                        category: r.get(1)?,
                        author: r.get(2)?,
                        latest: String::new(),
                        description: r.get(3)?,
                        downloads: r.get::<_, i64>(4)? as u64,
                        updated: r.get(5)?,
                        tags: serde_json::from_str(&r.get::<_, String>(6)?).unwrap_or_default(),
                    },
                    r.get::<_, String>(7)?,
                    r.get::<_, Option<String>>(8)?,
                    r.get::<_, Option<i64>>(9)?,
                ))
            },
        )
        .optional()?;
    let (mut summary, license, homepage, owner) = row.ok_or_else(|| ApiError::not_found("no such mod"))?;
    let privileged = viewer.is_some_and(|a| a.is_staff() || (owner.is_some() && a.account.as_ref().map(|x| x.id) == owner));
    let mut st =
        db.prepare("SELECT version, game_version, deps, sha256, size, state, yanked, submitted, reason FROM mod_versions WHERE game=?1 AND name=?2")?;
    let mut versions: Vec<ModVersionInfo> = st
        .query_map(params![game, name], |r| {
            Ok(ModVersionInfo {
                version: r.get(0)?,
                game_version: r.get(1)?,
                dependencies: serde_json::from_str(&r.get::<_, String>(2)?).unwrap_or_default(),
                sha256: r.get(3)?,
                size: r.get::<_, i64>(4)? as u64,
                state: r.get(5)?,
                yanked: r.get::<_, i64>(6)? != 0,
                published: r.get(7)?,
                reason: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|v| privileged || v.state == "approved")
        .collect();
    if versions.is_empty() {
        return Err(ApiError::not_found("no such mod"));
    }
    versions.sort_by(|a, b| {
        let (x, y) = (a.version.parse::<Version>().ok(), b.version.parse::<Version>().ok());
        y.cmp(&x)
    });
    summary.latest = latest_version(&db, game, name)?.unwrap_or_default();
    Ok(ModDetail { summary, license, homepage, contains_code: g.mods.as_ref().is_some_and(|m| m.contains_code), versions })
}

pub async fn detail(State(state): State<AppState>, MaybeAuth(auth): MaybeAuth, Path((game, name)): Path<(String, String)>) -> ApiResult<ModDetail> {
    detail_inner(&state, &game, &name, auth.as_ref()).map(Json)
}

pub async fn download(
    State(state): State<AppState>,
    MaybeAuth(auth): MaybeAuth,
    Path((game, name, version)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    bad_name(&game, &name)?;
    let (sha, size, st, filename, owner) = {
        let db = state.db();
        db.query_row(
            "SELECT v.sha256, v.size, v.state, v.filename, m.owner FROM mod_versions v JOIN mods m ON m.game=v.game AND m.name=v.name
             WHERE v.game=?1 AND v.name=?2 AND v.version=?3",
            params![game, name, version],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<i64>>(4)?)),
        )
        .optional()?
        .ok_or_else(|| ApiError::not_found("no such mod version"))?
    };
    let privileged = auth.as_ref().is_some_and(|a| a.is_staff() || (owner.is_some() && a.account.as_ref().map(|x| x.id) == owner));
    if st != "approved" && !(privileged && st != "rejected") {
        return Err(ApiError::not_found("no such mod version"));
    }
    if st == "approved" {
        let _ = state.db().execute("UPDATE mods SET downloads = downloads + 1 WHERE game=?1 AND name=?2", params![game, name]);
    }
    let file = tokio::fs::File::open(blob_path(&state, &sha)).await.map_err(ApiError::internal)?;
    let ext = filename.rsplit_once('.').map(|(_, e)| e).unwrap_or("bin");
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, size)
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}_{version}.{ext}\""))
        .header("x-warpzone-sha256", sha)
        .body(Body::from_stream(tokio_util::io::ReaderStream::new(file)))
        .unwrap())
}

#[derive(serde::Deserialize)]
pub struct Yank {
    pub yanked: bool,
}

pub async fn yank(
    State(state): State<AppState>,
    auth: Auth,
    Path((game, name, version)): Path<(String, String, String)>,
    Json(req): Json<Yank>,
) -> ApiResult<serde_json::Value> {
    bad_name(&game, &name)?;
    let db = state.db();
    let owner: Option<i64> = db.query_row("SELECT owner FROM mods WHERE game=?1 AND name=?2", params![game, name], |r| r.get(0)).optional()?.flatten();
    let mine = owner.is_some() && auth.account.as_ref().map(|a| a.id) == owner;
    if !mine && !auth.is_moderator() {
        return Err(ApiError::forbidden("only the author or a moderator can yank"));
    }
    let n = db.execute("UPDATE mod_versions SET yanked = ?1 WHERE game=?2 AND name=?3 AND version=?4", params![req.yanked as i64, game, name, version])?;
    if n == 0 {
        return Err(ApiError::not_found("no such mod version"));
    }
    Ok(Json(serde_json::json!({ "yanked": req.yanked })))
}

// ---------------------------------------------------------------- resolve / lockfiles

/// All approved versions of `game` as resolver candidates.
fn catalog(db: &rusqlite::Connection, game: &str) -> Result<BTreeMap<String, Vec<Candidate>>, ApiError> {
    let mut st = db.prepare(
        "SELECT v.name, v.version, v.deps, v.game_version, v.sha256, v.size, m.category, v.yanked
         FROM mod_versions v JOIN mods m ON m.game=v.game AND m.name=v.name WHERE v.game=?1 AND v.state='approved'",
    )?;
    let mut out: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    let rows = st.query_map([game], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, i64>(7)?,
        ))
    })?;
    for row in rows {
        let (name, version, deps, gv, sha, size, cat, yanked) = row?;
        let (Ok(version), Ok(game_req)) = (Version::parse(&version), VersionReq::parse(&gv)) else { continue };
        let deps: Vec<String> = serde_json::from_str(&deps).unwrap_or_default();
        let deps: Vec<Dependency> = deps.iter().filter_map(|d| Dependency::parse(d).ok()).collect();
        out.entry(name.clone()).or_default().push(Candidate {
            name,
            version,
            deps,
            game_req,
            sha256: sha,
            size: size as u64,
            category: cat,
            yanked: yanked != 0,
        });
    }
    Ok(out)
}

fn store_lockfile(db: &rusqlite::Connection, lock: &Lockfile) -> rusqlite::Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO lockfiles (hash, game, body, created) VALUES (?1, ?2, ?3, ?4)",
        params![lock.hash, lock.game, serde_json::to_string(lock).unwrap(), now()],
    )?;
    Ok(())
}

pub async fn resolve(State(state): State<AppState>, Path(game): Path<String>, Json(req): Json<ResolveRequest>) -> ApiResult<Lockfile> {
    state.game(&game)?;
    if req.mods.len() > 200 {
        return Err(ApiError::bad("at most 200 mods per request"));
    }
    let gv = mods::parse_version(&req.game_version).map_err(|e| ApiError::bad(e.to_string()))?;
    let wanted = req.mods.iter().map(|m| Dependency::parse(m)).collect::<Result<Vec<_>, _>>().map_err(|e| ApiError::bad(e.to_string()))?;
    let db = state.db();
    let cat = catalog(&db, &game)?;
    let lock =
        mods::resolve(&cat, &wanted, &game, &gv).map_err(|e| ApiError::new(axum::http::StatusCode::UNPROCESSABLE_ENTITY, "unresolvable", e.to_string()))?;
    store_lockfile(&db, &lock)?;
    Ok(Json(lock))
}

pub async fn get_lockfile(State(state): State<AppState>, Path((game, hash)): Path<(String, String)>) -> ApiResult<Lockfile> {
    let body: Option<String> = state.db().query_row("SELECT body FROM lockfiles WHERE game=?1 AND hash=?2", params![game, hash], |r| r.get(0)).optional()?;
    let body = body.ok_or_else(|| ApiError::not_found("unknown lockfile"))?;
    Ok(Json(serde_json::from_str(&body).map_err(ApiError::internal)?))
}

/// Store a lockfile a client composed (e.g. a host's hand-picked set). Every
/// entry must be an approved version with the same hash.
pub async fn put_lockfile(State(state): State<AppState>, Path(game): Path<String>, Json(lock): Json<Lockfile>) -> ApiResult<Lockfile> {
    if lock.game != game || lock.format != 1 || !lock.verify_hash() || lock.mods.len() > 500 {
        return Err(ApiError::bad("lockfile does not verify"));
    }
    let db = state.db();
    for m in &lock.mods {
        let ok: bool = db
            .query_row(
                "SELECT 1 FROM mod_versions WHERE game=?1 AND name=?2 AND version=?3 AND sha256=?4 AND state='approved'",
                params![game, m.name, m.version, m.sha256],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !ok {
            return Err(ApiError::bad(format!("{} {} is not an approved version with that hash", m.name, m.version)));
        }
    }
    store_lockfile(&db, &lock)?;
    Ok(Json(lock))
}

// ---------------------------------------------------------------- reports & moderation

pub async fn report(State(state): State<AppState>, auth: Auth, Json(req): Json<ReportRequest>) -> ApiResult<serde_json::Value> {
    let acc = auth.account()?.clone();
    state.limiter.check(&format!("report:{}", acc.id), state.cfg.limits.reports_per_account_per_day, std::time::Duration::from_secs(86_400))?;
    if !matches!(req.target.as_str(), "mod" | "user") || !REPORT_REASONS.contains(&req.reason.as_str()) {
        return Err(ApiError::bad(format!("target is mod or user; reason one of {}", REPORT_REASONS.join(", "))));
    }
    if req.detail.as_ref().is_some_and(|d| d.len() > 1000) || req.name.len() > 64 {
        return Err(ApiError::bad("report too long"));
    }
    state.db().execute(
        "INSERT INTO reports (target, game, name, version, reason, detail, reporter, created) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![req.target, req.game, req.name, req.version, req.reason, req.detail, acc.id, now()],
    )?;
    state.notify(format!("Report ({}): {} {} {}", req.reason, req.target, req.game.as_deref().unwrap_or(""), req.name));
    Ok(Json(serde_json::json!({ "reported": true })))
}

pub async fn queue(State(state): State<AppState>, auth: Auth) -> ApiResult<ModerationQueue> {
    if !auth.is_staff() {
        return Err(ApiError::forbidden("staff only"));
    }
    let games = state.games.read().unwrap().by_id.clone();
    let db = state.db();
    let mut st = db.prepare(
        "SELECT v.game, v.name, v.version, m.title, m.category, COALESCE(a.username,'(deleted)'), v.size, v.sha256, v.submitted, v.manifest
         FROM mod_versions v JOIN mods m ON m.game=v.game AND m.name=v.name LEFT JOIN accounts a ON a.id=m.owner
         WHERE v.state='pending' ORDER BY v.submitted",
    )?;
    let mut pending: Vec<PendingMod> = st
        .query_map([], |r| {
            let game: String = r.get(0)?;
            let name: String = r.get(1)?;
            let version: String = r.get(2)?;
            Ok(PendingMod {
                download: format!("/api/v1/games/{game}/mods/{name}/{version}/download"),
                contains_code: false,
                game,
                name,
                version,
                title: r.get(3)?,
                category: r.get(4)?,
                author: r.get(5)?,
                size: r.get::<_, i64>(6)? as u64,
                sha256: r.get(7)?,
                submitted: r.get(8)?,
                manifest: serde_json::from_str(&r.get::<_, String>(9)?).unwrap_or_default(),
                notes: Vec::new(),
            })
        })?
        .collect::<Result<_, _>>()?;
    drop(st);
    for p in pending.iter_mut() {
        p.contains_code = games.get(&p.game).and_then(|g| g.mods.as_ref()).is_some_and(|m| m.contains_code);
        let mut st = db.prepare("SELECT role, text, created FROM notes WHERE game=?1 AND name=?2 AND version=?3 ORDER BY created")?;
        p.notes = st
            .query_map(params![p.game, p.name, p.version], |r| Ok(Note { role: r.get(0)?, text: r.get(1)?, created: r.get(2)? }))?
            .collect::<Result<_, _>>()?;
    }
    let mut st = db.prepare(
        "SELECT r.id, r.target, r.game, r.name, r.version, r.reason, r.detail, COALESCE(a.username,'(deleted)'), r.created
         FROM reports r LEFT JOIN accounts a ON a.id = r.reporter WHERE r.closed IS NULL ORDER BY r.created",
    )?;
    let reports = st
        .query_map([], |r| {
            Ok(OpenReport {
                id: r.get(0)?,
                target: r.get(1)?,
                game: r.get(2)?,
                name: r.get(3)?,
                version: r.get(4)?,
                reason: r.get(5)?,
                detail: r.get(6)?,
                reporter: r.get(7)?,
                created: r.get(8)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(Json(ModerationQueue { pending, reports }))
}

pub async fn decide(
    State(state): State<AppState>,
    auth: Auth,
    Path((game, name, version)): Path<(String, String, String)>,
    Json(req): Json<Decision>,
) -> ApiResult<serde_json::Value> {
    if !auth.is_moderator() {
        return Err(ApiError::forbidden("moderators only"));
    }
    bad_name(&game, &name)?;
    let new_state = match req.decision.as_str() {
        "approve" | "unhide" => "approved",
        "reject" => "rejected",
        "hide" => "hidden",
        _ => return Err(ApiError::bad("decision is approve, reject, hide or unhide")),
    };
    if new_state == "rejected" && !req.reason.as_deref().is_some_and(|r| REJECT_REASONS.contains(&r)) {
        return Err(ApiError::bad(format!("rejections need a reason: {}", REJECT_REASONS.join(", "))));
    }
    let db = state.db();
    let row: Option<(String, String, String)> = db
        .query_row("SELECT state, manifest, sha256 FROM mod_versions WHERE game=?1 AND name=?2 AND version=?3", params![game, name, version], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let (old, manifest, sha) = row.ok_or_else(|| ApiError::not_found("no such mod version"))?;
    db.execute(
        "UPDATE mod_versions SET state=?1, reason=?2, decided=?3 WHERE game=?4 AND name=?5 AND version=?6",
        params![new_state, req.reason, now(), game, name, version],
    )?;
    if new_state == "approved" {
        // The newest approved manifest describes the mod.
        if let Ok(m) = serde_json::from_str::<Manifest>(&manifest) {
            let newest = latest_version(&db, &game, &name)?;
            if newest.as_deref() == Some(version.as_str()) {
                db.execute(
                    "UPDATE mods SET title=?1, category=?2, description=?3, tags=?4, license=?5, homepage=?6, updated=?7 WHERE game=?8 AND name=?9",
                    params![m.title, m.category, m.description, serde_json::to_string(&m.tags).unwrap(), m.license, m.homepage, now(), game, name],
                )?;
            }
        }
    }
    if new_state == "rejected" {
        let still_used: bool = db.query_row("SELECT 1 FROM mod_versions WHERE sha256=?1 AND state != 'rejected'", [&sha], |_| Ok(())).optional()?.is_some();
        if !still_used {
            let _ = std::fs::remove_file(blob_path(&state, &sha));
        }
    }
    let owner: Option<i64> = db.query_row("SELECT owner FROM mods WHERE game=?1 AND name=?2", params![game, name], |r| r.get(0)).optional()?.flatten();
    drop(db);
    if let Some(o) = owner {
        state.events.push(
            &principal_account(o),
            ServerEvent::ModDecision {
                game: game.clone(),
                name: name.clone(),
                version: version.clone(),
                decision: req.decision.clone(),
                reason: req.reason.clone(),
            },
        );
    }
    log::info!("mod {game}/{name} {version}: {old} -> {new_state}");
    Ok(Json(serde_json::json!({ "state": new_state })))
}

pub async fn add_note(State(state): State<AppState>, auth: Auth, Json(req): Json<NoteRequest>) -> ApiResult<serde_json::Value> {
    if !auth.is_staff() {
        return Err(ApiError::forbidden("staff only"));
    }
    if req.text.is_empty() || req.text.len() > 8000 {
        return Err(ApiError::bad("notes are 1-8000 characters"));
    }
    let role = if auth.is_moderator() { "moderator" } else { "reviewer" };
    state.db().execute(
        "INSERT INTO notes (game, name, version, role, text, created) VALUES (?1,?2,?3,?4,?5,?6)",
        params![req.game, req.name, req.version, role, req.text, now()],
    )?;
    Ok(Json(serde_json::json!({ "noted": true })))
}

pub async fn close_report(State(state): State<AppState>, auth: Auth, Path(id): Path<i64>) -> ApiResult<serde_json::Value> {
    if !auth.is_moderator() {
        return Err(ApiError::forbidden("moderators only"));
    }
    let n = state.db().execute("UPDATE reports SET closed=?1 WHERE id=?2 AND closed IS NULL", params![now(), id])?;
    if n == 0 {
        return Err(ApiError::not_found("no open report with that id"));
    }
    Ok(Json(serde_json::json!({ "closed": id })))
}
