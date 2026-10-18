# Prompt tuning rig for the spot check

Runs a prompt variant straight against Ollama (no app) over the 12-document corpus and
scores it against the expected verdicts. Used to go from v2 (92 %) to v5 (95 %); the
results and what each variant changed are in `docs/SPOTCHECK.md`.

```bash
cd tests/tuning && python3 -I tune.py spot_v5.txt            # default model qwen2.5:7b
cd tests/tuning && python3 -I tune.py spot_v5.txt qwen2.5:3b  # another local model
```

`tune.py` reproduces the scanner hints (wordy phrases, hedge stacks, repeats) that the app
adds in `src-tauri/src/features/spotcheck.rs`; the app's guards are not part of this rig,
so a prompt score here is a floor, not the gate. The gate is `tests/e2e/spotcheck.sh`.
