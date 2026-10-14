---
task_uid: "01m4w17v4sk5g20n0e3zctkm8m"
task: "T713"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln-ui tests::tree_row_status: TreeRow::label_color(c) draws the label in c, but an error row keeps the error colour and an ignored row stays muted; TreeRow::badge(\"M\", c) draws M in c at the row's right end (tree-badge:{label}), and a long label ellipsizes before it"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench project_panel_git_status_tests: after GitPanel::set_statuses with a.rs Modified, n.rs Added, u.rs Untracked and x.rs Conflicted, those rows show badges M, A, U and C with label and badge in git_panel::letter_color for their kind; an unchanged file and an ignored file show no badge and keep their colour"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench project_panel_git_status_tests: a collapsed folder c holding c/d/e.rs Untracked shows a • badge and the Untracked tint, as does c/d once expanded; a folder holding both a Modified and a Conflicted file takes the Conflicted colour; a folder holding only a deleted file still shows the dot; after set_statuses(vec![]) no row has a badge or tint"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench project_panel_git_status_tests: a workbench opened on a real repo with one modified file shows that file's M in the project tree after run_until_parked, without the Changes panel ever being shown; git_panel_tests' show_changes test now expects that one run before Changes shows and otherwise passes unchanged"
    required: true
    manual: false
---

# Show git status in the project tree: tinted file names, an M A U D C letter at each changed file's right end, and a tint and dot on folders holding changes

## Problem

The user (2026-10-14) asked for "better git file status indicators", picking: tint tree file names by
status, status letters in the tree, and a roll-up to folders so a collapsed tree still shows where the
changes are (tab tints are a separate card).

The project panel (`crates/fe-workbench/src/project_panel.rs`, `ProjectPanel::render_rows`) shows no git
status. Only the Changes panel does: `git_panel.rs` `letter` (M/A/D/U/C, ~line 406) and `letter_color`
(error colour for deleted/conflicted, accent for modified, accent turned `ADDED_HUE` for added/untracked).
Its list comes from `GitState` (`git.rs`) through `GitPanel::begin`/`landed`, or `GitPanel::set_statuses`,
and `GitState` reads nothing until the Changes panel is first drawn
(`git_panel_tests.rs` `show_changes_lists_the_changed_files_sorted_with_their_status`: "Nothing is read
until the panel shows"), so today a tree would have no statuses until the user opens Changes.

`TreeRow` (`kiln/crates/kiln-ui/src/tree_row.rs`) has no way to colour its label or show anything after it.

## Change

One status pipeline: the Changes panel's list. No second status read, no new theme tokens.

1. **kiln-ui `TreeRow`**: add `label_color(Hsla)`, used for the label (not the icon or chevron) unless the
   row is `error` (error colour wins) or `ignored` (muted wins); and `badge(text, Hsla)`, a short text drawn
   at the row's right end in that colour, `flex_none`, so a long label ellipsizes before it. Give the badge
   `debug_selector` `tree-badge:{label}`. Tests in the new `kiln/crates/kiln-ui/src/tests/tree_row_status.rs`
   (registered in `src/tests/mod.rs`) through `TreeRow::colors` and the drawn badge.
2. **git_panel.rs**: make `letter` and `letter_color` `pub(crate)` for the tree to reuse. Add
   `GitPanelEvent::Listed`, emitted whenever the listed entries change (`show`, `landed` with no repository
   or a failure clearing them), and a `pub(crate) fn statuses(&self)` yielding each entry's path relative to
   the root and its `ChangeKind`.
3. **project_panel.rs**: `ProjectPanel::set_git_statuses(iter of (relative path, ChangeKind))` stores a map
   for files and one for folders: every ancestor directory of a changed path (up to, not including, the root)
   gets the highest-precedence kind beneath it, Conflicted > Modified > Deleted > Added > Untracked. It
   notifies only if the maps changed. In `render_rows`, look up each visible row's path relative to the tree
   root (`FileTree::path` stripped of `root_path`):
   - a changed file: `label_color(git_panel::letter_color(kind, &self.theme))` and
     `badge(git_panel::letter(kind), same colour)`;
   - a folder holding changes: the same tint, and badge `•` instead of a letter, expanded or collapsed;
   - anything else, ignored rows included, unchanged.
   A deleted file has no row; its folders still roll up.
4. **workbench.rs**: in `follow_changes`, on `GitPanelEvent::Listed` hand the Changes panel's `statuses()`
   to the root's project panel. Read the status as soon as the root's tree opens, so the tree shows changes
   without the Changes panel ever being drawn: `ProjectPanel` emits `PanelEvent::Opened` from `opened` on
   success, and `follow_root` answers it with `git.refresh`. Update the module doc in `git.rs` ("Nothing runs
   until the panel's drawn…") and the one assertion in `git_panel_tests.rs` that expects no run before the
   Changes panel shows (it now expects the run the tree's opening started).

Tests go in the new `crates/fe-workbench/src/project_panel_git_status_tests.rs`, registered from
`project_panel.rs` with `#[path]` as `project_panel_hover_tests.rs` is. Drive statuses with
`GitPanel::set_statuses` except for the live criterion. Keep repositories tiny.
