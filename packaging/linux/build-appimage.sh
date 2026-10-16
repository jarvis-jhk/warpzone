#!/bin/bash
# Build the launcher AppImage from a built launcher binary.
#
#   packaging/linux/build-appimage.sh <warpzone binary> <version> <out dir> [dir with legal files]
#
# appimagetool (static runtime, no libfuse2 needed on the player's machine) is
# downloaded into .toolchain/ on first use. The update information lets
# AppImageUpdate/AppImageLauncher update it too; the launcher updates itself
# without them.
set -euo pipefail
bin=$1 version=$2 out=$3 legal=${4:-}
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tool="$root/.toolchain/appimagetool-x86_64.AppImage"
if [ ! -x "$tool" ]; then
  mkdir -p "$root/.toolchain"
  curl -sSfL -o "$tool" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod +x "$tool"
fi
dir=$(mktemp -d -p "${TMPDIR:-/tmp}")
trap 'rm -rf "$dir"' EXIT
app="$dir/Warpzone.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/share/applications" "$app/usr/share/icons/hicolor/256x256/apps"
install -m 755 "$bin" "$app/usr/bin/warpzone"
"$bin" --render-icon "$app/warpzone.png"
cp "$app/warpzone.png" "$app/usr/share/icons/hicolor/256x256/apps/warpzone.png"
cp "$app/warpzone.png" "$app/.DirIcon"
cp "$root/packaging/linux/warpzone.desktop" "$app/warpzone.desktop"
cp "$root/packaging/linux/warpzone.desktop" "$app/usr/share/applications/warpzone.desktop"
ln -s usr/bin/warpzone "$app/AppRun"
if [ -n "$legal" ]; then
  mkdir -p "$app/usr/share/doc/warpzone"
  find "$legal" -maxdepth 1 -type f \( -name 'LICENSE*' -o -name 'THIRD-PARTY*' \) -exec cp {} "$app/usr/share/doc/warpzone/" \;
fi
mkdir -p "$out"
name="warpzone-launcher-$version-x86_64.AppImage"
(cd "$out" && ARCH=x86_64 VERSION="$version" "$tool" --appimage-extract-and-run --no-appstream \
  -u "gh-releases-zsync|jarvis-jhk|warpzone|latest-all|warpzone-launcher-*-x86_64.AppImage.zsync" \
  "$app" "$name" >/dev/null)
echo "$out/$name"
