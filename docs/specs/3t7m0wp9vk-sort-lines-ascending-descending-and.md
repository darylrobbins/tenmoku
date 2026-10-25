---
task_uid: "01m5qnvce86607nw3t7m0wp9vk"
task: "T1135"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor line_sort_tests::sort_ascending_whole_document: with one caret, editor.sort_lines_ascending sorts every row (stable, terminators and final newline kept) as one undo step. Rows compare with ICU4X icu_collator 2.x (compiled data, root locale \"und\", default tertiary strength, the collator Intl.Collator and so VS Code use), ties broken by code point so the order is total; add icu_collator to [workspace.dependencies] and fe-editor, and cargo deny passes (Unicode-3.0 is allowed). line_sort_tests::sort_uses_locale_order writes out the expected arrays: [\"b\",\"B\",\"a\",\"A\",\"é\",\"e\",\"10\",\"9\"] gives [\"10\",\"9\",\"a\",\"A\",\"b\",\"B\",\"e\",\"é\"], and the review counterexamples sort as root collation orders them (z before þ; å before ä)"
    required: true
    manual: false
  - id: "C2"
    text: "line_sort_tests::sort_descending_selection: with a selection over rows 2-4 ending at column 0 of row 5, only rows 2-4 reverse-sort; row 5 untouched; a selection inside one row changes nothing"
    required: true
    manual: false
  - id: "C3"
    text: "line_sort_tests::multiple_selections: two selections on separate row blocks sort independently in one transaction and keep their selections on the same text"
    required: true
    manual: false
  - id: "C4"
    text: "line_sort_tests::delete_duplicates: editor.delete_duplicate_lines keeps the first of each equal row in the range and removes later ones including their terminators; undo restores in one step"
    required: true
    manual: false
  - id: "C5"
    text: "command_catalog_tests lists the three ids with their titles, AND crates/fe-workbench/src/typed_command_ids_tests.rs gains the three ids (editor.sort_lines_ascending, editor.sort_lines_descending, editor.delete_duplicate_lines) in its expected list, failing without them; cargo nextest run -p fe-editor -p fe-workbench and clippy pass"
    required: true
    manual: false
  - id: "C6"
    text: "line_sort_tests::shared_row_selections_keep_their_text (review F6, F7): when two selections share a row and form one group, sorting maps each multi-row selection through the row permutation instead of collapsing it (e.g. [b\\nc]c[\\naa]), and delete-duplicates keeps a selection over the text that survives rather than treating two deleted endpoints as the whole selection deleted (e.g. [aa\\na][a\\nx\\naa] keeps x selected)"
    required: true
    manual: false
---

# Sort Lines Ascending/Descending and Delete Duplicate Lines commands, as VS Code's editor.action.sortLines* and removeDuplicateLines

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". VS Code's Selection/Edit commands Sort Lines Ascending, Sort Lines Descending and Delete Duplicate Lines (ids editor.action.sortLinesAscending, editor.action.sortLinesDescending, editor.action.removeDuplicateLines; no default keys) do not exist here. Verified: no sort, dedupe or line-transform command in fe-editor or the command catalog.

## Current state
- Row commands (indent, outdent, move/duplicate/delete rows) are registered in `crates/fe-editor/src/rows.rs:25-60` through `add(registry, name, title, true, ..)`, and the block-of-touched-rows logic is `Block` / `EditorModel::blocks` there (`rows.rs:62-`). A selection ending at column 0 of a later row leaves that row out.
- The exact list of registered command ids is asserted in `crates/fe-workbench/src/command_catalog_tests.rs` (editor.* ids around line 353) and titles in `typed_command_ids_tests.rs`; both must gain the new ids.

## Change
- New `crates/fe-editor/src/line_sort.rs` with `register` called from `commands.rs` and `mod line_sort;` in `lib.rs`. Commands `editor.sort_lines_ascending` ("Sort Lines Ascending"), `editor.sort_lines_descending` ("Sort Lines Descending"), `editor.delete_duplicate_lines` ("Delete Duplicate Lines").
- VS Code semantics: with a single empty selection the whole document is the range; otherwise each selection's touched rows (same rule as `Block`); a selection within one row does nothing for sort (VS Code needs two rows) . Sort is a stable sort of the rows' text (line terminators and the final newline kept in place), locale-aware as VS Code's `localeCompare` (so `a` sorts before `B`) with ties broken by code point; document the comparison chosen. Delete Duplicate Lines keeps the first occurrence of each distinct row text in the range and removes the later ones whole (row and its terminator).
- One transaction per command (one undo step); selections stay on their text where it survives, else collapse to the start of the range.
- No default keys and no menu entry (VS Code has none for these either); they appear in the command palette.

## Tests
New `crates/fe-editor/src/line_sort_tests.rs` (declared from line_sort.rs) with model-level cases; update the two catalog test lists.
