# Development

## Layout

```text
crates/protocol   wire types, descriptors, tickets, UDP formats, mod resolver (pure logic)
crates/server     warpzone-server (axum + SQLite + UDP), website; tests/e2e.rs
crates/core       client: identity, API, NAT traversal, peer channels, mods, installer, game data, tunnel
crates/capi       libwarpzone (C ABI) and include/warpzone.h
crates/cli        wz
crates/launcher   warpzone (eframe/egui), generated artwork, Steam integration
games/            game descriptors
site/             website CSS and logo (compiled into the server)
assets/fonts/     Russo One (OFL) for titles and artwork
deploy/           compose, Dockerfiles, nginx, config examples
examples/c/       a complete C client
docs/             this documentation
tools/            toolchain scripts
```

## Toolchain

Everything needed to build lives in the repository directory; nothing is
installed system-wide:

```sh
bash tools/fetch-toolchain.sh            # Rust (pinned in rust-toolchain.toml) into .toolchain/
bash tools/fetch-toolchain.sh --cross    # + zig and cargo-zigbuild (musl, aarch64 glibc 2.27, mingw)
bash tools/fetch-xvfb.sh                 # a private Xvfb for headless launcher screenshots
. tools/env.sh                           # use them in this shell (also sets TMPDIR inside the tree)
```

`.toolchain/`, `target/` and `build/` are recreatable and ignored. On Linux
the launcher's controller support links `libudev`; when its `-dev` package is
missing, `fetch-toolchain.sh` adds a project-local pkg-config shim for the
system library.

## Build and test

```sh
cargo build --workspace
cargo test --workspace                       # unit tests + crates/server/tests/e2e.rs
cargo run -p warpzone-server -- check-games games
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
make -C examples/c                           # C example against libwarpzone.a
```

The end-to-end tests start the real server binary with a temporary config and
drive it with real clients over loopback: accounts and devices, hosting,
codes, knocking, direct and forced-relay encrypted traffic, ordered reliable
delivery, a rated ranked match, leaderboards, and mod upload → moderation →
resolve → sync.

## Running locally

```sh
cargo run -p warpzone-server -- --config dev/server.toml       # http://127.0.0.1:2998
export WARPZONE_SERVER=http://127.0.0.1:2998 WARPZONE_HOME=$PWD/build/home-a
cargo run -p warpzone-cli -- register DevA
cargo run -p warpzone-launcher
```

Launcher screenshots without a desktop: start your own `Xvfb` on a free
display and run `warpzone --screenshot-dir build/shots`.

## Static binaries

```sh
cargo zigbuild --release --target x86_64-unknown-linux-musl -p warpzone-server
cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.27 -p warpzone-capi
```

## Releasing

1. Update `CHANGELOG.md` (`## <version>` — the release notes come from it)
   and the workspace version in `Cargo.toml`.
2. Tag `v<version>` and push. `.github/workflows/release.yml` builds the server
   (musl x86_64/aarch64), `libwarpzone` (Linux x86_64, aarch64 glibc 2.27,
   Windows MSVC and MinGW), `wz` and the launcher (Linux, Windows), publishes
   them with `SHA256SUMS`, and pushes a container image to ghcr.io.
3. Deploy: set `WARPZONE_VERSION` and `WARPZONE_SHA256` in the instance's
   `.env` or `docker-compose.override.yml` (the public instance uses the
   override file), `docker compose up -d --build`.

## Conventions

* The protocol crate is the contract; change API types there first, then the
  server, then `docs/api.md`.
* No chat, no free-text fields players can fill in: anything new that shows
  player-written text to other players needs moderation designed first.
* Keep the game library small and poll-driven; nothing may call into game code.
