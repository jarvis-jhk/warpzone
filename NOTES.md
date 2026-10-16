# Notes

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
