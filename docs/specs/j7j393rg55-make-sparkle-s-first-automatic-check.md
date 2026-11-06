---
task_uid: "01m6rgr13sa1s8ebj7j393rg55"
task: "T1390"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "sparkle_tests::first_launch_seeds_the_last_check_time: first_check_seed(None, now) is Some(now), so the first scheduled check is one interval away"
    required: true
    manual: false
  - id: "C2"
    text: "sparkle_tests::a_past_last_check_is_left_alone: first_check_seed(Some(now - 3600.0), now) is None"
    required: true
    manual: false
  - id: "C3"
    text: "sparkle_tests::a_future_last_check_is_reseeded: first_check_seed(Some(now + 86400.0), now) is Some(now)"
    required: true
    manual: false
  - id: "C4"
    text: "rg -n 'first_check_seed' crates/fastedit/src/sparkle.rs shows the macOS platform::start calling it before the updater controller is created, only for Policy::Scheduled"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fastedit sparkle and cargo clippy --workspace --all-targets -- -D warnings pass; the existing sparkle_tests are unchanged"
    required: true
    manual: false
---

# Make Sparkle's first automatic check follow the daily schedule instead of firing at first launch

## Problem
With SUFeedURL and SUPublicEDKey in the Info.plist (SPARKLE_ED_PUBLIC_KEY and a feed set at bundle time), Sparkle checks the feed immediately at the first launch instead of following the daily schedule. The user's steer for Sparkle is "ask before install, daily" (SUAutomaticallyUpdate=false, SUScheduledCheckInterval=86400, SUEnableAutomaticChecks=true are already in crates/fastedit/macos/Info.plist). Cause to confirm against the pinned framework's SPUUpdater headers (Context7 or the framework in the bundle): a fresh install has no SULastCheckTime in the app's user defaults, so Sparkle treats the last check as never and schedules the next one at once; and sparkle.rs's Updater::set_automatic_checks calls setAutomaticallyChecksForUpdates: right after the controller starts (its comment says "before the updater's first scheduled look"), which Sparkle answers by resetting its update cycle.

## Change (crates/fastedit/src/sparkle.rs, sparkle_tests.rs only)
- Portable, pure, testable without the framework: `pub(crate) fn first_check_seed(last_check: Option<f64>, now: f64) -> Option<f64>` (seconds since the Unix epoch). None in means "never checked" or a last check in the future (clock set back): return Some(now), so the first automatic check comes one interval after the first launch. A last check in the past returns None (leave Sparkle's own record alone).
- macOS `platform::start`: before `load` creates the SPUStandardUpdaterController (whose init starts the updater), when the policy is Scheduled, read `SULastCheckTime` from NSUserDefaults standardUserDefaults (an NSDate, via objc2-foundation; add the NSUserDefaults/NSDate features to the objc2-foundation line in crates/fastedit/Cargo.toml only if the crate does not already have them) and, if `first_check_seed` says so, write the seed back with setObject:forKey:. Manual policy needs no seed (no schedule).
- Avoid the reset: at start, call setAutomaticallyChecksForUpdates: only when the updater's current `automaticallyChecksForUpdates` differs from the setting (read the getter first). The settings-observer path keeps calling it on a real change. Check in the framework's behaviour that this does not leave a stale value in user defaults when the setting is off (Manual: SUEnableAutomaticChecks=true in the plist must not win over the user's off setting; keep the unconditional call there).
- No new crate, no real keys: nothing here needs T1368 (the human key task). A real launch is covered by the existing human end-to-end card T1375; do not add a new one.

## Tests
In the existing crates/fastedit/src/sparkle_tests.rs (declared from main.rs): the three pure tests below. The objc path cannot be unit-tested without a signed bundle; say so in the hand-in rather than faking the framework.
