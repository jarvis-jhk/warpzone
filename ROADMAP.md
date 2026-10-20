# Roadmap

JARVIS maintains Warpzone; players and game developers add wishes as issues.
Order is priority. Done items move to the [changelog](CHANGELOG.md).

## Current priorities

1. **Games integrate Warpzone** through their own roadmaps: Funny Furries
   (rollback online, tier 3), Super Mario Land (mods, speedrun leaderboards,
   16-player shared screens), GTetrinet NG, The Settlers II: Coop (next to the
   RTTR lobby), OpenTS Co-op (verify the tier-0 lobby end to end), the Diablo
   ports. Warpzone follows their needs.
2. **macOS, the rest** (libraries, `wz` and the launcher are out since
   0.1.11): the launcher's autostart / background mode, notifications,
   `warpzone://` links and *Add to Steam* on macOS; a notarised, signed
   launcher once there is an Apple Developer ID.
3. **LAN play without internet**, seamlessly: everything works as it does
   now — same UI in the launcher and in games, same rooms, codes and
   joining — but when devices have no internet and share a LAN, they find
   each other's games by themselves (local auto-discovery, e.g. a broadcast /
   multicast beacon, no server needed) and connect directly over the LAN.
   (Jan, 2026-10-19)
4. **Federation**: any number of Warpzone servers work together, with cross
   play between their players. Every server keeps a list of the other
   servers it knows and keeps it current by exchanging it with them
   (gossip: new servers spread, dead ones drop out). Every game and Warpzone
   itself start from a list of known servers kept in their repositories (for
   now only warpzone.virtualtabletop.io). Lightweight and "just works": no
   central registry, no setup beyond pointing a new server at one it knows;
   rooms, the room browser, codes and invites span servers; players keep
   their device key as identity. (Jan, 2026-10-19)
5. **NAT test matrix in CI**: Linux network namespaces with nftables
   emulating full-cone, restricted, port-restricted and symmetric NAT, IPv4
   and IPv6, plus relay fallback.
6. **Signed endpoint bootstrap**: a minisign-signed `endpoints.json` in this
   repository, so an instance can move to another domain without breaking
   old clients (likely the same file as federation's known-server list).
7. **Automated pre-review of mod submissions** (a reviewer key adds notes:
   file listing, executables, likely copyrighted content) — decisions stay
   with the human moderator.
8. **Sync across a player's devices**: settings, progress and savegames
   follow the same player from device to device. The devices of one player
   are linked into one identity by exchanging a code (no passwords or
   e-mail; each device keeps its own key). Settings and progress sync for
   everyone; savegames only with the account's permission (storage quota
   per account, later a Patreon tier). A small per-game key/value and blob
   API in `libwarpzone` and `wz`, last-writer-wins with conflict copies for
   savegames. (Jan, 2026-10-19)
9. **Steam compatibility tool "Warpzone"** for the Steam Deck (and Steam on
   Linux): on the Deck in Game Mode you install a game such as ROUNDS
   normally from Steam, pick *Warpzone* as its compatibility tool in the
   game's Steam properties and press Play — Warpzone then downloads, keeps
   updated and launches our open-source version (e.g. the rounds-port)
   with the player's own game files, instead of the original. The tool is
   installable the way Steam Deck users expect (dropped into
   `compatibilitytools.d`, e.g. via ProtonUp-Qt-style install), and the
   launcher can also install it with one click. (Jan, 2026-10-19)

## Next

* Friends (mutual, by exchanging codes) and a *friends* room visibility;
  background notifications "a friend hosts".
* Tier-0 **TCP** tunnel (for games with TCP lobbies, such as Tetrinet).
* Optional full-mesh rooms for games that cannot relay through the host.
* Launcher: tray icon for background mode, Windows toast notifications,
  mod upload from the launcher, mod previews and screenshots, Steam Input
  controller templates for added shortcuts.
* Mod voting and reviews (structured, no free text) and trusted authors.

## Deliberately not planned

* **Chat** — deferred: it needs moderation designed first.
* Accounts with passwords or e-mail.
