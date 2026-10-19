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
warpzone --add-to-steam             menu entry, warpzone:// links and a Steam shortcut for the launcher, then exit
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
| `<data>/cache/griddb/` | SteamGridDB grid lists (kept a week) and downloaded tile pictures |

`<data>` is `~/.local/share/warpzone` on Linux (or `$XDG_DATA_HOME/warpzone`),
`%APPDATA%\warpzone` on Windows; `WARPZONE_HOME` overrides it.

## The launcher updating itself

At start and every 30 minutes after (whatever *Update games automatically*
says) the launcher asks the server whether a newer launcher exists. **As an AppImage** it downloads the new
AppImage next to the running one, verifies it against GitHub's SHA-256 digest
and renames it over the old file; the running copy stays mounted until it
exits, so the new version starts next time. The AppImage also carries
update information (`gh-releases-zsync`), so AppImageUpdate or AppImageLauncher
can update it too. **Unpacked** (`.tar.gz`, `.zip`) it replaces its own
executable the same way. Shortcuts, autostart entries and the `warpzone://`
handler always point at the AppImage file (`$APPIMAGE`), never at its
temporary mount. A launcher running in the background (window closed)
restarts itself into the new version as soon as no game, room or ranked
queue is active; with its window open it says the update is installed and
starts it next time. `WARPZONE_NO_SELF_UPDATE=1` turns self-updates off.

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

When a game crashes — it is killed by SIGSEGV, SIGABRT, SIGBUS, SIGFPE,
SIGILL or SIGTRAP, or exits with a non-zero code (on Windows: also an
exception code) — the launcher sends a `crash` report for that game with the
last 60 lines of `last-run.log` to the server (`POST /api/v1/feedback`), with
the home directory and login name replaced. Being stopped (SIGINT, SIGTERM,
SIGKILL, SIGHUP) is not a crash. The launcher's own panics go the same way as
feedback for Warpzone. *Settings → Report crashes* turns both off.

## Feedback

The bar at the bottom of every page sends a problem or an idea: pick the game
(preset to the game started last, or *Warpzone* for the launcher and the
online parts), *Problem* or *Idea*, type, Enter. A problem about a game
carries the last 40 lines of its last run's log. The server keeps it and may
pass it on to the game's developers ([self-hosting](self-hosting.md)).

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

## Library tiles

Every game is a portrait tile (2:3, like Steam's library) with no name on
it — only icons for what it offers: players on one screen, online players,
mods, ranked ladder (dim while still coming), plus *Update*, *Playing*,
*Not installed* or *Files needed* in the corner and a progress bar while
installing. The art is the best-rated static portrait grid on
[SteamGridDB](https://www.steamgriddb.com) for the descriptor's
`steamgriddb` id, looked up at run time — the repository and the server
carry only the id — and cached in `<data>/cache/griddb/`, so it shows
offline too. Games without SteamGridDB art get the generated capsule, which
does show the name. Grids marked NSFW, humour or epilepsy and animated ones
are never offered.

*Change artwork…* (right-click a tile, Menu on a controller, or the game
page) shows the game's SteamGridDB grids, searches SteamGridDB for any other
game, takes a picture file from this computer, or goes back to the generated
art. The choice is kept in `launcher.json`; a chosen picture also becomes the
portrait capsule of *Add to Steam*. Without an account the launcher uses
SteamGridDB's public endpoints; with a personal API key (Settings → Library)
the documented v2 API.

Once games are installed, the library shows only those; *All games* (or X
on a controller, or the "+ N more to install" tile) shows the others.

## Controllers

A controller drives everything, so the launcher feels native in Steam Deck
Game Mode. A thread reads the gamepads (gilrs) and wakes the window, so
buttons work while nothing else happens on screen; while another window (a
game) has the focus, the launcher ignores them.

| Button | Does |
|---|---|
| D-pad / left stick | move to the next control in that direction (held: repeats), only within the page or the open dialog — never into the tab bar; the focused control gets a ring and scrolls into view |
| A | activate (open a game, press a button) |
| B | back (game page → library, close a dialog) |
| X | library: *Installed* / *All games* |
| Y | library: *Play*, *Install* or *Update* the focused game |
| Menu (Start) | *Change artwork* of the focused game |
| LB / RB | previous / next tab (also Ctrl+PageUp/PageDown) |
| LT | refresh the page (library, rooms, ladder, mods, moderation) |
| View (Select) | choose the player name |
| RT | quit Warpzone (asks first; quits even in background mode) |

With a controller connected (or on a Steam Deck) the tab bar shows LB/RB, a
hint bar at the bottom shows what each button does on the current page, and
the feedback box becomes a *Feedback* tab of its own.

The interface size follows the desktop's scale, but never so far that the
Steam Deck's 1280×800 layout no longer fits the screen (a Deck often reports
a scale of 1.5 or more); *Settings → Interface size* overrides it. Library
tiles are never so tall that two rows do not fit without scrolling.

## Steam Deck installer

`Install-Warpzone.desktop` (a release asset; source in
[`packaging/steamdeck`](../packaging/steamdeck)) is opened in Desktop Mode.
It downloads `install-warpzone.sh` from this repository, which asks once,
downloads the newest launcher AppImage into `~/Applications/Warpzone.AppImage`
(verified against GitHub's SHA-256 of the asset), runs
`Warpzone.AppImage --add-to-steam` — menu entry, `warpzone://` handler and a
Steam shortcut with artwork, closing Steam for that moment — and offers to go
back to Game Mode. Running it again updates both.

## Android

A light launcher of its own for Android phones and tablets
(`warpzone-launcher-<version>-android.apk` on the releases; source in
[`android/`](../android), plain Java without libraries, about 60 KB). It is
the Deck launcher made for touch: the same dark look, portrait SteamGridDB
tiles with player counts and state, a game page with Install / Update / Play
/ Host online, the game's open rooms, its game files and what's new, a Rooms
tab (public rooms of every Android game, join by code or invite link), a
Feedback tab and Settings. It shows **only games with an Android build**
(`install.assets.android` in the descriptor).

* Installing: it downloads the release's APK, checks it against the
  release's `SHA256SUMS` on GitHub and hands it to Android's package
  installer (Android asks once whether Warpzone may install apps, then
  confirms every install). Installed version and updates come from the
  package named by `launch.exec.android`.
* Joining: it starts the game with `warpzone.args` (see
  [integration](integration.md#android)). It has no account, so codes and
  invite secrets go to the game, which joins with its own identity.
* Invite links (`warpzone://join/…`, `https://<server>/join/…`) open in it.
* It updates itself: at start and every 30 minutes it asks the server for
  the launcher's newest release (`/games/launcher/updates`) and offers the
  APK (`android` in the server's `[launcher] assets`).
* Its own crashes are reported to the server's feedback API on the next
  start (game `warpzone`, kind `crash`; *Settings* turns it off).

Build: `tools/fetch-android.sh` (JDK 17, Android build-tools 35 and
platform 35 into `.toolchain/android`), then `android/build.sh` →
`dist/warpzone-launcher-<version>-android[-debugkey].apk`. Releases are
signed in CI with the release key (secrets `WZ_ANDROID_KEYSTORE_B64`,
`WZ_ANDROID_KS_PASS`). Settings accepts `http://10.0.2.2:2998` for a
`dev/server.toml` server on the emulator's host.

## Headless screenshots

```sh
Xvfb :217 -screen 0 1280x800x24 &
DISPLAY=:217 warpzone --screenshot-dir shots/
```

renders the first-start dialog, library, a game page, rooms, ladder, mods and
settings with live data from the configured server.
