---
task_uid: "01m4phfww2hwn86z6bzv88e4gc"
task: "T60"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T58

Findings of the review of T58 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/pane.rs:726: Closing is counted only after a tab disappears. If a diagnostic pick is pending and the user invokes workbench.close_tab on a dirty editor, close_active_item waits for the discard prompt without activating or removing anything. Both serials remain unchanged, so the diagnostic read can still activate its file and steal focus during the newer close intent. The new regression covers only a clean tab that closes immediately.
  fix: Record the close intent when the close action starts, before awaiting confirmation, rather than relying only on items_changed. Add a deterministic regression with a dirty active tab: confirm a diagnostic pick, invoke Close Tab, let the diagnostic read complete while the discard prompt remains pending, and verify the diagnostic does not open or take focus.
  instances: crates/fe-workbench/src/pane.rs:726
