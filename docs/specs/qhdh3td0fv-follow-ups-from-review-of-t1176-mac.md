---
task_uid: "01m7fmqtztb5dwtgqhdh3td0fv"
task: "T370"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T1176 (Mac T1651)

Findings of the review of T1176 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-editor/src/eol_commands.rs:25: EOL conversion, undo, and redo still dispatch as Effect::Nothing because Shown excludes line endings. The GPUI dispatcher consequently skips the change notification needed to refresh status and dirty indicators.
  fix: Include line endings in the dispatcher's change comparison and add registry-dispatch tests for conversion, no-op, undo, and redo.
  instances: crates/fe-editor/src/eol_commands.rs:25, crates/fe-editor/src/eol_commands.rs:35, crates/fe-buffer/src/buffer.rs:1070, crates/fe-buffer/src/buffer.rs:1116
