# stale_events_are_rejected: wait for Restarting before the backoff

Status: **fixed**, measured 2026-10-10 (T333).

## Cause

After a crash two 1 s sleeps arm on the `ManualClock`: the stderr drain's
`STDERR_GRACE` (dropped almost at once) and then the restart backoff, armed
after `ServerStatus::Restarting`. `wait_for_sleep(1s)` matches either. Under
load the test caught the grace sleep, `advance(1s)` fired only that, and the
backoff (deadline 2 s) never fired, so launch 2 never happened.

The test now blocks, without pumping the workbench, until the manager sink has
sent a `Restarting` (`Workbench::lsp_restarting_sent()`, `#[cfg(test)]`). By
then the grace sleep is gone and only the backoff can arm.

## Evidence

C3: a temporary, uncommitted `thread::sleep(50 ms)` in `drain_stderr` between
`clock.sleep(STDERR_GRACE)` and the read loop (`crates/fe-lsp/src/server.rs`).
At base (workbench files from HEAD~1) the test failed 3 of 3, each after
~10.2-10.3 s (the `wait_initialized_at(2)` timeout, "timed out waiting for
initialized"). With the fix it passed 3 of 3. The sleep was reverted.

C4: 20 consecutive runs of
`scripts/ci-env.sh cargo nextest run --profile ci -p fe-workbench -E 'test(=tests::lsp_diagnostics::stale_events_are_rejected)'`
while four back-to-back full `cargo nextest run -p fe-workbench` runs went
alongside: 20 of 20 passed. Load average (1 min) at start 47.8, at end 89.6.

C5: the four full fe-workbench runs each ended `255 tests run: 255 passed, 2 skipped`.
