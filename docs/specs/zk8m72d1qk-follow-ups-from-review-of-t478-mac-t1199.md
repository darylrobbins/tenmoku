---
task_uid: "01m5yapy7kcfyr4rzk8m72d1qk"
task: "T373"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T478 (Mac T1199)

Findings of the review of T478 that don't block the merge (below merge.block_severity):

- F4 medium crates/fe-workbench/src/editorconfig_tests.rs:144: The regression test closes the split instead of transferring its editor: the destination already shows the same buffer, so kiln-shell keeps the destination's original editor. Settings reload then checks only that original editor. The test therefore passes with the previous implementation and guards neither split configuration retention nor configuration transfer.
  fix: Reload settings while both split editors remain open and assert both retain width 8. Separately move an editor into a pane without a copy of its buffer, assert its identity survives the move, and verify its width after reload. Add the previously requested restoration and pending-read regression cases.
  instances: crates/fe-workbench/src/editorconfig_tests.rs:144, crates/fe-workbench/src/editorconfig_tests.rs:155
