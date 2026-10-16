//! `launcher.json` in the shared data directory.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Settings {
    /// `None` until the player answered the first-start question.
    pub background: Option<bool>,
    /// Update installed games automatically (in the background when the
    /// launcher runs, and before every start).
    pub auto_update: bool,
    /// Games the player hid from the library.
    pub hidden_games: Vec<String>,
    /// Folders the player pointed the data search at.
    pub data_folders: Vec<String>,
    /// Last version of each game the player saw the changelog for.
    pub seen_versions: BTreeMap<String, String>,
    /// Server URL override (empty = default).
    pub server: String,
    /// Show the "x" button as "hide to background" hint once.
    pub hinted_background: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            background: None,
            auto_update: true,
            hidden_games: Vec::new(),
            data_folders: Vec::new(),
            seen_versions: BTreeMap::new(),
            server: String::new(),
            hinted_background: false,
        }
    }
}

impl Settings {
    pub fn load() -> Settings {
        std::fs::read_to_string(warpzone_core::paths::launcher_settings()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = warpzone_core::paths::write_atomic(&warpzone_core::paths::launcher_settings(), &serde_json::to_vec_pretty(self).unwrap());
    }

    pub fn server(&self) -> String {
        if self.server.trim().is_empty() { warpzone_core::server_url() } else { self.server.trim().to_string() }
    }
}
