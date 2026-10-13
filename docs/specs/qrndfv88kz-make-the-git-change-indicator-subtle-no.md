---
task_uid: "01m4sbzn28daa2qjqrndfv88kz"
task: "T583"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor row_change_decorations_tests: an added or modified change yields exactly one decoration, Gutter(Bar) with Tint::Added or Tint::Modified over its rows at priority -100, and no RowBackground; a deleted change still yields one Gutter(Deletion); the file's other expectations change only in these per-change counts"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui change_rail_tests: a drawn EditorView whose own model has a diff base (an added run, a modified row, a deleted row) lays out no RowBackground and paints nothing in Layer::Backgrounds (PAINTED has no \"backgrounds\"), and lays out one bar per change inside frame.change_lane in change_added / change_modified at opacity 0.6"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui change_rail_tests: the deleted row's mark is centred on its row edge inside change_lane, lane-wide and 3 px tall as T484 made it, in change_deleted at opacity 0.6"
    required: true
    manual: false
  - id: "C4"
    text: "Every other test passes; the only edited expectations are the two opacity(0.8) -\u003e 0.6 in decorations_tests.rs and the counts in row_change_decorations_tests.rs; change_bar_tests.rs, the theme files and fe-config change_colors are untouched; cargo clippy -p fe-editor -p fe-gpui --all-targets -- -D warnings is clean"
    required: true
    manual: false
  - id: "C5"
    text: "git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Make the git change indicator subtle: no background on changed rows, and a softer bar and deletion mark in the change lane

## Problem

The user, 2026-10-12: "Let's aim for a more subtle indicator when lines are added or removed from a file in
the working copy. Lighter color, not a background. Maybe we need an initial rail between line numbers and
text." Their reference (a JetBrains editor): a thin green bar in a narrow rail just right of the line
numbers on added rows, a small red tick where rows were deleted, and no row background.

At main 3eec614a the rail already exists: T484 (147b9f1a) gave the change marks their own lane,
`Frame::change_lane` in `crates/fe-display/src/layout.rs` (digits | 0.5 em gap | 0.5 em lane | fold
column | text), and `Decorations::layout` in `crates/fe-gpui/src/element/decorations.rs` draws one rounded
3 px bar per run of rows there, and a deletion dash across the lane on the row edge. What makes the
indicator loud is the rest:

- `crates/fe-editor/src/row_change_decorations.rs` (`RowChangeDecorations::decorations`) pushes a
  `DecorationKind::RowBackground` next to each `Gutter(Bar)` for every added and modified change, so every
  changed row is tinted across the whole text area (`ROW_BACKGROUND_ALPHA` 0.08 of the change colour).
- The marks are painted at `GUTTER_ALPHA` 0.8 of `change_added` / `change_modified` / `change_deleted`
  (T485, 016efde2), which in fastedit Dark are the saturated `#61ef79`, `#61afef` and `#e06c75`.

## Change

- `crates/fe-editor/src/row_change_decorations.rs`: added and modified changes push only
  `Gutter(GutterMark::Bar)`; no `RowBackground`. Deleted changes are unchanged (one `Gutter(Deletion)`).
  Update the expectations in `crates/fe-editor/src/row_change_decorations_tests.rs` that pin the
  background (`added_modified_and_deleted_rows`, `queries_return_only_overlapping_changes`,
  `follows_edits_and_diffs_once_per_version` count two decorations per change; it is now one).
- `crates/fe-gpui/src/element/decorations.rs`: `GUTTER_ALPHA` 0.8 -> 0.6, so the bar and the deletion
  mark are a softer blend of the theme colour over the background in every theme, kiln themes included
  (fastedit Dark added becomes about `#439954`, Light about `#82e392`). Bar geometry, the lane, the 3 px
  width, the 1 px inset and the deletion dash's shape and place stay as T484 made them. The theme files
  and their `change_*` values don't change. Update the two `opacity(0.8)` expectations in
  `crates/fe-gpui/src/element/decorations_tests.rs` to 0.6; nothing else in that file changes.
- New `crates/fe-gpui/src/element/change_rail_tests.rs`, included from decorations.rs with
  `#[cfg(test)] #[path = "change_rail_tests.rs"] mod change_rail_tests;` as `change_bar_tests` is. It draws
  a headless `EditorView` whose own `EditorModel` has a diff base (`set_diff_base`), not a fixed provider,
  so it covers the real provider end to end.

T486 (human check of the change bar) still says "The colours look like before"; the lead updates its text
after this lands.
