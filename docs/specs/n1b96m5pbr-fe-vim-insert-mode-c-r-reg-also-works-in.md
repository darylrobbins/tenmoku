---
task_uid: "01m5ec49shcgs16gn1b96m5pbr"
task: "T246"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "the integration test insert_registers_match_neovim passes on crates/fe-vim/tests/corpus/insert_registers.toml extended with at least 6 nvim 0.12.x-recorded replace-mode cases (asserted by name), among them replace_ctrl_r_overwrites, replace_ctrl_r_past_end_of_line, replace_ctrl_r_backspace_restores, replace_ctrl_r_linewise"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui headless-window tests in crates/fe-gpui/src/vim/insert_ctrl_keys_tests.rs pass on both keymaps: \u003cC-r\u003e{reg} in replace mode inserts the register; cargo nextest run -p fe-gpui -p fe-vim and clippy pass"
    required: true
    manual: false
---

# fe-vim: insert-mode <C-r>{reg} also works in replace mode, overwriting and restoring on <BS> as nvim's (T244 known limit)

## Problem

T241 (= Mac T880) and T244 (its fe-gpui wiring) give insert mode `<C-r>{reg}` and `<C-a>`. In replace mode, `<C-r>{reg}` is a no-op: fe-vim's `insert_register_start` and its session id act only in `Mode::Insert`, and `insert_value` has no overwrite/restore semantics for Replace (the replace ledger). nvim's `<C-r>{reg}` in Replace mode overwrites the characters under the inserted text, and `<BS>` restores them.

## Change

In fe-vim (`crates/fe-vim/src/insert/keys.rs`, the replace ledger), make `<C-r>{reg}` (and the `<C-r><C-r>/<C-o>/<C-p>` forms) work in replace mode as nvim's do, probing each rule with `nvim --clean` (overwrite over text and at end of line, `<BS>` restoring, linewise registers, counted and dot repeat), with cases added to `crates/fe-vim/tests/corpus/insert_registers.toml` recorded into `.expected.json`, asserted by `insert_registers_oracle.rs`. In `crates/fe-gpui/src/vim.rs`, lift T244's replace-mode exception so `<C-r>` is routed there too, and flip T244's replace-mode test (it pinned the no-op) in `crates/fe-gpui/src/vim/insert_ctrl_keys_tests.rs`.
