---
task_uid: "01m57ya98j0scbw5vggtyg7s2z"
task: "T201"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "day_one_bindings: every binding in the card resolves to its command in its context on macOS and Linux, and fastedit.toml still resolves to exactly default-macos.toml and default-linux.toml (one_keymap passes)"
    required: true
    manual: false
  - id: "C2"
    text: "day_one_bindings::vim_safe: no binding this card adds matches an editor frame in vim normal, visual, operator-pending (vim.pending) or insert mode, except ] d and [ d in normal mode with nothing pending; ctrl-r runs workbench.open_recent with vim off or outside the editor, and stays vim's with vim on in the editor"
    required: true
    manual: false
  - id: "C3"
    text: "day_one_bindings::tree_vim: in project_panel with the vim flag, j k h l, g g, shift-g, a, %, r, shift-r, d, shift-d, shift-y and g y resolve to the tree commands; without the flag none of the letter keys do"
    required: true
    manual: false
  - id: "C4"
    text: "the menu tests pass with File \u003e Open Recent…, Edit \u003e Replace and Replace in Files, View \u003e Problems and Go \u003e Next Problem and Previous Problem, and fe-gpui command_tests::table_is_the_only_binding_source passes with the new count"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-keymap -p fe-gpui and cargo clippy -p fe-keymap -p fe-gpui --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Bind the day-one commands to keys and menus, with vim-mode keys in the tree and lists and none taken from vim's normal, visual or operator-pending modes (Mac T523)

## Problem

The day-one feature cards (find-and-replace, project replace, tree file operations, Open Recent, Problems,
notifications) add commands without keys or menu items, so that each can be built without colliding on the
shared keymap and menu tables. User 2026-10-12: every day-one feature must be vi friendly, and new keys must never
take a key from vim's normal, visual or operator-pending modes.

## Change

In `crates/fe-keymap/keymaps/fastedit.toml`, `default-macos.toml` and `default-linux.toml` (`secondary` is cmd on
macOS, ctrl elsewhere), and the matching menus in `crates/fe-keymap/menus/*.toml`:

- Find bar: `secondary-alt-f` → `find.show_replace` (context `workbench`); in `find_bar`: `secondary-shift-1` →
  `find.replace_next`, `secondary-alt-enter` → `find.replace_all`; `enter` in `find_bar && replace_input` →
  `find.replace_next`.
- Project replace: `secondary-shift-h` → `search.show_replace`; in `search_panel`, `secondary-alt-enter` →
  `search.replace_all`; in `search_results`: `up`/`down`/`enter` → `search.select_previous`/`select_next`/
  `open_selected`, and with `vim` also `k`/`j`.
- Tree (`project_panel`): `f2` → rename, `secondary-backspace` → delete; with `vim` (nvim-tree, with netrw's
  aliases where they don't clash): `j`/`k` select next/previous, `h` collapse or parent, `l` open, `g g` first,
  `shift-g` last, `a` and `%` new file (a trailing `/` makes a folder, as in nvim-tree), `r` and `shift-r` rename,
  `d` and `shift-d` delete (to the Trash, after the prompt), `shift-y` copy relative path and `g y` copy absolute path (nvim-tree's `Y` and `gy`). netrw's `d` (make a folder) loses to nvim-tree's `d`.
- Problems: `secondary-shift-m` → `problems.show`; `f8`/`shift-f8` → `diagnostics.next`/`previous` (context
  `workbench`); `] d`/`[ d` → the same, in an editor in vim normal mode with nothing pending (nvim's defaults); in
  `problems_panel`: `up`/`down`/`enter`, and with `vim` `j`/`k`.
- Open Recent: `ctrl-r` → `workbench.open_recent` (VS Code's macOS key), except in an editor with vim on, where
  ctrl-r stays vim's redo and insert-mode register key (the predicate is the implementer's; the test proves both).
- Notifications: `secondary-k escape` → `toast.dismiss_all`.
- Menus: File gets "Open Recent…" after Open File; Edit gets "Replace" after Find and "Replace in Files" after
  Find in Files; View gets "Problems"; Go gets "Next Problem" and "Previous Problem".
- Update `crates/fe-gpui/src/command_tests.rs`'s binding count.

## Tests

New file `crates/fe-keymap/tests/day_one_bindings.rs`: each binding above resolves in its context on both
platforms, and a collision check over every binding this card adds.

## Implementation notes

- **Linux ctrl chords.** fe-gpui hands vim `ctrl-<letter>` as `<C-letter>` whatever else is held, so off macOS
  `ctrl-alt-f`, `ctrl-shift-h`, `ctrl-shift-m` and `ctrl-k escape` would be vim's `<C-f>`, `<C-h>`, `<C-m>` and
  `<C-k>`. There they're bound in `workbench && !(editor && vim.enabled)`; the macOS cmd keys stay in `workbench`.
  With vim on, an editor has the ex commands for these (`:s`, `:Replace`, `:copen`, `:oldfiles`) and the menus.
- **ctrl-r** is `workbench && !terminal && !vim_line && !(editor && vim.enabled)` on both platforms: vim's in a
  vim editor in every mode, and also the shell's in the terminal (reverse search) and the vim command line's.
- **Replace input's `enter`** is `find_bar && replace_input && text_input`: `find.next`'s
  `find_bar && text_input` holds at the input's depth, deeper than `replace_input`, so naming `text_input` too
  puts both at one depth, where the later binding wins.
- **`secondary-shift-1`** is `secondary-!`: GPUI reports cmd-shift-1 as `cmd-!` on macOS (shift cleared, the key
  what it types: `gpui-pre-macos` `parse_keystroke`) and ctrl-shift-1 as `ctrl-!` on Linux (review F2).
- **Replace** (`find.show_replace`) is bound in `workbench` (Linux: without vim editors). The pane handles it, and
  from a panel (the tree, Problems, a list) the pane isn't on focus's path, so `find_commands::init` adds an app
  listener that runs it in the workbench's active pane when nothing on the path did, after the dispatch
  (review F3; `find_keys::replace_opens_from_a_panel`). The listener lives there, not on the workbench's root,
  because T135 holds `workbench.rs`. The command's declared context is `workbench` too, so the palette lists it
  from a panel and the pane's `applies` check runs it with only the pane focused (the same test covers both).
- **Open Recent on macOS** shows no shortcut (`shortcut_context = []` on a macOS-only item): a macOS menu
  shortcut is an AppKit key equivalent, which would run the item when a vim editor in insert or replace mode
  leaves ctrl-r unhandled (review F1). `day_one_bindings::native_safe` checks every other shortcut the card's
  items show on macOS runs through the keymap first in an editor in every vim mode. Linux's in-window menu
  bar has no key equivalents and still shows ctrl-r.
- **`] d` / `[ d`** are `editor && vim.mode == normal && !vim.pending`; any other `]`/`[` key is handed to vim
  when the chord fails, as `ctrl-w`'s are.
- **Edit menu**: there was no Find item, so Edit gets "Find" and "Replace" in a group of their own before
  "Find in Files" and "Replace in Files", as in VS Code. View's "Problems" is after "Outline"; Go's
  "Next Problem" and "Previous Problem" lead the Next Change group.
- **vim_safe** reads "matches" as taking a key vim reads: a binding that applies in a vim editor frame must not
  start with a key fe-vim's reducer waits on or commits in normal, operator-pending or a visual mode, nor any
  key vim reads in insert mode (only cmd chords, function keys and alt chords are free there).
- **Scope** grew (respec) for tests that pinned the old tables: fe-keymap `menu.rs` and `menu_go_nav.rs`;
  fe-gpui `keymap_tests.rs` and `vim/tests.rs`; fe-workbench `tests/default_keymaps.rs`, `find_keys.rs`,
  `menu_bar.rs`, `tree_keys.rs` and `activity_rail.rs` (its user rebinding moved off Replace's `alt-cmd-f` to
  `alt-cmd-j`); and for F3 fe-workbench's `find_commands.rs`, `lib.rs` and `typed_command_ids_tests.rs`. Two
  editor-frame helpers in those tests now set `vim.enabled`, as fe-gpui stamps it on every editor with vim on.
