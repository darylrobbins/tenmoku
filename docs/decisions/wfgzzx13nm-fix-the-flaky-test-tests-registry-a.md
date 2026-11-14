# T652: flaky `a_fixed_command_brings_back_the_documents_of_a_failed_server`

The flake did not reproduce here (0 failures in 40 serial, 260 concurrent
runs before the change), so the fix removes the races found by reading.

The test took the failing server's id from `manager.server_of(a)` right after a
settings or spec change, then waited on that id's `Starting`/`Stopped` events.
The document moves off the old server only as the restart gets to it, so
`server_of` could be `None` or the old server. It also asserted the placement
after the second launch with no wait.

Changes (test code only, `crates/fe-lsp/src/tests/registry.rs`): a
`next_failure` helper reads the failed server's id from the "pyright not
found" `Stopped` event, and the final placement check uses `wait_until`.

Evidence (Rust, not `go test`): with the 24-core host already at load ~45,
6 concurrent runs of the whole fe-lsp suite (`--test-threads=48`) plus 20
consecutive runs of the test: 0 failures of this test. Unrelated tests
(`first_answer::primary_non_empty_answer_wins_even_when_add_on_is_first`,
`formatting::tests::asked_while_the_server_restarts`) flaked under that load,
with and without this change.
