#!/bin/bash
# Install the pinned Rust toolchain into .toolchain/ (project-local, recreatable).
# The version comes from rust-toolchain.toml; rustup reads it on first use.
#
#     bash tools/fetch-toolchain.sh            # host toolchain only
#     bash tools/fetch-toolchain.sh --cross    # plus zig + cargo-zigbuild for
#                                              # aarch64/glibc-2.27 and windows-gnu
set -euo pipefail
. "$(dirname "$0")/env.sh"
mkdir -p "$WZ_ROOT/.toolchain"
if [ ! -x "$CARGO_HOME/bin/rustup" ]; then
  curl -sSf https://sh.rustup.rs -o "$WZ_ROOT/.toolchain/rustup-init.sh"
  sh "$WZ_ROOT/.toolchain/rustup-init.sh" -y --no-modify-path --profile minimal --default-toolchain none
fi
cd "$WZ_ROOT" && rustup show active-toolchain >/dev/null 2>&1 || rustup toolchain install
rustup show active-toolchain
if [ "${1:-}" = "--cross" ]; then
  ZIG_VERSION=0.13.0
  if [ ! -x "$WZ_ROOT/.toolchain/zig/zig" ]; then
    curl -sSfL "https://ziglang.org/download/$ZIG_VERSION/zig-linux-x86_64-$ZIG_VERSION.tar.xz" \
      | tar -xJ -C "$WZ_ROOT/.toolchain"
    mv "$WZ_ROOT/.toolchain/zig-linux-x86_64-$ZIG_VERSION" "$WZ_ROOT/.toolchain/zig"
  fi
  command -v cargo-zigbuild >/dev/null || cargo install --locked cargo-zigbuild
  rustup target add aarch64-unknown-linux-gnu x86_64-pc-windows-gnu x86_64-unknown-linux-musl
fi
