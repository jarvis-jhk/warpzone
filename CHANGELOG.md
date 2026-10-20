# Changelog

## 0.1.11

**macOS**, for Apple Silicon and Intel (macOS 11 or newer):

- `libwarpzone-0.1.11-universal-apple-darwin.tar.gz`: one universal
  `libwarpzone.a` (and `libwarpzone.dylib`, install name
  `@rpath/libwarpzone.dylib`) with `warpzone.h`, ready for a game to link;
  docs/integration.md#macos says how (no frameworks needed).
- `wz` and the launcher (`Warpzone.app`, ad-hoc signed, not notarised) as
  universal builds. The launcher shows games with a macOS build, installs
  and updates them, and updates itself.
- Descriptors take a `macos` platform in `install.assets` and `launch.exec`.
  Operation Spacehog has one.
- CI builds and tests every client crate on macOS.

Also: `NATIVE_LIBS.txt` was empty in the Linux x86_64 package and ended in
a stray terminal colour code in the Windows MSVC one; both are clean now.

## 0.1.10

- Launcher: the dimmed icons on a library tile (online, mods, ladder that are
  still coming) are an even grey again. They were drawn translucent, so where
  a glyph's strokes and shapes overlapped (the globe's lines, the trophy's
  handles, the puzzle's knobs) they showed brighter or muddier.

## 0.1.9

Launcher, from Steam Deck testing:

- Scrolling is instant: the page jumps to the control the D-pad focused
  instead of gliding there.
- The D-pad no longer climbs up from the games to *Installed* / *All games*
  (X switches between them) or *Refresh* (LT).
- The dark shade at the bottom of a tile reaches its edge: depending on where
  the tile fell on the screen's pixels, a bright line of the artwork could
  show under it.

Android launcher:

- **Player name and linking** in *Settings*: choose a player name, or enter a
  link code from another device to play as that player there (and get a link
  code for the next device). The name shows top right.
- Games started from it play as that player: they get the launcher's key
  (`warpzone.identity`; games call `launch::adopt_identity` /
  `wz_adopt_identity`, see docs/integration.md).

## 0.1.8

- Android launcher: installing a game no longer loses track of it when the
  library reloads on return from Android's install confirmation. The progress
  bar kept updating a stale copy, the busy state could stick, and the
  installed package was not remembered for the game shown.
- `server.example.toml`: `[launcher] checksums = "SHA256SUMS"`, so launcher
  releases on the website and in the update API carry their checksum file.

## 0.1.7

Launcher, from Steam Deck testing:

- The D-pad's focus scrolls into view: going down into a row of the library
  (or any page) below the screen scrolls it up. It never did — egui only
  scrolls for a request made inside the scroll area.
- The quit dialog shows its buttons: (A) Quit, (B) Stay. A quits and B stays
  whichever button the D-pad is on.
- Games that cannot run here are not shown: a Windows-only game is missing
  from the Linux and Steam Deck launcher, an Android-only one from every
  desktop (the Deck also takes Linux builds and AppImages). Installed games
  always show.
- Any controller button hides the mouse pointer and its hover highlight (and
  tooltip) until the mouse or touch moves again.

## 0.1.6

- **Warpzone for Android**: a light launcher for phones and tablets
  (`warpzone-launcher-0.1.6-android.apk`), the Steam Deck launcher made for
  touch. It shows only the games that run on Android (for now Baba Is You,
  baba-port), installs and updates them (checked against the release's
  SHA256SUMS, confirmed in Android's installer), starts them, hosts and joins
  rooms, takes codes and invite links, sends feedback, reports its own
  crashes and updates itself.
- Descriptors: platform `android` in `install.assets` (an APK) and
  `launch.exec.android` (the package name). The server lists Android
  downloads on the website.
- Games: `--warpzone-code <CODE>` joins a private game by its code
  (`wz_handle_launch`, `launch::from_args`); on Android the launcher's
  arguments arrive as the intent extra `warpzone.args`.

## 0.1.5

- Launcher: updates itself everywhere, always. It checks at start and every
  30 minutes (before: only at start, and only with *Update games
  automatically* on), and a launcher running in the background restarts
  into the new version by itself once no game, room or queue is active.
  Launchers up to 0.1.4 still pick the update up at their next start.

## 0.1.4

- Launcher, controller (from the first test on a real Steam Deck): the D-pad
  moves only within the page or the open dialog, never into the tab bar, and
  works on every tab (text fields no longer swallow it); the focused control
  gets a clearly visible ring. LT refreshes the page, View chooses the
  player name, RT quits (asks first). The feedback box is a tab of its own.
  Everything was far too big on the Deck: the interface follows the
  desktop's scale but keeps the 1280×800 layout on the screen, with
  *Settings → Interface size* to override, and two rows of library tiles
  always fit without scrolling.

## 0.1.3

- Launcher: the library is a wall of portrait tiles, like Steam's — each
  game's artwork from SteamGridDB with no name over it, only icons for
  players on one screen, online, mods and ladder, and the game's state in
  the corner. Descriptors carry only the SteamGridDB id (new field
  `steamgriddb`); the launcher looks the art up at run time and caches it,
  so it shows offline too. Games without artwork keep the generated picture
  with their name. *Change artwork…* picks another SteamGridDB picture,
  searches SteamGridDB for any game, takes your own picture file or goes
  back to the generated one; the choice is also used by *Add to Steam*.
  Optional personal SteamGridDB API key in Settings. Servers leave
  `steamgriddb` out for launchers up to 0.1.2.

- Launcher: controller navigation that feels native on the Steam Deck. The
  D-pad and stick move between controls in their direction (held: repeat),
  A opens, B goes back, Y plays or installs the focused game, X switches the
  library between installed and all games, Menu changes artwork, LB/RB
  switch tabs (LB did nothing before). Button glyphs in the tab bar and a
  hint bar at the bottom whenever a controller is connected. Buttons now
  work while the window is idle, and are ignored while a game has the focus.

- Launcher: with games installed, the library shows only those; *All
  games* or the "+ N more to install" tile shows the others.

- Steam Deck installer: `Install-Warpzone.desktop` (a release asset), opened
  in Desktop Mode, installs the launcher AppImage (checked against GitHub's
  SHA-256), adds it to Steam with artwork, a menu entry and the
  `warpzone://` handler, and offers to go back to Game Mode.
  `warpzone --add-to-steam` does the Steam part on any Linux desktop.

- Build option: pure-Rust crypto. `--no-default-features --features
  pure-rust-crypto` builds every crate (server, libwarpzone, `wz`, launcher)
  with RustCrypto's rustls provider for HTTPS instead of `ring`, so no C
  compiler or assembler is involved in any crypto. The default stays `ring`;
  release builds are unchanged.

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
