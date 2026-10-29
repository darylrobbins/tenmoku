---
task_uid: "01m6479bb1f3nwn9yhthx6q0rb"
task: "T1238"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Each clock.advance(5s) in a_cut_checks_again_once_servers_answer (both sites) and a_cut_checks_its_folder_again_once_servers_answer is preceded by a wait that the fake server's 5 s reply sleeper is armed on the ManualClock (wait_for_sleep(5s), or a wait for two 5 s entries in clock.sleeping() if the client's own timer is also 5 s), as start_held_rename in src/tests/lsp_file_operations.rs does"
    required: true
    manual: false
  - id: "C2"
    text: "No production code or other file changes; the tests keep their assertions and the 60 s until() bound"
    required: true
    manual: false
  - id: "C3"
    text: "Proof under load: both tests pass 30 runs each with --retries 0 while the machine is loaded (e.g. 24 yes burners), via cargo nextest run -p fe-workbench -E 'test(a_cut_checks_again_once_servers_answer) | test(a_cut_checks_its_folder_again_once_servers_answer)'"
    required: true
    manual: false
  - id: "C4"
    text: "cargo clippy -p fe-workbench --tests and cargo fmt --check are clean"
    required: true
    manual: false
---

# Flake: fe-workbench project_panel_clipboard_tests a_cut_checks_again_once_servers_answer and a_cut_checks_its_folder_again_once_servers_answer hang 60 s waiting for the toast under load

Test-only flake fix (no regression test: a test fix can't fail at base on demand; prove it with repeated runs under load).

## Problem
a_cut_checks_again_once_servers_answer failed on main 0699e7f (run 01m62wsa0thq) and a_cut_checks_its_folder_again_once_servers_answer failed in T564's finish gate (run 01m645882q2g7cwa2pbf2s21w3). Both hang for 60 s and then panic "timed out waiting for the toast" (src/tests/lsp.rs:522) under heavy load; they normally take 0.5 s.
Cause: modules() (project_panel_clipboard_tests.rs:467) scripts the fake's willRenameFiles reply with a 5 s delay on the shared ManualClock. The test does fake.wait_for("willRenameFiles") then clock.advance(5s). The fake records the request before it calls clock.sleep(delay) (fe-lsp/src/tests/fake.rs:1178-1180). If advance runs in that gap, the fake's sleeper lands at 10 s and never fires, so the paste never re-checks and no toast appears. Production is fine: the client timer is armed synchronously in will_rename_files.

## Change
Before each clock.advance(Duration::from_secs(5)) (lines 587, 627, 952) wait until the fake's 5 s sleeper is armed, as start_held_rename (src/tests/lsp_file_operations.rs:421) does with s.clock.wait_for_sleep(Duration::from_secs(5)). Check the default lsp.timeouts for willRenameFiles: if the client's own timer is also 5 s, wait for two 5 s entries in clock.sleeping() so the fake's is the one awaited. Test code only.
