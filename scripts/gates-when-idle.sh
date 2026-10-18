#!/usr/bin/env bash
# Run the release gates once the machine is idle (no keyboard/mouse for a while
# and no video call), because the host gate drives TextEdit and the pointer and
# the debug app draws overlays on whatever text field is focused. Unlike
# wait_idle this never "runs anyway": it waits as long as it takes.
#   scripts/gates-when-idle.sh [idle_seconds]
cd "$(dirname "${BASH_SOURCE[0]}")/.."
source tests/e2e/common.sh
NEED="${1:-180}"
echo "$(date '+%H:%M:%S') waiting for ${NEED}s of idle before the gates run"
while :; do
  if in_call; then sleep 30; continue; fi
  idle="$(ioreg -c IOHIDSystem 2>/dev/null | awk '/HIDIdleTime/ {print int($NF/1000000000); exit}')"
  [ -n "$idle" ] && [ "$idle" -ge "$NEED" ] && break
  sleep 10
done
echo "$(date '+%H:%M:%S') idle ${idle}s: running the gates"
bash tests/e2e/all_release.sh; rc=$?
# The gates stop every GSpot instance; bring the installed app back.
[ -d /Applications/GSpot.app ] && open -g /Applications/GSpot.app
echo "$(date '+%H:%M:%S') gates finished with exit $rc"
exit $rc
