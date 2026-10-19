---
task_uid: "01m57w9mssy6yyyzcrp3vjyn4a"
task: "T982"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Dragging a divider moves weight between its two neighbours down to a minimum body height, for weights of ordinary size (huge restored weights, up to f32::MAX, are T1001's)"
    required: true
    manual: false
  - id: "C2"
    text: "Double-click resets the pair to equal weights; collapsed views have no divider"
    required: true
    manual: false
  - id: "C3"
    text: "Resized weights survive save/restore"
    required: true
    manual: false
  - id: "C4"
    text: "tests/view_stack_resize.rs proves C1-C3 for ordinary weights"
    required: true
    manual: false
---

# Drag the dividers between a ViewStack's expanded views to resize them, double-click to reset

## Problem
`ViewStack` (previous card) has fixed proportions; VS Code lets the user drag the border between two views. 
## Change
Add draggable dividers between adjacent expanded views in view_stack.rs: drag moves weight between the two neighbours down to a minimum body height, double-click resets the pair to equal; collapsed views have no divider. Weights persist through the existing save/restore. Follow the pointer handling of `pane_group.rs`'s dividers and `dock.rs`'s EdgeDrag. Tests in a new kiln/crates/kiln-shell/tests/view_stack_resize.rs.
