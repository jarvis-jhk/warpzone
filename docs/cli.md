# `wz` — command line

`wz` uses the same identity and data directory as the launcher and the games.
`--server URL` (or `WARPZONE_SERVER`) selects an instance; `WARPZONE_HOME`
selects another data directory (a second identity, for testing).
`wz help` lists everything.

## Account

```sh
wz whoami                 # key, account, roles, data directory
wz register Alice         # claim a username for this device
wz link-code              # on device 1: prints an 8-character code (10 minutes)
wz link K7QM3XAB          # on device 2: joins that account
wz recovery-code          # keep it secret — it IS your identity
wz restore XXXX-XXXX-…    # replace this device's key with a recovered one
wz delete-account
```

## Games

```sh
wz games
wz rooms funny-furries
wz install gtetrinet-ng        # verified against GitHub, streamed, one step
wz installed
wz launch gtetrinet-ng
wz updates gtetrinet-ng 0.1.6  # is there something newer, what changed
wz changelog gtetrinet-ng
wz find-data s25coop           # look for the original game files it needs
wz fetch-data spacehog         # download freeware data the descriptor allows (pinned checksum)
wz rollback gtetrinet-ng
wz uninstall gtetrinet-ng
```

## Ladder

```sh
wz standings funny-furries 1v1 [2026-Q3]
wz board super-mario-land-port any [ruleset] [season]
```

## Mods

```sh
wz mods super-mario-land-port [search]
wz mod super-mario-land-port cave-tiles
wz mod-new mymod/                                   # manifest template
wz mod-upload super-mario-land-port mymod/warpzone-mod.json mymod.zip
wz resolve super-mario-land-port 0.4.0 campaign "cave-tiles >= 2"
wz mod-sync super-mario-land-port <lockfile-hash>
wz mod-installed super-mario-land-port
```

## Moderation (staff keys)

```sh
wz queue
wz approve <game> <name> <version>
wz reject  <game> <name> <version> copyright
wz hide | unhide <game> <name> <version>
wz note <game> <name> <version> looks fine, 3 levels, original art
wz close-report 12
wz feedback [game]          # newest crash, problem and idea reports
wz rename-user BadName Player123
```

## Feedback

```sh
wz report spacehog crash "ended with signal 11 (SIGSEGV)" --version 0.2.0 --detail crash.txt
some-tool 2>&1 | tail -50 | wz report warpzone bug "relay test fails" --detail -
wz report warpzone idea "show ping in the room list"
```

No account needed. Games without libwarpzone can report their own crashes
this way (or with a plain HTTP POST, see [api](api.md)).

## Network diagnostics

```sh
wz netcheck                       # the addresses the server sees for this machine
wz host funny-furries             # host an unlisted test room, print its code, echo peers
wz join funny-furries K7QM3X      # join it from elsewhere; prints RTT and direct/relayed
wz join funny-furries K7QM3X --relay   # force the relay path
```

`host` and `join` need a username on each side. Run the host at home and the
joiner on a phone hotspot to see hole punching (or the relay fallback) on a
real connection.
