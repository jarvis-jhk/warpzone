//! Game descriptors: one TOML file per game (`games/<id>.toml`).
//!
//! The descriptor is everything the server, the launcher and the website know
//! about a game. The server does not know what a game *does* with rooms, mods
//! or scores; the descriptor only says which features the game uses, what its
//! mod categories are called, how to install and launch it, and which room
//! metadata keys the browser should show. Adding a game — ours or a
//! third-party one — means adding a descriptor; see `docs/descriptors.md`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GameDescriptor {
    /// Stable id, also the file name (`funny-furries`).
    pub id: String,
    pub name: String,
    /// One line for cards.
    pub tagline: String,
    /// A paragraph for the game page (plain text).
    #[serde(default)]
    pub description: String,
    /// Project page (source repository).
    #[serde(default)]
    pub homepage: Option<String>,
    /// Card artwork (absolute URL or a path under the site's `/static/`).
    #[serde(default)]
    pub art: Option<String>,
    /// Accent colour for cards (`#rrggbb`).
    #[serde(default = "default_accent")]
    pub accent: String,
    /// Integration tier the game currently implements (0–3, `docs/integration.md`).
    #[serde(default)]
    pub tier: u8,
    /// `true` for games not made by this project; shown with a badge.
    #[serde(default)]
    pub third_party: bool,
    /// Development status shown on the card (`preview`, `in development`, ...).
    #[serde(default)]
    pub status: Option<String>,
    /// What the player must bring (original game data, ROM, ...). Shown before
    /// installing.
    #[serde(default)]
    pub requires: Option<String>,
    /// Hidden from the website and launcher (still served by id).
    #[serde(default)]
    pub hidden: bool,
    /// Players on one machine with their own controllers (couch play); shown
    /// as the controller icon. `None` = not stated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_players: Option<u32>,
    #[serde(default)]
    pub install: Option<Install>,
    #[serde(default)]
    pub launch: Option<Launch>,
    #[serde(default)]
    pub rooms: Option<Rooms>,
    #[serde(default)]
    pub mods: Option<Mods>,
    #[serde(default)]
    pub ladder: Vec<LadderQueue>,
    #[serde(default)]
    pub leaderboards: Vec<Leaderboard>,
    /// Season length for this game's ladder: `monthly`, `quarterly` (default)
    /// or `yearly`.
    #[serde(default = "default_season")]
    pub season: String,
    #[serde(default)]
    pub tier0: Option<Tier0>,
    /// Original game data the player must provide; the launcher searches for
    /// it and hands it to the game (`docs/descriptors.md#game-data`).
    #[serde(default)]
    pub data: Vec<DataRequirement>,
    /// "Add to Steam": name and artwork for the non-Steam shortcut.
    #[serde(default)]
    pub steam: Option<SteamShortcut>,
    /// Banner motif for the launcher's procedural tile when there is no
    /// `art`: `arena`, `blocks`, `castle`, `crypt`, `pipes`, `tanks`, `waves`.
    #[serde(default)]
    pub motif: Option<String>,
    /// The game's id on SteamGridDB (`steamgriddb.com/game/<id>`): the
    /// launcher looks up portrait art there at run time and caches it. Only
    /// the id is stored; no artwork is shipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steamgriddb: Option<u64>,
}

/// How the game appears in Steam when the launcher adds it as a non-Steam
/// game. Missing images are rendered from the launcher's tile artwork.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct SteamShortcut {
    /// Name in the Steam library (default: the game's `name`).
    #[serde(default)]
    pub name: Option<String>,
    /// 600×900 portrait capsule (PNG/JPEG URL).
    #[serde(default)]
    pub capsule: Option<String>,
    /// 920×430 wide capsule.
    #[serde(default)]
    pub wide: Option<String>,
    /// 1920×620 hero banner.
    #[serde(default)]
    pub hero: Option<String>,
    /// Transparent logo drawn over the hero.
    #[serde(default)]
    pub logo: Option<String>,
    /// Square icon.
    #[serde(default)]
    pub icon: Option<String>,
}

/// Original data a game needs and how the launcher finds and hands it over.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DataRequirement {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    /// What identifies a usable source, case-insensitive globs. A plain name
    /// matches a file (`DIABDAT.MPQ`, `setup_the_settlers_2_gold_*.exe`); a
    /// pattern with `/` matches a directory containing that path
    /// (`DATA/IO/IO.DAT` finds the folder holding `DATA`); `./d2exp.mpq`
    /// matches the folder that directly contains the file.
    pub files: Vec<String>,
    /// Folder names under Steam's `steamapps/common`.
    #[serde(default)]
    pub steam: Vec<String>,
    /// Folder names of GOG installs (GOG Galaxy, Heroic, `~/GOG Games`).
    #[serde(default)]
    pub gog: Vec<String>,
    /// Also look in the Downloads folder.
    #[serde(default = "yes")]
    pub downloads: bool,
    /// Also look on mounted discs and images.
    #[serde(default = "yes")]
    pub discs: bool,
    /// Where to buy it.
    #[serde(default)]
    pub buy: Vec<Link>,
    /// Legal free sources (shareware, freeware releases).
    #[serde(default)]
    pub free: Vec<Link>,
    /// Free downloads the launcher may fetch by itself, tried in order (the
    /// same file from several places, or alternatives). Only for data whose
    /// licence allows it (freeware that may be passed on unchanged); every
    /// entry is pinned by size and SHA-256, so a changed file is never used.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fetch: Vec<Fetch>,
    /// Hand-over: run the game once with these arguments (`{path}` = what was
    /// found) — preferred, the game's own importer validates it.
    #[serde(default)]
    pub import_args: Vec<String>,
    /// Or copy what was found here (`{install}` = the game's install
    /// directory, `~` = home). A directory match copies the directory's contents.
    #[serde(default)]
    pub copy_to: Option<String>,
    /// Path (same placeholders) whose existence means the data is already in
    /// place, so the launcher does not ask again.
    #[serde(default)]
    pub present: Vec<String>,
    /// Nice to have, not needed to play (e.g. a ROM that only adds sound): the
    /// launcher still looks for it and hands it over, but never blocks on it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub optional: bool,
    /// Fingerprints of known-good files. When present, the launcher checks a
    /// found file before anything is downloaded: verified, unknown or wrong.
    #[serde(default)]
    pub known: Vec<KnownFile>,
}

/// A known-good file: for a file match the file itself (`file` empty), for a
/// directory match the file at `file` inside it. Size is compared first, so
/// large files are only hashed when the size already fits.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KnownFile {
    /// What this fingerprint is ("World, Rev A (v1.1)", "GOG installer 2.0").
    pub label: String,
    #[serde(default)]
    pub file: String,
    pub size: u64,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    /// `false` for versions the game does not accept (shown as "wrong version").
    #[serde(default = "yes")]
    pub supported: bool,
}

/// One file the launcher downloads by itself. What it holds is found like a
/// download the player made (zips are looked into), so `files` still decides.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fetch {
    /// Where it comes from, shown to the player ("DOS Games Archive").
    pub label: String,
    /// https URL of the file itself (redirects are followed).
    pub url: String,
    /// Saved under this name (`sh_dos.zip`).
    pub file: String,
    pub size: u64,
    pub sha256: String,
    /// Why it may be downloaded: the licence, quoted or named. Shown to the player.
    pub license: String,
}

/// Largest file a `[[data]] fetch` may name.
pub const MAX_FETCH: u64 = 512 * 1024 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub label: String,
    pub url: String,
}

fn yes() -> bool {
    true
}

fn default_accent() -> String {
    "#7c5cff".into()
}

fn default_season() -> String {
    "quarterly".into()
}

/// Where releases come from.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Install {
    /// `owner/repo` on GitHub.
    pub github: String,
    /// Offer prereleases when there is no full release yet.
    #[serde(default)]
    pub prerelease: bool,
    /// Platform → asset name pattern (`*` matches anything, e.g.
    /// `s25coop-*-linux-x86_64.tar.gz`). Platforms: `linux`, `appimage`
    /// (self-contained Linux build), `windows`, `deck`, `portmaster`.
    pub assets: BTreeMap<String, String>,
    /// Name of the checksum file in the release (`SHA256SUMS`), if any.
    #[serde(default)]
    pub checksums: Option<String>,
    /// minisign public key; when set, the launcher requires `<asset>.minisig`.
    #[serde(default)]
    pub minisign_key: Option<String>,
    /// How the asset is installed: `extract` (tar.gz/zip, default) or `file`.
    #[serde(default = "default_unpack")]
    pub unpack: String,
}

fn default_unpack() -> String {
    "extract".into()
}

/// How the launcher starts the game. Paths are relative to the install
/// directory; `*` in a path segment matches one directory (release archives
/// often contain a versioned top-level folder).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    /// Platform → executable.
    #[serde(default)]
    pub exec: BTreeMap<String, String>,
    /// Extra arguments for a plain start.
    #[serde(default)]
    pub args: Vec<String>,
    /// Arguments to join a room; `{room}` is replaced (tier ≥ 1).
    #[serde(default)]
    pub join: Vec<String>,
    /// Arguments to open the game's host screen (tier ≥ 2).
    #[serde(default)]
    pub host: Vec<String>,
    /// Arguments that run the game's own first-start data import, if it has one.
    #[serde(default)]
    pub import: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rooms {
    pub max_players: u32,
    /// Presets name a room (`"Alex's 2v2"`) instead of free text.
    #[serde(default)]
    pub presets: Vec<Preset>,
    /// Metadata keys the browser shows, in order.
    #[serde(default)]
    pub display: Vec<DisplayField>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub id: String,
    pub label: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DisplayField {
    pub key: String,
    pub label: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Mods {
    pub categories: Vec<Category>,
    /// Largest archive accepted, in MiB.
    #[serde(default = "default_mod_mb")]
    pub max_size_mb: u32,
    /// Whether mods can contain code (scripts, DLLs). Shown as a warning.
    #[serde(default)]
    pub contains_code: bool,
    /// How the client stores a mod: `archive` (keep the file, default) or
    /// `extract` (unpack into the version directory).
    #[serde(default = "default_mod_install")]
    pub install: String,
    /// Tier-0 games: category → directory (relative to the game's data
    /// directory) the launcher links active mods into.
    #[serde(default)]
    pub mounts: BTreeMap<String, String>,
}

fn default_mod_mb() -> u32 {
    50
}

fn default_mod_install() -> String {
    "archive".into()
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Category {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LadderQueue {
    pub id: String,
    pub title: String,
    /// Number of teams per match (2 for 1v1 and 2v2, N for free-for-all).
    pub teams: u32,
    /// Players per team.
    #[serde(default = "one")]
    pub team_size: u32,
}

fn one() -> u32 {
    1
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Leaderboard {
    pub id: String,
    pub title: String,
    /// `time` (milliseconds, lower wins) or `score` (higher wins).
    pub kind: String,
    /// Also keep per-season boards (all-time is always kept).
    #[serde(default)]
    pub seasonal: bool,
}

/// Integration tier 0: the launcher runs the lobby, tunnels the game's own
/// traffic over Warpzone and starts the game on every machine.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tier0 {
    /// `udp` or `tcp`.
    pub transport: String,
    /// The port the game itself uses on every machine.
    pub game_port: u16,
    /// Settings the host picks in the launcher before Start.
    #[serde(default)]
    pub settings: Vec<Tier0Setting>,
    /// Files written before the game starts. Key = path relative to the game's
    /// data directory; value = template (`docs/descriptors.md#templates`).
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    /// Arguments for the staged start.
    #[serde(default)]
    pub args: Vec<String>,
    /// For games that restart themselves (a new process per mission): seconds
    /// the tunnel and room stay up after the launched process exits, renewed
    /// while the game port keeps sending. 0 = the session ends with the process.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub relaunch_linger: u32,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tier0Setting {
    pub key: String,
    pub label: String,
    pub options: Vec<String>,
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DescriptorError {
    #[error("{0}")]
    Invalid(String),
}

impl GameDescriptor {
    /// Consistency checks beyond what serde enforces.
    pub fn validate(&self) -> Result<(), DescriptorError> {
        use crate::names::{valid_game_id, valid_slug};
        let bad = |m: String| Err(DescriptorError::Invalid(format!("{}: {m}", self.id)));
        if !valid_game_id(&self.id) {
            return bad("invalid id".into());
        }
        if self.tier > 3 {
            return bad("tier must be 0..=3".into());
        }
        if self.local_players.is_some_and(|n| n == 0 || n > 64) {
            return bad("local_players must be 1..=64".into());
        }
        if !matches!(self.season.as_str(), "monthly" | "quarterly" | "yearly") {
            return bad(format!("unknown season length {}", self.season));
        }
        if let Some(i) = &self.install {
            for p in i.assets.keys() {
                if !matches!(p.as_str(), "linux" | "appimage" | "windows" | "deck" | "portmaster") {
                    return bad(format!("unknown platform {p}"));
                }
            }
            if !i.github.contains('/') {
                return bad("install.github must be owner/repo".into());
            }
        }
        if let Some(r) = &self.rooms {
            if r.max_players < 2 || r.max_players > 64 {
                return bad("rooms.max_players must be 2..=64".into());
            }
            for p in &r.presets {
                if !valid_slug(&p.id) {
                    return bad(format!("bad preset id {}", p.id));
                }
            }
        }
        if let Some(m) = &self.mods {
            if m.categories.is_empty() {
                return bad("mods needs at least one category".into());
            }
            for c in &m.categories {
                if !valid_slug(&c.id) {
                    return bad(format!("bad category id {}", c.id));
                }
            }
            if !matches!(m.install.as_str(), "archive" | "extract") {
                return bad("mods.install must be archive or extract".into());
            }
        }
        for q in &self.ladder {
            if !valid_slug(&q.id) || q.teams < 2 || q.team_size < 1 || q.teams * q.team_size > 16 {
                return bad(format!("bad ladder queue {}", q.id));
            }
        }
        for b in &self.leaderboards {
            if !valid_slug(&b.id) || !matches!(b.kind.as_str(), "time" | "score") {
                return bad(format!("bad leaderboard {}", b.id));
            }
        }
        if let Some(t) = &self.tier0
            && !matches!(t.transport.as_str(), "udp" | "tcp")
        {
            return bad("tier0.transport must be udp or tcp".into());
        }
        if let Some(t) = &self.tier0
            && t.relaunch_linger > 3600
        {
            return bad("tier0.relaunch_linger is at most 3600 seconds".into());
        }
        for d in &self.data {
            if !valid_slug(&d.id) || d.files.is_empty() {
                return bad(format!("data {}: needs an id and file patterns", d.id));
            }
            for l in d.buy.iter().chain(d.free.iter()) {
                if !l.url.starts_with("https://") {
                    return bad(format!("data {}: links must be https", d.id));
                }
            }
            for f in &d.fetch {
                if !f.url.starts_with("https://") {
                    return bad(format!("data {}: fetch urls must be https", d.id));
                }
                let name_ok = !f.file.is_empty()
                    && f.file.len() <= 100
                    && !f.file.starts_with('.')
                    && f.file.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
                if !name_ok {
                    return bad(format!("data {}: fetch file must be a plain file name", d.id));
                }
                if f.size == 0 || f.size > MAX_FETCH {
                    return bad(format!("data {}: fetch size must be 1 byte to {} MiB", d.id, MAX_FETCH >> 20));
                }
                if f.sha256.len() != 64 || !f.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
                    return bad(format!("data {}: fetch needs a sha256", d.id));
                }
                if f.license.trim().is_empty() {
                    return bad(format!("data {}: fetch needs the licence that allows it", d.id));
                }
            }
        }
        Ok(())
    }

    /// Features this game uses, for badges and `ServerInfo`-style listings.
    pub fn features(&self) -> Vec<&'static str> {
        let mut f = Vec::new();
        if self.rooms.is_some() {
            f.push("online");
        }
        if !self.ladder.is_empty() {
            f.push("ranked");
        }
        if !self.leaderboards.is_empty() {
            f.push("leaderboards");
        }
        if self.mods.is_some() {
            f.push("mods");
        }
        f
    }

    /// What the game can do today, for icons: (players online, mods usable,
    /// ranked usable). Online needs tier 2+ (or a launcher lobby), mods tier 3
    /// (or tier-0 mounts), ranked tier 2+. Declared-but-not-yet-supported
    /// features come back as `planned`.
    pub fn capabilities(&self) -> Capabilities {
        let online_now = self.tier >= 2 || self.tier0.is_some();
        let mods_now = self.tier >= 3 || (self.tier0.is_some() && self.mods.as_ref().is_some_and(|m| !m.mounts.is_empty()));
        Capabilities {
            local_players: self.local_players,
            online_players: self.rooms.as_ref().map(|r| r.max_players),
            online_now,
            mods: self.mods.is_some(),
            mods_now,
            ladder: !self.ladder.is_empty(),
            ladder_now: self.tier >= 2,
        }
    }

    pub fn preset_label(&self, id: &str) -> Option<&str> {
        self.rooms.as_ref()?.presets.iter().find(|p| p.id == id).map(|p| p.label.as_str())
    }
}

/// See [`GameDescriptor::capabilities`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Capabilities {
    pub local_players: Option<u32>,
    pub online_players: Option<u32>,
    pub online_now: bool,
    pub mods: bool,
    pub mods_now: bool,
    pub ladder: bool,
    pub ladder_now: bool,
}

/// Glob match with `*` only (asset patterns, executable paths).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(p) => rest = &rest[p + part.len()..],
                None => return false,
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("s25coop-*-linux-x86_64.tar.gz", "s25coop-0.1.19-linux-x86_64.tar.gz"));
        assert!(!glob_match("s25coop-*-linux-x86_64.tar.gz", "s25coop-0.1.19-windows-x64.zip"));
        assert!(glob_match("SHA256SUMS", "SHA256SUMS"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn parses_minimal_and_full() {
        let d: GameDescriptor = toml::from_str(
            r#"
            id = "demo"
            name = "Demo"
            tagline = "A demo"
            tier = 3
            [install]
            github = "o/r"
            assets = { linux = "demo-*-linux.tar.gz" }
            [rooms]
            max_players = 4
            presets = [{ id = "ffa", label = "Free for all" }]
            [mods]
            categories = [{ id = "level", label = "Levels" }]
            [[ladder]]
            id = "1v1"
            title = "1 vs 1"
            teams = 2
            [[leaderboards]]
            id = "any"
            title = "Any%"
            kind = "time"
            "#,
        )
        .unwrap();
        d.validate().unwrap();
        assert_eq!(d.features(), vec!["online", "ranked", "leaderboards", "mods"]);
        assert_eq!(d.preset_label("ffa"), Some("Free for all"));
    }

    #[test]
    fn relaunch_linger_is_optional_and_not_serialized_when_off() {
        let src =
            |extra: &str| format!("id = \"demo\"\nname = \"Demo\"\ntagline = \"A demo\"\ntier = 0\n[tier0]\ntransport = \"udp\"\ngame_port = 1234\n{extra}");
        let off: GameDescriptor = toml::from_str(&src("")).unwrap();
        assert_eq!(off.tier0.as_ref().unwrap().relaunch_linger, 0);
        // Launchers before 0.1.2 reject unknown fields: leave it out unless set.
        assert!(!serde_json::to_string(&off).unwrap().contains("relaunch_linger"));
        let on: GameDescriptor = toml::from_str(&src("relaunch_linger = 120")).unwrap();
        on.validate().unwrap();
        assert_eq!(on.tier0.unwrap().relaunch_linger, 120);
        let long: GameDescriptor = toml::from_str(&src("relaunch_linger = 7200")).unwrap();
        assert!(long.validate().is_err());
    }

    /// Every descriptor the project ships parses, validates and is named by its id
    /// (the server skips one that does not, which drops the game from the site).
    #[test]
    fn shipped_descriptors_are_valid() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games");
        let mut n = 0;
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "toml") {
                continue;
            }
            let d: GameDescriptor = toml::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            d.validate().unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            assert_eq!(p.file_stem().unwrap(), d.id.as_str(), "{}", p.display());
            n += 1;
        }
        assert!(n > 0);
    }
}
