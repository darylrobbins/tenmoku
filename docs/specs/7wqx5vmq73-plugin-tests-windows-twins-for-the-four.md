---
task_uid: "01m74rgv9qapc7sn7wqx5vmq73"
task: "T580"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The four cases in the spec run on every OS or keep cfg(unix) only as Unix-mode checks with a named Windows twin: a_program_run_leaves_the_call_its_time_on_the_buffers and the dangling-symlink save checks run on Windows too; an_unwritable_cache_directory_still_loads and a_fifo_at_the_entry_is_a_miss each have a named Windows twin (listed in the finish notes)."
    required: true
    manual: false
  - id: "C2"
    text: "The tests pass on Linux unchanged in behaviour."
    required: true
    manual: false
---

# Plugin tests: Windows twins for the four cfg(unix) cases W-h missed (follow-up to T578, Mac T1694)

# Plugin tests: Windows twins for the four cfg(unix) cases W-h's inventory missed (follow-up to T578 / Mac T1694)

The plugins stage 3 design (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 2.3, "The rule after P5")
says no kiln-plugin, fe-plugin or fe-workbench plugin test is `cfg(unix)` unless it checks Unix modes
themselves, and each of those has a named Windows twin. W-h's inventory (section 2.2) missed four
cases, which T578's review (F1, review 01m74r1xbgzs6ep6qbvn9e1117) found outside T578's scope:

1. `crates/fe-workbench/src/plugin_bridge_tests.rs` `a_program_run_leaves_the_call_its_time_on_the_buffers`
   (~:582): its program is a `/bin/sh` script. Run it on every OS with a `.cmd` stand-in, using the same
   ten-line `script(dir, name, sh, cmd)` helper pattern P4/P5 use (copy it, no cross-crate test-support
   dependency).
2. `crates/fe-workbench/src/plugin_access_tests.rs` the dangling-symlink save checks (~:362, a symlink in the
   workspace to a missing file outside it, directly and through a second link): add the Windows case with
   `std::os::windows::fs::symlink_file`, which the admin runner can create.
3. `kiln/crates/kiln-plugin/tests/cache.rs` `an_unwritable_cache_directory_still_loads` (~:188): a Unix-mode
   check; keep it `cfg(unix)` and add a named Windows twin that makes the cache directory unwritable through
   its DACL (deny write to the current user), as P7's DACL twin does.
4. `kiln/crates/kiln-plugin/tests/cache.rs` `a_fifo_at_the_entry_is_a_miss` (~:233): Windows has no FIFOs in
   the file system. Keep it `cfg(unix)` and add a named Windows twin where the entry path is a non-regular
   file (a directory, or a symlink to a directory) and loading is a miss.

The Linux behaviour of every existing test stays unchanged. Type-check the Windows code with
`cargo check --target x86_64-pc-windows-msvc --tests` for the crates touched, where the build scripts allow
it (see the design's "Checking off the Mac"); only the Mac's windows CI job runs it.
