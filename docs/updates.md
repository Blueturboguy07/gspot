# Updates

GSpot updates itself with `tauri-plugin-updater`. Every update package is signed with the publik
update key. The app checks each package against the public key in `src-tauri/tauri.conf.json`
before it installs anything.

## How it works

- The app reads `https://publikhq.com/api/updates/gspot/latest.json`. publik serves the
  `latest.json` asset of the scan-approved release tag, byte for byte. publik does not sign or
  change it, so the release `latest.json` must be complete and correct. The route answers 404
  until a scan-approved release carries a `latest.json` asset. A 404 is silent in the background
  check, and a manual check shows it as an error.
- The app checks 10 s after launch and then once a day. "Check for Updates…" in the tray menu
  checks at once. `PUBLIK_NO_AUTO_UPDATE=1` turns off the automatic checks (the CI workflows set it).
- macOS: the app downloads the new `GSpot.app`, verifies it, puts it in place of the old one,
  and asks "Restart" or "Later". If the user cannot write to the app's folder, the app does not
  replace it in the background. It asks "Install and Restart" or "Later", and macOS asks for an
  admin password only after "Install and Restart".
- Windows: the app downloads the installer, verifies it, and asks "Restart and Install" or
  "Later". The installer runs with a progress bar only, then opens GSpot again.
- The signature must also name the version that `latest.json` announces
  (`requireSignedVersion`). The app never installs an older version.
- A failed automatic check goes to the log only. A failed manual check shows the error.
- Code: `src-tauri/src/updates.rs` (unit test of the daily schedule on paused time). Proof:
  `.github/workflows/updater-proof.yml` (test builds, a local feed) and
  `.github/workflows/live-update-proof.yml` (a published release, the live feed).

## The key

| Where | What |
|---|---|
| Release Mac, login keychain, service `publik-update-key` | `mann-private` (private key), `mann-password` (its password), `mann-public` (public key) |
| GitHub secrets on this repo | `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (used by `windows.yml` and `updater-proof.yml`) |
| `src-tauri/tauri.conf.json` | `plugins.updater.pubkey` (public, key ID `F5E0515C77912BD0`) |

Keep a backup of the private key and its password. Without them, installed copies can never
update again.

## Cut a release

One command on the release Mac does the whole release, for macOS arm64 and Windows x64:

1. Set the new version in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`,
   `src-tauri/Cargo.lock` (the `grammarlama` entry) and `ui/package.json`. Add a `## <v>` section
   to `CHANGELOG.md`; its first line becomes the feed's note. Commit and push to `main`.
2. Run:
   ```sh
   scripts/publik-release.sh <v>
   ```

The script (`scripts/publik-release.sh`):

| Step | What |
|---|---|
| Checks | On `main`, clean tree, pushed. The version in every file above. Higher than the last release. Tag `v<v>` free. Developer ID identity, notary profile `publik`, update key in the keychain. 1.5 GB free. |
| CI | Runs `publik-build.yml` (unsigned app) and `windows.yml` (installer and its `.sig`, unit tests, Windows e2e) on the commit and waits. A failed `windows.yml` test job is re-run once (the Edge test is flaky); the build job must pass. |
| Mac | `scripts/sign-ci-macos.sh`: signs the app (inner code first, hardened runtime, `src-tauri/Entitlements.plist`) with `Developer ID Application: Mann Bellani (R5R3ZS54LV)`, notarizes and staples it, packs and signs the update bundle from the stapled app, builds the dmg, signs, notarizes and staples it. |
| Feed | `latest.json` with absolute GitHub URLs (`scripts/updater-latest-json.mjs`), signed with the update key as `latest.json.sig`. `SHA256SUMS.txt`. |
| Draft | Uploads the 8 files to a draft release `v<v>` and runs `mac-smoke.yml` on it. |
| Verify | Downloads the draft back: the file list, the checksums, the feed and every signature with the app's public key (and that key is the last release's), the versions in Info.plist and in the Windows exe, `codesign`, `spctl` ("Notarized Developer ID"), `stapler`, the designated requirement against the last release, the publik app token (`~/.publik/gspot-app-token`) in the binary as in the last release. |
| Publish | Only when every check passed. Then the tag and every feed URL are checked. |

Run the same command again after a failure. It reuses the CI runs of the commit, the files in
the work folder (`.release/v<v>`, git-ignored, deleted after the publish) and the draft. For a
published release it runs the verify only. It starts no app and prints no secret: the update key
goes from the keychain into the signer's environment only. `PUBLIK_RELEASE_NO_PUBLISH=1` stops
after the verify and leaves the draft.

publik takes new releases in once a day at 10:00 UTC. Publish before 09:30 UTC to be in that
day's intake. Then run the live proof (below) from the old version to the new one.

The steps by hand, if the script cannot run: `gh run download <publik-build run> -n
GSpot-macos-arm64-unsigned -D in`, `scripts/sign-ci-macos.sh in/GSpot-unsigned-aarch64.app.zip out`,
`gh run download <windows run> -n GSpot-windows-x64 -D out`, then `scripts/updater-latest-json.mjs`
as in the script, a draft release with the 8 files, `mac-smoke.yml` on the draft, publish.

`publik-build.yml` needs no update key. It makes the unsigned app only, and an update bundle of
that app must never ship. The key is needed where the final files are made: the Windows build
(repo secret) and the release Mac (keychain). A pipeline that takes over either step needs
`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and on macOS must run
`scripts/updater-pack-macos.sh` after stapling.

`tauri build` without the key fails when `createUpdaterArtifacts` is on. For a local build
without the key, add `--config '{"bundle":{"createUpdaterArtifacts":false}}'`.

## Roll back

The app installs only a version higher than its own. You cannot push an older version.

1. Stop the bad release at once: withdraw it in publik, or delete its `latest.json` asset.
   Copies that did not update yet stay on the old version.
2. Revert the bad change, give it a higher version (for example 0.2.4 after a bad 0.2.3), and cut
   a release. Copies on the bad version update to it.

Do not turn on `allowDowngrades`. It removes the only guard against an old, signed release
served as a new one.

## Rotate the key

The public key is compiled into each copy, so a new key reaches users only through an update.

1. Make a new key: `ui/node_modules/.bin/tauri signer generate -w new.key`. Store the private
   key, its password and the public key in the keychain items and the repo secrets (new names
   first, so the old key stays available).
2. Put the new public key in `tauri.conf.json`. Release this version signed with the OLD key.
3. From the next release on, sign with the new key. Copies that skipped the bridge release can
   still update only to a release signed with the old key, so keep the old key until you accept
   that those copies stop updating.
4. If the private key leaks, cut the bridge release at once. A leaked key alone cannot push an
   update: the attacker must also control the feed on publikhq.com.

## Test

### Live proof (a published release, the live feed)

Run `live-update-proof.yml` (workflow_dispatch) with `from_tag` (the release to install first)
and `expect_version`. On `macos-latest` and `windows-latest` it downloads `from_tag` from GitHub,
checks it, installs it as a user does and starts it with no test hook and no feed override, so
the app asks `https://publikhq.com/api/updates/gspot/latest.json`.

- `expect_version` = the version of `from_tag`: the app must find no update. Today's harness check.
- `expect_version` higher (after publik serves it): the app must install it by itself and ask.
  The proof answers "Restart" (the app must run the new version) and, from a fresh install, "Later"
  (the old process keeps running; the menu offers the update again).
- macOS: before the update it writes real TCC grants (camera, microphone, screen capture,
  accessibility) for `com.publik.grammarlama` with the csreq of the designated requirement; after
  it, every stored csreq must still be satisfied by the installed app (the check tccd makes) and
  the designated requirement must be unchanged. The app stays on its sign-in window there, so the
  accessibility grant is never used.
- Windows: "Check for Updates…" is clicked in the tray menu and its answer is read.
- Sign-in goes to a local stand-in (`tests/live-proof/fake-publik.py`), so a run never writes to
  publik's database. The update feed is the real one.

Code: `tests/live-proof/`. Logs, screenshots and the results table: the `live-proof-*` artifacts.

### Proof with test builds

Run `updater-proof.yml` (workflow_dispatch). It builds 0.0.1 and 0.0.2, installs 0.0.1 on
`macos-latest` and `windows-latest`, and checks that 0.0.1 refuses three bad feeds and then
updates to 0.0.2. Then it starts 0.0.1 once more and checks that the restart question is on
screen while the app keeps running.

The app has a test hook for this: `PUBLIK_UPDATE_SELF_TEST=version|check|install` runs the
updater with no window and no prompt and prints one JSON line. `prompt` runs the background
path and shows the real question. The hook does not click the buttons.

## Copy to another Tauri 2 app

1. `cargo add tauri-plugin-updater@2 tauri-plugin-dialog@2` in `src-tauri`.
2. Copy `src-tauri/src/updates.rs`. Change `APP_NAME`. Add `pub mod updates;`.
3. In `run()`: wrap the builder with `updates::plugins(...)`. In `setup`, first run the self-test
   (`updates::self_test_mode()` / `updates::run_self_test`), then call `updates::start`.
4. Add a "Check for Updates…" menu item that calls `updates::check_now`.
5. `build.rs`: `println!("cargo:rerun-if-env-changed=PUBLIK_UPDATE_ENDPOINT");`.
6. `tauri.conf.json`: `bundle.createUpdaterArtifacts: true`, and `plugins.updater` with the
   `pubkey`, the endpoint `https://publikhq.com/api/updates/<slug>/latest.json`,
   `requireSignedVersion: true` and `windows.installMode: "passive"`.
7. Copy `scripts/updater-pack-macos.sh` and `scripts/updater-latest-json.mjs`. Call the pack
   script after stapling in the macOS release script.
8. Set the two repo secrets from the keychain; pass them to the Windows build step.
9. Turn off updater artifacts in any build whose output is re-signed later.
10. Copy `updater-proof.yml` and `tests/updater/`. Change the app name, bundle paths and binary
    name. Run it.
