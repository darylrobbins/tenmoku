---
task_uid: "01m5pawp7yqg8d3hg33gt4k34y"
task: "T307"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Pending edits are keyed by setting, scope and target file, so two edits to the same key in different files or workspace roots never collide; a test per case"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Key the Settings tab's pending edits by setting and target file (T296 F15)

T296 (Mac T1097) review round 3 F15: pending edits distinguish scopes but still collide across target files and workspace roots. T296 documents it as a limit; this card removes it.
