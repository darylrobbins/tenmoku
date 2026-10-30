---
task_uid: "01m65f5cvaq4btv1mtwabgpgam"
task: "T1244"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Cause: under load (T1213 snapshot gate, an fe-vim-only change; log ~/.local/state/regatta/runs/01m65brw383463n5q5qv4gjnfd/test.fastedit.log) assert_eq!(requests(\u0026s, PREPARE).len(), 2) saw 1 after 1.1 s. Suspected: the test toggles right after nav.show_call_hierarchy without waiting for the first prepare to reach the fake server, so toggle_direction (hierarchy_panel.rs:390) cancels it before it is recorded. Confirm or refute this in the spec's notes."
    required: true
    manual: false
  - id: "C2"
    text: "Fix the test only: before hierarchy.toggle_direction, tick the clock until requests(\u0026s, PREPARE).len() == 1 (the first prepare is really pending), keeping the 500 ms reply delay and every existing assertion (2 prepares, outgoing asked about [\"a\"] only, no incoming, no toast). If the cause is in production code (hierarchy_panel.rs, or the LSP manager loses a send), block and tell the lead: it becomes a bugfix with a regression test in a new file under crates/fe-workbench/src/tests/."
    required: true
    manual: false
  - id: "C3"
    text: "Proof: the test passes 20 runs in a row while the rest of the fe-workbench lib tests run alongside it, recorded in the spec's notes; existing tests pass unedited; clippy passes."
    required: true
    manual: false
---

# Flake: fe-workbench hierarchy_panel_tests toggling_while_the_prepare_is_pending_keeps_it_in_the_new_direction asserts 1 prepare request, not 2, under load (hierarchy_panel_tests.rs:469)

## Implementation notes

- C1 confirmed: the test toggled straight after `nav.show_call_hierarchy`, so under load `toggle_direction` (hierarchy_panel.rs:390) took the pending prepare and restarted it before the first request was recorded by the fake server; only 1 prepare was ever seen. Production code is fine.
- C2: the test now ticks the clock (1 ms steps) until `requests(&s, PREPARE).len() == 1` before toggling. The 500 ms reply delay and all assertions are unchanged.
- C3: the test passed 20/20 runs in a row while the rest of the fe-workbench lib tests ran alongside (a full lib run with `--skip hierarchy_panel_tests` started concurrently). Unrelated: `path_commands_tests::chords_drive_copy_and_reveal` failed in 2 full-suite runs here; it is in a different module and not touched by this change (not checked against base).
