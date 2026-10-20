#!/bin/bash
# Wrap the universal launcher binary into Warpzone.app (macOS only: needs
# sips and iconutil). Unsigned: the first start needs right-click > Open.
#   packaging/macos/build-app.sh <warpzone binary> <version> <output dir>
set -euo pipefail
bin=$1 ver=$2 out=$3
app="$out/Warpzone.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/warpzone"
chmod 755 "$app/Contents/MacOS/warpzone"
tmp=$(mktemp -d)
"$bin" --render-icon "$tmp/icon.png"
mkdir "$tmp/warpzone.iconset"
for s in 16 32 128 256; do
  sips -z $s $s "$tmp/icon.png" --out "$tmp/warpzone.iconset/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2)); [ $d -le 256 ] && sips -z $d $d "$tmp/icon.png" --out "$tmp/warpzone.iconset/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns -o "$app/Contents/Resources/warpzone.icns" "$tmp/warpzone.iconset"
rm -rf "$tmp"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Warpzone</string>
  <key>CFBundleDisplayName</key><string>Warpzone</string>
  <key>CFBundleIdentifier</key><string>io.virtualtabletop.warpzone</string>
  <key>CFBundleVersion</key><string>$ver</string>
  <key>CFBundleShortVersionString</key><string>$ver</string>
  <key>CFBundleExecutable</key><string>warpzone</string>
  <key>CFBundleIconFile</key><string>warpzone</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
# Ad-hoc signature over the whole bundle (Apple Silicon refuses unsigned code).
codesign --force --deep --sign - "$app"
