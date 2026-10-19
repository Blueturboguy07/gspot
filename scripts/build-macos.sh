#!/usr/bin/env bash
# Build GSpot.app signed with the Developer ID identity (hardened runtime).
# Notarization runs when NOTARY_PROFILE or APPLE_ID/APPLE_PASSWORD/APPLE_TEAM_ID are set.
# Usage: scripts/build-macos.sh [--skip-notarize]
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"
SKIP_NOTARIZE=0
[ "${1:-}" = "--skip-notarize" ] && SKIP_NOTARIZE=1
DEFAULT_IDENTITY="Developer ID Application: Mann Bellani (R5R3ZS54LV)"
IDENTITY="${APPLE_SIGNING_IDENTITY:-$DEFAULT_IDENTITY}"
case "$IDENTITY" in "Developer ID Application:"*) ;; *) echo "Refusing: '$IDENTITY' is not a Developer ID Application identity." >&2; exit 1;; esac
security find-identity -v -p codesigning | grep -qF "$IDENTITY" || { echo "identity not in keychain: $IDENTITY" >&2; exit 1; }
TAURI="$ROOT/ui/node_modules/.bin/tauri"
[ -x "$TAURI" ] || { echo "run pnpm install in ui/ first" >&2; exit 1; }
NOTARIZE=0; NOTARY_ARGS=()
if [ "$SKIP_NOTARIZE" = 0 ]; then
  if [ -n "${NOTARY_PROFILE:-}" ]; then NOTARIZE=1; NOTARY_ARGS=(--keychain-profile "$NOTARY_PROFILE")
  elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then NOTARIZE=1; NOTARY_ARGS=(--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID")
  else echo "==> No notarization credentials: signed but NOT notarized." >&2; fi
fi
if [ "$NOTARIZE" = 1 ] && [ -z "${APPLE_PASSWORD:-}" ]; then unset APPLE_ID APPLE_TEAM_ID 2>/dev/null || true; fi
export APPLE_SIGNING_IDENTITY="$IDENTITY"
# The publik app token lets a downloaded copy mint its own publik API key (src/publik.rs,
# option_env!). It is read from the environment, else from ~/.publik/gspot-app-token
# (mode 0600, written by publik's scripts/mint-app-token.mts). Never echo its value.
if [ -z "${PUBLIK_APP_TOKEN:-}" ] && [ -s "$HOME/.publik/gspot-app-token" ]; then
  PUBLIK_APP_TOKEN="$(tr -d '[:space:]' < "$HOME/.publik/gspot-app-token")"; export PUBLIK_APP_TOKEN
fi
if [ -z "${PUBLIK_APP_TOKEN:-}" ]; then echo "==> No PUBLIK_APP_TOKEN: this build cannot offer publik API (own key and local models still work)." >&2; else echo "==> publik app token present (compiled in)."; fi
echo "==> Building with identity: $IDENTITY"
cd "$ROOT/src-tauri" && "$TAURI" build
APP="$ROOT/src-tauri/target/release/bundle/macos/GSpot.app"
DMG="$(/usr/bin/find "$ROOT/src-tauri/target/release/bundle/dmg" -name "*.dmg" -print -quit 2>/dev/null || true)"
[ -d "$APP" ] || { echo "No GSpot.app produced" >&2; exit 1; }
if [ "$NOTARIZE" = 1 ]; then
  if ! xcrun stapler validate "$APP" >/dev/null 2>&1; then
    ZIP="$(mktemp -d)/GSpot.zip"; /usr/bin/ditto -c -k --keepParent "$APP" "$ZIP"
    xcrun notarytool submit "$ZIP" "${NOTARY_ARGS[@]}" --wait; xcrun stapler staple "$APP"; rm -f "$ZIP"
  fi
  if [ -n "$DMG" ]; then
    STAGE="$(mktemp -d)"; cp -R "$APP" "$STAGE/GSpot.app"; ln -s /Applications "$STAGE/Applications"; rm -f "$DMG"
    hdiutil create -volname "GSpot" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null; rm -rf "$STAGE"
    codesign --force --sign "$IDENTITY" --timestamp "$DMG"
    xcrun notarytool submit "$DMG" "${NOTARY_ARGS[@]}" --wait; xcrun stapler staple "$DMG"
  fi
fi
echo; echo "App: $APP"; [ -n "$DMG" ] && echo "Dmg: $DMG"; echo "Now run: scripts/verify-macos.sh"
