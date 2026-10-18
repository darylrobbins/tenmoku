---
task_uid: "01m569n12jj0adx7kcket5kgxj"
task: "T195"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test kiln-shell same_titled_tabs_show_their_shortest_distinguishing_folders passes: same-titled tabs for crates/fe-vim/src/ex.rs and crates/fe-vim/src/tests/ex.rs are described 'src' and 'tests', a/src/y.rs and b/src/y.rs 'a/src' and 'b/src'; a lone tab and a tab with no path have no description"
    required: true
    manual: false
  - id: "C2"
    text: "Test kiln-shell a_description_goes_when_its_twin_closes passes: closing one of two same-named tabs leaves the other undescribed"
    required: true
    manual: false
  - id: "C3"
    text: "Test fe-workbench workbench::git_commands::git_log::git_log_open_tests::tab_names::two_commit_diffs_of_same_named_files_show_their_folders passes: opening src/ex.rs and src/tests/ex.rs from one Git Log commit gives two tabs titled 'ex.rs (\u003cs\u003e^ ↔ \u003cs\u003e)' described 'src' and 'tests'"
    required: true
    manual: false
  - id: "C4"
    text: "Test fe-workbench workbench::git_commands::git_log::git_log_open_tests::tab_names::two_file_tabs_of_same_named_files_show_their_folders passes: opening src/ex.rs and src/tests/ex.rs as files gives two 'ex.rs' tabs described 'src' and 'tests'"
    required: true
    manual: false
  - id: "C5"
    text: "DiffView still reports no PaneItem::path (Pane::position never finds a diff tab for a file); the description is drawn muted after the title; existing kiln-shell, fe-gpui diff_view and fe-workbench git tests pass; no file outside the scope changes, and no command registration changes"
    required: true
    manual: false
---

# Tell same-named tabs apart by the fewest folders that differ, shown muted after the title as VS Code does, for file tabs and diff tabs alike (Mac T810)

## Problem

The user's screenshot (2026-10-17, main 92c57c57) shows two tabs that both read
`ex.rs (81309b1^ ↔ 81309b1)`, opened from the Git Log for crates/fe-vim/src/ex.rs and
crates/fe-vim/src/tests/ex.rs. Nothing tells them apart.

Same-named **file** tabs aren't told apart either: `EditorView::tab_title` is the bare file name
(crates/fe-gpui/src/pane_item.rs:746), and kiln's tab strip draws `item.tab_title(cx)` alone
(kiln/crates/kiln-shell/src/pane.rs:501 `PaneState::tabs`, kiln/crates/kiln-ui/src/tab_bar.rs `Tab`).
So two `main.rs` tabs read `main.rs` twice. A diff tab can't use its path for this today: `DiffView` keeps
only the file *name* (`DiffView::new(file_name, …)`, crates/fe-gpui/src/diff_view.rs:196), and it must not
report `PaneItem::path`, because `Pane::position` finds a file's tab by its path (see T796).

VS Code adds, after the label and muted, the shortest part of the folder path that tells same-named
editors apart (`src` against `tests`).

## Change

1. **kiln-ui** (kiln/crates/kiln-ui/src/tab_bar.rs): `Tab::description(Option<SharedString>)`, drawn after
   the label in the theme's muted text colour at the label's size, with a small gap, and a getter
   `Tab::description_text() -> Option<&SharedString>` for tests.
2. **kiln-shell item** (kiln/crates/kiln-shell/src/item.rs): `PaneItem::tab_path(&self, cx) -> Option<&Path>`.
   It's the file a tab is named for, used only to tell same-titled tabs apart, and defaults to
   `self.path(cx)`. Add the matching `ItemHandle::tab_path -> Option<PathBuf>` and forward it.
   `#[kiln::item]` needs no change, as the method has a default.
3. **kiln-shell pane** (kiln/crates/kiln-shell/src/pane.rs `PaneState::tabs`): group the pane's items by
   equal `tab_title`. In a group of two or more, each item with a `tab_path` gets as description the
   fewest trailing folders of its path's parent, joined with `/`, that no other path in the group has as
   its own trailing folders. Compare whole components. When an item runs out of folders first, it gets
   its whole parent as the description (as `a/src/x.rs` against `src/x.rs`). Items whose paths are equal
   (two views of one file), items with no `tab_path` and items in no group get none. Examples:
   `crates/fe-vim/src/ex.rs` and `crates/fe-vim/src/tests/ex.rs` give `src` and `tests`;
   `a/src/y.rs` and `b/src/y.rs` give `a/src` and `b/src`.
4. **DiffView** (crates/fe-gpui/src/diff_view.rs): `DiffView::with_compared_path(self, PathBuf) -> Self`.
   `PaneItem::tab_path` returns it, else the right side's model path. `PaneItem::path` stays `None`.
5. **Hosts** set the compared path:
   - crates/fe-workbench/src/git_history.rs `land_commit` (commit diffs from History and the Git Log):
     `commit.path`, absolute;
   - crates/fe-workbench/src/git_blame.rs `show_commit`: `path`;
   - crates/fe-workbench/src/git_diff.rs `new_diff`: `path` (covers HEAD ↔ Index, whose sides are both
     path-less, and file compares).

   Each is one builder call on the `DiffView::new(...)` inside the existing `cx.new`.

Tests:

- kiln: a new integration test kiln/crates/kiln-shell/tests/tab_descriptions.rs, over notes with paths
  (as tests/support/note.rs builds them, or a local item with `path`/`tab_path`), reading
  `pane.pane().tabs(cx)`:
  - `same_titled_tabs_show_their_shortest_distinguishing_folders`: the examples above. A lone `z.rs`
    has none, and an item with no path has none.
  - `a_description_goes_when_its_twin_closes`: closing one of two same-named tabs leaves the other with
    none.
- fastedit: crates/fe-workbench/src/git_log_tab_names_tests.rs, declared at the end of
  crates/fe-workbench/src/git_log_open_tests.rs by
  `#[path = "git_log_tab_names_tests.rs"] mod tab_names;`, reusing that file's `git`, `commit`, `write`,
  `window`, `panel`, `press` and `draw`. It reads `kiln_ui::Tab`s through
  `workbench.pane().read(cx).pane().tabs(cx)` (`kiln_shell::PaneView`), so crates/fe-workbench/src/pane.rs
  doesn't change.
  - `two_commit_diffs_of_same_named_files_show_their_folders`: one commit edits `src/ex.rs` and
    `src/tests/ex.rs`. Opening both from the Git Log gives two tabs titled `ex.rs (<s>^ ↔ <s>)`,
    described `src` and `tests`.
  - `two_file_tabs_of_same_named_files_show_their_folders`: opening `src/ex.rs` and `src/tests/ex.rs`
    as files gives tabs `ex.rs` described `src` and `tests`.

Not in this card: tooltips with the full path on tabs.

Files T704 (helium) also touches: git_history.rs (one builder call in `land_commit`). Nothing here
touches command registration, git_commands.rs, git_log.rs, workbench.rs, workbench_commands.rs, lib.rs
or typed_command_ids_tests.rs.

## Implementation notes

- `PaneItem::tab_path` is `fn tab_path<'a>(&'a self, cx: &'a App) -> Option<&'a Path>`, so `DiffView`
  can return the right side's model path live, borrowed through `cx`; the default `self.path(cx)` fits.
- `ItemHandle::tab_path` has a default, `self.path(cx)`, so crates/fe-workbench/src/hot_exit.rs's
  `BackedUp` wrapper (an `ItemHandle` impl used only while quitting) builds without changing.
- Equal paths in a title group don't tell each other apart: an item is compared only with the group's
  *other* paths, so two views of one file get no description, and with a third, different file they
  get the description that tells them from it (as VS Code does).
- The description is a second text child after the label, in `colors.text.muted`, inheriting the label's
  size, `gap_1p5` between them, baseline-aligned.
- Carried T132 F1: `a_release_over_a_row_that_replaced_the_pressed_one_clicks_nothing` in
  git_log_open_tests.rs uses real mouse down / up, with the expansion landing and a redraw between; it
  fails if `render_rows` goes back to flattened-index row IDs (checked by hand), and checks ordinary
  clicks on a commit row and a file row still work.
