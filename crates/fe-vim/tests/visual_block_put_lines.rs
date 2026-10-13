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
