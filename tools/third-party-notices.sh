#!/bin/bash
# Write THIRD-PARTY-NOTICES.html for one crate (its normal dependencies, all
# release targets) — the notices for code statically linked into a release.
#     tools/third-party-notices.sh warpzone-capi out.html
# Needs cargo-about (cargo install --locked cargo-about --features cli).
set -euo pipefail
cd "$(dirname "$0")/.."
cargo about generate --fail --locked -c about.toml -m "crates/${1#warpzone-}/Cargo.toml" -o "$2" about.hbs
