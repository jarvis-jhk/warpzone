# Source this file to use the project's own pinned Rust toolchain:
#     . tools/env.sh
# Nothing is installed system-wide; everything lives in .toolchain/ (gitignored).
WZ_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]:-$0}")/.." && pwd)
export RUSTUP_HOME="$WZ_ROOT/.toolchain/rustup"
export CARGO_HOME="$WZ_ROOT/.toolchain/cargo"
export PATH="$CARGO_HOME/bin:$WZ_ROOT/.toolchain/zig:$PATH"
# Temporary files of builds and tests stay inside the project too.
export TMPDIR="$WZ_ROOT/.toolchain/tmp"
mkdir -p "$TMPDIR"
# Link shims for system libraries whose -dev packages may be missing.
[ -d "$WZ_ROOT/.toolchain/sysshim/lib/pkgconfig" ] && export PKG_CONFIG_PATH="$WZ_ROOT/.toolchain/sysshim/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
