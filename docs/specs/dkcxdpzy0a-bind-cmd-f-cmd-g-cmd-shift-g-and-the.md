---
task_uid: "01m4fhv8r48w8sb5dkcxdpzy0a"
task: "T266"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::find_keys::default_bindings: the default keymap binds exactly the nine keys in the spec's table to their commands and contexts, and keymap_report against fe_workbench::command_catalog() finds no issue"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::find_keys::keys_drive_the_bar: in a test window, cmd-f in a focused editor (vim off, and vim on in normal mode) opens the bar; typing foo then enter, shift-enter, cmd-g and cmd-shift-g step through the matches; alt-cmd-c toggles Match Case; escape closes the bar and returns focus to the editor"
    required: true
    manual: false
  - id: "C3"
    text: "The counts and context lists pinned in fe-gpui's command_tests.rs and keymap_tests.rs change only by these nine bindings and the find_bar context; tests::outline_panel::the_default_keymap_binds_outline_show passes unchanged; the screenshot goldens are unchanged"
    required: true
    manual: false
---

# Bind cmd-F, cmd-G, cmd-shift-G and the find bar's enter, shift-enter, escape and toggle keys in the default keymap

## Problem

Card 4 of 4 for in-file find, asked by the user on 2026-10-09: "In-file find (cmd-F) reusing T213's row
matcher". Card 3 registers the `find.*` commands; nothing binds them. `crates/fe-keymap/keymaps/default-macos.toml`
has no `cmd-f`, `cmd-g` or `cmd-shift-g` binding today (`ctrl-cmd-f` is full screen, `cmd-shift-f` Find in
Files). The keymap and fe-gpui's pinned binding counts are shared with T146, T147, T219, T231 and T259, so the
bindings are a card of their own.

## Change

Append to `crates/fe-keymap/keymaps/default-macos.toml`:

| Keys | Command | Context |
|---|---|---|
| `cmd-f` | `find.show` | `editor` |
| `cmd-g` | `find.next` | `editor \|\| find_bar` |
| `cmd-shift-g` | `find.previous` | `editor \|\| find_bar` |
| `enter` | `find.next` | `find_bar && text_input` |
| `shift-enter` | `find.previous` | `find_bar && text_input` |
| `escape` | `find.close` | `find_bar` |
| `alt-cmd-c` | `find.toggle_case` | `find_bar` |
| `alt-cmd-w` | `find.toggle_whole_word` | `find_bar` |
| `alt-cmd-r` | `find.toggle_regex` | `find_bar` |

The `alt-cmd-*` toggles are the search panel's keys in the `find_bar` context. None of these clash with T259's
`editor && proposal` keys (`alt-f5`, `alt-shift-f5`, `cmd-y`, `cmd-n`, `cmd-enter`, `cmd-backspace`) or T219's
`vim_line` keys. `cmd-f` reaches the editor with vim on: fe-vim's reducer reads no `cmd` key
(`crates/fe-gpui/src/vim.rs`).

Update the binding counts and context lists pinned in `crates/fe-gpui/src/command_tests.rs` and
`crates/fe-gpui/src/keymap_tests.rs` by exactly these nine bindings and the `find_bar` context.

## Tests (new file)

`crates/fe-workbench/src/tests/find_keys.rs`, declared in `crates/fe-workbench/src/tests/mod.rs`.

## Size

About 50 lines of TOML and pinned numbers, plus the test.
