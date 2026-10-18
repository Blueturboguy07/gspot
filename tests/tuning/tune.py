import json, re, sys, time, urllib.request
prompt = open(sys.argv[1]).read()
model = sys.argv[2] if len(sys.argv) > 2 else "qwen2.5:7b"
corpus = json.load(open("corpus.json"))
def split(text):
    out = []
    for para in text.split("\n"):
        for m in re.finditer(r"[^.!?]+[.!?]+[\"”']?|[^.!?]+$", para):
            s = m.group(0).strip()
            if len(s.split()) >= 4: out.append(s)
    return out
WORDY = ["in order to","at this point in time","due to the fact that","in the event that","it is important to note that","please be advised that","in due course","circle back","utilization of","a large number of","a very large number of","the reason for this is","for the purpose of","in the process of","with regard to","in spite of the fact that","on a daily basis","at the end of the day","needless to say","the fact that","in a position to","as a matter of fact","it should be noted that","what i am trying to say"]
STOP = set("the and that this with for are was were have has had not but you your they them their from into than then there here what when which who will would could should can all any its our out very just also been being did does doing much many more most some such too about over under after before again still ever even only own same those these each other so as at by in of on or to up is it be we he she him her his me my an a i".split())
def cw(s): return [w for w in "".join(c if c.isalnum() or c=="'" else " " for c in s.lower()).split() if len(w)>=3 and w not in STOP]
HEDGES = ["i think", "probably", "maybe", "kind of", "sort of", "consider", "i feel like", "perhaps", "might", "i guess"]
def hints(sents):
    out = []
    for i, s in enumerate(sents):
        low = s.lower()
        found = [w for w in WORDY if w in low]
        if found: out.append(f"{i+1}: wordy phrase \"{found[0]}\"")
        if sum(1 for h in HEDGES if h in low) >= 2: out.append(f"{i+1}: hedge stack")
        if len(s.split()) > 30: out.append(f"{i+1}: too long ({len(s.split())} words): split it into two or three sentences at its natural joints")
        words = cw(s)
        if words and len(s.split()) <= 12:
            for j in range(i):
                pool = set(cw(sents[j]))
                if sum(1 for w in words if w in pool) / len(words) >= 0.7:
                    out.append(f"{i+1}: may repeat sentence {j+1}"); break
    return out
def ask(sents):
    user = "SENTENCES:\n" + "\n".join(f"{i+1}. {s}" for i, s in enumerate(sents))
    h = hints(sents)
    if h and "HINTS" in prompt: user += "\n\nHINTS:\n" + "\n".join(h)
    body = {"model": model, "stream": False, "format": "json", "options": {"temperature": 0.2, "num_ctx": 8192},
            "messages": [{"role": "system", "content": prompt}, {"role": "user", "content": user}]}
    req = urllib.request.Request("http://127.0.0.1:11434/api/chat", data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    t = time.time(); d = json.load(urllib.request.urlopen(req, timeout=300)); dt = time.time() - t
    try: items = json.loads(d["message"]["content"]).get("items", [])
    except Exception: items = []
    return {int(it.get("i", 0)): it for it in items if isinstance(it, dict)}, dt
total = 0; got = 0; details = []
for c in corpus:
    sents = split(c["text"]); items, dt = ask(sents)
    acts = {i + 1: (items.get(i + 1, {}).get("action") or "keep") for i in range(len(sents))}
    hits = 0; checks = 0; notes = []
    for k, want in c["expect"].items():
        checks += 1; have = acts.get(int(k), "keep")
        if have == want: hits += 1
        else: notes.append(f"s{k}: want {want} got {have}")
    n_act = sum(1 for a in acts.values() if a != "keep")
    if "max_actions" in c:
        checks += 1
        if n_act <= c["max_actions"]: hits += 1
        else: notes.append(f"over-edit: {n_act} actions > {c['max_actions']}")
    if c.get("no_remove"):
        checks += 1
        if not any(a == "remove" for a in acts.values()): hits += 1
        else: notes.append("removed something: " + ", ".join(f"s{i}" for i, a in acts.items() if a == "remove"))
    total += checks; got += hits
    details.append(f"{c['name']:16s} {hits}/{checks} ({dt:4.1f}s) {'; '.join(notes)}")
    for i, s in enumerate(sents):
        it = items.get(i + 1, {})
        if it.get("action") in ("rephrase", "remove"): details.append(f"    s{i+1} {it.get('action')}: {it.get('text','')[:110]!r} why={it.get('why','')[:60]!r}")
print("\n".join(details)); print(f"SCORE {got}/{total} = {got/total:.0%}")
