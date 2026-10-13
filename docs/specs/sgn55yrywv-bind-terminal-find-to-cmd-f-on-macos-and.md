---
task_uid: "01m4skab5dmzn0ntsgn55yrywv"
task: "T611"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-keymap test terminal_find_bindings::resolves: on macOS cmd-f and on Linux ctrl-shift-f resolve to terminal.find in a workbench \u003e terminal context stack; on Linux ctrl-shift-f still resolves to search.show with an editor focused and ctrl-f has no binding in the terminal context; in the terminal find row's stack (find_bar, text_input) enter, shift-enter and escape resolve to find.next, find.previous and find.close on both platforms"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test terminal_find_keys_tests::keys_drive_the_row: with the default macOS keymap, cmd-f with a terminal focused opens its find row with focus in the input; typing a printed word then enter and shift-enter step through the terminal's matches while the pane's find bar stays closed; escape closes the row and focuses the terminal"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-keymap (one_keymap_resolves_to_each_platforms among them), cargo test -p fe-workbench (find_keys among them) and cargo clippy -p fe-keymap -p fe-workbench --all-targets -- -D warnings pass; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Bind terminal.find to cmd-f on macOS and ctrl-shift-f on Linux in the terminal, with enter, shift-enter and escape driving the terminal's find row

## Problem

T586 (landed on argon, arrives on the Mac's main with stack 6) adds `terminal.find`, the terminal panel's find row,
but binds no key: its spec says "binding `secondary-f` (and Linux `ctrl-shift-f`) in the `terminal` context belongs
to a later key-binding card, since the keymap files are held by T462". This is that card. Today cmd-f with focus in
a terminal does nothing (`find.show` is bound in the `editor` context only, `crates/fe-keymap/keymaps/fastedit.toml`
line ~806), and on Linux ctrl-shift-f opens project search (`search.show`, `workbench` context, line ~447).

## Change

- `crates/fe-keymap/keymaps/fastedit.toml`: one binding, `terminal.find` in context `terminal`
  (`fe_terminal_view::KEY_CONTEXT`), keys `{ macos = "cmd-f", default = "ctrl-shift-f" }`, placed next to the
  other terminal bindings, with a comment that Linux keeps ctrl-f for the shell (readline's forward-char).
- `crates/fe-keymap/keymaps/default-macos.toml` and `default-linux.toml`: the same binding, so fe-keymap's
  `one_keymap_resolves_to_each_platforms` still holds.
- Inside the find row, T586 already sets key context `find_bar` on the row, so the existing `find_bar` bindings
  apply: enter `find.next`, shift-enter `find.previous` (`find_bar && text_input`), escape `find.close`,
  alt-cmd-c / alt-cmd-r toggles, and cmd-g / cmd-shift-g (`editor || find_bar`; T462 adds f3 / shift-f3 on
  Linux). This card adds no binding for them; it proves they resolve in the row's context stack and drive the
  terminal's find, not the pane's find bar. If one doesn't (for example because the row sits inside the
  `terminal` context and a terminal binding wins), fix it in the keymap files only, by tightening that terminal
  binding's context with `!find_bar`.
- No menu entry (the Edit menu's Find stays the editor's).

## Tests

- New file `crates/fe-keymap/tests/terminal_find_bindings.rs`: parse `DEFAULT` for `Os::MacOs` and `Os::Linux` and
  resolve keys against context stacks (as `crates/fe-keymap/tests/one_keymap.rs` parses).
- New file `crates/fe-workbench/src/terminal_find_keys_tests.rs`, declared `#[cfg(test)] mod
  terminal_find_keys_tests;` from `crates/fe-workbench/src/terminal_find.rs` (T586's module), loading the macOS
  default keymap as `crates/fe-workbench/src/tests/find_keys.rs` does and driving a window with the terminal
  panel's test shell, as T586's `terminal_find_tests.rs` does.

## Decided by the lead overnight

- **Keys.** cmd-f on macOS (VS Code's and Zed's terminal find), ctrl-shift-f on Linux and Windows (Zed's Linux
  terminal find; VS Code's ctrl-f would take forward-char from the shell). With focus in a terminal on Linux,
  ctrl-shift-f therefore opens the terminal's find, not project search, as in Zed; project search from the
  terminal stays in the palette and the editor's ctrl-shift-f.
- **Escape with focus in the terminal itself** goes to the shell (Zed), even while the find row is open; escape
  in the row closes it.
- **No new find_bar bindings**: the row reuses the editor find bar's keys, so users learn one set.
- Depends on T586 (`terminal.find`, `terminal_find.rs`) and T462 (holds the three keymap files).
