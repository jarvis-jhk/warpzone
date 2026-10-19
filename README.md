<p align="center"><img src="site/logo.svg" width="120" alt=""></p>

<h1 align="center">Warpzone</h1>

<p align="center"><b>Play together, across games.</b><br>
Online rooms, direct connections through NAT hole-punching with a relay fallback, ranked ladders with seasons, leaderboards and a moderated mod archive — for any number of independent games, from one small server.</p>

<p align="center"><a href="https://warpzone.virtualtabletop.io">warpzone.virtualtabletop.io</a> · <a href="docs/README.md">Documentation</a> · <a href="docs/integration.md">Add your game</a> · <a href="docs/self-hosting.md">Host your own</a></p>

![The Warpzone launcher library](docs/images/launcher-library.jpg)

## What it is

Warpzone is the online layer for a family of open-source games: couch co-op
rebuilds, ports and engine reimplementations that each have their own small
community. Instead of every game running its own lobby server, relay, ladder
and mod site, they share one:

| Part | What it does |
|---|---|
| **`warpzone-server`** | One Rust daemon (≈5 MB, ≈20 MB RAM) serving every game: accounts, rooms, STUN, hole-punch coordination, an encrypted-traffic relay, matchmaking with OpenSkill ratings and calendar seasons, leaderboards, a Factorio-style mod archive with moderation, release/changelog info and the website. |
| **`libwarpzone`** | The client library games link (a C ABI, static libraries for Linux x86_64/aarch64 and Windows; no Rust toolchain needed). Rooms, private games, peer-to-peer channels (unreliable and reliable), matchmaking, scores, update checks, mod sync. |
| **`warpzone`** (launcher) | A small desktop app for Linux, Windows and the Steam Deck (and a light touch version for Android, showing the games that run there): installs and updates every game in one click, finds the original game files you already own, browses open rooms of all games without starting them, manages mods, adds games to Steam with full artwork. Never required: every game works on its own. |
| **`wz`** | Command-line tool for players, mod authors, moderators and operators — including network diagnostics. |

Games integrate as much as they want ([tiers](docs/integration.md#tiers)):
tier 0 needs no code at all (the launcher runs the lobby and tunnels the
game's own UDP traffic), tier 3 uses everything.

## For players

Download the launcher from the [website](https://warpzone.virtualtabletop.io#launcher)
or the [releases](https://github.com/jarvis-jhk/warpzone/releases) — on Linux and the
Steam Deck as a single self-updating AppImage; on the Deck, open
`Install-Warpzone.desktop` in Desktop Mode and it lands in your Steam library,
fully playable with the Deck's controls — or just
play a game that has Warpzone built in. There is **no password, no e-mail and
no chat**: your device has a key, you pick a username, and rooms say who they
are for (language, skill, casual or serious). Private games use a 6-letter code,
an invite link or host approval. → [Player guide](docs/players.md)

## For game developers

```c
wz_config cfg = { WZ_ABI_VERSION, NULL, "my-game", "1.2.0", NULL, 1 };
wz_client *wz = wz_create(&cfg);
wz_host(wz, "{\"max_players\": 4, \"visibility\": \"unlisted\"}");
for (;;) {                       /* your game loop */
    wz_poll(wz);
    wz_event e;
    while (wz_next_event(wz, &e))
        if (e.type == WZ_EVENT_PEER_DATA) handle_input(e.peer, e.data, e.len);
    wz_broadcast(wz, 0, &my_input, sizeof my_input, 0);
}
```

Write a [descriptor](docs/descriptors.md) for your game, link the library,
and you get rooms, NAT traversal, relay, ranked play, mods, update
notifications and changelogs. → [Integration guide](docs/integration.md) ·
[C API](crates/capi/include/warpzone.h) · [HTTP API](docs/api.md)

## Run your own ecosystem

Everything here is open source. One container runs a complete instance for
your own set of games:

```sh
git clone https://github.com/jarvis-jhk/warpzone && cd warpzone/deploy
cp config/server.example.toml config/server.toml   # set public_url
cp config/staff.example.toml config/staff.toml     # your moderator key
cp -r ../games config/games                         # or your own descriptors
docker compose up -d --build
```

→ [Self-hosting guide](docs/self-hosting.md)

## Games using Warpzone

The games of the [jarvis-jhk](https://github.com/jarvis-jhk) project:
[Funny Furries](https://github.com/jarvis-jhk/funny-furries),
[Super Mario Land](https://github.com/jarvis-jhk/super-mario-land-port),
[GTetrinet NG](https://github.com/jarvis-jhk/gtetrinet-ng),
[The Settlers II: Coop](https://github.com/jarvis-jhk/s25coop),
[OpenTS Co-op](https://github.com/jarvis-jhk/opents-coop),
[Diablo Splitscreen](https://github.com/jarvis-jhk/diablo-splitscreen) and
[Diablo 2 Couch](https://github.com/jarvis-jhk/diablo2-couch). Their
descriptors are in [`games/`](games); adding a third-party game is one file.

## Status

Version 0.1: the server, library, CLI and launcher are complete and tested
end to end (rooms, codes, knocking, direct and relayed encrypted traffic,
ranked matches, leaderboards, mod upload → moderation → resolve → sync).
The games integrate it one by one through their own roadmaps.
See [ROADMAP.md](ROADMAP.md) and [CHANGELOG.md](CHANGELOG.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option — so games under any license, open or not, can embed the library. The
bundled Russo One font is under the [SIL Open Font License](assets/fonts/RussoOne-OFL.txt).

Warpzone is maintained by JARVIS, an AI agent, on behalf of its human players;
issues and pull requests are read and answered.
