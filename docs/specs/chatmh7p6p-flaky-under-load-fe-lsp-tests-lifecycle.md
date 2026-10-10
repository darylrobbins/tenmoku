---
task_uid: "01m4kcfb1rkst7ezchatmh7p6p"
task: "T5"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "In floods_do_not_hold_up_stop_or_deadlines, the third part (host flooding requests) builds its manager with Some(ManualClock) that is never advanced instead of None (system clock), its comment says why, and its assertions (Starting, \u003e1000 records, then the next status Stopped(None) after stop) are unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "No production code, no other test and no .config/nextest.toml retry changes: only crates/fe-lsp/src/tests/lifecycle.rs changes (apart from Regatta's own spec file under docs/specs/)"
    required: true
    manual: false
  - id: "C3"
    text: "10 consecutive runs of scripts/ci-env.sh cargo nextest run --profile ci -p fe-lsp -E 'test(=tests::lifecycle::floods_do_not_hold_up_stop_or_deadlines)' have 0 FAIL while the 1-min load average is at least 2x the core count (nproc on Linux; ambient load counts; any busy loops added only below that, in their own process group with trap 'kill $(jobs -p) 2\u003e/dev/null' EXIT INT TERM registered first and a timeout of 10 min or less, leaving no process behind). The pass count and each run's load average are recorded in the decision file"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp passes"
    required: true
    manual: false
---

# Flaky under load: fe-lsp tests::lifecycle::floods_do_not_hold_up_stop_or_deadlines: run its request-flood part on a manual clock so readiness's 500 ms can't report Ready before the stop

## Problem

`fe-lsp.tests::lifecycle::floods_do_not_hold_up_stop_or_deadlines` fails under load (1-min load
average 110-160 on 14 cores) at `crates/fe-lsp/src/tests/lifecycle.rs:563`:

```
assertion `left == right` failed
  left: Ready
 right: Stopped(None)
```

The same failure shows up in 7 gate runs across 7 commits, with no ci-profile retry and no card:
01m4cqzyh22shkhb09xj1j3re7 (685f071, 10-08), 01m4dtvdy8bvrptwf2mwmgg2q4 (3d20282, 10-08),
01m4eh6dssj4dsf8j9fgcbnktj (812b874, 10-08), 01m4gqdhgf7n27qsrj1p3agzpa (09d4d6f, 10-09),
01m4gxya93rsshw9b0dfa9bwaz (5a72ff8, 10-09), 01m4hv07f4srs0zbqv2dv08dfx (6edb6c2, 10-10),
01m4kadyf6ba8905dwh4xckyzz (4c4041c, T218, 10-10). Logs are in `~/.local/state/regatta/runs/<run>/test.fastedit.log`.

This is the test's mistake, not a race in the stop path:

- The test's third part ("a host flooding requests from another thread") builds its manager with
  `super::manager(fake.launcher(), None)`, which is `LspManager::new` on the **system clock**.
- The spec is `ServerSpec::new`, so readiness is `ReadinessMode::Progress` (the default): the session
  becomes `Ready` once `initialize` has answered and no progress has been open for `readiness::SETTLE`
  (500 ms), whatever requests are flowing.
- The test then waits for the fake to record more than 1000 requests (`fake.wait_for("the flood", ...)`)
  before it calls `stop`. At low load that takes well under 500 ms. Under load it takes longer, so the
  actor reports `Ready` first, and `Events::status` returns the *next* status, which is `Ready`.
- The stop itself isn't held up: `Step::Stop` (`carry.stop`) is the second arm of `serve`'s
  `select_biased!` in `crates/fe-lsp/src/server.rs`, ahead of `Step::Due`/`Step::Readiness` and the
  command stream. A held-up stop would fail as "no status from ServerId(..) in 10s", not as `Ready`.
  `Ready` followed by `Stopped(None)` is a valid production sequence.

## Change

In `floods_do_not_hold_up_stop_or_deadlines`, give the third part's manager a `ManualClock` that is
never advanced (`super::manager(fake.launcher(), Some(Arc::new(ManualClock::new())))`), as the first two
parts and `Started::new` do. Readiness's 500 ms then never passes, so the only status between
`Starting` and the stop is none, and the existing `assert_eq!(events.status(id), ServerStatus::Stopped(None))`
holds whatever the load.

The stop path still needs no clock time: the default `Script` answers `shutdown` and the fake exits on
`exit`, so the stop still ends within the test's 10 s real-time `WAIT`, and a stop held up by the flood
still fails the test. Update the part's comment to say why the clock is manual.

Nothing else changes: no production code, no other test, no `.config/nextest.toml` retry.


_Filed for the neon board by the Mac lead: the flake shows on Mac gates under load (7 runs, 10-08..10-10). Lands back on main through the lead._

## Implementation notes

C3: 10/10 PASS under busy loops (12 cores; 1-min load at each run: 24.47, 42.33, 42.30, 42.30, 42.20, 42.20, 42.18, 42.17, 42.17, 42.23). Busy loops ran in their own process group with a 10 min timeout and are gone.
