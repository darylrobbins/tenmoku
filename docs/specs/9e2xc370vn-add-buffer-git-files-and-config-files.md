---
task_uid: "01m6svbz4tfbj0gp9e2xc370vn"
task: "T490"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "buffer_picker_tests::lists_open_editors_most_recent_first and buffer_picker_tests::marks_modified_and_current"
    required: true
    manual: false
  - id: "C2"
    text: "buffer_picker_tests::first_row_selected_is_the_previous_buffer and buffer_picker_tests::enter_on_a_row_activates_that_editor"
    required: true
    manual: false
  - id: "C3"
    text: "listed_files_returns_tracked_and_untracked_unignored_paths"
    required: true
    manual: false
  - id: "C4"
    text: "git_files_picker_tests::lists_tracked_and_untracked_but_not_ignored, git_files_picker_tests::outside_a_repo_says_so and git_files_picker_tests::picking_opens_the_file"
    required: true
    manual: false
  - id: "C5"
    text: "config_files_picker_tests::settings_toml_first_then_the_rest, config_files_picker_tests::skips_dotfiles_and_cache_folders and config_files_picker_tests::stops_at_the_limit"
    required: true
    manual: false
  - id: "C6"
    text: "vim_lazyvim_preset_tests::lazyvim_layout_is_the_table (rows for fb, comma, bb, fg, fc) and vim_lazyvim_preset_tests::unmapped_keys_are_listed_and_absent pass"
    required: true
    manual: false
  - id: "C7"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-workbench -p fe-git -p fastedit pass"
    required: true
    manual: false
---

# Add buffer, git-files and config-files pickers: LazyVim's <leader>fb, <leader>, , <leader>fg and <leader>fc (Mac T1407)

# Add buffer, git-files and config-files pickers: LazyVim's <leader>fb, <leader>, , <leader>fg and <leader>fc

## Problem
In the lazyvim preset (crates/fastedit/presets/lazyvim.vimrc) `<leader>fb` (buffer list), `<leader>fg` (git files) and `<leader>fc` (config files) are unmapped, and `<leader>,` and `<leader>bb` only cycle tabs (`workbench.next_tab`), where LazyVim opens a buffer picker. Quick open (`quick_open.rs`) lists every project file, with no buffer-only, tracked-only or config view.

## Change
Three delegates for the kiln-shell `Picker` (as `symbol_picker.rs` and `recent_picker.rs` are), each opened in the modal layer through `Workbench::show_palette` with its own `Palette` variant (`workbench.rs`), rows matched by subsequence with `toolbar_picker::subsequence`, Enter opens the file in the active pane (Cmd/ctrl-Enter in a split is not needed):
- `buffer_picker.rs`, command `buffers.show` ("Show Buffers"): the open editors across panes, most recently used first (the order `tab_switcher.rs` already tracks), a dot for modified, the path relative to the root dimmed after the name, the current one marked. The first Enter-selected row is the previous buffer, not the current, so `<leader>,` then Enter is alt-tab. `ctrl-d`/`d` in the picker's normal mode closes the highlighted buffer (the picker stays open), if the kiln-shell picker lets a delegate bind a key; otherwise leave it out.
- `git_files_picker.rs`, command `git.show_files` ("Go to Tracked File"): the tracked and untracked-but-not-ignored files from `git ls-files -z --cached --others --exclude-standard` run off the main thread in the repo that contains the root (via a new `Repo::listed_files` in `crates/fe-git/src/files.rs`, using the existing git CLI runner `cli.rs`); outside a repo the picker shows "Not a git repository". The list is cached per HEAD id and index mtime so repeated opens are instant.
- `config_files_picker.rs`, command `workbench.show_config_files` ("Open Config File…"): the files under `fe_config::config_dir()` (settings.toml, keymap.toml, vimrc, themes, snippets, anything else), found by a small bounded walk (depth 3, 500 entries, skipping dotfiles and any `cache`, `state` or `target` folder), sorted settings.toml first. A pure `list_config_files(dir, limit)` is the testable core.
- Preset: `<leader>fb`, `<leader>,`, `<leader>bb` -> `buffers.show`; `<leader>fg` -> `git.show_files`; `<leader>fc` -> `workbench.show_config_files`; remove `fb`, `fg`, `fc` from the "Unmapped" comment list; `TABLE` rows in `vim_lazyvim_preset_tests.rs` (replacing the `<leader>,` and `<leader>bb` rows). The commands are `#[kiln::command]` methods in a new `crates/fe-workbench/src/picker_commands.rs` so `workbench_commands.rs` is not touched.

## Tests (new files)
- `crates/fe-workbench/src/buffer_picker_tests.rs` (`buffer_picker_tests::...`): `lists_open_editors_most_recent_first`, `marks_modified_and_current`, `enter_on_a_row_activates_that_editor`, `first_row_selected_is_the_previous_buffer`.
- `crates/fe-workbench/src/git_files_picker_tests.rs` (`git_files_picker_tests::...`): temp repo with a tracked, an untracked and an ignored file: `lists_tracked_and_untracked_but_not_ignored`, `outside_a_repo_says_so`, `picking_opens_the_file`.
- `crates/fe-workbench/src/config_files_picker_tests.rs` (`config_files_picker_tests::...`): `settings_toml_first_then_the_rest`, `skips_dotfiles_and_cache_folders`, `stops_at_the_limit`.
- `crates/fe-git/tests/listed_files.rs` (bare id `listed_files_returns_tracked_and_untracked_unignored_paths`).
- The existing `vim_lazyvim_preset_tests::lazyvim_layout_is_the_table` (TABLE gains the rows) and `vim_lazyvim_preset_tests::unmapped_keys_are_listed_and_absent` pass.

## Implementation notes

- Scope gained `crates/fe-git/src/lib.rs` (respec) for `mod files;`. The new modules are declared from `workbench.rs` (`#[path]`), so `lib.rs` of fe-workbench is untouched; the commands' actions are handled on the workbench's root element (`Workbench::attach_picker_commands`), not by app listeners, which would need `lib.rs`'s `init`.
- The pickers are made afresh each time they show, as the symbol picker is; `Workbench::shown_palette` finds the one showing, for tests.
- Buffers: panes keep no cross-pane clock, so the order is the active pane's tabs by recency, then each other pane's, in layout order; a file open in several panes is one row. No `d`/`ctrl-d` close: kiln-shell's picker gives a delegate no keys (its normal mode drops unbound letters).
- Git files: `Repo::listed_files` keeps only files and symlinks on disk (a deleted tracked file, a submodule and an untracked nested repo are left out) and lists the whole repo; the picker keeps those under the root. The kept list (keyed by HEAD id and `Repo::index_mtime`) shows at once and is then replaced by a fresh listing, since untracked files change neither.
- Config files: `settings.toml` is taken before the walk so the limit can't drop it; the limit counts every directory entry read (dotfiles and skipped folders too), and the walk stops reading once it's spent, so a folder the limit cuts short gives its entries in the file system's order; symlinked folders aren't followed.
- Buffers: an untitled buffer only another pane shows is activated there and that pane made the workspace's active one (`Workbench::activate`).
- Git files: a list landing keeps the selection on the selected file, if it's still listed.
