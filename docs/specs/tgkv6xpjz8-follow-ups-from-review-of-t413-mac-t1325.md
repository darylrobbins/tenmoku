---
task_uid: "01m6fare4k8hmnpxtgkv6xpjz8"
task: "T424"
rev: 3
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Make targets with literal parentheses (not archive-member syntax) are listed as make tasks, and archive-member syntax still is not; tests for both pass"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
---

# Follow-ups from review of T413 (Mac T1325): make targets with literal parentheses are told apart from archive members

## Problem
Review of T413 (Mac T1325, tasks.toml and detection) left one medium finding after round 3, taken as a known limit:
- F7 crates/fe-workbench/src/task_defs.rs:669 and :711: literal parentheses in a make target name are still mistaken for GNU make archive-member syntax (lib(member)), so valid literal targets are combined or dropped.
## Change
Recognise real archive-member syntax (a word of the form name(member...) with no space) before grouping or filtering parentheses; otherwise keep the literal text as a target name. Add tests for literal-parenthesis targets next to the archive-member cases, and remove the known-limit comment.
