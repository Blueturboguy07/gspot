# Changelog

## 0.2.4 (2026-11-20)

After you sign in with publik, the app opens at once. The app asks you to get a plan only when your usage is at zero.

- After sign-in, GSpot opens at once and uses your publik account for AI. The "Choose how to pay for AI" and "Connect your plan" window does not open at start.
- When your publik usage is at zero, the AI feature shows publik's message. Settings → AI shows the same message with one link to get a plan.
- An answer that you gave before ("Use my own key" or "Connect your plan") is kept. An own key that you pasted in Settings → AI stays as it is.
- Settings → AI still changes between publik API and your own key.

## 0.2.3 (2026-11-19)

No change in what the app does. This release proves the update from 0.2.2.

- The app does the same as in 0.2.2. The version number is new.
- `scripts/publik-release.sh <version>` makes the whole release in one command. See `docs/updates.md`.
- `live-update-proof.yml` installs a published release on clean macOS and Windows runners and checks the update from the live publik feed.

## 0.2.2 (2026-11-18)

Updates.

- GSpot updates itself. It checks at start and once a day. "Check for Updates…" in the tray menu checks at once.
- Every update is signed. GSpot checks the signature and the version before it installs anything.
- macOS: GSpot installs the update in the background and asks to restart. Windows: GSpot downloads the update and asks; the installer then runs with a progress bar only and opens GSpot again.
- `PUBLIK_NO_AUTO_UPDATE=1` turns off the automatic checks. See `docs/updates.md`.

## 0.2.1 (2026-11-10)

Sign in with publik.

- GSpot opens with "Sign in with publik". The button opens your browser, and the window continues when you are signed in. There is no way past it. Offline, it says "publik is not reachable. Check your connection." with a Retry button.
- After sign-in: "Choose how to pay for AI" (Get a plan, Use my own key), or "Connect your plan" (Connect your plan, Use my own key) when your account has a plan or a balance. The answer is kept. Settings → AI changes it later.
- Your own key works as before (OpenAI, Anthropic, OpenRouter, Groq, Google).
- Usage counts (`app_opened`, `active_session`) now carry this computer's publik key, so a signed-in person is counted once. `DO_NOT_TRACK=1` still turns them off.
- One publik module (`src-tauri/src/publik.rs`) replaces the old key provisioning and the vendored telemetry client.

## 0.2.0 (2026-11-08)

GSpot for Windows.

- Windows 10 and 11 (x64) installer, `GSpot_0.2.0_x64-setup.exe`. It installs for your account only, with no admin prompt. It is not code-signed yet, so SmartScreen asks once ("More info", then "Run anyway").
- On Windows, GSpot reads the text field you type in through UI Automation and needs no permission. Underlines, the hover card (Accept, Dismiss), the panel and the edge tab work as on macOS. The sidebar tools run the same code as on macOS; the Windows test covers spot check.
- Accept on Windows uses the field's own select-and-replace first in Win32 edit and rich-edit fields (Notepad, WordPad), so Ctrl+Z undoes it and the clipboard is untouched. Other fields, such as a browser text area, get a verified paste.
- Classic Notepad (Windows Server 2022 and 2025) has no UI Automation text pattern; GSpot measures its words with the edit control's own messages.
- Every classic console window is skipped, whatever program runs in it.
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
