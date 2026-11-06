---
task_uid: "01m6rmd93y6rd0pbepvcnewap5"
task: "T474"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-config test (tests/harpoon_store.rs) add_dedupes_caps_and_keeps_order: adding the same path twice keeps one slot, a tenth add is refused, move_to reorders, remove closes the gap"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test (tests/harpoon_store.rs) lists_are_per_project_and_survive_reload: two roots keep separate lists in \u003cstate_dir\u003e/harpoon/, and save then load returns the same order; a corrupt file loads empty and is left on disk until the next change"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test harpoon_tests::add_and_jump_open_the_marked_files: with three marks, harpoon_jump 2 activates (or opens) the second file, reusing an existing tab; a missing file toasts 'file missing' and leaves the list unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test harpoon_tests::the_list_picker_reorders_and_removes: the list shows slots in order, ctrl-k and ctrl-j move the selected row and persist, the remove key drops it"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test harpoon_tests::rootless_window_has_no_marks: the commands toast 'Open a folder first' and write nothing"
    required: true
    manual: false
  - id: "C6"
    text: "fastedit test vim_lazyvim_harpoon_tests::preset_maps_harpoon_keys: loading the lazyvim preset maps \u003cleader\u003eH, \u003cleader\u003eh and \u003cleader\u003e1..5 to the harpoon commands, none shadows an existing preset map, and every \u003cCmd\u003efe: id in the preset still names a registered command (vim_lazyvim_preset_tests' existing check)"
    required: true
    manual: false
  - id: "C7"
    text: "cargo clippy --workspace --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Harpoon-style file marks: a small per-project pinned file list you add to, reorder and jump through with 1-9, persisted, with palette commands and the lazyvim preset's <leader>H, <leader>h and <leader>1..5 (Mac T1397)

Lent from Mac T1397 (lend y42, 2026-11-06). Cut it back as-is with no Mac numbers in a helium batch.

## Problem

Neovim users coming from LazyVim's harpoon extra expect a short, per-project list of pinned files they jump between with one key (`<leader>H` add, `<leader>h` open the list, `<leader>1`..`5` jump). Tenmoku has recent files, quick open and tabs, but no small hand-curated list. Non-vim users get the same thing from palette commands.

Linux-OK: pure data plus palette-list UI, no screenshot criterion.

## Change

- `crates/fe-config/src/harpoon.rs` (new, GPUI-free like `recent.rs`): `MarkList` of project-relative paths (cap 9, deduped, order = slot), `add`, `remove`, `move_to(from, to)`, `get(slot)`, and `load`/`save` at `<state_dir>/harpoon/<key>.toml`, `<key>` = a stable hash of the canonical project root. Atomic write like `recent.rs`; an unreadable or bad file loads as empty and is not overwritten until the next change. Paths that no longer exist are kept in the list (shown dimmed in the picker) so a branch switch does not lose marks; jumping to one toasts "file missing".
- `crates/fe-workbench/src/harpoon.rs` (new): commands `workbench.harpoon_add` (Harpoon: Add File; already there = toast, no duplicate), `workbench.harpoon_toggle_list` (Harpoon: Show List), `workbench.harpoon_jump` with an integer arg 1..9 (arg shape: copy how `workbench.go_to_tab`-style indexed commands take their argument; if none exists, register `harpoon_jump_1`..`harpoon_jump_5`), `workbench.harpoon_remove_current`. The list is a `Picker` (the recent picker in `recent_picker.rs` is the model): rows show slot number, file name, dimmed directory; Enter opens; `ctrl-k`/`ctrl-j` move the selected row up/down (writes the new order), `dd` in vim normal mode / `ctrl-d` otherwise removes. Marks are per window root; a rootless window has none and the commands toast "Open a folder first".
- Opening goes through the normal open path, so it reuses an existing tab.
- `crates/fastedit/presets/lazyvim.vimrc`: new `" harpoon` block: `nnoremap <leader>H <Cmd>fe:workbench.harpoon_add<CR>`, `<leader>h` toggle list, `<leader>1`..`<leader>5` jump. Check `<leader>1..5` and `<leader>h`/`<leader>H` are not already mapped in the preset (batch 2 card "Start page" may add `<leader>` dashboard keys; they do not use these).

## Out of scope

Per-branch lists, the harpoon2 "quick menu" text-buffer editing, tmux marks.

## Conflicts

`crates/fastedit/presets/lazyvim.vimrc`, `crates/fe-workbench/src/lib.rs` (mod list), `workbench_commands.rs` and the command catalog test are touched by batch-1 cards (quickfix, pickers, run test at cursor, todo-comments) and by the Start page, snippets and line-ending cards in this batch. Land order does not matter, but rebase carefully: edits are additive blocks.


## Helium notes
- New bindings or commands have broken count tests before (fe-gpui binding counts / DEDICATED, the git_keys chord, crates/fastedit/src/vim_remaps.rs for lazyvim.vimrc). Run the affected crates' tests early and respec any count-pinning test file into scope before the first finish (`rgt task respec --scope`, which replaces the list).

## Implementation notes

- Jump ids: a typed command takes no args, and the existing indexed commands
  carry the index in the id (`workbench.open_editor_at_index1`..`9`; kiln's
  snake case wants no underscore before the digit). So there are nine ids,
  `workbench.harpoon_jump1`..`workbench.harpoon_jump9`, one per slot; the
  lazyvim preset maps `<leader>1`..`5` to the first five.
- `workbench.harpoon_remove_current` is titled "Harpoon: Remove File".
- The list picker's `ctrl-k`, `ctrl-j`, `ctrl-d` and vim-normal `d d` are
  caught with `App::intercept_keystrokes` while the picker has focus, ahead
  of the keymap: on Linux `ctrl-k` starts the `secondary-k` chords, and the
  default keymap isn't touched.
- The picker is shown through the shell's modal layer under its own
  `ModalId("harpoon")`, as git's stash picker is, so `workbench.rs` and the
  `Palette` list are unchanged; it closes when the window's root changes.
- List files hold `files = [...]` of project-relative paths; entries that
  are absolute, use `..`, or repeat are dropped on load.
- `vim_lazyvim_preset_tests.rs` was respecced into scope: its table pins
  every preset map.
