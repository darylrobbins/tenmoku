---
task_uid: "01m6ab49vwrfbfv326vab1rz3w"
task: "T1269"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "scripts/test-bundle-macos.sh exits 0 on macOS; it bundles a copy of /usr/bin/true as the binary into a temp dir and asserts every following criterion"
    required: true
    manual: false
  - id: "C2"
    text: "The bundle has Contents/MacOS/tenmoku (executable), Contents/Resources/Tenmoku.icns, Contents/Info.plist; plutil -lint passes; PlistBuddy reads CFBundleName=Tenmoku, CFBundleDisplayName=Tenmoku, CFBundleExecutable=tenmoku, CFBundleIconFile=Tenmoku, CFBundlePackageType=APPL, NSHighResolutionCapable=true, and CFBundleShortVersionString equals the --version given, with no \"@VERSION@\" left"
    required: true
    manual: false
  - id: "C3"
    text: "CFBundleIdentifier equals the reverse_dns in crates/fe-config/src/paths.rs app_id() (dev.tenmoku.tenmoku after T1258); the test reads both and compares them"
    required: true
    manual: false
  - id: "C4"
    text: "iconutil -c iconset on the produced Tenmoku.icns yields icon_16x16.png through icon_512x512@2x.png (ten files), and the 1024 render is from crates/fastedit/assets/icon/fastedit-1024.png (no .icns is committed: git ls-files \"*.icns\" prints nothing)"
    required: true
    manual: false
  - id: "C5"
    text: "codesign --verify --strict --deep passes on the bundle; codesign -dvv shows Signature=adhoc and flags containing runtime; codesign -d --entitlements - shows com.apple.security.cs.allow-jit"
    required: true
    manual: false
  - id: "C6"
    text: "scripts/bundle-macos.sh --binary /nonexistent exits non-zero with a message naming the missing path; --sign-only \u003cbundle\u003e re-signs an existing bundle and verify passes; --help documents --binary, --out, --version, --sign-only and SIGN_IDENTITY"
    required: true
    manual: false
  - id: "C7"
    text: "docs/release/macos-signing.md exists and names, in order: Developer ID Application certificate, SIGN_IDENTITY= scripts/bundle-macos.sh, ditto -c -k --keepParent, xcrun notarytool store-credentials, xcrun notarytool submit ... --wait, xcrun stapler staple, spctl -a -vv; rg -c for each of those seven strings is at least 1"
    required: true
    manual: false
  - id: "C8"
    text: "git diff --stat against the base lists only the six scope paths; no Rust file changes"
    required: true
    manual: false
---

# Build Tenmoku.app from the release binary: Info.plist, entitlements, an .icns generated from the app icon, an ad-hoc signature, and the Developer ID and notarization steps written down for a person

## Goal

Build `Tenmoku.app` from the release binary with one script, so the Dock and menu bar say "Tenmoku" (capitalised;
unbundled macOS shows the executable name `tenmoku`, which T1257 noted). Ad-hoc sign it. Developer ID signing and
notarization need the user's certificate and Apple ID: they are documented as human steps, not done here.

## Facts found

- No `.app`, Info.plist, entitlements or icns exist. The release profile already strips (`strip = "symbols"`, T1256);
  the binary is `target/release/tenmoku` (`[[bin]] name = "tenmoku"` in `crates/fastedit/Cargo.toml`, T1257).
- The icon source is `crates/fastedit/assets/icon/fastedit-1024.png` (and `.svg`); `app_icon.rs` sets the Dock icon
  from the same PNG at startup, which keeps working inside a bundle. `sips`, `iconutil` and `rsvg-convert` exist on
  the Mac hosts.
- The repo has no bundling tool (no cargo-bundle, no xtask); its scripts are POSIX/zsh shell with a
  `scripts/test-*.sh` sibling each (`prune-build-cache.sh` / `test-prune-build-cache.sh`). Follow that.
- wasmtime (kiln-plugin) and tree-sitter's wasm feature JIT-compile code. Under the hardened runtime that needs
  `com.apple.security.cs.allow-jit` (and `allow-unsigned-executable-memory`); without it a Developer-ID-signed
  build would crash at first plugin load. Entitlements go in the repo now so the signing step is only a flag.
- The bundle id is `fe_config::paths::app_id()`'s reverse-DNS, which T1258 changes to `dev.tenmoku.tenmoku`.
  Hence the dependency.
- Workspace version is `0.1.0` (`[workspace.package]` in `Cargo.toml`).

## Changes

All new files; no existing file is edited except the doc line in TESTING.md's Running section.

1. `crates/fastedit/macos/Info.plist`: CFBundleName/DisplayName `Tenmoku`, CFBundleExecutable `tenmoku`,
   CFBundleIdentifier `dev.tenmoku.tenmoku`, CFBundleIconFile `Tenmoku`, CFBundlePackageType `APPL`,
   CFBundleShortVersionString and CFBundleVersion as `@VERSION@` placeholders, LSMinimumSystemVersion (use the
   deployment target the binary builds with: read it with `otool -l`, do not guess),
   NSHighResolutionCapable true, NSPrincipalClass `NSApplication`, LSApplicationCategoryType
   `public.app-category.developer-tools`, NSHumanReadableCopyright `Copyright 2026 Daryl Robbins`.
2. `crates/fastedit/macos/Tenmoku.entitlements`: allow-jit and allow-unsigned-executable-memory, nothing else
   (the app is not sandboxed).
3. `scripts/bundle-macos.sh [--binary PATH] [--out DIR] [--version X.Y.Z]`: defaults `target/release/tenmoku`,
   `target/bundle`, the workspace version (read from Cargo.toml). It refuses a missing binary with a clear message
   (it does not build; say so in `--help`). It writes `DIR/Tenmoku.app/Contents/{MacOS/tenmoku,Resources/Tenmoku.icns,
   Info.plist}`, substitutes the version, generates the `.icns` at bundle time from the 1024 PNG with
   `sips` + `iconutil` (all ten iconset sizes; no `.icns` is committed), then signs with
   `codesign --force --options runtime --entitlements ... -s "${SIGN_IDENTITY:--}"` (default `-`, ad-hoc), and
   finally runs `codesign --verify --strict --deep`. `--sign-only APP` skips the assembly and only re-signs an existing bundle (the release card's Developer ID step uses it). Exits non-zero on any failure; prints the app path last.
4. `scripts/test-bundle-macos.sh`: builds a bundle from a copy of `/usr/bin/true` standing in for the binary
   (so no cargo build), into a temp dir, and asserts the criteria below. Prints one line per check; exits non-zero on
   the first failure.
5. `docs/release/macos-signing.md` (new): the human steps, concretely. Developer ID Application certificate in the
   keychain; `SIGN_IDENTITY="Developer ID Application: <name> (<team>)" scripts/bundle-macos.sh`;
   `ditto -c -k --keepParent Tenmoku.app Tenmoku.zip`; `xcrun notarytool store-credentials`, `xcrun notarytool submit
   Tenmoku.zip --keychain-profile <p> --wait`; `xcrun stapler staple Tenmoku.app`; `spctl -a -vv Tenmoku.app`
   and `codesign -dvv` as the checks. Say plainly which parts only the user can do.
6. `TESTING.md` Running section: two lines on `scripts/bundle-macos.sh` and `scripts/test-bundle-macos.sh`.

## Not here

The release workflow (next card), a `.dmg`, document types / file associations in Info.plist (a product
decision), Sparkle/auto-update, and any change to `app_icon.rs`.

## Human steps after landing

Open `target/bundle/Tenmoku.app` on a Mac and check the Dock label, the menu bar title and Cmd-Tab icon say
"Tenmoku"; then do the Developer ID + notarization steps in the doc.
