---
task_uid: "01m5aqvpjn1d0ez96n8j7shbzs"
task: "T206"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-lsp test tests::registration::globs::notifications_reach_the_session_in_order passes and calls ManualClock::wait_for_sleep before the advance that releases initialize; cargo nextest run -p fe-lsp passes; clippy -D warnings clean; only files under crates/fe-lsp/src/tests change, besides this lane's docs/specs file"
    required: true
    manual: false
---

# Flake: notifications_reach_the_session_in_order advances the manual clock before the fake is asleep on it (fe-lsp)

## Problem

`tests::registration::globs::notifications_reach_the_session_in_order` (T181) flakes under load: the lane gate of T202 and argon's own sync gate each failed on `fake: timed out waiting for initialized` after the 60 s WAIT (4407 of 4408 passed both times).

The cause is a race on the manual clock. The test starts a fake with `initialize_delay: 1 s` on a `ManualClock`, waits until the fake has *recorded* the `initialize` request, and calls `fake_clock.advance(1 s)`. The fake registers its sleep (`clock.sleep(script.initialize_delay)` in `fake.rs`, `deadline = elapsed + delay`) only after it records the request. If the test's `advance` runs in between, the sleep's deadline becomes 2 s and is never reached, so `wait_initialized()` waits out its 60 s.

## Fix

`ManualClock::wait_for_sleep(left)` already exists (`crates/fe-lsp/src/clock.rs`). In the test, call `fake_clock.wait_for_sleep(Duration::from_secs(1))` after the `fake.wait_for("initialize", ..)` and before `fake_clock.advance(..)`, so the advance can't run before the fake is asleep. Look at the other `advance(` call sites in `crates/fe-lsp/src/tests` that follow a `wait_for(...)` on a request, and give any with the same shape the same wait; change nothing else.
