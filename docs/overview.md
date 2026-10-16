# Overview

```text
                        ┌─────────────────────────── warpzone-server ───────────────────────────┐
  game (libwarpzone) ──▶│ HTTPS  /api/v1    accounts · rooms · tickets · events (long poll)       │
  launcher, wz CLI   ──▶│                    ladder · leaderboards · mods · moderation · releases │
  browser            ──▶│        /           website: games, downloads, rooms, ladders, mods      │
                        │ UDP    :3478      STUN reflection · relay channels                      │
                        └──────────────▲───────────────────────────────▲────────────────────────┘
                                       │ candidates, tickets             │ only if direct fails
               ┌──────── host ─────────┴───┐      encrypted UDP       ┌──┴──────── guest ─────────┐
               │ game ⇄ libwarpzone        │◀════════════════════════▶│ libwarpzone ⇄ game         │
               └───────────────────────────┘   (direct or relayed)    └───────────────────────────┘
```

## The pieces

* **Server** — one process for every game. State that must survive restarts
  (accounts, ratings, matches, scores, mods, lockfiles) is in SQLite; rooms,
  sessions and relay channels live in memory (after a restart, games simply
  host again). [Self-hosting](self-hosting.md), [API](api.md).
* **Core / library** — the client logic in Rust (`warpzone-core`), exposed to
  C and C++ games as `libwarpzone` ([header](../crates/capi/include/warpzone.h)).
  [Integration](integration.md).
* **Launcher** — desktop app on top of the same core. [Players](players.md),
  [internals](launcher.md).
* **Descriptors** — one TOML file per game telling the server and launcher
  what the game supports, how to install and start it and what original files
  it needs. [Descriptors](descriptors.md).

## Identity

Every device has an Ed25519 key, created on first use and shared by every
Warpzone program of that user (launcher, CLI, games). Logging in is a signed
challenge; there are no passwords and no e-mail. A username is claimed once
per account; more devices join an account with a short-lived **link code**; a
**recovery code** restores a key on a new installation.

## Rooms

A room belongs to one game and one game version, optionally one mod set
(lockfile hash). It has:

* **visibility** — *public* (listed) or *unlisted* (code or link only);
* **entry** — *open*, *code* (6 characters from an alphabet without
  look-alikes) or *approve* (the joiner knocks, the host accepts);
* a **target audience** instead of free text: languages, skill (beginners
  welcome / mixed / experienced) and style (casual / serious / learning);
* a generated **name** ("Alex's 2v2") from the game's presets, and game-defined
  **metadata** the browser shows (map, mode). There is no chat and no free-text
  field anywhere, so there is nothing to moderate in rooms.

Rooms are a **star around the host**: each guest connects to the host. Games
that need everyone to reach everyone relay through the host (the tier-0
tunnel does this automatically).

## Tickets and connections

The server hands a member a **ticket** only after the room's access check
(open / code / approval / invite / match). Tickets are signed by the server
key. When two peers connect, each sends its ticket, an ephemeral X25519 key
and an Ed25519 signature by its device key; each side accepts only a valid
ticket for the right room, account and key. Knowing a host's IP address is
therefore not enough to get into a private game.

Connecting tries, in order: **IPv6 direct**, **IPv4 hole punching** (both
sides send to every candidate address of the other at once), and the
**relay** (after 1.5 s without a direct path; direct probing continues and the
connection upgrades when it works). Peer traffic is ChaCha20-Poly1305
encrypted end to end; the relay sees only ciphertext. Details:
[protocol](protocol.md).

## Integration tiers

| Tier | The game does | It gets |
|---|---|---|
| 0 | nothing — the launcher starts it from templates | launcher-staged lobby, its own UDP traffic tunnelled over Warpzone, mod sync |
| 1 | reads `--warpzone-join <room>` | "Join" from the launcher and the website |
| 2 | hosts and lists rooms itself through the library | full online play without the launcher |
| 3 | ladder, leaderboards, mod browser in game | everything |

## Ranked play and leaderboards

Optional per game. Queues are keyed by game, mode, game version and ruleset,
so modded rules never mix with stock ranked play. Ratings use **OpenSkill**
(Weng-Lin, Plackett-Luce) — it handles 1v1, teams and free-for-all — shown on an
Elo-like scale; **seasons** follow the calendar (quarterly by default) with a
soft reset. Leaderboards record best times or scores, all-time and per season.
[Ladder](ladder.md).

## Mods

Each game declares its own categories; the server never opens an archive and
does not know what a mod does. Dependencies use Factorio's syntax (required,
`?` optional, `(?)` hidden optional, `!` conflict, `~` no load-order effect).
The server resolves a set into a **lockfile** (exact versions, hashes, load
order); rooms carry the lockfile hash and joining installs exactly that set.
**Every version is approved by a moderator** before anyone else can see it.
[Mods](mods.md), [moderation](moderation.md).

## Updates and changelogs

The server caches each game's GitHub releases. Games ask "is there something
newer than my version, and what changed since?" (`wz_check_update`) and show
the changes on the first start of a new version; the launcher updates its
games in one click or automatically and shows "What's new". The launcher
verifies downloads against GitHub's own digest or checksum file, never
against data from the Warpzone server alone.
