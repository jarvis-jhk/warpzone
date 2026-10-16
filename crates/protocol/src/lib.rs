//! Shared vocabulary of the Warpzone ecosystem.
//!
//! Everything the server, the client library, the CLI and the launcher must agree
//! on lives here and nowhere else:
//!
//! * [`api`] — JSON request/response bodies of the HTTP API (`/api/v1/...`).
//! * [`descriptor`] — the per-game descriptor files (`games/<id>.toml`).
//! * [`ticket`] — server-signed join tickets that peers show each other.
//! * [`wire`] — the UDP datagram formats (STUN, relay bind/data).
//! * [`mods`] — mod manifests, Factorio-style dependencies, the resolver,
//!   lockfiles and offline validation.
//! * [`names`] — validation of usernames, ids, join codes and language tags.
//!
//! The crate is pure logic: no I/O, no threads, no async.

pub mod api;
pub mod descriptor;
pub mod mods;
pub mod names;
pub mod ticket;
pub mod wire;

/// Version of the HTTP API. Bumped only for incompatible changes; the path
/// prefix carries it (`/api/v1`).
pub const API_VERSION: u32 = 1;

/// Path prefix of every API route.
pub const API_PREFIX: &str = "/api/v1";

/// Default UDP port for STUN, hole-punch coordination and the relay.
pub const DEFAULT_UDP_PORT: u16 = 3478;

/// Largest payload a game may send in one datagram through the client library.
/// Chosen so that payload + encryption + relay framing stays below a 1280-byte
/// IPv6 minimum MTU.
pub const MAX_DATAGRAM: usize = 1100;

/// Current time in seconds since the Unix epoch.
pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
