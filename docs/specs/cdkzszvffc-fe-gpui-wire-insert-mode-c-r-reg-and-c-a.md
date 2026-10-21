---
task_uid: "01m5e9bcj08r9zckcdkzszvffc"
task: "T244"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui headless-window tests in crates/fe-gpui/src/vim/insert_ctrl_keys_tests.rs pass on the macOS and Linux keymaps: in INSERT mode \u003cC-r\u003e{reg} (\", a, and the \u003cC-r\u003e\u003cC-r\u003e, \u003cC-r\u003e\u003cC-o\u003e, \u003cC-r\u003e\u003cC-p\u003e forms) inserts the register and \u003cC-a\u003e inserts the last inserted text; \u003cC-r\u003e then \u003cEsc\u003e or a non-register key inserts nothing and a later \u003cC-w\u003e still works; a macro replaying \u003cC-r\u003ea inserts the register; in REPLACE mode (a documented limit: fe-vim has no replace-mode \u003cC-r\u003e) \u003cC-r\u003e leaves nothing pending and the next key types as it did before this card"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-gpui -p fe-vim and clippy pass; the fe-gpui binding and command count tests are updated if the change moves them"
    required: true
    manual: false
---

# fe-gpui: wire insert-mode <C-r>{reg} and <C-a> into the app's input paths, with a pending prefix that can never swallow a later key (T241 review F1 follow-up)

## Problem

Helium T241 (= Mac T880) adds insert-mode `<C-r>{reg}` (and `<C-r><C-r>`, `<C-r><C-o>`, `<C-r><C-p>`) and `<C-a>` to fe-vim: `insert_register_start`, `insert_register_key` and `insert_last_inserted` in `crates/fe-vim/src/insert/keys.rs`, with the pending prefix held in `VimState::register_key`. They are wired only into `HeadlessVim::type_key`. fe-gpui does not call them, so in the app `<C-r>` and `<C-a>` are still dropped. Worse, a pending `<C-r>` that never sees its register name could swallow a later ctrl key. T241's review finding F1 (high) said so; it was disputed as outside T241's fe-vim-only scope and is this card.

## Change

In `crates/fe-gpui/src/vim.rs` (read T241's `insert/keys.rs` first for the exact API and how `HeadlessVim::type_key` drives it):

- After `ctrl-r` in insert or replace mode, route the NEXT key (the register name, including typed text such as `"` or `a`, and the `<C-r>`, `<C-o>` and `<C-p>` second keys) to fe-vim's register-insert path instead of the normal input handler, in the typed path and the replayed path (macros, `.`, maps), exactly as `insert_key_typing` does for `<C-w>` and `<C-u>`. `<Esc>` or a key that names no register cancels the prefix and leaves insert mode as it was; mode changes, focus loss and a session ending clear a pending prefix so it can never swallow a later key.
- `ctrl-a` reaches fe-vim's `<C-a>` (insert the last inserted text) in insert mode on Linux too: `crates/fe-keymap/keymaps/fastedit.toml` binds `ctrl-a` elsewhere on Linux (select all), so leave the binding as the app's with the vim insert context taking it first, as `ctrl-w` and `ctrl-u` do; check macOS and Linux keymaps both.
- Tests in `crates/fe-gpui/src/vim/insert_ctrl_keys_tests.rs` (their file's pattern: a headless window on both platform keymaps): `<C-r>"` inserts the register, `<C-r>a`, `<C-r><C-r>`, `<C-o>` and `<C-p>` variants, `<C-r><Esc>` and `<C-r>` followed by a non-register key insert nothing and a later ctrl key (`<C-w>`) still works, replace mode, and `<C-a>`; plus a replay case (a macro with `<C-r>a`).
- If a binding or command count asserted in `crates/fe-gpui/src/keymap_tests.rs` or `command_tests.rs` changes, update it in this card.

## Known limit (lead decision, 2026-10-21)

Replace mode is out of this card. fe-vim's `insert_register_start` and its session id act only in `Mode::Insert`, and `insert_value` has no overwrite/restore semantics for Replace, so `<C-r>` in replace mode is a no-op in fe-vim and fe-gpui must not intercept it (the next key then types as before this card, the same as at base). Making `<C-r>{reg}` work in replace mode is a fe-vim change in `crates/fe-vim/src/insert/keys.rs` (the replace ledger) that waits for T245's edits to that file; it is filed as its own card. The tests pin that a `<C-r>` in replace mode leaves the next key typing as it did, and that nothing is left pending.
