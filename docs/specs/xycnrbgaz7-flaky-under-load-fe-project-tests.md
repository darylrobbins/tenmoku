---
task_uid: "01m4kjja0qet95ppxycnrbgaz7"
task: "T331"
rev: 2
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "The hand-in records the C1 finding from the first attempt (FSEvents drops a burst of events without a drop flag while fe-project's 100k-file tests run, with the raw-probe numbers) for the follow-up production bugfix, and confirms from the code that apart from that, ProjectWatcher and ConfigWatcher start their stream before returning, and Batch::add and Tracker lose no path"
    required: true
    manual: false
  - id: "C2"
    text: "Each delivery-dependent wait (fe-project wait_for for create, rename and delete; fe-config after_change and project_edit_seen_while_registering) writes a sentinel after its change into a separate directory watched only by a raw notify control stream. At the 60 s deadline the test fails if the control reported that sentinel at least 10 s before the deadline. Otherwise it prints 'FSEvents delivered nothing past \u003cwhat\u003e in 60 s; not a watcher failure' to stderr and passes"
    required: true
    manual: false
  - id: "C3"
    text: ".config/nextest.toml gains a commented test group 'fsevents' with max-threads = 1 holding the real-FSEvents tests of fe-project and fe-config (tests::watcher::, tests::watch::, tests::watch_late::) and fe-project's tests::large::a_100k_file_tree_loads_lazily and tests::finder::a_100k_file_index_bounds_query_work (plus any other workspace test that writes 10k+ files, named in the hand-in), so no flood runs alongside a delivery wait in one run; the existing retries override stays"
    required: true
    manual: false
  - id: "C4"
    text: "Every existing assertion stays, including the in-memory Batch dedupe assertions, the first-batch assertions and late_user_event_does_not_hide_project_edit's user-batch wait. No production code changes: only crates/fe-project/src/tests/watcher.rs, crates/fe-config/src/tests/watch.rs and .config/nextest.toml, apart from the task's own spec under docs/specs/"
    required: true
    manual: false
  - id: "C5"
    text: "Under a load average of at least 28: 20 consecutive runs of scripts/ci-env.sh cargo nextest run --profile ci -p fe-project -p fe-config -E over the five tests (watcher_batches_create_rename_delete, hot_reload_applies_valid_and_keeps_last_good, symlink_targets_are_watched, watcher_follows_links_changed_while_registering, late_user_event_does_not_hide_project_edit) have 0 FAIL, and 5 consecutive full runs of scripts/ci-env.sh cargo nextest run --profile ci -p fe-project -p fe-config have 0 FAIL among those five tests. The hand-in records the PASS, FLAKY and FAIL counts, how many control notes were printed, and each run's load average and fseventsd %CPU. The load generator registers its trap before starting and leaves no process behind"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-project -p fe-config passes"
    required: true
    manual: false
---

# Flaky under load: fe-project tests::watcher::watcher_batches_create_rename_delete and fe-config's FSEvents tests: fail a timed-out wait only when a control stream shows FSEvents delivered past the change

# Flaky under load: fe-project watcher_batches_create_rename_delete and fe-config's FSEvents tests

## Problem

T300 (c27af9a) raised `wait_for`'s deadline in `crates/fe-project/src/tests/watcher.rs` to 60 s and put
`tests::watcher::watcher_batches_create_rename_delete` in the `retries = 2` override in
`.config/nextest.toml`. It doesn't hold. Since it landed the test has failed all 3 tries, each at its own
60 s deadline (`watcher.rs:32:35`, "no batch showing the created file" or "... the deletion: timed out
waiting on channel"), in:

- T326's gate runs 01m4kgh56yh79z9f48edgtyc55 (2026-10-10 18:17, base 340097a) and
  01m4khnxk9d21gsxs9vb3exyp0 (18:37), under `~/.local/state/regatta/runs/<run>/test.fastedit.log`;
- the lead's macOS gate on dc8234a (log gone, same test).

T218's run 01m4kadyf6ba8905dwh4xckyzz (16:30, base 94371aa) failed it 3 times at 30 s; that base predates
T300. The load average was 90–260 on this 14-core Mac. The test passes in about 5 s alone.

In 01m4kgh56yh79z9f48edgtyc55 the fe-config real-FSEvents tests failed the same way, 60–64 s per try:
`tests::watch::hot_reload_applies_valid_and_keeps_last_good` (`watch.rs:45:50`, "a batch in time: Timeout")
and `tests::watch_late::late_user_event_does_not_hide_project_edit` (`watch.rs:336:9`, the TIMEOUT assert)
failed all 3 tries; `tests::watch::symlink_targets_are_watched` and
`tests::watch::watcher_follows_links_changed_while_registering` failed try 1 and passed try 2 in about 4 s.
(fe-keymap's `hot_reload_swaps_the_keymap` failed there too; T325 covers it.)

## Cause: one cause, five tests

All five wait on real FSEvents delivery with a fixed deadline. fseventsd is one daemon shared by every lane;
when it is saturated (T109's probe: p99 20–46 s, writes undelivered after 40 s, no `MustScanSubDirs` or
drop flags), delivery is late for minutes at a time, so three back-to-back 60 s tries all miss. T99, T109,
T211, T212, T234 and T300 each raised a deadline, added retries or rewrote a file, and none holds. No
deadline can: the test can't tell "our watcher lost the event" from "FSEvents hasn't delivered anything
yet". As T300 checked, `ProjectWatcher::new` and `ConfigWatcher` start their stream before returning, and
`Batch::add` / `Tracker` lose no path.

If you find while reading that `ProjectWatcher` or `ConfigWatcher` really loses an event, stop and report
it as a production bug to re-file as a bugfix with a regression test.

## Change (tests only)

Give each real-FSEvents wait a control, so a timeout fails only when FSEvents demonstrably delivered past
the change and our watcher still didn't report it.

1. In each crate's test file, add a small helper: a `Control` that owns a raw `notify::recommended_watcher`
   over a sentinel directory of its own (a separate `tempfile::tempdir()`, outside every watched path, so
   no watcher under test sees it), started before the test's change, and an `mpsc` receiver of the paths
   it reports. `Control::mark()` writes a new file `sentinel-<n>` there and returns its path;
   `Control::seen(&path)` drains the receiver and says whether that path has been reported, noting when.
2. Every wait that depends on delivery calls `mark()` right after making its change (and, in
   `project_edit_seen_while_registering`, after each `RETRY` rewrite too). When the 60 s deadline passes
   without the awaited batch:
   - if the control reported a sentinel written after the change at least 10 s before the deadline
     (FSEvents delivered past the change, and the watcher had 10 s more), the test fails as now;
   - otherwise it prints `FSEvents delivered nothing past <what> in 60 s; not a watcher failure` to stderr
     and returns early, passing.
3. Apply it to `wait_for` in `crates/fe-project/src/tests/watcher.rs` (each of create, rename, delete), and
   to `after_change` and `project_edit_seen_while_registering` in `crates/fe-config/src/tests/watch.rs`.
   The first-batch waits (built from `Tracker::files()`, not FSEvents) stay as they are. Keep every existing
   assertion, including the in-memory `Batch` dedupe assertions and `late_user_event`'s user-batch wait.
   Update the module docs and `wait_for`'s comment to describe the control.
4. In `.config/nextest.toml`, add one test group (see "Rev 2" below). Keep the existing retries override.

## Proof

A test-only fix can't fail at base on demand, so prove it under load. Build first. Start the load generator
in a script that registers `trap 'kill $(jobs -p) 2>/dev/null; pkill -P $$ 2>/dev/null' EXIT INT TERM`
before starting busy loops that bring the load average to at least 28 (twice the cores); ambient load
above that counts. Run 20 consecutive times:

    scripts/ci-env.sh cargo nextest run --profile ci -p fe-project -p fe-config -E 'test(=tests::watcher::watcher_batches_create_rename_delete) or test(=tests::watch::hot_reload_applies_valid_and_keeps_last_good) or test(=tests::watch::symlink_targets_are_watched) or test(=tests::watch::watcher_follows_links_changed_while_registering) or test(=tests::watch_late::late_user_event_does_not_hide_project_edit)'

Record in the hand-in the PASS, FLAKY and FAIL counts, how many control notes were printed, and per run the
load average and fseventsd's %CPU (`ps -o %cpu= -p $(pgrep -x fseventsd)`). Leave no load process behind.


## Rev 2 (lead, 2026-10-10): the C1 stop condition hit; isolate the floods instead

The worker's first attempt (lane commits 59853fe, 1d4250d) found that load alone doesn't cause the failures:
C4 as written passed 20/20 at load 99–127. The trigger is fe-project's two 100k-file tests,
`tests::large::a_100k_file_tree_loads_lazily` and `tests::finder::a_100k_file_index_bounds_query_work`,
running at the same time as the FSEvents tests. During that flood FSEvents drops a burst of events (a raw
probe lost sentinels 5–31 of 70) and sets no MustScanSubDirs, UserDropped or KernelDropped flag. So C2's
rule, "a delivered sentinel means the change was delivered", is unsound while a flood runs, and
`late_user_event_does_not_hide_project_edit` failed under it in a full `-p fe-project -p fe-config` run.

The production gap this exposes (the watchers silently miss changes made during a big write burst) is a
separate bugfix task. Don't fix it here.

What this task does now:

- Keep the control (C2). It still tells "FSEvents is late" apart from "our watcher lost it", once nothing
  in the same run is flooding FSEvents.
- Add a nextest test group to `.config/nextest.toml`, `fsevents`, with `max-threads = 1`, holding the
  real-FSEvents tests (fe-project `tests::watcher::`, fe-config `tests::watch::` and `tests::watch_late::`,
  and any other test that waits on real FSEvents delivery in those crates) and the 100k-file tests named
  above. Add any other test in the workspace that writes 10k or more files, if you find one. That way no
  flood runs at the same time as a delivery wait in one nextest run. The gate runs one pipeline at a time
  on this Mac, so one run is the unit that matters. Comment the group with this reason.
- The existing `retries = 2` override stays. T325 (in the inbox) also edits this file; this task goes first.

## Implementation notes

Blocked: FSEvents loses events silently under a write flood, so the control can't tell a watcher failure
from FSEvents' own loss, and the watchers lose events that way in production too (C1's stop condition).

**What's done (59853fe).** Both test files have a `Control` as specified: a raw `notify::recommended_watcher`
over its own `tempdir`, events stamped with `Instant::now()` on arrival, `mark()`, `seen()`, and
`timed_out(marks, deadline, what)` (fail if any mark was seen at least 10 s before the deadline, else
print the note). `wait_for` (fe-project), `after_change` and `project_edit_seen_while_registering`
(fe-config, marking after each `RETRY` rewrite too) use it. fe-project's in-memory `Batch` assertions move
to the start of the test, since a passing timeout now ends it early. Both branches were checked with
throwaway tests: a wait that never holds fails at 60 s ("though FSEvents delivered past it"), and a control
whose sentinel goes unwatched prints the note and passes.

**C1 from the code (holds as far as the code goes).** notify 8.2's `FsEventWatcher::watch` calls `run()`,
which blocks on `rl_rx.recv()` until the loop thread has called `FSEventStreamStart`, so
`ProjectWatcher::new` and `ConfigWatcher` are streaming on return. The fsevent backend never emits
`Access` events, `Batch::add` pushes every path of every other event and a rescan for errors and
`need_rescan`, and `Tracker::batch` matches every event path against every chain, re-resolving and
re-registering as documented. Nothing in our code drops what FSEvents delivers. (notify ignores
`FSEventStreamStart`'s return value, and its callback discards events whose path isn't under a watched
path before it reads their flags. Neither turned out to matter here.)

**C4 as written: 20/20 runs, 0 FAIL.** Load generator: `trap … EXIT INT TERM` first, then 16
`nice -n 19` busy loops, 60 s settle, then 20 runs of the spec's command plus `--success-output immediate`
(so the notes of passing tests reach the log). Every run: 5 PASS, 0 FLAKY, 0 FAIL, 0 notes, 3–7 s. 1-minute
load per run: 99.2, 100.4, 99.4, 102.7, 105.9, 107.7, 107.1, 107.0, 107.0, 110.1, 119.2, 119.5, 119.5,
116.4, 118.2, 118.2, 119.2, 121.3, 124.0, 127.4; fseventsd %CPU 99.8, 105.4, 103.4, 103.1, 104.7, 102.7,
103.4, 105.1, 102.7, 103.4, 105.3, 105.6, 108.7, 104.7, 105.4, 110.7, 103.1, 102.6, 104.5, 105.2. No
load process was left. CPU load alone doesn't reproduce the gate failures.

**What does reproduce them: the 100k-file tests.** `cargo nextest run -p fe-project -p fe-config` runs
`tests::large::a_100k_file_tree_loads_lazily` and `tests::finder::a_100k_file_index_bounds_query_work`
beside the watcher tests (so does the gate's workspace run). In 3 full runs, every FSEvents test waited
60 s, mostly with the control's note, and one run FAILED
`watch_late::late_user_event_does_not_hide_project_edit` with "though FSEvents delivered past it".
Rerunning those five tests with the two 100k tests and temporary prints in `ConfigWatcher`'s thread failed
the same way on the first try: after the project's edit at T0, the test's rewrites of `a/settings.toml`
and control sentinels 1–23 (T0 … T0+23 s) were never reported by either stream; sentinel 24 and the
rewrite just before it arrived together at T0+25.6 s; the project's edit never arrived, and no rescan.

**FSEvents itself drops them.** An unfiltered probe (Swift, `FSEventStreamCreate` over one fresh
directory, `FileEvents | NoDefer | UseCFTypes`, latency 0, every callback printed with flags) writing one
sentinel a second for 70 s while the two 100k tests ran (about 3–33 s): sentinels 1–4 reported within
1–2 s; **5–31 (written 4.6–33 s) never reported in any form, neither the final name nor the `.sb-` temp,
and no event carried `MustScanSubDirs`, `UserDropped` or `KernelDropped`** (59 events in all: 29 final,
29 temp, the directory's own creation); delivery resumed at 36 s with sentinel 32; 35, 36, 38, 39, 41, 42
and 62 were lost too after the flood. So FSEvents doesn't just deliver late: it loses a burst of events
silently, then carries on.

**Consequences.**
- The spec's control rule rests on in-order delivery ("a sentinel after the change was delivered, so the
  change was too"). With silent loss that doesn't hold: a change lost in a burst, followed by a sentinel
  written after the burst, fails the test with no watcher fault. Retried waits (`project_edit_seen_…`)
  hit this deterministically, since their marks span the burst's end. C2's rule as written can't be made
  sound; at best it trades this flake for another.
- Production: `ProjectWatcher` and `ConfigWatcher` miss on-disk changes made during such a burst, with no
  rescan to recover (the tree goes stale; a settings save is ignored). Any process writing ~100k files
  (a checkout, a build, `npm install`) causes it. This is outside our code's logic but inside the
  watchers' contract, so per C1 it's reported for re-filing as a bugfix, e.g. a recovery rescan triggered
  by a stall heuristic, or a periodic `stat` re-check of the watched config files.
- Tests: what can work is making the awaited change again while waiting (as `RETRY` already does for the
  user's target), so one lost event can't decide the outcome, or keeping the 100k tests out of the same
  run window (a nextest test group). Both are outside this spec's C3 scope.

## Implementation notes (rev 2)

**C1, for the production bugfix.** The finding stands as recorded above: an unfiltered FSEvents probe
stream (one fresh directory, `FileEvents | NoDefer | UseCFTypes`, latency 0) writing a sentinel a second
for 70 s lost sentinels 5–31 (written 4.6–33 s, while fe-project's two 100k-file tests ran from about 3
to 33 s), in any form and with no `MustScanSubDirs`, `UserDropped` or `KernelDropped` flag (59 events in
all), resumed at 36 s with sentinel 32, and lost 35, 36, 38, 39, 41, 42 and 62 after. Apart from that loss,
from the code: notify 8.2's `FsEventWatcher::watch` blocks in `run()` until `FSEventStreamStart` has run,
so `ProjectWatcher::new` and `ConfigWatcher` stream on return; the backend emits no `Access` events;
`Batch::add` keeps every path and turns errors and `need_rescan` into `Rescan`; `Tracker::batch`
matches every path against every chain. Two notify quirks to look at in the bugfix: it ignores
`FSEventStreamStart`'s result, and its callback drops events outside the watched paths before reading
their flags.

**C3, the group.** `[test-groups.fsevents]`, `max-threads = 1`, with a `[[profile.default.overrides]]`
`test-group` override, so it applies to the plain run and, falling through the ci profile's own override
(which only sets `retries`), to `--profile ci`. `cargo nextest show-config test-groups --profile ci` lists
12 members: fe-config `tests::watch::*` (7, the `Tracker` ones included, harmlessly) and
`tests::watch_late::late_user_event_does_not_hide_project_edit`; fe-project
`tests::watcher::watcher_batches_create_rename_delete`, `tests::large::a_100k_file_tree_loads_lazily`,
`tests::finder::a_100k_file_index_bounds_query_work`; and two more 10k+-file writers found in the
workspace, fe-workbench `tests::m3_signoff::a_100k_file_project_end_to_end` (100 × 1,000) and
`tests::project_panel::only_visible_rows_are_built` (1 × 100,000). No other test writes 10k files: the
next largest are `finder_walk_safety` (4,000 directories), fe-nav `build` (1,500) and fe-project
`search` (1,000). No other fe-project or fe-config test uses a real watcher. Outside those crates,
fe-keymap's `tests::load::hot_reload_swaps_the_keymap` also waits on FSEvents; it's T325's and not in the
group. The retries override is unchanged.

**C5.** Same load script as before (`trap … EXIT INT TERM` first, 16 `nice -n 19` busy loops, a build, 60 s
settle), `--success-output immediate` added so passing tests' notes reach the log; counts are the five
tests' final status lines. No load process was left.

- 20 runs of the five tests: 100 PASS, 0 FLAKY, 0 FAIL, 0 failed tries, 0 notes, 8–16 s each. Load:
  93.0, 95.4, 99.2, 102.3, 141.8, 148.1, 154.8, 160.7, 166.7, 166.7, 169.6, 170.0, 171.2, 170.7, 169.4,
  171.4, 173.6, 173.7, 174.1, 177.1; fseventsd %CPU: 104.2, 106.5, 105.2, 97.9, 98.1, 100.1, 104.6, 103.4,
  101.2, 102.3, 102.6, 102.8, 103.6, 102.8, 104.1, 104.6, 103.7, 102.5, 105.1, 103.8.
- 5 full `--profile ci -p fe-project -p fe-config` runs: 129/129 passed each time; among the five tests
  25 PASS, 0 FLAKY, 0 FAIL, 0 failed tries, 6 notes (run 1: 0; 2: 1 "the created file"; 3: 1 "the created
  file"; 4: 2, "the change" and "the created file"; 5: 2, the same). Load 177.8, 208.3, 214.8, 233.3,
  300.0; fseventsd %CPU 104.1, 100.0, 101.5, 117.8, 98.9; 69–191 s each.
- C6: `cargo nextest run -p fe-project -p fe-config` 129/129; `rgt ci run --fast` passed.

**What the group doesn't settle.** The flood outlives its test in fseventsd: in full runs 2–5,
`watcher_batches_create_rename_delete` ran straight after `a_100k_file_tree_loads_lazily` and every time
saw nothing delivered for 60 s, so it passed through the note without checking the watcher (its own 60 s
are the slow tests). If the backlog ends inside such a wait, a lost change and a delivered sentinel can
still fail it; a single mark is written microseconds after its change, so both usually share a fate, but
`project_edit_seen_while_registering`'s retry marks span time. Having the 100k tests wait for a control
sentinel before returning (a drain) would close that, but it edits `large.rs` and `finder.rs`, outside
this task's files; it fits the production bugfix or a follow-up.

## Implementation notes (review round 1, fee56k)

**F1, a flood's backlog outlives its test.** Each delivery wait now first calls `Control::ready(what)`:
it writes a sentinel every 100 ms (`READY_GAP`) and returns once 5 in a row (`READY_RUN`) have all been
reported, so FSEvents is demonstrably delivering, not dropping, right before the change; only then is the
change made and marked. If that doesn't happen within `TIMEOUT` it prints `FSEvents delivered nothing past
the control's sentinels before <what> in 60 s; not a watcher failure` and the test ends early, passing.
A wait that held more than 2 s prints `FSEvents caught up before <what> after <t>`. fe-project's
`wait_for` takes the change as a closure for this; `after_change` and
`project_edit_seen_while_registering` call `ready` after the first batch, before the change. A fresh
flood starting inside a wait can still lose a change; the `fsevents` group keeps that out of one run.

**F2, expiry with batches queued.** Every wait loop checks the remaining time before receiving, calls
`timed_out` as soon as it's zero, and treats `RecvTimeoutError::Timeout` as "go round and check". A
throwaway fe-project test fed a never-satisfying batch every 10 ms: the wait decided at 61.2 s and failed
("though FSEvents delivered past it", its sentinel having been delivered); the old loop wouldn't end.

**The transition, validated.** 5 rounds, each starting the two 100k-file tests in a separate nextest
process (so outside the group) and the five tests 5 s into that flood: 25 PASS, 0 FLAKY, 0 FAIL, 0 failed
tries, 18 waits that held until FSEvents caught up mid-flood, 2 notes. Load 123.3, 196.1, 220.5, 230.7,
202.0; fseventsd %CPU 103.5, 182.2, 156.8, 147.5, 169.3; floods 48–105 s.

**C5 again on this commit** (same load generator: trap first, 16 `nice -n 19` loops, build, 60 s settle;
no load process left):

- 20 runs of the five tests: 100 PASS, 0 FLAKY, 0 FAIL, 0 failed tries, 1 note, 42 catch-ups, 19–109 s.
  Load 186.5, 172.4, 171.3, 162.4, 171.4, 179.8, 184.4, 182.2, 171.5, 165.0, 161.6, 148.1, 139.2, 141.0,
  187.5, 187.3, 202.3, 226.6, 242.4, 237.8; fseventsd %CPU 102.0, 100.5, 105.0, 100.9, 99.5, 101.4, 105.8,
  102.9, 103.3, 103.4, 101.1, 101.7, 105.3, 101.6, 113.0, 102.5, 109.9, 103.4, 102.9, 104.8.
- 5 full `--profile ci -p fe-project -p fe-config` runs: 129/129 each; the five tests 25 PASS, 0 FLAKY,
  0 FAIL, 0 failed tries, 0 notes, 18 catch-ups. Load 235.2, 224.5, 200.8, 171.9, 145.6; fseventsd %CPU
  103.6, 102.7, 106.9, 101.0, 100.3; 80–104 s.
- Across all 30 runs: 78 catch-ups, median 3.8 s, longest 53.5 s; 3 notes (2 "the control's sentinels
  before the change", 1 "the change"). Unlike the earlier full runs, where
  `watcher_batches_create_rename_delete` after a 100k test passed only through the note, it now checks
  the watcher in every full run.
- C6: `cargo nextest run -p fe-project -p fe-config` 129/129; `rgt ci run --fast` passed.
