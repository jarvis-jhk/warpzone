# Roadmap

JARVIS maintains Warpzone; players and game developers add wishes as issues.
Order is priority. Done items move to the [changelog](CHANGELOG.md).

## Current priorities

1. **Games integrate Warpzone** through their own roadmaps: Funny Furries
   (rollback online, tier 3), Super Mario Land (mods, speedrun leaderboards,
   16-player shared screens), GTetrinet NG, The Settlers II: Coop (next to the
   RTTR lobby), OpenTS Co-op (verify the tier-0 lobby end to end), the Diablo
   ports. Warpzone follows their needs.
2. **NAT test matrix in CI**: Linux network namespaces with nftables
   emulating full-cone, restricted, port-restricted and symmetric NAT, IPv4
   and IPv6, plus relay fallback.
3. **Signed endpoint bootstrap**: a minisign-signed `endpoints.json` in this
   repository, so an instance can move to another domain without breaking
   old clients.
4. **Automated pre-review of mod submissions** (a reviewer key adds notes:
   file listing, executables, likely copyrighted content) — decisions stay
   with the human moderator.

## Next

* Friends (mutual, by exchanging codes) and a *friends* room visibility;
  background notifications "a friend hosts".
* Tier-0 **TCP** tunnel (for games with TCP lobbies, such as Tetrinet).
* Optional full-mesh rooms for games that cannot relay through the host.
* Launcher: tray icon for background mode, Windows toast notifications,
  mod upload from the launcher, mod previews and screenshots, Steam Input
  controller templates for added shortcuts.
* Mod voting and reviews (structured, no free text) and trusted authors.
* macOS builds of the library and launcher.

## Deliberately not planned

* **Chat** — deferred: it needs moderation designed first.
* Accounts with passwords or e-mail.
