# Velopack spike (T1427)

Can Velopack update Tenmoku on macOS, with our own Ed25519 check in front of it,
before anything replaces Sparkle? **Yes.** An ad-hoc signed 0.0.1 updated itself
to 0.0.2 from a signed local feed. Its bundle verified with `codesign --verify
--strict --deep` before and after. A missing `.sig` and a one-byte change to the
feed were each refused before anything was downloaded.

Before users see this:

- **UpdateMac applies the update even when the user cancels the quit** (§ 3.4).
  This is follow-up 1 (§ 11).
- **vpk signs nothing unless it is given an identity** (§ 1), so the script
  requires one, with `-` for ad hoc.
- **Deltas don't pay off yet.** The package velopack rebuilds from a delta never
  matches the signed full package, so every delta update falls back to the full
  download (§ 4).

Sparkle is unchanged. Velopack takes Check for Updates… only in a build that
`scripts/velopack-pack.sh` made with a feed and a key baked in, and only once
that build is installed as a Velopack app. Otherwise `sparkle::start` runs as
before.

Versions: crate `velopack =1.3.39-g8d2844f` with `public-utils`; `vpk
1.3.39-g8d2844f` (a .NET tool, `~/.dotnet/tools/vpk`, run on the user-local .NET
SDK 8.0.303 with `DOTNET_ROOT=~/.dotnet`); macOS 27 on Apple Silicon. Every
transcript below comes from commit e567ee7c, the last code change, with a
throwaway key pair, `HOME=$S/home` and the feed in `$S/feed`, where `$S` is a
scratch directory.

## What was built

- `crates/fastedit/src/update_signature.rs` holds Ed25519 over a file's exact
  bytes, in Sparkle's formats (ring and base64 only).
- `crates/fastedit/src/velopack_update.rs` holds the policy, `SignedSource`, the
  `startup()` hook, the headless `TENMOKU_VELOPACK_UPDATE_NOW=1` path and the
  Check for Updates… hook.
- `crates/fastedit/examples/sign_update_feed.rs` is the signer. It shares the
  signature module through `#[path]`.
- `scripts/velopack-pack.sh` builds with the feed and key baked in, bundles,
  runs `vpk pack` on a non-production channel, and signs the feed.
- `velopack_update_tests` has eight tests: feed signature, downloads, policy and
  signer.

## 1. Does an ad-hoc signed update apply, and how do vpk's signing and ad-hoc signatures interact?

**It applies, but only when vpk does the signing.** `bundle-macos.sh` signs
Tenmoku.app ad hoc, with the hardened runtime and our entitlements. `vpk pack`
then adds `Contents/MacOS/UpdateMac` and `Contents/MacOS/sq.version` (a symlink
to `Contents/Resources/sq.version`). When vpk is given no identity it says
`Package will not be signed or notarized. Missing the --signAppIdentity and
--notaryProfile options.`, and the bundle it packs no longer verifies:

```text
$ codesign --verify --strict --deep TenmokuSpike.app     # first pack, no --signAppIdentity
TenmokuSpike.app: a sealed resource is missing or invalid
```

When vpk is given `--signAppIdentity -`, it logs `Code signing application bundle
recursively (with --deep)...` and signs the whole bundle ad hoc. UpdateMac is
included, and gets our entitlements because `--deep` applies them to nested code
(`com.apple.security.cs.allow-jit` and `allow-unsigned-executable-memory`, flags
`adhoc,runtime`). That bundle verifies. The script therefore **requires
`VPK_SIGN_APP_IDENTITY`** for a real pack, with `-` for ad hoc. It passes
`--signAppIdentity` only when that variable is set. A pack without it is refused
with the reason, and `--dry-run` prints the same reason as a note.

The update applies the package's bundle, which vpk signed, wholesale. So the
bundle is still clean after the update:

```text
$ VPK_SIGN_APP_IDENTITY=- TENMOKU_UPDATE_PUBLIC_KEY=RNYy0L4Pqbeg6FBjWXq1Cu7m5afu5JJNkN3UZKV9ZQM= TENMOKU_UPDATE_PRIVATE_KEY_FILE=$S/key scripts/velopack-pack.sh --version 0.0.1 --feed $S/feed --out $S/feed
exit=0
$S/feed/releases.spike.json.sig
$ codesign --verify --strict --deep <0.0.1 from Portable.zip>
codesign: valid
$ VPK_SIGN_APP_IDENTITY=- ... scripts/velopack-pack.sh --version 0.0.2 --feed $S/feed --out $S/feed
exit=0
== case update: $S/inst-update/TenmokuSpike.app          (0.0.1, unzipped from its Portable.zip)
-- before: sq.version 0.0.1, CFBundleShortVersionString 0.0.1
$ codesign --verify --strict --deep <installed 0.0.1>
codesign: valid
$ HOME=$S/home TENMOKU_NO_IPC=1 TENMOKU_VELOPACK_UPDATE_NOW=1 $S/inst-update/TenmokuSpike.app/Contents/MacOS/tenmoku
velopack: installed 0.0.1 at $S/inst-update/TenmokuSpike.app; checking $S/feed
velopack: feed signature verified; 0.0.2 is available (TenmokuSpike-0.0.2-spike-full.nupkg, 33313442 bytes); downloading
velopack: downloaded; the package's size and SHA-256 match the signed feed
velopack: applying 0.0.2 and restarting
exit status: 0
[12:38:01] [WARN] Failed to wait for process (86157) to exit (No such process (os error 3)). Continuing...
-- after: sq.version 0.0.2, CFBundleShortVersionString 0.0.2
$ codesign --verify --strict --deep $S/inst-update/TenmokuSpike.app
codesign: valid
-- packages downloaded: TenmokuSpike-0.0.2-spike-full.nupkg
```

(UpdateMac's `WARN` line is benign. `apply_updates_and_restart` exits at once, so
the process was already gone when UpdateMac started waiting for it.)

The refused cases:

```text
$ rm $S/feed/releases.spike.json.sig
== case nosig
$ HOME=$S/home TENMOKU_NO_IPC=1 TENMOKU_VELOPACK_UPDATE_NOW=1 $S/inst-nosig/TenmokuSpike.app/Contents/MacOS/tenmoku
velopack: installed 0.0.1 at $S/inst-nosig/TenmokuSpike.app; checking $S/feed
velopack: refused: the feed has no signature: releases.spike.json.sig could not be read ($S/feed/releases.spike.json.sig: No such file or directory (os error 2))
exit status: 1
-- after: sq.version 0.0.1, CFBundleShortVersionString 0.0.1
codesign: valid
-- packages downloaded:
$ changed byte 54 of releases.spike.json: 0.0.2 -> 0.0.3 (still valid JSON)
== case tampered
velopack: installed 0.0.1 at $S/inst-tampered/TenmokuSpike.app; checking $S/feed
velopack: refused: the feed signature check failed for releases.spike.json: the signature does not verify against the public key (changed bytes or a different key)
exit status: 1
-- after: sq.version 0.0.1, CFBundleShortVersionString 0.0.1
codesign: valid
-- packages downloaded:
```

Not covered: Gatekeeper. The installs were unzipped locally, so they had no
quarantine attribute. A downloaded ad-hoc app won't pass Gatekeeper, so real
users need the Developer ID path (`VPK_SIGN_APP_IDENTITY`,
`VPK_SIGN_INSTALL_IDENTITY`, `VPK_NOTARY_PROFILE`; § 11). Its flags appear in
the dry-run below but have not been run.

The relaunch is `open -n <app>` (UpdateMac's log: `Starting application: open
["-n", ".../TenmokuSpike.app"]`). It goes through LaunchServices, so the
relaunched app gets neither our environment (`HOME`, `TENMOKU_*`) nor any
arguments. Two seconds later no process was running from the install directory.
This shell can't see or drive GUI apps, so what the relaunched instance did is
unobserved.

## 2. `.pkg` vs Portable.zip

Every pack writes both, named without a version (`TenmokuSpike-spike-Setup.pkg`,
`TenmokuSpike-spike-Portable.zip`), so each pack overwrites the previous one.
Only the `.nupkg` files keep their versions.

- **Portable.zip** unzips anywhere the user can write and needs no admin. The
  evidence above ran from one. An update replaces the bundle in place (UpdateMac:
  `Replacing bundle at ".../TenmokuSpike.app"`), so the user needs write access
  to the bundle's parent folder.
- **Setup.pkg** was expanded with `pkgutil --expand`, not installed, because
  installing writes to `/Applications` and asks for an admin password. Its
  `PackageInfo` has `install-location="/Applications" auth="root"`. Its
  `Distribution` enables `enable_localSystem` and `enable_currentUserHome`. Its
  `postinstall` deletes `/tmp/velopack/TenmokuSpike` and the user's
  `~/Library/Caches/velopack/TenmokuSpike`, then runs `open` on the app with
  `VELOPACK_FIRSTRUN=1`. vpk didn't sign it ("No Installer signing identity
  provided"). Once installed in `/Applications`, later updates replace the bundle
  as the user, which works only for an admin user, since `/Applications` is
  admin-group writable.
- **The app is renamed to `<packId>.app`.** Both artifacts hold
  `TenmokuSpike.app`, not `Tenmoku.app`. Info.plist, including
  `CFBundleIdentifier` `dev.tenmoku.tenmoku`, is byte-identical to
  bundle-macos.sh's. A production pack id must be the name we want in
  `/Applications`, which is `Tenmoku`.

## 3. Startup, Check for Updates… and the cancelled quit

1. **Startup.** `velopack_update::startup()` comes before `cli::parse`, so
   Velopack's `--veloapp-*` hook launches never reach our CLI. It runs
   `VelopackApp::run()` only for a Velopack install. Otherwise it would log an
   error on every `cargo run`. It passes a locator built from the canonicalized
   `current_exe`, because velopack's own locator looks for `.app/` in the
   uncanonicalized path, and a `tenmoku` symlink in PATH has none.
   `VelopackApp::build()` itself collects `env::args()`, which panics on an
   argument that isn't UTF-8, so a launch with one skips Velopack's startup
   altogether. It can't be a hook launch, whose arguments are Velopack's own, and
   the OsString-aware CLI then takes it as before. The
   `installed_launch_with_a_non_utf8_argument_does_not_panic` test launches the
   real binary from a fake install, once with a hook argument and once with such
   an argument. Velopack's own log lines are shown at error level only: it warns
   about the explicit locator on every launch. `RUST_LOG` still shows
   everything.

   The headless `TENMOKU_VELOPACK_UPDATE_NOW=1` path runs right after
   `cli::parse`, before IPC, through `update_now_if_asked(perf, wait_launch)`.
   So a `--perf` or `--wait` launch, or the detached server of one, never checks:
   `excluded_launches_never_reach_the_check` covers it.
2. **Auto-apply on startup is off** (`set_auto_apply_on_startup(false)`). By
   default `run()` applies any newer full package it finds in
   `~/Library/Caches/velopack/<id>/packages` with no check at all
   (`local_path_to_asset` leaves SHA-1 and SHA-256 empty), then exits. That's
   anything a same-user process drops there. Now only a package that our check
   has just passed is applied.
3. **Check for Updates…** If `velopack_update::start` returns true, the
   `UpdaterHook` is ours and `sparkle::start` isn't called. The check and
   download run on the background executor, and one check runs at a time. On the
   main thread, `wait_exit_then_apply_updates(asset, false, true, [])` starts
   UpdateMac, and then `quit` (the Quit command, with its unsaved-files prompt)
   runs. With no update, or on an error, the hook only logs; the spike has no
   toasts. The GUI path couldn't be driven here. The tests and the headless path
   cover the same check and download code.
4. **If the user cancels the quit, the update is applied anyway, under the
   running app.** UpdateMac waits 60 s for the process, then carries on. To
   observe this I ran UpdateMac's apply exactly as `wait_exit_then_apply_updates`
   builds it, with `--waitPid` pointing at a process that doesn't exit:

   ```text
   $ UpdateMac apply --package <0.0.2 full> --waitPid <a process that stays, as after a cancelled quit> --root <app> --packageDir <packages> --norestart
   [09:25:28] [INFO] Waiting Some(60s) for process (1463) to exit.
   [09:26:28] [WARN] Failed to wait for process (1463) to exit (Parent process timed out.). Continuing...
   [09:26:30] [INFO] Replacing bundle at "$S/inst-cancel/TenmokuSpike.app"
   [09:26:30] [INFO] Package version 0.0.2 applied successfully.
   UpdateMac exit=0 after 63s
   sq.version: 0.0.2
   ```

   The spec fixes the order (UpdateMac first, then quit), and the spike keeps
   it. The follow-up should start UpdateMac only once the quit is certain, from
   `on_app_quit` after the prompt has passed, or wait for the user's choice
   before starting it. Until then the hook is reachable only in a spike build.

## 4. Delta packages

A second pack into the same output directory adds
`TenmokuSpike-0.0.2-spike-delta.nupkg` (5,826 bytes, against 33,313,442 for the
full package; the binaries were identical, so only the version files changed)
beside the full package. It lists both in the feed with their own SHA-256s.

**Velopack checks each delta's SHA-256, but not the full package it rebuilds
from them.** In `download_updates`, the delta path never calls
`verify_package_checksum` on the result. So `download_checked` checks the full
package left in the packages directory against the signed feed's size and
SHA-256. A package that came from deltas and fails the check is deleted, and
the signed full package is downloaded and checked instead. A full package that
fails is refused. `delta_rebuild_that_fails_the_check_falls_back_to_the_full_package`
covers both outcomes, with a stand-in UpdateMac `patch`. Deltas are downloaded
only when they are in the verified feed, like any other asset: `SignedSource`
refuses an asset that isn't in that feed, deltas included, and passes on only
the deltas the feed lists. Velopack then checks each delta's SHA-256.

Whether the check takes the delta depends on the packages directory:

- **A fresh Portable.zip install has no base** (its packages directory is empty),
  so velopack offers only the full package. That is the 0.0.1 → 0.0.2 case in
  § 1.
- **After one update, the applied full package stays in the packages directory**
  (`TenmokuSpike-0.0.2-spike-full.nupkg`), so the next update tries the delta.
  **The rebuilt package doesn't match the packed one, so the delta saves
  nothing.** On the final code (commit e567ee7c), a third pack (0.0.3) applied
  to the install that had just updated to 0.0.2, with `RUST_LOG=info`:

  ```text
  -- before: sq.version 0.0.2; packages: TenmokuSpike-0.0.2-spike-full.nupkg
  [INFO velopack::manager] Found 1 delta updates between 0.0.2 and 0.0.3.
  velopack: feed signature verified; 0.0.3 is available (TenmokuSpike-0.0.3-spike-full.nupkg, 33313444 bytes); downloading
  velopack: trying 1 delta package(s) on the cached 0.0.2
  [INFO velopack::manager] Beginning delta update process.
  [INFO velopack::manager] Downloading delta package: 'TenmokuSpike-0.0.3-spike-delta.nupkg'
  [INFO velopack::manager] Applying 1 patches to "$S/home/Library/Caches/velopack/TenmokuSpike/packages/TenmokuSpike-0.0.3-spike-full.partial".
  [INFO velopack::manager] Successfully applied delta updates.
  velopack: the package TenmokuSpike-0.0.3-spike-full.nupkg does not match the signed feed's size and SHA-256, so it was deleted; downloading the full package instead
  [INFO velopack::sources::file] About to copy from file '"$S/feed/TenmokuSpike-0.0.3-spike-full.nupkg"' to file '"…/TenmokuSpike-0.0.3-spike-full.partial"'
  velopack: downloaded; the package's size and SHA-256 match the signed feed
  velopack: applying 0.0.3 and restarting
  [12:38:39] [INFO] Package version 0.0.3 applied successfully.
  -- after: sq.version 0.0.3, CFBundleShortVersionString 0.0.3
  $ codesign --verify --strict --deep $S/inst-update/TenmokuSpike.app
  codesign: valid
  ```

  The delta reconstructs the package's files but writes a new zip
  (`patch.rs` in velopack's update binary), whose bytes differ from the zip
  `vpk pack` made. A content-level check is what would let deltas pay off: the
  signed feed (or a signed manifest beside it) would list each file's hash, and
  the check would compare the extracted files instead of the zip. Until then,
  with our check in front, deltas cost one small extra download and save
  nothing. Disabling them (`UpdateOptions::MaximumDeltasBeforeFallback = -1`)
  is the simple alternative. Follow-up 4 (§ 11) covers both.

## 5. The feed and our `.sig`

vpk writes `releases.<channel>.json`, which is what the crate reads:

```json
{"Assets":[
  {"PackageId":"TenmokuSpike","Version":"0.0.2","Type":"Full","FileName":"TenmokuSpike-0.0.2-spike-full.nupkg",
   "SHA1":"705FB744…","SHA256":"1FFB4DDF…","Size":33227488},
  {"PackageId":"TenmokuSpike","Version":"0.0.2","Type":"Delta","FileName":"TenmokuSpike-0.0.2-spike-delta.nupkg", …},
  {"PackageId":"TenmokuSpike","Version":"0.0.1","Type":"Full","FileName":"TenmokuSpike-0.0.1-spike-full.nupkg", …}]}
```

It writes three more files, which we don't read and don't sign. `assets.<channel>.json`
lists the files to upload. `RELEASES-<channel>` is the legacy Squirrel index,
with SHA-1 only. `.nupkg` is a zip of the bundle.

Beside the feed, `releases.<channel>.json.sig` holds one line: the base64 of the
64-byte Ed25519 signature over the feed file's exact bytes, plus a newline (89
bytes). It has the same shape as a Sparkle `edSignature`. The signer
(`sign_update_feed sign FEED`) reads the private key from stdin only. A key given
on argv is a usage error. It prints the key's public half, and the script refuses
to leave a `.sig` whose key doesn't match `TENMOKU_UPDATE_PUBLIC_KEY`.

**The chain of trust:**

1. The baked public key verifies the `.sig` over the feed's bytes, before they
   are parsed.
2. The verified feed must give every asset a SHA-256. Without one, velopack
   would fall back to SHA-1.
3. `SignedSource` downloads only assets that match the verified feed by name,
   SHA-256 and size.
4. Velopack checks each download's size and SHA-256.
5. We check the final full package again (§ 4).

Per-package signatures would add nothing, because the signed feed already pins
each package's SHA-256.

## 6. Our Ed25519 check vs Velopack's checksums vs Sparkle's EdDSA

| | Velopack alone | Velopack + our feed signature | Sparkle (as T1374 releases it) |
|---|---|---|---|
| What is signed | nothing | the feed, which pins every package's SHA-256 and size | each update archive (`sparkle:edSignature` in the appcast); the appcast itself is not signed |
| Someone who controls the feed host or TLS (§ 9) | can serve any package with matching hashes: arbitrary code | can serve only feeds we signed | can't change an archive, but can choose which signed archive the appcast offers |
| Replay of an old signed feed | n/a | **freeze**: users stay on that version. Downgrade is refused: `AllowVersionDowngrade` is false, so an older feed reads as "nothing newer" | freeze, the same way (no downgrade by default) |
| Withholding the feed or `.sig` | no update | refused, no update (a freeze) | no update |
| Package swap after download (local) | the auto-apply-on-startup path applies any newer full package from the cache with no check | auto-apply off; only a just-verified package is applied, and its SHA-256 is re-checked | Sparkle re-checks the archive's EdDSA signature and the new app's code signing |
| New app's code signature | not checked | not checked (only its bytes, through the hash) | checked against the old app's |

Neither design stops a freeze, because the signed feed has no expiry and no
counter. A follow-up can add a signed `issued` time or sequence to the feed and
have the client keep the highest it has seen. A user-level process can still
replace the bundle or the cache directly, which is the same trust boundary as
the app itself. Turning off auto-apply removes the one path where Velopack
applied a package nobody had checked.

## 7. Reusing T1368's Sparkle key

**Yes, as is.** For a key it makes, Sparkle 2's `generate_keys` keeps only the
32-byte seed, and `-x` exports the base64 of that keychain item unchanged (in
Sparkle's `generate_keys/main.swift`, newly generated keys store only the seed,
and import accepts "32 or 96 bytes (for the older format)"). That 32-byte seed is
our private key format, and the 32-byte public key `generate_keys` prints is our
`TENMOKU_UPDATE_PUBLIC_KEY` format. T1368 generates its key with Sparkle 2.10.1
(`scripts/fetch-sparkle.sh`), so the `SPARKLE_ED_PRIVATE_KEY` secret can go
straight into `TENMOKU_UPDATE_PRIVATE_KEY`, and the `SPARKLE_ED_PUBLIC_KEY`
variable into `TENMOKU_UPDATE_PUBLIC_KEY`.

I checked this against Sparkle's source and our tests, not against the real
secret, which only a human holds.

Our parser also takes 64 bytes (seed followed by public key, libsodium's layout,
with the halves checked to agree). It refuses Sparkle's legacy 96-byte format
with a message that names it: that format holds an expanded key with no seed,
and ring can't sign from one.

Both schemes sign raw bytes with no domain separation. That is harmless in
practice: an archive is not a JSON feed, and a feed is not an archive. A
separate key once Sparkle is retired would still be cleaner.

## 8. Bundle layout vs bundle-macos.sh

- **New files:** `Contents/MacOS/UpdateMac`, `Contents/Resources/sq.version`
  (the nuspec: id, version, channel, `mainExe` `Contents/MacOS/tenmoku`), and
  `Contents/MacOS/sq.version`, a symlink to it. `UpdateMac` is 9.1 MB (9,490,192
  bytes from vpk; 9,513,216 once signed), about 12% on top of the 76.9 MB
  `tenmoku`.
- **UpdateMac's notices.** It is Velopack's Rust updater (MIT), statically
  linked, and no licence or third-party notice for it or its crates goes into
  the bundle. vpk's `vendor/THIRD_PARTY_NOTICES.md` covers only vpk's own vendor
  tools (signtool, zstd, the AppImage runtime, mksquashfs, WiX). Shipping it
  needs its notices generated, from its `Cargo.lock` at the tag, into
  `Contents/Resources` and THIRD_PARTY.md: a follow-up.
- **Info.plist** is unchanged, byte for byte (`plutil -convert xml1` diff).
- **Entitlements:** our `Tenmoku.entitlements` through `--signEntitlements`.
  With `--deep`, UpdateMac gets them too.
- **Bundle name:** `<packId>.app` (§ 2).

## 9. TLS: ureq + webpki-roots, not fe-net

Velopack fetches over its own stack: `ureq` 3.5 with rustls (ring) and
`webpki-roots` 1.0.11, Mozilla's roots compiled in. fe-net, in contrast, checks
certificates against the OS trust store (`rustls-platform-verifier`), sends
nothing to hosts without a `net:` grant, and follows no redirects. With ureq:

- Roots a company installs aren't trusted, and the root set ages with the
  binary.
- There is no host grant. The feed URL is baked in at build time, but a package's
  URL is the feed's base joined with its `FileName`, which a signed feed could
  point anywhere. ureq follows redirects by its defaults.
- With `HttpOptions::default()` there is no timeout. The spike sets 30 s for the
  feed and its `.sig` and 30 min for a package
  (`FEED_TIMEOUT_MS`/`PACKAGE_TIMEOUT_MS`).

Integrity doesn't depend on TLS, because of the signed hashes. Availability and
privacy do. `webpki-roots` needs a CDLA-Permissive-2.0 exception in deny.toml
and a THIRD_PARTY.md note. ureq stays linked even if a follow-up writes an
`UpdateSource` over fe-net, because velopack depends on it unconditionally.

Advisories: `cargo deny check advisories` fails on this branch only for
`pulldown-cmark` (RUSTSEC-2026-0433, via fe-markdown), which is already on main.
No crate velopack brings in has an advisory. `cargo deny check licenses bans
sources` passes.

## 10. Linux AppImage and Windows Setup.exe with this source

Not run here; this is from the crate's source and vpk's help.

- `SignedSource` and the feed signature are platform-neutral: an
  `UpdateSource` over any fetcher.
- What's macOS-only is `Install::locate` (the `.app` layout).
- **On Linux**, velopack's locator finds the AppImage through `$APPIMAGE`, and
  UpdateNix replaces the AppImage file.
- **On Windows**, velopack's locator expects `Update.exe` in the install root
  beside `current\`, and Setup.exe runs the `--veloapp-install`/`--veloapp-updated`
  hooks, which `startup()` already handles before the CLI.
- **On both**, `startup()` would need `Install::locate` for that layout, or
  could use velopack's auto-locate, since the symlink concern is macOS-specific.
- **Packing:** vpk ships `UpdateNix_*`, `update*.exe`, `setup*.exe` and the
  AppImage runtimes in its vendor directory. Whether it cross-packs from macOS is
  unverified.
- **Signing:** Windows needs Authenticode, through vpk's Windows signing
  options (not looked at here). An AppImage is unsigned, so there the feed signature is the only
  check.

## 11. Follow-up cards

1. **Apply only after a certain quit** (§ 3.4). Start UpdateMac from
   `on_app_quit`, after the unsaved-files prompt, or wait for the user's choice.
   This is needed before any UI points users at Check for Updates….
2. **A release job on a non-default channel** (`beta`, never `osx`/`stable`
   until Sparkle retires). It should:
   - run `scripts/velopack-pack.sh` with Developer ID signing and notarization
     (`VPK_SIGN_APP_IDENTITY`, `VPK_SIGN_INSTALL_IDENTITY` without the
     "(TEAMID)" suffix, and `VPK_NOTARY_PROFILE`/`VPK_KEYCHAIN` on the runner);
   - take the private key from a secret into `TENMOKU_UPDATE_PRIVATE_KEY`;
   - upload the feed, `.sig`, `.nupkg`s, zip and pkg next to the previous
     release's packages, so deltas keep working;
   - install vpk at the crate's pin.

   A workflow edit, so it needs a human risk approval.
3. **UI:** toasts for "no update", progress, refusal reasons and "restart to
   update", plus an `update.automatic_checks` schedule like Sparkle's.
4. **Feed freshness and deltas:** a signed issue time or sequence against
   freeze attacks (§ 6). Either per-file hashes in what we sign, so a package
   rebuilt from deltas can be checked by content, or deltas off (§ 4).
5. **Shipping hygiene:** UpdateMac's notices (§ 8), a production pack id that
   gives `Tenmoku.app` (§ 2), and Gatekeeper on a quarantined download with
   Developer ID.
6. **Retiring Sparkle** once 1–5 land and a human end-to-end check passes:
   - remove `sparkle.rs`, `sparkle_tests.rs`, `fetch-sparkle.sh`,
     `bundle-macos.sh --sparkle`, the SU* keys and release.yml's appcast step;
   - **T1368** (key) carries over: reuse the key (§ 7) or make a new one with
     `sign_update_feed generate`;
   - **T1369** (where updates are served) still has to be decided, now for the
     Velopack feed;
   - **T1375** (human Sparkle end-to-end) becomes the same check against a
     Velopack feed with a Developer ID build.
7. **Optional:** an `UpdateSource` over fe-net (OS trust store, grants, no
   redirects), § 9.
