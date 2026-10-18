#!/usr/bin/env bash
# Focused check: the plagiarism feature finds the Wikipedia source for a copied sentence.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
WIKI="The Great Barrier Reef is the world's largest coral reef system, composed of over 2,900 individual reefs and 900 islands stretching for over 2,300 kilometres over an area of approximately 344,400 square kilometres. The reef is located in the Coral Sea, off the coast of Queensland, Australia, separated from the coast by a channel 160 kilometres wide in places and over 61 metres deep."
cleanup() { stop_app; }
trap cleanup EXIT
start_app
WB="$(python3 -I -c 'import json,sys; print(json.dumps({"name":"plagiarism","args":{"text":sys.argv[1]}}))' "$WIKI")"
OUT="$(probe /feature "$WB" 300)"
echo "$OUT" | jsonq '(len(d["sources"]), round(d["overall_pct"],1), [(s["site"], s["matched_words"]) for s in d["sources"][:4]], d["queries"])'
if [ "$(echo "$OUT" | jsonq 'len(d["sources"]) >= 1')" = "True" ]; then pass "plagiarism finds a source: $(echo "$OUT" | jsonq 'd["sources"][0]["citations"]["APA"]')"; else fail "no sources: $(echo "$OUT" | head -c 400)"; fi
summary
