---
task_uid: "01m6svc3hcs8qtqsyd9yv0nbt0"
task: "T491"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "keymaps_picker_tests::lists_default_bindings_with_titles, keymaps_picker_tests::includes_user_vim_remaps and keymaps_picker_tests::enter_runs_the_command_in_the_focused_editor"
    required: true
    manual: false
  - id: "C2"
    text: "diagnostics_picker_tests::document_variant_lists_the_active_file_only and diagnostics_picker_tests::workspace_variant_lists_every_source_errors_first"
    required: true
    manual: false
  - id: "C3"
    text: "diagnostics_picker_tests::enter_opens_at_the_range and diagnostics_picker_tests::moving_previews_and_dismiss_restores_the_scroll"
    required: true
    manual: false
  - id: "C4"
    text: "buffer_lines_picker_tests::skips_blank_rows, buffer_lines_picker_tests::subsequence_match_highlights and buffer_lines_picker_tests::caps_a_huge_buffer_and_says_so"
    required: true
    manual: false
  - id: "C5"
    text: "vim_lazyvim_preset_tests::lazyvim_layout_is_the_table (rows for sk, sh, sd, sD, sb) and vim_lazyvim_preset_tests::unmapped_keys_are_listed_and_absent pass"
    required: true
    manual: false
  - id: "C6"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-workbench -p fastedit pass"
    required: true
    manual: false
---

# Add keymaps, diagnostics and buffer-lines pickers: LazyVim's <leader>sk, <leader>sh, <leader>sd, <leader>sD and <leader>sb (Mac T1408)

# Add keymaps, diagnostics and buffer-lines pickers: LazyVim's <leader>sk, <leader>sh, <leader>sd, <leader>sD and <leader>sb

## Problem
lazyvim.vimrc leaves `<leader>sk` (keymaps), `<leader>sh` (help), `<leader>sd`/`<leader>sD` (document and workspace diagnostics) and `<leader>sb` (grep the buffer) unmapped. Diagnostics are reachable only through the Problems panel, there is no way to search the keymap, and Tenmoku has no `:help`.

## Change
Delegates for the kiln-shell `Picker`, shown through `Workbench::show_palette` as the quick-open siblings are (see `symbol_picker.rs` for the preview-while-moving pattern):
- `keymaps_picker.rs`, `keymaps.search` ("Search Keymaps"): one row per binding in the active keymap (the bindings the command palette and the Keyboard Shortcuts editor already read), `keys  Command Title  context`, vim leader maps from `[[vim.remap]]` and the preset included with their `<Space>` spelled out; Enter runs the command in the editor that had focus, as the palette does. Rows match on keys and title.
- `<leader>sh` has no `:help` to open: it maps to `workbench.open_keyboard_shortcuts` (the Keyboard Shortcuts editor, the closest thing to a reference) and the lazyvim.vimrc comment says why. No new help system.
- `diagnostics_picker.rs`, `diagnostics.search_document` and `diagnostics.search_workspace` ("Search Diagnostics in File/Workspace"): rows from the Problems panel's sources (`problems_panel.rs` `FileProblems`; language server, tasks and plugins alike), severity icon, message, `file:row`; sorted errors first; Enter opens at the diagnostic's range; moving the selection previews in the editor, restoring scroll and caret on dismiss, as the symbol picker does. The document variant lists the active file only.
- `buffer_lines_picker.rs`, `search.buffer_lines` ("Search Lines in Buffer"): every non-blank row of the active editor, `row  text` with the match highlighted, same preview behaviour, for `<leader>sb` (telescope's current_buffer_fuzzy_find). Capped at 50,000 rows; a larger buffer lists the first 50,000 and says so in the header.
- Preset: `sk` -> `keymaps.search`, `sh` -> `workbench.open_keyboard_shortcuts`, `sd` -> `diagnostics.search_document`, `sD` -> `diagnostics.search_workspace`, `sb` -> `search.buffer_lines`; the five lines removed from the "Unmapped" comment list; TABLE rows added. Commands live in `crates/fe-workbench/src/picker_commands.rs` if it exists by then (the previous picker card creates it; otherwise create it) so `workbench_commands.rs` is untouched.

## Tests (new files)
- `crates/fe-workbench/src/keymaps_picker_tests.rs` (`keymaps_picker_tests::...`): `lists_default_bindings_with_titles`, `includes_user_vim_remaps`, `enter_runs_the_command_in_the_focused_editor`.
- `crates/fe-workbench/src/diagnostics_picker_tests.rs` (`diagnostics_picker_tests::...`): `document_variant_lists_the_active_file_only`, `workspace_variant_lists_every_source_errors_first`, `enter_opens_at_the_range`, `moving_previews_and_dismiss_restores_the_scroll`.
- `crates/fe-workbench/src/buffer_lines_picker_tests.rs` (`buffer_lines_picker_tests::...`): `skips_blank_rows`, `subsequence_match_highlights`, `caps_a_huge_buffer_and_says_so`.
- The existing `vim_lazyvim_preset_tests::lazyvim_layout_is_the_table` (TABLE gains the five rows) and `vim_lazyvim_preset_tests::unmapped_keys_are_listed_and_absent` pass.

## Implementation notes

- Scope widened to `crates/fe-workbench/src/problems_panel.rs` (revision 2): the
  panel's listed problems and where each opens (`Target`) are private, so it gains
  `ProblemsPanel::listed` / `ListedProblem`, `severity_color` and
  `Workbench::open_listed_problem`. The picker then lists exactly what the panel
  lists (open editors' diagnostics, unopened files' server diagnostics, task
  sources) and opens a row as the panel does, rather than copying `sync_problems`.
- Vim maps are read from the active editor's `VimState::remaps()` (walked with
  `RemapSet::continuations`), so no change to fe-gpui or fastedit's
  `vim_remaps.rs` is needed. Only normal-mode maps are listed. A map that runs
  `<Cmd>fe:{id}<CR>` is run as that command; any other has its keys typed into
  the editor it was listed from (focused, `<Esc>` first if it's not in normal
  mode), never wherever else focus was.
- The diagnostics picker previews only rows in the active editor's file: a
  workspace row in another file would have to open that file to preview.
- The "Unmapped" list had four of the five keys (`<leader>sD` was never listed),
  so it shrinks from 18 to 14; the "listed key the file maps is caught" check
  moves from `<leader>sk` to `<leader>cm`.
