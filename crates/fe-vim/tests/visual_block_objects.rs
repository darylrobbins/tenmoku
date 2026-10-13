//! A word or quote text object typed in blockwise visual mode keeps the
//! block, as in nvim: `<C-v>jiw` selects a block over both rows' words, and
//! an operator then takes a block. Bracket, tag and sentence objects still
//! make the selection charwise, and paragraphs linewise. Each expectation
//! was probed with `nvim --clean --headless` (0.12.6, the oracle's settings).

use fe_vim::HeadlessVim;

/// Feeds `keys` to a vim on `buffer` at `cursor` and checks the text, the
/// cursor, the mode and the unnamed register as (text, linewise, block
/// width), `None` when it's empty.
#[allow(clippy::type_complexity)]
fn check(
    buffer: &str,
    cursor: (u32, usize),
    keys: &str,
    want: (
        &str,
        (u32, usize),
        &str,
        Option<(&str, bool, Option<usize>)>,
    ),
) {
    let mut vim = HeadlessVim::new(buffer, cursor);
    vim.feed(keys).unwrap();
    let register = vim
        .registers()
        .get('"')
        .filter(|register| !register.text.is_empty())
        .map(|register| {
            (
                register.text.clone(),
                register.linewise,
                register.block_width,
            )
        });
    let (text, at, mode, unnamed) = want;
    assert_eq!(
        (vim.text(), vim.cursor(), vim.mode(), register),
        (
            text.to_string(),
            at,
            mode,
            unnamed.map(|(text, linewise, width)| (text.to_string(), linewise, width))
        ),
        "{keys:?} on {buffer:?} at {cursor:?}"
    );
}

#[test]
fn word_and_quote_objects_keep_a_block() {
    let rows = "ab cd\nef gh";
    check(
        rows,
        (0, 0),
        "<C-v>iw",
        (rows, (0, 1), "visual_block", None),
    );
    check(
        rows,
        (0, 0),
        "<C-v>jiwd",
        (
            " cd\n gh",
            (0, 0),
            "normal",
            Some(("ab\nef", false, Some(2))),
        ),
    );
    check(
        rows,
        (0, 0),
        "<C-v>jawd",
        (
            "cd\ngh",
            (0, 0),
            "normal",
            Some(("ab \nef ", false, Some(3))),
        ),
    );
    check(
        rows,
        (0, 0),
        "<C-v>aWy",
        (rows, (0, 0), "normal", Some(("ab ", false, Some(3)))),
    );
    check(
        rows,
        (0, 3),
        "<C-v>jbiwd",
        (
            "ad\neh",
            (0, 1),
            "normal",
            Some(("b c\nf g", false, Some(3))),
        ),
    );
    check(
        rows,
        (0, 0),
        "<C-v>jiw>",
        ("    ab cd\n    ef gh", (0, 0), "normal", None),
    );
    check(
        "ab cd ef",
        (0, 0),
        "<C-v>2iwd",
        ("cd ef", (0, 0), "normal", Some(("ab ", false, Some(3)))),
    );
    check(
        " x (a) \"q\"\n0x1f",
        (0, 6),
        "<C-v>iwD",
        (
            " x (a)\n0x1f",
            (0, 5),
            "normal",
            Some((" \"q\"", false, Some(4))),
        ),
    );
    let quoted = "x \"ab\" y\nx \"cd\" y";
    check(
        quoted,
        (0, 3),
        "<C-v>i\"d",
        (
            "x \"\" y\nx \"cd\" y",
            (0, 3),
            "normal",
            Some(("ab", false, Some(2))),
        ),
    );
    check(
        quoted,
        (0, 3),
        "<C-v>a\"y",
        (quoted, (0, 2), "normal", Some(("\"ab\" ", false, Some(5)))),
    );
    check(
        "x 'ab' y",
        (0, 3),
        "<C-v>i'",
        ("x 'ab' y", (0, 4), "visual_block", None),
    );
    check(
        "x `ab` y",
        (0, 3),
        "<C-v>a`d",
        ("x y", (0, 2), "normal", Some(("`ab` ", false, Some(5)))),
    );
}

#[test]
fn bracket_tag_sentence_and_paragraph_objects_leave_the_block() {
    check(
        "x (ab) y",
        (0, 3),
        "<C-v>i(d",
        ("x () y", (0, 3), "normal", Some(("ab", false, None))),
    );
    check(
        "<t>ab</t>",
        (0, 3),
        "<C-v>itd",
        ("<t></t>", (0, 3), "normal", Some(("ab", false, None))),
    );
    check(
        "a b. c d.",
        (0, 6),
        "<C-v>isd",
        ("a b. ", (0, 4), "normal", Some(("c d.", false, None))),
    );
    check(
        "a\nb\n\nc",
        (0, 0),
        "<C-v>ipd",
        ("\nc", (0, 0), "normal", Some(("a\nb\n", true, None))),
    );
}
