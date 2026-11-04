---
task_uid: "01m6j60ym7r0bsn0qd7e8z4pg7"
task: "T1374"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "scripts/test-appcast.sh passes"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/test-release-workflow.sh passes with the Sparkle checks"
    required: true
    manual: false
  - id: "C3"
    text: "actionlint .github/workflows/release.yml is clean"
    required: true
    manual: false
  - id: "C4"
    text: "Finish raises the expected human-only risk exception for .github/workflows/**; nothing else is flagged"
    required: true
    manual: false
---

# Publish Sparkle updates from the release workflow: EdDSA-sign the zip, generate appcast.xml and upload it with the release

## Problem

Releases must publish what Sparkle needs: an EdDSA-signed zip and an `appcast.xml`. The zip step in `.github/workflows/release.yml` makes the zip and checksum only.

## Change

1. `scripts/make-appcast.sh --zip PATH --version X.Y.Z --download-url URL --signature BASE64 --out appcast.xml [--notes-url URL] [--min-system 11.0]`: writes a one-item Sparkle appcast (RSS, `xmlns:sparkle`, `sparkle:version` = CFBundleVersion = X.Y.Z, `sparkle:shortVersionString`, `sparkle:minimumSystemVersion` from the bundle's LSMinimumSystemVersion, `<enclosure url length type='application/octet-stream' sparkle:edSignature>` with `length` taken from the file, `pubDate` RFC 822, `sparkle:releaseNotesLink` when --notes-url). A single latest item is enough: Sparkle updates any older version straight to it. Validates with `xmllint --noout`. The signature comes from the caller; the script never touches the private key.
2. release.yml, after the zip step (all new secrets only as step `env:` values, matching the existing hygiene rules): fetch Sparkle tools with `scripts/fetch-sparkle.sh` (card A); sign with `sign_update --ed-key-file -` fed from `SPARKLE_ED_PRIVATE_KEY` on stdin (printf into the pipe, never an argument, never echoed); skipped with a clear 'skipping Sparkle signing' line when the secret is absent so forks and the manual run still work; run `make-appcast.sh`; upload `appcast.xml` alongside the zip in the same `gh release` commands. Build the bundle with `TENMOKU_FEED_URL` from `vars.TENMOKU_FEED_URL` (repository variable; default `https://github.com/${{ github.repository }}/releases/latest/download/appcast.xml`) and `TENMOKU_ED_PUBLIC_KEY` from `vars.SPARKLE_ED_PUBLIC_KEY`, and `SPARKLE_FRAMEWORK` from the fetch step. `DOWNLOAD_URL_PREFIX` (var, default the release download URL) lets a separate public host be used without editing the workflow.
3. Extend `scripts/test-release-workflow.sh`: the secret list gains SPARKLE_ED_PRIVATE_KEY (env-only), checks that appcast.xml is uploaded, that sign_update gets the key on stdin only, that fetch precedes bundling, and a run of make-appcast.sh on a fixture zip.
4. `docs/release/macos-signing.md` Releasing section: SPARKLE_ED_PRIVATE_KEY secret, SPARKLE_ED_PUBLIC_KEY and TENMOKU_FEED_URL variables, appcast location, how to rotate nothing (key loss means users must reinstall; say so).

## Human-only exception

Edits to `.github/workflows/**` raise a `risk` exception at finish. That is expected: leave it for the human, do not work around it.

## Mac-only

test-release-workflow.sh needs macOS and actionlint. Do not lend to Linux.

## Tests

New `scripts/test-appcast.sh` (macOS or Linux with xmllint): fixture zip, asserts every field above, length equals file size, xmllint passes, a missing required argument or non-https URL is refused, and the item version equals the passed version. `scripts/test-release-workflow.sh` stays green with its new checks.
