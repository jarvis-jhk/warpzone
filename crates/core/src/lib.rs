//! # warpzone-core
//!
//! The client side of Warpzone, shared by the C library (`warpzone-capi`), the
//! CLI (`wz`) and the launcher. Games written in Rust can use it directly.
//!
//! | Module | What |
//! |---|---|
//! | [`paths`] | the shared data directory (identity, mods, installed games) |
//! | [`identity`] | the device's Ed25519 key, recovery codes |
//! | [`api`] | blocking HTTP client for every API route, with automatic login |
//! | [`net`] | UDP: STUN, hole punching, relay fallback, encrypted peer channels |
//! | [`client`] | the game-facing session: rooms, peers, ladder, events |
//! | [`modstore`] | installing lockfiles, validating enabled mods |
//! | [`gamedata`] | finding the player's original game data (Steam, GOG, Downloads, ...) |
//! | [`griddb`] | SteamGridDB artwork, looked up at run time and cached |
//! | [`install`] | installing and updating games from their releases |
//! | [`template`] | tier-0 launch-file templates |
//! | [`tunnel`] | tier-0 tunnel: a game's own UDP traffic over Warpzone |
//! | [`launch`] | how a game learns that the launcher started it |
//! | [`feedback`] | crash, problem and idea reports |
//!
//! Threads: [`client::Client`] runs two service threads (HTTP requests and the
//! event long-poll). Peer traffic is processed on the caller's thread in
//! [`client::Client::poll`]; nothing ever calls back into game code.

pub mod api;
pub mod client;
pub mod feedback;
pub mod gamedata;
pub mod griddb;
pub mod identity;
pub mod install;
pub mod launch;
pub mod modstore;
pub mod net;
pub mod paths;
pub mod template;
pub mod tunnel;

pub use warpzone_protocol as protocol;

/// The instance games and tools use unless configured otherwise
/// (`WARPZONE_SERVER` at run time). Ecosystems that run their own instance set
/// it when building: `WARPZONE_DEFAULT_SERVER=https://play.example.org cargo build`.
pub const DEFAULT_SERVER: &str = match option_env!("WARPZONE_DEFAULT_SERVER") {
    Some(s) => s,
    None => "https://warpzone.virtualtabletop.io",
};

/// Agent configuration for every HTTP client of this crate: TLS uses the crypto
/// provider the build chose (feature `ring`, the default, or
/// `pure-rust-crypto`, which wins when both are on).
pub fn http_config() -> ureq::config::ConfigBuilder<ureq::typestate::AgentScope> {
    let config = ureq::Agent::config_builder();
    #[cfg(feature = "pure-rust-crypto")]
    let config =
        config.tls_config(ureq::tls::TlsConfig::builder().unversioned_rustls_crypto_provider(std::sync::Arc::new(rustls_rustcrypto::provider())).build());
    config
}

#[cfg(not(any(feature = "ring", feature = "pure-rust-crypto")))]
compile_error!("warpzone-core needs a TLS crypto provider: feature `ring` (default) or `pure-rust-crypto`");

/// Server URL from `WARPZONE_SERVER`, else [`DEFAULT_SERVER`].
pub fn server_url() -> String {
    std::env::var("WARPZONE_SERVER").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The server answered with an error body.
    #[error("{message}")]
    Api { status: u16, code: String, message: String },
    #[error("network: {0}")]
    Network(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn code(&self) -> &str {
        match self {
            Error::Api { code, .. } => code,
            Error::Network(_) => "network",
            Error::Io(_) => "io",
            Error::Other(_) => "other",
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
