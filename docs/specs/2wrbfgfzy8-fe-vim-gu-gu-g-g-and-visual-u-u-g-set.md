---
task_uid: "01m4tf6k3d39872n2wrbfgfzy8"
task: "T648"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test case_operators_set_change_marks_over_what_they_cover (tests/case_operator_marks.rs, as given in the spec) passes; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test a_case_operator_that_changes_nothing_keeps_the_dot_mark passes: a case operator that changes nothing leaves '. where it was"
    required: true
    manual: false
  - id: "C3"
    text: "g_operators::apply keeps its signature, crates/fe-vim/src/visual/block.rs is untouched, and the known limits (blockwise, gq/gw, '. over changed rows) are in g_operators.rs's module doc"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-vim passes with every tests/corpus/*.expected.json byte-identical"
    required: true
    manual: false
  - id: "C5"
    text: "Review F1: charwise visual recasing (visual/operate.rs) clamps the start mark to its row as well as the end, with a test in tests/case_operator_marks.rs for a selection that starts on a line break"
    required: true
    manual: false
---

# fe-vim: gu gU g~ g? and visual u U ~ g? set '[ and '] to what they cover, even when nothing changes, as in nvim

## Problem

nvim parity (probed 2026-10-13 with `nvim --clean --headless -l crates/fe-vim/tests/support/oracle.lua`,
nvim 0.12.6, the oracle's own settings, reading the marks back with `` `[ma`]mb ``): after `gu gU g~ g?`
over a motion, text object or rows, and after visual `u U ~ g?`, nvim sets `'[` and `']` to what the
operator **covers**, whether or not it changed a character:

- charwise: `'[` on the first character covered, `']` on the last (an exclusive motion's end is not
  covered: `gUw` from (0,0) on `abc DEF 12` gives (0,0)-(0,3), `gU0` from (0,4) gives (0,0)-(0,3));
- over rows (`gUU`, `gUj`, `gUip`, `3gUU`, `V…gU`): `'[` at the first row's column 0, `']` on the last
  row's last byte (`gUU` on `abc DEF 12` gives (0,0)-(0,9));
- visual charwise: the selection's ends (clamped to the row).

fe-vim instead derives them from the edits the batch made (`marks.rs`, `Changes` reading batches), so
they span only the characters that changed: `gUU` on `abc DEF 12` gives (0,0)-(0,2), `guu` gives
(0,4)-(0,6), `gUip` ends on the last lowercase letter. When nothing changes (`gUiw` on `DEF`, `veu` on
`abc`, `gUiwj.`), the only batches are the hidden undo step `g_operators::apply` makes (a space put in at
the text's end and taken out), so `'[`, `']` **and `'.`** jump to the end of the buffer: with
`TEXT = "abc DEF 12\n  x Y }\n}\n\nend."`, `jxk0guiw` leaves `'.` at (4,4) where nvim keeps it at (1,0),
where the `x` was.

Code: `g_operators::operate` (normal mode, from `operator.rs`'s `g_operators::operate` call),
`visual/operate.rs` `recase` (visual `u U ~ g?`), both applying through `g_operators::apply`.

## Change

- `crates/fe-vim/src/marks.rs`: add `pub(crate) fn recased(state, model, version: Option<Version>,
  start: (u32, usize), end: (u32, usize))`, modelled on `marks::shifted`: with the batch's version it
  notes a new `Note::Recased { start, end }`, which sets `'[` to `start` and `']` to `end` (`'.` as the
  batch already says); with `None` (nothing changed) it catches the marks up and sets `'[` and `']`
  itself, leaving `'.` alone. Add `Note::Unchanged` too: a batch noted with it sets no mark. Positions
  are (row, byte column) in the text after the change; a recased character that grew (`ß` to `ẞ`) keeps
  the byte column, as the caret does.
- `crates/fe-vim/src/g_operators.rs`: keep `apply`'s signature (`visual/block.rs`, held by T641, calls
  it) and add a sibling that both callers below use, which notes the no-change undo step's two batches
  `Note::Unchanged` and reports whether it changed anything. In `operate`, after applying, call
  `marks::recased` with the covered span: for `Span::Chars(range)`, the first character and the last
  character before `range.end`; for `Span::Lines { rows, .. }`, (first row, 0) and (last row, its last
  byte). With several carets, the first span's start and the last span's end.
- `crates/fe-vim/src/visual/operate.rs` `recase`: the same, for the case operators only (not `r`), with
  the selection's ends, or its rows for `V`.
- Document the rule in `g_operators.rs`'s module doc.

Out of scope (note them in that doc as known limits): blockwise `u U ~ g?`, which go through
`visual/block.rs` (held by T641; nvim sets `'[` at the block's top-left and `']` at its bottom-right);
`gq`/`gw`; `'.` after a case operator over rows that does change text (nvim puts it at the first row's
column 0, fe-vim at the first changed character); the marks after `u`.

Tests go in the new `crates/fe-vim/tests/case_operator_marks.rs`, through `fe_vim::HeadlessVim`; every
value was probed in nvim. Use this file as written (both tests fail at the base):

```rust
//! `gu gU g~ g?`, and visual `u U ~ g?`, set `'[` and `']` to what they
//! cover, as nvim does: the first and last character of a charwise span,
//! or the first row's column 0 and the last row's last byte over rows,
//! whether or not they change anything. One that changes nothing leaves `'.`
//! where it was. Each expectation was probed with `nvim --clean --headless`
//! (0.12.6).

use fe_vim::HeadlessVim;

const TEXT: &str = "abc DEF 12\n  x Y }\n}\n\nend.";

/// Feeds `keys` to a vim on [`TEXT`] at `cursor` and checks `'[` and `']`.
fn check(cursor: (u32, usize), keys: &str, start: (u32, usize), end: (u32, usize)) {
    let mut vim = HeadlessVim::new(TEXT, cursor);
    vim.feed(keys).unwrap();
    assert_eq!(
        (vim.mark('['), vim.mark(']')),
        (Some(start), Some(end)),
        "{keys:?} from {cursor:?}"
    );
}

#[test]
fn case_operators_set_change_marks_over_what_they_cover() {
    // Over rows: the first row's column 0 to the last row's last byte.
    check((0, 0), "gUU", (0, 0), (0, 9));
    check((0, 4), "guu", (0, 0), (0, 9));
    check((0, 0), "g~~", (0, 0), (0, 9));
    check((0, 0), "g??", (0, 0), (0, 9));
    check((0, 0), "gUj", (0, 0), (1, 6));
    check((1, 2), "gUj", (1, 0), (2, 0));
    check((1, 2), "gUk", (0, 0), (1, 6));
    check((0, 0), "gUip", (0, 0), (2, 0));
    check((0, 0), "gUap", (0, 0), (3, 0));
    check((1, 2), "3gUU", (1, 0), (3, 0));
    // Charwise: the first and the last character covered.
    check((0, 0), "gUw", (0, 0), (0, 3));
    check((0, 0), "gUaw", (0, 0), (0, 3));
    check((0, 4), "gU$", (0, 4), (0, 9));
    check((0, 4), "gU0", (0, 0), (0, 3));
    check((1, 2), "gUb", (0, 8), (1, 1));
    check((1, 2), "gUe", (1, 2), (1, 4));
    // Changing nothing ("DEF" is already upper case, "abc" lower).
    check((0, 4), "gUiw", (0, 4), (0, 6));
    check((0, 4), "gUw", (0, 4), (0, 7));
    check((0, 4), "gUiW", (0, 4), (0, 6));
    check((0, 0), "guiw", (0, 0), (0, 2));
    check((1, 2), "guiw", (1, 2), (1, 2));
    check((0, 4), "gUiwj.", (1, 4), (1, 4));
    // Visual: the selection's ends, or its rows.
    check((0, 0), "vjgU", (0, 0), (1, 0));
    check((0, 0), "vjU", (0, 0), (1, 0));
    check((0, 4), "vjU", (0, 4), (1, 4));
    check((1, 2), "vjg?", (1, 2), (2, 0));
    check((0, 0), "veu", (0, 0), (0, 2));
    check((0, 0), "VjgU", (0, 0), (1, 6));
    check((0, 4), "Vj~", (0, 0), (1, 6));
    check((1, 2), "Vj~", (1, 0), (2, 0));
}

#[test]
fn a_case_operator_that_changes_nothing_keeps_the_dot_mark() {
    // `x` on the second row sets `'.` there; the case operator after it
    // changes nothing, so `'.` stays.
    for keys in ["jxk0guiw", "jxk0wgUiw", "jxk0wgUw", "jxk$gUb", "jxk0vegu"] {
        let mut vim = HeadlessVim::new(TEXT, (0, 0));
        vim.feed(keys).unwrap();
        assert_eq!(vim.mark('.'), Some((1, 0)), "{keys:?}");
    }
}
```
