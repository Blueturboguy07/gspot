#!/usr/bin/env bash
# AI set-up gate: the two onboarding choices end to end through the probe —
# publik API (provision → key file → a metered call answered per contract →
# forget) and your own key (prefix detection → model listing → ping) — plus
# the rig's own Ollama (the product offers no local model; the LLM gates run
# on it through the probe). Settings written by the app still load.
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
cleanup() { stop_app; }
trap cleanup EXIT
start_app
ORIG="$(probe /settings)"
ORIG_LLM="$(echo "$ORIG" | jsonq 'json.dumps(d["llm"])' 2>/dev/null || python3 -I -c 'import json,sys; print(json.dumps(json.load(sys.stdin)["llm"]))' <<<"$ORIG")"
restore() { probe /settings "{\"llm\": $ORIG_LLM, \"onboarded\": $(echo "$ORIG" | jsonq 'str(d["onboarded"]).lower()')}" >/dev/null; }
trap 'restore; cleanup' EXIT

# --- the rig's model (not a product option)
O="$(probe /ollama)"
if [ "$(echo "$O" | jsonq 'd["running"] and d["pick"] is not None')" = "True" ]; then pass "rig model: Ollama with $(echo "$O" | jsonq 'd["pick"]') ($(echo "$O" | jsonq 'len(d["models"])') models)"; else echo "  ----  Ollama not running; rig-model check skipped"; fi
[ "$(echo "$ORIG" | jsonq 'd["llm"]["provider"]')" = "ollama" ] && pass "start_app pointed the app at the rig's Ollama through the probe" || fail "start_app did not set the rig model: $(echo "$ORIG" | jsonq 'd["llm"]["provider"]')"

# --- publik API
S="$(probe /publik '{"action":"status"}')"
if [ "$(echo "$S" | jsonq 'd["can_provision"]')" != "True" ]; then
  echo "  ----  no app token compiled into this binary; publik checks skipped (build with scripts/build-debug.sh)"
else
  [ "$(echo "$S" | jsonq 'd["justification"].startswith("The model") and "dashboard" in d["justification"]')" = "True" ] && pass "status carries the one justification sentence" || fail "justification: $(echo "$S" | jsonq 'd["justification"][:60]')"
  [ "$(echo "$S" | jsonq '"$0.00" in d["disclosure"]["cost"] and "$0.05" in d["disclosure"]["cost"] and "never trains" in d["disclosure"]["data"]')" = "True" ] && pass "disclosure says \$0.00 start, \$0.05 on link, no training" || fail "disclosure: $(echo "$S" | jsonq 'd["disclosure"]')"
  CRED="$HOME/Library/Application Support/publik/apps/gspot.json"
  HAD_KEY=0; [ -f "$CRED" ] && HAD_KEY=1 && cp "$CRED" "$CRED.e2e-backup"
  [ "$HAD_KEY" = 1 ] && rm -f "$CRED"
  P="$(probe /publik '{"action":"provision"}' 60)"
  if [ "$(echo "$P" | jsonq 'd.get("has_key") and d.get("selected")' 2>/dev/null)" = "True" ]; then
    pass "POST /installs minted a key; publik API selected"
    [ -f "$CRED" ] && [ "$(stat -f %Lp "$CRED")" = "600" ] && pass "credential file written with mode 600" || fail "credential file: $(ls -la "$CRED" 2>&1)"
    [ "$(python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); print(d["key"].startswith("pk_") and d["base_url"].startswith("https://publikhq.com") and d["app_slug"]=="grammarlama")' "$CRED")" = "True" ] && pass "credential has a pk_ key, the publikhq root and the app slug" || fail "credential contents"
    [ "$(echo "$P" | jsonq 'd["claim_state"]=="anonymous" and d["balance_line"].startswith("$0.00") and "$0.05" in d["balance_line"]')" = "True" ] && pass "anonymous install: '$(echo "$P" | jsonq 'd["balance_line"]')'" || fail "balance line: $(echo "$P" | jsonq 'd["balance_line"], d["claim_state"]')"
    [ "$(echo "$P" | jsonq 'd["first_run_owed"] and d["cta"]["label"]=="Sign in with publik" and d["cta"]["url"].startswith("https://publikhq.com/")')" = "True" ] && pass "first-run card owed with '$(echo "$P" | jsonq 'd["cta"]["label"]')' → publikhq.com" || fail "cta: $(echo "$P" | jsonq 'd["cta"], d["first_run_owed"]')"
    [ "$(probe /settings | jsonq 'd["llm"]["provider"]=="publik" and d["llm"]["api_key"]=="" and d["llm"]["model"].startswith("publik-")')" = "True" ] && pass "settings select publik with the alias model and no key stored" || fail "settings.llm: $(probe /settings | jsonq 'd["llm"]')"
    # A metered call. At $0.00 the contract answers 402 with a message and exactly one link; with credit it answers.
    R="$(probe /feature '{"name":"ping_llm","args":{}}' 120)"
    S2="$(probe /publik '{"action":"status"}')"
    if echo "$R" | grep -q '"reply"'; then
      pass "publik API answered a chat call (balance now $(echo "$S2" | jsonq 'd["balance_line"]'))"
    elif [ "$(echo "$S2" | jsonq 'd["state"]=="needs_credit" and d["notice"] is not None and d["notice"]["link_url"].startswith("https://publikhq.com/") and len(d["notice"]["message"])>10')" = "True" ]; then
      pass "at \$0.00 the gateway's 402 is rendered as its message plus one publikhq.com link ('$(echo "$S2" | jsonq 'd["notice"]["link_label"]')')"
    else
      fail "metered call: $(echo "$R" | head -c 200) / status $(echo "$S2" | jsonq 'd["state"], d["notice"]')"
    fi
    D="$(probe /publik '{"action":"dismiss_first_run"}')"
    [ "$(echo "$D" | jsonq 'not d["first_run_owed"]')" = "True" ] && pass "'Later' settles the first-run card" || fail "first_run_owed still set"
    F="$(probe /publik '{"action":"forget"}')"
    [ "$(echo "$F" | jsonq 'not d["has_key"]')" = "True" ] && [ ! -f "$CRED" ] && pass "forget removes the credential file" || fail "forget: $(echo "$F" | jsonq 'd["has_key"]') file=$([ -f "$CRED" ] && echo present || echo gone)"
    # No local model to fall back to (Oct 18): publik API stays the provider and reports itself as not set up.
    [ "$(probe /settings | jsonq 'd["llm"]["provider"]=="publik"')" = "True" ] && [ "$(echo "$F" | jsonq 'd["state"]=="not_set_up" and not d["selected"] or d["state"]=="not_set_up"')" = "True" ] && pass "after forget publik API stays selected and reports not set up" || fail "after forget: provider=$(probe /settings | jsonq 'd["llm"]["provider"]') state=$(echo "$F" | jsonq 'd["state"]')"
  else
    fail "provision: $(echo "$P" | head -c 240)"
  fi
  [ "$HAD_KEY" = 1 ] && mv "$CRED.e2e-backup" "$CRED" && probe /publik '{"action":"provision"}' >/dev/null && echo "  ----  restored the existing publik credential"
fi

# --- your own key (one field). Uses a real key only if one is at hand, never printed.
KEY="${E2E_BYO_KEY:-}"
if [ -z "$KEY" ] && [ -f "$HOME/publik/.env.local" ]; then KEY="$(grep -E '^ANTHROPIC_API_KEY=' "$HOME/publik/.env.local" | head -1 | cut -d= -f2- | tr -d '"'"'"' ' | tr -d '[:space:]')"; fi
B="$(probe /probe_key '{"key":"sk-bogus-not-a-key"}' 60)"
echo "$B" | grep -q '"error"' && echo "$B" | grep -qi "not accepted" && pass "a bad key is refused with a plain message" || fail "bad key: $(echo "$B" | head -c 160)"
if [ -n "$KEY" ]; then
  KB="$(python3 -I -c 'import json,sys; print(json.dumps({"key": sys.argv[1]}))' "$KEY")"
  R="$(probe /probe_key "$KB" 90)"
  if [ "$(echo "$R" | jsonq 'd["provider"]=="anthropic" and d["model"].startswith("claude") and "pong" in d["reply"].lower()' 2>/dev/null)" = "True" ]; then
    pass "own key: prefix → anthropic, model $(echo "$R" | jsonq 'd["model"]') picked from $(echo "$R" | jsonq 'd["models"]') listed, ping answered"
    PATCH="$(python3 -I -c 'import json,sys; r=json.loads(sys.argv[2]); print(json.dumps({"llm":{"provider":r["provider"],"base_url":r["base_url"],"api_key":sys.argv[1],"model":r["model"]}}))' "$KEY" "$R")"
    probe /settings "$PATCH" >/dev/null
    R2="$(probe /feature '{"name":"ping_llm","args":{}}' 90)"
    echo "$R2" | grep -q '"reply"' && pass "features run on the pasted key" || fail "ping on own key: $(echo "$R2" | head -c 160)"
  else
    fail "own key probe: $(echo "$R" | sed 's/sk-ant-[A-Za-z0-9_-]*/sk-ant-…/g' | head -c 200)"
  fi
else
  echo "  ----  no Anthropic key at hand (E2E_BYO_KEY or ~/publik/.env.local); own-key live check skipped"
fi

# --- settings written by this build still load with every new field present
SET="$HOME/Library/Application Support/com.publik.grammarlama/settings.json"
[ "$(python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); print("auto_spot_check" in d and "publik" in d and "disclosure_version" in d["publik"])' "$SET")" = "True" ] && pass "settings.json carries auto_spot_check and publik fields" || fail "settings.json fields"
summary
