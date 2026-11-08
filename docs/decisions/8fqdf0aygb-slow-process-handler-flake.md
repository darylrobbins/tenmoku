# an_event_handler_edits_its_buffer_after_a_slow_process under load (T330)

**Cause: the test, not the executor or the host.** The test ran the plugin with
the real `CALL_DEADLINE` (100 ms). That deadline excludes the time the handler's
program runs (it is charged to `PROGRAM_DEADLINE`) but counts everything else in
the call: the wasm itself and each buffer read/edit, which waits on the main
thread that the test drives from a 5 ms poll loop. Under load that remaining
work overruns 100 ms, `PluginHost::guarded` (`plugin_host.rs`, `passed()` after
the call) returns `Reason::Timeout`, and the plugin is stopped after it logged
`alpha` and before it logged `edited`. The poll loop then waits out its 30 s.
The 0.3 s process was never the problem; the poll loop does drive the executor
(`draw` runs `run_until_parked`), and no wakeup is lost.

**Evidence (reproduction).** On this 12-core host with 120-150 busy loops running
(load ~135-165), the unchanged test failed 3 of 8 runs, each after ~33 s. With the
failure message extended to print `plugin_states`, the failing runs showed:

    log acme.napper on plugin acme.napper: alpha
    state: Stopped(PluginDiagnostic { reason: Timeout,
        message: "a call ran past its 100ms deadline" })

Unloaded, the test passed. Same class as T488 (tests whose subject isn't the
deadline get a roomy one).

**Fix (test only, `plugin_host_tests.rs`).** The test's subject is that the
deadline doesn't count the program's time, so it keeps a deadline shorter than
the process but roomy for the handler's own work: call deadline 2 s, process
`sleep 2.5`. The failure message now includes `plugin_states`, so a stopped
plugin shows up instead of a bare 30 s wait.

**Verification.**

    cargo nextest run -p fe-workbench --lib \
      plugin_host::plugin_host_tests::an_event_handler_edits_its_buffer_after_a_slow_process \
      --retries 0

run 30 times in a row while a second `cargo build -p fe-workbench --tests` ran
(its own target dir) alongside 120 busy loops (load ~135): 30 of 30 passed.
