# Notes

- 2026-10-16 (report t_mvb0neyv46ndxt, launcher test on Jan's laptop): PR #8
  (launcher-appimage) and drmario-data merged; released as 0.1.2. Review found
  that `local_players`/`optional` were always serialized and 0.1.1 clients
  read /games with deny_unknown_fields (empty game list after the deploy):
  new fields now skip when unset, and /games, /games/{id} strip
  local_players, data.optional and tier0.relaunch_linger for the user agents
  warpzone-core/0.1.0 and 0.1.1 — add every future descriptor field to
  `for_client` in crates/server/src/main.rs. So `relaunch_linger` can go into
  opents-coop.toml now that 0.1.2 is deployed. Registration limit counts only
  created accounts; accounts table migrated to AUTOINCREMENT (sequence seeded
  above ids in match_reports/match_ratings/reports). Worker tests belong on a
  local server (dev/server.toml), never production: they share Jan's IP.
  The container has no pkg-config; .toolchain/sysshim/bin/pkg-config is a
  tiny libudev-only shim (put it on PATH to build the launcher here).
  DEPLOY PENDING (vtt): config/server.toml already has the appimage asset
  (pushed; 0.1.1 reads it fine). Still to do once Jan approves the .env change
  (asked via the gateway 14:25 UTC; the gateway dropped the connection twice,
  so it may not have reached him): WARPZONE_VERSION=0.1.2,
  WARPZONE_SHA256=4ead1de3c36f05d34c7c0561a6bd86d2bedb88aec4518685fb727d3a856f872c,
  back up data/warpzone.db*, `docker compose up -d --build`; THEN push
  games/*.toml to config/games/ (they carry local_players/optional, which the
  0.1.1 server rejects — never push them before the server runs 0.1.2).
- 2026-10-16 (report t_mvb1n7hu4dd364, drmario lane): drmario64-vs tier 3 with
  1v1 rooms/ladder and join/host args is PR #10. Merge only after
  drmario64-vs v0.2.0 is released (only v0.1.0 exists; the launcher installs the
  latest release and would hand --warpzone-join to a game without it). #5 and
  #9 closed (in main).

- 2026-10-16 (reports t_mvayqjr13bdvuz from furries, t_mvaz3n503dowbo from
  diablo2): funny-furries is tier 2, not 3 (no mod sync, leaderboards, room
  browser or approval yet); its [mods] is commented out so no "mods" badge or
  archive page promises what the game cannot use. PR #1 (--import-copy) and
  PR #2 (diablo2-couch asset names) merged. diablo2-couch is Windows-only: its
  v0.0.1 Linux tarball needs glibc 2.43 and the launcher cannot install
  .flatpak bundles; restore linux/deck when that lane ships an older-glibc
  build. Public UDP 169.58.177.166:3478 answers STUN from outside (laptop
  container); STUN and relay share that socket, so the furries lane's failed
  netcheck from the vtt container is hairpin. A full two-account relay test
  was not possible: registration is capped per IP per day. Release archives
  now ship licenses + cargo-about THIRD-PARTY-NOTICES.html (from the next tag).

- 2026-10-16 (report t_mvayewss38upjn, from the opents lane): tier-0 games that
  restart themselves per mission (OpenTS Co-op, Spawner_Relaunch) lost the
  tunnel when the first process exited. New `[tier0] relaunch_linger = <s>`
  (<= 3600): the tunnel and room stay up until the game port has been silent
  that long. Staged seed is now 1..=2^31-1 (games atoi it into a signed int).
  NOT yet set in games/opents-coop.toml, on purpose: the server parses
  descriptors with deny_unknown_fields (needs a server deploy first), and
  launchers <= 0.1.1 deserialize /games as one Vec with deny_unknown_fields, so
  a descriptor carrying the field would empty their whole game list. Add
  `relaunch_linger = 120` to opents-coop.toml (or in the opents lane's
  descriptor PR) once 0.1.2 is released, deployed and launchers have updated.
  Launcher checked with clippy for x86_64-pc-windows-gnu (CC=gcc, no link).
- 2026-10-16 (report t_mvaxy58u35mk9e, from the furries lane): Client::poll held
  every Msg until the UDP net existed (after a successful login), so offline
  clients got no events at all, not even WZ_EVENT_ERROR {op:login}. Plain
  Msg::Event now passes straight through; only room traffic waits. Test:
  client::tests::offline_login_error_is_delivered. Reaches games with the next
  libwarpzone release (no server deploy).
- 2026-10-16 (report t_mvax59g72ybhrq, from the furries lane): a new libwarpzone
  client started the event cursor at 0 and replayed the account's buffered
  MatchFound/MatchFinished (auto-joining dead match rooms). Fixed in the client:
  the cursor starts at the queue's head (`events?after=0&timeout=0`) before
  LoggedIn goes out. Works against the deployed 0.1.x server, so no server deploy
  is needed; it reaches games with the next libwarpzone release. MATCH_FOUND JSON
  now has `queue`. Launcher not built here (no pkg-config/libudev in the container).
