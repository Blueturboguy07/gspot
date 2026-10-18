# Spot check: sentence-level suggestions, and how it was tuned

**Problem (Mann, 2026-10-15):** "it's not good at spot checking and rephrasing or removing
sentences effectively; I think it might be intaking the whole thing and cutting for clarity."

Two causes, two fixes:

1. The rewrite tools (Paraphraser, Humanizer) sent the whole text in one call. A 7B model
   answers a long input by summarizing it: sentences vanish. Now every rewrite runs
   paragraph-chunk by paragraph-chunk (≤ 900 chars per call), the prompt says "work sentence
   by sentence, keep the same number of sentences", and a rewrite that drops more than 25 % of
   the sentences is retried once with "output exactly N sentences". The answer carries
   `sentences_in` / `sentences_out` / `retries` so the gate can see it.
2. There was no sentence-level check at all. The new **Spot check** (Review → "Spot check",
   and by itself 3 s after typing pauses) sends numbered sentences in batches of ≤ 8 and gets
   one verdict per number: keep, rephrase (one replacement sentence) or remove. Verdicts become
   ordinary Clarity alerts with exact offsets, so they underline in the host app, open on
   hover, and apply with Accept like every other suggestion.

## The pipeline (src-tauri/src/features/spotcheck.rs)

| Stage | What it does |
|---|---|
| Split | Sentences with char offsets; breaks at `.!?` + space and at newlines; never inside "Dr.", "e.g.", "$1.2M", "J. K. Rowling". Skips fragments under 4 words, headings, URLs, code. |
| Hints | A deterministic scanner adds `HINTS` the model can trust: 34 wordy phrases ("in order to", "at this point in time"…), a hedge stack (2+ of "I think", "probably", "maybe"…), sentences over 32 words, and "may repeat sentence N" when ≥ 70 % of a short sentence's content words already appeared earlier. |
| Model | Batches of ≤ 8 sentences / 1400 chars with the previous sentence as context; JSON `{items:[{i, flags, action, text, why, confidence}]}`; temperature 0.2. |
| Guards | A **rephrase** must: differ from the original, stay one sentence, keep every number, every proper noun and every quoted span, add no AI phrase, keep the author's opening case and terminal punctuation, stay within a length band (down to 28 % of a ≥ 20-word sentence, 40 % otherwise, up to 140 %), have confidence ≥ 3, and not touch a sentence under 7 words unless a hint named it. A **remove** must: be ≤ 30 words, carry no number or name that appears nowhere else, and be either a repeat (≥ 60 % content-word overlap with an earlier sentence) or filler (regex: "as I said before", "anyway, that is what happened", "so here is the thing"…). A filler sentence the model "rephrased" becomes a remove. |
| Churn cap | At most half of a batch may change; the rest is dropped by confidence. |
| Cache | Verdicts are cached per (previous sentence, sentence); re-running after an edit asks only about changed sentences. |
| Anchor | Alerts re-anchor to the live text after edits (same text at the same offset, else the unique new position, else dropped). |

## Tuning log (qwen2.5:7b on Ollama, 12-document corpus, 63 expectations)

The corpus is `tests/e2e/fixtures/spotcheck_corpus.json`: a wordy+duplicate paragraph, clean
Hemingway-style prose, corporate wordiness, a numbers-only paragraph, repeats, filler, a casual
lowercase post, a passive tangle, an academic sentence, Mann's real draft, a quoted line, and a
hedge-heavy note. Expectations name which sentences must be rephrased, removed, or left alone,
plus "no removes" and "at most N actions" guards against over-editing.

| Prompt | Change | Score | What still missed |
|---|---|---|---|
| v1 | keep / rephrase / remove, "prefer keep" | 8-sentence probe: missed the wordy sentence, removed a good one | model too eager to say "redundant" |
| v2 | two steps: list `flags` first, then `action` from the flags | 58/63 = 92 % | formal wordy phrases, partial repeats |
| v3 | + scanner HINTS, expanded flag definitions with examples | 60/63 = 95 % (two runs, identical) | a partial repeat, filler rephrased instead of removed, a hedge stack |
| v4 | + "may repeat sentence N" hints, hedge-stack flag | 60/63 and 61/63 = 95–97 % | filler rephrased (now a guard), a 5-word sentence rephrased (now a guard) |
| v5 (shipped) | + "do not rephrase a sentence under 8 words unless a HINT names it" | 60/63 = 95 % | same two, both caught by the guards |

Every miss in v4/v5 is handled deterministically: the guards turn a filler "rephrase" into a
remove and reject a rephrase of a very short unhinted sentence, and the length band rejects a
rewrite that lost most of the sentence ("Go back 42 days." for a 15-word sentence).

Per-document latency on this Mac: 2–8 s per batch of ≤ 8 sentences once the model is warm;
a 300-word document is ~3 batches.

## Deterministic fallback

A hinted wordy phrase gets a rewrite even when the model leaves the sentence alone (or when
no model is configured): a table of 30 unambiguous swaps ("in order to" → "to", "it is
important to note that" → deleted, "the reason for this is due to the fact that" → "this is
because", "utilization of" → "use of", …) rewrites the sentence, keeps the author's opening
case, and goes through the same guards. So the spot check is never empty-handed on the
phrases Grammarly's Clarity tab flags most.

## Round 2 (2026-10-15 AM): "telling me to remove instead of rephrase"

Mann's screenshot showed a 44-word sentence with a blue underline whose card struck the whole
sentence through and offered nothing. That card was Harper's long-sentence notice
("This sentence is 44 words long."), which carries no replacement, rendered by a UI that drew
every alert as `old → new`. Running the real pipeline on the same paragraphs showed the second
half of the problem: the model's "rephrases" were summaries ("…take you back, to exactly 41
days ago" → "Go back 41 days to understand why"; a 25-word sentence on hedging bets → 9 words
that kept 18 % of its content words) and its attempt at the 44-word sentence was a 14-word
précis that the length band rejected, so nothing useful survived.

What changed:

| Change | Where |
|---|---|
| **Long sentences get a split, not a summary.** The hint is now "too long (N words): split it…" at > 30 words; the prompt asks for two or three sentences at the natural joints; the guard accepts up to three pieces for an original of ≥ 24 words, each shorter than the original, within 55–135 % of its length. When the model keeps a > 34-word sentence or its split fails a guard, `plain_split` cuts the original at the joint nearest its middle (", and" dropped; ", but" → "But"; ", so" → "So"; ";"), capitalises the second half and keeps every word. Title "Split the long sentence". | `spotcheck.rs`: `LONG_WORDS`, `SPLIT_MIN_WORDS`, `MAX_SPLIT`, `SPLIT_FALLBACK_WORDS`, `plain_split`, `plain_verdict` |
| **Content guard.** A rephrase must keep the original's content words (light stemming; the scanner's wordy phrases and hedges excluded first): at most one lost word, or 40 % when that is more. Summaries fail with `ContentLost("9 of 12 content words lost")`. | `content_loss`, `MIN_CONTENT_KEPT` |
| **Unhinted rewrites are a light touch.** A sentence the scanner did not flag needs model confidence ≥ 4 and may shrink to no less than half its length. | `MIN_CONFIDENCE_UNHINTED`, the length-band floor |
| **Harper's note gives way.** "Improve readability" became "Split the long sentence" with the message "… Split it into two or three sentences, one point each."; `merge_visible` drops it when a spot rewrite covers the same span; the panel and the hover card show a note (no replacement) as plain text with Dismiss only, never as a strikethrough. | `engine.rs`, `merge_visible`, `Review.tsx`, `Underlines.tsx` |
| **The host runs the check.** The auto spot check moved from the Review view to the host loop: 3.5 s after typing pauses, for documents of 8–6000 words, whatever view the panel shows and when it is tucked. The button stays for "check now". | `host.rs` (`SPOT_DEBOUNCE`, `SPOT_MAX_WORDS`), `commands::spot_check_auto` |
| **Rewrites show as a word diff** when the sentence is longer than six words, in the panel and in the hover card. | `Review.tsx` `Diff`, `text.ts` `wordDiff` |
| **A pronoun "i" at the start may become "I"** in a rewrite; other lowercase starts are preserved as before. | `validate` |

Result on Mann's six paragraphs (local qwen2.5:7b, `tests/tuning/live_corpus.sh` style run):
one suggestion, the lossless split of the 44-word sentence; five model rewrites rejected
(two length-band summaries, two content losses, one casual-voice rewrite the scanner had not
asked for). Before the round the same text produced two summaries and no split.

Tooling for this round, all without app windows or the desktop:

* `cargo test live_text -- --ignored --nocapture` with `GRAMMARLAMA_LIVE_TEXT=<file>`
  runs the real pipeline on a file against the local model and prints every alert and
  rejection (`REJECTED LengthBand: <sentence> => <model text>`).
* `tests/tuning/live_corpus.sh [corpus] [model]` scores the whole corpus the same way, with
  the gate's evaluator; `tests/e2e/spotcheck.sh` still does it through the running app.
* `?win=panel&mock=doc` (a browser, no Tauri) renders the panel from a fixture
  (`ui/src/lib/mock.ts`); `&tab=clarity`, `&view=tone`, `&mock=nodoc|clean|spotting|onboarding`.
  Headless Chrome screenshots of it checked the new category header and the diff view.

The corpus grew to 14 documents: "script run-on" (a casual narrative with the 44-word
sentence, which must split while its neighbours stay) and "long but clear" (a 42-word chained
sentence that must split; a clear 33-word list sentence that may stay).

## Round 3 (2026-10-15 PM): "where's the button to fix this?"

Mann hovered the 44-word sentence in Notion while the model was still working through the
document (a long page: 87 alerts, several batches) and saw Harper's "Split the long sentence"
note with Dismiss only. The note had no rewrite, and the model's split had not arrived yet.

* `engine.rs`: the note now carries the lossless split from `plain_split` as its suggestion, so
  Accept is there the moment Harper flags the sentence, with no model involved. The message says
  "The split keeps every word. Accept it, or break the sentence where you prefer." When the model's
  own split lands, `merge_visible` hides the note under it as before. A sentence with no joint
  keeps the plain note.
* `plain_split`: a ", and " that closes a list ("milk, eggs, bread, and butter") is not a joint any
  more: the run since the previous comma must be at least three words. Unit test with a 41-word
  list sentence; the clause joint further out wins.
* The panel adds "Checking for a rewrite with the model." to a Clarity note while a check runs.
* `tests/e2e/host_textedit.sh`: the fixture gained a 47-word fourth sentence; the gate asserts its
  note carries ". But the sponsors" before the spot check is called.
* The product is now **GSpot** (bundle `GSpot.app`, same identifier `com.publik.grammarlama` so
  the Accessibility grant and settings carry over; crate, env vars and the publik slug unchanged).

## Gates (2026-10-15, qwen2.5:7b; the same numbers on the debug build and the signed bundle)

| Gate | Result |
|---|---|
| `cargo test` | 62 passed (18 on the spot check: splitter, hints, guards, plain rewrites, offsets, re-anchoring, no-model run, cache) |
| `tests/e2e/spotcheck.sh` | 13 passed; corpus 50/51 = 98 % in 51 s; two runs on the same text agree |
| `tests/e2e/features.sh` | 15 passed, incl. paraphrase/humanize sentence counts and the standalone spot check |
| `tests/e2e/host_textedit.sh` | 27 passed; on the live TextEdit document the wordy sentence got "Make it more concise", was underlined across its two wrapped lines, and Accept rewrote TextEdit to "I decided to send many emails to the founders to make progress." |
| `tests/e2e/setup.sh` | 17 passed: Ollama detected; publik `POST /installs` → key file (0600) → anonymous balance line → the $0.00 call answered `402` rendered as its message plus one publikhq.com link → "Later" → forget; a bad key refused; an Anthropic key probed → `claude-sonnet-5-5` picked from 14 models → ping answered → features run on it. |

* `cargo test`: unit tests on the splitter, hints, guards, offsets, re-anchoring, chunking.
* `tests/e2e/spotcheck.sh`: the corpus through the app (`/feature spot_check {"text"}`), one
  line per document plus the invariants (offsets round-trip, numbers survive, Clarity kind);
  the gate passes at ≥ 90 % corpus score.
* `tests/e2e/host_textedit.sh`: on a real TextEdit document the wordy sentence gets a
  sentence-level suggestion anchored to the live text, the underline is drawn, Accept rewrites
  TextEdit and the alert disappears.
* `tests/e2e/features.sh`: paraphrase and humanize keep ≥ 75 % of the sentence count; a
  one-sentence selection stays one sentence and keeps its numbers.
