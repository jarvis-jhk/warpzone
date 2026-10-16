#!/bin/bash
# Put an Xvfb binary into .toolchain/xvfb/ for headless launcher tests and
# screenshots, without installing anything system-wide. Uses the distribution's
# own package (apt-get download needs no root); its libraries are the ones every
# desktop install already has. CI installs xvfb with apt instead.
set -euo pipefail
. "$(dirname "$0")/env.sh"
dest="$WZ_ROOT/.toolchain/xvfb"
[ -x "$dest/usr/bin/Xvfb" ] && { echo "Xvfb already in $dest"; exit 0; }
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
(cd "$tmp" && apt-get download xvfb >/dev/null)
mkdir -p "$dest" && dpkg -x "$tmp"/xvfb_*.deb "$dest"
"$dest/usr/bin/Xvfb" -help 2>&1 | head -1 || true
echo "Xvfb in $dest/usr/bin/Xvfb"
