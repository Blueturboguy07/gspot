#!/usr/bin/env bash
# Debug build with the publik app token compiled in (same rung as build-macos.sh), for the e2e harness.
set -euo pipefail
cd "$(dirname "$0")/../src-tauri"
if [ -z "${PUBLIK_APP_TOKEN:-}" ] && [ -s "$HOME/.publik/gspot-app-token" ]; then
  PUBLIK_APP_TOKEN="$(tr -d '[:space:]' < "$HOME/.publik/gspot-app-token")"; export PUBLIK_APP_TOKEN
fi
[ -n "${PUBLIK_APP_TOKEN:-}" ] && echo "publik app token present" || echo "no publik app token (publik checks will be skipped)"
cargo build "$@"
