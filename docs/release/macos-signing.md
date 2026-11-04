# Signing and notarizing Tenmoku.app

`scripts/bundle-macos.sh` builds `Tenmoku.app` and signs it ad hoc. An ad-hoc signature is
enough to run the app on the Mac that built it, but Gatekeeper on any other Mac refuses it.
Distribution needs a Developer ID signature and Apple's notarization, and both need the
owner's Apple Developer account, certificate and Apple ID. **Only a person with that account
can do the steps below**; no script or agent in this repo holds those credentials.

The steps, in order:

1. Developer ID Application certificate in the login keychain (once).
2. `SIGN_IDENTITY= scripts/bundle-macos.sh` with the certificate's name: build and sign the app.
3. `ditto -c -k --keepParent`: zip the app for upload.
4. `xcrun notarytool store-credentials`: save the notary login in the keychain (once).
5. `xcrun notarytool submit ... --wait`: upload the zip and wait for Apple's verdict.
6. `xcrun stapler staple`: attach the notarization ticket to the app.
7. `spctl -a -vv` and `codesign -dvv`: check the result.

## 1. Developer ID Application certificate (person, once)

In the Apple Developer account (Certificates, Identifiers & Profiles), or Xcode > Settings >
Accounts > Manage Certificates, create a **Developer ID Application** certificate and install it
with its private key in the login keychain. Check it is there:

```zsh
security find-identity -v -p codesigning
# 1) ABCDEF... "Developer ID Application: <name> (<team>)"
```

The quoted name is the identity used below. `<team>` is the ten-character Team ID.

## 2. Build and sign with the Developer ID

```zsh
cargo build --release -p fastedit
SIGN_IDENTITY="Developer ID Application: <name> (<team>)" scripts/bundle-macos.sh
```

This writes `target/bundle/Tenmoku.app`, signed with the hardened runtime, a secure timestamp
and `crates/fastedit/macos/Tenmoku.entitlements` (JIT for wasmtime and tree-sitter's wasm
loader). macOS asks once for keychain access to the private key; allow it.

To re-sign a bundle that already exists, for example one a release build produced ad hoc:

```zsh
SIGN_IDENTITY="Developer ID Application: <name> (<team>)" scripts/bundle-macos.sh --sign-only target/bundle/Tenmoku.app
```

## 3. Zip for upload

`notarytool` takes a zip, dmg or pkg. `ditto` keeps the bundle's symlinks and metadata where
`zip` would not:

```zsh
cd target/bundle
ditto -c -k --keepParent Tenmoku.app Tenmoku.zip
```

## 4. Store the notary credentials (person, once)

Create an app-specific password for the Apple ID at <https://account.apple.com> (Sign-In and
Security > App-Specific Passwords), then save it in the keychain under a profile name:

```zsh
xcrun notarytool store-credentials tenmoku-notary \
  --apple-id "<apple id email>" --team-id "<team>" --password "<app-specific password>"
```

Nothing goes in the repo: later commands name only the profile.

## 5. Submit and wait

```zsh
xcrun notarytool submit Tenmoku.zip --keychain-profile tenmoku-notary --wait
```

It usually takes a few minutes and ends with `status: Accepted`. On `Invalid`, read why:

```zsh
xcrun notarytool log <submission id> --keychain-profile tenmoku-notary
```

The common causes are a missing hardened runtime or timestamp (both set by the script) or a
binary inside the bundle that is not signed with the same identity.

## 6. Staple the ticket

Stapling attaches the ticket to the app so Gatekeeper accepts it offline. Staple the `.app`,
not the zip, then zip it again for distribution:

```zsh
xcrun stapler staple Tenmoku.app
rm Tenmoku.zip && ditto -c -k --keepParent Tenmoku.app Tenmoku.zip
```

## 7. Check

```zsh
spctl -a -vv Tenmoku.app
# Tenmoku.app: accepted
# source=Notarized Developer ID
codesign -dvv Tenmoku.app
# Authority=Developer ID Application: <name> (<team>)
# flags=0x10000(runtime)
xcrun stapler validate Tenmoku.app
```

An ad-hoc build shows `Signature=adhoc` in `codesign -dvv` and `rejected` from `spctl`; that is
expected for local builds.

## Releasing

`.github/workflows/release.yml` does the steps above on a GitHub macOS arm64 runner. To cut a
release, set `version` under `[workspace.package]` in `Cargo.toml` to `X.Y.Z`, refresh
`Cargo.lock` with `cargo check --workspace`, commit both, tag that commit `vX.Y.Z` and push the
tag:

```zsh
git tag v0.2.0
git push origin v0.2.0
```

The tag minus its `v` becomes the app's version in `Info.plist`, while the binary has
`Cargo.toml`'s built in, which Check for Updates compares with the latest release. So the
workflow starts with `scripts/check-release-version.sh`, which refuses a tag that isn't exactly
`v` plus `Cargo.toml`'s version: a release that got past it would offer itself as its own update.

The workflow builds `cargo build --release -p fastedit --locked`, bundles `Tenmoku.app`, runs
`scripts/sign-notarize-macos.sh`, zips the app as `Tenmoku-X.Y.Z-macos-arm64.zip` with a
`.sha256` beside it, and attaches both to the GitHub Release for the tag (creating it with
generated notes, or replacing the files on a release that already exists). Running the
workflow by hand (Actions > Release > Run workflow) builds the same zip as a workflow
artifact and publishes nothing.

Signing and notarization use six repository secrets (Settings > Secrets and variables >
Actions). Each stage runs only when its secrets are all set; otherwise the log says
`skipping ...` and the release carries on with what it has:

| Secret | Stage | Value |
| --- | --- | --- |
| `MACOS_CERTIFICATE` | signing | the Developer ID Application certificate and key, exported from Keychain Access as a `.p12`, then `base64 -i cert.p12 \| pbcopy` |
| `MACOS_CERTIFICATE_PASSWORD` | signing | the password chosen when exporting the `.p12` |
| `MACOS_SIGN_IDENTITY` | signing | the identity name, `Developer ID Application: <name> (<team>)` |
| `APPLE_ID` | notarization | the Apple ID email |
| `APPLE_TEAM_ID` | notarization | the ten-character Team ID |
| `APPLE_APP_PASSWORD` | notarization | an app-specific password for that Apple ID (step 4) |

- No `MACOS_CERTIFICATE`: no Developer ID signature and no notarization. The release is
  ad-hoc signed, and Gatekeeper on another Mac refuses to open it until the user allows it
  (right-click > Open, or System Settings > Privacy & Security > Open Anyway).
- The signing secrets but not all three notarization ones: Developer ID signed, not
  notarized. Gatekeeper still warns.
- All six: signed, notarized and stapled; it opens without a warning.

The certificate lives in a temporary keychain for the job only. To check the signing script
locally without any secrets, `scripts/test-release-workflow.sh` runs it against a fixture
bundle along with `actionlint` and checks on the workflow.

## Sparkle

Tenmoku.app can embed Sparkle 2 for auto-updates. It is loaded at run time, so nothing links
against it, and a plain `scripts/bundle-macos.sh` builds a bundle without it.

**Fetching.** `scripts/fetch-sparkle.sh` downloads the release pinned at its top into
`target/sparkle/<version>/` and refuses it unless the SHA-256 matches. `bundle-macos.sh --sparkle`
runs it; `SPARKLE_FRAMEWORK=<path>` uses that framework instead and downloads nothing, with its
licence beside it as `LICENSE`. The framework lands in `Contents/Frameworks/Sparkle.framework`
without `Versions/B/XPCServices`, which only sandboxed apps need, and its licence in
`Contents/Resources/Sparkle-LICENSE.txt`. The generate_keys and sign_update tools stay in the
fetch directory, under `bin/`.

**Signing order.** Inside out: `Versions/B/Autoupdate`, `Versions/B/Updater.app`, the framework,
then the app. All use the hardened runtime and the same identity. The first three get no
entitlements, and nothing uses `--deep`. `--sign-only` follows the same order, so
`scripts/sign-notarize-macos.sh` needs nothing extra.

**The SU\* keys.** `--feed-url` or `TENMOKU_FEED_URL` (https only) and `--ed-public-key` or
`TENMOKU_ED_PUBLIC_KEY` fill in `SUFeedURL` and `SUPublicEDKey`, beside `SUEnableAutomaticChecks`
(true), `SUScheduledCheckInterval` (86400) and `SUAutomaticallyUpdate` (false: Tenmoku asks before
installing). If either is empty, all five are deleted, so that build never polls.

**Library validation.** The hardened runtime only loads a framework signed by the app's own team,
which a Developer ID build satisfies with the committed entitlements. An ad-hoc signature has no
team, so an ad-hoc bundle holding Sparkle is signed with a temporary copy of the entitlements plus
`com.apple.security.cs.disable-library-validation`. That entitlement must never reach a Developer ID
build, and `Tenmoku.entitlements` does not have it.
