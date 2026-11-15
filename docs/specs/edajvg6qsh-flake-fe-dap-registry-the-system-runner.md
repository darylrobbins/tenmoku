---
task_uid: "01m7fkmdnr3pgvymedajvg6qsh"
task: "T661"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "the printing and failing cases of the_system_runner_stops_a_descendant_holding_its_stdout use a runner with a generous deadline; only the stdout-holding descendant case keeps the short deadline, and still proves it is cut off at it; SystemRunner is unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "20 runs of the fe-dap registry tests alongside a parallel cargo build all pass, recorded in the decision file"
    required: true
    manual: false
---

# Flake: fe-dap registry the_system_runner_stops_a_descendant_holding_its_stdout times out the printing case (500 ms covers a whole script spawn on macOS) (Mac T1877)

fe-dap `tests::registry::the_system_runner_stops_a_descendant_holding_its_stdout` (crates/fe-dap/src/tests/registry.rs ~l.985, from Mac T1843) fails now and then on macOS: `left: None, right: Some("/opt/rust\n")` at l.996. It failed the Mac stack 185 gate (run73, load ~20) and 1 of 5 targeted reruns at low load; the passes take ~1.2 s.

Cause: one `SystemRunner::with_timeout(Duration::from_millis(500))` serves every case in the test, including the first, which only runs a script printing `/opt/rust`. Spawning `/bin/sh` for a script on macOS can take longer than 500 ms now and then, so the runner times out a program that would have printed, and returns None.

Fix (test only): give the printing and failing cases a runner with a generous deadline (e.g. 10 s), and keep the short deadline only for the case that proves a descendant holding stdout is cut off at the deadline (assert it returns within a bound well below the generous one). Don't change SystemRunner.

Proof: 20 runs of the fe-dap registry tests alongside a parallel cargo build, all passing, recorded in a decision file.
