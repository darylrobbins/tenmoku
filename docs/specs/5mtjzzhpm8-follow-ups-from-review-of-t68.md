---
task_uid: "01m5xfydp2hnphr95mtjzzhpm8"
task: "T369"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T68

Findings of the review of T68 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-keymap/tests/editing_bindings.rs:199: The binding contexts are corrected, but F1's requested regression coverage through actual Vim key input is still missing. The new test accepts an absent lookup result without verifying that the fallback listener receives the key and performs the expected Vim action; the existing test only enumerates explicit bindings.
  fix: Add tests through the GPUI key-input path proving Shift-Tab invokes vim.jump_newer in normal mode and modified arrows move the selection in every visual mode, using both platform keymaps and their duplicate-row shortcuts.
  instances: crates/fe-keymap/tests/editing_bindings.rs:156, crates/fe-keymap/tests/editing_bindings.rs:199
