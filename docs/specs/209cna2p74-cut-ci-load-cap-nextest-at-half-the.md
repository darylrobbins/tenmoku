---
task_uid: "01m6s0rd3m3syrah209cna2p74"
task: "T1421"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "scripts/ci-env.sh exports NEXTEST_TEST_THREADS defaulting to max(2, getconf _NPROCESSORS_ONLN / 2) unless the caller set it, keeps CARGO_BUILD_JOBS defaulting to 6, and regatta.toml and .config/nextest.toml are unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/ci-env.sh runs a command whose $PWD is under */.worktrees/ but not */.worktrees/.verify/ under nice -n FASTEDIT_LANE_NICE (default 10, 0 = off), and every other command at the caller's priority"
    required: true
    manual: false
  - id: "C3"
    text: "scripts/ci-env.sh prints one stderr line matching ^ci-env: test-threads=[0-9]+ jobs=[0-9]+ nice=[0-9]+ load= before it execs the command"
    required: true
    manual: false
  - id: "C4"
    text: "scripts/test-ci-env.sh exits 0 and asserts the default and caller-set NEXTEST_TEST_THREADS, CARGO_BUILD_JOBS=6, nice +10 in a lane dir, caller niceness in a .verify dir, outside .worktrees and with FASTEDIT_LANE_NICE=0, and the stderr line format"
    required: true
    manual: false
  - id: "C5"
    text: "rgt ci run --fast passes in the lane"
    required: true
    manual: false
  - id: "C6"
    text: "TESTING.md documents the test-thread cap (half the cores, because host_pipeline_slots = 2, NEXTEST_TEST_THREADS overrides), the lane nice and FASTEDIT_LANE_NICE, the ci-env stderr line, and lists scripts/test-ci-env.sh with the script self-tests"
    required: true
    manual: false
  - id: "C7"
    text: "Evidence: the finish gate's test stage log contains the ci-env line with test-threads=7 jobs=6 nice=0 and a load value (quote the line and the run id in the finish notes)"
    required: true
    manual: false
---

# Cut CI load: cap nextest at half the cores per pipeline run and nice lane-side CI runs in scripts/ci-env.sh

## Problem

The Mac (14 cores: 10 performance, 4 efficiency; 36 GiB) runs `lanes_max = 2` lanes plus gates,
lane fast runs, regression checks and post-merge checks at once. Load sits at 50-280 (280 on 11-06
09:4x with a post-check rerun, a regression check and a fast run going), and about 40% of gates
need a retry because timing-sensitive tests fail under load (T1417 fe-lsp login-shell test, T1420
kiln-plugin program_budget test, T1382 and T1387 before them). A gate takes ~635 s quiet.

`caps.host_pipeline_slots = 2` already holds gates, fast runs and post-merge checks (every
`RunPipeline`) to two at a time, and `scripts/ci-env.sh` caps cargo at `CARGO_BUILD_JOBS=6`, so two
builds fit in 14 cores. Nothing caps the tests: each nextest run uses `test-threads = num-cpus`
(14), so two concurrent test stages run 28 tests on 14 cores, many of them spawning threads or
login shells of their own, and a test stage beside the other slot's build runs 14 + 6. The
2026-11-06 bench found more cargo jobs (12) bought nothing, so the cores are already the limit.

Lane fast runs (`rgt ci run --fast` in the lane worktree, feedback only) compete with gates,
post-merge checks and regression checks (evidence) at the same priority.

## Change

Everything goes through `scripts/ci-env.sh`, which every regatta.toml command already runs
through, so `regatta.toml` itself does not change.

1. `scripts/ci-env.sh`: export `NEXTEST_TEST_THREADS` (nextest reads it; a caller's value wins, as
   for `CARGO_BUILD_JOBS`), defaulting to half the online cores, `getconf _NPROCESSORS_ONLN` / 2,
   at least 2 (7 on the Mac). Half because `host_pipeline_slots = 2`: two runs' tests then fit the
   cores, as two runs' builds already do at 6 jobs. Leave `CARGO_BUILD_JOBS` at 6. Do not set
   `test-threads` in `.config/nextest.toml`: that would also cap people's local runs, and a fixed
   number would be wrong on the other hosts that share this file.
2. `scripts/ci-env.sh`: when `$PWD` is in a lane worktree (under `*/.worktrees/` but not
   `*/.worktrees/.verify/`), exec the command under `nice -n "$FASTEDIT_LANE_NICE"` (default 10;
   0 turns it off). Snapshots (gates, `--clean`/`--commit` runs, post-merge and regression checks)
   and runs outside `.worktrees/` keep normal priority. POSIX `nice`, not `taskpolicy`: background
   QoS would pin lane runs to the 4 efficiency cores and throttle their I/O.
3. `scripts/ci-env.sh`: print one line to stderr before exec,
   `ci-env: test-threads=<n> jobs=<n> nice=<n> load=<1-min load average>`, so every stage log
   records the limits it ran under and the load it started at (`sysctl -n vm.loadavg` on macOS,
   `/proc/loadavg` on Linux; `load=?` when neither works). Update the header comment.
4. New `scripts/test-ci-env.sh`, a fixture test in the style of `scripts/test-prune-build-dirs.sh`
   (temp dirs, `sh -c` probes through ci-env.sh, `FASTEDIT_MIN_FREE_GIB=0` for the `.verify` case),
   asserting: the default `NEXTEST_TEST_THREADS` is max(2, cores / 2); a caller's
   `NEXTEST_TEST_THREADS=3` is kept; `CARGO_BUILD_JOBS` is still 6 by default; a command run from
   `<tmp>/.worktrees/lane1` runs 10 nicer than the caller (`ps -o nice= -p $$`), one from
   `<tmp>/.worktrees/.verify/x` and one from `<tmp>` run at the caller's niceness, and
   `FASTEDIT_LANE_NICE=0` in a lane runs at the caller's niceness; the stderr line matches
   `^ci-env: test-threads=[0-9]+ jobs=[0-9]+ nice=[0-9]+ load=`.
5. `TESTING.md`: next to the `CARGO_BUILD_JOBS` text, document the test-thread cap (half the cores,
   why, how to override), the lane `nice` (what counts as a lane, `FASTEDIT_LANE_NICE`), and the
   stderr line; list `scripts/test-ci-env.sh` with the other script self-tests.

Expected effect: at most ~14 busy compile or test threads from the two pipeline slots together,
instead of up to 28 test threads, and evidence runs ahead of lane feedback when they contend. A
quiet single gate's test stage gets slower (7 threads instead of 14); a retried gate costs a whole
gate (~10 min), so this pays if it removes even a fraction of the ~40% retries.

## After landing (lead, not a criterion)

Over the first 10 gates and post-merge checks, record retries and the `load=` values in their
stage logs, against the ~40% retry baseline. If quiet test stages are much slower and retries did
not fall, revert to `num-cpus` by setting `NEXTEST_TEST_THREADS` in ci-env.sh's default.

## Out of scope

- `regatta.toml` (`host_pipeline_slots`, `max_parallel`, `lanes_max` stay as they are).
- Cargo or nextest runs a worker starts itself in its lane, which bypass ci-env.sh, and how often
  workers run `rgt ci run --fast` (a separate card).
- Regatta source: the bugfix regression check (`test_one` in the `-rb`/`-rc` snapshots) runs
  outside `host_pipeline_slots`, so it is a third pipeline the slots do not count.
- The flaky tests themselves (T1417, T1420).

## Implementation notes

C7 evidence (finish gate run 01m6t5xhrgs3phnmqa4zkskfx2, `~/.local/state/regatta/runs/01m6t5xhrgs3phnmqa4zkskfx2/`):

- `test.fastedit.log` line 3: `ci-env: test-threads=7 jobs=6 nice=0 load=113.41`
- `vet.fastedit.log`: `ci-env: test-threads=7 jobs=6 nice=0 load=70.61`
- `fmt.fastedit.log`: `ci-env: test-threads=7 jobs=6 nice=0 load=68.95`
- `no_gpui.fastedit.log`: `ci-env: test-threads=7 jobs=6 nice=0 load=38.55`

The gate (a `.verify` snapshot) runs at the caller's priority (nice=0) with the 7-thread nextest cap
exported on the 14-core Mac.
