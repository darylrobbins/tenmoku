---
task_uid: "01m4kks4mtb8a0y2wk1n28r3mv"
task: "T333"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "In tests::lsp_diagnostics::stale_events_are_rejected, after s.fake.crash() the test blocks with the non-pumping wait helper until Workbench::lsp_restarting_sent() has gone up, before clock.wait_for_sleep(1s) and advance(1s); the test's other steps and assertions are unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "Only crates/fe-workbench/src/lsp.rs and crates/fe-workbench/src/tests/lsp_diagnostics.rs change, apart from the task's own spec under docs/specs/ and its decision record; the new counter and accessor are #[cfg(test)], like diagnostics_sent; no change to crates/fe-lsp (server.rs STDERR_GRACE, clock.rs, tests/fake.rs) and no .config/nextest.toml retry"
    required: true
    manual: false
  - id: "C3"
    text: "The decision file shows the cause: with a temporary, uncommitted 50 ms thread::sleep in drain_stderr between clock.sleep(STDERR_GRACE) and its read loop (crates/fe-lsp/src/server.rs), the test fails at base with 'fake: timed out waiting for initialized: []' and passes with the fix"
    required: true
    manual: false
  - id: "C4"
    text: "20 consecutive runs of scripts/ci-env.sh cargo nextest run --profile ci -p fe-workbench -E 'test(=tests::lsp_diagnostics::stale_events_are_rejected)' pass while the full fe-workbench suite runs alongside, with the load average and results in the decision file"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
---

# Flaky under load: fe-workbench tests::lsp_diagnostics::stale_events_are_rejected: wait for Restarting before waiting out the backoff

## Problem

`fe-workbench tests::lsp_diagnostics::stale_events_are_rejected` fails under load and passes when run alone. It failed in gate
runs `01m4hsnb6xsvzb6j012xjs6fxa` (10-10 02:18Z), `01m4jbb8p8c5kwhmnr0j03vymk` (07:27Z) and
`01m4jknfgd356en7jeqt744jed` (09:52Z), each time after about 10.2–10.5 s with:

```
panicked at crates/fe-lsp/src/tests/fake.rs:433:13:
fake: timed out waiting for initialized: []
```

The logs are in `~/.local/state/regatta/runs/<run>/test.fastedit.log`. The panic comes from
`s.fake.wait_initialized_at(2)` in the restart step (`crates/fe-workbench/src/tests/lsp_diagnostics.rs` ~181–184).
Launch 2 never happens.

```rust
s.fake.crash();
s.clock.wait_for_sleep(Duration::from_secs(1));
s.clock.advance(Duration::from_secs(1));
s.fake.wait_initialized_at(2);
```

When a session ends, two 1 s sleeps are armed on the `ManualClock`, one after the other on the server thread:

1. `drain_stderr` (`crates/fe-lsp/src/server.rs` ~1590) arms `clock.sleep(STDERR_GRACE)` (1 s) once `session_over`
   drops. The fake's stderr pipe never closes, because `Launched` holds the writer (`crates/fe-lsp/src/tests/fake.rs`
   ~917–959), so it always reaches that line. It then finds nothing ready to read and drops the sleep. The sleep is
   armed only between `ManualClock::sleep`'s `notify_all` and that drop.
2. After `join3` returns, `Server::run` sends `ServerStatus::Restarting(1)` and only then arms the 1 s backoff in
   `Carry::wait_out` (`server.rs` ~473–476, `crates/fe-lsp/src/restart.rs` ~184).

`ManualClock::wait_for_sleep(1s)` (`crates/fe-lsp/src/clock.rs` ~110) matches any uncanceled sleep with 1 s left. The
`notify_all` from step 1 wakes the test thread. Under load the server thread can be preempted before it drops the grace
sleep, so the test catches the grace sleep instead of the backoff. `advance(1s)` then fires only the grace sleep. The
backoff is armed afterwards, with its deadline at 2 s, and never fires, so launch 2's record stays `[]`. This confirms
the lead's guess.

fe-lsp's own crash tests avoid this race. Each one waits for `ServerStatus::Restarting(n)` before `wait_for_sleep`
(`tests/lifecycle.rs` ~277, `tests/kill.rs` ~494, `tests/restart.rs` via its callers). So does fe-workbench's
`tests::lsp::status_item_follows_the_server`, which calls `wait_item("…restarting (1/5)")` before its
`wait_for_sleep`. By the time `Restarting` is sent, the grace sleep has been dropped. After that, the only 1 s sleep
that can still arm is the backoff.

This test can't use `wait_item`. Pumping the workbench would deliver the queued "stale across the restart" diagnostic
before the restart, and that diagnostic is what the step checks (`assert!(s.queued() > 0)` and `rejected_first`).

No production race: the grace timer is harmless outside a manual clock.

## Change

- `crates/fe-workbench/src/lsp.rs`: next to the `#[cfg(test)] diagnostics_sent` counter, add a `#[cfg(test)]`
  `Arc<AtomicUsize>` that the manager sink (~513) increments for each
  `LspEvent::Status { status: ServerStatus::Restarting(_), .. }`. Add a
  `#[cfg(test)] pub(crate) fn lsp_restarting_sent(&self) -> usize` beside `lsp_diagnostics_sent` (~1234).
- `crates/fe-workbench/src/tests/lsp_diagnostics.rs`: read the count before `s.fake.crash()`. After the crash, block
  with the file's non-pumping `wait` helper until the count has gone up, then call `wait_for_sleep(1s)` and
  `advance(1s)` as before. Add a short comment saying why: the stderr drain's 1 s `STDERR_GRACE` comes first. The
  test's other steps and assertions stay as they are.
