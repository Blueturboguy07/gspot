# Changelog

## 0.2.0 (2026-11-08)

GSpot for Windows.

- Windows 10 and 11 (x64) installer, `GSpot_0.2.0_x64-setup.exe`. It installs for your account only, with no admin prompt. It is not code-signed yet, so SmartScreen asks once ("More info", then "Run anyway").
- On Windows, GSpot reads the text field you type in through UI Automation and needs no permission. Underlines, the hover card, Accept, the panel, the edge tab and every sidebar tool work as on macOS.
- Accept on Windows uses the field's own select-and-replace first (Notepad, WordPad, other standard edit fields), so Ctrl+Z undoes it and the clipboard is untouched. Browsers and Office get a verified paste.
- Terminals on Windows (Windows Terminal, cmd, PowerShell and others) are skipped, as on macOS.
- macOS: no change in behavior. The macOS code moved behind one platform interface (`platform.rs`) that both systems share.

## 0.1.1 (2026-11-06)

Same as 0.1.0, plus usage counts.

- Usage counts for publik: app opened, active session. No content is sent. Turn it off with `DO_NOT_TRACK=1`. See "Usage counts" in the README.

## 0.1.0 (2026-10-18)

First release.

- Harper correctness engine with in-place underlines in the text field you type in, in any app.
- Spot check, paraphraser, humanizer, tone, performance score, reader reactions, translate, explain it, AI rewriter, AI detector and AI chat.
- Plagiarism check with web search through publik API, your own Brave Search API key, or a keyless chain.
- publik API by default. An own key (OpenAI, Anthropic, OpenRouter, Groq, Google) is optional.
- Skipped apps list. Terminals are skipped by default.
- Signed with a Developer ID and notarized by Apple.
