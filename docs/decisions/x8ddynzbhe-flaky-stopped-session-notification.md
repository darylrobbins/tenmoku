# T719: a_notification_to_a_stopped_session_is_not_found flake

Cause: the test restarts the fake server five times by advancing the manual clock after
`wait_for_sleep(backoff)`. Other timers on that clock can have the same remaining length,
so the wait could match one that was not the restart's, the advance missed the real timer,
and `wait_initialized_at(n + 1)` timed out after 60 s ("timed out waiting for initialized: []").

Fix (test only, crates/fe-workbench/src/plugin_lsp_tests.rs): the restart loop no longer blocks on
`wait_for_sleep`. It polls until the next launch has started, and advances the clock by the backoff
only while no launch has started and a sleep of exactly that length is armed (`clock.sleeping()`),
so it never waits for a sleep that already fired. Gives up after 30 s.

Evidence (before): with 6 copies of the plugin_lsp_tests suite as load, the original test failed 5 of 12
runs, all at the wait for the next launch.

Evidence (after), commands run from the worktree with CARGO_TARGET_DIR=$PWD/target-t719:

    cargo test -p fe-workbench --lib --no-run
    cargo test -p fe-workbench > load.log 2>&1 &        # the load: whole suite, 3688 lib tests
    # once load.log showed "running 3688 tests", 20 times in a row:
    target-t719/debug/deps/fe_workbench-1fb7e2d7ed53b27d --exact \
      plugin_bridge::plugin_lsp::plugin_lsp_tests::a_notification_to_a_stopped_session_is_not_found

Result: 20 of 20 runs passed ("1 passed; 0 failed"); the full suite was still running at the start of
every run. An earlier 20/20 pass, taken while the suite was still compiling, is not counted.
