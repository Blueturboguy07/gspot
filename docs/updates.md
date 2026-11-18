# Updates

GSpot updates itself with `tauri-plugin-updater`. Every update package is signed with the publik
update key. The app checks each package against the public key in `src-tauri/tauri.conf.json`
before it installs anything.

## How it works

- The app reads `https://publikhq.com/api/updates/gspot/latest.json`. publik serves the
  `latest.json` asset of the scan-approved release tag, byte for byte. publik does not sign or
  change it, so the release `latest.json` must be complete and correct.
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
- Code: `src-tauri/src/updates.rs`. Proof: `.github/workflows/updater-proof.yml`.

## The key

| Where | What |
|---|---|
| Release Mac, login keychain, service `publik-update-key` | `mann-private` (private key), `mann-password` (its password), `mann-public` (public key) |
| GitHub secrets on this repo | `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (used by `windows.yml` and `updater-proof.yml`) |
| `src-tauri/tauri.conf.json` | `plugins.updater.pubkey` (public, key ID `F5E0515C77912BD0`) |

Keep a backup of the private key and its password. Without them, installed copies can never
update again.

## Cut a release

1. Set the new version in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and
   `ui/package.json`. Add it to `CHANGELOG.md`. Push.
2. Run `windows.yml` on that commit. Its `GSpot-windows-x64` artifact holds
   `GSpot_<v>_x64-setup.exe` and `GSpot_<v>_x64-setup.exe.sig`.
3. Run `publik-build.yml` on that commit. Its artifact holds the unsigned app.
4. On the release Mac, sign, notarize and pack:
   ```sh
   gh run download <publik-build run> -n GSpot-macos-arm64-unsigned -D in
   scripts/sign-ci-macos.sh in/GSpot-unsigned-aarch64.app.zip out
   gh run download <windows run> -n GSpot-windows-x64 -D out
   ```
   `out/` now has the dmg, `GSpot_<v>_aarch64.app.tar.gz` and its `.sig`, and the Windows files.
   The `.app.tar.gz` is the stapled Developer ID app, so an update keeps the TCC permissions.
5. Write the feed. The script refuses a signature that does not verify or names another version:
   ```sh
   node scripts/updater-latest-json.mjs --version <v> \
     --base-url https://github.com/Blueturboguy07/gspot/releases/download/v<v>/ \
     --platform darwin-aarch64=out/GSpot_<v>_aarch64.app.tar.gz \
     --platform windows-x86_64=out/GSpot_<v>_x64-setup.exe \
     --notes-file notes.md --out out/latest.json
   (cd out && shasum -a 256 GSpot_<v>_aarch64.dmg GSpot_<v>_x64-setup.exe GSpot_<v>_aarch64.app.tar.gz > SHA256SUMS.txt)
   ```
6. Make a draft release with all the files:
   ```sh
   gh release create v<v> --draft --title "GSpot <v>" --notes-file notes.md \
     out/GSpot_<v>_aarch64.dmg out/GSpot_<v>_x64-setup.exe \
     out/GSpot_<v>_aarch64.app.tar.gz out/GSpot_<v>_aarch64.app.tar.gz.sig \
     out/GSpot_<v>_x64-setup.exe.sig out/latest.json out/SHA256SUMS.txt
   ```
7. Run `mac-smoke.yml` with the draft tag. Publish the release. Installed copies update when
   publik approves the release and serves its `latest.json`.

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

Run `updater-proof.yml` (workflow_dispatch). It builds 0.0.1 and 0.0.2, installs 0.0.1 on
`macos-latest` and `windows-latest`, and checks that 0.0.1 refuses three bad feeds and then
updates to 0.0.2. Then it starts 0.0.1 once more and checks that the restart question is on
screen while the app keeps running. A push to `feat/updater` or `updater/**` also starts it.

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
