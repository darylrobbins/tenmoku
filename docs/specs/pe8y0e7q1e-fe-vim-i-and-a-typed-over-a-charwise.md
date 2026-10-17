---
task_uid: "01m54r2cqpbdtswxpe8y0e7q1e"
task: "T132"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test backtick_objects_over_a_charwise_selection_match_nvim (tests/visual_backtick_selection.rs, top level) passes: i` and a` over a charwise selection of more than one position select as nvim --clean does (\"a `bc` d `ef` g\" at 0 vli`: 3..4; vlla`: 0..9; at 4 vla`: 4..13; at 9 vhi`: 9..3; \"x `foo bar` y\" at 4 vli`: 3..9, vli`a`: 2..11, vlli`d leaves \"x `` y\" with the caret at 3; \"x \\`a` `b`\" at 6 vli`: 6..8); it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Unchanged: one-position i`/a` (\"x `foo bar` y\" at 5 vi`: 3..9, vi`i`: 2..10), a selection on two rows (vji` still fails), linewise and blockwise selections, the bracket and tag objects, and operator-pending i`/a`"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: i` and a` typed over a charwise selection of more than one position select the backtick string as i" and a" do, as nvim does, instead of failing (Mac T762)

## Problem

`` i` `` and `` a` `` typed in charwise visual mode over a selection of more than one position fail in fe-vim
and leave the selection as it was, in nearly every case where nvim selects or extends to a backtick string.
`crates/fe-vim/src/visual/object.rs` `select_object` sends backtick objects to
`crate::delimiters::select_visual` (the bracket-style `grow`), whose quote rule only takes the string when it
equals the selection. nvim treats `` ` `` there exactly as `"` and `'`: the rule fe-vim already has for
`i" a" i' a'` over a charwise selection of more than one position in `crates/fe-vim/src/visual/quote.rs`
`select` (T731 left backticks out on purpose).

Reproduced at Mac main 838c0b35 with `HeadlessVim` against `nvim --clean --headless` 0.12.6 (the oracle's
settings). One row, columns are bytes, anchor..cursor:

| buffer | cursor | keys | nvim | fe-vim |
|---|---|---|---|---|
| ``a `bc` d `ef` g`` | 0 | ``vli` `` | 3..4 | 0..1, fails |
| ``a `bc` d `ef` g`` | 0 | ``vlla` `` | 0..9 | 0..2, fails |
| ``a `bc` d `ef` g`` | 1 | ``vli` `` | 1..8 | 1..2, fails |
| ``a `bc` d `ef` g`` | 4 | ``vla` `` | 4..13 | 4..5, fails |
| ``a `bc` d `ef` g`` | 9 | ``vhi` `` | 9..3 | 9..8, fails |
| ``say `hi` and `bye` `` | 15 | ``vla` `` | 12..17 | 15..16, fails |
| ``say `hi` and `bye` `` | 17 | ``vha` `` | 17..12 | 17..16, fails |
| ``x `foo bar` y`` | 4 | ``vli` `` | 3..9 | 4..5, fails |
| ``x `foo bar` y`` | 4 | ``vli`a` `` | 2..11 | 4..5, fails |
| ``` `a` `b` `c` ``` | 5 | ``vli` `` | 5..9 | 5..6, fails |
| ``x \`a` `b` `` | 6 | ``vli` `` | 6..8 | 6..7, fails |
| ``x `foo bar` y`` | 4 | ``vlli`d`` | text ``x `` y``, caret 3 | ``x `fbar` y``, caret 4 |
| ``x `foo bar` y`` | 4 | ``vlla`y`` | yanks `` `foo bar` ``, caret 2 | yanks `oo`, caret 4 |

Already the same as nvim, and must stay so: ``vi` `` at 5 on ``x `foo bar` y`` (3..9), ``vi`i` `` there
(2..10), and ``vji` `` from a row onto the next (fails, as a selection on two rows does).

A prototype (about 10 lines in `object.rs`: keep `InnerBacktick | ABacktick` out of the `delimiters` branch when
the selection is charwise and longer than one position, and give them the quote branch with `` b'`' ``,
`ABacktick` counting as `include`) took a 2,500-case random fuzz of `v`/`V`/`<C-v>` plus a motion, a count and
`` i` ``/`` a` `` (with `d`, `y`, `c`) from 245 mismatches with nvim to 86, all of them in `V` and `<C-v>`
selections (out of scope: `i"` has the same gap there), with no new mismatch; `cargo test -p fe-vim` passed
unchanged.

## Change

In `crates/fe-vim/src/visual/object.rs` `select_object`: for `InnerBacktick` and `ABacktick` over a charwise
selection whose anchor and cursor differ, take the quote branch (`quote::select` with `` b'`' ``, `include`
for `ABacktick`) instead of `delimiters::select_visual`. Everything else is unchanged: one-position
selections, linewise and blockwise selections, the bracket and tag objects, and operator-pending
`` i` ``/`` a` ``.

Regression test: a new integration test `crates/fe-vim/tests/visual_backtick_selection.rs`, at the top level
of the file, `backtick_objects_over_a_charwise_selection_match_nvim`, driving `fe_vim::HeadlessVim` through the
table's rows (as `tests/visual_quote_selection.rs` does: mode, anchor, cursor, and for the `d`/`y` rows the
text, caret and unnamed register). Probe any added row with `nvim --clean --headless` first.
