---
task_uid: "01m58sa7021n85a6fdkkmqrbhw"
task: "T1001"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "With huge neighbour weights (up to f32::MAX), a divider drag reaches the 40px minimum body height and each neighbour gets its requested height"
    required: true
    manual: false
  - id: "C2"
    text: "A third view's share is unchanged by the drag, and dragging back moves the divider across rendered frames"
    required: true
    manual: false
  - id: "C3"
    text: "Ordinary-weight drags, double-click reset and saved weights behave as T982 left them"
    required: true
    manual: false
---

# Normalise a ViewStack's huge weights when a divider drag can't fit, so the drag still reaches the minimum height (T982 F4)

## Problem
Follow-up to T982 (approved over its open review finding F4). When a ViewStack's restored weights are huge (up to f32::MAX), the divider drag's overflow handling at kiln/crates/kiln-shell/src/view_stack.rs:376 restricts the requested split to a range instead of normalising, so a drag stops short of the 40px minimum body height or does not move at all.

## Change
When the requested split cannot fit, normalise all of the stack's weights consistently, keeping their relative shares, and update the drag's baseline so the rest of the drag follows the pointer. Ordinary-size weights behave as today.

Tests in a new kiln/crates/kiln-shell/tests/view_stack_resize_overflow.rs (don't edit view_stack_resize.rs): with huge neighbour weights (including f32::MAX), assert both neighbours' requested heights, that the drag reaches the 40px endpoint, that a third view's share is unchanged, and that dragging back moves the divider across rendered frames.
