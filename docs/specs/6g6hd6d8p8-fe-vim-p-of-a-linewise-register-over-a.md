---
task_uid: "01m4tj954dgvrejw6g6hd6d8p8"
task: "T651"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test lines_put_before_a_block_reaching_its_rows_end_go_below_its_first_row (tests/visual_block_put_lines.rs, as given in the spec) passes; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test lines_put_before_other_blocks_still_go_above passes: a block from column 0 or with text left after it still takes the lines above, and p still puts below the cursor's row"
    required: true
    manual: false
  - id: "C3"
    text: "Only crates/fe-vim/src/visual/put.rs changes under src; put's doc comment states the exception; cargo nextest run -p fe-vim passes with every tests/corpus/*.expected.json byte-identical"
    required: true
    manual: false
---

# fe-vim: P of a linewise register over a block that ran to its first row's end puts the lines below that row, as in nvim

## Problem

nvim parity (probed 2026-10-13 with `nvim --clean --headless -l crates/fe-vim/tests/support/oracle.lua`,
nvim 0.12.6, the oracle's settings, one nvim per case). `P` with a **linewise** register over a
**blockwise** selection puts the lines above the block's first row, except when the block ran to that
row's end from a column other than 0: then the delete leaves nothing on the first row at or after the
block's left column, nvim's caret lands left of where the delete started, and its visual put turns
forward (`nv_put`: `if (VIsual_mode != 'V' && curwin->w_cursor.col < curbuf->b_op_start.col) dir =
FORWARD;`), so the lines go **below** the first row. fe-vim always puts them above.

On `"ab cd ef\ngh ij kl\nmn op qr"` at (0,0):

- `yyjl<C-v>j$P`: nvim `"ab cd ef\ng\nab cd ef\nm"`, cursor (2,0), `'<` (2,0) `'>` (2,7); fe-vim
  `"ab cd ef\nab cd ef\ng\nm"`, cursor (1,0). The same with `j6l` for `j$`, with `2P`, with a one-row
  `<C-v>$P`, from the row end (`j$<C-v>jP`, `G$<C-v>kP`, `G$<C-v>P` on the last row).
- Not affected (fe-vim already matches nvim): a block from column 0 (`yyj<C-v>j$P` empties the row, the
  caret stays at the start, lines go above), a block with text left after it (`yyjl<C-v>jlP`), `p`
  (below the cursor's row), charwise and blockwise registers, `v` and `V` selections.

The branch is in `crates/fe-vim/src/visual/put.rs` `put`, the `(None, kind) if value.linewise` arm:
`_ if args.before => (emptied.row_start(first), repeated.clone(), 0)`.

Found by a sweep of visual puts (5 register kinds x 9 selections x `p P 2p`, 270 cases); these were its
only diffs.

## Change

In that arm of `crates/fe-vim/src/visual/put.rs`, for a blockwise selection with `P`: when `at` (the
block's start on its first row, in the text the delete leaves) is past that row's start and at or past
its end, put the lines below the first row, as the `p` arm does but after row `first`
(`(row_end(&emptied, first), format!("\n{body}"), 1)`); otherwise keep putting them above. The caret,
`'<` and `'>` then follow from the existing code (first non-blank of the first put row; the marks on the
put lines). Update `put`'s doc comment ("over a block below it (`P`: above)") to state the exception. A
prototype of exactly this (6 lines) passed the tests below and the whole `cargo nextest run -p fe-vim`
with the corpus unchanged.

Tests go in the new `crates/fe-vim/tests/visual_block_put_lines.rs`, through `fe_vim::HeadlessVim`;
every value was probed in nvim. Use this file as written (the first test fails at the base on its first
check; the second passes at the base):

```rust
//! `P` of a linewise register over a blockwise selection puts the lines
//! below the block's first row, not above it, when the delete leaves nothing
//! on that row at or after the block's left column (the block ran to the
//! row's end, and that column isn't 0): nvim's caret then lands left of
//! where the delete started, and its visual put turns forward. Each
//! expectation was probed with `nvim --clean --headless` (0.12.6, the
//! oracle's settings).

use fe_vim::HeadlessVim;

const ROWS: &str = "ab cd ef\ngh ij kl\nmn op qr";

/// Feeds `keys` to a vim on `buffer` at (0, 0) and checks the text, the
/// cursor and the `'<` and `'>` marks.
fn check(
    buffer: &str,
    keys: &str,
    text: &str,
    cursor: (u32, usize),
    marks: ((u32, usize), (u32, usize)),
) {
    let mut vim = HeadlessVim::new(buffer, (0, 0));
    vim.feed(keys).unwrap();
    assert_eq!(
        (vim.text(), vim.cursor(), vim.mark('<'), vim.mark('>')),
        (text.to_string(), cursor, Some(marks.0), Some(marks.1)),
        "{keys:?} on {buffer:?}"
    );
}

#[test]
fn lines_put_before_a_block_reaching_its_rows_end_go_below_its_first_row() {
    let below = "ab cd ef\ng\nab cd ef\nm";
    check(ROWS, "yyjl<C-v>j$P", below, (2, 0), ((2, 0), (2, 7)));
    check(ROWS, "yyjl<C-v>j6lP", below, (2, 0), ((2, 0), (2, 7)));
    check(
        ROWS,
        "yyjl<C-v>j$2P",
        "ab cd ef\ng\nab cd ef\nab cd ef\nm",
        (2, 0),
        ((2, 0), (3, 7)),
    );
    check(
        ROWS,
        "yyjl<C-v>$P",
        "ab cd ef\ng\nab cd ef\nmn op qr",
        (2, 0),
        ((2, 0), (2, 7)),
    );
    check(
        ROWS,
        "yyj$<C-v>jP",
        "ab cd ef\ngh ij k\nab cd ef\nmn op q",
        (2, 0),
        ((2, 0), (2, 7)),
    );
    check(
        ROWS,
        "yyG$<C-v>kP",
        "ab cd ef\ngh ij k\nab cd ef\nmn op q",
        (2, 0),
        ((2, 0), (2, 7)),
    );
    check(
        ROWS,
        "yyG$<C-v>P",
        "ab cd ef\ngh ij kl\nmn op q\nab cd ef",
        (3, 0),
        ((3, 0), (3, 7)),
    );
    check(
        "ab\ncd\nxy",
        "yyjl<C-v>jP",
        "ab\nc\nab\nx",
        (2, 0),
        ((2, 0), (2, 1)),
    );
}

#[test]
fn lines_put_before_other_blocks_still_go_above() {
    // From column 0 the emptied row keeps the caret at the block's start.
    check(
        ROWS,
        "yyj<C-v>j$P",
        "ab cd ef\nab cd ef\n\n",
        (1, 0),
        ((1, 0), (1, 7)),
    );
    // Text left after the block keeps it there too.
    check(
        ROWS,
        "yyjl<C-v>jlP",
        "ab cd ef\nab cd ef\ngij kl\nmop qr",
        (1, 0),
        ((1, 0), (1, 7)),
    );
    // `p` goes below the cursor's row, as before.
    check(
        ROWS,
        "yyjl<C-v>j$p",
        "ab cd ef\ng\nm\nab cd ef",
        (3, 0),
        ((3, 0), (3, 7)),
    );
}
```
