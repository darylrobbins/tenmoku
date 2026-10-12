---
task_uid: "01m4rbj4t9qdnpagfctrdne6bd"
task: "T45"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test view::multi_cursor_tests::keys_add_carets: with vim off and the macOS default keymap, cmd-alt-down twice, cmd-d twice, cmd-shift-l and alt-shift-i each change the selections as the fe-editor commands do, and escape then leaves one caret"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::multi_cursor_tests::vim_keeps_its_keys: with vim on, the editor frame has the vim.enabled flag; escape after d in normal mode cancels the operator, and cmd-d, alt-shift-down and escape in normal and visual mode leave the selection count at one; vim::* tests pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test view::multi_cursor_tests::alt_click_and_box: alt-click adds a caret and alt-click on it removes it; alt-shift-drag from row 0 column 2 to row 3 column 5 gives one selection per row at those display columns; typing x then edits every row as one undo step; a bound click still runs its command"
    required: true
    manual: false
  - id: "C4"
    text: "fe-keymap one_keymap test passes, and command_tests::table_is_the_only_binding_source passes with its count raised by the new bindings"
    required: true
    manual: false
---

# Bind the multi-cursor commands to keys outside vim, and add carets with alt-click and a column with alt-shift-drag (Mac T427)

Filed on argon from Mac T427 (card ~/coordination/argon-cards/T427.json rev 1, Mac lead message ~/coordination/from-mac-lead-2026-10-12 note j (relayed by the user).md). Mac task uid 01m4p30h0ghd137m3t9rmn1h2g. Its dependencies (Mac T426) are already on argon main (Mac T426 as 7ec8b5f, from a trunk sync), so none are recorded here. Title the host commit "... (Mac T427)".

## Problem

After the previous card (fe-editor's multi-cursor commands, `crates/fe-editor/src/multi_cursor.rs`) nothing in the
app reaches them: no key is bound, and a click always replaces every caret (`EditorView::on_mouse_down`,
`crates/fe-gpui/src/view.rs:347`, `model.set_cursor(offset)` in its click closure). M6 of the plan
(`~/.claude/plans/glittery-foraging-graham.md` line 110) is "Editing polish: multi-cursor, ..."; user 2026-10-11:
"Draft any work in earlier phases we haven't drafted yet."

Vim must keep its keys. Binding `escape` or (off macOS) `ctrl-d` in the plain `editor` context would take them from
vim's normal, operator-pending, replace and visual modes, where today they reach vim unmatched, and no context
predicate can say "vim is off": `vim.mode` is a value, and `!=` is true when it's unset
(`kiln/crates/kiln-command/src/predicate.rs:26-28`).

## Change

- Flag: with vim on, the editor frame carries the flag `vim.enabled`, in both stamps in
  `crates/fe-gpui/src/vim.rs`: `stamp` (:262, which strips `vim.*` and restamps) and `add_to_key_context` (:288).
- Keys, in `crates/fe-keymap/keymaps/fastedit.toml` (and `default-macos.toml` / `default-linux.toml` to match, as
  fe-keymap's `one_keymap` test requires), all with `context = "editor && !vim.enabled"`, placed in the editor
  section so that completion's escape and any later escape binding (ghost text) keep winning while shown:
  - `editor.add_cursor_above`: `{ macos = "cmd-alt-up", default = "alt-shift-up" }`
  - `editor.add_cursor_below`: `{ macos = "cmd-alt-down", default = "alt-shift-down" }`
  - `editor.select_next_occurrence`: `secondary-d`
  - `editor.select_all_occurrences`: `secondary-shift-l`
  - `editor.split_selection_into_lines`: `alt-shift-i`
  - `editor.single_selection`: `escape`
  With vim on none of them apply in any mode, so vim keeps escape, ctrl-d and the rest exactly as today.
- Mouse, in `on_mouse_down` / `on_mouse_move`, only when vim doesn't own the mouse (`vim_owns_mouse`) and the click
  isn't a bound click (`click_command` keeps priority, as today):
  - alt-click (click_count 1): `model.toggle_caret(offset)`; no drag.
  - alt-shift-drag: `model.select_box` from the mouse-down point to the pointer, as (buffer row, display column)
    from the layout, updated on every move; a pointer past a row's end keeps its column (the column may exceed
    the row), so dragging right over short rows still widens the box.
- Update the binding count in `crates/fe-gpui/src/command_tests.rs` (`table_is_the_only_binding_source`, 143).

## Tests

New file `crates/fe-gpui/src/view/multi_cursor_tests.rs`, declared `#[cfg(test)]` from view.rs like the other
`view/*_tests.rs`.

## Implementation notes

- Two files outside the listed scope: `crates/fe-gpui/src/element.rs` gains
  `FrameLayout::columns_past_row_end` (the frame's char widths and long-row index are private to that module), which
  the box drag uses to carry its column past a row's end; and `crates/fe-gpui/src/keymap_tests.rs`'s
  `default_keymap_matches_the_old_table` lists the six new bindings and counts 150, as `command_tests` does.
- An alt-click with ctrl, cmd or fn also held isn't a caret toggle (`alt_only` in view.rs); with shift and
  click_count 1 it starts a box. A box drag begun in vim's insert mode does nothing once a key leaves insert mode.
- `alt_click_and_box` checks the bound click by swapping in a layer that binds `alt-click` (`set_user_keymap`).
