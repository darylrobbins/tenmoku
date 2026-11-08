---
task_uid: "01m6xfekd4xacpq8bjhf46nqzb"
task: "T528"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T335

Findings of the review of T335 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/project_panel_clipboard.rs:398: Reopening invalidates the pending check without scheduling reconciliation against the new tree. If the copied path remains deleted, its old missing result is discarded, but opened() never calls drop_gone_clip(). FileTree::open() already fills the root listing, so no subsequent listed() callback is guaranteed; the nonexistent clipboard entry remains until another filesystem event or listing occurs.
  fix: Call drop_gone_clip() after installing the newly opened tree, retaining the invalidation at reopen start. Add a regression case where the path remains missing, reopen completes without additional filesystem events, and the clipboard is cleared.
  instances: crates/fe-workbench/src/project_panel_clipboard.rs:398

## Implementation notes

F1 was already addressed at base: `ProjectPanel::opened` calls `drop_gone_clip` after installing the new tree (project_panel.rs), and `reopen_drops_clipboard_that_is_still_gone` covers the still-missing reopen case. No code change needed; verified both reopen tests pass.
