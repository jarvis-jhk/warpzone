# Notes

- 2026-10-19 (request t_mvezo9ej4iovu, Jan): launcher library = SteamGridDB
  portrait tiles (descriptor `steamgriddb` id only; core griddb.rs, launcher
  tileart.rs; cache <data>/cache/griddb), "Change artwork…" picker (grids,
  SGDB search, local file, generated), installed-only filter, controller
  navigation (pad.rs: gamepad thread, spatial D-pad focus, glyph hint bar),
  Steam Deck installer (packaging/steamdeck, release asset
  Install-Warpzone.desktop, `warpzone --add-to-steam`). SteamGridDB has no
  keyless v2 API: without a personal key (Settings) the launcher uses the
  website's public endpoints (`/api/public/search/autocomplete`,
  POST `/api/public/search/assets`, pages count from 0) — undocumented, so if
  tiles fall back to generated art for everyone, check those first. Docs
  screenshots deliberately NOT updated: they would put SGDB community/box art
  into this public repo. Headless shots: system Xvfb :217 + .toolchain/xlibs,
  `SteamDeck=1` shows the hint bar, steps library-pad/art-picker script the
  D-pad. Asked lanes baba-port, bopl-port, rounds-port, towerfall-port,
  opens4 to add their ids (22797, 5389529, 5281211, 3049, 5248521) to their
  vtt descriptors after the 0.1.3 server deploy. The Deck installer's
  zenity/Steam steps were not run on a real Deck (no device in this lane);
  download + SHA-256 check tested with stubbed zenity.
  The port lanes do not reach vtt: their descriptors live in their own repos
  (packaging/warpzone/<id>.toml of baba-port, bopl-port, rounds-port,
  towerfall-port) and this lane copies them to vtt config/games/ on request
  (done for the steamgriddb lines with 0.1.3, requests t_mvf0la0u8ty1q,
  t_mvf0lhe5bg029). They stay out of this repo's games/ (it would go stale);
  opens4.toml is the vtt copy plus `steamgriddb = 5248521`.

- 2026-10-18 (request t_mvczzhug1bsbc6, Jan: "Give Warpzone an option to use
  pure-Rust crypto"): cargo feature `pure-rust-crypto` on every crate
  (default `ring`). Only HTTPS used non-Rust crypto (ureq -> rustls -> ring);
  now ureq is `rustls-no-provider` + webpki roots and every agent comes from
  `http_config()` (core lib.rs, server http.rs) with rustls-rustcrypto
  0.0.2-alpha when the feature is on. Verified: `wz games` over HTTPS
  against the live server, workspace tests and clippy in both modes, no ring
  in the pure tree (CI step added). Release builds stay on ring: the
  provider is alpha/unaudited (pulls rsa 0.9, which has the Marvin timing
  advisory RUSTSEC-2023-0071 — only matters for RSA private-key use, the
  client only verifies). Do not use the look-alike "oxitls-*" crates.
  Not told Jan (releases-only lane).

- 2026-10-18 (Jan: "make sure that the warpzone lane can update the running
  server without nagging me"): the vtt release pin moved out of `.env` into
  `/root/warpzone/docker-compose.override.yml` (Compose merges it by itself;
  resolved config identical, server recreated once, healthy on 0.1.2). The
  gateway classifies every `*.env` as T3 (`dotenv`), so the old deploy always
  asked Jan; the override file is in the warpzone project's vtt scope.
  **Deploy a release on vtt, all without asking Jan** (checked against the
  gateway's own policy, `hostCommandAuto`/`hostFileAuto`), cwd `/root/warpzone`:
  1. `mkdir -p /root/warpzone/backups`, `docker compose stop`, then for each
     of `data/warpzone.db`, `-wal`, `-shm` that exists:
     `cat /root/warpzone/data/warpzone.db > /root/warpzone/backups/warpzone-<old>.db`
     (`cp` out of `data/` is not automatic: data/ is write-protected for workers).
  2. Write `/root/warpzone/docker-compose.override.yml` (gateway file write):
     `WARPZONE_VERSION` and `WARPZONE_SHA256` (of
     `warpzone-server-<v>-x86_64-linux-musl`, from SHA256SUMS) and the image tag.
  3. `docker compose up -d --build`, then `docker compose ps` and
     `curl -s http://127.0.0.1:2998/healthz` (not `docker inspect`: T3).
  4. Descriptors and art: gateway file writes to `config/games/<id>.toml` and
     `config/games/art/<id>.jpg`; the server mounts config read-only, so 0644
     is enough (no chown); it reloads games and releases every 15 min, or at
     once with `docker compose restart`.
  Never: `.env` (gone; never bring it back), `config/staff.toml`, ssh/scp,
  `docker exec`/`run`/`inspect`/`image prune`. Old images stay; they are small.
  The DEPLOY PENDING item of 2026-10-16 below is done (vtt runs 0.1.2).

- 2026-10-17 (report t_mvc5ffe8wno81, furries lane): FF CI run 39017751821
  attempt 1, test_3_ranked_1v1: guest got ROOM_CLOSED after PEER_CONNECTED,
  host nothing (libwarpzone + server v0.1.0). Attempt 2 passed. Not
  reproduced: the same sequence (two code rooms, then 1v1 with the earlier
  guest as ranked host) ran 8x clean on v0.1.0 and 1x on main. The server
  never closes a room for one member: every close (host close/leave, 45 s
  heartbeat sweep, account deletion) queues RoomClosed to all members, host
  included, and the library makes one up only for a host whose heartbeat
  gets not_found. So that host's requests were failing (heartbeat errors
  were swallowed, the event poll retries silently) and the room was swept.
  Why they failed is not visible in the tails (server.log of that run was
  not kept; job logs need GitHub auth). Fix in libwarpzone: host gets
  Error{op:heartbeat} + RoomClosed after 50 s without an accepted heartbeat
  (client::tests::host_without_heartbeats_loses_the_room). Not told Jan
  (releases-only lane).

- 2026-10-17 (report t_mvc4macrq7ykb, furries lane): ranked 4-player lobby
  hang (FF CI test_4_ranked_ffa / test_5_ranked_2v2). Cause: client.rs
  dropped room events (PeerJoined) that the events thread delivered before
  the join answer from the API thread, so the host never knew the last
  guest. Fix in libwarpzone: early room events buffered (60 s) and replayed
  on join, duplicate PeerJoined deduplicated, a peer lost by timeout re-added
  once (PeerLeft then PeerJoined; NetEvent::Lost has `bye`). Why host<->guest
  links went silent for 15 s in that run is not proven (all-loopback, FF
  had auto_join off and joined once); the retry covers it. e2e test
  ranked_ffa_four_players_connect + unit tests. Ships with the next
  libwarpzone release; FF needs to pick it up (told furries lane).

- 2026-10-17 (request t_mvc38s7b0g6uv, Jan): feedback service. Server
  `POST /api/v1/feedback` (no account; game id or "warpzone"; kinds
  crash|bug|idea; kept in table `feedback`, same game+kind+title within 7 d
  counted; 10/h per IP; `GET /moderation/feedback`, `wz feedback`). With
  `[feedback] url` each new report goes out as {lane, kind bug|feature,
  title, detail, source} (30/h), lane from `[feedback] lanes` (game id ->
  lane), else `lane`. Forwarded detail says the text is player-written data.
  Launcher: always-visible bottom bar (game preset = settings.last_game,
  Problem/Idea, Enter sends), crash reports for games killed by
  SEGV/ABRT/BUS/FPE/ILL/TRAP or exiting non-zero (60 log lines, home dir and
  login name scrubbed), own panics; Settings → "Report crashes".
  libwarpzone: wz_report (background), wz_report_problem (no client,
  blocking); `wz report`. e2e test feedback_is_kept_counted_and_passed_on.
  DEPLOY (with the next release, 0.1.3): Config is deny_unknown_fields, so add
  to vtt /root/warpzone/config/server.toml only once the server runs 0.1.3:
    [feedback]
    url = "<the JARVIS report URL — secret, never in this repo>"
    lane = "warpzone"
    lanes = { diablo-splitscreen = "diablo", diablo2-couch = "diablo2",
      drmario64-vs = "drmario", funny-furries = "furries",
      gtetrinet-ng = "tetrinet", opents-coop = "opents", s25coop = "siedler",
      super-mario-land-port = "mario", spacehog = "spacehog", opens4 = "opens4" }
  (and `[fault_reports] url` from the 2026-10-16 entry, still pending). Until
  then feedback is only stored on the server. Asked the game lanes (diablo,
  diablo2, drmario, furries, tetrinet, opents, siedler, mario, spacehog,
  opens4) to report their own faults with wz_report/wz_report_problem once
  libwarpzone 0.1.3 is out.

- 2026-10-17 (report t_mvbq58qs35wt4z, spacehog lane): PR #13 (spacehog tier 2,
  join/host, max_players 2, no [mods]/leaderboards) NOT merged yet: it waits for
  github.com/jarvis-jhk/spacehog/releases/tag/v0.2.0, which does not exist
  (latest v0.1.2). It conflicts with main's `fetch` description text; the
  resolved merge is on local branch pr13-ready (check-games: 9 ok). When v0.2.0
  is out: merge pr13-ready into main. vtt deploy: vtt runs 0.1.2, which rejects
  `fetch`, so push the tier-2 descriptor with the `fetch` entries stripped (as
  vtt's current copy is), or all of main's spacehog.toml once 0.1.3 is deployed.

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
  DEPLOY PENDING (done 2026-10-17, vtt runs 0.1.2): config/server.toml already has the appimage asset
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
