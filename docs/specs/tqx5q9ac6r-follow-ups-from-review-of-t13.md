---
task_uid: "01m4pc47h532ar8htqx5q9ac6r"
task: "T17"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T13

Findings of the review of T13 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-display/src/layout.rs:900: The three scroll attempts can all leave the caret offscreen, after which unwrap_or(0) returns the first invisible layout. Using the regression helper with offset 40, head 60, and an 8200-character monospace inlay reproduces this: scroll 327.5 includes the inlay and places the caret at x=82600; scroll 82327.5 excludes it because extending the slice exceeds MAX_SHAPED_ROW_BYTES; scroll 560 includes it again. None shows the caret, so the original wide-inlay bug remains at the slice-size boundary.
  fix: Reconcile the slice with the inlay displacement so a slice containing the caret can be shaped at the required scroll while respecting the buffer-byte limit. Add a regression with an 8200-character inlay and assert the actual caret rectangle is visible; do not fall back to a known invisible candidate.
  instances: crates/fe-display/src/layout.rs:900
