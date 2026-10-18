#!/usr/bin/env bash
# Score the spot-check corpus through the real Rust pipeline against a local
# model, with no app windows and nothing on the desktop. Same evaluator and
# expectations as the e2e gate (tests/e2e/spotcheck.sh).
#   tests/tuning/live_corpus.sh [corpus.json] [model]
set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CORPUS="${1:-$ROOT/tests/e2e/fixtures/spotcheck_corpus.json}"
MODEL="${2:-qwen2.5:7b}"
TMP="$(mktemp -d)"
N="$(python3 -I -c 'import json,sys; print(len(json.load(open(sys.argv[1]))))' "$CORPUS")"
TOTAL=0; HIT=0; START=$(date +%s)
cd "$ROOT/src-tauri" && cargo test --lib live_text -- --ignored >/dev/null 2>&1 || true  # compile once
for i in $(seq 0 $((N-1))); do
  python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))[int(sys.argv[2])]["text"], end="")' "$CORPUS" "$i" >"$TMP/$i.txt"
  NAME="$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))[int(sys.argv[2])]["name"])' "$CORPUS" "$i")"
  OUT="$(GRAMMARLAMA_LIVE_TEXT="$TMP/$i.txt" GRAMMARLAMA_LIVE_MODEL="$MODEL" cargo test --lib live_text -- --ignored --nocapture 2>/dev/null | sed -n 's/^LIVE_JSON //p')"
  RES="$(python3 -I "$ROOT/tests/e2e/spotcheck_eval.py" "$CORPUS" "$i" <<<"$OUT")"
  H="$(python3 -I -c 'import json,sys; print(json.load(sys.stdin)["hits"])' <<<"$RES")"; C="$(python3 -I -c 'import json,sys; print(json.load(sys.stdin)["checks"])' <<<"$RES")"
  TOTAL=$((TOTAL+C)); HIT=$((HIT+H))
  MARK="ok  "; [ "$H" = "$C" ] || MARK="MISS"
  printf "%s %-16s %s/%s  %s\n" "$MARK" "$NAME" "$H" "$C" "$(python3 -I -c 'import json,sys; d=json.load(sys.stdin); print("; ".join(d["notes"]))' <<<"$RES")"
  python3 -I -c 'import json,sys; d=json.load(sys.stdin); print("     ", d["detail"] or "no suggestions"); [print("      rejected:", r) for r in d["rejections"]]' <<<"$RES"
done
echo "corpus score: $HIT/$TOTAL = $((HIT*100/TOTAL))% in $(( $(date +%s) - START )) s (model $MODEL)"
rm -rf "$TMP"
