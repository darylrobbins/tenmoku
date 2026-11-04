---
task_uid: "01m6j60rdfnbp2pnx878cn6pr7"
task: "T1370"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "scripts/test-sparkle-bundle.sh passes"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/test-bundle-macos.sh passes (a bundle without Sparkle is unchanged)"
    required: true
    manual: false
  - id: "C3"
    text: "copied_files_and_rows_agree (crates/fastedit/tests/third_party.rs) passes with the new Sparkle row"
    required: true
    manual: false
  - id: "C4"
    text: "With a feed URL and key, plutil -p on the bundle's Info.plist shows SUFeedURL, SUPublicEDKey, SUEnableAutomaticChecks, SUScheduledCheckInterval and SUAutomaticallyUpdate; without them none of the five is present"
    required: true
    manual: false
---

# Embed a pinned Sparkle.framework in Tenmoku.app: fetch with a SHA-256 check, sign it inside-out under the hardened runtime, and add the SU* Info.plist keys filled from a feed URL and public key

## Problem

Tenmoku.app has no updater framework. The first public release will auto-update through Sparkle 2 (MIT; a bundled binary framework, not a crate). This card gets Sparkle.framework into the bundle, signed correctly, with the Info.plist keys. The Rust side is a later card; the framework is loaded at run time, so no link flags, no build.rs and no rpath change.

## Change

1. `scripts/fetch-sparkle.sh [--out DIR]`: downloads the pinned Sparkle 2.x release tarball (pick the newest 2.x on sparkle-project/Sparkle GitHub releases; pin VERSION and SHA-256 as variables at the top), verifies the SHA-256 (refuses on mismatch), extracts Sparkle.framework and the `bin/` tools (`generate_keys`, `sign_update`) into DIR (default `target/sparkle/<version>/`), and prints the framework path. Idempotent: reuses a verified cache. `SPARKLE_FRAMEWORK=<path>` in the environment skips the download everywhere (tests use a fixture).
2. `scripts/bundle-macos.sh`: when `SPARKLE_FRAMEWORK` is set or `--sparkle` is passed, copy the framework into `Contents/Frameworks/Sparkle.framework` with `ditto`, deleting `Versions/B/XPCServices` (Tenmoku is not sandboxed, so Sparkle's documented non-sandboxed setup needs only Autoupdate and Updater.app). Without it the bundle is built exactly as today (dev bundles stay Sparkle-free).
3. Signing inside-out in `sign()`: `Versions/B/Autoupdate`, `Versions/B/Updater.app`, then `Sparkle.framework`, each `codesign --force --options runtime <timestamp> -s <identity>` WITHOUT the app entitlements and without `--deep`, then the app as today. For an ad-hoc identity ('-'), sign the app with a temp copy of Tenmoku.entitlements plus `com.apple.security.cs.disable-library-validation` (hardened runtime refuses to load an ad-hoc framework otherwise); a Developer ID identity keeps the committed entitlements unchanged. `scripts/sign-notarize-macos.sh` re-signs through `bundle-macos.sh --sign-only`, so check it still works and edit it only if it does not.
4. `crates/fastedit/macos/Info.plist`: add `SUFeedURL` (@FEED_URL@), `SUPublicEDKey` (@ED_PUBLIC_KEY@), `SUEnableAutomaticChecks` (true), `SUScheduledCheckInterval` (86400), `SUAutomaticallyUpdate` (false: Tenmoku asks before installing). bundle-macos.sh fills them from `--feed-url`/`TENMOKU_FEED_URL` and `--ed-public-key`/`TENMOKU_ED_PUBLIC_KEY`; when either is empty it deletes all five SU* keys with PlistBuddy, so a build without a feed never polls. The feed URL must be https (refuse otherwise). `CFBundleVersion` stays equal to the dotted version (Sparkle's comparator orders dotted numbers; releases are tag-checked to increase).
5. `THIRD_PARTY.md` (and `crates/fastedit/tests/third_party.rs` stays green): a row for Sparkle (MIT, pinned version, source URL, SHA-256), with its licence text kept in the bundle at `Contents/Resources/Sparkle-LICENSE.txt` (copy from the tarball). `docs/release/macos-signing.md`: a short Sparkle section (framework fetch, signing order, the SU* keys, the library-validation note).

## Mac-only

All of this needs macOS (codesign, plutil, PlistBuddy, ditto). Do not lend to a Linux host. Network is needed only by fetch-sparkle.sh itself; the tests use a fixture.

## Tests

New `scripts/test-sparkle-bundle.sh` (do not grow test-bundle-macos.sh): builds a fixture `Sparkle.framework` (Versions/B with Sparkle binary copy of /usr/bin/true, Autoupdate, Updater.app/Contents/MacOS/Updater, XPCServices dir, Resources) and a fixture binary; asserts the framework lands in Contents/Frameworks, XPCServices is gone, `codesign --verify --strict --deep` passes, `codesign -dvv` of Autoupdate/Updater/framework show hardened runtime (flags=0x10002(adhoc,runtime)), the app has the disable-library-validation entitlement when ad-hoc, the five SU* keys exist with the given values, and are all absent without a feed/key, an http feed URL is refused, and the SHA-256 mismatch path of fetch-sparkle.sh refuses (served from a file:// tarball via `SPARKLE_URL_OVERRIDE`).

## Implementation notes

- Pinned Sparkle 2.10.1, the newest 2.x on 2026-11-04; the SHA-256 matches GitHub's asset digest.
- The release has a top-level `XPCServices` symlink as well as `Versions/B/XPCServices`. Both go, since a dangling symlink fails `codesign --strict`; the fixture has the symlink too.
- Sparkle's licence comes from `LICENSE` beside the framework (`fetch-sparkle.sh` extracts it there). A `SPARKLE_FRAMEWORK` with none beside it is refused, so the bundle can't ship without it.
- Checked by hand against the real 2.10.1 with a small dlopen binary as the app: `--strict --deep` verifies; a hardened ad-hoc app loads Sparkle with disable-library-validation and is refused without it. A plain bundle's Info.plist equals the base's as JSON.
- `sign-notarize-macos.sh` works unchanged through `--sign-only` and was not edited, so T1270 F1 stays open.
- For the Rust card: Sparkle 2.10.1's LSMinimumSystemVersion and Mach-O minos are 12.0, while Tenmoku's Info.plist says 11.0. Either raise Tenmoku's minimum or don't start the updater on macOS 11. Left alone here: the plist's minimum follows the binary's minos, and nothing loads the framework yet.
