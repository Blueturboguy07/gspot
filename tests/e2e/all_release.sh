#!/usr/bin/env bash
# Runs every gate against the signed release bundle: host (TextEdit) + features (LLM/web) + plagiarism.
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
export APP_BIN="$PWD/src-tauri/target/release/bundle/macos/GSpot.app/Contents/MacOS/grammarlama"
[ -x "$APP_BIN" ] || { echo "no release bundle at $APP_BIN"; exit 1; }
pkill -f 'target/debug/grammarlama$' 2>/dev/null; pkill -f 'GSpot.app/Contents/MacOS/grammarlama$' 2>/dev/null; sleep 0.5
FAILED=0
for s in host_textedit features spotcheck setup; do
  echo "=== $s (release) ==="
  bash "tests/e2e/$s.sh" || FAILED=1
  sleep 1
done
echo; [ "$FAILED" -eq 0 ] && echo "ALL GATES GREEN on the signed bundle" || echo "SOME GATES FAILED"
exit $FAILED
