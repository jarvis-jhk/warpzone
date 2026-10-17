# Changelog

## Unreleased

- Rooms: a host that cannot reach the server any more now finds out. Failed
  heartbeats used to pass silently while the server closed the room for the
  guests after 45 s, so the guests saw `RoomClosed` and the host waited
  forever. After 50 s without an accepted heartbeat the host gets
  `Error {op: "heartbeat"}` and `RoomClosed`.

- Rooms: a host no longer misses a player who joins while its own join is
  still on the way (ranked rooms, where everybody joins at once after a match
  is found): room events that arrive before the join answer are kept and
  applied once it is there. The same peer announced twice keeps its session.
  A peer that times out while still in the room gets one fresh handshake
  (the game sees `PeerLeft`, then `PeerJoined` again) instead of being lost
  for good. Fixes ranked 4-player lobbies hanging.

- Feedback: `POST /api/v1/feedback` takes crashes, problems and ideas for a
  game or for Warpzone itself, no account needed. The server keeps them
  (identical titles are counted), shows them to staff (`wz feedback`) and,
  with the new `[feedback] url`, passes each new one on as JSON, routed per
  game with `lanes`. Limited per address and per hour.

- Launcher: a feedback bar at the bottom of every page — pick the game
  (preset to the one started last, or Warpzone itself), *Problem* or *Idea*,
  type, Enter. Games that crash (a fatal signal or a non-zero exit) and the
  launcher's own panics are reported with the last log lines, home directory
  and login name removed; *Settings → Report crashes* turns that off.

- libwarpzone: `wz_report(c, kind, title, detail)` (background) and
  `wz_report_problem(server, game, version, kind, title, detail)` (no client
  needed, for crash handlers and the next start). `wz report` for scripts
  and games without the library.

- Launcher: *Browse…* next to "Files elsewhere?" opens the desktop's folder
  dialog (zenity or kdialog on Linux and the Deck, the standard dialog on
  Windows and macOS) and searches the chosen folder; typing the path still
  works.

- Launcher: smaller library tiles — five columns in the default window, so
  ten games fit without scrolling; the status line is cut short (full text on
  hover) instead of running under the buttons.

- Launcher and `wz fetch-data`: freeware game data is downloaded by the
  launcher itself when it is not found (on Install, Play or *Download now*),
  from the sources the descriptor lists under the new `[[data]] fetch`, each
  pinned by size and SHA-256 and shown with the licence that allows it.
  Operation Spacehog's original data now comes this way (DOS Games Archive,
  else the author's 2000 site on the Wayback Machine). Servers leave `fetch`
  out for launchers up to 0.1.2.

- Launcher and `wz find-data`: game files are also found inside `.zip`
  downloads (and zips inside them) in Downloads, on the Desktop and in chosen
  folders, e.g. the DOS Games Archive's `sh_dos.zip` for Operation Spacehog;
  the matching part is unpacked once into `<home>/unpacked/`. Old freeware
  installers' default `C:\Games` (and Wine's `drive_c/games`) are searched too.

- Server: optional `[fault_reports]` in `server.toml` — 500s, errors in
  background jobs, a failed start and panics are POSTed as JSON to a URL of
  your choice (deduplicated and capped per hour), not only logged.

## 0.1.2

- Server: only registrations that created an account count towards the daily
  per-address limit (a taken name or a failed attempt used it up).
- Server: account ids are never reused after a deletion (`AUTOINCREMENT`;
  existing databases are migrated on start), so old match and rating rows
  never point at a new player.
- Server: launchers and games up to 0.1.1 get game descriptors without the
  fields added since, so their game list keeps working.
- Launcher as a self-updating **AppImage** for Linux and the Steam Deck: one file that replaces itself with the newest verified release; also updatable with AppImageUpdate. Steam shortcuts, autostart and invite links point at the AppImage itself.
- Launcher: pressing Play shows *Starting…* and *Running*, steps aside (minimizes) so the game and its first-start dialogs are in front, comes back when the game ends, and explains a start that failed right away with the last lines of the game's output (`games/<id>/last-run.log`).
- Launcher: game files it found are handed to the game on every start as `WARPZONE_DATA_<ID>` (plus the descriptor's import arguments until the data is in place), so games use what the launcher found.
- Library and website: icons per game for controllers (players on one screen), online (players), mods and ladder — bright when available, dim when coming. New descriptor field `local_players`.
- Artwork: every game has its own custom pixel-art scene (original sprites, no game assets) for library tiles, Steam artwork and the website.
- Descriptors: Dr. Mario 64 VS added; Diablo Splitscreen offers its preview release and starts; `[[data]] optional = true` for files that are nice to have (they never block installing).
- libwarpzone: a newly started client no longer replays the account's older
  events — it used to receive (and with `auto_join_matches` join) matches found
  in earlier sessions. `WZ_EVENT_MATCH_FOUND` now also carries `queue`.
- libwarpzone: while the server is unreachable, events are delivered again —
  the promised `WZ_EVENT_ERROR {op:login}` was held back until a login succeeded.
- Launcher (tier 0): descriptors can set `[tier0] relaunch_linger` for games
  that restart themselves between missions; the tunnel and room stay up until
  the game has been quiet that long. The staged seed now stays below 2^31.
- Release archives: every libwarpzone, wz and launcher archive (the Windows
  ones too) carries LICENSE-MIT, LICENSE-APACHE and THIRD-PARTY-NOTICES.html
  for the Rust crates linked into it (generated with cargo-about).

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
