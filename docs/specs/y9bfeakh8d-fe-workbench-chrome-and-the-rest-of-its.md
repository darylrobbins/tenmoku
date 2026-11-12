---
task_uid: "01m6vvw1apttnhcvy9bfeakh8d"
task: "T1563"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-workbench --test l10n passes with l10n-allow.toml's migrated list replaced by all = true"
    required: true
    manual: false
  - id: "C2"
    text: "en-US byte-equal for the files moved here; cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
---

# fe-workbench chrome and the rest of its text; the lint covers the whole crate (i18n W8)

# fe-workbench chrome and the rest; the lint covers the whole crate (W8)

Spec: card **W8** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9, for the fe-workbench files the B3 surface
cards don't restyle (they move their own files' text). Sections 3–8 are the
design. en-US stays byte-equal to today's, so no golden and no behaviour test
changes except tests that construct removed literals.

Files: every remaining `crates/fe-workbench/src/*.rs` (non-test) not in an earlier card's `migrated` list: `workbench.rs`, `workbench_root.rs`, `workbench_palettes.rs`, `pane_group.rs`, `pane_drag.rs`, `dock_focus.rs`, `dock_leave.rs`, `focus_group.rs`, `auto_save.rs`, `compare_saved.rs`, `disk_sync.rs`, `close_many.rs`, `closed_editors.rs`, `external_drop.rs`, `external_format.rs`, `hot_exit.rs`, `loader.rs`, `markdown_preview.rs`, `open.rs`, `open_at.rs`, `path_commands.rs`, `persist.rs`, `picker.rs`, `picker_commands.rs`, `preserve_case.rs`, `save_commands.rs`, `spell.rs`, `spell_ranges.rs`, `spell_squiggles.rs`, `tab_index_commands.rs`, `tab_switcher.rs`, `todo_comments.rs`, `vim_buffers.rs`, `vim_filter.rs`, `vim_host.rs`, `vim_pick.rs`, `vim_tabs.rs`, `vim_windows.rs`, `workspace_edit.rs`, `zen_mode.rs` and `vim_line/`.

Add `crates/fe-workbench/l10n/en-US/workbench.ftl` (keys `workbench-workbench-…`) to
`resources!` in `crates/fe-workbench/src/l10n.rs`, and these files to
`l10n-allow.toml`'s `migrated` list. English plural helpers become plural
messages; fragments become whole messages.

Then replace the `migrated` list with `all = true`: the lint covers every file.
Check first that no file is left out (the lint, run with `all = true`, is the
check).

Platform: **Linux-OK**.

## Implementation notes

- **Files beyond the list.** With `all = true` the lint also reported
  `lib.rs` (the commit box's registry title), `todo_picker.rs`,
  `todo_commands.rs`, `plugin_access.rs` and `plugin_format.rs`, which
  postdate the list; rev 2 added them to the scope. `vim_line/` holds only
  tests. The registry titles (`lib.rs`, `plugin_access.rs`) now register
  with their `command-<id>` message, as `project_panel.rs` and
  `search_panel.rs` do.
- **Vim messages keep their consts.** `git_diff.rs` and `workbench_files.rs`
  (other cards' files) take `vim_host::E13`, `E37`, `NO_SHELL` and `RELOAD`
  as `impl Into<String>`. So the consts became `Said`, a key resolved each
  time it's said that converts to `String` and compares by its text
  (`vim_filter`'s tests compare its consts).
- **Whole messages.** The `[noeol] ` report, the `File missing:` prefix,
  the workspace edit's `(… already written)` and the TODO picker's
  lowercased cap note are each whole messages now.
- **Known gap.** fe-gpui's `end_vim_host(what)` puts `what` into its own
  `gpui-vim-map-keys-not-run` ("a map's keys after { $what } aren't run"),
  so `a quit` and `a command that finishes later` are still translated
  phrases inserted into another message. Making them whole messages needs a
  change to fe-gpui's API.
- **Byte equality.** Each new message, with its placeholders normalised,
  equals a removed literal, or one of the joins above, which give the same
  bytes.
- **Carried findings.** T1548 F4 is fixed: one origin flag,
  `Workbench::bottom_from_sidebar`, cleared when focus leaves any of the
  bottom panel's tool windows (the terminal's own flag is gone).
  `dock_leave_tests::up_from_another_tool_window_goes_to_the_editor` runs
  both cross-tool sequences. T1378 F1 is fixed: under test,
  `Dictionary::user()` reads only a config directory the test names
  (`set_test_config_dir`). `user_dictionary_ignores_the_environment_under_test`
  shows that a `$TENMOKU_CONFIG_DIR` dictionary is neither read nor
  written. T1310 F2 (open_at.rs) isn't fixed, and this card doesn't touch
  that file. Character columns and the last-row clamp belong in fe-gpui's
  `FileViewer` (`RowTarget`, `put_back`), which is out of scope.
