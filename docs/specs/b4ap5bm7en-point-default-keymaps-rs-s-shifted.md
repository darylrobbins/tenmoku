---
task_uid: "01m54rna1yjcjd7pb4ap5bm7en"
task: "T768"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "crates/fe-workbench/src/tests/default_keymaps.rs line 83's comment names fe-keymap/keymaps/fastedit.toml, rg -n 'default-linux.toml' crates finds no match, and no other line of the file changes"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench -E 'test(default_keymaps)' passes"
    required: true
    manual: false
---

# Point default_keymaps.rs's shifted-punctuation comment at keymaps/fastedit.toml instead of the deleted default-linux.toml

## Problem

crates/fe-workbench/src/tests/default_keymaps.rs:83, in `linux_keys_use_ctrl`, reads
`// Shifted punctuation, by what it types (see default-linux.toml).` T120 (6e0633e4) deleted
crates/fe-keymap/keymaps/default-linux.toml when it collapsed the per-platform keymaps into one table; the
note it points at now lives at the top of crates/fe-keymap/keymaps/fastedit.toml (lines 10-11: a shifted
punctuation key is named by what it types, `ctrl-shift-[` is `ctrl-{`). `rg default-linux.toml crates`
finds only this comment.

## Change

Change the comment to `// Shifted punctuation, by what it types (see fe-keymap/keymaps/fastedit.toml).`
Nothing else in the file changes. docs/decisions/ and docs/specs/ keep their old file names: they record
what was true when they were written.
