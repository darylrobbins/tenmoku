---
task_uid: "01m4rbveg4ksy7mtm3a2x7nn48"
task: "T86"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "indent_guides_tests: a nested Rust sample gives guides at columns 0, 4 and 8 over exactly the rows indented deeper; with tab_width 4, tab and mixed tab-and-space rows count columns as the spec says"
    required: true
    manual: false
  - id: "C2"
    text: "indent_guides_tests: blank rows inside a block keep its guides, a blank row before a dedent takes the greater neighbouring indent, and guides come clipped to the requested rows, sorted by column then row"
    required: true
    manual: false
  - id: "C3"
    text: "indent_guides_tests: active_indent_guide on a block's opening row is the guide starting on the next row, and inside a block the innermost guide containing the row"
    required: true
    manual: false
  - id: "C4"
    text: "indent_guides_tests: a row with over 400 bytes of indentation counts as 400 columns, a blank row 1,001 rows from any non-blank row on one side counts that side as 0, and indent_width 0 uses tab_width (both 0: 4)"
    required: true
    manual: false
---

# Compute the editor's indent guides headless in fe-editor: guide columns and row runs for the shown rows, and the active guide at the caret (Mac T491)

## Problem

Users browse code more than they edit it (~/.claude/plans/glittery-foraging-graham.md, Context), and in deeply
nested code the eye loses which block a row belongs to. The project tree has indent guides (T116,
843de07c) but the editor has none. Painting belongs in fe-gpui, but which rows and columns get a guide is
plain text work that can be computed and tested headless.

## Change

Add a new crates/fe-editor/src/indent_guides.rs with `IndentGuide { column: u32, rows: Range<u32> }`,
`indent_guides(text: &Text, rows: Range<u32>, indent_width: u32, tab_width: u32) -> Vec<IndentGuide>` and
`active_indent_guide(text, caret_row, indent_width, tab_width) -> Option<IndentGuide>`. Its only line in
crates/fe-editor/src/lib.rs is the `pub mod indent_guides;`, left out of the scope on purpose so this card doesn't block the other fe-editor cards that own lib.rs: expect one out-of-scope notice for it. Painting the guides (under folds and soft wrap)
and passing the file's detected indent width are a later Mac card; this card is headless.

- A row's indent is the columns of its leading spaces and tabs, a tab advancing to the next multiple of
  `tab_width`. Only the first 400 bytes of a row are read; a row indented further counts as indented 400
  columns.
- A blank row (only whitespace) takes the greater indent of the nearest non-blank rows above and below,
  looking at most 1,000 rows each way; past that, or past the text's ends, a side counts as 0.
- There is a guide at column `c = k * indent_width` (k = 0, 1, 2, …) over each maximal run of rows whose
  indent is greater than `c`. Guides come sorted by column then first row, with `rows` clipped to the
  requested `rows`. Work is proportional to the requested rows plus the blank-row look-around, never the
  whole text.
- `active_indent_guide`: if the next non-blank row after `caret_row` is indented deeper than `caret_row`
  (the caret is on a block's opening row), the guide at that next row's innermost column that starts there;
  otherwise the innermost guide whose rows contain `caret_row`. Its `rows` are not clipped (bounded by the
  same 1,000-row look-around each way).
- `indent_width` 0 is treated as `tab_width`, and both 0 as 4.

Tests go in a new crates/fe-editor/src/indent_guides_tests.rs, included from indent_guides.rs with
`#[cfg(test)] #[path = "indent_guides_tests.rs"] mod tests;` as decorations.rs does.
