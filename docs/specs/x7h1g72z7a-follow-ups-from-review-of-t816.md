---
task_uid: "01m69tfs1jd0tjm3x7h1g72z7a"
task: "T1265"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T816

Findings of the review of T816 that don't block the merge (below merge.block_severity):

- F3 medium crates/fe-gpui/src/hover.rs:663: Annotation hover cancellation remains incomplete. Hiding and re-showing the gutter before HOVER_DELAY leaves the pending timer alive, allowing the old popover to appear without another mouse move: set_gutter does not cancel it, and render_popover drops only shown popovers. Also, moving the editor away from a stationary pointer leaves a shown popover visible because annotation hover is still reconciled only on mouse movement. The new test waits for the timer while hidden and misses the hide/show case.
  fix: Cancel pending and shown annotation popovers when hiding the gutter, and reconcile existing annotation hover against the current column hitbox after layout changes. Add regressions for hide/show before HOVER_DELAY and moving the editor away from a stationary pointer.
  instances: crates/fe-gpui/src/hover.rs:663, crates/fe-gpui/src/element.rs:1321
