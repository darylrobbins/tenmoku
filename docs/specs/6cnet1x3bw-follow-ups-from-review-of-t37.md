---
task_uid: "01m4r0asmfr5n5766cnet1x3bw"
task: "T39"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T37

Findings of the review of T37 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-display/src/layout.rs:943: The fallback can cycle between invisible layouts and still select a known invisible caret after eight retries. With the new Double regression's inputs, but changing both selection and primary_head to 8000, the retries alternate between (scroll, caret) = (79960, 323600) and (299727.5, 80000). The first slice includes the inlay; following its caret selects a slice that omits the inlay and clamps scrolling to the indexed row width. Following that caret restores the first slice. Neither layout is visible, so the minimum-miss selection still violates the requested fix.
  fix: Reconcile the slice and scroll around the actual caret, preserving the shaped displacement and appropriate scroll bounds instead of repeating the same view-indexed shaping operation. Ensure cycle or retry exhaustion produces a visible caret, and extend the Double regression to cover a caret farther past the inlay, such as offset 8000.
  instances: crates/fe-display/src/layout.rs:943
