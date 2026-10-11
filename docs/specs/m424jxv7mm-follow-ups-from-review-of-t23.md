---
task_uid: "01m4ph7scf12jftgm424jxv7mm"
task: "T25"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T23

Findings of the review of T23 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-terminal-view/tests/view.rs:323: The regression records shaped glyph positions but never asserts them. Grid checks still inspect only caller-provided origins and input text; the new glyph assertions inspect counts. Incorrect glyph offsets, or missing glyphs for the narrow characters and cursor text, would still pass. This leaves the requested glyph-placement coverage incomplete.
  fix: Assert the reported glyph byte indices and x offsets for each tested cell, including cursor text, and verify that origin plus glyph offset places each glyph at its expected grid column.
  instances: crates/fe-terminal-view/tests/view.rs:323
