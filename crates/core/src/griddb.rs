//! Artwork from [SteamGridDB](https://www.steamgriddb.com), looked up at run
//! time: descriptors carry only the SteamGridDB game id, never the images.
//!
//! With a personal API key (steamgriddb.com → Preferences → API) the
//! documented v2 API is used; without one, the public endpoints the website
//! itself uses (no key, same data). Only portrait grids (2:3 and 22:31) that
//! are static and not marked NSFW, humour or epilepsy are offered.
//!
//! [`Cache`] keeps grid lists (a week) and downloaded images under
//! `<home>/cache/griddb/`, so the library shows its art offline too.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

const SITE: &str = "https://www.steamgriddb.com";
/// Portrait grid sizes, as SteamGridDB names them.
const PORTRAIT: [&str; 3] = ["600x900", "342x482", "660x930"];
/// Images are only fetched from SteamGridDB's own hosts.
const IMAGE_HOSTS: [&str; 3] = ["https://cdn2.steamgriddb.com/", "https://cdn.steamgriddb.com/", "https://steamgriddb.com/"];
/// How long a cached grid list is used before asking again.
const LIST_TTL: u64 = 7 * 24 * 3600;

/// A game on SteamGridDB (a search hit).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GameHit {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub release_date: Option<i64>,
    #[serde(default)]
    pub verified: bool,
}

impl GameHit {
    /// "Name (1997)".
    pub fn label(&self) -> String {
        match self.release_date.filter(|d| *d > 0) {
            Some(t) => format!("{} ({})", self.name, 1970 + t / 31_556_952),
            None => self.name.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Author {
    #[serde(default)]
    pub name: String,
}

/// One grid image.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Grid {
    pub id: u64,
    pub url: String,
    pub thumb: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub mime: String,
    #[serde(default)]
    pub nsfw: bool,
    #[serde(default)]
    pub humor: bool,
    #[serde(default)]
    pub epilepsy: bool,
    #[serde(default)]
    pub is_animated: bool,
    #[serde(default)]
    pub author: Option<Author>,
}

impl Grid {
    /// Fit for a library tile: portrait, static, nothing a player did not ask for.
    pub fn suitable(&self) -> bool {
        !self.nsfw && !self.humor && !self.epilepsy && !self.is_animated && self.height > self.width && !self.mime.contains("gif")
    }
}

/// One page of grids for a game.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct GridPage {
    pub grids: Vec<Grid>,
    /// Grids on all pages (before filtering).
    pub total: u32,
    pub page: u32,
    pub per_page: u32,
}

impl GridPage {
    pub fn more(&self) -> bool {
        (self.page + 1) * self.per_page.max(1) < self.total
    }
}

#[derive(Deserialize)]
struct Answer<T> {
    success: bool,
    data: Option<T>,
    #[serde(default)]
    errors: Vec<String>,
}

#[derive(Deserialize)]
struct PublicAssets {
    #[serde(default)]
    assets: Vec<Grid>,
    #[serde(default)]
    total: u32,
    #[serde(default)]
    limit: u32,
}

pub struct GridDb {
    agent: ureq::Agent,
    key: Option<String>,
}

impl GridDb {
    /// `key`: a personal SteamGridDB API key, or `None` for the public endpoints.
    pub fn new(key: Option<&str>) -> GridDb {
        let agent: ureq::Agent = crate::http_config()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .user_agent(format!("warpzone-launcher/{} (+https://github.com/jarvis-jhk/warpzone)", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        GridDb { agent, key: key.map(str::trim).filter(|k| !k.is_empty()).map(String::from) }
    }

    fn read<T: serde::de::DeserializeOwned>(what: &str, mut resp: ureq::http::Response<ureq::Body>) -> Result<T> {
        let status = resp.status().as_u16();
        let body = resp.body_mut().with_config().limit(4 * 1024 * 1024).read_to_vec().map_err(|e| Error::Network(e.to_string()))?;
        let a: Answer<T> = serde_json::from_slice(&body).map_err(|e| Error::Other(format!("SteamGridDB {what}: HTTP {status}: {e}")))?;
        match (a.success, a.data) {
            (true, Some(d)) => Ok(d),
            _ => Err(Error::Other(format!("SteamGridDB {what}: HTTP {status}: {}", a.errors.join(", ")))),
        }
    }

    fn get<T: serde::de::DeserializeOwned>(&self, what: &str, url: &str) -> Result<T> {
        let mut r = self.agent.get(url);
        if let Some(k) = &self.key {
            r = r.header("Authorization", &format!("Bearer {k}"));
        }
        Self::read(what, r.call().map_err(|e| Error::Network(e.to_string()))?)
    }

    /// Games whose name matches `term`, best match first.
    pub fn search(&self, term: &str) -> Result<Vec<GameHit>> {
        let term = term.trim();
        if term.is_empty() {
            return Ok(Vec::new());
        }
        match &self.key {
            Some(_) => self.get("search", &format!("{SITE}/api/v2/search/autocomplete/{}", path_segment(term))),
            None => self.get("search", &format!("{SITE}/api/public/search/autocomplete?term={}", crate::api::qs(term))),
        }
    }

    /// Portrait grids of a game, best rated first (`page` counts from 0).
    pub fn grids(&self, game: u64, page: u32) -> Result<GridPage> {
        let mut out = match &self.key {
            Some(_) => {
                let url =
                    format!("{SITE}/api/v2/grids/game/{game}?dimensions={}&types=static&nsfw=false&humor=false&epilepsy=false&page={page}", PORTRAIT.join(","));
                let grids: Vec<Grid> = self.get("grids", &url)?;
                // v2 does not say how many there are; a full page means "maybe more".
                let n = grids.len() as u32;
                GridPage { total: page * 50 + n + if n >= 50 { 1 } else { 0 }, grids, page, per_page: 50 }
            }
            None => {
                let body = serde_json::json!({
                    "asset_type": "grid", "game_id": [game], "order": "score_desc", "page": page, "dimensions": PORTRAIT,
                });
                let resp = self
                    .agent
                    .post(&format!("{SITE}/api/public/search/assets"))
                    .header("Content-Type", "application/json")
                    .send(body.to_string())
                    .map_err(|e| Error::Network(e.to_string()))?;
                let a: PublicAssets = Self::read("grids", resp)?;
                GridPage { grids: a.assets, total: a.total, page, per_page: a.limit.max(1) }
            }
        };
        out.grids.retain(Grid::suitable);
        Ok(out)
    }

    /// Download an image (only from SteamGridDB's hosts, at most 16 MiB).
    pub fn image(&self, url: &str) -> Result<Vec<u8>> {
        if !IMAGE_HOSTS.iter().any(|h| url.starts_with(h)) {
            return Err(Error::Other(format!("not a SteamGridDB image: {url}")));
        }
        let mut resp = self.agent.get(url).call().map_err(|e| Error::Network(e.to_string()))?;
        if resp.status().as_u16() != 200 {
            return Err(Error::Network(format!("{url}: HTTP {}", resp.status().as_u16())));
        }
        resp.body_mut().with_config().limit(16 * 1024 * 1024).read_to_vec().map_err(|e| Error::Network(e.to_string()))
    }
}

fn path_segment(s: &str) -> String {
    crate::api::qs(s).replace('+', "%20")
}

#[derive(Serialize, Deserialize)]
struct CachedList {
    fetched: u64,
    page: GridPage,
}

/// Grid lists and images on disk (`<home>/cache/griddb/`).
pub struct Cache {
    dir: PathBuf,
}

impl Default for Cache {
    fn default() -> Self {
        Cache { dir: crate::paths::home().join("cache").join("griddb") }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Cache {
    pub fn at(dir: PathBuf) -> Cache {
        Cache { dir }
    }

    fn list_file(&self, game: u64) -> PathBuf {
        self.dir.join(format!("grids-{game}.json"))
    }

    /// The first page of a game's grids: from the cache while it is fresh,
    /// else from SteamGridDB (falling back to a stale copy when offline).
    pub fn grids(&self, db: &GridDb, game: u64) -> Result<GridPage> {
        let cached: Option<CachedList> = std::fs::read(self.list_file(game)).ok().and_then(|b| serde_json::from_slice(&b).ok());
        if let Some(c) = &cached
            && now().saturating_sub(c.fetched) < LIST_TTL
        {
            return Ok(c.page.clone());
        }
        match db.grids(game, 0) {
            Ok(page) => {
                let _ = std::fs::create_dir_all(&self.dir);
                let _ = crate::paths::write_atomic(
                    &self.list_file(game),
                    &serde_json::to_vec(&CachedList { fetched: now(), page: page.clone() }).unwrap_or_default(),
                );
                Ok(page)
            }
            Err(e) => cached.map(|c| c.page).ok_or(e),
        }
    }

    /// The best grid of a game without asking the network (offline start).
    pub fn cached_default(&self, game: u64) -> Option<Grid> {
        let c: CachedList = serde_json::from_slice(&std::fs::read(self.list_file(game)).ok()?).ok()?;
        c.page.grids.into_iter().next()
    }

    /// Where an image URL is kept.
    pub fn image_path(&self, url: &str) -> PathBuf {
        use sha2::Digest;
        let h = hex::encode(&sha2::Sha256::digest(url.as_bytes())[..12]);
        let ext = url.rsplit('.').next().filter(|e| e.len() <= 4 && e.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or("img");
        self.dir.join(format!("img-{h}.{ext}"))
    }

    /// An image's bytes: from the cache, else downloaded and kept.
    pub fn image(&self, db: &GridDb, url: &str) -> Result<Vec<u8>> {
        let p = self.image_path(url);
        if let Ok(b) = std::fs::read(&p)
            && !b.is_empty()
        {
            return Ok(b);
        }
        let b = db.image(url)?;
        std::fs::create_dir_all(&self.dir)?;
        crate::paths::write_atomic(&p, &b)?;
        Ok(b)
    }

    /// The cached image, if it was downloaded before.
    pub fn cached_image(&self, url: &str) -> Option<Vec<u8>> {
        std::fs::read(self.image_path(url)).ok().filter(|b| !b.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(w: u32, h: u32) -> Grid {
        Grid {
            id: 1,
            url: "https://cdn2.steamgriddb.com/grid/x.png".into(),
            thumb: String::new(),
            width: w,
            height: h,
            style: "alternate".into(),
            mime: "image/png".into(),
            nsfw: false,
            humor: false,
            epilepsy: false,
            is_animated: false,
            author: None,
        }
    }

    #[test]
    fn only_static_safe_portraits_are_suitable() {
        assert!(grid(600, 900).suitable());
        assert!(!grid(920, 430).suitable());
        assert!(!Grid { nsfw: true, ..grid(600, 900) }.suitable());
        assert!(!Grid { humor: true, ..grid(600, 900) }.suitable());
        assert!(!Grid { is_animated: true, ..grid(600, 900) }.suitable());
        assert!(!Grid { mime: "image/gif".into(), ..grid(600, 900) }.suitable());
    }

    #[test]
    fn public_answer_parses() {
        let j = r#"{"success":true,"data":{"assets":[{"id":7,"style":"alternate","width":600,"height":900,"nsfw":false,"humor":false,
            "url":"https://cdn2.steamgriddb.com/grid/a.png","thumb":"https://cdn2.steamgriddb.com/thumb/a.jpg","mime":"image/png",
            "is_animated":false,"epilepsy":false,"author":{"name":"x","steam64":"1"}}],"total":71,"limit":48}}"#;
        let a: Answer<PublicAssets> = serde_json::from_str(j).unwrap();
        let d = a.data.unwrap();
        assert_eq!((d.total, d.limit, d.assets[0].id), (71, 48, 7));
        assert!(GridPage { grids: d.assets, total: 71, page: 0, per_page: 48 }.more());
    }

    #[test]
    fn images_only_from_steamgriddb() {
        let db = GridDb::new(None);
        assert!(db.image("https://example.org/x.png").is_err());
    }

    #[test]
    fn image_cache_paths_are_stable_and_keep_the_extension() {
        let c = Cache::at(PathBuf::from("/tmp/x"));
        let p = c.image_path("https://cdn2.steamgriddb.com/grid/a.webp");
        assert_eq!(p, c.image_path("https://cdn2.steamgriddb.com/grid/a.webp"));
        assert!(p.to_string_lossy().ends_with(".webp"));
    }

    #[test]
    fn labels_have_the_year() {
        let g = GameHit { id: 1, name: "Diablo".into(), release_date: Some(851990400), verified: true };
        assert_eq!(g.label(), "Diablo (1996)");
    }
}

/// Against the real SteamGridDB: `cargo test -p warpzone-core griddb -- --ignored`.
#[cfg(test)]
mod live {
    use super::*;

    #[test]
    #[ignore]
    fn public_endpoints_find_diablo_portraits() {
        let db = GridDb::new(None);
        let hits = db.search("Diablo").unwrap();
        assert!(hits.iter().any(|h| h.id == 34237), "{hits:?}");
        let page = db.grids(34237, 0).unwrap();
        assert!(!page.grids.is_empty() && page.grids.iter().all(Grid::suitable));
        let img = db.image(&page.grids[0].thumb).unwrap();
        assert!(img.len() > 1000);
    }
}
