---
task_uid: "01m4q2mhnmg0ndh52dec60c1rz"
task: "T66"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T64

Findings of the review of T64 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-gpui/src/view.rs:277: Middle-click paste updates only the model's caret. In vim visual mode, VimState retains the previous visual anchor and cursor, so the next frame's sync_visual restores the old selection and cursor instead of leaving a caret after the pasted text. Subsequent vim operations also use that stale visual extent.
  fix: Reconcile vim's visual state with the paste destination and resulting caret while preserving the required mode, and ensure frame synchronization does not restore the previous selection. Add middle-click tests in visual modes that check behavior after rendering and on the next vim command.
  instances: crates/fe-gpui/src/view.rs:277

- F2 medium crates/fe-gpui/src/view.rs:274: The clicked offset is calculated before split_session(Edits) ends a vim insert or replace session. Ending that session can delete unused autoindent before the clicked position, but the closure then uses the original numeric offset. For example, middle-clicking a later row after opening an unused indented row pastes too far to the right, or into a later row.
  fix: Anchor the clicked position before ending the session and resolve that anchor against the updated model inside the edit closure. Add a regression test with unused autoindent before the paste destination, checking the destination, caret, preserved mode, and undo isolation.
  instances: crates/fe-gpui/src/view.rs:274
