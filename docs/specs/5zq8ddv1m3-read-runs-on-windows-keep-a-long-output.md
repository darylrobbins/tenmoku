---
task_uid: "01m7tcz874j4zppb5zq8ddv1m3"
task: "T1976"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "Cause found and written in the decision file with evidence (Windows CI log or probe): why the 3,200-char compiler-artifact line reaches Capture (task_run.rs) as 255 chars on Windows."
    required: true
    manual: false
  - id: "C2"
    text: "Production fix: run_task_reading hands its keep callback each logical output line whole on Windows, however long and whatever the terminal's columns."
    required: true
    manual: false
  - id: "C3"
    text: "T1973's Windows-only short src_path in tests::debug_session::a_read_run_keeps_its_output_whatever_the_scrollback is reverted: src_path is 3000 chars on every OS with no cfg(windows) branch, assertions unchanged (one kept line equals the artifact, launch::cargo_artifact finds the executable)."
    required: true
    manual: false
  - id: "C4"
    text: "New windows-only test crates/fe-terminal/tests/conpty_long_line.rs (#![cfg(windows)], like conpty.rs): an 80-column ConPTY session whose OutputTap hears a 3,200-char line gets it back whole."
    required: true
    manual: false
  - id: "C5"
    text: "Windows CI on the lead's draft PR: the windows job passes the restored debug_session test and conpty_long_line; the restored test failed on Windows before the production fix (link both runs in the decision file); Linux and macOS jobs pass it too."
    required: true
    manual: false
  - id: "C6"
    text: "fe-terminal output_tap tests and fe-workbench read-run/task_run tests still pass on macOS."
    required: true
    manual: false
---

# Read runs on Windows keep a long output line whole: the debugger's cargo compiler-artifact line reaches the reader uncut

On Windows CI (PR #8, run 44558333306, commit 152a3d2e1), `tests::debug_session::a_read_run_keeps_its_output_whatever_the_scrollback` fed a cargo `compiler-artifact` JSON line of about 3,200 characters through a read run. The line the debugger kept was 255 characters long. It ended partway through `src_path`, and the `"executable"` field was gone. A read run that cuts long lines on Windows can lose the built executable's path when debugging a cargo target. A copy of the job log is in the lead's notes (evidence/t1973-windows-r2-job.log; ask the lead for the relevant lines).

The cause is not confirmed. Read-run output reaches `Capture::hear` in `crates/fe-workbench/src/task_run.rs` through `fe_terminal::OutputTap` (`pty_loop.rs`), and `Capture` itself only caps at 16 MiB. So the cut probably happens upstream: ConPTY re-renders output with hard wraps or cursor moves at the terminal's column count, or something chunks it. Find the real cause before fixing.

T1973 worked around the failure by shrinking the test's `src_path` to 20 characters on Windows only. This card reverts that and fixes the production path.

Windows evidence comes from a draft PR the lead pushes; workers don't push. Ask the lead before any cold build and before `rgt finish`.
