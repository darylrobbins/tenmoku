---
task_uid: "01m4p3yxczf7agjbaseajvmkzw"
task: "T437"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T392

Findings of the review of T392 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/folds.rs:258: Recursive opening removes overlapping folds, not just the selected fold and its descendants. FoldMap explicitly permits overlapping ranges, and unfold_rows removes every intersecting fold. With closed folds 0..=4 and 4..=6 and the caret on row 0, zO or zA removes both, although 4..=6 extends outside the selected fold. The new branching test covers only fully nested ranges and misses this regression.
  fix: Remove only the selected fold and folds fully contained within its bounds, preserving overlapping folds that extend outside it. Add regression coverage for both zO and zA with overlapping ranges alongside the branching nested-fold test.
  instances: crates/fe-vim/src/folds.rs:258
