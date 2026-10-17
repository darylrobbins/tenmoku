---
task_uid: "01m4xqgmgd1fh99psdskbg0ym1"
task: "T129"
rev: 6
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-keymap test git_keys::bound: on macOS ctrl-shift-g runs git.show_changes, f7/shift-f7 and alt-f5/shift-alt-f5 run diff.next_change/previous_change in a DiffView, alt-f3/shift-alt-f3 git.next_change/previous_change in an editor, cmd-y git.stage_hunk, cmd-alt-z git.revert_hunk, cmd-enter git.commit_box.commit in the commit box; on Linux the ctrl-alt-y/ctrl-alt-z and ctrl-enter equivalents"
    required: true
    manual: false
  - id: "C2"
    text: "fe-keymap test git_keys::no_vim_keys: for every binding this card adds whose context an editor can match, its keystroke is not one fe-vim takes in normal, visual or operator-pending mode (checked against fe-vim's registry keys and the vim.key/vim.move bindings), or its context excludes those modes; plain ctrl-y, ctrl-e, ctrl-d, ctrl-u are not bound by it"
    required: true
    manual: false
  - id: "C3"
    text: "fe-keymap test git_keys::menus: the Go menu has Next Change and Previous Change, View has Toggle Line Blame and Toggle Inline Diff, File has Compare Active File With…, each showing its key where bound; existing menu and keymap tests pass"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-keymap passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Bind the git and diff commands to keys and menus, VS Code's and Zed's where they have them, taking no key vim uses in normal, visual or operator-pending mode (Mac T557)

## Problem

The git and diff commands (git-commands card and the feature cards after it) are in the palette and reachable by ex
commands, but have no keys or menu items. The vim-mode requirement (coordinator, 2026-10-12): new bindings must never
take a key fe-vim uses in normal, visual or operator-pending mode when vim is on. Keymaps and menus are shared files,
so all bindings for this request go in this one card (as T462 and T480 do for theirs).

## Change

In the single tables `crates/fe-keymap/keymaps/fastedit.toml` and `crates/fe-keymap/menus/fastedit.toml` (each binding
with a comment naming its VS Code or Zed origin). Mac T707 deleted the per-platform `default-macos.toml` and
`default-linux.toml` copies; `build.rs` now generates the per-platform forms from these tables (decision:
docs/decisions/ymt5qk6698-one-keymap-table.md). Write no per-platform file:

- `git.show_changes`: `ctrl-shift-g` (VS Code's Show Source Control; today unbound, `keys = ""`).
- `diff.next_change` / `diff.previous_change` in the `DiffView` context: `f7` / `shift-f7` (VS Code's diff editor) and
  `alt-f5` / `shift-alt-f5`.
- `git.next_change` / `git.previous_change` in `editor`: `alt-f3` / `shift-alt-f3` (VS Code's dirty-diff next/previous).
- `git.stage_hunk`: `cmd-y` on macOS (Zed's `git::StageAndNext` key, without the "and next"); `git.revert_hunk`:
  `cmd-alt-z` (Zed's `git::Restore`). On Linux `ctrl-alt-y` and `ctrl-alt-z`; never plain `ctrl-y` (vim's scroll).
- `git.commit_box.commit`: `cmd-enter` / `ctrl-enter` in the commit box's context (VS Code).
- Menus: Go gets "Next Change" and "Previous Change" (VS Code's Go menu); View gets "Toggle Line Blame" and "Toggle
  Inline Diff" (enabled in a DiffView); File gets "Compare Active File With…".
- No binding for blame, branch switching or file staging (VS Code has none); they're in the palette and ex commands.
- Every new binding in a context an editor can be in either has a `cmd` (macOS) or `alt`/function-key chord, or its
  context excludes `vim.mode == normal`, `vim.visual` and operator-pending; none is a key fe-vim's reducer or a vim
  feature module takes.

## Tests

New file `crates/fe-keymap/tests/git_keys.rs`.
