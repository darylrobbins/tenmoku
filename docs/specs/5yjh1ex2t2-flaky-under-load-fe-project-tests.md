---
task_uid: "01m4h1553aa99gvh5yjh1ex2t2"
task: "T300"
rev: 3
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The hand-in confirms from the code that the failure is the test's own 30 s wait_for deadline on unbounded FSEvents delivery (ProjectWatcher::new starts the stream before returning, and Batch::add loses no path). If ProjectWatcher can miss an event instead, the work stops and this is reported as a production race to re-file as a bugfix"
    required: true
    manual: false
  - id: "C2"
    text: "watcher_batches_create_rename_delete still shows the created file, then its rename, then its deletion through a real ProjectWatcher, and keeps its in-memory Batch dedupe assertions unchanged, with wait_for's deadline raised to 60 s"
    required: true
    manual: false
  - id: "C3"
    text: "The ci profile's retries = 2 override in .config/nextest.toml also covers exactly package(fe-project) and test(=tests::watcher::watcher_batches_create_rename_delete), and its comment's count of tests is updated"
    required: true
    manual: false
  - id: "C4"
    text: "No production code changes; only crates/fe-project/src/tests/watcher.rs and .config/nextest.toml change. The task's own spec file under docs/specs/, committed by Regatta, is expected in the diff and is not a violation"
    required: true
    manual: false
  - id: "C5"
    text: "10 consecutive runs of scripts/ci-env.sh cargo nextest run --profile ci -p fe-project -E 'test(=tests::watcher::watcher_batches_create_rename_delete)' have 0 FAIL while the load average is at least twice the core count (background load added when the machine is quieter than that). Pass count, FLAKY count, load average and fseventsd %CPU are recorded in the hand-in. The load generator registers trap 'kill $(jobs -p) 2\u003e/dev/null' EXIT INT TERM before starting and leaves no process behind"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-project passes"
    required: true
    manual: false
---

# Flaky under load: fe-project tests::watcher::watcher_batches_create_rename_delete times out waiting for an FSEvents batch

# Flaky under load: fe-project tests::watcher::watcher_batches_create_rename_delete

## Symptom

`crates/fe-project/src/tests/watcher.rs` `watcher_batches_create_rename_delete` fails under load at
its own deadline, about 30.1–32.4 s in, panicking at `watcher.rs:30:35`:

    no batch showing the created file: timed out waiting on channel

Most recently T145's gate run 01m4h04f1yq56yarb4j2crq0s0 (2026-10-09 15:00, 1187 of 1188 passed, none
of them in T145's diff). The gate run logs under `~/.local/state/regatta/runs/` show it failing in about 20
runs since 2026-10-06, among them 01m4949chk9dhf6228vfp85ye6, 01m49hw04yxft2bvkqhn5c232c,
01m4e9tex9r49k6g14a850yscd, 01m4fmyvkstb7mkdqdgfc9xtqh, 01m4gg2vtntdtjmjhasckp7v71 and
01m4gvwbkdccpq296yfxe3vqrv. It often fails in the same run as the fe-config and fe-keymap FSEvents tests
that T99, T109 and T211 dealt with. Unlike them, it isn't in the `retries = 2` override in
`.config/nextest.toml`, so it gets one try.

## Cause: the test's assumption, not production

`ProjectWatcher::new` (`crates/fe-project/src/watcher.rs`) calls notify's `watch()`, which starts the
FSEvents stream before it returns (notify 8.2 `src/fsevent.rs`, as T109 checked). The test writes
`src/new.rs` only after that. So the event can't be missed because the stream started late. What fails is
the test's 30 s `recv_timeout` deadline in `wait_for`.

T109's raw-FSEvents probe measured the same thing on this Mac. fseventsd is one daemon shared by every
lane. When it's saturated, p99 delivery was 20–46 s, and 13–17 of about 58 writes weren't delivered within
30–40 s. No `MustScanSubDirs`, `UserDropped` or `KernelDropped` event arrived, so no deadline we pick
bounds delivery.

If you find a real loss in `ProjectWatcher` while reading this (for example, the stream isn't started when
`new` returns, or `Batch::add` drops a path), stop. Report it as a production race to re-file as a bugfix
with a regression test.

## Fix (tests and test config only; no production change)

1. In `watcher.rs`, raise `wait_for`'s deadline from 30 s to 60 s to match the fe-config watcher tests. Say
   in a comment that FSEvents delivery under load isn't bounded, and point at the retries override.
2. Add `package(fe-project) and test(=tests::watcher::watcher_batches_create_rename_delete)` to the
   existing `[[profile.ci.overrides]]` filter in `.config/nextest.toml` (`retries = 2`), and update its
   comment's count of tests. A pass after a retry is reported FLAKY in the JUnit report, so it stays visible.

## Proof

A test-only fix can't fail at base on demand, so prove it with repeated runs under load. Build first. Then
start the load generator in a script that runs `trap 'kill $(jobs -p) 2>/dev/null' EXIT INT TERM` before
it starts any load: busy loops that bring the load average to at least twice the core count (28 on this
14-core Mac). When other work already holds it above that, the ambient load counts; a higher load is a stronger proof, not a weaker one. Under that load, run
`scripts/ci-env.sh cargo nextest run --profile ci -p fe-project -E 'test(=tests::watcher::watcher_batches_create_rename_delete)'`
10 times, and no more. Record the pass count, the FLAKY count, the load average and fseventsd's %CPU
(`ps -o %cpu= -p $(pgrep -x fseventsd)`) in the hand-in. Leave no load process behind.

## Implementation notes

- C1: notify 8.2 `fsevent.rs` sends the runloop back only after `FSEventStreamStart`, and `watch()` blocks on that, so the stream is live before `ProjectWatcher::new` returns. `Batch::add` pushes every event path (only `Access` events are skipped), so no path is lost. Failure is the test's own deadline on unbounded delivery.
- C3 (retries override and "six tests" comment) was already committed at base in e0bf189.
- Proof, 10 runs, ci profile, load generator (14 busy loops, trap + `pkill -P $$`): 10 pass, 0 FLAKY, 0 FAIL. Ambient load average was already 74-82 from other lanes (not ~28), fseventsd 101-110 %CPU. Plain `cargo nextest run -p fe-project` (no retries) failed 3 times at the 60 s deadline while fseventsd was saturated, and then passed once, with the test taking 48 s.

### C5 proof record

- Command, 10 consecutive times (script registers `trap 'kill $(jobs -p) 2>/dev/null; pkill -P $$ 2>/dev/null' EXIT INT TERM` before starting 14 busy loops): `scripts/ci-env.sh cargo nextest run --profile ci -p fe-project -E 'test(=tests::watcher::watcher_batches_create_rename_delete)'`
- Result: 10 pass, 0 FLAKY, 0 FAIL.
- Load average (1 min) per run: 73.9, 77.0, 77.0, 78.4, 78.4, 79.5, 79.5, 80.0, 81.7, 81.7, all at least 2x the 14 cores; fseventsd %CPU per run: 106.0, 105.0, 110.7, 107.8, 106.9, 107.5, 104.3, 106.3, 107.2, 101.3.
- Cleanup: `ps` afterwards showed 0 load.sh/busy-loop processes.
