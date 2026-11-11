---
task_uid: "01m74ccaewqh4gfb69ce2fe6q7"
task: "T578"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The five tests named in section 2.2 W-h (`a_granted_program_runs_and_an_ungranted_one_is_denied`, `a_granted_program_that_outlasts_the_call_deadline_completes`, `a_spawn_in_flight_ends_before_the_plugin_unloads`, `a_reauthorized_program_reloads_the_plugin`, `an_event_handler_edits_its_buffer_after_a_slow_process`) and their helpers (`script`, `spawner`, `spawn`, `SPAWN`, `NAPPER`) run on every OS, using P4's script helper pattern."
    required: true
    manual: false
  - id: "C2"
    text: "No plugin test in kiln-plugin, fe-plugin or fe-workbench is cfg(unix) except checks of Unix modes, each with a named Windows twin (listed in the card's finish notes), and except the four tests the spec's Known limits leave to follow-up T580."
    required: true
    manual: false
  - id: "C3"
    text: "The tests pass on Linux unchanged in behaviour."
    required: true
    manual: false
---

# fe-workbench plugin host tests: Windows twins for the five cfg(unix) tests (P5, W-h) (Mac T1694)

# fe-workbench plugin host tests: Windows twins for the five cfg(unix) tests (P5, W-h)

Draft id P5 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 2.3 'The rule after P5'. If P4's helper cannot be shared across crates, copy the ten-line helper into plugin_host_tests.rs rather than adding a cross-crate test-support dependency.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Known limits

Section 2.2 W-h's inventory for P5 named only the five tests in plugin_host_tests.rs. Four further
`cfg(unix)` plugin tests sit outside this card's scope and are left to follow-up T580 (review
01m74r1xbgzs6ep6qbvn9e1117, F1): fe-workbench plugin_bridge_tests.rs
`a_program_run_leaves_the_call_its_time_on_the_buffers`, the dangling-symlink save checks in
fe-workbench plugin_access_tests.rs, and kiln-plugin tests/cache.rs `an_unwritable_cache_directory_still_loads`
and `a_fifo_at_the_entry_is_a_miss`. Until T580 lands, the rule after P5 holds for every plugin test
except those four.

## Implementation notes

- The five tests and helpers lost `cfg(unix)`. `script(dir, name, sh, cmd)` is the P4 helper copied into the file (`write_script` + `resolve_program`); `.cmd` scripts stand in on Windows, sleeps are `ping -n N+1 127.0.0.1`, and expected output uses `LINE_END` (CRLF on Windows).
- fe-workbench has no other cfg(unix) plugin tests, so no further Unix-mode twins are owed there. Not run on Windows from this Linux lane.
- Windows twins (finish notes): the five tests in plugin_host_tests.rs are themselves the cross-OS versions (`write_script` has a `cfg(unix)` mode branch and a `cfg(windows)` `.cmd` branch); no other plugin test in this card's scope is `cfg(unix)`. The four tests in Known limits go to T580.
