# Notes

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
