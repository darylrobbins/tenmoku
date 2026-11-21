# T732: flaky `a_next_failure_delayed_past_stop_changes_nothing_and_the_session_still_ends`

## Cause
T727 made the test wait for the late failure, but the fake adapter's
`terminate` handler sent `terminated` (via `end()`) *before* releasing the
held `FailAfter` failure. The session closes the transport on `terminated`, so
the failure's send raced the close and was sometimes never recorded; the wait
then timed out.

## Fix (test-support code only: `crates/fe-dap/src/fake.rs`)
When a terminate arrives with the run still going, release the held failure and
send it before `end()`. When the run has already ended, the order is unchanged
(`tests::fake::a_resume_failure_delayed_until_after_a_terminate` pins it). The
test still proves a late failure changes nothing.

## Evidence
`cargo nextest run -p fe-dap --no-fail-fast`: 209/209 pass. The target test, 20
consecutive nextest runs while four loops of the full fe-dap suite ran
concurrently (load average about 52): pass=20/20.
