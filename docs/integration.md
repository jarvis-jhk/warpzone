# Integrating a game

This guide takes a game from "not online" to "everything": rooms, private
games, NAT traversal, ranked play, leaderboards, mods, update notifications.
Each step stands alone; stop at the tier that fits your game.

* [Tiers](#tiers)
* [1. Describe the game](#1-describe-the-game)
* [2. Link the library](#2-link-the-library)
* [3. The client lifecycle](#3-the-client-lifecycle)
* [4. Tier 1: joining from the launcher](#4-tier-1-joining-from-the-launcher)
* [5. Tier 2: hosting, browsing, playing](#5-tier-2-hosting-browsing-playing)
* [6. Tier 3: ranked, leaderboards, mods](#6-tier-3-ranked-leaderboards-mods)
* [7. Updates and changelogs](#7-updates-and-changelogs)
* [8. Testing locally](#8-testing-locally)
* [9. Rust games](#9-rust-games)
* [Checklists](#checklists)

## Tiers

| Tier | You implement | Players get |
|---|---|---|
| **0** | nothing (a descriptor with a `[tier0]` section) | The launcher runs the lobby; your game's own UDP traffic is tunnelled over Warpzone, including NAT traversal and relay. Your game must already support LAN/IP play and be startable from a launch file or arguments. |
| **1** | `--warpzone-join <room>` | "Join" buttons in the launcher and on the website start your game straight into the room. |
| **2** | host screen and room list through the library | Online play without the launcher: public and private rooms, codes, invite links, host approval, direct or relayed encrypted connections. |
| **3** | queue, results, scores, mod sync | Ranked queues with seasons, leaderboards, an in-game mod browser and automatic mod sync on join. |

## 1. Describe the game

Every game has a [descriptor](descriptors.md) in the server's `games/`
directory. The id you choose there (`my-game`) is what you pass to the
library. A minimal one for a tier-2 game:

```toml
id = "my-game"
name = "My Game"
tagline = "One line for the library tile."
tier = 2

[install]
github = "me/my-game"
assets = { linux = "my-game-*-linux-x86_64.tar.gz", windows = "my-game-*-windows-x86_64.zip" }

[launch]
exec = { linux = "my-game/my-game", windows = "my-game/my-game.exe" }
join = ["--warpzone-join", "{room}"]
host = ["--warpzone-host"]

[rooms]
max_players = 4
presets = [{ id = "ffa", label = "free-for-all" }]
display = [{ key = "map", label = "Map" }]
```

Send it as a pull request to [`games/`](../games) (or put it on your own
instance). `wz` and the server validate descriptors: `warpzone-server
check-games games`.

## 2. Link the library

Download `libwarpzone-<version>-<target>` from the
[releases](https://github.com/jarvis-jhk/warpzone/releases):

| Target | File | Link |
|---|---|---|
| Linux x86_64 (glibc) | `libwarpzone-…-x86_64-linux-gnu.tar.gz` | `lib/libwarpzone.a -lpthread -ldl -lm` |
| Linux aarch64, glibc ≥ 2.27 (PortMaster, Pi) | `libwarpzone-…-aarch64-linux-gnu.tar.gz` | same |
| Windows, MSVC | `libwarpzone-…-x86_64-windows-msvc.zip` | `lib/warpzone.lib` + the system libraries in `NATIVE_LIBS.txt` |
| Windows, MinGW / Zig | `libwarpzone-…-x86_64-windows-gnu.zip` | `lib/libwarpzone.a` + `-lws2_32 -luserenv -lntdll -lbcrypt -ladvapi32` (see `NATIVE_LIBS.txt`) |

Each package has `include/warpzone.h` and a shared library as an alternative.
Pin the version and its SHA-256 from the release's `SHA256SUMS` in your build,
exactly like you pin other toolchain downloads.

CMake:

```cmake
add_library(warpzone STATIC IMPORTED)
set_target_properties(warpzone PROPERTIES
  IMPORTED_LOCATION ${WARPZONE_DIR}/lib/libwarpzone.a
  INTERFACE_INCLUDE_DIRECTORIES ${WARPZONE_DIR}/include)
target_link_libraries(my-game PRIVATE warpzone pthread dl m)
```

Building from source instead: `cargo build --release -p warpzone-capi`
produces `target/release/libwarpzone.a`. The static library adds about
3 MB to a release binary.

## 3. The client lifecycle

```c
#include "warpzone.h"

wz_config cfg = {
    .abi = WZ_ABI_VERSION,
    .server = NULL,              /* WARPZONE_SERVER or the public instance */
    .game = "my-game",
    .game_version = "1.2.0",
    .home = NULL,                /* the shared data directory */
    .auto_join_matches = 1,
};
wz_client *wz = wz_create(&cfg);
if (!wz) fprintf(stderr, "warpzone: %s\n", wz_last_error());

/* every frame */
wz_poll(wz);
wz_event e;
while (wz_next_event(wz, &e)) {
    switch (e.type) {
    case WZ_EVENT_LOGGED_IN:     /* e.json: {"account": null | {...}, "roles": [...]} */ break;
    case WZ_EVENT_PEER_DATA:     handle(e.peer, e.channel, e.data, e.len); break;
    case WZ_EVENT_ERROR:         show_error(e.json); break;  /* {"op","code","message"} */
    default: break;
    }
}

/* on exit */
wz_destroy(wz);                  /* leaves or closes the current room */
```

Rules that keep it simple:

* Call every `wz_*` function from your game thread. The library runs two
  internal threads for HTTP and **never calls into your code**.
* Requests return immediately; their outcome is an event. Nothing blocks a
  frame — `wz_poll` only moves results and processes UDP packets.
* `e.data` and `e.json` stay valid until the next `wz_next_event`, `wz_poll`
  or `wz_destroy`. Copy what you keep.
* JSON in events has exactly the shapes documented in the [API reference](api.md).
* Offline is normal: if the server cannot be reached you get
  `WZ_EVENT_ERROR {"op":"login"}` and the library retries in the background.
  Local play must not depend on Warpzone.

### Usernames

On first start a device has a key but no username:
`WZ_EVENT_LOGGED_IN` carries `"account": null`. Ask the player for a name
(3–16 letters, digits, `_` or `-`) and call `wz_register(wz, name)`; the
result is `WZ_EVENT_REGISTERED` or `WZ_EVENT_ERROR` with a message you can
show as-is ("that username is taken"). The identity is shared with the
launcher and every other Warpzone game of this user, so most players only do
this once. `wz_username()` returns the current name.

## 4. Tier 1: joining from the launcher

The launcher and the website start your game with
`--warpzone-join <room>` (and `--warpzone-host` for "Host"). It has already
joined the room as the same player, so your join succeeds without a code.

```c
int intent = wz_handle_launch(wz, argc, (const char *const *)argv);
if (intent == 1) show_connecting_screen();     /* joining the room */
if (intent == 2) open_host_screen();           /* then wz_host(...) */
```

`wz_handle_launch` also understands invite links passed as an argument
(`warpzone://join/<room>#<secret>`), `--warpzone-code <CODE>` (join a private
game by its code; with `--warpzone-join` it is that room's code) and the
environment variables `WARPZONE_JOIN` / `WARPZONE_HOST=1`.

### Android

The [Android launcher](launcher.md#android) cannot pass a command line. It
starts the game's launch activity with the same arguments as the
string-array extra **`warpzone.args`** (e.g. `["--warpzone-join", "<room or
invite link>"]`, `["--warpzone-host"]`, `["--warpzone-code", "K7QM3X"]`),
plus `warpzone.launched` (boolean) and `warpzone.server` (string). Read them
in `onCreate` and `onNewIntent` (the launcher adds `FLAG_ACTIVITY_CLEAR_TOP`
when it passes arguments, so a running game is restarted into the room) and
hand them to `wz_handle_launch` or `warpzone_core::launch::from_args`. The
Android launcher has no account of its own: it has not joined the room
first, so a private room needs its code or invite link, which it passes on.
Name the package in the descriptor (`launch.exec.android`) and publish the
APK with the release (`install.assets.android`), listed in the release's
`SHA256SUMS`: the launcher installs nothing it cannot check.

## 5. Tier 2: hosting, browsing, playing

### Hosting

```c
wz_host(wz,
  "{\"max_players\": 4,"
  " \"visibility\": \"public\","            /* or \"unlisted\" */
  " \"entry\": \"open\","                   /* or \"code\", \"approve\" */
  " \"audience\": {\"languages\": [\"en\"], \"skill\": \"mixed\", \"style\": \"casual\"},"
  " \"preset\": \"ffa\","
  " \"metadata\": {\"map\": \"Arena 3\"}}");
```

* `WZ_EVENT_ROOM_HOSTED` → `{"room": {...}, "code": "K7QM3X" | null, "invite": "warpzone://join/…#…"}`.
  Show the code (unlisted and code rooms always have one) and offer to copy the invite link.
* The room name is generated from the player's name and the preset label
  ("Alex's free-for-all"). Free text is deliberately not possible.
* `wz_set_metadata(wz, "{\"map\": \"Arena 5\"}")` updates what browsers show.
* `wz_set_closed(wz, 1)` when the match starts stops new joiners; `0` reopens.
* The library sends the heartbeats; a room disappears 45 s after the host's
  game stops polling.

Entry rules and what you must handle:

| `entry` | The joiner… | You handle |
|---|---|---|
| `open` | joins at once | — |
| `code` | must type the code (or use the invite link) | — |
| `approve` | knocks | `WZ_EVENT_KNOCK` `{"knock","account","username"}` → `wz_answer_knock(wz, knock, 1/0)` |

Guests appear as `WZ_EVENT_PEER_JOINED` (`{"account","username"}`), then
`WZ_EVENT_PEER_CONNECTED` (`{"relayed": bool}`) once a path works.
`wz_kick(wz, account, ban)` removes one.

### Browsing and joining

```c
wz_list_rooms(wz, 1);        /* → WZ_EVENT_ROOM_LIST: [RoomInfo, ...] with free slots */
wz_join(wz, room_id, NULL, NULL);
wz_join_code(wz, "K7QM3X");
```

`RoomInfo` carries `name`, `players`/`max_players`, `audience`, `metadata`,
`game_version`, `lockfile` and `entry`. Show the audience; filter out rooms
whose `game_version` you cannot play. Joining gives `WZ_EVENT_ROOM_JOINED`
(`"host": false`), or `WZ_EVENT_KNOCKING` and later `ROOM_JOINED` or
`WZ_EVENT_KNOCK_DENIED`.

### Sending data

```c
wz_send(wz, peer, channel, data, len, reliable);
wz_broadcast(wz, channel, data, len, reliable);   /* to every peer */
```

* **Unreliable** (`reliable = 0`): a datagram, may be lost or reordered,
  dropped if the peer is not connected yet. For inputs, snapshots, rollback.
* **Reliable** (`reliable = 1`): delivered once, in order per peer, queued
  until connected. For chat-free game events, lobby state, results.
* At most `WZ_MAX_DATAGRAM` (1100) bytes per call. Split larger data yourself.
* Channels 0–249 are yours (a byte you can use to route messages); 250–255
  are reserved.
* Rooms are a **star**: a guest is connected to the host only. Games where
  everyone talks to everyone relay through the host (forward what a guest
  sends to the others). That is also what the tier-0 tunnel does.
* `wz_peer_rtt_ms`, `wz_peer_relayed`, `wz_peer_name`, `wz_peer_account`
  describe a peer. Show "relayed" in your network screen: relayed connections
  work but add latency.

Peers leave with `WZ_EVENT_PEER_LEFT` (gracefully, kicked, or 15 s of
silence). The host closing or timing out gives guests `WZ_EVENT_ROOM_CLOSED`.

### Determinism and rollback

Warpzone transports bytes; it imposes no tick model. Deterministic games
usually send inputs unreliably every tick with a little redundancy (the last
few ticks per packet) and run rollback or lockstep themselves. Nothing in the
library uses the host clock for game logic.

## 6. Tier 3: ranked, leaderboards, mods

### Ranked queues

Queues are declared in the descriptor (`[[ladder]]`: teams and team size).

```c
wz_queue(wz, "1v1", NULL);            /* NULL ruleset = "stock" */
/* WZ_EVENT_QUEUED       {"queued": true, "waiting": 3, ...}
   WZ_EVENT_MATCH_FOUND  {"match_id","queue","room","host","teams":[["Ann"],["Bob"]]}
   (the library joins the match room automatically; play as usual) */
wz_report_match(wz, match_id,
  "{\"placements\": [[17], [42]], \"state_hash\": \"9f2c…\"}");
/* WZ_EVENT_MATCH_FINISHED {"match_id","state": "rated" | "disputed" | "abandoned"} */
```

* `placements[0]` is the winning team, as account ids (from
  `WZ_EVENT_PEER_JOINED` and your own account). Ties share an entry.
* Every player reports. Identical reports (and identical `state_hash` if you
  send one — recommended for deterministic games) are rated; disagreement is
  `disputed` and unrated. If the others never report, the first report counts
  after 10 minutes.
* Ratings, seasons and matchmaking: [ladder.md](ladder.md).

### Leaderboards

```c
wz_submit_score(wz, "any", 4123456 /* ms */, NULL /* stock */, replay_sha256_or_NULL);
/* WZ_EVENT_SCORE {"board","result":{"best","rank","improved"}} */
wz_request_board(wz, "any", "stock", "all");      /* WZ_EVENT_BOARD */
```

`time` boards rank lower values first, `score` boards higher. Use the
ruleset `stock` for unmodified play and the lockfile hash for modded play —
records never mix.

### Mods

Mods are published by players and approved by a moderator ([mods.md](mods.md)).
Your game decides what a mod *is*; Warpzone delivers files.

* A room's `lockfile` (hash) says which mod set everyone runs. Before
  connecting, call `wz_sync_mods(wz, hash)`: you get `WZ_EVENT_MODS_PROGRESS`
  and then `WZ_EVENT_MODS_SYNCED` with `paths` — the archives (or unpacked
  directories, if your descriptor says `install = "extract"`) **in load
  order**. Load exactly those.
* To host with mods, resolve the player's chosen set through the API
  (`POST /api/v1/games/{game}/mods/resolve`) and pass its hash as `lockfile` in
  `wz_host`. The launcher's Mods tab does this for players too: the active
  set is in `<data>/mods/<game>/active.lock.json`.
* Show the "mods for this game may contain code" warning if your descriptor
  sets `contains_code`.

## Game files the launcher found

For every `[[data]]` requirement the launcher found a source for, it starts the
game with `WARPZONE_DATA_<ID>=<path>` in the environment (`id = "rom"` →
`WARPZONE_DATA_ROM`; upper case, `-` and `.` become `_`), on every start, and
with the descriptor's `import_args` until the `present` path exists. Read the
variable at start: import from it if you have not yet, or simply use the
files there. That way the player never has to point your game at files the
launcher already found.

## 7. Updates and changelogs

```c
wz_check_update(wz);
/* WZ_EVENT_UPDATE {"current":"1.2.0","update_available":true,
                    "latest": {...}, "changes":[{"version","name","notes",...}]} */
```

* Show `changes[].notes` (Markdown from your GitHub release) on the first
  start of a new version, and an "update available" hint when
  `update_available` is true.
* If your game has its own updater: when its install directory contains
  `.warpzone-managed`, the launcher installed it and keeps it updated —
  **your updater must stay silent** then. Standalone installs keep yours.

## Reporting crashes and problems

Send your game's real faults to the server instead of a log nobody reads;
they reach whoever looks after the game on that server:

```c
/* a caught fault: a corrupt save, a failed load, a failed assertion */
wz_report(wz, "bug", "save file failed to load", details);

/* a crash: in your crash handler write what you know to a file; send it on
   the next start (wz_report_problem needs no client and may block) */
wz_report_problem(NULL, "my-game", "1.2.0", "crash",
                  "SIGSEGV in render_frame", backtrace_text);
```

* `title` is one line that stays the same for the same fault (keep
  addresses, times and counts in `detail`): identical titles are counted,
  not repeated.
* Only real faults — never ordinary player mistakes — and never passwords,
  tokens or personal data. The home directory and login name are removed.
* Started from the launcher, a crashing game (killed by a signal, a non-zero
  exit) is already reported with its last log lines; your own reports add
  what only the game knows (a stack, the state it was in).
* No libwarpzone? `wz report <game> crash "<title>" --detail file`, or POST
  to `/api/v1/feedback` ([api](api.md)) — `WARPZONE_SERVER` holds the server
  when the launcher started the game.

## 8. Testing locally

```sh
cargo run -p warpzone-server -- --config dev/server.toml     # http://127.0.0.1:2998
export WARPZONE_SERVER=http://127.0.0.1:2998
WARPZONE_HOME=/tmp/a ./my-game &      # first player (own identity)
WARPZONE_HOME=/tmp/b ./my-game        # second player
```

* Each `WARPZONE_HOME` is a separate identity; register a different name in each.
* `wz host my-game` / `wz join my-game <code>` start a test peer that echoes
  data — handy as the "other side" while you develop.
* `wz join my-game <code> --relay` forces the relay, so you can test that
  path on one machine.
* `wz netcheck` shows the addresses the server sees for this machine.

## 9. Rust games

Use `warpzone-core` directly; [`client::Client`](../crates/core/src/client.rs)
is the same API as the C library with Rust types:

```rust
use warpzone_core::client::{Client, Config, Event, HostOptions};
let mut wz = Client::new(Config::new("my-game", "1.2.0"))?;
wz.host(HostOptions { max_players: 4, ..Default::default() });
loop {
    wz.poll();
    while let Some(e) = wz.next_event() {
        if let Event::PeerData { peer, data, .. } = e { /* … */ }
    }
}
```

## Checklists

**Tier 0** — descriptor with `[tier0]` (transport, game port, settings,
launch-file templates, arguments); verify a staged game end to end between
two machines on different networks.

**Tier 1** — `wz_create` at start, `wz_handle_launch`, a "connecting…"
screen, errors shown.

**Tier 2** — username prompt; host screen with visibility, entry and
audience; room browser with audience and version filters; code entry; knock
handling; connection status (direct/relayed, RTT); peer leave and room
close handled; `wz_set_closed` when the game starts.

**Tier 3** — ranked queue UI and result reporting with a state hash; score
submission; mod sync before connecting and loading in lockfile order;
update/changelog screen; `.warpzone-managed` respected by your updater.
