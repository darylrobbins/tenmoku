---
task_uid: "01m48vw5k3apkytbqh4skzqbjn"
task: "T109"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "watcher_follows_links_changed_while_registering accepts later batches that are non-empty in-order subsets of [user, project] and loops until the project's edit is loaded; the first-batch assertion is unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "A new deterministic test in crates/fe-config/src/tests/watch_late.rs, whose hook touches the user's target when the last directory is registered, passes; the old strict loop would fail it"
    required: true
    manual: false
  - id: "C3"
    text: "The ci nextest profile retries exactly the four real-FSEvents tests (fe-config's three, fe-keymap tests::load::hot_reload_swaps_the_keymap) twice; TIMEOUT stays 60 s; no production code changes"
    required: true
    manual: false
  - id: "C4"
    text: "Under T99's synthetic load, 20 runs of the filtered nextest ci command have 0 FAIL; FLAKY count and fseventsd %CPU are recorded in the hand-off"
    required: true
    manual: false
---

# Flaky under load: fe-config tests::watch::{watcher_follows_links_changed_while_registering,hot_reload_applies_valid_and_keeps_last_good,symlink_targets_are_watched} assume prompt, start-bounded FSEvents delivery

# Real-FSEvents watcher tests assume prompt, start-bounded delivery: fix the tests

## Symptom

Under load, after T99's 60 s bound (T99 is landing fe-keymap only):

- run 6: `hot_reload_applies_valid_and_keeps_last_good`, `symlink_targets_are_watched` and
  `watcher_follows_links_changed_while_registering` all panicked `a batch in time: Timeout`
  in the same run.
- run 7: `watcher_follows_links_changed_while_registering` failed at
  `crates/fe-config/src/tests/watch.rs:309` with a batch naming `user/settings.toml`.
- Reproduced at base on main with no synthetic load (load average ~100 from other lanes,
  fseventsd at ~100% CPU): `cargo test -p fe-config tests::watch` failed 5 of 5 runs at
  watch.rs:309 with `left: [.../user/settings.toml, .../project/settings.toml]`,
  `right: [.../project/settings.toml]`. Minutes later, with fseventsd less backed up, 0 of 3 failed.

## Cause: the tests' assumptions, not production

Production is behaving as documented. `ConfigWatcher::with_hook`
(`crates/fe-config/src/watch.rs:52-62`) watches every directory before it returns. Each
`notify` `watch()` call starts its stream before returning (notify 8.2 `src/fsevent.rs:474-495`).
`Tracker::batch` (`watch.rs:176-212`) names the files whose chains an event touches,
and every file on a rescan or an error.

Two assumptions in the tests fail under load:

1. **Events from before the watcher started can arrive after it started.** Starting a
   stream with `kFSEventStreamEventIdSinceNow` doesn't stop it getting events that
   fseventsd hasn't finished processing yet. When fseventsd is backed up, that includes
   writes made before the watcher existed. Instrumented runs (a scratch copy of fe-config
   printing every raw event and batch) saw this in every run, and none had a rescan:
   - the fixture's `Create(Folder)` for the tempdir;
   - the fixture's `Create(File)` and `Data(Content)` for `settings.toml`;
   - `c/settings.toml` from the fixture;
   - the hook's `project/settings.toml.tmp` rename.

   A raw-FSEvents probe measured the delay: a file written before `FSEventStreamStart` was
   delivered 0.1–25 s after it, and a file written while the stream was stopped was
   delivered too. In `watcher_follows_links_changed_while_registering`, a late event for
   `a/settings.toml` or the `user/settings.toml` link (both on the user's chain) lands in a
   batch after the first one. That batch names `user` alone, or `user` and `project` when it
   arrives with the edit of `c`. Line 309 asserts that every later batch is exactly `[project]`.
   An extra reload of an unchanged user file is harmless, and nothing in `ConfigWatcher`'s
   docs promises otherwise. `after_change` already tolerates such batches, since it watches
   only one file.

2. **Delivery is not bounded by any timeout we can pick.** On this Mac, fseventsd is one
   saturated daemon shared with every lane and Time Machine (~100–120% CPU while
   sampled). The probe watched one directory with notify's flags (`FileEvents|NoDefer`,
   latency 0) and wrote a file every 250 ms:
   - p99 delivery was 1–2 s in quiet spells and 20–46 s in busy ones;
   - in busy runs 13–17 of ~58 writes were not delivered within 30–40 s;
   - no `MustScanSubDirs`, `UserDropped` or `KernelDropped` event arrived, so neither
     notify nor `Tracker` could know anything was missing.

   Run 6's three timeouts in one run fit a system-wide fseventsd stall, not a per-test race.
   `hot_reload` watches a single directory, so no stream restart is involved, and it timed out anyway.

## Fix (tests and test config only; no production change)

1. `watcher_follows_links_changed_while_registering` (watch.rs:304-314): after the first
   batch, accept any batch that is a non-empty, in-order subset of `[user, project]`. Reload
   each one, and loop until the project's font size is 18. Say in a comment that fixture
   and hook events can arrive late. Keep the first-batch assertion as it is.
2. Add a deterministic test in a new file `crates/fe-config/src/tests/watch_late.rs`
   (registered in `tests/mod.rs`). Its hook rewrites `a/settings.toml` (the user target,
   with unchanged content) when the last directory (`c`) is registered. The user's chain then
   always gets an event after the first batch, standing in for a late fixture event. Assert
   that the edit to `c` is still seen. It uses the same tolerant loop, through a shared helper
   in `tests/watch.rs`. The current strict loop would fail it every time.
3. The bound can't be fixed, so retry the real-FSEvents tests instead of failing the gate.
   In `.config/nextest.toml`, add a `[[profile.ci.overrides]]` with `retries = 2` filtered
   to exactly these tests:
   - fe-config `tests::watch::{hot_reload_applies_valid_and_keeps_last_good,
     symlink_targets_are_watched, watcher_follows_links_changed_while_registering}`;
   - fe-keymap `tests::load::hot_reload_swaps_the_keymap`.

   nextest reports a pass after a retry as FLAKY in the JUnit report, so it stays visible.
   Keep TIMEOUT at 60 s.

## Proof

- Build first, then use T99's synthetic load: 2×ncpu busy loops plus file churn in a temp
  directory, as a process group killed afterwards, with the whole window under about
  5 minutes. Under it, run
  `scripts/ci-env.sh cargo nextest run --profile ci -E '<the override's filter>'`
  20 times. Expect 0 FAIL; record the count of FLAKY passes and fseventsd's %CPU
  (`ps -o %cpu= -p $(pgrep -x fseventsd)`) in the hand-off.
- Without the retries override, 20 runs of `cargo test -p fe-config tests::watch` under the
  same load: no failure at the old watch.rs:309 assertion. Timeouts may still occur; record them.

## Implementation notes

- Shared helper `project_edit_seen_while_registering` in `tests/watch.rs`; the new `watch_late` test passes an extra hook that rewrites `a/settings.toml` when `c` is registered.
- Proof: under 2×ncpu busy loops plus file churn, 20 runs of the filtered `cargo nextest run --workspace --profile ci -E ...` gave 0 FAIL and 0 FLAKY (4 tests each); fseventsd was at 80-118 %CPU. The filter needs `--workspace`, or nextest builds only the root package and runs 0 tests. The second proof (20 runs of `cargo test -p fe-config tests::watch` without retries) was not run.
