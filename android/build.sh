#!/bin/bash
# Builds dist/warpzone-launcher-<version>-android.apk: the Android launcher
# (android/), plain Java with no libraries, no Gradle — aapt2, javac, d8,
# zipalign and apksigner. Toolchain: tools/fetch-android.sh (into
# .toolchain/android), or $JAVA_HOME and $ANDROID_HOME when they are set.
#
# Signing (Android only updates an app with the same key):
#   WZ_ANDROID_KEYSTORE=<file.jks> WZ_ANDROID_KS_PASS=<password>   the release key
#     (alias "warpzone"; CI writes it from secrets; a local copy lives in
#     .toolchain/android/release/, never in git).
#   otherwise: a throwaway debug key, and the file is named ...-debugkey.apk.
# The version is the workspace's (Cargo.toml); versionCode = major*1000000 +
# minor*1000 + patch, so it grows with every release.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
T=$ROOT/.toolchain/android
JAVA_HOME=${JAVA_HOME:-$T/jdk}
[ -x "$JAVA_HOME/bin/javac" ] || JAVA_HOME=$T/jdk
SDK=${ANDROID_HOME:-$T/sdk}
[ -f "$SDK/platforms/android-35/android.jar" ] || SDK=$T/sdk
BT=$SDK/build-tools/35.0.0
JAR=$SDK/platforms/android-35/android.jar
[ -x "$BT/aapt2" ] && [ -f "$JAR" ] || { echo "no Android toolchain: run tools/fetch-android.sh" >&2; exit 1; }
export JAVA_HOME PATH="$JAVA_HOME/bin:$PATH"

V=${WZ_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}
IFS=. read -r MA MI PA <<<"${V%%-*}"
CODE=$((MA * 1000000 + MI * 1000 + PA))
if [ -n "${WZ_ANDROID_KEYSTORE:-}" ]; then
    KS=$WZ_ANDROID_KEYSTORE SUFFIX=
    : "${WZ_ANDROID_KS_PASS:?WZ_ANDROID_KS_PASS is needed with WZ_ANDROID_KEYSTORE}"
else
    KS=$T/debug.jks SUFFIX=-debugkey
    export WZ_ANDROID_KS_PASS=android
    [ -f "$KS" ] || keytool -genkeypair -keystore "$KS" -storepass android -keypass android -alias warpzone \
        -keyalg EC -groupname secp256r1 -validity 10000 -dname "CN=Warpzone debug" >/dev/null 2>&1
    echo "note: signing with the debug key" >&2
fi

B=$ROOT/build/android
rm -rf "$B" && mkdir -p "$B/classes" "$B/dex" "$ROOT/dist"
"$BT/aapt2" compile --dir android/res -o "$B/res.zip"
"$BT/aapt2" link -o "$B/unsigned.apk" -I "$JAR" --manifest android/AndroidManifest.xml \
    --min-sdk-version 26 --target-sdk-version 34 --version-code "$CODE" --version-name "$V" "$B/res.zip"
find android/src -name '*.java' > "$B/sources.txt"
javac -source 8 -target 8 -bootclasspath "$JAR:$BT/core-lambda-stubs.jar" -Xlint:-options -encoding UTF-8 -d "$B/classes" @"$B/sources.txt"
"$BT/d8" --release --min-api 26 --lib "$JAR" --output "$B/dex" $(find "$B/classes" -name '*.class')
cp LICENSE-MIT LICENSE-APACHE "$B/dex/"
(cd "$B/dex" && jar -u -M -f "$B/unsigned.apk" classes.dex LICENSE-MIT LICENSE-APACHE)
"$BT/zipalign" -p -f 4 "$B/unsigned.apk" "$B/aligned.apk"
OUT=$ROOT/dist/warpzone-launcher-$V-android$SUFFIX.apk
"$BT/apksigner" sign --ks "$KS" --ks-pass env:WZ_ANDROID_KS_PASS --ks-key-alias warpzone --out "$OUT" "$B/aligned.apk"
"$BT/apksigner" verify "$OUT"
echo "$OUT ($(du -k "$OUT" | cut -f1) KB, versionName $V, versionCode $CODE)"
