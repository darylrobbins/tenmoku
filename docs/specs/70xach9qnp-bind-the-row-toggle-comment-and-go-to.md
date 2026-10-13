---
task_uid: "01m4saf405xrw48e70xach9qnp"
task: "T68"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-keymap test editing_bindings::macos and editing_bindings::linux: each key in the card's table resolves to its command in its context with vim off, on that platform; ctrl-g resolves to go_to_line.toggle on both; on Linux f3 and shift-f3 resolve to find.next and find.previous in editor and find_bar contexts and enter still steps in the find bar, while macOS keeps cmd-g and cmd-shift-g"
    required: true
    manual: false
  - id: "C2"
    text: "fe-keymap test editing_bindings::vim_keeps_its_keys: in every vim mode context, each key vim already binds (ctrl-[ in insert mode on Linux among them) still resolves to its vim binding"
    required: true
    manual: false
  - id: "C3"
    text: "the Edit menu lists the seven editing commands and the Go menu Go to Line/Column…, and cargo test -p fe-keymap, cargo test -p fe-workbench (find_keys among them), cargo test -p fastedit --test menus and cargo clippy -p fe-keymap --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Bind the row, toggle-comment and go-to-line commands to keys and add them to the Edit and Go menus (Mac T462)

Filed on helium from Mac T462 (~/coordination/helium-cards/T462.json; Mac lead message ~/coordination/from-mac-lead-2026-10-13-helium-zf.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The row commands (`editor.indent`, `editor.outdent`, `editor.move_rows_up`, `editor.move_rows_down`,
`editor.duplicate_rows`, `editor.delete_rows`), `editor.toggle_comment` and `go_to_line.toggle` land without keys
or menu items (their cards leave binding to this one, so they don't all collide on the keymap files). They are
reachable only from the command palette.

## Change

Keys, in `crates/fe-keymap/keymaps/fastedit.toml`, mirrored in `default-macos.toml` and `default-linux.toml` as that
file's header requires. VS Code's defaults, written with `secondary` where they differ only by cmd/ctrl:

| Command | macOS | Linux | Context |
|---|---|---|---|
| `editor.indent` | cmd-] | ctrl-] | editor |
| `editor.outdent` | cmd-[, shift-tab | ctrl-[, shift-tab | editor |
| `editor.move_rows_up` / `_down` | alt-up / alt-down | alt-up / alt-down | editor |
| `editor.duplicate_rows` | shift-alt-down | ctrl-shift-alt-down | editor |
| `editor.delete_rows` | cmd-shift-k | ctrl-shift-k | editor |
| `editor.toggle_comment` | cmd-/ | ctrl-/ | editor |
| `go_to_line.toggle` | ctrl-g | ctrl-g | workbench |

- No new binding may take a key from vim in a vim context: wherever vim already binds the same keys (ctrl-[ is
  Escape in vim's insert mode on Linux; shift-tab, alt-up and the others wherever vim claims them), the new
  binding's context excludes that vim mode, so vim's binding still wins there.
- Linux duplicate_rows is ctrl-shift-alt-down, VS Code's Linux default (rev 2, helium lead): VS Code keeps shift-alt-down
  for insert-cursor-below on Linux, and so does fastedit (`editor.add_cursor_below`, alt-shift-down), so rev 1's
  shift-alt-down shadowed it. add_cursor_below keeps its keys.
- ctrl-g is Go to Line on every platform (VS Code's convention; user decision 2026-10-12). Off macOS that takes ctrl-g
  from find next, so `find.next`'s `secondary-g` binding becomes `{ macos = "cmd-g", default = "f3" }` and
  `find.previous`'s `secondary-shift-g` becomes `{ macos = "cmd-shift-g", default = "shift-f3" }`, both in context
  `editor || find_bar` as now. Enter and shift-enter in the find bar are unchanged. macOS keeps cmd-g and cmd-shift-g.

Menus, in `crates/fe-keymap/menus/fastedit.toml`, `default-macos.toml` and `default-linux.toml`: the Edit menu gets
a group with Indent, Outdent, Move Line Up, Move Line Down, Duplicate Line, Delete Line and Toggle Line Comment;
the Go menu gets Go to Line/Column… after Go to File….

## Tests

New file `crates/fe-keymap/tests/editing_bindings.rs`, resolving keys on both platforms through the same API the
existing keymap tests use. fe-keymap's `tests/one_keymap.rs` and `crates/fastedit/tests/menus.rs` must still pass.

The new bindings change fe-gpui tests that count or list the default keymap's bindings (rev 2): update
`command_tests.rs` (table_is_the_only_binding_source), `keymap_tests.rs` (default_keymap_matches_the_old_table),
`tests.rs` (bindings_are_all_tested) and `vim/tests.rs` (vim_layer_of_the_default_keymap) to the new keymap, and
nothing else in them.
