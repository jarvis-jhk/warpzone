# Changelog

## Unreleased

- libwarpzone: a newly started client no longer replays the account's older
  events — it used to receive (and with `auto_join_matches` join) matches found
  in earlier sessions. `WZ_EVENT_MATCH_FOUND` now also carries `queue`.
- libwarpzone: while the server is unreachable, events are delivered again —
  the promised `WZ_EVENT_ERROR {op:login}` was held back until a login succeeded.
- Launcher (tier 0): descriptors can set `[tier0] relaunch_linger` for games
  that restart themselves between missions; the tunnel and room stay up until
  the game has been quiet that long. The staged seed now stays below 2^31.

## 0.1.1

- Website: every game card and game page shows the game's generated artwork.
- Connections measure their round-trip time every second (it was only sampled while idle).

## 0.1.0

First release.

- **Server**: keypair accounts with link and recovery codes; public and
  private rooms (codes, invite links, host approval) with structured target
  audiences; server-signed join tickets; STUN and an encrypted-traffic relay
  with bandwidth caps and no amplification; OpenSkill ladder with calendar
  seasons, matchmaking and result reports; leaderboards with replays;
  Factorio-style mod archive with moderation, server-side resolution and
  lockfiles; release, changelog and update information for every game; the
  website with game list, downloads, rooms, ladders and mods.
- **libwarpzone** (C) and **warpzone-core** (Rust): rooms, IPv6-first
  hole punching with relay fallback, X25519 + ChaCha20-Poly1305 peer channels
  with unreliable datagrams and a reliable ordered stream, ranked queue,
  scores, update checks, mod sync.
- **Launcher**: library with generated artwork and hiding, one-click verified
  install and update (streamed), finding original game files in Steam, GOG,
  Heroic, Lutris, Wine, Downloads and discs with fingerprints, rooms across
  games, launcher-run lobbies with a UDP tunnel for games without Warpzone
  support, ladder and matchmaking, mods, moderation, Add to Steam with full
  artwork, background mode, controller navigation, self-update.
- **wz** CLI for players, mod authors, moderators and operators, with network
  diagnostics.
- Descriptors for seven games; deployment kit and self-hosting guide.
