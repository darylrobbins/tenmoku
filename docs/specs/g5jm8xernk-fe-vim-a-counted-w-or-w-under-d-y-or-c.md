---
task_uid: "01m54wt5m5azs73gg5jm8xernk"
task: "T774"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test fe-vim::word_count_empty_last_row counted_word_running_out_stops_before_the_empty_last_row (new crates/fe-vim/tests/word_count_empty_last_row.rs, top level) passes, as nvim --clean --headless does: d2w on \"foo bar\\n\" from (0,4) leaves \"foo \\n\" at (0,3) with \" = - = bar charwise and no 1; d3w on \"foo bar\\n\" from (0,0) yanks \"foo bar\\n\" linewise; d2w on \"foo\\n\\n\" from (1,0) leaves \"foo\\n\" at (1,0) with \"\\n\" linewise; plus every other row of the spec's table"
    required: true
    manual: false
  - id: "C2"
    text: "Unchanged, in the same test: dw on \"foo\\n\" from (0,0) leaves \"\\n\" (foo charwise); d2w on \"foo bar\\n\" from (0,0) leaves \"\\n\" (foo bar charwise); d2w on \"foo\\n\\n\" from (0,0) leaves \"\" (foo\\n\\n linewise)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# fe-vim: a counted w or W under d, y or c that runs out onto the text's empty last row stops at the end of the row before, as nvim does

## Problem

When a counted `w` or `W` under an operator (`d`, `y`, `c`) runs out on a text that ends in a line break
(so its last row is empty), fe-vim takes that empty last row as well. It deletes or yanks the line break
before it, and for a linewise range it takes one row too many. nvim stops at the end of the row before
the empty last row. It treats the run-out as an exclusive end at column 0 of the last row (`:help
exclusive`): the end moves back to the end of the row before, and when the start is at or before its row's
first non-blank, the rows are taken whole. A charwise delete within one row then writes `-`, not `1`.

The cause is `motion_span` in `crates/fe-vim/src/operator.rs` (the `RangeClass::Exclusive` arm for
`Motion::WordForward | Motion::BigWordForward`, lines 274-296). With `cursor.hit_end` set and the target at
the start of the empty last row, it uses `cursor.word_row`, which is that empty row, and takes rows through
it (`rows: row(head)..=word_row`) or bytes up to its end (`Span::Chars(head..row_end(text, word_row))`).

Reproduced at Mac main 0e02c820 with `HeadlessVim` against `nvim --clean --headless` 0.12.6, using the
oracle's settings (`tests/support/oracle.lua`):

| buffer | cursor | keys | nvim: text, cursor, `"` | fe-vim: text, cursor, `"` |
|---|---|---|---|---|
| `foo bar\n` | (0,4) | `d2w` | `foo \n`, (0,3), `bar` charwise (and `-`) | `foo `, (0,3), `bar\n` (and `1`) |
| `foo bar\n` | (0,4) | `y2w` | unchanged, (0,4), `bar` | unchanged, (0,4), `bar\n` |
| `x foo\n` | (0,2) | `d9w` | `x \n`, (0,1), `foo` | `x `, (0,1), `foo\n` |
| `foo bar\n` | (0,0) | `d3w` | ``, (0,0), `foo bar\n` linewise | ``, (0,0), `foo bar\n\n` linewise |
| `foo\n` | (0,0) | `d2W` | ``, (0,0), `foo\n` linewise | ``, (0,0), `foo\n\n` linewise |
| `foo\n\n` | (0,0) | `d9w` | ``, (0,0), `foo\n\n` linewise | ``, (0,0), `foo\n\n\n` linewise |
| `foo\n\n` | (1,0) | `d2w` | `foo\n`, (1,0), `\n` linewise | `foo`, (0,0), `\n\n` linewise |
| `foo\n  \n` | (0,2) | `d2w` | `fo\n`, (0,1), `o\n  ` charwise | `fo`, (0,1), `o\n  \n` charwise |
| `foo\n  \n` | (1,1) | `d2w` | `foo\n`, (1,0), `  \n` linewise | `foo`, (0,0), `  \n\n` linewise |
| `  foo\n` | (0,0) | `c9wX<Esc>` | `  X\n`, (0,2), `  foo\n` linewise | `  X`, (0,2), `  foo\n\n` linewise |
| `foo\n\n` | (1,0) | `c2wX<Esc>` | `foo\nX\n`, (1,0), `\n` linewise | `foo\nX`, (1,0), `\n\n` linewise |

These already match nvim and must stay so. In each, the count doesn't run out:
- `dw` on `foo\n` from (0,0) gives `\n`, with `foo` charwise.
- `d2w` on `foo bar\n` from (0,0) gives `\n`, with `foo bar` charwise.
- `d2w` on `foo\n\n` from (0,0) gives ``, with `foo\n\n` linewise.

A broader probe compared 1,950 cases against nvim: `d`, `y` and `c` with `w`/`W` and counts 1, 2, 3, 4
and 9, from each word start and row end of 14 buffers ending in `\n`. 756 of them differed at base. A
prototype of the change below (one extra condition on the arm's guard) cleared all 756, and
`cargo test -p fe-vim` passed unchanged.

## Change

In `crates/fe-vim/src/operator.rs` `motion_span`, the `w`/`W` arm no longer takes a run-out that ends at
the start of the text's empty last row, below the head's row. That is the case where `cursor.hit_end` is
set, `target == text.len()`, `row(target) > row(head)` and `text.row_start(row(target)) == target`. Such a
target falls through to the next arm, the `:help exclusive` rule for an end at column 0 of a later row,
which already gives nvim's rows or bytes. The arm's other cases (not run out, stuck on an empty last row,
ending on an earlier empty row) stay as they are. Update the arm's comment to say why the run-out onto the
empty last row goes to the exclusive rule.

Regression test: a new integration test `crates/fe-vim/tests/word_count_empty_last_row.rs`, with the
function `counted_word_running_out_stops_before_the_empty_last_row` at the top level of the file. It drives
`fe_vim::HeadlessVim` through the table's rows plus the three unchanged ones, and checks the text, the
cursor, and the `"` register's text and linewise flag. For the first row it also checks that `-` is set and
`1` is not. Probe any row added beyond these with `nvim --clean --headless` first.
