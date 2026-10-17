---
task_uid: "01m54f2jd7zvaz7re0gfa9j79z"
task: "T131"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test yank_of_a_linewise_inner_block_leaves_the_caret_at_its_first_row_start (tests/inner_block_yank_caret.rs, top level) passes: yiB, yi{, yib, yi[ and yi\u003c over a block whose brackets end and start their rows leave the caret at column 0 of the first inner row, as nvim --clean does (\"  if (x) {\\n    y = z;\\n  }\\n\\nelse\" at (0,2) yiB: (1,0); at (1,6) yi{: (1,0); \"fn main() {\\n    let v = vec![1, 2];\\n}\\n\" at (1,21) yiBw: (1,4); \"f (\\n  a,\\n  b\\n)\" at (2,2) yib: (1,0); the same with [ ] and \u003c \u003e: (1,0)); it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Other operators on these objects and other linewise yanks are unchanged (at (1,6) on the first buffer: diB (1,2), gUiB (1,0), \u003eiB (1,6), viBy (1,0), yaB (0,9), yip (0,0), yj (1,6)), and the yanked text and registers are unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: a yank of i( i{ i[ or i< that takes whole rows leaves the caret at the first inner row's column 0, as nvim does, instead of at the old column (Mac T756)

## Problem

When `i(`/`ib`, `i{`/`iB`, `i[` or `i<` covers whole rows (the opening bracket ends its row and the closing
one has only blanks before it), the object becomes linewise, as in nvim. A **yank** of it then leaves nvim's
caret at column 0 of the first inner row (where the charwise object started), but fe-vim's at the caret's
old column on that row (clamped), as a plain linewise yank such as `yj` does.

`crates/fe-vim/src/text_object.rs` `brackets_at` (line ~330 at Mac 80e6c76d) returns
`Span::Lines { rows, first_non_blank: true, from: None }`. `operator.rs` already takes a yank's caret from
`Span::Lines { from: Some(offset) }` ("where a charwise selection started, for a yank's caret"); the inner
bracket span doesn't fill it in.

Reproduced at Mac 80e6c76d with `HeadlessVim` against `nvim --clean --headless` 0.12.6 (the oracle's
settings). Buffer rows joined by `\n`, caret (row, byte column); text and registers already match:

| buffer | cursor | keys | nvim caret | fe-vim caret |
|---|---|---|---|---|
| `  if (x) {\n    y = z;\n  }\n\nelse` | (0,2) | `yiB` | (1,0) | (1,2) |
| same | (0,2) | `yi{` | (1,0) | (1,2) |
| same | (1,6) | `yiB` | (1,0) | (1,6) |
| `fn main() {\n    let v = vec![1, 2];\n}\n` | (1,21) | `yiBw` | (1,4) | (2,0) |
| `f (\n  a,\n  b\n)` | (2,2) | `yib` | (1,0) | (1,2) |
| `f [\n  a,\n  b\n]` | (2,2) | `yi[` | (1,0) | (1,2) |
| `f <\n  a,\n  b\n>` | (2,2) | `yi<` | (1,0) | (1,2) |

Already matching and to stay so: `f (\n  a,\n  b\n)` at (0,0) `yib` (1,0); on the first table buffer at
(1,6): `diB` (1,2), `gUiB` (1,0), `>iB` (1,6), `viBy` (1,0), `yaB` (0,9), `yip` (0,0), `yj` (1,6).

## Change

In `brackets_at`, return `from: Some(start)` (the inner range's start, column 0 of its first row) in the
whole-rows `Span::Lines`, so a yank's caret goes there as nvim's does. Nothing else reads `from` for these
operators. A prototype of exactly this one-line change made every table row match, took a 2,500-case random
fuzz of `y d gU > g~` with counted `ib iB i( i{ i[ i< i) i} i] i>` over multi-row blocks from 19 mismatches to
0 with no new mismatch, and `cargo test -p fe-vim` passed unchanged.

This is in `text_object.rs`, which T748 (the `i"`/`i'` quote fix in `quotes()`) also changes, hence the
dependency; the two edits are in different functions.

Regression test: a new integration test `crates/fe-vim/tests/inner_block_yank_caret.rs`, at the top level of
the file, `yank_of_a_linewise_inner_block_leaves_the_caret_at_its_first_row_start`, driving
`fe_vim::HeadlessVim` through the table's rows and the unchanged ones and asserting the text, the caret and
register `"`. Probe any added row with `nvim --clean --headless` first.
