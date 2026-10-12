---
task_uid: "01m4q691jqrbxqvp4k53700s2b"
task: "T68"
rev: 3
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T66

Findings of the review of T66 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-gpui/src/view.rs:277: Middle-click paste now calls cancel_vim, which exits characterwise, linewise, and blockwise visual modes to normal mode. This avoids restoring the stale selection by discarding the mode, contrary to T66's requirement to reconcile visual state while preserving the required mode and T64's explicit requirement that Vim stays in its mode. The new regression test asserts this incorrect transition.
  fix: Reconcile Vim's visual anchor and cursor with the paste destination and resulting caret without exiting the current visual mode, ensuring subsequent frame synchronization and Vim commands use the updated extent. Update the visual-mode regression tests to assert mode preservation, the resulting caret after rendering, and the next command's behavior.
  instances: crates/fe-gpui/src/view.rs:277, crates/fe-gpui/src/view/primary_selection_tests.rs:137

Lead note: first check what nvim 0.12.4 (~/.local/bin/nvim, with mouse=a) does on a middle-click paste in characterwise, linewise and blockwise Visual mode: does it keep or leave Visual, and where do the anchor and cursor end up? Match nvim. If nvim leaves Visual mode, keep cancel_vim and record the check in the commit message.
