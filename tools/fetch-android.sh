#!/bin/bash
# Fetches what packaging/android/build.sh needs into .toolchain/android/
# (nothing outside the project): a JDK 17 and from the Android SDK only
# build-tools 35 (aapt2, d8, zipalign, apksigner) and the android-35 platform
# (android.jar). No Gradle, no NDK, no sdkmanager. CI runners that already
# have $ANDROID_HOME and a JDK do not need this.
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
A=$ROOT/.toolchain/android
mkdir -p "$A/downloads"

get() { # url file hash-tool hash
    local z=$A/downloads/$2
    if [ -f "$z" ] && echo "$4  $z" | $3 -c --status; then return; fi
    echo "downloading $2"
    curl -sSfL --retry 3 -o "$z.part" "$1"
    echo "$4  $z.part" | $3 -c --status || { echo "checksum mismatch: $2" >&2; rm -f "$z.part"; exit 1; }
    mv "$z.part" "$z"
}

JDK=OpenJDK17U-jdk_x64_linux_hotspot_17.0.20.1_1.tar.gz
get "https://github.com/adoptium/temurin17-binaries/releases/download/jdk-17.0.20.1%2B1/$JDK" "$JDK" sha256sum \
    3808d1d15e3ec6bd5b84057fb5d84c33d8a1536a258146bcea2e603fc726e08e
get https://dl.google.com/android/repository/build-tools_r35_linux.zip build-tools_r35_linux.zip sha1sum \
    2cfaa0bbb2336e9ec18ed3ecea84fa2e2af607bc
get https://dl.google.com/android/repository/platform-35_r02.zip platform-35_r02.zip sha1sum \
    0bb560a90a7a2cbd0dd8348224d518b638fe7949

if [ ! -x "$A/jdk/bin/javac" ]; then
    rm -rf "$A/jdk" && mkdir -p "$A/jdk"
    tar -xzf "$A/downloads/$JDK" -C "$A/jdk" --strip-components=1
fi
if [ ! -x "$A/sdk/build-tools/35.0.0/aapt2" ]; then
    rm -rf "$A/sdk/build-tools" && mkdir -p "$A/sdk/build-tools"
    python3 -I -m zipfile -e "$A/downloads/build-tools_r35_linux.zip" "$A/sdk/build-tools"
    chmod -R +x "$A/sdk/build-tools"   # zipfile drops the mode bits
    mv "$A/sdk/build-tools/android-15" "$A/sdk/build-tools/35.0.0"
fi
if [ ! -f "$A/sdk/platforms/android-35/android.jar" ]; then
    rm -rf "$A/sdk/platforms" && mkdir -p "$A/sdk/platforms"
    python3 -I -m zipfile -e "$A/downloads/platform-35_r02.zip" "$A/sdk/platforms"
    mv "$A/sdk/platforms/android-35"* "$A/sdk/platforms/android-35" 2>/dev/null || true
fi
echo "Android toolchain ready: JAVA_HOME=$A/jdk ANDROID_HOME=$A/sdk"
