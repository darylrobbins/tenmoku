---
task_uid: "01m4k7292ejfpdsx6pp4jaa2xa"
task: "T325"
rev: 3
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The hand-in confirms from the code that the failure is the test's own 60 s wait for FSEvents delivery, and that neither KeymapStore::reload nor ConfigWatcher loses the change. If either can lose it, the work stops and this is reported as a production bug to re-file as a bugfix"
    required: true
    manual: false
  - id: "C2"
    text: "hot_reload_swaps_the_keymap still starts a ConfigWatcher on store.paths() and asserts its first batch is [path]. After each replace it calls store.reload(\u0026[path]) directly, asserts the result is Some, and keeps every existing matcher and diagnostics assertion, including the error at (2, 13). reload_until is deleted"
    required: true
    manual: false
  - id: "C3"
    text: "The retries = 2 filter in .config/nextest.toml no longer names package(fe-keymap) and test(=tests::load::hot_reload_swaps_the_keymap) (a stale line since kiln's K1.3a move), and its comment's count of tests goes from seven to six"
    required: true
    manual: false
  - id: "C4"
    text: "No production code changes. Only kiln/crates/kiln-core/src/tests/keymap/load.rs and .config/nextest.toml change, apart from the task's own spec file under docs/specs/, which Regatta commits"
    required: true
    manual: false
  - id: "C5"
    text: "10 consecutive runs of scripts/ci-env.sh cargo nextest run --profile ci -p kiln-core -E 'test(=tests::keymap::load::hot_reload_swaps_the_keymap)' have 0 FAIL while the load average is at least twice the core count (background load added when the machine is quieter than that). The hand-in records the pass count, each run's duration, the load average and fseventsd's %CPU. The load generator sets trap 'kill $(jobs -p) 2\u003e/dev/null' EXIT INT TERM before it starts, each loop stops itself, and no process is left behind"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p kiln-core passes"
    required: true
    manual: false
---

# Flaky under load: kiln-core tests::keymap::load::hot_reload_swaps_the_keymap: feed KeymapStore::reload directly instead of waiting on FSEvents delivery

# Flaky under load: kiln-core tests::keymap::load::hot_reload_swaps_the_keymap

## Problem

`hot_reload_swaps_the_keymap` (then `crates/fe-keymap/src/tests/load.rs`, now `kiln/crates/kiln-core/src/tests/keymap/load.rs`) failed all 3 nextest tries in T147's
finish gate, run 01m4k54rjct1nckhv8j3vwwssd (2026-10-10 ~11:05), each at its own 60 s deadline:

    panicked at crates/fe-keymap/src/tests/load.rs:55:51:
    a batch in time: Timeout

Line 55 is `reload_until`'s `recv_timeout`: the watcher's first batch arrived, and the FSEvents batch
for the first `replace` never did. 1284 of 1285 tests passed. It passes alone in about 1 s. At the time
the load average was about 235–247 on this 14-core Mac and fseventsd ran at 99–115% CPU.

It isn't new. Since 2026-10-06 it has had a failed try in 33 gate runs (of about 1,100 under
`~/.local/state/regatta/runs/`). Its retry passed in all but three, which failed all 3 tries:
01m49hw04yxft2bvkqhn5c232c (10-06), 01m4e9tex9r49k6g14a850yscd (10-08) and this one. That rate is
similar to the fe-config and fe-project FSEvents tests'.

T99 already raised its deadline to 60 s. T109 already put it in the `retries = 2` override in
`.config/nextest.toml`. T300's fix (60 s plus retries) is therefore already in place here, and it
isn't enough: when fseventsd is backed up for minutes, three back-to-back 60 s tries all miss.

## Cause: the test waits on delivery it doesn't need

As T99, T109 and T300 found, FSEvents delivery on this Mac has no bound when fseventsd is saturated
(T109's probe: p99 20–46 s, some writes not delivered within 40 s, no rescan events). No deadline fixes
that.

This test doesn't need delivery. What it tests is `KeymapStore::paths` and `KeymapStore::reload`
(`kiln/crates/kiln-core/src/load.rs`). Delivering a rename-over save as a batch is `fe_config::ConfigWatcher`'s
job (now `kiln_core::ConfigWatcher`), and fe-config's own real-FSEvents tests cover it, retries included
(`tests::watch::hot_reload_applies_valid_and_keeps_last_good` does the same temp-write-and-rename).
`ConfigWatcher`'s first batch is built from `Tracker::files()` and sent from its thread before any event
(`kiln/crates/kiln-core/src/watch.rs`, `with_hook`), so waiting for it doesn't depend on fseventsd.

If you find while reading that `KeymapStore::reload` or `ConfigWatcher` really loses the change, stop.
Report it as a production bug to re-file as a bugfix with a regression test.

## Change (test and test config only)

1. In `hot_reload_swaps_the_keymap`, keep the `ConfigWatcher::new(store.paths(), …)` start and its
   first-batch assertion (`first == [path]`). After each `replace`, call `store.reload(&[path.clone()])`
   directly instead of `reload_until`. Assert it returns `Some` and feed `event.keymap` to the matcher.
   Keep every existing assertion on the matcher and diagnostics, including the invalid edit's error at
   (2, 13) with `cmd-j` still `editor.duplicate_line`. Delete `reload_until`. Update the module doc and
   `TIMEOUT`'s comment to say that the only wait is for the watcher's first batch, which doesn't wait on
   FSEvents delivery.
2. In `.config/nextest.toml`, remove `(package(fe-keymap) and test(=tests::load::hot_reload_swaps_the_keymap))`
   from the `[[profile.ci.overrides]]` `retries = 2` filter, and change the comment's count of tests
   from seven to six. (Since kiln's K1.3a move that line no longer matches the test, which is now
   kiln-core's `tests::keymap::load::hot_reload_swaps_the_keymap`, so the test has had no retry since.) The test no longer uses FSEvents delivery, so a retry would only hide a real
   failure. This depends on T300, which edits the same override.

## Proof

A test-only fix can't fail at base on demand, so prove it with repeated runs under load. Build first.
Then start the load generator from a script that runs `trap 'kill $(jobs -p) 2>/dev/null' EXIT INT TERM`
before it starts any load. Each busy loop must also stop itself, for example `perl -e 'alarm 600; 1 while 1'`.
The loops hold the load average at about twice the core count (about 28 on this 14-core Mac). Under that
load, run

    scripts/ci-env.sh cargo nextest run --profile ci -p kiln-core -E 'test(=tests::keymap::load::hot_reload_swaps_the_keymap)'

10 times, and no more. Record in the hand-in the pass count, each run's duration, the load average and
fseventsd's %CPU (`ps -o %cpu= -p $(pgrep -x fseventsd)`). Leave no load process behind.

## Implementation notes

- C1: `KeymapStore::reload` (kiln-core `load.rs`) unconditionally re-reads the file when its path is in `changed`, so it cannot lose a change. `ConfigWatcher::with_hook` (`watch.rs`) builds its first batch from `Tracker::files()` and sends it from its thread before any event, and a periodic reconcile re-stats files as a backstop. The failure is the test's own 60 s wait for FSEvents delivery. No production bug.
- C5: 10/10 passed, 0 FAIL, on 14 cores. The load average was already over 2x the core count (28), so no load generator was started and no load process was left behind. Load is the 1/5/15-minute averages read at the start of each run.

  | run | duration | load average | fseventsd %CPU |
  |---|---|---|---|
  | 1 | 0.074 s | 129.14 145.09 105.63 | 55.3 |
  | 2 | 0.041 s | 129.14 145.09 105.63 | 56.6 |
  | 3 | 0.031 s | 129.14 145.09 105.63 | 51.4 |
  | 4 | 0.024 s | 123.92 143.75 105.38 | 32.9 |
  | 5 | 0.019 s | 123.92 143.75 105.38 | 2.0 |
  | 6 | 0.036 s | 123.92 143.75 105.38 | 2.0 |
  | 7 | 0.051 s | 123.92 143.75 105.38 | 0.9 |
  | 8 | 0.030 s | 118.00 142.19 105.06 | 1.6 |
  | 9 | 0.042 s | 118.00 142.19 105.06 | 69.3 |
  | 10 | 0.034 s | 118.00 142.19 105.06 | 81.6 |
