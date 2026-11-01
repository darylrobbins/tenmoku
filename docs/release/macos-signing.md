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
