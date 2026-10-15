---
task_uid: "01m50ew2n2nd54nhth771reea2"
task: "T116"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-keymap test lsp_bindings::both_platforms: each key in the card's table resolves to its command in the editor context with vim off, on macOS and on Linux, and cmd-k cmd-i still resolves to the hover command"
    required: true
    manual: false
  - id: "C2"
    text: "fe-keymap test lsp_bindings::vim_keeps_its_keys: in every vim mode context, each key vim already binds still resolves to its vim binding"
    required: true
    manual: false
  - id: "C3"
    text: "the Edit menu lists Rename Symbol…, Code Actions…, Format Document and Format Selection, and cargo test -p fe-keymap, cargo test -p fastedit --test menus and cargo clippy -p fe-keymap --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Bind rename, code actions, formatting and signature help to keys, and add them to the Edit menu (Mac T480)

## Problem

`lsp.rename`, `lsp.code_actions`, `lsp.format_document`, `lsp.format_selection` and `editor.show_signature_help`
land without keys or menu items (their cards leave keys to this one so they don't collide on the keymap files).

## Change

Keys in `crates/fe-keymap/keymaps/fastedit.toml`, mirrored in `default-macos.toml` and `default-linux.toml` (VS
Code's defaults):

| Command | macOS | Linux | Context |
|---|---|---|---|
| `lsp.rename` | f2 | f2 | editor |
| `lsp.code_actions` | cmd-. | ctrl-. | editor |
| `lsp.format_document` | shift-alt-f | ctrl-shift-i | editor |
| `lsp.format_selection` | cmd-k cmd-f | ctrl-k ctrl-f | editor |
| `editor.show_signature_help` | cmd-shift-space | ctrl-shift-space | editor |

As in T462, no binding takes a key from vim in a vim context: where vim binds the same keys, the new binding's
context excludes that mode. The two-key chords must not break the existing cmd-k chords (cmd-k cmd-i is hover).

Menus (`crates/fe-keymap/menus/fastedit.toml`, `default-macos.toml`, `default-linux.toml`): an Edit menu group with
Rename Symbol…, Code Actions…, Format Document and Format Selection.

## Tests

New file `crates/fe-keymap/tests/lsp_bindings.rs`. fe-keymap's `tests/one_keymap.rs` and
`crates/fastedit/tests/menus.rs` must still pass.
