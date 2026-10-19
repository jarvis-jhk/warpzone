#!/bin/bash
# Warpzone for the Steam Deck: installs the launcher (a self-updating
# AppImage) and adds it to Steam with its artwork, so it starts from Game
# Mode. Run by "Install Warpzone.desktop" (open it in Desktop Mode); safe to
# run again — it updates the AppImage and the Steam shortcut.
#
# Inspired by ArnoldSmith86's minecraft-splitscreen installer.
set -u

REPO=jarvis-jhk/warpzone
DIR=$HOME/Applications
APP=$DIR/Warpzone.AppImage

fail() {
    echo -e "\n\n❌ $1\n"
    zenity --error --width=420 --text="$1" 2>/dev/null
    # Steam may have been closed for the shortcut: bring it back.
    pgrep -x steam >/dev/null || (nohup steam >/dev/null 2>&1 &)
    exit 1
}

zenity --question --width=420 --title="Warpzone" \
    --text='This installs the Warpzone launcher in ~/Applications and adds it to Steam with artwork, so you can start it from Game Mode.\n\nSteam closes for a moment while the shortcut is added.\n\nInstall Warpzone?' 2>/dev/null || exit 1

echo "🔎 Looking for the newest Warpzone release"
# Newest release with an AppImage (Warpzone 0.x releases are prereleases, so
# not /releases/latest), and GitHub's own SHA-256 of it.
info=$(curl -fsSL "https://api.github.com/repos/$REPO/releases?per_page=10" | python3 -c '
import json, sys
for r in json.load(sys.stdin):
    if r.get("draft"):
        continue
    for a in r.get("assets", []):
        if a["name"].endswith("-x86_64.AppImage") and (a.get("digest") or "").startswith("sha256:"):
            print(r["tag_name"], a["digest"][7:], a["browser_download_url"])
            sys.exit(0)
sys.exit(1)
') || fail "Could not find a Warpzone release on GitHub. Check the internet connection and try again."
read -r tag sha url <<<"$info"

mkdir -p "$DIR" || fail "Cannot create $DIR."
if [ -f "$APP" ] && echo "$sha  $APP" | sha256sum -c --status; then
    echo "✅ Warpzone $tag is already installed"
else
    echo "📦 Downloading Warpzone $tag"
    curl -fL --progress-bar "$url" -o "$APP.part" || fail "The download failed. Please try again."
    echo "$sha  $APP.part" | sha256sum -c --status || { rm -f "$APP.part"; fail "The download was damaged (checksum mismatch). Please try again."; }
    chmod +x "$APP.part" && mv -f "$APP.part" "$APP" || fail "Cannot write $APP."
    echo "✅ Warpzone $tag installed in $APP"
fi

echo "⏳ Adding Warpzone to Steam (Steam closes for a moment)"
# The launcher writes the shortcut and artwork itself (and a menu entry and
# the warpzone:// link handler). AppImages need FUSE; without it, unpack and run.
"$APP" --add-to-steam || "$APP" --appimage-extract-and-run --add-to-steam || fail "Adding Warpzone to Steam failed."

if zenity --question --width=420 --title="Warpzone" --ok-label="Game Mode" --cancel-label="Stay here" \
    --text='Warpzone is installed and in your Steam library.\n\nGo to Game Mode now?' 2>/dev/null; then
    qdbus org.kde.Shutdown /Shutdown org.kde.Shutdown.logout 2>/dev/null || steam steam://open/bigpicture >/dev/null 2>&1 &
elif ! pgrep -x steam >/dev/null; then
    nohup steam >/dev/null 2>&1 &
fi

# END OF FILE
