#!/usr/bin/env bash
# Hero screenshot for the README and the publik listing: the installed GSpot with the
# panel untucked beside a TextEdit draft, mistakes underlined, the suggestion card open
# on one of them. Only the app's windows and the draft are rendered (scripts/shot-compose.swift),
# on a dark backdrop, so nothing else on the desktop appears. It activates TextEdit and moves the pointer, so it waits for IDLE_NEED
# seconds (default 180) of no keyboard/mouse input and no video call, however long that
# takes; it never "runs anyway". The e2e probe (GRAMMARLAMA_DEBUG_PORT) drives the app.
#   scripts/shot-hero.sh [out.png]      default docs/hero.png
cd "$(dirname "${BASH_SOURCE[0]}")/.."
OUT="${1:-docs/hero.png}"
export APP_BIN="${APP_BIN:-/Applications/GSpot.app/Contents/MacOS/grammarlama}"
export E2E_KEEP_LLM=1              # leave the model setting alone; the shot needs no model
export LOG="${LOG:-/tmp/gspot-shot.log}"
source tests/e2e/common.sh
NEED="${IDLE_NEED:-180}"
P1="Hi team, I has attached teh final draft of the launch post. In order to be able to ship on Friday we would really need everyone's feedback by tomorrow. I think it is probably fine, but the intro could maybe be more stronger."
P2="The pricing section was wrote last night and I did not had time to proof it, so please read it twice. Their are a few places where the numbers do not match the dashboard. Let me know what you think, and thanks for you're patience!"
COMPOSE="${COMPOSE:-/tmp/gspot-shot-compose}"   # built from scripts/shot-compose.swift on demand

close_fixture_docs() {
  osascript >/dev/null 2>&1 <<'APPLESCRIPT'
tell application "TextEdit"
  set n to count documents
  repeat with i from n to 1 by -1
    try
      if (text of document i) starts with "Hi team, I has" then close document i saving no
    end try
  end repeat
end tell
APPLESCRIPT
}
restore() {
  close_fixture_docs
  [ -n "${MX0:-}" ] && probe /mouse "{\"x\": $MX0, \"y\": $MY0}" >/dev/null 2>&1
  stop_app
  # The shot stops the installed app; bring it back like the gates do.
  [ -d /Applications/GSpot.app ] && open -g /Applications/GSpot.app
}
trap restore EXIT

echo "$(date '+%H:%M:%S') waiting for ${NEED}s of idle (no call)"
while :; do
  if in_call; then sleep 30; continue; fi
  idle="$(ioreg -c IOHIDSystem 2>/dev/null | awk '/HIDIdleTime/ {print int($NF/1000000000); exit}')"
  [ -n "$idle" ] && [ "$idle" -ge "$NEED" ] && break
  sleep 10
done
echo "$(date '+%H:%M:%S') idle ${idle}s: taking the shot"

# The installed app may be running without the probe: replace it with a probed instance.
pkill -x grammarlama 2>/dev/null; for _ in $(seq 1 20); do pgrep -x grammarlama >/dev/null || break; sleep 0.25; done
start_app
wait_for 'set(d["ui_ready"]) >= {"panel","gbutton","underlines","tab"}' 20 || { echo "webviews not ready"; exit 1; }
probe /panel '{"tucked": false}' >/dev/null
wait_for 'd["panel"]["visible"] and not d["panel"]["tucked"]' 5 || { echo "panel did not untuck"; exit 1; }
# A panel whose settings lack `onboarded` jumps to the onboarding view once the state
# lands; give that a moment, then put the Review view back for the shot.
sleep 1.5
probe /panel '{"view": "review"}' >/dev/null
sleep 0.6
S="$(probe /state)"
PX="$(echo "$S" | jsonq 'int(d["panel"]["x"])')"; PY="$(echo "$S" | jsonq 'int(d["panel"]["y"])')"
PW=440; PH=740   # overlay.rs PANEL_W / PANEL_H (min(740, screen-80) on this display)
# The draft sits to the right of the panel, in big type so the thumbnail reads.
TX1=$((PX + PW + 28)); TY1=$PY; TX2=$((TX1 + 820)); TY2=$((PY + PH))   # top and bottom aligned with the panel
osascript >/dev/null <<APPLESCRIPT
tell application "TextEdit"
  activate
  set d to make new document
  set text of d to "$P1" & linefeed & linefeed & "$P2"
  set bounds of front window to {$TX1, $TY1, $TX2, $TY2}
  try
    set size of text of d to 17
  end try
  try
    set font of text of d to "Helvetica Neue"
  end try
end tell
APPLESCRIPT
sleep 0.6
osascript -e 'tell application "System Events" to tell process "TextEdit" to keystroke (ASCII character 31)' >/dev/null 2>&1 || true
wait_for 'd["doc"] is not None and d["doc"]["text"].startswith("Hi team")' 8 || { echo "document not read through AX"; exit 1; }
# The card opens on the last flagged word ("you're", first word of the last line), so it
# drops into the empty page instead of covering the draft. HOVER_WORD=teh picks a word.
wait_for 'len(d["alerts"]) >= 6' 8 || { echo "fewer than 6 alerts: $(probe /state | jsonq 'len(d["alerts"])')"; exit 1; }
if [ -n "${HOVER_WORD:-}" ]; then
  AID="$(probe /state | jsonq "next((a['id'] for a in d['alerts'] if a['original']=='$HOVER_WORD'), '')")"
else
  AID="$(probe /state | jsonq 'max(d["alerts"], key=lambda a: a["start"])["id"]')"
fi
[ -n "$AID" ] || { echo "no alert to hover"; exit 1; }
wait_for "any(r['id']=='$AID' for r in d['geometry']['rects'])" 8 || { echo "no underline rect for the hover target"; exit 1; }
sleep 1.2   # let every underline settle
M0="$(probe /mouse)"; MX0="$(echo "$M0" | jsonq 'd["x"]')"; MY0="$(echo "$M0" | jsonq 'd["y"]')"
S="$(probe /state)"
HX="$(echo "$S" | jsonq "next((round(d['geometry']['origin_x']+r['x']+r['w']/2, 1) for r in d['geometry']['rects'] if r['id']=='$AID'), None)")"
HY="$(echo "$S" | jsonq "next((round(d['geometry']['origin_y']+r['y']+r['h']/2, 1) for r in d['geometry']['rects'] if r['id']=='$AID'), None)")"
probe /mouse "{\"x\": $HX, \"y\": $HY}" >/dev/null; sleep 0.4
wait_for 'len(d["card_rects"]) > 0' 4 || echo "card did not open; shooting without it"
sleep 0.8
RX=0; RY=$((PY - 20)); RW=$((TX2 + 20)); RH=$((PH + 40))
[ -x "$COMPOSE" ] || swiftc -O -swift-version 5 -o "$COMPOSE" scripts/shot-compose.swift || { echo "compositor build failed"; exit 1; }
TE_PID="$(pgrep -x TextEdit | head -1)"
"$COMPOSE" "$OUT" "$RX" "$RY" "$RW" "$RH" "$APP_PID" "$TE_PID:$TX1,$TY1,$((TX2 - TX1)),$((TY2 - TY1))" || { echo "compose failed"; exit 1; }
echo "$(date '+%H:%M:%S') shot: $OUT $(sips -g pixelWidth -g pixelHeight "$OUT" | awk '/pixel/ {printf "%s ", $2}') alerts=$(probe /state | jsonq 'len(d["alerts"])') card=$(probe /state | jsonq 'len(d["card_rects"])')"
