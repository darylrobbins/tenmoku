---
task_uid: "01m6smp63c9esgxhmh0vr0a5jj"
task: "T1427"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "velopack_update_tests::good_signature_yields_the_feed_and_downloads passes"
    required: true
    manual: false
  - id: "C2"
    text: "velopack_update_tests::tampered_feed_is_refused_before_download passes"
    required: true
    manual: false
  - id: "C3"
    text: "velopack_update_tests::wrong_key_is_refused_before_download passes"
    required: true
    manual: false
  - id: "C4"
    text: "velopack_update_tests::missing_signature_is_refused_before_download passes"
    required: true
    manual: false
  - id: "C5"
    text: "velopack_update_tests::feed_asset_without_sha256_is_refused passes"
    required: true
    manual: false
  - id: "C6"
    text: "velopack_update_tests::asset_outside_the_verified_feed_is_not_downloaded passes"
    required: true
    manual: false
  - id: "C7"
    text: "velopack_update_tests::signer_and_verifier_agree passes"
    required: true
    manual: false
  - id: "C8"
    text: "velopack_update_tests::policy_off_without_build_keys_or_install passes"
    required: true
    manual: false
  - id: "C9"
    text: "Evidence: an ad-hoc signed 0.0.1 from scripts/velopack-pack.sh (channel spike, local feed dir, HOME=\u003cscratch\u003e), run with TENMOKU_VELOPACK_UPDATE_NOW=1 after 0.0.2 is packed into the same feed, ends with the bundle at 0.0.2 (sq.version and CFBundleShortVersionString) and codesign --verify --strict --deep passing; with the .sig deleted, and with one byte of the feed changed, it stays at 0.0.1 and stderr says why"
    required: true
    manual: false
  - id: "C10"
    text: "Evidence: scripts/velopack-pack.sh --dry-run passes --signAppIdentity/--signInstallIdentity/--notaryProfile/--keychain only when VPK_SIGN_APP_IDENTITY/VPK_SIGN_INSTALL_IDENTITY/VPK_NOTARY_PROFILE/VPK_KEYCHAIN are set, refuses --channel osx and stable, and takes the private key only from TENMOKU_UPDATE_PRIVATE_KEY(_FILE) on the signer's stdin; shellcheck scripts/velopack-pack.sh is clean"
    required: true
    manual: false
  - id: "C11"
    text: "velopack is pinned =1.3.39-g8d2844f with public-utils, as an ordinary (not feature-gated) dependency; cargo deny check licenses bans sources passes with deny.toml's only change being a CDLA-Permissive-2.0 exception for webpki-roots; cargo deny check advisories lists no advisory on a crate velopack brings in"
    required: true
    manual: false
  - id: "C12"
    text: "docs/decisions/\u003clane\u003e-velopack-spike.md answers every question in the spec's item 6 from observed results"
    required: true
    manual: false
  - id: "C13"
    text: "git diff main --stat touches none of crates/fastedit/src/sparkle.rs, sparkle_tests.rs, scripts/bundle-macos.sh, scripts/fetch-sparkle.sh, .github/workflows/release.yml, crates/fastedit/macos/Info.plist, crates/fastedit/macos/Tenmoku.entitlements, crates/*/src/lib.rs"
    required: true
    manual: false
---

# Velopack spike: update an ad-hoc signed Tenmoku.app from an Ed25519-signed local feed (velopack crate, our own feed signature check, vpk pack script, findings), beside Sparkle

Velopack spike (user-approved 2026-11-06: "draft the velopack spike card. I have an Apple developer membership"; "We should be able to add our own signature check before updating"). Prove Velopack can update Tenmoku on macOS, with our own Ed25519 check in front of it, before anything replaces Sparkle. **Sparkle stays exactly as it is in this card.**

## Facts checked while drafting (2026-11-06)

- Crate: `velopack` on crates.io, MIT. Latest stable is 1.2.161, but it needs `zip ^8.6`, which has RUSTSEC-2026-0367, -0368, -0369 and -0429 (all fixed only in zip >= 9). **Pin `velopack = "=1.3.39-g8d2844f"`** (2026-10-17). Velopack ships most builds with a git-suffixed version, and this one uses zip 9.x and passes `cargo deny check advisories`. The `vpk` dotnet tool has the same version on NuGet (`vpk 1.3.39-g8d2844f`, MIT, targets net8/9/10). Keep the crate and vpk on the same version.
- Licences: running `cargo deny check licenses bans sources` against the repo's deny.toml on a scratch crate with that pin fails on exactly one crate: `webpki-roots 1.0.11` (CDLA-Permissive-2.0, via `ureq 3.5`). It is Mozilla's root store as data, and unlike `webpki-root-certs` it ships. Add one exception, `{ allow = ["CDLA-Permissive-2.0"], crate = "webpki-roots" }`, with a comment, plus a short THIRD_PARTY.md note beside the shipped-MPL section linking the CDLA-Permissive-2.0 text. **deny.toml is in `human_paths`, so finish raises a human exception. That is expected: leave it.** Everything else is MIT, Apache-2.0, ISC or BSD (ring is "Apache-2.0 AND ISC"). New crates in Cargo.lock: velopack, ureq, ureq-proto, zip, zopfli, xml, glob, normpath, sha1 0.11, typed-path, utf_types, utf8-zero, waitpid-any, derivative.
- `velopack::sources::UpdateSource` is a public trait: `get_release_feed(&self, channel, app: &bundle::Manifest, staged_user_id) -> Result<VelopackAssetFeed, Error>` and `download_release_entry(&self, asset, local_file, progress)`. `bundle::Manifest` is nameable only with the crate's **`public-utils` feature**, which also makes `velopack::download` public. Enable it.
- `UpdateManager::download_updates` checks each downloaded package's size and SHA-256 against the asset that the source's feed returned. **If the asset's SHA256 is empty, it falls back to SHA-1.** The SHA-256s are only as trustworthy as the feed, so the feed is what we sign.
- On macOS, `UpdateManager::new` returns `NotInstalled` unless `<X>.app/Contents/MacOS/UpdateMac` and `sq.version` exist. Only `vpk pack` puts them there. Packages go to `$HOME/Library/Caches/velopack/<packId>/packages`, so `HOME=<scratch>` isolates a test run.
- `VelopackApp::run()` with auto-apply on (the default) applies any newer full package already in the packages dir at startup. `apply_updates_and_restart` calls `exit(0)` straight away, so nothing gets a chance to save.
- `fe_workbench::update_check::set_updater_hook` / `UpdaterHook` already let the app binary take over Check for Updates…, as `crates/fastedit/src/sparkle.rs` does.
- There is a user-local .NET SDK 8.0.303 at `~/.dotnet/dotnet` (not on PATH). Install vpk with `~/.dotnet/dotnet tool install vpk --version 1.3.39-g8d2844f --tool-path ~/.dotnet/tools`. If that fails, `brew install dotnet` (10.0.403, no sudo), and tell the lead so it goes in the Brewfile. Never edit files outside the repo.

## Change

1. **Dependencies.** Root Cargo.toml `[workspace.dependencies]`: `velopack = { version = "=1.3.39-g8d2844f", features = ["public-utils"] }`, `ring = "0.17"` (already in Cargo.lock via rustls) and `base64 = "0.22"` (already in Cargo.lock). Use them from crates/fastedit/Cargo.toml as ordinary dependencies, **not behind a cargo feature**. The gate builds, lints, tests and deny-checks default features only, so code behind a feature would never be checked. The opt-in happens at build time and run time (item 3). These are append-only lines in shared files. Cargo.lock: refresh with `cargo check --workspace`, nothing else.

2. **`crates/fastedit/src/update_signature.rs`** (new, ring + base64 only, no velopack or GPUI, so the signer can share it). The scheme is the same as Sparkle's: Ed25519 over the exact bytes of a file, signature as base64 of the 64 raw bytes, public key as base64 of the 32 raw bytes, private key as base64 of the 32-byte seed (also accept Sparkle's `generate_keys -x` export if its length differs, and say which format it is in the decision file). Provide `PublicKey::from_base64`, `verify(bytes, sig_text) -> Result<(), SignatureError>`, `sign(seed, bytes) -> String` and `generate() -> (public, private)`. Trim the trailing whitespace of the signature text before decoding it.

3. **`crates/fastedit/src/velopack_update.rs`** (new), declared from main.rs as `mod velopack_update;` with `#[cfg(test)] mod velopack_update_tests;`.
   - **Opt-in:** `option_env!("TENMOKU_UPDATE_FEED")` (an https URL, or an absolute local directory for the spike) and `option_env!("TENMOKU_UPDATE_PUBLIC_KEY")`, both baked in at build time. A pure `Policy::decide(feed, key, installed, perf, wait_launch, under_test)`, modelled on `sparkle::Policy`, turns it on only when both values are present and valid (https or an absolute path, refusing http and relative paths, and a key that decodes to 32 bytes) and the process is a Velopack install. Find the bundle from the canonicalized `current_exe` (a symlink in PATH counts), as `sparkle::start` does.
   - **`SignedSource<S: UpdateSource>`:** the hook that does our own check. It has a feed-bytes fetcher (a local dir read, or `velopack::download::download_url_as_string` for https) and an inner source `S` for packages (`FileSource` or `HttpSource`). `get_release_feed` fetches `releases.<channel>.json` and `releases.<channel>.json.sig` and verifies the bytes against the baked key **before parsing them**. It refuses with a clear error on a missing sig, a bad sig, a wrong key, or any asset with an empty SHA256, so velopack never falls back to SHA-1. Then it parses the verified bytes into `VelopackAssetFeed` and remembers the verified assets. `download_release_entry` delegates to `S` only for an asset whose FileName, SHA256 and Size match one in the last verified feed, and refuses anything else (delta assets included). Per-package signatures aren't needed: the signed feed pins each package's SHA-256, and velopack checks it. Explain that chain in the decision file.
   - **Startup:** `velopack_update::startup()` is the first call in `main` (before `cli::parse`, because Velopack's `--veloapp-*` hook arguments would otherwise be unknown flags; `init_logging()` may come first). It runs `VelopackApp::build().set_auto_apply_on_startup(false).run()`, so nothing applies an update that our check hasn't just passed. Then, if `TENMOKU_VELOPACK_UPDATE_NOW=1` is set and the policy is on, it runs headless: check, download, `apply_updates_and_restart`. It prints each step and the outcome on stderr, exits 0 for "no update" and 1 for a refusal or error, and opens no window. This is the agent-drivable demo path, since agents here can't drive the GUI.
   - **Check for Updates…:** in main.rs where `sparkle::start` is called, `if !velopack_update::start(perf, wait_launch, under_test, cx) { sparkle::start(...) }`. When the policy is on, `start` sets an `UpdaterHook`. That hook checks and downloads on the background executor, then on the main thread calls `wait_exit_then_apply_updates(asset, false, true, args)` and dispatches `app.quit` (main.rs's `quit`), so the usual unsaved-files prompt runs. With no update or an error it only logs (no UI in this spike; the follow-up adds toasts). Record what UpdateMac does if the user cancels the quit.

4. **`crates/fastedit/examples/sign_update_feed.rs`** (new): `#[path = "../src/update_signature.rs"] mod update_signature;`. `sign FEED` reads the base64 private key from **stdin only** (never argv) and writes `FEED.sig`. `generate --out-private FILE` writes the private key at mode 600 and prints the public key.

5. **`scripts/velopack-pack.sh`** (new, POSIX sh, macOS, shellcheck-clean). Usage: `--version X.Y.Z --feed DIR|URL --out DIR [--channel NAME] [--pack-id ID] [--dry-run]`.
   - It builds `cargo build --release -p fastedit` with `TENMOKU_UPDATE_FEED` and `TENMOKU_UPDATE_PUBLIC_KEY` in the environment, so they are baked in.
   - It bundles with `scripts/bundle-macos.sh --version X.Y.Z` (no `--sparkle`, no SU* keys), then runs `vpk pack --packId <id> --packVersion X.Y.Z --packDir target/bundle/Tenmoku.app --mainExe tenmoku --channel <channel> --outputDir <out> --signEntitlements crates/fastedit/macos/Tenmoku.entitlements`.
   - It signs `<out>/releases.<channel>.json` with the example, the private key coming from `TENMOKU_UPDATE_PRIVATE_KEY` (env) or `TENMOKU_UPDATE_PRIVATE_KEY_FILE`, piped to its stdin.
   - The channel defaults to `spike`, and the script refuses `osx` (vpk's default on macOS) and `stable`. The pack id defaults to a test id (e.g. `TenmokuSpike`), not the production bundle id.
   - Developer ID signing and notarization are opt-in through `VPK_SIGN_APP_IDENTITY`, `VPK_SIGN_INSTALL_IDENTITY`, `VPK_NOTARY_PROFILE` and `VPK_KEYCHAIN`, passed as `--signAppIdentity`, `--signInstallIdentity`, `--notaryProfile` and `--keychain` only when set. vpk wants the identity without the "(TEAMID)" suffix: say so in `--help`. The script refuses an install identity without an app identity, and a notary profile without both.
   - `--dry-run` prints the cargo, bundle, vpk and sign commands and runs none of them.
   - It refuses a vpk whose `--version` doesn't match the crate pin, and prints the install command.

   No release-workflow job in this card: workflow edits need a human risk approval, so that's the follow-up's.

6. **`docs/decisions/<lane>-velopack-spike.md`** (your lane's id). Answer each of these with what you saw:
   - Does an ad-hoc signed update apply on macOS, and how do vpk's deep signing and ad-hoc signatures interact (is the bundle still `codesign --verify --strict --deep` clean after pack and after update)?
   - `.pkg` vs Portable.zip install: where each lands and whether it needs admin.
   - Delta packages: what a second pack into the same output dir produces, and whether the check takes the delta.
   - The feed format and our `.sig` beside it.
   - Our Ed25519 check vs Velopack's checksum-only integrity vs Sparkle's EdDSA: what each protects against, including replay or freeze of an old signed feed and the auto-apply-on-startup path we turned off.
   - Whether T1368's Sparkle key can be reused as is.
   - Bundle layout changes vs bundle-macos.sh: UpdateMac (size, its own third-party notices), sq.version, Info.plist, entitlements.
   - The TLS stack: ureq + webpki-roots, not fe-net's OS trust store and host grants.
   - The Linux AppImage and Windows Setup.exe outlook with this same source.
   - What the follow-up cards should be: a release job on a non-default channel, UI toasts, retiring Sparkle and T1368/T1369/T1375.

## Tests

Put them in the NEW file `crates/fastedit/src/velopack_update_tests.rs`, as flat functions (no nested module, so the ids stay `velopack_update_tests::<name>`). Use temp dirs as local feeds, with a throwaway keypair from `update_signature::generate`. Make the inner source a spy that counts `download_release_entry` calls.

- `good_signature_yields_the_feed_and_downloads`: the feed is returned. Through `UpdateManager::new(signed, None, Some(VelopackLocatorConfig { .. }))` with a temp RootAppDir, a minimal sq.version and a temp PackagesDir, `check_for_updates` is `UpdateAvailable` and `download_updates` leaves the package in PackagesDir.
- `tampered_feed_is_refused_before_download`: one byte changed after signing. `get_release_feed` errs with a signature error, `check_for_updates` errs, and the spy saw 0 downloads.
- `wrong_key_is_refused_before_download`
- `missing_signature_is_refused_before_download`
- `feed_asset_without_sha256_is_refused`
- `asset_outside_the_verified_feed_is_not_downloaded`
- `signer_and_verifier_agree`: round trip, plus a malformed key or signature is an error, not a panic.
- `policy_off_without_build_keys_or_install`: also off for perf, wait, under test, http and relative feeds.

## Evidence (quote transcripts in the decision file and in the finish evidence)

With a throwaway keypair, `HOME=<scratch>` and the feed in a scratch dir:
1. Pack 0.0.1 and install it from its Portable.zip into the scratch dir.
2. Pack 0.0.2 into the same feed.
3. Run the 0.0.1 binary with `TENMOKU_NO_IPC=1 TENMOKU_VELOPACK_UPDATE_NOW=1`. The bundle ends at 0.0.2 (sq.version and CFBundleShortVersionString), and `codesign --verify --strict --deep` passes.
4. Redo it with the `.sig` deleted, and again with one byte of the feed changed. Each time the bundle stays at 0.0.1 and stderr says why.
5. `scripts/velopack-pack.sh --dry-run`, with and without the VPK_* signing variables, shows the signing flags present only when set.

If macOS refuses to apply an ad-hoc update, don't weaken the check or the criterion: block the card with the exact error.

## Scope notes

Mac-only (vpk pack, codesign): don't lend to Linux. Don't touch crates/*/src/lib.rs, sparkle.rs, sparkle_tests.rs, bundle-macos.sh, fetch-sparkle.sh, release.yml, Info.plist or the entitlements. Nothing secret goes in the repo or in argv.
