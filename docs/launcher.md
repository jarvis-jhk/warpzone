# Launcher internals

What the launcher does on a player's machine, for the curious and for
packagers. Source: [`crates/launcher`](../crates/launcher).

## Command line

```text
warpzone                            open the launcher
warpzone warpzone://join/<room>#…   open an invite link
warpzone --play <game>              update if needed, start the game, exit (Steam shortcuts use this)
warpzone --background               start hidden (the autostart entry of background mode)
warpzone --screenshot-dir <dir>     render every screen to PNG and exit (docs, tests)
warpzone --render-art <dir>         write every game's generated artwork and exit
```

A second start hands its arguments to the running launcher (a loopback TCP
port recorded in `<data>/launcher.port`) and exits, so invite links and
`--play` work while it runs.

## Files

| Path | What |
|---|---|
| `<data>/identity.key` | device key, shared with games and `wz` (mode 0600) |
| `<data>/launcher.json` | settings: background mode, auto-update, hidden games, extra data folders, seen versions, server |
| `<data>/games/<game>/<version>/` | installed games; `current` and `previous` name the active and rollback versions |
| `<data>/games/<game>/<version>/.warpzone-managed` | marks a launcher install: the game's own updater stays silent |
| `<data>/mods/<game>/` | installed mods and `active.lock.json` |

`<data>` is `~/.local/share/warpzone` on Linux (or `$XDG_DATA_HOME/warpzone`),
`%APPDATA%\warpzone` on Windows; `WARPZONE_HOME` overrides it.

## The launcher updating itself

At start (with *Update games automatically* on) the launcher asks the server
whether a newer launcher exists. **As an AppImage** it downloads the new
AppImage next to the running one, verifies it against GitHub's SHA-256 digest
and renames it over the old file; the running copy stays mounted until it
exits, so the new version starts next time. The AppImage also carries
update information (`gh-releases-zsync`), so AppImageUpdate or AppImageLauncher
can update it too. **Unpacked** (`.tar.gz`, `.zip`) it replaces its own
executable the same way. Shortcuts, autostart entries and the `warpzone://`
handler always point at the AppImage file (`$APPIMAGE`), never at its
temporary mount. `WARPZONE_NO_SELF_UPDATE=1` turns self-updates off.

## Installing and updating

One click: the launcher downloads the release asset for this platform and
verifies it against **GitHub's own SHA-256 digest** of the asset (or the
release's checksum file) — fetched from GitHub, not from the Warpzone server.
Descriptors with a `minisign_key` also need a valid signature. `.tar.gz`
releases are downloaded, hashed and unpacked in one stream into a staging
directory; nothing replaces the installed version unless the hash matches.
The previous version stays for rollback; older ones are removed.

With *Update games automatically* on, updates install in the background
(when the launcher runs) and before every start.

## Starting a game

Play starts the game with its output in `<data>/games/<game>/last-run.log`,
shows *Running*, and minimizes the launcher so the game (and its first-start
dialogs) is in front; the launcher comes back when the game ends. If it ends
with an error within 30 seconds, the game page shows why, with the last lines
of that log. Found game files go to the game as `WARPZONE_DATA_<ID>`
environment variables ([integration](integration.md#game-files-the-launcher-found)).

## Game files

For games with `[[data]]` requirements the launcher searches Steam libraries
(native, Flatpak, Snap, every `libraryfolders.vdf` entry, Proton prefixes),
GOG Galaxy / `~/GOG Games`, Heroic (`~/Games/Heroic`, its `installed.json`),
Lutris (`~/Games`), Wine prefixes, the Downloads folder (XDG `user-dirs`
aware), the Desktop and mounted media (`/media`, `/run/media`, other Windows
drives), with bounded depth. It checks matches against the descriptor's
fingerprints before anything is downloaded, then copies the data or passes it
to the game's importer.

## Background mode

Off by default until the player chooses on first start. On: an autostart
entry (`~/.config/autostart/warpzone.desktop`, or the `Run` registry key on
Windows) starts `warpzone --background`; closing the window hides it; it
checks for updates every 30 minutes and posts desktop notifications
(`notify-send` on Linux). Turning the setting off removes the autostart entry.

## Steam

*Add to Steam* writes a non-Steam shortcut into
`userdata/<user>/config/shortcuts.vdf` of every Steam account on the machine
(a backup of the previous file is kept as `shortcuts.vdf.warpzone-backup`) and
the artwork into `config/grid/` — portrait capsule `<id>p.png`, wide
`<id>.png`, `<id>_hero.png`, `<id>_logo.png`, `<id>_icon.png` — using the
descriptor's images or generated ones. An existing shortcut with the same name
is updated, not duplicated. Steam overwrites the file when it exits, so the
launcher asks Steam to shut down first.

## Invite links

Settings → *Open warpzone:// links with Warpzone* registers the scheme:
`~/.local/share/applications/warpzone.desktop` with
`MimeType=x-scheme-handler/warpzone` and `xdg-mime default`, or
`HKCU\Software\Classes\warpzone` on Windows.

## Controllers

Gamepads (via gilrs) drive egui's keyboard navigation: D-pad/left stick =
Tab/Shift+Tab, A = Enter, B = Escape, Y = Space, LB/RB = switch tabs.

## Headless screenshots

```sh
Xvfb :217 -screen 0 1280x800x24 &
DISPLAY=:217 warpzone --screenshot-dir shots/
```

renders the first-start dialog, library, a game page, rooms, ladder, mods and
settings with live data from the configured server.
