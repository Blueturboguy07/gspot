"""Scores one corpus document's spot-check answer (stdin) against its expectations."""
import json, re, sys
c = json.load(open(sys.argv[1]))[int(sys.argv[2])]
raw = sys.stdin.read()
try:
    d = json.loads(raw)
except Exception:
    print(json.dumps({"hits": 0, "checks": 1, "notes": ["no JSON answer: " + raw[:120]], "detail": "", "ms": None, "checked": None, "sentences": None, "batches": None, "rejections": []}))
    sys.exit(0)
text = c["text"]
alerts = d.get("alerts", [])
u16 = text.encode("utf-16-le")
def sl(a, b): return u16[a * 2:b * 2].decode("utf-16-le")
def action(a): return "remove" if a["suggestions"] and a["suggestions"][0] == "" else "rephrase"
hits = 0; checks = 0; notes = []
for e in c["expect"]:
    checks += 1
    matched = [a for a in alerts if e["has"] in a["original"]]
    have = action(matched[0]) if matched else "keep"
    if have == e["action"]: hits += 1
    else: notes.append(f"'{e['has'][:24]}': want {e['action']} got {have}")
    if e.get("split"):
        checks += 1
        out = matched[0]["suggestions"][0] if matched and matched[0]["suggestions"] else ""
        if len(re.findall(r"[.!?]+(?:\s|$)", out)) >= 2: hits += 1
        else: notes.append(f"'{e['has'][:24]}': rewrite is not a split: {out[:50]!r}")
n_act = len(alerts)
if "max_actions" in c:
    checks += 1
    if n_act <= c["max_actions"]: hits += 1
    else: notes.append(f"over-edit: {n_act} > {c['max_actions']}")
if "min_actions" in c:
    checks += 1
    if n_act >= c["min_actions"]: hits += 1
    else: notes.append(f"under-edit: {n_act} < {c['min_actions']}")
if c.get("no_remove"):
    checks += 1
    rem = [a for a in alerts if action(a) == "remove"]
    if not rem: hits += 1
    else: notes.append("removed: " + "; ".join(a["original"][:30] for a in rem))
bad = []
for a in alerts:
    if sl(a["u16_start"], a["u16_end"]) != a["original"]: bad.append("offsets:" + a["original"][:20])
    if a["category"] != "clarity" or not a["kind"].startswith("Spot"): bad.append("kind:" + a["kind"])
    if action(a) == "rephrase":
        for num in re.findall(r"\d[\d,.:/%]*", a["original"]):
            if num.rstrip(".,:") not in a["suggestions"][0]: bad.append("number lost:" + num)
checks += 1
if not bad: hits += 1
else: notes.append("invariants: " + ", ".join(bad))
detail = " | ".join(f"{action(a)} '{a['original'][:28]}…'" + (f" -> '{a['suggestions'][0][:40]}…'" if action(a) == "rephrase" else "") for a in alerts)
print(json.dumps({"hits": hits, "checks": checks, "notes": notes, "detail": detail, "ms": d.get("ms"), "checked": d.get("checked"), "sentences": d.get("sentences"), "batches": d.get("batches"), "rejections": d.get("rejections", [])}))
