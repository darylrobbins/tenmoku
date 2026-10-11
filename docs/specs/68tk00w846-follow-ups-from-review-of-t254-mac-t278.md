---
task_uid: "01m4kxpvkt7pgynm68tk00w846"
task: "T14"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T254 (Mac T278)

Findings of the review of T254 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-display/src/layout.rs:439: A viewport filled entirely by a block has no laid-out text rows, so dragging beyond the block into a text row reaches the empty-row fallback and returns offset 0. For the test document, a five-row block above display row 5, scroll.y=100 and height=100 produces this at y=-1 and y=101. Selection dragging consequently jumps to the document start instead of a nearby buffer boundary. The tests cover empty frames only through hits inside a trailing block.
  fix: Provide block-aware edge clamping when Frame::rows is empty, using a nearby block's buffer anchor rather than returning 0. Add regression coverage for dragging above and below a viewport entirely occupied by an interior block.
  instances: crates/fe-display/src/layout.rs:439, crates/fe-display/src/layout.rs:573



Moved from the Mac board: Mac T278. Its dependencies are finalized on Mac and arrive here with the mac/main sync.
