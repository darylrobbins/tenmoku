# plugin_manage uninstall flake (T383, Mac T1871)

`confirming_uninstall_removes_everything` panicked "timed out waiting for the
plugins to run" after 31 s under a load average near 28.

**Cause.** The wait was not the problem: it is a 30 s poll of the plugin states.
The plugins never reached `Running` because the plugin host's real call
deadline (100 ms) also covers instantiate and init. On a starved thread that
deadline passes, the host stops the plugin for good (`Timeout`), and no amount
of waiting brings it back. `plugin_host_tests` already raises
`limits.call_deadline` for this reason; `plugin_manage_tests::start_with` did not.

**Fix.** `start_with` sets `call_deadline` to 30 s, so every plugin_manage test
(they share `start_with`) loads without a deadline stop. No production change:
the 100 ms limit is the behaviour under test elsewhere.

**Proof.** With a parallel build of another crate in a separate target dir and a
host load average of ~35:

    CARGO_TARGET_DIR=/path/to/scratch/bt cargo build -p fe-chat &
    for i in $(seq 20); do cargo test -p fe-workbench --lib plugin_manage; done

All 20 runs: `22 passed; 0 failed` (3.4 s to 7.5 s each).
