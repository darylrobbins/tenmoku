---
task_uid: "01m5pzc5aqbw540hsrtaqbx6gk"
task: "T313"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Keymap edits from the Keyboard Shortcuts tab are written in submission order per destination file by one detached worker that outlives the tab. Tests delayed_remove_then_reset_lands_in_order, write_survives_tab_close_and_orders_with_next_tab"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Write Keyboard Shortcuts edits through one ordered queue per keymap file (T299 F7)

## Problem
T299 (Mac T1100) review round 6 finding F7: each keymap edit in the Keyboard Shortcuts tab starts its own background write, so two quick edits (for example Remove then Reset) can land out of order, and a write started by a tab that is then closed is not ordered with a later tab's.

## Change
- `crates/fe-workbench/src/shortcuts_editor.rs`: one keymap-write queue per destination file, a Global holding an async_channel sender and a detached background worker that writes in submission order and outlives the tab/window.

## Tests
`crates/fe-workbench/src/shortcuts_editor_queue_tests.rs` (declared with `#[path]`): a delayed Remove followed by a Reset lands in submission order; a write submitted before the tab closes still lands before a later tab's.
