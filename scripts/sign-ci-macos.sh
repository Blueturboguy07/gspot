#!/usr/bin/env bash
# Sign, notarize and package the unsigned GSpot.app that the publik-build workflow makes
# (artifact GSpot-macos-arm64-unsigned). Nothing opens a window: codesign, notarytool,
# stapler and hdiutil only, so it is safe on a Mac someone is using.
#
# Usage: scripts/sign-ci-macos.sh <GSpot-unsigned-aarch64.app.zip> <out dir>
# Env:   NOTARY_PROFILE (notarytool keychain profile, default "publik"),
#        APPLE_SIGNING_IDENTITY (default: the Developer ID below).
# Out:   <out dir>/GSpot_<version>_aarch64.dmg (signed, notarized, stapled; the app inside too).
set -euo pipefail
ZIP="${1:?unsigned app zip}"
OUT="${2:?out dir}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IDENTITY="${APPLE_SIGNING_IDENTITY:-Developer ID Application: Mann Bellani (R5R3ZS54LV)}"
PROFILE="${NOTARY_PROFILE:-publik}"
case "$IDENTITY" in "Developer ID Application:"*) ;; *) echo "Refusing: '$IDENTITY' is not a Developer ID Application identity." >&2; exit 1;; esac
mkdir -p "$OUT"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

notarize() {
  local file="$1" log="$WORK/notary.log"
  xcrun notarytool submit "$file" --keychain-profile "$PROFILE" --wait 2>&1 | tee "$log"
  grep -q "status: Accepted" "$log" || { echo "Notarization of $(basename "$file") was not accepted." >&2; exit 1; }
}

ditto -x -k "$ZIP" "$WORK/x"
APP="$WORK/x/GSpot.app"
[ -d "$APP" ] || { echo "No GSpot.app in $ZIP" >&2; exit 1; }
VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist")"
echo "==> GSpot $VERSION: signing with $IDENTITY"
# Same as the Tauri bundler: one executable, hardened runtime, the (empty) entitlements, a timestamp.
codesign --force --options runtime --timestamp --entitlements "$ROOT/src-tauri/Entitlements.plist" --sign "$IDENTITY" "$APP"
codesign --verify --strict --deep --verbose=2 "$APP"

echo "==> Notarizing the app"
ditto -c -k --keepParent "$APP" "$WORK/GSpot.zip"
notarize "$WORK/GSpot.zip"
xcrun stapler staple "$APP"

echo "==> Building the dmg"
STAGE="$WORK/stage"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/GSpot.app"
ln -s /Applications "$STAGE/Applications"
DMG="$OUT/GSpot_${VERSION}_aarch64.dmg"
rm -f "$DMG"
hdiutil create -volname "GSpot" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
codesign --force --sign "$IDENTITY" --timestamp "$DMG"
echo "==> Notarizing the dmg"
notarize "$DMG"
xcrun stapler staple "$DMG"

echo "==> Checks (no launch)"
xcrun stapler validate "$DMG"
spctl -a -vv -t open --context context:primary-signature "$DMG"
MNT="$WORK/mnt"
mkdir -p "$MNT"
hdiutil attach -nobrowse -noautoopen -readonly -mountpoint "$MNT" "$DMG" >/dev/null
xcrun stapler validate "$MNT/GSpot.app"
spctl -a -vv -t exec "$MNT/GSpot.app"
codesign -dvv "$MNT/GSpot.app" 2>&1 | grep -E "Authority=Developer ID Application|flags=|TeamIdentifier"
hdiutil detach "$MNT" -quiet
shasum -a 256 "$DMG"
echo "Dmg: $DMG"
