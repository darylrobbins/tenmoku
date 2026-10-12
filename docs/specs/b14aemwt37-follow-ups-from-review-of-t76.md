---
task_uid: "01m4r5e7sx06vaz1b14aemwt37"
task: "T78"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T76

Findings of the review of T76 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-editor/src/rows.rs:135: Comparing the two units for equality misses text-identical permutations of unequal-sized blocks. Moving "[a\na]\na" down or "a\n[a\na]" up leaves the text unchanged, but this guard fails because "a" differs from "a\na". The command then moves the selection, reports Changed, creates an undo transaction, and marks the buffer dirty. Multi-row empty blocks and repeated folded units have the same problem.
  fix: Compare the combined rows before and after the swap, including their separating newline, rather than requiring the units themselves to be equal. Skip committing globally text-identical moves and add regression tests in both directions for unequal-sized repeated non-empty and empty blocks, including folded units.
  instances: crates/fe-editor/src/rows.rs:135
