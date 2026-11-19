#!/usr/bin/env bash
# The whole GSpot release in one command, on the release Mac (docs/updates.md, "Cut a release"):
#
#   scripts/publik-release.sh <version>
#
# Before it: put <version> in src-tauri/tauri.conf.json, src-tauri/Cargo.toml, src-tauri/Cargo.lock
# and ui/package.json, add a "## <version>" section to CHANGELOG.md, commit, push to main.
#
# What it does, for macOS arm64 and Windows x64:
#  1. Checks: on main, clean tree, pushed; the version in every file; higher than the last
#     release; the tag is free; the Developer ID, the notary profile and the update key are there.
#  2. Runs publik-build.yml (unsigned Mac app) and windows.yml (installer + its updater .sig, unit
#     tests, e2e) on that commit and waits. A failed windows.yml test job is re-run once (its Edge
#     test is known to be flaky); the build job itself must pass.
#  3. Downloads the unsigned app and the installer (checked against the CI checksum).
#  4. scripts/sign-ci-macos.sh: signs the app (inner code first, hardened runtime,
#     src-tauri/Entitlements.plist) with the Developer ID, notarizes and staples it, packs and signs
#     the update bundle from the stapled app, builds the dmg, signs, notarizes and staples it.
#  5. Writes latest.json (absolute GitHub URLs) and signs it with the update key (latest.json.sig).
#  6. Uploads everything to a DRAFT release, and runs mac-smoke.yml on the draft.
#  7. Downloads the draft back and verifies it: asset list, checksums, the feed and every
#     signature with the app's public key, versions (Info.plist, exe), codesign, spctl, stapler,
#     the designated requirement against the last release, the publik app token.
#  8. Publishes, then checks the tag and every feed URL.
#
# Run the same command again after a failure: it reuses the CI runs of the commit, the files in
# the work folder and the draft, and goes on from there. For a published release it only runs
# the checks of step 7. It stops before publishing on any failed check (the draft stays), starts
# no app, and prints no secret: the update key goes from the keychain into the signer's
# environment only.
#
# Env: PUBLIK_RELEASE_WORK  work folder (default <repo>/.release/v<version>, git-ignored);
#                           deleted after a successful publish
#      APPLE_SIGNING_IDENTITY, NOTARY_PROFILE (default "publik"), TAURI_CLI
#      UPDATE_KEY_SERVICE   keychain service of the update key (default "publik-update-key",
#                           accounts mann-private and mann-password)
#      PUBLIK_RELEASE_NO_PUBLISH=1  stop after the verify and leave the draft
set -euo pipefail

V="${1:?usage: scripts/publik-release.sh <version>}"
V="${V#v}"
TAG="v$V"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

APP=GSpot
BUNDLE_ID=com.publik.grammarlama
BRANCH=main
IDENTITY="${APPLE_SIGNING_IDENTITY:-Developer ID Application: Mann Bellani (R5R3ZS54LV)}"
TEAM=R5R3ZS54LV
PROFILE="${NOTARY_PROFILE:-publik}"
SERVICE="${UPDATE_KEY_SERVICE:-publik-update-key}"
TOKEN_FILE="$HOME/.publik/gspot-app-token"
WORK="${PUBLIK_RELEASE_WORK:-$ROOT/.release/$TAG}"
DMG="${APP}_${V}_aarch64.dmg"
EXE="${APP}_${V}_x64-setup.exe"
TGZ="${APP}_${V}_aarch64.app.tar.gz"
ASSETS=("$DMG" "$EXE" "$TGZ" "$TGZ.sig" "$EXE.sig" latest.json latest.json.sig SHA256SUMS.txt)

say() { printf '\n==> %s\n' "$*"; }
ok() { printf '  ok    %s\n' "$*"; }
note() { printf '  note  %s\n' "$*"; }
die() { printf '\nSTOP: %s\n' "$*" >&2; exit 1; }
FAILS=0
bad() { printf '  FAIL  %s\n' "$*"; FAILS=$((FAILS + 1)); }
# expect <description> <command...>: ok when the command succeeds, FAIL otherwise.
expect() { local what="$1"; shift; if "$@"; then ok "$what"; else bad "$what"; fi; }
plist() { /usr/libexec/PlistBuddy -c "Print :$2" "$1/Contents/Info.plist"; }
sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
dr() { codesign -d -r- "$1" 2>&1 | sed -n 's/^designated => //p'; }
stapled() { xcrun stapler validate "$1" >/dev/null 2>&1; }

[[ "$V" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "the version must be x.y.z, not $V"
for t in gh git node npm xcrun codesign hdiutil security shasum spctl ditto tar unzip file curl; do
  command -v "$t" >/dev/null || die "$t is not installed"
done
gh auth status >/dev/null 2>&1 || die "gh is not logged in"
mkdir -p "$WORK"
# gh for reads and downloads, with retries: the GitHub API limit (5000 calls an hour) is per user
# and other jobs of the same user share it; a 5xx or a dropped connection is retried too.
ghr() {
  local out err i
  for i in $(seq 1 45); do
    if out="$(gh "$@" 2> "$WORK/gh.err")"; then
      [ -z "$out" ] || printf '%s\n' "$out"
      return 0
    fi
    err="$(tr '\n' ' ' < "$WORK/gh.err")"
    case "$err" in
      *"rate limit"* | *"HTTP 5"* | *"timed out"* | *"timeout"* | *"connection"* | *"EOF"* | *"TLS"*) ;;
      *) printf '%s\n' "$err" >&2; return 1 ;;
    esac
    echo "    GitHub API: ${err:0:110}... waiting 60 s (try $i of 45)" >&2
    sleep 60
  done
  printf '%s\n' "$err" >&2
  return 1
}
REPO="$(ghr repo view --json nameWithOwner -q .nameWithOwner)"
BASE_URL="https://github.com/$REPO/releases/download/$TAG/"
STATE="$WORK/state"
touch "$STATE"
get_state() { sed -n "s/^$1=//p" "$STATE" | tail -1; }
set_state() { echo "$1=$2" >> "$STATE"; }

# ---------------------------------------------------------------------------------------------
say "$APP $V: checks"
RELEASE="$(ghr release view "$TAG" -R "$REPO" --json isDraft,targetCommitish 2>/dev/null || true)"
if [ -z "$RELEASE" ]; then RSTATE=none
elif [ "$(node -p 'JSON.parse(process.argv[1]).isDraft' "$RELEASE")" = true ]; then RSTATE=draft
else RSTATE=published; fi
ok "release $TAG: $RSTATE"
PREV_TAG="$(ghr release list -R "$REPO" --exclude-drafts --exclude-pre-releases -L 20 --json tagName -q '.[].tagName' | grep -vx "$TAG" | head -1)"
[ -n "$PREV_TAG" ] || die "no earlier published release"
ok "last published release: $PREV_TAG"
git fetch -q origin "$BRANCH" --tags

if [ "$RSTATE" = published ]; then
  SHA="$(git rev-parse "$TAG^{commit}")"
  note "$TAG is already published: verifying it only"
else
  [ "$(git rev-parse --abbrev-ref HEAD)" = "$BRANCH" ] || die "check out $BRANCH first"
  [ -z "$(git status --porcelain)" ] || die "the working tree is not clean"
  SHA="$(git rev-parse HEAD)"
  [ "$SHA" = "$(git rev-parse "origin/$BRANCH")" ] || die "HEAD is not origin/$BRANCH: push (or pull) first"
  ok "on $BRANCH at ${SHA:0:12}, clean, pushed"
  if git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1; then die "the tag $TAG exists but has no published release"; fi
  ok "tag $TAG is free"
  if [ "$RSTATE" = draft ]; then
    TARGET="$(node -p 'JSON.parse(process.argv[1]).targetCommitish' "$RELEASE")"
    [ "$TARGET" = "$SHA" ] || die "the draft $TAG is for commit $TARGET, not HEAD $SHA: delete the draft or check out that commit"
    ok "resuming the draft $TAG"
  fi
  # The version in every file a bump touches.
  CONF_V="$(node -p 'require("./src-tauri/tauri.conf.json").version')"
  UI_V="$(node -p 'require("./ui/package.json").version')"
  CARGO_V="$(sed -n 's/^version = "\(.*\)"$/\1/p' src-tauri/Cargo.toml | head -1)"
  LOCK_V="$(awk '/^name = "grammarlama"$/ { getline; gsub(/version = |"/, ""); print; exit }' src-tauri/Cargo.lock)"
  for pair in "tauri.conf.json:$CONF_V" "ui/package.json:$UI_V" "Cargo.toml:$CARGO_V" "Cargo.lock:$LOCK_V"; do
    [ "${pair#*:}" = "$V" ] || die "${pair%%:*} has version ${pair#*:}, not $V"
  done
  grep -Eq "^## $V( |$)" CHANGELOG.md || die "CHANGELOG.md has no \"## $V\" section"
  ok "version $V in tauri.conf.json, Cargo.toml, Cargo.lock, ui/package.json and CHANGELOG.md"
  [ "$(printf '%s\n%s\n' "${PREV_TAG#v}" "$V" | sort -V | tail -1)" = "$V" ] && [ "${PREV_TAG#v}" != "$V" ] ||
    die "$V is not higher than the last release ${PREV_TAG#v}"
  ok "$V is higher than ${PREV_TAG#v}"
  security find-identity -v -p codesigning | grep -qF "\"$IDENTITY\"" || die "no signing identity \"$IDENTITY\" in the keychain"
  ok "signing identity: $IDENTITY"
  security find-generic-password -s "$SERVICE" -a mann-private >/dev/null 2>&1 || die "no update key: keychain service $SERVICE, account mann-private"
  ok "update key: keychain item $SERVICE (not read yet)"
  xcrun notarytool history --keychain-profile "$PROFILE" >/dev/null 2>&1 || die "the notary profile \"$PROFILE\" does not work (xcrun notarytool store-credentials)"
  ok "notary profile: $PROFILE"
fi
FREE_KB="$(df -k "$WORK" | awk 'NR == 2 { print $4 }')"
[ "$FREE_KB" -ge 1500000 ] || die "only $((FREE_KB / 1024)) MB free on the disk of $WORK; the release needs 1.5 GB"
ok "disk: $((FREE_KB / 1024)) MB free"
[ -s "$TOKEN_FILE" ] || die "no $TOKEN_FILE: the app token check needs it"

# The Tauri CLI signs the update files: the repo's, else the locked version in the work folder.
TAURI="${TAURI_CLI:-}"
if [ "$RSTATE" != published ]; then
  for c in "$ROOT/node_modules/.bin/tauri" "$ROOT/ui/node_modules/.bin/tauri" "$WORK/tauri-cli/node_modules/.bin/tauri"; do
    [ -z "$TAURI" ] && [ -x "$c" ] && TAURI="$c"
  done
  if [ -z "$TAURI" ]; then
    CLI_V="$(awk "/^      '@tauri-apps\/cli':\$/ { f = 1; next } f && /version:/ { print \$2; exit }" ui/pnpm-lock.yaml)"
    [ -n "$CLI_V" ] || die "no @tauri-apps/cli version in ui/pnpm-lock.yaml"
    npm install --silent --no-save --no-package-lock --no-audit --no-fund --prefix "$WORK/tauri-cli" "@tauri-apps/cli@$CLI_V" >/dev/null
    TAURI="$WORK/tauri-cli/node_modules/.bin/tauri"
  fi
  ok "Tauri CLI $("$TAURI" --version | awk '{ print $NF }')"
  export TAURI_CLI="$TAURI"
fi

# Runs a command with the update key in its environment. The key is never printed.
with_update_key() {
  if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then "$@"; return; fi
  local key pw
  key="$(security find-generic-password -s "$SERVICE" -a mann-private -w 2>/dev/null)" || die "cannot read the update key"
  pw="$(security find-generic-password -s "$SERVICE" -a mann-password -w 2>/dev/null || true)"
  TAURI_SIGNING_PRIVATE_KEY="$key" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$pw" "$@"
}

# ---------------------------------------------------------------------------------------------
run_field() { ghr run view "$1" -R "$REPO" --json "$2" -q ".$2"; }
# The newest workflow_dispatch run of a workflow on $SHA, or nothing.
last_run() {
  ghr run list -R "$REPO" --workflow "$1" --commit "$SHA" --event workflow_dispatch -L 20 \
    --json databaseId,createdAt -q 'sort_by(.createdAt) | reverse | .[0].databaseId // empty'
}
# Starts a workflow on $BRANCH (the extra arguments are -f inputs) and prints the new run's id.
start_run() {
  local wf="$1" before id
  shift
  before="$(ghr run list -R "$REPO" --workflow "$wf" -L 50 --json databaseId -q '.[].databaseId' | sort)"
  gh workflow run "$wf" -R "$REPO" --ref "$BRANCH" "$@" >/dev/null
  for _ in $(seq 1 40); do
    sleep 5
    id="$(ghr run list -R "$REPO" --workflow "$wf" --commit "$SHA" --event workflow_dispatch -L 20 --json databaseId -q '.[].databaseId' |
      sort | comm -13 <(printf '%s\n' "$before") - | tail -1)"
    [ -n "$id" ] && { echo "$id"; return; }
  done
  die "$wf did not start"
}
wait_run() {
  local id="$1" t=0 status
  while :; do
    status="$(run_field "$id" status)"
    [ "$status" = completed ] && break
    [ $((t % 300)) -eq 0 ] && echo "    run $id: $status ($((t / 60)) min)"
    sleep 30; t=$((t + 30))
    [ "$t" -lt 9000 ] || die "run $id did not finish in 150 minutes"
  done
}
# windows.yml may re-run its failed jobs once when its build job passed (the Edge e2e is flaky).
can_rerun() {
  [ "$1" = windows.yml ] && [ "$(get_state "rerun_$2")" != "done" ] &&
    [ "$(ghr run view "$2" -R "$REPO" --json jobs -q '.jobs[] | select(.name == "Build the Windows installer") | .conclusion')" = success ]
}
rerun_failed() {
  echo "    $1 run $2: a test job failed; re-running the failed jobs once" >&2
  set_state "rerun_$2" "done"
  gh run rerun "$2" -R "$REPO" --failed >/dev/null
  sleep 15
}
# A run of the workflow on $SHA that is successful, running, or worth a re-run; else a new one.
ensure_run() {
  local wf="$1" id
  id="$(last_run "$wf")"
  if [ -n "$id" ] && [ "$(run_field "$id" status)" = completed ] && [ "$(run_field "$id" conclusion)" != success ]; then
    if can_rerun "$wf" "$id"; then rerun_failed "$wf" "$id"; else id=""; fi
  fi
  if [ -z "$id" ]; then
    id="$(start_run "$wf")"
    echo "    started $wf: https://github.com/$REPO/actions/runs/$id" >&2
  fi
  echo "$id"
}
# Waits for the run; it must end in success (after at most one re-run of failed jobs).
finish_run() {
  local wf="$1" id="$2" conclusion
  wait_run "$id"
  conclusion="$(run_field "$id" conclusion)"
  if [ "$conclusion" != success ] && can_rerun "$wf" "$id"; then
    rerun_failed "$wf" "$id"
    wait_run "$id"
    conclusion="$(run_field "$id" conclusion)"
  fi
  [ "$conclusion" = success ] || die "$wf run $id ended with $conclusion: https://github.com/$REPO/actions/runs/$id"
}

IN="$WORK/in"
OUT="$WORK/out"
mkdir -p "$IN" "$OUT"
have_all_assets() {
  local names
  names="$(ghr release view "$TAG" -R "$REPO" --json assets -q '.assets[] | select(.state == "uploaded") | .name' 2>/dev/null | sort)"
  [ "$names" = "$(printf '%s\n' "${ASSETS[@]}" | sort)" ]
}

if [ "$RSTATE" != published ]; then
  say "CI builds of ${SHA:0:12}"
  WIN_RUN="$(ensure_run windows.yml)"
  MAC_RUN="$(ensure_run publik-build.yml)"
  finish_run publik-build.yml "$MAC_RUN"
  ok "publik-build.yml: https://github.com/$REPO/actions/runs/$MAC_RUN"
  finish_run windows.yml "$WIN_RUN"
  ok "windows.yml: https://github.com/$REPO/actions/runs/$WIN_RUN"
  set_state windows_run "$WIN_RUN"
  set_state mac_run "$MAC_RUN"

  if [ "$RSTATE" = draft ] && have_all_assets && [ ! -f "$OUT/$DMG" ]; then
    note "the draft has every file and the work folder has none: verifying the draft"
  else
    say "Download the CI builds"
    [ -f "$OUT/SHA256SUMS-windows.txt" ] || ghr run download "$WIN_RUN" -R "$REPO" -n GSpot-windows-x64 -D "$OUT"
    (cd "$OUT" && shasum -a 256 -c SHA256SUMS-windows.txt >/dev/null) || die "the Windows installer does not match the CI checksum"
    [ -f "$OUT/$EXE" ] && [ -f "$OUT/$EXE.sig" ] || die "the windows.yml artifact has no $EXE and $EXE.sig"
    ok "$EXE and its .sig (checksum as built by CI)"
    [ -f "$IN/GSpot-unsigned-aarch64.app.zip" ] || ghr run download "$MAC_RUN" -R "$REPO" -n GSpot-macos-arm64-unsigned -D "$IN"
    unzip -p "$IN/GSpot-unsigned-aarch64.app.zip" GSpot.app/Contents/Info.plist > "$IN/Info.plist"
    [ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$IN/Info.plist")" = "$V" ] || die "the CI app is not version $V"
    ok "GSpot-unsigned-aarch64.app.zip (version $V)"

    say "Sign, notarize and staple the Mac app; update bundle; dmg"
    if [ -f "$OUT/$DMG" ] && [ -f "$OUT/$TGZ" ] && [ -f "$OUT/$TGZ.sig" ]; then
      note "already done in $OUT"
    else
      rm -f "$OUT/$DMG" "$OUT/$TGZ" "$OUT/$TGZ.sig" "$OUT/latest.json" "$OUT/latest.json.sig"
      with_update_key "$ROOT/scripts/sign-ci-macos.sh" "$IN/GSpot-unsigned-aarch64.app.zip" "$OUT"
    fi

    say "Feed and checksums"
    awk -v v="$V" '$0 ~ "^## " v "( |$)" { f = 1; next } /^## / { f = 0 } f' CHANGELOG.md |
      sed -e '/./,$!d' > "$WORK/changes.md"
    [ -s "$WORK/changes.md" ] || die "the CHANGELOG.md section of $V is empty"
    if [ ! -f "$OUT/latest.json" ]; then
      printf '%s %s: %s\n' "$APP" "$V" "$(sed -n '1p' "$WORK/changes.md")" > "$WORK/feed-notes.txt"
      node scripts/updater-latest-json.mjs --version "$V" --base-url "$BASE_URL" \
        --platform "darwin-aarch64=$OUT/$TGZ" --platform "windows-x86_64=$OUT/$EXE" \
        --notes-file "$WORK/feed-notes.txt" --out "$OUT/latest.json"
      rm -f "$OUT/latest.json.sig"
    fi
    if [ ! -f "$OUT/latest.json.sig" ]; then
      with_update_key "$TAURI" signer sign --app-version "$V" "$OUT/latest.json" >/dev/null
    fi
    [ -s "$OUT/latest.json.sig" ] || die "latest.json was not signed"
    ok "latest.json and latest.json.sig"
    (cd "$OUT" && shasum -a 256 "$DMG" "$EXE" "$TGZ" > SHA256SUMS.txt)
    ok "SHA256SUMS.txt"

    say "Draft release $TAG"
    {
      cat "$WORK/changes.md"
      printf '\n## Downloads\n\n| System | File |\n|---|---|\n'
      printf '| macOS 13 or later, Apple silicon | `%s` (signed with a Developer ID, notarized by Apple) |\n' "$DMG"
      printf '| Windows 10 and 11, x64 | `%s` (not code-signed yet) |\n\n' "$EXE"
      printf '`SHA256SUMS.txt` has the checksums of the dmg, the Windows installer and the macOS update bundle.\n\n'
      printf '## Updates\n\nGSpot 0.2.2 and later install this version themselves after publik approves the release. '
      printf 'Every update is signed with the publik update key, and GSpot checks the signature and the version before it installs anything.\n\n'
      printf '## Update files\n\nThese files are for the updater. You do not need to download them.\n\n'
      printf -- '- `latest.json`: the update feed. publik serves it at `https://publikhq.com/api/updates/gspot/latest.json` after it approves this release. `latest.json.sig` is its signature.\n'
      printf -- '- `%s` and `.sig`: the macOS update (the same notarized app as in the dmg).\n' "$TGZ"
      printf -- '- `%s.sig`: the signature of the Windows installer.\n\n' "$EXE"
      printf '## Windows\n\nThe installer is not code-signed yet. Windows SmartScreen can show "Windows protected your PC". Click "More info", then "Run anyway".\n\n'
      printf '## Built and tested\n\n'
      printf -- '- Windows installer, unit tests and the Windows end-to-end test: https://github.com/%s/actions/runs/%s\n' "$REPO" "$WIN_RUN"
      printf -- '- macOS app (unsigned, then signed and notarized on the release Mac with `scripts/publik-release.sh`): https://github.com/%s/actions/runs/%s\n' "$REPO" "$MAC_RUN"
    } > "$WORK/release-notes.md"
    if [ "$RSTATE" = none ]; then
      (cd "$OUT" && gh release create "$TAG" -R "$REPO" --draft --target "$SHA" --title "$APP $V" \
        --notes-file "$WORK/release-notes.md" "${ASSETS[@]}" >/dev/null)
      RSTATE=draft
      ok "draft created with ${#ASSETS[@]} files"
    else
      DIGESTS="$(ghr release view "$TAG" -R "$REPO" --json assets -q '.assets[] | "\(.name) \(.digest)"')"
      for a in "${ASSETS[@]}"; do
        if printf '%s\n' "$DIGESTS" | grep -qxF "$a sha256:$(sha "$OUT/$a")"; then continue; fi
        gh release upload "$TAG" -R "$REPO" --clobber "$OUT/$a" >/dev/null
        ok "uploaded $a"
      done
      gh release edit "$TAG" -R "$REPO" --notes-file "$WORK/release-notes.md" >/dev/null
      ok "draft up to date"
    fi
  fi

  say "mac-smoke.yml on the draft"
  DMG_DIGEST="$(ghr release view "$TAG" -R "$REPO" --json assets -q ".assets[] | select(.name == \"$DMG\") | .digest")"
  SMOKE="$(get_state "smoke_$DMG_DIGEST")"
  if [ -z "$SMOKE" ] || [ "$(run_field "$SMOKE" conclusion)" != success ]; then
    SMOKE="$(start_run mac-smoke.yml -f "tag=$TAG")"
    echo "    started mac-smoke.yml: https://github.com/$REPO/actions/runs/$SMOKE"
    wait_run "$SMOKE"
    [ "$(run_field "$SMOKE" conclusion)" = success ] || die "mac-smoke.yml failed on the draft: https://github.com/$REPO/actions/runs/$SMOKE"
    set_state "smoke_$DMG_DIGEST" "$SMOKE"
  fi
  ok "mac-smoke.yml: https://github.com/$REPO/actions/runs/$SMOKE"
  if [ -f "$WORK/release-notes.md" ] && ! grep -q "actions/runs/$SMOKE" "$WORK/release-notes.md"; then
    printf -- '- macOS: a clean GitHub macOS runner opens the notarized dmg of this release as a download, passes Gatekeeper, installs and starts the app: https://github.com/%s/actions/runs/%s\n' "$REPO" "$SMOKE" >> "$WORK/release-notes.md"
    gh release edit "$TAG" -R "$REPO" --notes-file "$WORK/release-notes.md" >/dev/null
  fi
fi

# ---------------------------------------------------------------------------------------------
say "Verify $TAG as published files (downloaded back)"
VER="$WORK/verify"
rm -rf "$VER"
mkdir -p "$VER/prev" "$VER/tgz" "$VER/mnt"
ghr release download "$TAG" -R "$REPO" -D "$VER" --clobber >/dev/null
PREV_TGZ="$VER/prev/${APP}_${PREV_TAG#v}_aarch64.app.tar.gz"
ghr release download "$PREV_TAG" -R "$REPO" -p "$(basename "$PREV_TGZ")" -D "$VER/prev" --clobber >/dev/null 2>&1 || note "$PREV_TAG has no $(basename "$PREV_TGZ")"
GOT="$(cd "$VER" && find . -maxdepth 1 -type f | sed 's|^\./||' | sort)"
if [ "$GOT" = "$(printf '%s\n' "${ASSETS[@]}" | sort)" ]; then ok "the release has exactly: ${ASSETS[*]}"
else bad "the release has exactly: ${ASSETS[*]}"; echo "        it has: $(echo "$GOT" | tr '\n' ' ')"; fi
expect "SHA256SUMS.txt matches the dmg, the installer and the update bundle" sh -c "cd '$VER' && shasum -a 256 -c SHA256SUMS.txt >/dev/null"
git show "$SHA:src-tauri/tauri.conf.json" > "$VER/tauri.conf.json"
git show "$PREV_TAG:src-tauri/tauri.conf.json" > "$VER/tauri.conf.prev.json"
pubkey_of() { node -p 'require(process.argv[1]).plugins?.updater?.pubkey ?? ""' "$1"; }
if [ -n "$(pubkey_of "$VER/tauri.conf.prev.json")" ]; then
  expect "the update public key is the one $PREV_TAG ships (installed copies trust it)" \
    test "$(pubkey_of "$VER/tauri.conf.json")" = "$(pubkey_of "$VER/tauri.conf.prev.json")"
else
  note "$PREV_TAG has no updater: no public key to compare"
fi
if node scripts/updater-latest-json.mjs --check "$VER/latest.json" --config "$VER/tauri.conf.json" --version "$V" --base-url "$BASE_URL" \
  --platform "darwin-aarch64=$VER/$TGZ" --platform "windows-x86_64=$VER/$EXE" 2>&1 | sed 's/^/        /'; then
  ok "latest.json: version $V, absolute URLs, every signature (and latest.json.sig) verifies with the app's public key"
else
  bad "latest.json and its signatures"
fi
expect "latest.json.sig is there" test -s "$VER/latest.json.sig"
expect "Windows installer version resource is $V" node scripts/exe-version.mjs "$VER/$EXE" --expect "$V"
if [ -f "$OUT/SHA256SUMS-windows.txt" ]; then
  expect "Windows installer is the CI build, byte for byte" grep -q "^$(sha "$VER/$EXE")  $EXE\$" "$OUT/SHA256SUMS-windows.txt"
fi

# The dmg and the app in it.
SPCTL_DMG="$(spctl -a -vv -t open --context context:primary-signature "$VER/$DMG" 2>&1 || true)"
expect "dmg: Gatekeeper accepts it (Notarized Developer ID)" sh -c 'printf "%s\n" "$1" | grep -q ": accepted" && printf "%s\n" "$1" | grep -q "source=Notarized Developer ID"' _ "$SPCTL_DMG"
expect "dmg: notarization ticket stapled" stapled "$VER/$DMG"
expect "dmg: signed by $IDENTITY" sh -c 'codesign -dvv "$1" 2>&1 | grep -qxF "Authority=$2"' _ "$VER/$DMG" "$IDENTITY"
hdiutil attach -nobrowse -noautoopen -readonly -mountpoint "$VER/mnt" "$VER/$DMG" >/dev/null
trap 'hdiutil detach "$VER/mnt" -quiet 2>/dev/null || true' EXIT
COPYFILE_DISABLE=1 tar -xzf "$VER/$TGZ" -C "$VER/tgz"
[ -f "$PREV_TGZ" ] && tar -xzf "$PREV_TGZ" -C "$VER/prev"
expect "update bundle: no AppleDouble (._) files" sh -c '! tar -tzf "$1" | grep -q "/\._"' _ "$VER/$TGZ"
PREV_DR=""
PREV_BIN=/nonexistent
if [ -d "$VER/prev/$APP.app" ]; then
  PREV_DR="$(dr "$VER/prev/$APP.app")"
  PREV_BIN="$VER/prev/$APP.app/Contents/MacOS/$(plist "$VER/prev/$APP.app" CFBundleExecutable)"
fi
TOKEN="$(tr -d '[:space:]' < "$TOKEN_FILE")"
for where in "dmg:$VER/mnt/$APP.app" "update bundle:$VER/tgz/$APP.app"; do
  w="${where%%:*}"
  a="${where#*:}"
  info="$(codesign -dvv "$a" 2>&1)"
  expect "$w app: Info.plist version $V" test "$(plist "$a" CFBundleShortVersionString)" = "$V"
  expect "$w app: bundle id $BUNDLE_ID" test "$(plist "$a" CFBundleIdentifier)" = "$BUNDLE_ID"
  expect "$w app: codesign --verify --strict --deep" codesign --verify --strict --deep "$a"
  expect "$w app: $IDENTITY, team $TEAM, hardened runtime" sh -c 'printf "%s\n" "$1" | grep -qxF "Authority=$2" && printf "%s\n" "$1" | grep -qxF "TeamIdentifier=$3" && printf "%s\n" "$1" | grep -q "flags=.*runtime"' _ "$info" "$IDENTITY" "$TEAM"
  expect "$w app: notarization ticket stapled" stapled "$a"
  SPCTL_APP="$(spctl -a -vv -t exec "$a" 2>&1 || true)"
  expect "$w app: Gatekeeper accepts it (Notarized Developer ID)" sh -c 'printf "%s\n" "$1" | grep -q ": accepted" && printf "%s\n" "$1" | grep -q "source=Notarized Developer ID"' _ "$SPCTL_APP"
  expect "$w app: designated requirement as in $PREV_TAG (TCC grants carry over)" test -n "$PREV_DR" -a "$(dr "$a")" = "$PREV_DR"
  BIN="$a/Contents/MacOS/$(plist "$a" CFBundleExecutable)"
  expect "$w app: the publik app token is compiled in, as in $PREV_TAG" sh -c \
    'test "$(LC_ALL=C grep -c -a -F "$1" "$2" 2>/dev/null)" -ge 1 && test "$(LC_ALL=C grep -c -a -F "$1" "$3" 2>/dev/null)" -ge 1' _ "$TOKEN" "$BIN" "$PREV_BIN" 2>/dev/null
done
unset TOKEN
CD_DMG="$(codesign -dvvv "$VER/mnt/$APP.app" 2>&1 | sed -n 's/^CDHash=//p')"
CD_TGZ="$(codesign -dvvv "$VER/tgz/$APP.app" 2>&1 | sed -n 's/^CDHash=//p')"
expect "the update bundle is the app of the dmg (CDHash $CD_DMG)" test -n "$CD_DMG" -a "$CD_DMG" = "$CD_TGZ"
echo "  designated requirement: $(dr "$VER/tgz/$APP.app")"
hdiutil detach "$VER/mnt" -quiet
trap - EXIT
if [ "$RSTATE" = draft ]; then
  expect "the draft targets ${SHA:0:12}" test "$(ghr api "repos/$REPO/releases" -q ".[] | select(.tag_name == \"$TAG\") | .target_commitish")" = "$SHA"
fi
if [ "$RSTATE" = draft ]; then [ "$FAILS" -eq 0 ] || die "$FAILS check(s) failed. $TAG stays a draft: fix the cause and run the command again."; fi
[ "$FAILS" -eq 0 ] || die "$FAILS check(s) failed on the published $TAG."
ok "every check passed"

# ---------------------------------------------------------------------------------------------
if [ "$RSTATE" = draft ]; then
  if [ "${PUBLIK_RELEASE_NO_PUBLISH:-0}" = 1 ]; then
    say "Not published (PUBLIK_RELEASE_NO_PUBLISH=1): https://github.com/$REPO/releases (draft $TAG)"
    exit 0
  fi
  say "Publish $TAG"
  gh release edit "$TAG" -R "$REPO" --draft=false --latest >/dev/null
  git fetch -q origin tag "$TAG"
  expect "tag $TAG points at ${SHA:0:12}" test "$(git rev-parse "$TAG^{commit}")" = "$SHA"
  expect "the release is public" test "$(ghr release view "$TAG" -R "$REPO" --json isDraft -q .isDraft)" = false
  for url in $(node -p 'Object.values(require(process.argv[1]).platforms).map((p) => p.url).join("\n")' "$VER/latest.json") "${BASE_URL}latest.json" "${BASE_URL}$DMG"; do
    code="$(curl -s -o /dev/null -L -r 0-0 -w '%{http_code}' "$url")"
    expect "$url answers ($code)" test "$code" = 200 -o "$code" = 206
  done
  [ "$FAILS" -eq 0 ] || die "$FAILS check(s) failed after publishing $TAG: look at https://github.com/$REPO/releases/tag/$TAG"
  rm -rf "$WORK"
  say "Published: https://github.com/$REPO/releases/tag/$TAG"
  echo "publik takes the release in at its next daily intake (10:00 UTC) and then serves its"
  echo "latest.json at https://publikhq.com/api/updates/gspot/latest.json."
else
  rm -rf "$VER"
  say "$TAG is published and passes every check: https://github.com/$REPO/releases/tag/$TAG"
fi
