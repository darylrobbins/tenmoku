---
task_uid: "01m6j60x2gwhev67rhs60y6tj9"
task: "T1373"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "sparkle_tests::framework_path_inside_a_bundle passes"
    required: true
    manual: false
  - id: "C2"
    text: "sparkle_tests::policy_off_for_perf_wait_test_and_missing_framework passes"
    required: true
    manual: false
  - id: "C3"
    text: "sparkle_tests::policy_manual_when_setting_off passes"
    required: true
    manual: false
  - id: "C4"
    text: "sparkle_tests::policy_scheduled_by_default passes"
    required: true
    manual: false
  - id: "C5"
    text: "sparkle_tests::policy_off_without_feed_keys passes"
    required: true
    manual: false
  - id: "C6"
    text: "rg -n 'sparkle::' crates/fastedit/src/main.rs shows one start call, guarded by perf and the wait-server check and not run under is_test()"
    required: true
    manual: false
  - id: "C7"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean and cargo deny check bans passes (no new crate)"
    required: true
    manual: false
---

# Start Sparkle at launch from the bundled framework: runtime load, Check for Updates… routed to it, update.automatic_checks applied; off for dev builds, --wait, --perf and tests

## Problem

With Sparkle.framework in the bundle (card A), the app must load it, start Sparkle's standard updater at launch, route Check for Updates… to it, and obey `update.automatic_checks`. Dev builds, `cargo run`, tests, Linux and bundles without the framework must keep working untouched.

## Change

New `crates/fastedit/src/sparkle.rs`, declared in main.rs, macOS-only code behind `#[cfg(target_os = "macos")]` (the pure policy is portable):

- `framework_path(exe: &Path) -> Option<PathBuf>`: `<exe>/../../Frameworks/Sparkle.framework` when the exe sits in `X.app/Contents/MacOS/`, and the directory exists. Else None (cargo run, tests, Linux).
- `Policy::decide(bundled_framework: bool, has_feed_keys: bool, perf: bool, wait_launch: bool, under_test: bool) -> Policy::{Off, Manual, Scheduled}`: Off for perf/wait/test or no framework; Manual (Sparkle started with `startingUpdater:true` but automaticallyChecksForUpdates forced off) when the settings say off; Scheduled otherwise. Feed keys come from the bundle's Info.plist (SUFeedURL, SUPublicEDKey present); without them Off.
- the objc part (objc2 0.5 as the crate already uses; add `NSBundle`, `NSString`, `NSDictionary` features to the `objc2-foundation` line in `crates/fastedit/Cargo.toml` as needed, no new crate): `NSBundle::bundleWithPath` the framework, `load()`, `NSClassFromString('SPUStandardUpdaterController')`, `alloc` + `initWithStartingUpdater:updaterDelegate:userDriverDelegate:` via `msg_send!` with `true, nil, nil`, retained for the process life, on the main thread (`MainThreadMarker`). `updater.setAutomaticallyChecksForUpdates:` follows the setting at start and on every settings reload (find how main.rs applies other live settings, e.g. theme). A failed load logs one `log::warn!` and leaves the GitHub fallback in place (this is the expected result for an ad-hoc bundle missing library validation, and for dev bundles).
- Check for Updates…: call `update_check::set_updater_hook` (card C) with a closure that sends `checkForUpdates:` to the controller and returns true; not installed when Policy is Off.
- main.rs wires it once, after the app icon call (`app_icon::set()`), never when `cli.perf` or `cli.wait`/the detached wait server (`how.waits_by_itself`), never under `is_test()`.

## Mac-only

The objc code and the unit tests compile on macOS only; Policy tests are portable. Real loading is verified by the human end-to-end card, not here.

## Tests

New `crates/fastedit/src/sparkle_tests.rs`: `framework_path_inside_a_bundle`, `framework_path_none_outside_a_bundle`, `policy_off_for_perf_wait_test_and_missing_framework`, `policy_manual_when_setting_off`, `policy_scheduled_by_default`, `policy_off_without_feed_keys`. Plus `rg -n 'sparkle::' crates/fastedit/src/main.rs` shows the single call guarded by perf/wait.
