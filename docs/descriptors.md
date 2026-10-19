# Game descriptors

A descriptor is one TOML file per game, `games/<id>.toml` on the server
(`games_dir` in its configuration). It tells the server, the website and the
launcher everything they need — what the game supports, how to install and
start it, what original files it needs, what its mod categories are called.
The server never needs to know how the game works.

The server re-reads the directory within a minute of a change; invalid files
are skipped with a log line. Check them with:

```sh
warpzone-server check-games games      # or: cargo run -p warpzone-server -- check-games games
```

The Rust definition is [`crates/protocol/src/descriptor.rs`](../crates/protocol/src/descriptor.rs).
Real examples: [`games/`](../games).

## Adding a game

1. Copy a similar descriptor, pick an id (2–40 characters of `a-z 0-9 -`,
   also the file name).
2. Fill in `[install]` and `[launch]` from your release layout.
3. Add `[rooms]`, `[[ladder]]`, `[[leaderboards]]`, `[mods]` for what the game
   implements — or `[tier0]` if the launcher should run its lobby.
4. Add `[[data]]` if players must bring original files.
5. Validate, then open a pull request (or drop it into your own instance).

Third-party games are welcome: set `third_party = true` (the website shows a
badge), point `[install]` at their releases, and use `[tier0]` if they can be
started into a network game from a file or arguments.

## Top level

| Key | Type | Meaning |
|---|---|---|
| `id` | string | stable id, = file name |
| `name` | string | display name |
| `tagline` | string | one line for tiles and cards |
| `description` | string | paragraphs (blank line between) for the game page |
| `homepage` | URL | project page |
| `art` | URL | banner image for the website card (else a generated gradient) |
| `accent` | `#rrggbb` | accent colour of cards and generated artwork |
| `motif` | string | generated artwork style: `arena`, `blocks`, `castle`, `crypt`, `pipes`, `ruins`, `tanks`, `waves` (default) |
| `steamgriddb` | integer | the game's [SteamGridDB](https://www.steamgriddb.com) id (`steamgriddb.com/game/<id>`): the launcher's library tile shows its best portrait grid, looked up at run time and cached; players can pick another. Without it, the tile is generated |
| `tier` | 0–3 | integration [tier](integration.md#tiers) the game implements today; online/ranked/mod badges show only from tier 2 |
| `local_players` | number | players on one machine with controllers — the controller icon in the library and on the website |
| `third_party` | bool | not made by this project |
| `status` | string | badge such as `preview`, `in development` |
| `requires` | string | what players must bring, shown before installing |
| `hidden` | bool | not listed (still served by id) |
| `season` | `monthly` \| `quarterly` \| `yearly` | ladder season length (default quarterly) |

## `[install]` — where releases come from

```toml
[install]
github = "owner/repo"
prerelease = true                 # offer prereleases when there is no full release
checksums = "SHA256SUMS"          # checksum file in the release (optional)
minisign_key = "RWQ..."           # optional: require <asset>.minisig signatures
unpack = "extract"                # extract (default: .tar.gz / .zip) or file
assets = { linux = "game-*-linux-x86_64.tar.gz", deck = "game-*-linux-x86_64.tar.gz", windows = "game-*-windows-x86_64.zip", portmaster = "game-*-portmaster.zip" }
```

Asset patterns use `*`. Platforms: `linux`, `windows`, `deck` (falls back to
`linux`), `portmaster` (download button only), `android` (an APK: the
[Android launcher](launcher.md#android) shows only games that have one, and
installs it through Android's package installer; servers before 0.1.6 reject
the key). The launcher verifies every
download against **GitHub's own SHA-256 digest** of the asset (or the
checksum file), never against the Warpzone server, and with `minisign_key`
also against a signature.

## `[launch]` — starting the game

```toml
[launch]
exec = { linux = "game/run.sh", deck = "game/run.sh", windows = "game/Game.exe" }
args = []                                   # always passed
join = ["--warpzone-join", "{room}"]        # tier ≥ 1
host = ["--warpzone-host"]                  # tier ≥ 2
import = []                                 # the game's own first-start importer
```

`exec` is relative to the install directory; a `*` matches one path segment
(versioned top-level folders). The launcher sets `WARPZONE_LAUNCHED=1` and
`WARPZONE_SERVER` in the game's environment.

On Android, `exec.android` is the game's **package name**
(`exec = { ..., android = "org.babaport" }`): the Android launcher uses it to
see whether the game is installed and which version, and to start it. `join`
and `host` arrive as the string-array extra `warpzone.args` of the launch
intent (plus `warpzone.launched = true` and `warpzone.server`), see
[integration](integration.md#android).

## `[rooms]`

```toml
[rooms]
max_players = 4
presets = [{ id = "ffa", label = "free-for-all" }, { id = "2v2", label = "2 vs 2" }]
display = [{ key = "map", label = "Map" }, { key = "mode", label = "Mode" }]
```

Presets name rooms ("Alex's 2 vs 2"). `display` lists the metadata keys the
browser and website show, in order, with their labels. Keys are 1–32
characters of `a-z 0-9 _ - .`.

## `[[ladder]]` and `[[leaderboards]]`

```toml
[[ladder]]
id = "2v2"
title = "2 vs 2"
teams = 2
team_size = 2        # default 1; teams × team_size ≤ 16

[[leaderboards]]
id = "any"
title = "Full game, any%"
kind = "time"        # time (ms, lower wins) | score (higher wins)
seasonal = true      # also keep per-season boards (all-time is always kept)
```

## `[mods]`

```toml
[mods]
categories = [{ id = "level", label = "Levels", description = "optional" }]
max_size_mb = 50          # per archive
contains_code = false     # mods can contain scripts/DLLs: warn players
install = "archive"       # archive (keep the file) | extract (unpack it)
mounts = { level = "levels" }   # tier 0: link active mods into these folders
```

See [mods.md](mods.md).

## `[[data]]` — original game files

For engines and ports that need the player's own copy of a game. The launcher
searches Steam libraries (including Proton prefixes), GOG/Heroic/Lutris/Wine
installs, the Downloads folder, the Desktop and mounted discs, checks what it
finds against known fingerprints **before downloading the engine**, offers
buy and free links, and hands the files over.

```toml
[[data]]
id = "rom"
label = "Super Mario Land ROM (World, Rev A / v1.1)"
description = "Shown to the player."
files = ["*super mario land*.gb", "sml.gb"]     # what identifies a source
steam = ["Folder Under steamapps/common"]
gog = ["GOG Folder Name"]
downloads = true          # search Downloads/Desktop (default true)
discs = true              # search mounted discs (default true)
buy = [{ label = "GOG.com", url = "https://..." }]
free = [{ label = "Shareware", url = "https://..." }]
fetch = [                 # freeware the launcher downloads by itself (0.1.3+)
  { label = "DOS Games Archive", url = "https://...", file = "game.zip", size = 554767, sha256 = "…", license = "Freeware: \"you may distribute it in its original form\" (README.TXT)" },
]
import_args = ["--rom", "{path}"]     # hand-over A: start the game with these once
copy_to = "~/.local/share/game/data"  # hand-over B: copy it here ({install} = install dir)
present = ["~/.local/share/game/data/DATA"]   # already in place → don't ask
optional = false          # true: nice to have (e.g. sound), never blocks installing
known = [
  { label = "World, Rev A (v1.1)", size = 65536, sha1 = "…", sha256 = "…" },
  { label = "v1.0 — not supported yet", size = 65536, sha1 = "…", supported = false },
]
```

In Downloads, on the Desktop and in folders the player chose, the launcher
also looks into `.zip` files and zips inside them (a freeware archive that
holds the game's original zip): only the part that matches is unpacked, once,
into `<warpzone home>/unpacked/`, and that folder or file is what the game gets.
So players can leave the download as it is. Installers (`.exe`) are not
opened; their files are found after running them, also in `C:\Games` and
Wine's `drive_c/games`.

`fetch` is for data whose licence lets anyone pass it on unchanged (freeware,
some shareware): when nothing is found, the launcher downloads it on
*Install* or *Play* (or *Download now*) into `<warpzone home>/downloads/<id>/`,
trying the entries in order, and keeps a file only if it has exactly the
pinned `size` and `sha256`. It is then found like a player's own download
(zips are looked into), so `files` still decides what is used. `license` is
shown to the player: quote or name what allows it. Never list data that is
only sold, or freeware whose licence forbids passing it on; a page that hosts
a file is not a licence. Servers strip `fetch` for launchers up to 0.1.2.

File patterns are case-insensitive globs:

| Pattern | Matches |
|---|---|
| `DIABDAT.MPQ`, `setup_game_*.exe` | a file |
| `DATA/IO/IO.DAT` | the directory that contains this path |
| `./d2exp.mpq` | the directory that directly contains this file |

`known` fingerprints (size, and SHA-1 and/or SHA-256; for directory matches a
`file` inside) make a found source *verified*, *unsupported* (wrong version)
or *unknown*. Prefer `import_args`: the game's own importer then validates and
converts the data; the launcher only finds it.

## `[steam]` — Add to Steam

```toml
[steam]
name = "The Settlers II: Coop"     # name in the Steam library (default: name)
capsule = "https://…/capsule.png"  # 600×900
wide = "https://…/wide.png"        # 920×430
hero = "https://…/hero.png"        # 1920×620
logo = "https://…/logo.png"        # transparent
icon = "https://…/icon.png"        # square
```

Missing images are generated from the game's name, accent and motif, so the
artwork set is always complete; the portrait capsule is the library tile's
art (SteamGridDB or the player's own choice) when there is one. Only use art you are allowed to distribute.

## `[tier0]` — launcher-run lobbies

For games with their own LAN/IP multiplayer that can be started into a
network game from a file and/or arguments. The launcher runs the lobby, writes
the files on every machine, starts the game and tunnels its UDP traffic.

```toml
[tier0]
transport = "udp"
game_port = 1234                 # the port the game itself uses
args = ["-SPAWN"]                # start arguments for a staged game
settings = [
  { key = "speed", label = "Game speed", options = ["0", "1", "2"], default = "1" },
]

[tier0.files]
"SPAWN.INI" = """
[Settings]
Name={{self.name}}
Color={{self.seat}}
Port={{game_port}}
Host={{self.host_yesno}}
GameSpeed={{setting.speed}}
Seed={{seed}}
{{#each others}}
[Other{{n}}]
Name={{name}}
Color={{seat}}
Ip={{ip}}
Port={{port}}
{{/each}}
"""
```

File paths are relative to the game executable's directory.

Games that restart themselves — one process per mission, the next one started
by the game with the same launch files — set `relaunch_linger = 120` (seconds,
at most 3600). The tunnel and the room then outlive the launched process: the
session ends only once the game port has sent nothing for that long (or the
player leaves). Without it the session ends when the process exits.

### Templates

| Key | Value |
|---|---|
| `{{self.name}}`, `{{self.seat}}` | this player's name and seat (0-based) |
| `{{self.host}}`, `{{self.host_yesno}}` | `true`/`false`, `yes`/`no` |
| `{{game_port}}`, `{{seed}}`, `{{players}}` | from `[tier0]`, a shared random seed, the player count |
| `{{setting.<key>}}` | the host's choice for a setting |
| `{{#each others}} … {{/each}}` | once per other player, with `{{name}}`, `{{seat}}`, `{{ip}}` (`127.0.0.1`), `{{port}}` (the local tunnel port for that player) and `{{n}}` (1-based) |

Unknown keys are errors, so a typo cannot silently produce a broken file.
