---
task_uid: "01m4eqssg9h61b3tjdtt72nj42"
task: "T211"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config test tests::watch_late::late_user_event_does_not_hide_project_edit passes at least 30 consecutive runs under load (counts and load average recorded in the task notes), and the rest of fe-config's tests::watch and tests::watch_late pass"
    required: true
    manual: false
---

# Flaky under load: fe-config tests::watch_late::late_user_event_does_not_hide_project_edit times out waiting for a watcher batch

# Flaky under load: fe-config tests::watch_late::late_user_event_does_not_hide_project_edit

## Problem

`crates/fe-config/src/tests/watch_late.rs` `late_user_event_does_not_hide_project_edit` times out
under load waiting for a watcher batch: it panics at `crates/fe-config/src/tests/watch.rs:321:50`
with "a batch in time: Timeout" after ~65 s. It has failed 10 gate runs since 2026-10-06 21:01,
most recently T175's run 01m4eq1mq3599d0ckfxrhhh3qn (load 242–272; 1050 of 1051 passed). Earlier
failures include runs 01m49xrfsq58pw86drshc257m9, 01m4dszxfqypfj9gj9f8m9gzfh and
01m4e9tex9r49k6g14a850yscd. Alone it passes in 0.27–0.92 s.

It isn't in the `retries = 2` override in `.config/nextest.toml`, so it gets one try. Its siblings
`watch::hot_reload_applies_valid_and_keeps_last_good` and `watch::symlink_targets_are_watched` also
timed out in that run but passed on their retry. T99 and T109 (both finalized) fixed the
`watch::` siblings under load; this test needs the same treatment.

## Change

Find why the batch doesn't arrive in time under load (a wait that assumes FSEvents latency, an
event coalesced into the user-config event, a registration race), and fix the test the way T109
fixed its siblings. If the cause is in fe-config's watcher rather than the test, say so in the
task notes and stop: a production fix needs a regression test and a respec.

Adding the test to the nextest retries override alone isn't the fix; it may go alongside one.

## Proof

A test-only fix can't fail at base on demand, so prove it by repeated runs under load: run the test
at least 30 times while the machine is loaded (e.g. alongside a full `cargo nextest run` of the
workspace, or `stress`-style CPU load), and record the counts and load average in the task notes.

## Implementation notes

Cause (revised in T212): not established beyond this. `ConfigWatcher` calls the hook after `watch()` returns, and notify 8.2.0's FSEvents `watch()` stops the old stream, appends the path and starts a new one (`FSEventStreamCreate` since-now, then `FSEventStreamStart`, waiting for the runloop), so the hook's write to `a/settings.toml` while registering `c`, the last directory, follows the final stream's start; there is no later registration. So the first version's account of a write landing before the stream for `a` runs doesn't fit. What remains is that a lone write went unreported under load (load 150+ here) even though the stream was started, e.g. fseventsd not having attached the new stream yet; no production defect in `watch.rs` was found, and the exact mechanism wasn't reproduced.
Fix: don't depend on one write. With `await_user`, the helper waits for a batch naming the user file for `RETRY` (1 s) at a time, rewriting the user's target (unchanged content) on each timeout, until `TIMEOUT` overall. The earlier single rewrite after the first batch is gone; the hook write stays as a first stimulus. The test is in the nextest `retries = 2` override.
Proof: 30/30 consecutive passes with 12 `yes` CPU hogs running, load average 170 to 180 (uptime before/after: 169.62 / 179.79); full `watch` filter in fe-config: 7/7 passed.
