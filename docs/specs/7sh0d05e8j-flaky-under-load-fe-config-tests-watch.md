---
task_uid: "01m48t4mjjxqq0zr7sh0d05e8j"
task: "T99"
rev: 3
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "TIMEOUT is 60 s in crates/fe-config/src/tests/watch.rs and crates/fe-keymap/src/tests/load.rs; no production code changes"
    required: true
    manual: false
  - id: "C2"
    text: "fe-keymap tests::load::hot_reload_swaps_the_keymap passes 20 consecutive runs under CPU and file-churn load, with the commands and results recorded in the hand-off"
    required: true
    manual: false
  - id: "C3"
    text: "The fe-config watcher failures under load (60 s timeouts in run 6; the wrong file in a batch at watch.rs:309 in run 7) are recorded in the hand-off verbatim and left to a separate bugfix task the lead files; this task does not change them further"
    required: true
    manual: false
---

# Flaky under load: fe-config tests::watch::{hot_reload_applies_valid_and_keeps_last_good,symlink_targets_are_watched,watcher_follows_links_changed_while_registering} and fe-keymap tests::load::hot_reload_swaps_the_keymap time out waiting for a watcher batch

# Watcher tests time out under load: raise their wall-clock bound

## Symptom

Under heavy load (load average ~70, several lanes building), `rgt ci run --fast`
failed only on the real-FSEvents watcher tests, panicking with
`a batch in time: Timeout`. They pass alone (0.15s for fe-config's
`tests::watch`, 0.04s for fe-keymap's `tests::load`).

Flaky tests (each waits on a real FSEvents batch after a change):

- fe-config `tests::watch::hot_reload_applies_valid_and_keeps_last_good` (via `after_change`)
- fe-config `tests::watch::symlink_targets_are_watched` (via `after_change`)
- fe-config `tests::watch::watcher_follows_links_changed_while_registering`
- fe-keymap `tests::load::hot_reload_swaps_the_keymap`

The bound is `const TIMEOUT: Duration = Duration::from_secs(10)` at
`crates/fe-config/src/tests/watch.rs:24` and
`crates/fe-keymap/src/tests/load.rs:17`, used by `recv_timeout` at
`watch.rs:41`, `watch.rs:304`, `load.rs:55` and `load.rs:79`.

## Cause: the test bound, not production

`ConfigWatcher::with_hook` (`crates/fe-config/src/watch.rs:52-62`) registers
every directory before it returns, and notify 8.2's FSEvents backend blocks in
`watch()` until its loop thread has called `FSEventStreamStart`
(notify `src/fsevent.rs:474-495`). So each test's change is made after the
stream is live; nothing can be missed by ordering. The stream has latency 0,
and dropped or coalesced events arrive as rescans, which `Tracker::batch`
already turns into "every file". What's left is delivery time: with many cargo
builds writing into target directories, fseventsd backs up and a batch can
take longer than 10 s. The comments already say the constant is "an upper
bound only" and no duration is asserted, so raising it costs nothing when the
tests pass.

## Fix (test-only)

- Raise `TIMEOUT` to 60 s in both files. Keep the constant per file (no shared
  helper across crates).
- Keep the "upper bound only" comment; say it's sized for a loaded CI host.
- No production change and no new tests.

## Proof

No regression test: this is a timing bound. Prove it with repeated runs under
load instead, and record the commands and results in the hand-off:

1. Generate load with a synthetic loop, not another lane's CI run (lead's choice:
   reproducible and bounded): 2×ncpu busy loops plus a loop creating and deleting
   thousands of files in a temp directory under the scratch/tmp area, started in the
   background, and killed (process group) as soon as the runs finish. Build the
   test binaries first so the load window is only the test runs; keep the whole
   proof under about 5 minutes.
2. Under that load, run each of the four tests at least 20 times, e.g.
   `cargo test -p fe-config tests::watch` and
   `cargo test -p fe-keymap tests::load` in a loop. All runs must pass.
3. If any run still times out at 60 s, stop and report it rather than raising
   the bound further: a batch that never arrives means an event was lost, a
   production race that needs a bugfix task with a regression test.
