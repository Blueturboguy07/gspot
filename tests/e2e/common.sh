#!/usr/bin/env bash
# Shared helpers for the e2e harness. The app is launched with the debug probe
# enabled on 127.0.0.1:$PORT; every check talks to that probe.
set -uo pipefail
PORT="${GRAMMARLAMA_DEBUG_PORT:-47331}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
APP_BIN="${APP_BIN:-$ROOT/src-tauri/target/debug/grammarlama}"
# Absolute path, so stop_app can kill exactly the binary it launched and nothing else (e.g. /Applications).
case "$APP_BIN" in /*) ;; *) APP_BIN="$PWD/$APP_BIN";; esac
LOG="${LOG:-/tmp/grammarlama-e2e.log}"
PASS=0; FAIL=0
pass() { printf '  \033[32mPASS\033[0m  %s\n' "$1"; PASS=$((PASS+1)); }
fail() { printf '  \033[31mFAIL\033[0m  %s\n' "$1"; FAIL=$((FAIL+1)); }
probe() { local body="${2:-}"; [ -z "$body" ] && body='{}'; curl -s -m "${3:-120}" -X POST "http://127.0.0.1:$PORT$1" -d "$body"; }
jsonq() { python3 -I -c 'import json,sys; d=json.load(sys.stdin); print(eval(sys.argv[1], {"d": d, "len": len, "any": any, "all": all, "str": str, "float": float, "int": int, "round": round, "abs": abs}))' "$1"; }

ensure_vite() {
  # A `cargo build` (debug) binary loads the devUrl, so the Vite dev server must be up for the UI.
  case "$APP_BIN" in */target/debug/*) ;; *) return 0;; esac
  if curl -s -m 1 -o /dev/null http://localhost:1430/; then return 0; fi
  (cd "$ROOT/ui" && nohup pnpm run dev >/tmp/grammarlama-vite.log 2>&1 &)
  for _ in $(seq 1 40); do sleep 0.25; curl -s -m 1 -o /dev/null http://localhost:1430/ && { echo "vite dev server up"; return 0; }; done
  echo "vite did not start; see /tmp/grammarlama-vite.log"; return 1
}

# The product offers no local model (publik API or your own key only). The rig
# still runs the LLM gates on a local Ollama: point the app at it through the
# probe once it is up. E2E_KEEP_LLM=1 runs the gates on the app's own setting.
rig_model() {
  [ "${E2E_KEEP_LLM:-0}" = 1 ] && return 0
  probe /settings "{\"llm\": {\"provider\": \"ollama\", \"base_url\": \"${E2E_OLLAMA_URL:-http://127.0.0.1:11434}\", \"api_key\": \"\", \"model\": \"${E2E_MODEL:-qwen2.5:7b}\"}}" >/dev/null 2>&1 || true
}

start_app() {
  if curl -s -m 2 "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then echo "app already running on :$PORT"; rig_model; return 0; fi
  [ -x "$APP_BIN" ] || { echo "no binary at $APP_BIN (build first)"; exit 1; }
  ensure_vite || exit 1
  GRAMMARLAMA_DEBUG_PORT="$PORT" RUST_LOG=info "$APP_BIN" >"$LOG" 2>&1 &
  APP_PID=$!
  for _ in $(seq 1 40); do
    sleep 0.25
    if curl -s -m 1 "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then echo "app up (pid $APP_PID)"; rig_model; return 0; fi
  done
  echo "app did not start; log:"; tail -20 "$LOG"; exit 1
}

stop_app() { [ -n "${APP_PID:-}" ] && kill "$APP_PID" 2>/dev/null; pkill -f "${APP_BIN}\$" 2>/dev/null; true; }

# The host gates activate TextEdit, type, and move the pointer; a person using the
# machine at the same time changes focus and overrides synthetic input. Wait for
# the HID idle time to reach $1 seconds (up to $2 seconds), then go.
# A video call is "in use" even when the keyboard and mouse are idle: never
# activate windows or move the pointer while one runs.
in_call() { pgrep -x FaceTime >/dev/null 2>&1 || pgrep -x zeroes >/dev/null 2>&1 || pgrep -x "zoom.us" >/dev/null 2>&1 || pgrep -x "Microsoft Teams" >/dev/null 2>&1 || pgrep -x Webex >/dev/null 2>&1; }

wait_idle() { # wait_idle idle_s max_wait_s  -> 0 when it is safe to drive the desktop, 1 when a call is on
  local need="${1:-5}" max="${2:-120}" waited=0 idle
  while :; do
    if in_call; then
      [ "$waited" -ge "$max" ] && { echo "a video call is running (FaceTime/Zoom/Teams); this gate drives the desktop, so it is skipped — rerun after the call"; return 1; }
      [ "$waited" -eq 0 ] && echo "a video call is running; waiting up to ${max}s for it to end"
      sleep 5; waited=$((waited+5)); continue
    fi
    idle="$(ioreg -c IOHIDSystem 2>/dev/null | awk '/HIDIdleTime/ {print int($NF/1000000000); exit}')"
    [ -z "$idle" ] && return 0
    [ "$idle" -ge "$need" ] && return 0
    [ "$waited" -ge "$max" ] && { echo "still in use after ${max}s (idle ${idle}s); running anyway"; return 0; }
    [ "$waited" -eq 0 ] && echo "machine in use (idle ${idle}s); waiting up to ${max}s for ${need}s of no input"
    sleep 2; waited=$((waited+2))
  done
}

wait_for() { # wait_for '<python expr over d from /state>' timeout_s
  local expr="$1" t="${2:-10}" i=0
  while [ "$i" -lt $((t*4)) ]; do
    if [ "$(probe /state | jsonq "$expr" 2>/dev/null)" = "True" ]; then return 0; fi
    sleep 0.25; i=$((i+1))
  done
  return 1
}

summary() { echo; echo "$PASS passed, $FAIL failed"; [ "$FAIL" -eq 0 ]; }
