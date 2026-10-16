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

# The launcher's controller support (gilrs) links against libudev. Desktop
# Linux always has the runtime library but often not the -dev package; a
# project-local pkg-config shim pointing at the runtime library is enough.
if [ "$(uname)" = Linux ] && ! pkg-config --exists libudev 2>/dev/null; then
  lib=$(ls /usr/lib/x86_64-linux-gnu/libudev.so.1 /usr/lib64/libudev.so.1 /usr/lib/libudev.so.1 2>/dev/null | head -1)
  if [ -n "$lib" ]; then
    mkdir -p "$WZ_ROOT/.toolchain/sysshim/lib/pkgconfig"
    ln -sf "$lib" "$WZ_ROOT/.toolchain/sysshim/lib/libudev.so"
    printf 'prefix=%s\nlibdir=${prefix}/lib\nName: libudev\nDescription: shim\nVersion: 255\nLibs: -L${libdir} -ludev\nCflags:\n' \
      "$WZ_ROOT/.toolchain/sysshim" > "$WZ_ROOT/.toolchain/sysshim/lib/pkgconfig/libudev.pc"
  fi
fi
