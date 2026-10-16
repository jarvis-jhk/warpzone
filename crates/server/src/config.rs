//! `server.toml` — see `deploy/config/server.toml` for a commented example and
//! `docs/self-hosting.md` for every option.

use serde::Deserialize;
use std::path::PathBuf;
use warpzone_protocol::descriptor::Install;

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Display name of the instance.
    #[serde(default = "default_name")]
    pub name: String,
    /// Public base URL, without trailing slash.
    pub public_url: String,
    /// HTTP listener (put a TLS reverse proxy in front of it).
    #[serde(default = "default_http")]
    pub http_listen: String,
    /// UDP listener for STUN, punching and relay. `[::]:3478` is dual-stack.
    #[serde(default = "default_udp")]
    pub udp_listen: String,
    /// Public UDP addresses clients should use (one per address family).
    /// Empty: derived from `public_url`'s host and the UDP port.
    #[serde(default)]
    pub udp_public: Vec<String>,
    /// Database, server key, mod archives and replays.
    pub data_dir: PathBuf,
    /// Directory of game descriptors (`<id>.toml`), re-read on change.
    pub games_dir: PathBuf,
    /// Staff keys (`moderators`, `reviewers`), re-read on change.
    pub staff_file: PathBuf,
    /// Optional URL that gets a plain-text POST for every mod submission and
    /// report (an ntfy topic works).
    #[serde(default)]
    pub notify_url: Option<String>,
    /// Use `X-Forwarded-For` for the client address (only behind a proxy you
    /// control).
    #[serde(default)]
    pub trust_forwarded_for: bool,
    /// Releases of the launcher, for the website's download buttons.
    #[serde(default)]
    pub launcher: Option<Install>,
    /// Where the website links for documentation and source.
    #[serde(default = "default_source")]
    pub source_url: String,
    /// Optional legal/contact page linked from every page footer.
    #[serde(default)]
    pub legal_url: Option<String>,
    #[serde(default)]
    pub limits: LimitsConfig,
    /// Optional: the server's own faults (500s, failed background jobs,
    /// panics) as JSON POSTs.
    #[serde(default)]
    pub fault_reports: Option<FaultReports>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct FaultReports {
    /// Gets `{"lane","kind":"bug","title","detail","source"}` per fault.
    pub url: String,
    /// Passed through as `lane`, for receivers that sort reports.
    #[serde(default = "default_lane")]
    pub lane: String,
}

fn default_lane() -> String {
    "warpzone".into()
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, default)]
pub struct LimitsConfig {
    pub max_room_players: u32,
    pub max_metadata_bytes: u32,
    pub rooms_per_account: u32,
    /// Total size of all stored mod archives, GiB.
    pub mod_store_gb: u64,
    /// Relay bandwidth per channel and direction, kbit/s.
    pub relay_kbps: u64,
    /// Relay bandwidth for the whole server, Mbit/s.
    pub relay_total_mbps: u64,
    pub registrations_per_ip_per_day: u32,
    pub logins_per_ip_per_minute: u32,
    pub code_guesses_per_ip_per_minute: u32,
    pub uploads_per_account_per_day: u32,
    pub reports_per_account_per_day: u32,
    pub max_replay_kb: u64,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        LimitsConfig {
            max_room_players: 64,
            max_metadata_bytes: 1024,
            rooms_per_account: 3,
            mod_store_gb: 10,
            relay_kbps: 2000,
            relay_total_mbps: 100,
            registrations_per_ip_per_day: 5,
            logins_per_ip_per_minute: 30,
            code_guesses_per_ip_per_minute: 10,
            uploads_per_account_per_day: 10,
            reports_per_account_per_day: 20,
            max_replay_kb: 512,
        }
    }
}

fn default_name() -> String {
    "Warpzone".into()
}

fn default_http() -> String {
    "127.0.0.1:2998".into()
}

fn default_udp() -> String {
    "[::]:3478".into()
}

fn default_source() -> String {
    "https://github.com/jarvis-jhk/warpzone".into()
}

impl Config {
    pub fn load(path: &std::path::Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut c: Config = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        c.public_url = c.public_url.trim_end_matches('/').to_string();
        // Relative paths are relative to the config file.
        let base = path.parent().unwrap_or(std::path::Path::new("."));
        for p in [&mut c.data_dir, &mut c.games_dir, &mut c.staff_file] {
            if p.is_relative() {
                *p = base.join(&*p);
            }
        }
        Ok(c)
    }
}

/// `staff.toml`: who may moderate. Keys are device public keys (hex).
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct Staff {
    /// Approve/reject mods, close reports, rename accounts.
    #[serde(default)]
    pub moderators: Vec<String>,
    /// Read the queue and add notes (e.g. an automated pre-review).
    #[serde(default)]
    pub reviewers: Vec<String>,
    /// Substrings no username may contain (case-insensitive). Kept in this
    /// private file rather than in the source tree.
    #[serde(default)]
    pub blocked_words: Vec<String>,
}
