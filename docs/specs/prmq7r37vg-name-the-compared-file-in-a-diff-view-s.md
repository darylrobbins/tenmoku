---
task_uid: "01m559x9pgssswy0prmq7r37vg"
task: "T796"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test fe-workbench workbench::git_commands::git_history::git_history_tests::diff_crumbs_tests::a_working_tree_diffs_crumbs_are_its_files_path passes: in a repository whose src/conf.toml is changed, with it open, git.diff_index and then git.diff_head each draw the crumbs [\"src\", \"conf.toml\"] (Pane::drawn_crumbs labels), the last of CrumbKind::File; never \"untitled\"."
    required: true
    manual: false
  - id: "C2"
    text: "Test fe-workbench workbench::git_commands::git_history::git_history_tests::diff_crumbs_tests::a_commit_diffs_crumb_is_its_tab_title passes: opening a commit's change from File History (git.file_history, enter) draws one crumb, the tab's title \"b.rs (\u003cshort\u003e^ ↔ \u003cshort\u003e)\", of CrumbKind::File."
    required: true
    manual: false
  - id: "C3"
    text: "Test fe-workbench workbench::git_commands::git_history::git_history_tests::diff_crumbs_tests::an_unsaved_editor_is_still_untitled passes: a path-less editor tab still draws the one crumb \"untitled\"."
    required: true
    manual: false
  - id: "C4"
    text: "DiffView still reports no PaneItem::path, so Pane::position never finds a diff tab for a file: no file under crates/fe-gpui or kiln changes, and the existing git_diff and pane_toolbar tests pass."
    required: true
    manual: false
---

# Name the compared file in a diff view's breadcrumbs instead of untitled: its working file's path, else the tab's title

## Problem

The user's screenshot (2026-10-17, main 92c57c57) shows the pane toolbar's breadcrumbs reading
"untitled", with a file icon, while the `regatta.toml (Index ↔ Working Tree)` diff tab is active.

Cause: the pane's crumbs (`Pane::crumbs`, crates/fe-workbench/src/pane.rs:1514) start with
`pane_toolbar::path_crumbs(item.path(cx), root)`, and kiln's `path_crumbs`
(kiln/crates/kiln-shell/src/crumbs.rs:29) names a document with no path `untitled`. `DiffView` doesn't
implement `PaneItem::path` (crates/fe-gpui/src/diff_view.rs:705), so it gets the trait's `None`. Every diff
view shows "untitled": git diffs, file compares and commit diffs from File History and blame. It isn't
tied to the plain-text left side (that's the language card).

`DiffView` mustn't simply report a path: `Pane::position` (pane.rs:877) finds a file's tab by its
`Tab::path`, so opening the file could land on its diff tab.

## Change

In `Pane::crumbs` (crates/fe-workbench/src/pane.rs) only, when the active item is a `DiffView`:

- its right side's model has a path (a diff against the working tree, a file compare): the path crumbs
  are that file's, as its editor's would be (folders under the root, then the file with its icon, each
  clickable as an editor's are);
- otherwise (HEAD ↔ Index, a commit's diff): one `CrumbKind::File` crumb labelled with the item's tab
  title (`b.rs (abc1234^ ↔ abc1234)`), not clickable, with `fe_ui::file_icon`'s icon for that label.

Other items with no path keep `untitled`, as an unsaved editor should. No change in crates/fe-gpui or kiln.

Tests in a new crates/fe-workbench/src/diff_crumbs_tests.rs, declared in
crates/fe-workbench/src/git_history_tests.rs by `#[path = "diff_crumbs_tests.rs"] mod diff_crumbs_tests;`,
using only base API and the parent's helpers (`git`, `fixture`, `window`, `dispatch`, `press`, `draw`), and
`Pane::drawn_crumbs` for the labels.
