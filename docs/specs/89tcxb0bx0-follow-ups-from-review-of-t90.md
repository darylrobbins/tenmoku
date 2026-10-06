---
task_uid: "01m47ma6zr1yanrx89tcxb0bx0"
task: "T93"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T90

Findings of the review of T90 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-editor/src/lib.rs:1196: History::record discards all batches when a call produces more than one, and FoldSpan::follow then unconditionally drops touched folds. This regresses grouped undo and redo, even when every batch preserves the header's row structure. Reproduce with "top\ndef a():\n    one\n    two\nend": fold rows 1..=3 in a sibling, group insertions of "X" at 6 and "Y" at 7 into one transaction, synchronize the sibling, then undo and synchronize again. The fold disappears although both inverse batches only delete header characters; the base implementation preserves it. Multiple edits through one extracted &mut Buffer have the same regression. The added tests omit grouped undo/redo.
  fix: Retain structural metadata for every applied batch, including batches generated inside undo/redo and edits through an extracted mutable borrow, rather than depending on intervening guard dereferences. Add fold-preservation regressions for grouped undo and redo and multiple benign header edits through one mutable borrow, covering sync_display and BufferMut drop.
  instances: crates/fe-editor/src/lib.rs:1196, crates/fe-editor/src/lib.rs:1368
