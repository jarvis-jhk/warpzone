# Mods

Warpzone hosts mods for every game but knows nothing about what they do. A
game declares its categories in its [descriptor](descriptors.md#mods); a mod is
an archive plus a small generic manifest. The server stores bytes and hashes,
resolves dependencies and serves downloads; the game loads the files.

**Every version of every mod is approved by a moderator** before anyone
else can see or download it ([moderation](moderation.md)).

## The manifest

`warpzone-mod.json`, uploaded next to the archive (`wz mod-new <dir>` writes a
template). It mirrors Factorio's `info.json`:

```json
{
  "name": "cave-tiles",
  "version": "1.2.0",
  "title": "Cave tiles",
  "category": "tileset",
  "game_version": ">=0.3, <0.5",
  "dependencies": ["bat >= 3", "? music-pack", "! old-tiles"],
  "license": "CC-BY-4.0",
  "description": "Dark caves with glowing crystals.",
  "tags": ["cave", "dark"],
  "homepage": "https://example.org/cave-tiles",
  "extra": {"anything": "the game wants"}
}
```

| Field | Rules |
|---|---|
| `name` | 1–48 characters of `a-z 0-9 _ -`; unique per game and owned by its first uploader |
| `version` | semantic version (`1.2.0`); **versions are immutable** — publish a new one |
| `title` | 1–80 characters |
| `category` | one of the game's categories |
| `game_version` | semver requirement on the game's version (`*` = any) |
| `dependencies` | up to 64, see below |
| `license` | SPDX id or short name; required |
| `description` | up to 4000 characters |
| `tags` | up to 10 slugs |
| `extra` | game-specific, stored and passed on, never interpreted |

The server never opens the archive. What is inside (and any game-specific
manifest) is between the author and the game.

## Dependencies

One string per dependency, exactly like Factorio:

| Entry | Meaning |
|---|---|
| `cave-tiles >= 1.2` | required; operators `<` `<=` `=` `>=` `>`; constraint optional |
| `? music-pack` | optional: not installed automatically, but if present it must match and loads first |
| `(?) debug-overlay` | hidden optional: like `?`, not shown to players |
| `! old-tiles` | conflict: never active together (`! old-tiles < 2` conflicts only with old versions) |
| `~ shared-lib >= 1` | required, but does not affect load order |

`1.2` means `1.2.0`. Cycles between ordering dependencies are refused.

## Resolution and lockfiles

The **server** resolves: given the mods a player wants and the game version,
it picks the newest approved, non-yanked versions that satisfy every
constraint (with backtracking) and returns a **lockfile**:

```json
{
  "format": 1, "game": "super-mario-land-port", "game_version": "0.4.0",
  "mods": [{"name": "bat", "version": "3.0.0", "sha256": "…", "size": 1234, "category": "enemy"}, …],
  "order": ["bat", "cave-tiles", "level-one", "world-one", "campaign"],
  "hash": "…"
}
```

`order` is the load order (dependencies first, ties alphabetical). `hash` is
the SHA-256 of the exact set, and it is what rooms carry: everyone in a room
runs the same files. When resolution fails the answer explains the chain:
`requested → campaign 1.0.0 → world-one 1.0.0 needs cave-tiles >= 2.0.0 (available: 1.0.0)`.

**Clients** only validate offline (`validate_set`): requirements present,
optional dependencies matching, no conflicts, supported game version, no
cycle. Resolving needs the catalog anyway, so it happens online.

A **yanked** version is no longer chosen for new installs, but stays
downloadable so existing lockfiles and replays keep working; an exact pin
(`= 1.1.0`) can still select it.

## Publishing

```sh
wz register MyName                      # once
wz mod-new mymod/                       # writes mymod/warpzone-mod.json
$EDITOR mymod/warpzone-mod.json
(cd mymod/content && zip -r ../mymod.zip .)
wz mod-upload super-mario-land-port mymod/warpzone-mod.json mymod/mymod.zip
```

The version is *pending* until a moderator approves it; you get a
`mod_decision` event (shown by the launcher and the CLI) with the decision and
the reason. You can install your pending version yourself to test it:
`wz mod <game> <name>` shows it.

Only upload your own or properly licensed content. Archives that contain
material from the original games (ROM data, extracted sprites, music) are
rejected, and so are executables for games whose mods are data only.

## In the game and the launcher

* **Launcher** — Mods tab: browse, add and remove; it keeps one active set
  per game (`<data>/mods/<game>/active.lock.json`) and installs dependencies.
* **Joining a modded room** — the library or launcher syncs exactly the
  room's lockfile before connecting (`wz_sync_mods`), then the game loads the
  returned paths in order.
* **Storage** — `<data>/mods/<game>/<name>/<version>/`: the archive, or an
  unpacked `files/` directory for games with `install = "extract"` (with
  checks against path traversal, symlinks, duplicate names and archive bombs),
  plus `warpzone-installed.json`. A sync downloads into staging and only then
  switches the active set, so an interrupted sync leaves the previous set
  usable.
