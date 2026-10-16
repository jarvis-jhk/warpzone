# HTTP API reference

Base URL: the instance's `public_url` + `/api/v1` (for the public instance
`https://warpzone.virtualtabletop.io/api/v1`). All bodies are JSON unless
noted. The Rust types in [`crates/protocol/src/api.rs`](../crates/protocol/src/api.rs)
are the contract; this page summarises them.

* **Auth**: routes marked 🔑 need `Authorization: Bearer <token>` from
  [login](#authentication). 👤 additionally needs a registered username.
  🛡 needs a staff key (moderator or reviewer); ⚖ needs a moderator key.
* **Errors**: non-2xx responses are `{"error": "<code>", "message": "<text>"}`.
  Codes: `bad_request`, `unauthorized`, `bad_challenge`, `bad_signature`,
  `forbidden`, `needs_account`, `not_found`, `conflict`, `too_large`,
  `rate_limited` (429), `unresolvable` (422), `store_full` (507), `internal`.
  The message is safe to show to players.
* **Times** are Unix seconds; keys and hashes are lower-case hex.

## Server

| Method | Path | Description |
|---|---|---|
| GET | `/info` | `ServerInfo`: name, version, `server_key` (verifies tickets), `public_url`, `udp` endpoints, features, limits |
| GET | `/stats` | rooms, players in rooms, relay channels and bytes, STUN requests, uptime |
| GET | `/endpoints.json` *(no `/api/v1` prefix)* | `{"api", "udp": [...]}` |
| GET | `/healthz` *(no prefix)* | `ok` |

## Authentication

Keypair login; no passwords.

1. `POST /auth/challenge` `{"key": "<ed25519 public key hex>"}` → `{"nonce", "expires_in"}`
2. Sign the bytes `"warpzone-login-v1\n" + server_key + "\n" + nonce` with the
   device key.
3. `POST /auth/login` `{"key", "nonce", "signature": "<hex>"}` →
   `{"token", "expires_in", "account": Account | null, "roles": [...]}`

Tokens last 24 h and are lost on server restart; on `401` log in again (the
client library does this transparently). Limit: `logins_per_ip_per_minute`.

## Accounts

| Method | Path | Auth | Description |
|---|---|---|---|
| POST | `/accounts` | 🔑 | `{"username"}` → `Account`. 3–16 ASCII letters, digits, `_`, `-`; case-insensitively unique; reserved and blocked words refused. `registrations_per_ip_per_day` |
| GET | `/accounts/me` | 🔑 | `{"key", "account", "roles"}` |
| DELETE | `/accounts/me` | 👤 | delete account, devices, ratings, scores (published mods stay, ownerless) |
| POST | `/accounts/me/link-code` | 👤 | `{"code", "expires_in": 600}` — 8 characters |
| POST | `/accounts/link` | 🔑 | `{"code"}` → `Account`: adds this device's key to the code's account (max 10 devices) |
| GET | `/accounts/me/keys` | 👤 | `[{"key", "added"}]` |
| DELETE | `/accounts/me/keys/{key}` | 👤 | remove another device |
| GET | `/accounts/{username}` | — | `Profile`: username, created, ratings per game/queue/season |

## Games and releases

| Method | Path | Description |
|---|---|---|
| GET | `/games` | every non-hidden [descriptor](descriptors.md) as JSON |
| GET | `/games/{game}` | one descriptor |
| GET | `/games/{game}/release` | `ReleaseInfo`: newest release, assets per platform (`linux`, `windows`, `deck`, `portmaster`), notes, checksum file URL. `{game}` = `launcher` for the launcher |
| GET | `/games/{game}/changelog` | `[ChangelogEntry]`: recent releases, newest first, with Markdown notes |
| GET | `/games/{game}/updates?current=<version>` | `UpdateInfo`: `update_available`, `latest`, `changes` newer than `current` |

The release cache refreshes every 15 minutes from GitHub.

## Rooms

| Method | Path | Auth | Description |
|---|---|---|---|
| POST | `/rooms` | 👤 | `CreateRoom` → `CreatedRoom` `{"room", "code", "invite", "session"}` |
| GET | `/rooms?game=&language=&skill=&style=&has_space=true` | — | public rooms, newest first |
| GET | `/rooms/{id}?s=<secret>` | optional | a room: public, member, or with the invite secret |
| POST | `/rooms/{id}/join` | 👤 | `{"code"?, "invite"?, "candidates": [...]}` → `JoinResponse` |
| POST | `/join` | 👤 | `{"code", "candidates"}` — join by code alone |
| POST | `/rooms/{id}/knocks/{knock}` | 👤 host | `{"accept": bool}` |
| POST | `/rooms/{id}/heartbeat` | 👤 host | `Heartbeat` `{"metadata"?, "candidates"?, "closed_to_joins"?, "connected"?}` → `RoomInfo`; send every ~15 s, the room closes 45 s after the last |
| POST | `/rooms/{id}/candidates` | 👤 member | `{"candidates"}` — tells the host (or, from the host, the guests) |
| POST | `/rooms/{id}/leave` | 👤 member | leave (the host leaving closes the room) |
| DELETE | `/rooms/{id}` | 👤 host | close |
| POST | `/rooms/{id}/kick` | 👤 host | `{"account", "ban": bool}` |

`CreateRoom`:

```json
{
  "game": "funny-furries", "game_version": "0.3.0",
  "max_players": 4,
  "visibility": "public | unlisted",
  "entry": "open | code | approve",
  "audience": {"languages": ["en", "de"], "skill": "beginners | mixed | experienced", "style": "casual | serious | learning"},
  "preset": "2v2",
  "metadata": {"arena": "3"},
  "lockfile": "<sha256 of the mod set>",
  "candidates": ["[2001:db8::5]:40123", "198.51.100.7:40123"],
  "staged": false
}
```

`JoinResponse` is either `{"status": "joined", "room", "session": {"ticket",
"role"}, "peers": [PeerInfo]}` or `{"status": "knocking", "room", "knock"}`.
`PeerInfo` = `{"account", "username", "key", "role", "candidates", "relay":
{"channel", "side", "token"}}` — everything needed to connect
([protocol](protocol.md)).

Access rules: unlisted rooms answer `404` unless the code, the invite secret
or an earlier approval is given; `code` rooms need the code (or invite);
`approve` rooms create a knock. Code guesses are limited per IP
(`code_guesses_per_ip_per_minute`). Banned accounts get `403`.

## Events (long poll)

`GET /events?after=<seq>&timeout=<1..30>` 🔑 → `{"events": [{"seq", "type", …}], "next"}`.
Pass `next` as `after` in the next call. Events for an account (or, for
staff keys without an account, the key):

| `type` | Fields | Who |
|---|---|---|
| `knock` | `room, knock, account, username` | host |
| `knock_result` | `room, accepted, joined?` | joiner |
| `peer_joined` | `room, peer: PeerInfo` | host |
| `peer_left` | `room, account` | members |
| `peer_candidates` | `room, account, candidates` | the other side |
| `kicked` | `room, banned` | the removed player |
| `room_closed` | `room` | members |
| `match_found` | `match_id, game, queue, room, host, teams` | matched players |
| `match_finished` | `match_id, state` | players |
| `mod_submitted` | `game, name, version, author` | staff |
| `mod_decision` | `game, name, version, decision, reason?` | the author |

Queues keep the last 256 events.

## Ladder

| Method | Path | Auth | Description |
|---|---|---|---|
| POST | `/ladder/queue` | 👤 | `{"game", "queue", "game_version", "ruleset": "stock" \| <lockfile hash>, "candidates"}` → `QueueStatus` |
| GET | `/ladder/queue` | 👤 | `QueueStatus` |
| DELETE | `/ladder/queue` | 👤 | leave the queue |
| GET | `/matches/{id}` | — | `MatchInfo`: teams with ratings before/after, state |
| POST | `/matches/{id}/report` | 👤 player | `{"placements": [[accounts], ...], "state_hash"?}` → `MatchInfo` |
| GET | `/games/{game}/ladder/{queue}?season=` | — | `StandingsResponse`: season list, standings (rank, username, rating, μ, σ, games, wins), players still placing |

## Leaderboards

| Method | Path | Auth | Description |
|---|---|---|---|
| POST | `/games/{game}/boards/{board}/scores` | 👤 | `{"value", "ruleset", "replay_hash"?, "replay"? (base64, ≤ max_replay_kb)}` → `{"best", "rank", "improved"}` |
| GET | `/games/{game}/boards/{board}?ruleset=stock&season=all` | — | top 100 |
| GET | `/replays/{sha256}` | — | a stored replay file |

## Mods

| Method | Path | Auth | Description |
|---|---|---|---|
| GET | `/games/{game}/mods?category=&q=&sort=downloads\|updated\|name` | — | published mods (`ModSummary`) |
| POST | `/games/{game}/mods` | 👤 | upload, `multipart/form-data`: `manifest` (JSON, [manifest](mods.md#the-manifest)) and `archive` (file) → `{"name", "version", "sha256", "state": "pending"}` |
| GET | `/games/{game}/mods/{name}` | optional | `ModDetail`; authors and staff also see pending/rejected versions |
| GET | `/games/{game}/mods/{name}/{version}/download` | optional | the archive (`x-warpzone-sha256` header); pending versions only for the author and staff |
| POST | `/games/{game}/mods/{name}/{version}/yank` | 👤 author or ⚖ | `{"yanked": bool}` |
| POST | `/games/{game}/mods/resolve` | — | `{"game_version", "mods": ["name", "name >= 1.2"]}` → `Lockfile` or `422 unresolvable` with the dependency chain |
| GET | `/games/{game}/lockfiles/{hash}` | — | a stored lockfile |
| POST | `/games/{game}/lockfiles` | 🔑 | store a client-composed lockfile (every entry must be an approved version with that hash) |
| POST | `/reports` | 👤 | `{"target": "mod" \| "user", "game"?, "name", "version"?, "reason", "detail"?}`; reasons: `copyright, malware, broken, inappropriate, spam, impersonation, other` |

## Moderation

| Method | Path | Auth | Description |
|---|---|---|---|
| GET | `/moderation/queue` | 🛡 | pending versions (manifest, size, hash, notes, download path) and open reports |
| POST | `/moderation/mods/{game}/{name}/{version}` | ⚖ | `{"decision": "approve" \| "reject" \| "hide" \| "unhide", "reason"?}`; rejections need a reason from `copyright, malware, broken, inappropriate, spam, wrong-category, missing-license, other` |
| POST | `/moderation/notes` | 🛡 | `{"game", "name", "version", "text"}` |
| POST | `/moderation/reports/{id}/close` | ⚖ | close a report |
| POST | `/moderation/accounts/{username}/rename` | ⚖ | `{"username"}` — force a rename |

## Website routes

`/`, `/games/{id}`, `/games/{id}/mods/{name}`, `/join/{room}` (invite
landing page; the `#secret` stays in the browser), `/static/style.css`,
`/static/logo.svg`.
