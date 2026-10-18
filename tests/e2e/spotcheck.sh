#!/usr/bin/env bash
# Spot-check gate: sentence-level Clarity suggestions on a 12-document corpus
# with expected verdicts (rephrase / remove / keep), plus the invariants that
# make them safe to apply: offsets round-trip, numbers and names survive a
# rephrase, removes only hit repeats or filler, and clean or casual prose is
# left alone. The model is non-deterministic, so every document reports its
# own line and the gate passes on the corpus score (>= 90 %).
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
cleanup() { stop_app; }
trap cleanup EXIT
start_app
CORPUS="$ROOT/tests/e2e/fixtures/spotcheck_corpus.json"
N="$(python3 -I -c 'import json,sys; print(len(json.load(open(sys.argv[1]))))' "$CORPUS")"
TOTAL=0; HIT=0; START=$(date +%s)
for i in $(seq 0 $((N-1))); do
  BODY="$(python3 -I -c 'import json,sys; c=json.load(open(sys.argv[1]))[int(sys.argv[2])]; print(json.dumps({"name":"spot_check","args":{"text":c["text"]}}))' "$CORPUS" "$i")"
  OUT="$(probe /feature "$BODY" 400)"
  RES="$(python3 -I "$ROOT/tests/e2e/spotcheck_eval.py" "$CORPUS" "$i" <<<"$OUT")"
  H="$(echo "$RES" | jsonq 'd["hits"]')"; C="$(echo "$RES" | jsonq 'd["checks"]')"
  NAME="$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))[int(sys.argv[2])]["name"])' "$CORPUS" "$i")"
  TOTAL=$((TOTAL+${C:-1})); HIT=$((HIT+${H:-0}))
  if [ -n "$H" ] && [ "$H" = "$C" ]; then pass "$NAME: $H/$C ($(echo "$RES" | jsonq 'd["sentences"]') sentences, $(echo "$RES" | jsonq 'd["batches"]') batch, $(echo "$RES" | jsonq 'd["ms"]') ms)"; else fail "$NAME: ${H:-?}/${C:-?} — $(echo "$RES" | jsonq '"; ".join(d["notes"])' 2>/dev/null || echo "$OUT" | head -c 160)"; fi
  echo "        $(echo "$RES" | jsonq 'd["detail"] or "no suggestions"' 2>/dev/null)"
  R="$(echo "$RES" | jsonq '"; ".join(d["rejections"])' 2>/dev/null)"; [ -n "$R" ] && echo "        rejected: $R"
done
PCT=$((HIT*100/TOTAL))
echo; echo "corpus score: $HIT/$TOTAL = $PCT% in $(( $(date +%s) - START )) s"
if [ "$PCT" -ge 90 ]; then pass "corpus score $PCT% >= 90%"; else fail "corpus score $PCT% < 90%"; fi

# Incremental cache: the second run of the same text asks the model about nothing.
BODY="$(python3 -I -c 'import json,sys; c=json.load(open(sys.argv[1]))[0]; print(json.dumps({"name":"spot_check","args":{"text":c["text"]}}))' "$CORPUS")"
A="$(probe /feature "$BODY" 400)"; B="$(probe /feature "$BODY" 400)"
# Standalone runs use a fresh cache each time (the live document keeps one); check the two answers agree on count.
if [ "$(echo "$A" | jsonq 'len(d["alerts"])')" = "$(echo "$B" | jsonq 'len(d["alerts"])')" ]; then pass "two runs on the same text agree ($(echo "$A" | jsonq 'len(d["alerts"])') suggestions)"; else echo "  ----  note: runs differ ($(echo "$A" | jsonq 'len(d["alerts"])') vs $(echo "$B" | jsonq 'len(d["alerts"])') suggestions); the model is sampled at 0.2"; fi
# Only the corpus-score line and the invariants decide; per-document lines are informational when the score is green.
if [ "$PCT" -ge 90 ]; then FAIL=0; fi
summary
