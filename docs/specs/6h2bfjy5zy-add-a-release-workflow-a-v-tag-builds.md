---
task_uid: "01m6ab4ag2htamfm6h2bfjy5zy"
task: "T1270"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "actionlint .github/workflows/release.yml exits 0"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/test-release-workflow.sh exits 0 on macOS; it runs actionlint and the checks below"
    required: true
    manual: false
  - id: "C3"
    text: ".github/workflows/release.yml triggers on push tags v* (and workflow_dispatch), builds on runs-on: macos-latest, runs cargo build --release -p fastedit --locked, calls scripts/bundle-macos.sh and scripts/sign-notarize-macos.sh, and uploads with gh release create/upload; contents: write appears only in the release job's permissions, and no secret appears inside a run: block (secrets only under env:)"
    required: true
    manual: false
  - id: "C4"
    text: "scripts/sign-notarize-macos.sh with no MACOS_CERTIFICATE in the environment prints a line containing \"skipping Developer ID signing and notarization\" and exits 0, leaving the bundle ad-hoc signed and verifiable (codesign --verify --strict passes); with MACOS_CERTIFICATE set but APPLE_ID unset, a stub-free dry check shows the notarize step prints its own skip line (assert with grep on the script text if running it needs a real certificate)"
    required: true
    manual: false
  - id: "C5"
    text: "The script has no set -x and echoes no variable named *PASSWORD*, *CERTIFICATE* or *APPLE_APP* (rg -n \"set -x|echo.*(PASSWORD|CERTIFICATE|APPLE_APP)\" scripts/sign-notarize-macos.sh prints nothing)"
    required: true
    manual: false
  - id: "C6"
    text: "docs/release/macos-signing.md has a \"Releasing\" section naming the six secrets (MACOS_CERTIFICATE, MACOS_CERTIFICATE_PASSWORD, MACOS_SIGN_IDENTITY, APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD) and the tag format vX.Y.Z"
    required: true
    manual: false
  - id: "C7"
    text: "git diff --stat against the base lists only the four scope paths; scripts/bundle-macos.sh is unchanged"
    required: true
    manual: false
---

# Add a release workflow: a v* tag builds the stripped release on macOS arm64, bundles Tenmoku.app, signs and notarizes when the secrets exist (skipping cleanly when they do not), and attaches a zip and checksum to a GitHub Release

## Goal

A tag push builds the stripped release binary on a macOS arm64 runner, bundles it with
`scripts/bundle-macos.sh`, signs and notarizes when the repository has the secrets (and says so and carries on
ad-hoc signed when it has not), zips `Tenmoku.app` and attaches the zip to a GitHub Release.

## Facts found

- `.github/workflows/` has `ci.yml` (macOS job `runs-on: macos-15`, `actions/checkout@v7`,
  `rustup toolchain install` reading `rust-toolchain.toml` (1.98.1), `Swatinem/rust-cache@v2`), `bench.yml`, `lsp.yml`,
  `soak.yml`. No release workflow. Copy ci.yml's checkout and toolchain steps.
- Release profile strips symbols already (`[profile.release] strip = "symbols"`, T1256); the binary is
  `target/release/tenmoku` from `cargo build --release -p fastedit --locked`.
- Regatta always sends `.github/workflows/**` to the user for approval; expect that gate on this card.
- Needs the bundle script and entitlements from the bundle card: depends on it.
- Secrets the user will create (not a criterion): `MACOS_CERTIFICATE` (base64 .p12), `MACOS_CERTIFICATE_PASSWORD`,
  `MACOS_SIGN_IDENTITY`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`.

## Changes

1. `scripts/sign-notarize-macos.sh APP_PATH ZIP_PATH`: the signing half, written so it runs anywhere with no CI.
   With `MACOS_CERTIFICATE` unset it prints `skipping Developer ID signing and notarization: no certificate secret`
   and exits 0 (the bundle stays ad-hoc signed). With it set: creates a temporary keychain, imports the .p12,
   re-signs with `SIGN_IDENTITY="$MACOS_SIGN_IDENTITY" scripts/bundle-macos.sh --sign-only APP` (the mode the bundle card added; do not edit bundle-macos.sh here),
   zips with `ditto`, runs `xcrun notarytool submit ... --wait` only when `APPLE_ID`, `APPLE_TEAM_ID` and
   `APPLE_APP_PASSWORD` are all set (else prints a skip line), staples, deletes the keychain in a trap. Never echo a
   secret; no `set -x`.
2. `.github/workflows/release.yml`: `on: push: tags: ['v*']` plus `workflow_dispatch`; `permissions: contents:
   write` only on the release job; one job on `macos-latest` (arm64). Steps: checkout, toolchain, rust-cache,
   `cargo build --release -p fastedit --locked`, `scripts/bundle-macos.sh --version "${GITHUB_REF_NAME#v}"`,
   `scripts/sign-notarize-macos.sh` (secrets as step `env:`, never in `run:` text), `ditto -c -k --keepParent` to
   `Tenmoku-<version>-macos-arm64.zip`, a SHA-256 file beside it, then `gh release create "$GITHUB_REF_NAME"
   --generate-notes` (or `upload --clobber` if it exists) with both files; on `workflow_dispatch` upload as a
   workflow artifact instead of a release.
3. `scripts/test-release-workflow.sh`: runs `actionlint .github/workflows/release.yml` (prints `actionlint not
   installed` and exits non-zero if absent: the criteria require it), then `rg`-asserts the properties in the
   criteria, and runs `scripts/sign-notarize-macos.sh` with a clean environment against a bundle from
   `scripts/bundle-macos.sh` (fake binary as in `test-bundle-macos.sh`), asserting the skip line and exit 0.
4. `docs/release/macos-signing.md` (from the bundle card): add a "Releasing" section: tag `vX.Y.Z`, push, which
   secrets enable which stage, and that an unsigned/ad-hoc release shows a Gatekeeper warning.

## Not here

Linux or Windows artifacts, a `.dmg`, auto-update, Homebrew cask, and creating the secrets or pushing a tag (human).
The first real tag run is the user's check.

## Implementation notes

- `scripts/sign-notarize-macos.sh` treats an empty variable as unset, since Actions passes a
  missing secret as an empty string. Its `ZIP_PATH` is the notary upload only; the workflow
  zips the app again after stapling. The skip path writes no zip and re-verifies the ad-hoc
  signature.
- Dry run of the certificate path with a self-signed code-signing `.p12`, under a throwaway
  `HOME` so the real keychain search list is untouched: keychain creation, import and
  `set-key-partition-list` succeed and `find-identity` lists the identity, but codesign
  refuses it as `CSSMERR_TP_NOT_TRUSTED`, as expected for a self-signed root. A real
  Developer ID certificate chains to Apple's roots; the first tagged run with the secrets is
  the end-to-end check.
- Releases are published only on a tag push (`github.event_name == 'push'`); a manual run
  uploads a workflow artifact even when started on a tag.
