---
task_uid: "01m4p9g3eesvszfw00yrryqcbg"
task: "T53"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-editor test multi_cursor::tests::carets_above_and_below: in \"abcdef\\nab\\nabcdef\" with the caret at (2,4), add_caret_above twice gives carets at (0,4), (1,2) and (2,4); add_caret_below from the last row changes nothing; a folded row is skipped"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test multi_cursor::tests::next_occurrence: from a caret inside the first foo of \"foo foobar foo\", select_next_occurrence selects that foo, the next press adds the last foo (not foobar's), a third adds nothing; from a hand-made selection of foo it matches inside foobar; a two-row needle matches across rows"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test multi_cursor::tests::all_and_lines: select_all_occurrences selects every whole-word match; split_selection_into_lines on a selection over 3 rows gives 3 per-row selections without line breaks, and inserting x then replaces all 3 as one undo step"
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor test multi_cursor::tests::single_toggle_and_box: single_selection keeps only the primary; toggle_caret adds a caret and removes it again but never the last selection; select_box over rows with a tab and a too-short row gives selections at the same display columns and none on the short row"
    required: true
    manual: false
  - id: "C5"
    text: "crates/fe-editor/src/tests/commands.rs lists the six new editor.* IDs as non-mutating, and cargo test -p fe-editor and cargo clippy -p fe-editor --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Add multi-cursor commands to fe-editor: a caret above or below, the next or every occurrence, a selection split into lines, and back to one caret (Mac T426)

## Problem

The plan's M6 is "Editing polish: multi-cursor, IME, undo, save/encodings" (`~/.claude/plans/glittery-foraging-graham.md`
line 110). The user (2026-10-11): "Draft any work in earlier phases we haven't drafted yet." The 2026-10-11 gap
analysis found no way to make a second caret: `EditorModel` is multi-cursor (`set_selections`,
`crates/fe-editor/src/lib.rs:589`; `insert` and `apply_with_carets` edit at every selection, :856/:1112) and the
view paints every caret, but the editor registry (`crates/fe-editor/src/commands.rs` `registry()`, :210) has no
command that adds one, and nothing collapses back to one.

## Change

New module `crates/fe-editor/src/multi_cursor.rs` (`mod multi_cursor;` in lib.rs) with an `impl EditorModel`
block, and `multi_cursor::register(&mut registry)` called from `commands::registry()`, using the same `add`
helper pattern (context `editor`). Every command here is non-mutating (`mutates_buffer(false)`) and returns
`Effect::Changed` when selections change, `Effect::Nothing` otherwise. Each catches up (`sync_display`) first.

Commands (ID: model method):

- `editor.add_cursor_above` / `editor.add_cursor_below`: `add_caret_above()` / `add_caret_below()`. Adds one
  caret on the display row above the topmost selection's head (below the bottommost one's), at that head's goal
  display column (`goals`, else its current column), using the same vertical-move code as `Movement::Up`/`Down`
  so folded rows are skipped and soft-wrapped rows count as rows. A shorter row puts the caret at its end, and the
  new caret keeps the goal, so repeating returns to the column on longer rows. At the first/last row: nothing.
- `editor.select_next_occurrence`: `select_next_occurrence()`. If any selection is a caret, each caret becomes the
  selection of `word_range_at` around it (a caret on whitespace stays a caret) and nothing else happens. Otherwise
  the needle is the primary selection's text; it is searched forward from the end of the bottommost selection,
  wrapping to the start once, and the first match that overlaps no existing selection is added. Matching is
  case-sensitive; it is whole-word exactly when the primary selection equals `word_range_at(primary.start)`.
  Multi-row needles match across rows. Search walks the rope's chunks; it never copies the whole text to a String.
- `editor.select_all_occurrences`: `select_all_occurrences()`. Same caret seeding and needle rules, then every
  match in the buffer is selected, capped at 10,000 selections (past that, stop and `log::warn!`).
- `editor.split_selection_into_lines`: `split_selection_into_lines()`. Each selection touching more than one row
  becomes one selection per row: the first from its start to that row's end, middle rows whole (without the line
  break), the last from the row start to its end; a last row it reaches only at column 0 is dropped. Carets and
  single-row selections are unchanged.
- `editor.single_selection`: `single_selection()`. Keeps only the primary selection; with one it does nothing.

Model methods without a command, for the mouse card that follows:

- `toggle_caret(offset)`: adds a caret at `offset` (clipped), or, if a selection contains or touches `offset`
  and there is more than one selection, removes that selection. Never removes the last one.
- `select_box(anchor: (u32, usize), head: (u32, usize))`, rows and display columns: one selection per buffer row
  from `anchor.0` to `head.0`, from the anchor's display column to the head's (tabs and wide chars through
  `ExpandedRow`, as the view lays them out), reversed when the head's column is left of the anchor's. A row that
  ends before the left column gets none, unless no row reaches it, then a single caret at the head.

Not in this card: key bindings, menus, the mouse and vim (the next card).

## Tests

New file `crates/fe-editor/src/multi_cursor_tests.rs`, declared from multi_cursor.rs
(`#[cfg(test)] #[path = "multi_cursor_tests.rs"] mod tests;`), using `crate::tests::{parse, render}`. Update the
expected command list in `crates/fe-editor/src/tests/commands.rs` (the `ids` assertion near :85).

## Implementation notes

- Two files outside the listed scope changed: `crates/fe-editor/Cargo.toml` gains the workspace `log` dependency
  for the `log::warn!` at the 10,000-selection cap, and `crates/fe-editor/tests/viewer.rs` lists the six new IDs
  as unsupported by `ViewerModel` (its `refuses_edits` test requires every registered command to be classified;
  the viewer already answers `Unsupported` for them).
- `commands::add` and `commands::Run` became `pub(crate)` so `multi_cursor::register` can use them; it runs last
  in `registry()`, so the six IDs follow `editor.insert` and `editor.move`.
- Whole-word matching checks that the chars either side of a match aren't of the needle's char class (the same
  classes `word_range_at` uses), with a line break never continuing a run, since `word_range_at` keeps to one row.
  Rejected matches step one byte, so an overlapping match can still be found.
- Caret seeding asks whether the char at the caret is whitespace, not the word `word_range_at` returns: just past a
  word that function gives the word, but such a caret stays a caret. At the end of the text the word before is taken.
- Wrapping, `select_next_occurrence` searches only matches that start before the bottommost selection's end, since
  later ones were all looked at already.
