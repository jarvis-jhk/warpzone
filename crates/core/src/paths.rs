//! The shared data directory. Every Warpzone program of one user — the
//! launcher, the CLI and every game using the library — uses the same one, so
//! a player has one identity and mods are downloaded once.
//!
//! | Platform | Default |
//! |---|---|
//! | Linux, Steam Deck | `$XDG_DATA_HOME/warpzone`, else `~/.local/share/warpzone` |
//! | Windows | `%APPDATA%\warpzone` |
//! | macOS | `~/Library/Application Support/warpzone` |
//!
//! `WARPZONE_HOME` overrides it (tests, portable installs).
//!
//! ```text
//! <home>/identity.key            device key (hex seed, mode 0600)
//! <home>/mods/<game>/<mod>/<version>/   installed mod versions
//! <home>/mods/<game>/active.lock.json   the active mod set
//! <home>/games/<game>/<version>/        games installed by the launcher
//! <home>/games/<game>/current           version that runs
//! <home>/launcher.json                  launcher settings
//! ```

use std::path::PathBuf;

pub fn home() -> PathBuf {
    if let Some(h) = std::env::var_os("WARPZONE_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(h);
    }
    #[cfg(windows)]
    {
        if let Some(a) = std::env::var_os("APPDATA") {
            return PathBuf::from(a).join("warpzone");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(h) = std::env::var_os("HOME") {
            return PathBuf::from(h).join("Library/Application Support/warpzone");
        }
    }
    if let Some(x) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(x).join("warpzone");
    }
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h).join(".local/share/warpzone");
    }
    PathBuf::from(".warpzone")
}

pub fn identity_file() -> PathBuf {
    home().join("identity.key")
}

pub fn mods_dir(game: &str) -> PathBuf {
    home().join("mods").join(game)
}

pub fn games_dir() -> PathBuf {
    home().join("games")
}

pub fn game_dir(game: &str) -> PathBuf {
    games_dir().join(game)
}

pub fn launcher_settings() -> PathBuf {
    home().join("launcher.json")
}

/// Write `data` to `path` atomically (temporary file + rename).
pub fn write_atomic(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}
