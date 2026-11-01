---
task_uid: "01m6c61yhg153n4nyxafb5hch3"
task: "T398"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench tests toggle_shows_authors_by_row_run, an_accepted_ai_hunk_shows_the_agent, edits_update_the_column, provenance_and_blame_share_the_column and toggle_twice_hides_it pass (src/provenance_column_tests.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited (git_blame_column_tests unchanged)"
    required: true
    manual: false
---

# Show who wrote each row in the gutter annotation column with Toggle Edit Provenance, AI rows tinted, sharing the column with git blame (Mac T1279)

## Problem

The buffer knows who wrote each span (`fe_buffer::Provenance`, previous card), but nothing shows it. User
direction 2026-11-01: "a gutter/blame-like annotation and a command to toggle it". The editor already has one
annotation column, left of the line numbers, used by git blame annotations
(`fe_gpui::GutterAnnotations`, `crates/fe-gpui/src/gutter_annotations.rs`; driven by
`crates/fe-workbench/src/git_blame_column.rs`, toggled by `git.toggle_blame_annotations`).

## Change

1. New `crates/fe-workbench/src/provenance_column.rs`, declared in `crates/fe-workbench/src/workbench_commands.rs`,
   which registers `editor.toggle_provenance` "Toggle Edit Provenance" (context `editor`).
   - Per editor, like blame annotations: on for the active editor only, gone when its tab closes.
   - Entries: one per run of rows from `Provenance::row_authors`, text `AI · <agent>` for an agent, `You` for the
     user, the tool's name (`Format`, `LSP`) or the host's; rows with no author blank. `heat` 1.0 for AI rows,
     none otherwise, so AI rows are the tinted ones.
   - Follows edits: rebuilt from the buffer's provenance after each edit, debounced as blame's `EDIT_REST` is.
   - Hover: `Written by <agent> (session <session>)` / `Written by you`; click does nothing.
   - One column: turning provenance on in an editor turns its blame annotations off and the reverse
     (`git_blame_column.rs` learns to yield), each toggle remembering only its own state.
2. No default keybinding; the command is in the palette.
3. Tests in new `crates/fe-workbench/src/provenance_column_tests.rs`, declared next to the module:
   `toggle_shows_authors_by_row_run`, `an_accepted_ai_hunk_shows_the_agent`, `edits_update_the_column`,
   `provenance_and_blame_share_the_column` (turning either on turns the other off), `toggle_twice_hides_it`.

Not here: persistence across sessions (see the previous card), a status-bar count of AI rows.

## Implementation notes

- The hover says `Written by <agent>` without the session: `fe_buffer::Author::Ai` keeps only the agent, and fe-buffer is out of this card's scope.
- Blame's column is private to `git_blame`, so it registers its remover with `provenance_column::yield_to_blame` when first toggled; each toggle then removes the other's column only.
- `EDIT_REST` is mirrored in `provenance_column.rs` (`git_blame` is private to `workbench`).
- Carried T215 F3 (edit rest during the initial blame) and F4 (release cleanup) fixed in `git_blame_column.rs`; F2 (cross-column dedupe) and T396 F1 (Restore availability) left for follow-up cards.
