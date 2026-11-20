---
task_uid: "01m7xcm88tdyx3kxmmda1ef1yc"
task: "T399"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T1395 (Mac T2074)

Findings of the review of T1395 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-gpui/src/vim.rs:710: Reloading a group name while its popup is visible hides the popup and leaves its prefix held indefinitely. This setter changes VimState but leaves the cached popup unchanged; the notified render calls settle_which_key(), detects different labels, and closes it. The remap timeout was already cancelled when the popup appeared, and the retained `seen` prefix prevents automatic reopening. Adding, renaming, or removing a visible group's name through either configuration source triggers this.
  fix: Refresh the visible popup when only group names change, preserving its held prefix and timing state. Add a regression test that changes and removes names after the popup appears, draws another frame, and verifies the updated popup remains visible and the mapping still completes.
  instances: crates/fe-gpui/src/vim.rs:710
