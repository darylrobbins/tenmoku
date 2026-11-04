---
task_uid: "01m6fsfdw9bnxb1haszhgr2agc"
task: "T429"
rev: 4
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Just recipes with escaped-newline headers are listed and a multiline [private] attribute hides its recipe; tests pass"
    required: true
    manual: false
  - id: "C2"
    text: "A make variable named define does not suppress later targets; tests pass"
    required: true
    manual: false
  - id: "C3"
    text: "Names beginning with '-' are run as tasks, not passed as options, with the label unchanged; tests pass"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
---

# Follow-ups from review of T413 (Mac T1325): just multiline headers and attributes, a make variable named define, and leading-dash task names

## Problem
Review p1vdhe of T413 (Mac T1325, tasks.toml and detection) left three medium findings after round 3, taken as known limits (C3 and C4 respecced to best-effort):
- F8 crates/fe-workbench/src/task_defs.rs:735, :753: the just scanner keeps state across newlines only inside strings; a recipe header continued over an escaped newline (`deploy env='prod':` split) is lost, and a multiline attribute (`[\nprivate\n]\nhidden:`) wrongly exposes `hidden`.
- F9 :591: a valid make variable assignment named `define` (`define := value`) is taken for a `define` directive and updates block depth, suppressing later targets.
- F10 :440, :522: detected names beginning with `-` are passed to the runner as options (npm/pnpm/yarn/bun scripts, make and just targets).
## Change
Accumulate whole logical headers and attributes across escaped newlines and open delimiters before classifying them; tell an assignment to a variable called `define` from the directive; protect leading-dash names with each runner's positional-argument syntax (`--`, or the runner's equivalent), keeping the displayed labels. Add tests for each case and remove the matching known-limit notes from task_defs.rs.
