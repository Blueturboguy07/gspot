# Grammarly Desktop (macOS) — reverse-engineering notes

Evidence gathered 2026-10-14 on this Mac from `/Applications/Grammarly Desktop.app` v1.200.0
and the public Chrome extension (id `kbfnbcaeplbcioakkpcpgfkobkghlhen`, v14.1337.0).
Tooling: `otool`, `nm` + `swift demangle`, `strings`, `sqlite3`, `lsof`, `assetutil`,
and REA 6.4.0 (`npx rea-agents`), run without Hopper/Ghidra.

## 1. Architecture (what the product actually is)

| Layer | Finding | Evidence |
|---|---|---|
| Host app | Native Swift (`com.grammarly.ProjectLlama`), universal arm64/x86_64, 263 MB. 39 internal frameworks (`GRLlamaCore`, `GRCoreApi`, `GRAccessibilityKit`, `GRAlertModels`, `GRDesignSystem`, `SDUI`, `QuillDelta`, `DiffMatchPatch`, `Sparkle`, `Sentry`, SwiftProtobuf, swift-crypto). | `otool -L`, framework tree, 148k symbols (37.8k Swift). |
| Panel UI | A **Tauri** (Rust + wry) helper `Superhuman Web UI.app` (`app_inkwell`, bundle `com.superhuman.web-client`) that renders a Vite/React bundle (`/assets/index-*.js`). Launched with `--transport=tls-ring --feature=plugin-proofreader` and talks to the host over a loopback TLS socket (127.0.0.1:53830/53831). | `strings app_inkwell` (tauri-runtime-wry, `src/connector/tcp_server.rs`), `lsof`. |
| Applets | JS "applets" fetched at runtime from `applet-bundles.grammarly.net/applets/assistant`, executed in JavaScriptCore with a `gos_message_bridge` RPC (`runtime.js`, avro-codegen ObjectBroker). | `Resources/runtime.js`, `GOSAppletManifest`, `GOSAppletRegistry`. |
| Server-driven UI | ~2,100 `SDUI.*` types: `ContentAssistantFeed`, `ContentGButtonPlagiarism/Tone/Proofit`, `ContentNativePerformanceScoreButton`, `ContentNativeToneInsightsModal`, `ActionUpgradeToPremium`… Cards are JSON from the server rendered natively. | `nm`/demangle. |
| Engine | **100 % server-side.** Text goes to `capi.grammarly.com` over WebSocket (`CapiSocketImpl`, `DeltaMessage`, `StartSessionMessage`, OT `CapiOTDocumentCheckManagerImpl`). LLM features are "cheetah" (`CheetahMessageStartSession/Chunk/SetVoiceProfile`) and `avi.grammarly.com`. Alerts arrive as `PushedContent*` (`RewriteCard`, `PlagiarismCard`, `AiDetector`, `AiGrader`, `CitationFinder`, `FactChecker`). | `GRCoreApi` types, URL cache (`capi.grammarly.com/api/configuration/*`, `gateway.grammarly.com/authorship/v1/user/<id>/settings`, `in.grammarly.com/v1/events`). |
| Text access | Accessibility API: `GRAccessibilityKit` (`GRAXElement`, `GRAXObserver`, `AccessibilityParameterizedAttributeKey`, `EventTapObserver`), `AXEditFieldObserver`, `EditFieldGeometryManager`, `HighlightRects`, `HighlightsProvider`, `DecorationsManager`. | `GRLlamaCore` types. |
| Replacement | A ladder: AX text operation (`AccessibilityTextOperation`), format-preserving HTML replace (`FormatPreservingReplaceSelectionStrategyHTML`), and **pasteboard patching** (`PasteboardPatchingService`, `PasteboardPatchingPipeline`) with per-app experiments (`llama_mac_hybrid_replacements_slack`, `..._outlook_legacy_v3`, `..._teams_web_v2`, `..._gmail_chrome_v2`). | types + 80 Statsig experiment names in prefs. |
| Corruption guard | `TextCorruptionDetector/Reporter`, `FormatCorruptionDetector` — they verify the host text after every replacement. | types. |
| Per-app rules | `KnownBundleIdentifier`, `Slack`, `Outlook`, `PowerPoint`, `Pages`, `ElectronJS`, `URLBlockList`, `WebDomainConfiguration`, `DlpConfig`. | types. |
| Design | Fonts Inter + "Matter for Grammarly" (proprietary). Dark theme tokens: base `#1c1c1c`, subdued `#2e2e2e`, brand `#2cc9b6`, correctness `#ff7a74`, clarity `#7d99f0`, engagement `#2cc9b6`, delivery `#a598f0`, plagiarism `#80f3f3`, premium `#ffbf47`. 270 asset-catalog images (`G2Assistant*`, `GButton*`, `Authorship*`, `AIDetector*`). | `Resources/Fonts`, `designSystem.styles.css`, `assetutil`. |

## 2. Feature inventory (from the UI copy inside the extension bundles)

* **Review suggestions** — tabs Correctness / Clarity / Engagement / Delivery; cards "Correct your spelling", "Add a comma", "Capitalize the word", "Punctuation problem", with Accept / Dismiss / ….
* **Performance** — "Overall score … of 100", "The score represents the quality of this text. Increase it by addressing issues.", needs ≥30 words; outcome labels: clarity `Unclear / A bit unclear / Mostly clear / Clear`, engagement `Bland / A bit bland / Engaging / Very engaging`, delivery `Very off / Slightly off / Just right`, correctness `N suggestions / Looking good`.
* **Tone insights** — "We couldn't detect your tone yet. Add more text to get tone insights."
* **Paraphraser** — "Change writing style": Humanize, Academic, Professional, Streamlined, Creative, Simple (+ Expand/Shorten).
* **Humanizer** — "Create a voice…", presets The Naturalist / The Precisionist / The Everyday Voice / The Executive / The Scholar; states "Analyzing your document", "Creating your voice", "Describe a style".
* **Reader Reactions** — Audience picker ("Tailor the feedback to who you're writing for", "Create audience"), minimum 25 words, "Reader will remember" (3), "Reader will ask" (5 questions with category labels).
* **Translate** — target-language dropdown, "Ready to translate. Select any text or translate the full page in one click.", "Translate all"; idiom cards with Meaning / Origin / Example.
* **Explain It** — "Spot something confusing? Highlight text for a quick explainer, or explain the whole page in one click.", TERM/definition cards.
* **AI Rewriter** (rebrand "AI Humanizer") — steps "Scanning your text / Checking common AI phrases / Generating report"; buckets "Almost always AI (over 50× frequency) / Usually AI (10–49×) / Sometimes AI (<10×)"; min 8 words.
* **AI Detector** — "You're a true original. Ensure your work is, too."; result copy "Some parts of your text contain patterns that resemble AI text…", "Possible AI text detected"; min 8 words.
* **Plagiarism** — "Check for plagiarism before you submit": Catch missing citations / Detect text matches / Create citations quickly; citation styles APA/MLA/Chicago.
* **AI Chat** — "{Name}, how can I help?", chips "Summarize this page", "Review N suggestions", context chip = current app, "Ask AI", "+" attach.
* **History** (clock icon), avatar initials, G-button pill anchored to the focused field with the alert count.

## 3. What REA could and could not do here

Worked without any decompiler: `inspect-macho`, `inspect-native-dispatch-metadata` (82 ObjC/Swift class records for `GRCoreApi` with file offsets), `demangle-swift`, `inspect-plist`, `analyze-javascript-application` on the extension's inkwell bundle (160 files, 1,798 findings, a 24k-node semantic graph, 50 MB of evidence JSON).

Not possible: the engine (it is a remote service), the Tauri UI bundle inside `app_inkwell` (brotli-embedded; the same code ships readable in the Chrome extension instead), function-level pseudocode (needs Hopper/Ghidra/IDA; Hopper's trial forbids automation).

Verdict: REA replaces *guessing about structure* (names, state machines, protocol shapes, copy, tokens). It does not replace the open-source Frankenstein build, because nothing that does the work lives on the laptop.
