# T727: flaky `a_next_failure_delayed_past_stop_changes_nothing_and_the_session_still_ends`

## Cause
The test asserted the fake adapter had already sent the failed `next` response
right after `wait_ended()`. The fault (`FailAfter` on `Terminate`) answers
asynchronously, so under load the session could end before the response was
recorded.

## Fix (test code only, `crates/fe-dap/src/tests/session_uncertain.rs`)
Wait for the failure response with `fake.wait_until(...)` instead of reading
`fake.sent()` once.

## Evidence
20 consecutive runs of the test (`--test-threads=1`) while four concurrent
`cargo test -p fe-dap --tests` runs (integration suite) loaded the machine:
pass=20/20.
