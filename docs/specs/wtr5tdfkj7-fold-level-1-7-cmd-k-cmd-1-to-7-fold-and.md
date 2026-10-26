---
task_uid: "01m5v8z1gj06x7ekwtr5tdfkj7"
task: "T1180"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fold_levels_tests::level_n_folds_that_depth: for candidates nested class \u003e fn \u003e block, editor.fold_level_1 folds only the class, fold_level_2 the fns (leaving level 1 open), and a level deeper than the nesting folds nothing; existing folds at other levels are kept"
    required: true
    manual: false
  - id: "C2"
    text: "fold_levels_tests::level_skips_the_caret_row: as VS Code, fold_level_N does not fold a candidate containing a caret; carets inside a newly folded range are not moved"
    required: true
    manual: false
  - id: "C3"
    text: "fold_levels_tests::recursive: editor.fold_recursively folds the candidate at the caret and every candidate nested in it; editor.unfold_recursively opens it and everything inside it, and leaves sibling folds closed"
    required: true
    manual: false
  - id: "C4"
    text: "fold_levels_tests::indent_fallback: with no syntax (host fold_ranges empty) levels follow indentation blocks, matching editor.fold_all's fallback"
    required: true
    manual: false
  - id: "C5"
    text: "fe-keymap fold_level_keys: secondary-k secondary-1..7, secondary-k secondary-[ and secondary-k secondary-] resolve in the editor context and do not disturb secondary-k secondary-0/j; Catalog tests updated (command_catalog_tests, typed_command_ids_tests, fe-editor tests::commands); cargo nextest run -p fe-editor -p fe-keymap -p fe-workbench -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# Fold Level 1-7 (cmd-k cmd-1 to 7), Fold and Unfold Recursively (cmd-k cmd-[ and ]), as VS Code's editor.foldLevelN, editor.foldRecursively and editor.unfoldRecursively

Today only Fold, Unfold, Fold All and Unfold All exist (commands.rs). Candidates come from the Host (fold_ranges(rows), fold_range_at(row)); depth is the nesting of candidate ranges. New module fold_levels.rs using EditorModel's public fold API (fold_rows/unfold_rows/folds), so lib.rs needs only its mod line. Seven level commands are registered from one table. Mac-or-Linux, no rendering change.

## Implementation notes

- Levels: candidates sorted by header row, the outermost of those sharing a
  header row standing for them (syntax can give two, such as `foo({`); a
  candidate's level is one more than the candidates around it, and one that
  overlaps another without nesting counts as beside it.
- Fold Level N skips a candidate holding a selection's first or last row
  (VS Code's uses the selection's start line; the end too keeps a head from
  being folded away), so no caret moves. Existing folds stay; a level with
  nothing to fold changes nothing rather than reaching Fold All's
  "no candidates" fallback.
- Fold Recursively folds the host's candidate at each caret and every
  candidate inside it, in the rows the host gave, then catches up; a caret
  with no candidate then folds the indented block at it and the blocks
  inside, found in the buffer once caught up (as Fold does), and the host's
  candidates inside it, followed through the catch-up's edits as anchors
  (after their first row's start, before their last row's end). Carets
  folded away move to the header, as Fold's do.
- Unfold Recursively opens the folds headed on the caret's row and those
  inside the candidate at it, the same two phases, keeping every other fold
  (re-made with `DisplayMap::unfold_all` + `fold_many`, since
  `DisplayMap::unfold` also opens a sibling sharing a boundary row).
  Review round 1 (F1, F2): `recursive_unfold_keeps_a_sibling_sharing_a_row`
  and `recursive_fallback_follows_edits_not_caught_up`; round 2 (F3, F4):
  `recursive_fallback_folds_the_candidates_inside`, and the unfold half of
  the stale-edit test now folds before the other model's edit.
- Outside the listed scope, two tests had to learn the new commands:
  `fe-editor/tests/viewer.rs` (a viewer reports them unsupported, as it does
  the other folds) and `fe-gpui/src/tests.rs` (`bindings_are_all_tested`
  requires a case for every editor binding).
