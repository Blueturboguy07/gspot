#!/usr/bin/env bash
# G3–G5 gate: every LLM/web feature returns the shape the UI renders, on real text.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
TEXT='Six weeks ago I pulled out of school and shut the fuck up. I didnt tell my friends and I didnt tell my peers. I lived in a tiny apartment with a blanket over the window to block the sun, watching people around me go to class while I sent thousands of cold emails. It did not feel great. Then one morning a founder replied, we got on a call, and by the next day the deal was done. We raised over half a million dollars to kill slop companies. I am coming after every subscription that charges you for something a free tool can do.'
WIKI='The Great Barrier Reef is the world'"'"'s largest coral reef system, composed of over 2,900 individual reefs and 900 islands stretching for over 2,300 kilometres over an area of approximately 344,400 square kilometres. The reef is located in the Coral Sea, off the coast of Queensland, Australia, separated from the coast by a channel 160 kilometres wide in places and over 61 metres deep.'
cleanup() { stop_app; }
trap cleanup EXIT
start_app
py() { python3 -I -c "import json,sys; t=json.dumps(sys.argv[1]); print('{\"name\": \"%s\", \"args\": %s}' % (sys.argv[2], sys.argv[3].replace('@TEXT@', t)))" "$TEXT" "$1" "$2"; }

run() { # run <label> <name> <args-json-with-@TEXT@> <python check>
  local label="$1" name="$2" args="$3" check="$4"
  local body; body="$(py "$name" "$args")"
  local out; out="$(probe /feature "$body" 300)"
  if [ "$(echo "$out" | jsonq "$check" 2>/dev/null)" = "True" ]; then pass "$label"; else fail "$label → $(echo "$out" | head -c 240)"; fi
}

run "ping model" ping_llm '{}' 'len(d["reply"]) > 0'
run "paraphrase (Professional) keeps the sentence count" paraphrase '{"text": @TEXT@, "style": "Professional"}' 'len(d["rewritten"]) > 80 and len(d["summary"]) > 10 and d["sentences_out"] >= 0.75 * d["sentences_in"]'
run "paraphrase of one selected sentence stays one sentence with its numbers" paraphrase '{"text": "In order to be able to make progress at this point in time, it was decided by me that we should send 2,000 cold emails to founders before March 3.", "style": "Streamlined"}' 'd["sentences_out"] <= 2 and "2,000" in d["rewritten"] and "March" in d["rewritten"]'
run "humanize (The Executive) keeps the sentence count" humanize '{"text": @TEXT@, "voice_name": "The Executive", "voice_description": "Confident, direct, decisive."}' 'len(d["rewritten"]) > 80 and d["sentences_out"] >= 0.75 * d["sentences_in"]'
run "reader reactions" reactions '{"text": @TEXT@, "audience": "Online followers", "audience_desc": "skim fast"}' 'len(d["remember"]) == 3 and len(d["ask"]) == 5 and all("question" in q for q in d["ask"])'
run "translate (Spanish)" translate '{"text": @TEXT@, "target": "Spanish"}' 'len(d["translation"]) > 80'
run "explain it" explain '{"text": @TEXT@, "whole": true}' 'len(d["explanation"]) > 60'
run "tone" tone '{"text": @TEXT@}' 'len(d["tones"]) >= 1 and all("name" in t and "confidence" in t and "emoji" not in t for t in d["tones"])'
run "ai rewriter scan" rewriter_scan '{"text": "In today'"'"'s fast-paced world, it is important to note that we must delve into the rich tapestry of innovation. Furthermore, leveraging synergy is crucial."}' 'len(d["phrases"]) >= 3 and d["phrases"][0]["bucket"] in ("frequent","often","sometimes")'
run "ai rewrite phrase" rewrite_phrase '{"sentence": "We must delve into the data before launch.", "phrase": "delve into"}' 'len(d["suggestions"]) >= 1 and "delve" not in d["suggestions"][0].lower()'
run "ai detector (human text)" detector '{"text": @TEXT@}' 'd["authenticity"] >= 50 and "signals" in d'
run "ai detector (ai text)" detector '{"text": "In today'"'"'s fast-paced digital landscape, it is important to note that organizations must leverage robust, scalable solutions. Furthermore, embracing a holistic approach fosters a culture of innovation. Ultimately, navigating these complexities requires a comprehensive strategy. Moreover, stakeholders must prioritize seamless integration to unlock the transformative potential of emerging technologies. In conclusion, the journey toward excellence is a testament to collective dedication."}' 'd["ai_likelihood"] >= 50'
run "summarize" summarize '{"text": @TEXT@}' 'len(d["summary"]) > 40'
run "spot check (standalone) finds the wordy sentence and keeps the facts" spot_check '{"text": "Six weeks ago I pulled out of school. In order to be able to make progress at this point in time, it was decided by me that we should send a very large number of cold emails. We raised $500,000 from Verdict Capital on October 12."}' 'any("In order to" in a["original"] and a["suggestions"][0] != "" for a in d["alerts"]) and not any("Verdict" in a["original"] for a in d["alerts"])'
WB="$(python3 -I -c "import json,sys; print(json.dumps({'name':'plagiarism','args':{'text':sys.argv[1]}}))" "$WIKI")"
OUT="$(probe /feature "$WB" 300)"
if [ "$(echo "$OUT" | jsonq 'len(d["sources"]) >= 1 and all(("APA" in s["citations"]) for s in d["sources"])')" = "True" ]; then pass "plagiarism finds the Wikipedia source ($(echo "$OUT" | jsonq 'd["sources"][0]["site"]'), $(echo "$OUT" | jsonq 'round(d["overall_pct"])')%)"; else fail "plagiarism → $(echo "$OUT" | head -c 300)"; fi
summary
