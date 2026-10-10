---
task_uid: "01m4kxpvhxjmyn00er27enma74"
task: "T12"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T210 (Mac T262)

Findings of the review of T210 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/lsp.rs:221: The force phase treats Stopped status as complete process cleanup. fe-lsp can emit Stopped while its thread still retains a process awaiting cleanup; its existing kill_switch_reaps_after_a_graceful_exit test demonstrates this with endless stderr and a stationary clock. Such a watch is excluded from pending, so quitting returns forced: 0, left: 0 without throwing the switch, potentially leaving process-group members alive. Previously retired watches can also lose their switch through retire_lsp's is_done-based pruning.
  fix: Distinguish terminal status from completed cleanup, retain unfinished watches and their switches across retirement, and force-stop those still unfinished at the deadline. Use that completion state consistently for waiting and Quit counts. Add workbench regression coverage equivalent to fe-lsp's graceful-exit/endless-stderr case, both with an open window and after window closure.
  instances: crates/fe-workbench/src/lsp.rs:172, crates/fe-workbench/src/lsp.rs:221, crates/fe-workbench/src/lsp.rs:222, crates/fe-workbench/src/lsp.rs:230, crates/fe-workbench/src/lsp.rs:700



Moved from the Mac board: Mac T262. Its dependencies are finalized on Mac and arrive here with the mac/main sync.
