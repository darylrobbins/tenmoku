---
task_uid: "01m4kcxxgw5grhrmc5svrxt1kx"
task: "T6"
rev: 3
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench.tests::lsp::quitting_waits_for_slow_servers passes in the gate, with its existing assertions unchanged, including the \u003c 4 s bound"
    required: true
    manual: false
  - id: "C2"
    text: "Hurry is replaced by a QuitDriver that advances the shared ManualClock by 2 s only once both fakes' launch-1 records show shutdown and two 2-s sleeps are armed, and by 1 s only once both records pass shut_down (shutdown then exit) and exactly one 1-s sleep is left"
    required: true
    manual: false
  - id: "C3"
    text: "Each QuitDriver wait gives up after 10 s of real time and falls back to free-running advances, so quitting never hangs, and Drop panics naming the step that missed"
    required: true
    manual: false
  - id: "C4"
    text: "3 consecutive runs of the test (nextest -E 'test(=tests::lsp::quitting_waits_for_slow_servers)') pass, with each run's 1-min load logged in the decision file (the 10-runs-under-load proof was waived by the user on 2026-10-10: the sandbox refuses loaded test loops here, and the post-merge checks carry the load evidence)"
    required: true
    manual: false
  - id: "C5"
    text: "The worker's own commits change only crates/fe-workbench/src/tests/lsp.rs, and nothing in crates/fe-lsp. docs/specs/c5svrxt1kx-*.md comes from Regatta's own 'Spec T6' and 'Respec T6' commits, which the landing drops (as T9's landing 9751c0e shows); it is not part of the change and is not a finding"
    required: true
    manual: false
---

# Flaky under load: fe-workbench tests::lsp::quitting_waits_for_slow_servers: advance the 1-s exit timeout only after the fakes have recorded exit

## Problem

`fe-workbench.tests::lsp::quitting_waits_for_slow_servers` failed once under heavy load (Mac 1-min load
120-160 on 14 cores), in main check run `01m4kbtaknsxracbhaxba70mp6` on abbf76c. That commit changed only
regatta.toml, and its parent was green. It failed at `crates/fe-workbench/src/tests/lsp.rs:771`,
`assert!(shut_down(&python))`. pyright's launch-1 records were `initialize`, `initialized`,
`textDocument/didOpen` and `shutdown`, with no `exit`. Run alone, it passed 5 of 5. It is the only failure in
the 368 run logs under `~/.local/state/regatta/runs/` that ran this test.

The cause is the test's clock driver, not fe-lsp. `Hurry` (lsp.rs ~700) is a thread that keeps advancing the
shared `ManualClock` by the shortest sleep it finds, waiting nothing in between. In
`ServerActor::shut_down` (`crates/fe-lsp/src/server.rs` ~1405), the actor queues `exit` in async-lsp
(`socket.notify::<Exit>`) and then arms `EXIT_TIMEOUT` (1 s). `Hurry` sees that sleep and expires it about
1 ms later. The fake's server thread still has to read `exit` off the pipe and record it, and at load 150 it
didn't get to run in time. `FakeProcess::kill` then ended the session, so `exit` was written but never
recorded. rust-analyzer's fake has the same race: if its 1-s sleep expires before the actor sees it exit, it
is killed, and `assert_eq!(s.fake.kills(), 0)` fails.

In production, `exit` is handed over before the 1-s timeout is armed, and the server then has 1 s of real
time to read it. A server that hasn't gone by then is killed by design, which is the outcome this test
expects for pyright anyway. fe-lsp's own `tests::lifecycle` checks the same path deterministically: it calls
`fake.wait_for("exit")` before advancing the 1-s sleep.

## Change

Test-only, in `crates/fe-workbench/src/tests/lsp.rs`. Replace `Hurry` with a scripted driver (`QuitDriver`)
that takes the clock and both fakes (`s.fake` and `s.python`, launch 1). It is used only by this test, and it
advances the clock only when the states below hold, as `tests::lifecycle` does:

1. Wait until `clock.sleeping()` has two sleeps with exactly 2 s left (both `SHUTDOWN_TIMEOUT`s) and both
   fakes' launch-1 records show `shutdown`. Then advance 2 s.
2. Wait until both fakes' launch-1 records pass `shut_down` (`shutdown`, then `exit`).
3. Wait until `clock.sleeping()` has exactly one sleep with 1 s left. That is pyright's `EXIT_TIMEOUT`;
   rust-analyzer's is cancelled once the actor sees it exit. Then advance 1 s, so pyright is killed.
4. Until dropped, keep advancing the shortest sleep as `Hurry` does now.

Each wait gives up after 10 s of real time, records which step missed, and falls through to step 4. That
way quitting can't hang, and the driver's `Drop` panics naming the step. Keep the test's assertions as they
are, including the `< 4 s` bound. Change nothing in fe-lsp.


_Filed for the neon board by the Mac lead: seen once on a Mac main check under load (run 01m4kbtaknsxracbhaxba70mp6). Lands back on main through the lead._
