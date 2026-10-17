//! JSON bodies of the HTTP API. Every route is documented in `docs/api.md`;
//! the types here are the contract.
//!
//! Conventions: ids are strings unless they are database row ids (`i64`),
//! keys and hashes are lower-case hex, times are Unix seconds, optional fields
//! are omitted rather than `null` where the server writes them.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Body of every non-2xx response.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ErrorBody {
    /// Stable machine-readable code (`not_found`, `rate_limited`, ...).
    pub error: String,
    /// Human-readable explanation, safe to show to a player.
    pub message: String,
}

// ---------------------------------------------------------------- server info

/// `GET /api/v1/info` — what a client needs before anything else.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerInfo {
    /// Display name of this Warpzone instance ("Warpzone").
    pub name: String,
    /// Server software version.
    pub version: String,
    /// Ed25519 public key (hex) that signs join tickets and the catalog.
    pub server_key: String,
    /// Public base URL (`https://warpzone.example.org`).
    pub public_url: String,
    /// UDP endpoints for STUN, punching and relay (`203.0.113.7:3478`,
    /// `[2001:db8::1]:3478`). May list one address per family.
    pub udp: Vec<String>,
    /// Enabled features: `rooms`, `relay`, `ladder`, `leaderboards`, `mods`.
    pub features: Vec<String>,
    pub limits: Limits,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Limits {
    pub max_room_players: u32,
    pub max_metadata_bytes: u32,
    pub max_datagram: u32,
    pub max_mod_mb: u32,
}

/// `GET /endpoints.json` (and the signed copy in the repository): where a
/// client finds the service. Lets an instance move without breaking old
/// clients.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Endpoints {
    pub api: String,
    pub udp: Vec<String>,
}

// ---------------------------------------------------------------- auth

/// `POST /api/v1/auth/challenge`
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChallengeRequest {
    /// Ed25519 public key of the device (hex, 32 bytes).
    pub key: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChallengeResponse {
    pub nonce: String,
    pub expires_in: u64,
}

/// `POST /api/v1/auth/login` — the signature is over [`login_message`].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LoginRequest {
    pub key: String,
    pub nonce: String,
    pub signature: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LoginResponse {
    /// Bearer token for `Authorization: Bearer ...`.
    pub token: String,
    pub expires_in: u64,
    /// The account this key belongs to, if it has registered one.
    pub account: Option<Account>,
    /// `moderator` and/or `reviewer` for staff keys.
    pub roles: Vec<String>,
}

/// The exact bytes a device signs to log in. Binding the server key in stops a
/// signature for one instance from being replayed against another.
pub fn login_message(server_key: &str, nonce: &str) -> Vec<u8> {
    format!("warpzone-login-v1\n{server_key}\n{nonce}").into_bytes()
}

// ---------------------------------------------------------------- accounts

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Account {
    pub id: i64,
    pub username: String,
    pub created: i64,
}

/// `POST /api/v1/accounts` — claim a username for the logged-in key.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegisterRequest {
    pub username: String,
}

/// `POST /api/v1/accounts/me/link-code` — code to add another device.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LinkCodeResponse {
    pub code: String,
    pub expires_in: u64,
}

/// `POST /api/v1/accounts/link` — the new device redeems the code.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LinkRequest {
    pub code: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DeviceKey {
    pub key: String,
    pub added: i64,
}

/// `GET /api/v1/accounts/{username}` — public profile.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Profile {
    pub username: String,
    pub created: i64,
    pub ratings: Vec<ProfileRating>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProfileRating {
    pub game: String,
    pub queue: String,
    pub season: String,
    pub rating: i32,
    pub games: u32,
    pub wins: u32,
    pub placing: bool,
}

// ---------------------------------------------------------------- rooms

/// Who can see a room in the browser.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    /// Listed for everyone.
    #[default]
    Public,
    /// Not listed; reachable by join code or invite link only.
    Unlisted,
}

/// How a player gets into a room once they found it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Entry {
    /// Anyone who sees it can join.
    #[default]
    Open,
    /// The 6-character join code is required.
    Code,
    /// The joiner knocks; the host accepts or declines.
    Approve,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Skill {
    Beginners,
    #[default]
    Mixed,
    Experienced,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    #[default]
    Casual,
    Serious,
    Learning,
}

/// The room's target audience. Structured on purpose: Warpzone has no chat
/// and no free-text room names, so there is nothing to moderate here.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Audience {
    /// ISO 639-1 codes; empty means "any language".
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub skill: Skill,
    #[serde(default)]
    pub style: Style,
}

/// `POST /api/v1/rooms`
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CreateRoom {
    pub game: String,
    pub game_version: String,
    /// Lockfile hash of the mod set everybody must run; `None` = no mods.
    #[serde(default)]
    pub lockfile: Option<String>,
    pub max_players: u32,
    #[serde(default)]
    pub visibility: Visibility,
    #[serde(default)]
    pub entry: Entry,
    #[serde(default)]
    pub audience: Audience,
    /// One of the game's room presets (`rooms.presets` in its descriptor);
    /// it names the room ("Alex's 2v2").
    #[serde(default)]
    pub preset: Option<String>,
    /// Game-defined key/values the browser shows (keys the descriptor lists
    /// under `rooms.display`). Limited to `limits.max_metadata_bytes`.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    /// The host's UDP candidates (`ip:port`), including server-reflexive ones.
    #[serde(default)]
    pub candidates: Vec<String>,
    /// A launcher-staged room (integration tier 0): the launcher, not the game,
    /// runs the lobby and starts the game on every machine.
    #[serde(default)]
    pub staged: bool,
}

/// A room as the browser sees it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RoomInfo {
    pub id: String,
    pub game: String,
    pub game_version: String,
    /// Generated name: "<host>'s <preset label>".
    pub name: String,
    pub host: String,
    pub players: u32,
    pub max_players: u32,
    pub visibility: Visibility,
    pub entry: Entry,
    pub audience: Audience,
    pub ranked: bool,
    pub staged: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lockfile: Option<String>,
    #[serde(default)]
    pub preset: Option<String>,
    pub metadata: BTreeMap<String, String>,
    pub members: Vec<String>,
    pub created: i64,
}

/// Response to `POST /api/v1/rooms`.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CreatedRoom {
    pub room: RoomInfo,
    /// Join code (for `entry = code` and for unlisted rooms).
    pub code: Option<String>,
    /// `warpzone://join/<room>#<secret>` — bypasses the entry rule.
    pub invite: String,
    pub session: PeerSession,
}

/// `POST /api/v1/rooms/{id}/join`
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JoinRequest {
    #[serde(default)]
    pub code: Option<String>,
    /// The `#secret` part of an invite link.
    #[serde(default)]
    pub invite: Option<String>,
    #[serde(default)]
    pub candidates: Vec<String>,
}

/// `POST /api/v1/join` — join by code alone.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct JoinByCode {
    pub code: String,
    #[serde(default)]
    pub candidates: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum JoinResponse {
    Joined(Joined),
    /// Waiting for the host; a `knock_result` event follows.
    Knocking {
        room: RoomInfo,
        knock: String,
    },
}

/// What a member needs to talk to the other members.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Joined {
    pub room: RoomInfo,
    pub session: PeerSession,
    /// For a guest: the host. (Rooms are a star around the host.)
    pub peers: Vec<PeerInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Host,
    Guest,
}

/// My own ticket for a room.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PeerSession {
    /// Server-signed ticket ([`crate::ticket`]); shown to peers in the handshake.
    pub ticket: String,
    pub role: Role,
}

/// Another member and how to reach them.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PeerInfo {
    pub account: i64,
    pub username: String,
    /// The peer's device key (hex); its handshake must be signed by it.
    pub key: String,
    pub role: Role,
    pub candidates: Vec<String>,
    /// My half of the relay channel to this peer.
    pub relay: RelayGrant,
}

/// A relay channel grant: bind with `WZB1 channel side token` on the server's
/// UDP port, then send `WZD1 channel payload` datagrams.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct RelayGrant {
    pub channel: u32,
    pub side: u8,
    /// 16 bytes, hex.
    #[serde(with = "hex16")]
    pub token: [u8; 16],
}

mod hex16 {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(v: &[u8; 16], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 16], D::Error> {
        let s = String::deserialize(d)?;
        let v = hex::decode(&s).map_err(D::Error::custom)?;
        v.try_into().map_err(|_| D::Error::custom("expected 16 bytes"))
    }
}

/// `POST /api/v1/rooms/{id}/knocks/{knock}`
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KnockDecision {
    pub accept: bool,
}

/// `POST /api/v1/rooms/{id}/heartbeat` (host, every ~15 s). Fields that are
/// present replace the stored value.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Heartbeat {
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub candidates: Option<Vec<String>>,
    /// The host may close the room to new joiners (game started) without
    /// closing it.
    #[serde(default)]
    pub closed_to_joins: Option<bool>,
    /// Accounts the host is actually connected to (besides itself). Members
    /// not listed are dropped from the room, so player counts stay true when a
    /// guest crashes instead of leaving.
    #[serde(default)]
    pub connected: Option<Vec<i64>>,
}

/// `POST /api/v1/rooms/{id}/kick`
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Kick {
    pub account: i64,
    #[serde(default)]
    pub ban: bool,
}

/// `POST /api/v1/rooms/{id}/candidates` (guest): update my candidates; the
/// host receives a `peer_candidates` event.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Candidates {
    pub candidates: Vec<String>,
}

// ---------------------------------------------------------------- events

/// `GET /api/v1/events?after=<seq>&timeout=<s>` — long poll.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EventsResponse {
    pub events: Vec<EventEnvelope>,
    /// Pass as `after` in the next poll.
    pub next: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EventEnvelope {
    pub seq: u64,
    #[serde(flatten)]
    pub event: ServerEvent,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    /// Host: somebody knocks.
    Knock { room: String, knock: String, account: i64, username: String },
    /// Joiner: the host decided.
    KnockResult { room: String, accepted: bool, joined: Option<Box<Joined>> },
    /// Host: a guest joined.
    PeerJoined { room: String, peer: PeerInfo },
    /// Everybody: a member left or was kicked.
    PeerLeft { room: String, account: i64 },
    /// A member's candidates changed.
    PeerCandidates { room: String, account: i64, candidates: Vec<String> },
    /// You were removed from a room.
    Kicked { room: String, banned: bool },
    /// The host closed the room or it timed out.
    RoomClosed { room: String },
    /// Matchmaking put you into a match; join `room` (you are a member already).
    MatchFound { match_id: String, game: String, queue: String, room: String, host: bool, teams: Vec<Vec<String>> },
    /// A match you played was rated, disputed or abandoned.
    MatchFinished { match_id: String, state: String },
    /// Moderators and reviewers: a mod version waits for review.
    ModSubmitted { game: String, name: String, version: String, author: String },
    /// Author: a moderator decided on your mod version.
    ModDecision { game: String, name: String, version: String, decision: String, reason: Option<String> },
}

// ---------------------------------------------------------------- ladder

/// `POST /api/v1/ladder/queue`
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct QueueJoin {
    pub game: String,
    pub queue: String,
    pub game_version: String,
    /// `stock`, or the lockfile hash of the mod set this queue runs with.
    #[serde(default = "stock")]
    pub ruleset: String,
    #[serde(default)]
    pub candidates: Vec<String>,
}

fn stock() -> String {
    "stock".into()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct QueueStatus {
    pub queued: bool,
    pub game: Option<String>,
    pub queue: Option<String>,
    pub since: Option<i64>,
    pub waiting: u32,
}

/// `POST /api/v1/matches/{id}/report`. `placements[0]` is the winning team
/// (account ids); ties share a slot.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MatchReport {
    pub placements: Vec<Vec<i64>>,
    /// Deterministic games: hash of the final game state, compared across
    /// reports.
    #[serde(default)]
    pub state_hash: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MatchInfo {
    pub id: String,
    pub game: String,
    pub queue: String,
    pub season: String,
    /// `open`, `rated`, `disputed` or `abandoned`.
    pub state: String,
    pub room: String,
    pub teams: Vec<Vec<MatchPlayer>>,
    pub created: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MatchPlayer {
    pub account: i64,
    pub username: String,
    pub rating_before: i32,
    pub rating_after: Option<i32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Standing {
    pub rank: u32,
    pub username: String,
    /// Display rating: `1500 + 20 * (mu - 3 sigma)`.
    pub rating: i32,
    pub mu: f64,
    pub sigma: f64,
    pub games: u32,
    pub wins: u32,
}

/// `GET /api/v1/games/{game}/ladder/{queue}?season=`
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StandingsResponse {
    pub game: String,
    pub queue: String,
    pub season: String,
    pub current_season: String,
    pub seasons: Vec<String>,
    pub standings: Vec<Standing>,
    /// Players still in their placement matches (not listed).
    pub placing: u32,
}

// ---------------------------------------------------------------- leaderboards

/// `POST /api/v1/games/{game}/boards/{board}/scores`
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScoreSubmit {
    pub value: i64,
    #[serde(default = "stock")]
    pub ruleset: String,
    #[serde(default)]
    pub replay_hash: Option<String>,
    /// Optional replay file, base64; the server checks it hashes to `replay_hash`.
    #[serde(default)]
    pub replay: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScoreSubmitted {
    pub best: i64,
    pub rank: u32,
    pub improved: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BoardEntry {
    pub rank: u32,
    pub username: String,
    pub value: i64,
    pub replay_hash: Option<String>,
    pub submitted: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BoardResponse {
    pub game: String,
    pub board: String,
    pub title: String,
    /// `time` (milliseconds, lower is better) or `score` (higher is better).
    pub kind: String,
    pub ruleset: String,
    /// `all` or a season id.
    pub season: String,
    pub entries: Vec<BoardEntry>,
}

// ---------------------------------------------------------------- mods

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModSummary {
    pub game: String,
    pub name: String,
    pub title: String,
    pub category: String,
    pub author: String,
    /// Newest approved, non-yanked version.
    pub latest: String,
    pub description: String,
    pub downloads: u64,
    pub updated: i64,
    pub tags: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModVersionInfo {
    pub version: String,
    pub game_version: String,
    pub dependencies: Vec<String>,
    pub sha256: String,
    pub size: u64,
    /// `pending`, `approved`, `rejected` or `hidden`.
    pub state: String,
    pub yanked: bool,
    pub published: i64,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModDetail {
    pub summary: ModSummary,
    pub license: String,
    pub homepage: Option<String>,
    pub contains_code: bool,
    pub versions: Vec<ModVersionInfo>,
}

/// `POST /api/v1/games/{game}/mods/resolve` — `mods` uses dependency syntax
/// without a prefix: `"cave-tiles >= 1.2"`, `"bat"`.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResolveRequest {
    pub game_version: String,
    pub mods: Vec<String>,
}

/// Response of an upload.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Uploaded {
    pub name: String,
    pub version: String,
    pub sha256: String,
    pub state: String,
}

/// `POST /api/v1/reports` — report a mod or a username to the moderators.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReportRequest {
    /// `mod` or `user`.
    pub target: String,
    #[serde(default)]
    pub game: Option<String>,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    /// One of [`REPORT_REASONS`].
    pub reason: String,
    /// Optional note, seen only by moderators.
    #[serde(default)]
    pub detail: Option<String>,
}

/// Reasons a player can pick when reporting.
pub const REPORT_REASONS: &[&str] = &["copyright", "malware", "broken", "inappropriate", "spam", "impersonation", "other"];

/// `POST /api/v1/feedback` — a crash, a problem or an idea for one game or
/// for Warpzone itself. No account needed; the server keeps it and may pass
/// it on to whoever looks after that game.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FeedbackRequest {
    /// A game id, or [`FEEDBACK_WARPZONE`] for the launcher, server and library.
    pub game: String,
    /// One of [`FEEDBACK_KINDS`].
    pub kind: String,
    /// One line (at most [`FEEDBACK_TITLE_MAX`] bytes). Identical titles are
    /// counted, not stored twice, so keep varying details out of it.
    pub title: String,
    /// Steps, stack trace, log lines (at most [`FEEDBACK_DETAIL_MAX`] bytes).
    #[serde(default)]
    pub detail: String,
    /// Version of the game (or of Warpzone).
    #[serde(default)]
    pub version: Option<String>,
    /// What sent it, e.g. `warpzone-launcher 0.1.3 (linux x86_64)`.
    #[serde(default)]
    pub client: Option<String>,
}

/// Answer to [`FeedbackRequest`].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FeedbackResponse {
    pub id: i64,
    /// The same report was already open: it was counted, not added.
    pub duplicate: bool,
}

/// `crash`: the program died; `bug`: something does not work; `idea`: a wish.
pub const FEEDBACK_KINDS: &[&str] = &["crash", "bug", "idea"];
/// The `game` for feedback about Warpzone itself.
pub const FEEDBACK_WARPZONE: &str = "warpzone";
pub const FEEDBACK_TITLE_MAX: usize = 160;
pub const FEEDBACK_DETAIL_MAX: usize = 16 * 1024;

/// `GET /api/v1/moderation/feedback` (staff) — the newest feedback first.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FeedbackEntry {
    pub id: i64,
    pub game: String,
    pub kind: String,
    pub title: String,
    pub detail: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub client: Option<String>,
    /// How often it came in.
    pub count: i64,
    pub created: i64,
    pub last: i64,
    /// When it was passed on (`[feedback] url`), if it was.
    #[serde(default)]
    pub forwarded: Option<i64>,
}

/// Reasons a moderator picks when rejecting; the author sees it.
pub const REJECT_REASONS: &[&str] = &["copyright", "malware", "broken", "inappropriate", "spam", "wrong-category", "missing-license", "other"];

/// `GET /api/v1/moderation/queue`
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModerationQueue {
    pub pending: Vec<PendingMod>,
    pub reports: Vec<OpenReport>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PendingMod {
    pub game: String,
    pub name: String,
    pub version: String,
    pub title: String,
    pub category: String,
    pub author: String,
    pub size: u64,
    pub sha256: String,
    pub submitted: i64,
    pub manifest: serde_json::Value,
    pub contains_code: bool,
    pub notes: Vec<Note>,
    /// API path to download the archive (moderators may fetch pending ones).
    pub download: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OpenReport {
    pub id: i64,
    pub target: String,
    pub game: Option<String>,
    pub name: String,
    pub version: Option<String>,
    pub reason: String,
    pub detail: Option<String>,
    pub reporter: String,
    pub created: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Note {
    pub role: String,
    pub text: String,
    pub created: i64,
}

/// `POST /api/v1/moderation/mods/{game}/{name}/{version}`
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Decision {
    /// `approve`, `reject`, `hide` or `unhide`.
    pub decision: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// `POST /api/v1/moderation/notes` (reviewer or moderator)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NoteRequest {
    pub game: String,
    pub name: String,
    pub version: String,
    pub text: String,
}

// ---------------------------------------------------------------- releases

/// `GET /api/v1/games/{game}/release` — the newest release and the asset for
/// each platform, as cached from the game's GitHub releases.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ReleaseInfo {
    pub version: String,
    pub name: String,
    pub published: String,
    pub prerelease: bool,
    pub page: String,
    /// Platform (`linux`, `windows`, `deck`, `portmaster`) → asset.
    pub assets: BTreeMap<String, AssetInfo>,
    /// URL of the checksum file, if the release has one. Clients fetch it from
    /// GitHub themselves rather than trusting this server.
    pub checksums: Option<String>,
    /// Release notes (Markdown, as written on GitHub).
    #[serde(default)]
    pub notes: String,
}

/// One release in a game's changelog.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ChangelogEntry {
    pub version: String,
    pub name: String,
    pub published: String,
    pub prerelease: bool,
    pub page: String,
    /// Markdown.
    pub notes: String,
}

/// `GET /api/v1/games/{game}/updates?current=<version>` — what a game (or the
/// launcher) shows on start: is there something newer, and what changed since
/// the running version.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct UpdateInfo {
    pub current: String,
    pub update_available: bool,
    pub latest: Option<ReleaseInfo>,
    /// Releases newer than `current`, newest first (at most the last ten).
    pub changes: Vec<ChangelogEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AssetInfo {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_tagged() {
        let e = EventEnvelope { seq: 3, event: ServerEvent::RoomClosed { room: "r1".into() } };
        let s = serde_json::to_string(&e).unwrap();
        assert_eq!(s, r#"{"seq":3,"type":"room_closed","room":"r1"}"#);
        let back: EventEnvelope = serde_json::from_str(&s).unwrap();
        assert_eq!(back.event, e.event);
    }

    #[test]
    fn join_response_is_tagged() {
        let s = r#"{"status":"knocking","room":{"id":"a","game":"g","game_version":"1","name":"n","host":"h","players":1,"max_players":2,"visibility":"public","entry":"approve","audience":{},"ranked":false,"staged":false,"metadata":{},"members":["h"],"created":0},"knock":"k"}"#;
        let r: JoinResponse = serde_json::from_str(s).unwrap();
        assert!(matches!(r, JoinResponse::Knocking { .. }));
    }

    #[test]
    fn relay_token_is_hex() {
        let g = RelayGrant { channel: 7, side: 1, token: [0xab; 16] };
        let s = serde_json::to_string(&g).unwrap();
        assert!(s.contains(&"ab".repeat(16)));
        assert_eq!(serde_json::from_str::<RelayGrant>(&s).unwrap(), g);
    }
}
