---
task_uid: "01m4p4xf7n78c9tgdtvw8cky0m"
task: "T9"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T6

Findings of the review of T6 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-display/src/layout.rs:664: Inserting an inlay into a long-row slice shifts only the shaped mapping; the RowIndex paths remain unshifted. With 9000 ASCII characters, 10-pixel cells, and "XYZ" at offset 2, an offset at the slice's right boundary maps to (offset + 3) * 10, while the next offset falls back to offset * 10, making positions move backwards. Clicks beyond the slice likewise return offsets three characters too far right. Long-row autoscroll and maximum horizontal scroll also ignore the insertion, so a sufficiently wide inlay can leave the primary caret offscreen. The new long-row test checks only an omitted inlay and misses these paths.
  fix: Account for the laid-out inlay's measured pixel displacement in both RowIndex mapping directions and in long-row horizontal autoscroll and scroll limits, while preserving the rule that an inlay outside the shown slice adds nothing. Add regression tests retaining the LongRows used for layout, covering slice-boundary mappings, clicks beyond the slice, and autoscroll with an inlay before the caret.
  instances: crates/fe-display/src/layout.rs:664, crates/fe-display/src/layout.rs:295, crates/fe-display/src/layout.rs:314, crates/fe-display/src/layout.rs:709, crates/fe-display/src/layout.rs:727
