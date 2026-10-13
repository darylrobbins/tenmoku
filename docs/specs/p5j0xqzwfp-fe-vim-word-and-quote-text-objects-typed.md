---
task_uid: "01m4tj94x9991v2sp5j0xqzwfp"
task: "T650"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test word_and_quote_objects_keep_a_block (tests/visual_block_objects.rs, as given in the spec) passes; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test bracket_tag_sentence_and_paragraph_objects_leave_the_block passes: i( it is from \u003cC-v\u003e leave a charwise selection and ip a linewise one"
    required: true
    manual: false
  - id: "C3"
    text: "Only crates/fe-vim/src/visual/object.rs changes under src; select_object's doc comment states which objects keep a block; cargo nextest run -p fe-vim passes with every tests/corpus/*.expected.json byte-identical"
    required: true
    manual: false
---

# fe-vim: word and quote text objects typed in blockwise visual keep the block, as in nvim

## Problem

nvim parity (probed 2026-10-13 with `nvim --clean --headless -l crates/fe-vim/tests/support/oracle.lua`,
nvim 0.12.6, the oracle's settings, one nvim per case). In blockwise visual mode, nvim's word objects
(`iw aw iW aW`) and quote objects (`i" a" i' a' i` a``) keep the selection **blockwise**: only `V` is
turned charwise by them (`current_word` / `current_quote` reset `VIsual_mode` only when it is `'V'`).
Bracket, tag and sentence objects (`i( a( i{ i[ i< it is as` …) make it charwise and `ip ap` linewise, as
fe-vim already does. fe-vim's `select_object` (`crates/fe-vim/src/visual/object.rs`) starts every object
at `let mut kind = VisualKind::Char;`, so from `<C-v>` a word or quote object always leaves a charwise
selection:

- `"ab cd\nef gh"` at (0,0), `<C-v>iw`: nvim stays `visual_block`; fe-vim goes `visual`.
- `<C-v>jiwd` there: nvim deletes the block (`" cd\n gh"`, `"`=`"ab\nef"` blockwise width 2); fe-vim
  deletes `"ab cd\nef"` charwise, leaving `" gh"`.
- `" x (a) \"q\"\n0x1f"` at (0,6), `<C-v>iwD`: nvim deletes `' "q"'` blockwise to the row end; fe-vim, now
  charwise, deletes the whole row.
- `<C-v>i"d`, `<C-v>a"y`, `<C-v>a`d`, `<C-v>2iwd`, `<C-v>aWy`: same text as nvim, but the register is
  charwise where nvim's is blockwise (`getregtype()` `^V{width}`).

Found by a differential fuzz of visual-mode commands against nvim (9 of its 64 diffs had this root cause:
`<C-v>…iw/aw` followed by `x D d s > rz p`).

## Change

In `crates/fe-vim/src/visual/object.rs` `select_object`, after the object's ends are found: when the
selection was blockwise (`ends.kind == VisualKind::Block`) and the object is a word object
(`InnerWord AWord InnerBigWord ABigWord`) or a quote object (`InnerDoubleQuote ADoubleQuote
InnerSingleQuote ASingleQuote InnerBacktick ABacktick`), the new selection keeps `VisualKind::Block`.
The anchor and cursor positions are unchanged (they are already nvim's); only the kind changes. Other
objects, and `v`/`V` selections, are untouched. Update the doc comment on `select_object` ("The mode
becomes charwise") to state the rule. A prototype of exactly this (8 lines) passed the tests below and
the whole `cargo nextest run -p fe-vim` with the corpus unchanged.

`crates/fe-vim/src/delimiters.rs` (held by T564) is not touched: `delimiters::select_visual` only finds
the backtick objects' ends, and their kind is set in `object.rs` like the others'.

Tests go in the new `crates/fe-vim/tests/visual_block_objects.rs`, through `fe_vim::HeadlessVim`; every
value was probed in nvim. Use this file as written (the first test fails at the base on its first
check; the second passes at the base and guards the objects that must still leave the block):

```rust
//! A word or quote text object typed in blockwise visual mode keeps the
//! block, as in nvim: `<C-v>jiw` selects a block over both rows' words, and
//! an operator then takes a block. Bracket, tag and sentence objects still
//! make the selection charwise, and paragraphs linewise. Each expectation
//! was probed with `nvim --clean --headless` (0.12.6, the oracle's settings).

use fe_vim::HeadlessVim;

/// Feeds `keys` to a vim on `buffer` at `cursor` and checks the text, the
/// cursor, the mode and the unnamed register as (text, linewise, block
/// width), `None` when it's empty.
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
```
