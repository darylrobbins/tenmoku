---
task_uid: "01m6w0b5f7twh1revnmdh04683"
task: "T1606"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::move_to_new_window (cargo nextest run -p fe-workbench, the opener faked as windows_tests.rs does) pass: the_new_window_gets_the_root_file_and_position_and_the_tab_closes_here, a_dirty_tab_asks_save_and_move_or_cancel, a_non_file_tab_has_no_item, the_last_tab_keeps_its_window_open, the_menu_item_and_command_exist"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench windows pane passes and cargo nextest run -p fastedit passes (the opener's new form)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Pane menu: Move to New Window for a file tab

# Pane menu: move this tab to a new window

Spec: `docs/design/design-system.md#workbench-anatomy` (Pane controls: the ⋯
menu offers "... move to new window ..."). There is no command today; the pane
menu card (T1542) leaves the item out for that reason. `window.new` and
`open_window(root)` exist (`crates/fe-workbench/src/open.rs`,
`set_window_opener` in `crates/fastedit/src/main.rs`).

## Change

- Command `pane.move_to_new_window` (palette "Pane: Move to New Window"), and
  the item in the pane's ⋯ menu: opens a window on the same root and opens the
  tab's file there at the same position, then closes the tab here. The opener
  gains a form that hands the new window a file to open (`open.rs`,
  `crates/fastedit/src/main.rs` `open_window_on`).
- A tab with unsaved changes asks first (Save and move / Cancel), as closing
  does; the move itself never carries an unsaved buffer. A tab that is not a
  file (settings, a diff, the start page) does not offer the item.
- The last tab of a window moves like any other and the window stays open.

## Not in this card

Moving a split with several tabs, drag a tab out to a window, moving between
existing windows.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-pane-…` in
`crates/fe-workbench/l10n/en-US/pane.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**: the opener is faked, as `windows_tests.rs` does.

## Implementation notes

- **Opener.** `set_file_window_opener(cx, Fn(Option<PathBuf>, Option<WindowFile>, &mut App) -> Task<bool>)`
  is the new form; `WindowFile { path, line, col }` is 1-based, column in chars, as
  `Workbench::open_document_at` takes it. The task says whether the file landed in a
  tab of the new window (`window.new` passes no file and its answer isn't read).
  Tenmoku's `open_window_on` (`main.rs`) uses it. The old `set_window_opener` stays
  (tests use it) but can't take a file, so a move with it does nothing.
- **Fresh window (review F2).** A window opened for a moved file is a fresh one: a new
  id in the root's layouts (`LayoutStore::claim_new`), restoring none of the root's
  saved windows, so no restored tabs race the moved file or duplicate it, and the
  saved window stays for the next `window.new`. Host test:
  `tests::a_moved_file_opens_alone_in_a_fresh_window_at_its_place` (fastedit).
- **Close after loading (review F1, rounds 1–2).** The tab closes only once the
  opener's task says the file loaded in the new window from disk, and only if the
  tab is still open, still that file's and has no unsaved changes. "Loaded" is
  `fe_workbench::file_loaded`: an editor whose buffer was read from the file (a
  missing file opens as a new file's empty buffer, with no disk stamp, and doesn't
  count), a large file's viewer, or an image once decoded (waited for; one that
  shows a message instead of a picture doesn't count). On failure the host closes
  the window it opened for the file, the tab stays and the source pane toasts
  "Can't move <name> to a new window". While a tab is moving (asked to save, or
  waiting for the window), asking again doesn't open a second window. It's taken
  out as Close Tab takes it: a split pane it empties closes, the window's last pane
  stays (empty). A dirty tab asks Save and Move / Cancel and moves once the save
  succeeds. Tests: `tests::move_to_new_window::a_tab_closes_only_once_its_file_has_landed_in_the_new_window`
  (fe-workbench); the fastedit host test above covers a decoded image, a missing
  file, an image that can't be decoded and an unreadable file (unix).
- **Which tabs.** A tab is a file if its item has a `path`: editors, the large-file
  viewer and the image viewer. Settings, diffs, the Markdown preview tab and
  untitled buffers have none; the ⋯ menu doesn't list the item and the command
  does nothing for them.
- **Known limit (lead-accepted).** The large-file read-only viewer moves without its
  position: the new window opens it at the start. Its head is a byte offset only
  its own index can turn into a line and column, and that is fe-gpui's, outside
  this card.
- **Scope added (self-respec).** `toolbar_picker.rs` holds the ⋯ menu's rows;
  `tests/pane_controls_b3.rs`, `tests/pane_toolbar.rs` and `tests/pane_maximize.rs`
  pin the row list and gained the new row.
- **i18n.** `pane.ftl` was already registered and fe-workbench's lint is
  `all = true`, so no `l10n.rs` or `l10n-allow.toml` change was needed.
