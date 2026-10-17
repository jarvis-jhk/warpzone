# Notes

- 2026-10-17 (requests t_mvbnkjww2aer1t, t_mvbnmbg02coger, t_mvbnncjq2d7bh8,
  Jan): diablo2-couch has linux/deck again (v0.0.5 tarball needs glibc 2.28;
  exec is d2d, like d2d.exe on Windows, not the Qt d2d-launcher). THE 0.1.2
  SERVER DEPLOY IS DONE (Jan approved via the gateway; data backed up to vtt
  /root/warpzone-data.bak-20261017) and main's games/*.toml are on vtt, except
  spacehog.toml there has no `fetch` (0.1.2 rejects it; checked with the real
  v0.1.2 binary's check-games): push main's spacehog.toml only with 0.1.3.
  vtt's opens4.toml is not in this repo (opens4 lane) and has no
  local_players. Player-count icons were missing because 0.1.1 stripped
  local_players; launchers <= 0.1.1 still get it stripped. Launcher: Browse…
  folder dialog (zenity/kdialog/PowerShell/osascript, no new crate; the
  Windows path is not compiled here, no mingw) and 5-column tiles; shots in
  the worker with system Xvfb + libXcursor/libXi from deb.debian.org in
  .toolchain/xlibs (LD_LIBRARY_PATH).

- 2026-10-17 (request t_mvbmwmyb22k8db, Jan: "if there are no legal hurdles,
  it should download the original freeware games itself"): new
  `[[data]] fetch` (url, file, size, sha256, license). Launcher downloads it on
  Install/Play/"Download now" into <home>/downloads/<data id>/, keeps it only
  with the pinned size+sha256, then finds it like a download (zip lookup).
  `wz fetch-data <game>`. Server strips `fetch` for warpzone-core 0.1.0-0.1.2
  (all deny_unknown_fields). spacehog.toml: DGA file.php?id=1001 (sh_dos.zip),
  fallback Wayback copy of the author's own spacehog.zip (byte-identical to the
  one inside sh_dos.zip); both tested end to end with a local server (fallback
  with a broken first URL). Licence: README.TXT "encouraged to distribute this
  game (in it's original form) like crazy", not in commercial compilations.
  Legal check of the others: funny-furries has only "(c) BdJ Entertainment",
  no permission to pass it on, so no fetch; diablo spawn.mpq (shareware,
  devilutionx-assets) left out for now: it is an extracted file, not the
  original shareware package; opents (EA freeware via CnCNet installer) left
  out. Launcher UI not screenshotted: .toolchain Xvfb needs glibc 2.38, the
  worker container has older. DEPLOY ORDER: spacehog.toml with `fetch` goes
  to vtt only after a server with this code (0.1.3) runs there (the server
  parses descriptors with deny_unknown_fields; 0.1.1/0.1.2 servers would
  skip the whole file).

- 2026-10-17 (request t_mvbmjbcg1yxavd, Jan): the launcher/`wz find-data`
  now look into .zip files (and one level of zips inside) in Downloads,
  Desktop and chosen folders and unpack only the match into
  <home>/unpacked/<zip>-<hash>/ (cached by path+size+mtime). Verified with the
  real DOS Games Archive downloads: sh_dos.zip (holds spacehog.zip with the
  DATs) is found; sh_install.zip holds only sh_install.exe, a Windows
  installer with a proprietary compressed payload (not zlib/bz2/lzma), so it
  cannot be read; its default target C:\games\spacehog (and Wine's
  drive_c/games) is searched now. games/spacehog.toml tells players to keep
  sh_dos.zip and links its file page directly. Push spacehog.toml to vtt only
  together with the launcher release carrying this (0.1.3; 0.1.2 launchers
  do not look into zips), and keep vtt's copy without local_players until the
  0.1.2 server deploy.

- 2026-10-16 (report t_mvbbejaqh3z4a, from the opents lane): PR #7 merged
  (c967f5c, CI green on cee7e98) and merged into local main. games/opents-coop.toml
  is now the co-op campaign template with `local_players = 1` and
  `[tier0] relaunch_linger = 900` (score screen + briefing + host version
  choice can keep the port silent ~10 min; supersedes the 120 s suggested
  below). Both fields need the 0.1.2 server: push it to vtt config/games/
  only as part of the pending 0.1.2 deploy (see the PR #8 entry), never
  before. After that deploy: two-network test of OpenTS Co-op with Jan.

- 2026-10-16 (request t_mvb4dzeyi7k6y): server fault reporting. New optional
  `[fault_reports] url/lane` in server.toml: every log::error from the server's
  own code (ApiError::internal = every 500, ladder start/finalize, udp, broken
  staff file), failed starts and panics (hook sends synchronously; release
  builds abort on panic) go out as JSON to JARVIS's report API. Dedup 6 h per
  title, max 20/h. Tested end to end against a local receiver. The report URL
  is SECRET and the repo is public: it lives only in vtt
  /root/warpzone/config/server.toml. NOT on vtt yet — Config is
  deny_unknown_fields, so add `[fault_reports] url = "<report URL>"` (lane
  defaults to "warpzone") only after a release containing this is deployed
  (0.1.2 deploy is still pending too). Launcher/libwarpzone run on players'
  machines and must never carry the secret; not wired there. Docker
  healthcheck failures are not reported (would need a host-side watcher).

- 2026-10-16 (request t_mvb58cu8m4249, furries lane): Funny Furries 0.5.0
  (mod sync) is out, so funny-furries-mods is merged and the game is tier 3
  (tier >= 3 is what marks mods usable in capabilities(); room browser, host
  approval and leaderboards are still missing, see the descriptor comment).
  vtt config/games/funny-furries.toml is this version WITHOUT local_players
  (server still 0.1.1; checked with v0.1.1 check-games) and is live; the 0.1.2
  deploy's "push games/*.toml" step restores the repo version. Furries lane
  told to run the three example mod uploads (FurryWorkshop, id 2); they then
  wait for Jan's moderation.

- 2026-10-16 (Jan's laptop): Play failed for super-mario-land-port ("cannot
  find the Super Mario Land executable"): the descriptor had no [launch] exec
  and the 0.4.0 tarball holds sml in a versioned folder. sml-descriptor-fixes
  is merged into main (exec "*/sml", exact asset names, ROM `present` path).
  vtt config/games/super-mario-land-port.toml now carries that version WITHOUT
  local_players (server 0.1.1; old copy in /root/*.bak-20261016). Do not push
  an older super-mario-land-port.toml to vtt or reopen that branch; the 0.1.2
  deploy's "push games/*.toml" step must use main at or after this note.

- 2026-10-16 (report t_mvb2jb4c4x3o9q, spacehog lane): PR #11 merged
  (games/spacehog.toml, art, launcher pixel scene "spacehog"); check-games,
  launcher pixel test, fmt and clippy pass; spacehog v0.1.0 prerelease has the
  three assets. Tier 0 with [mods]/[[leaderboards]] as roadmap, like
  super-mario-land-port (badges show only from tier 2). The pixel scene ships
  with the next launcher release (not cut yet). vtt config/games/spacehog.toml
  is a hand-edited copy WITHOUT local_players (server 0.1.1): the 0.1.2 deploy's
  "push games/*.toml" step restores the repo version.

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
