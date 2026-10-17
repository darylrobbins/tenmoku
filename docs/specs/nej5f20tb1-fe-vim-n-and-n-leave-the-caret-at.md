---
task_uid: "01m54zqekce3ygcfnej5f20tb1"
task: "T147"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test fe-vim::shift_column_caret shift_to_a_column_leaves_the_caret_there_as_nvim_does (tests/shift_column_caret.rs, top level) passes with every case in the spec, as nvim --clean does (ts=sw=4, et): e.g. \"abcdefghij\" at (0,8) with \u003e5| gives \"    abcdefghij\" at (0,4); at (0,2) \u003e20| gives (0,13); \"  abcdefgh\" at (0,7) \u003c3| gives \"abcdefgh\" at (0,2); \"\\tab\\tcd\" at (0,0) \u003e6| gives (0,5); 2\u003e3| gives (0,5); \u003e20|j onto a longer row gives (1,13); it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Unchanged: the caret after \u003e or \u003c with any other target (\u003ew, \u003ej, \u003e$, \u003e\u003e, a visual \u003e) is where the base puts it; u after \u003e{N}| restores the text, and . repeats it with the caret where nvim leaves it"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: >{N}| and <{N}| leave the caret at display column N-1 of the shifted row, clamped to it, as nvim does, instead of where it was (Mac T777)

## Problem

At Mac main 9b406468 the caret after `>{N}|` or `<{N}|` stays where it was. `crates/fe-vim/src/operator.rs`
(the `Operator::Indent | Operator::Outdent` block of the operator dispatch, "The caret goes to the first row at
its goal column") puts each caret at the first shifted row's *held* goal column: `(Some(rows), _) =>
(*rows.start(), *goal)`. Only `$` has its own arm. nvim's `|` sets curswant to N-1 (`nv_pipe`), and
`op_shift` then puts the cursor on the first row at curswant (`beginline(BL_SOL | BL_FIX)` with
'nostartofline'), so the caret lands at display column N-1 of the shifted row, clamped to the row. The goal
afterwards is that landed column, not N-1 (`>20|j` stays at the column it landed on).

nvim --clean --headless 0.12.6 (ts=sw=4, et, as the oracle corpus sets it; checked 2026-10-17), rows from 0, byte
columns, fe-vim at the base in brackets where it differs:

- `abcdefghij` (0,0) `>5|` gives `    abcdefghij` (0,4) [(0,0)]; from (0,8) also (0,4) [(0,8)]
- `abcdefghij` (0,2) `>20|` gives (0,13), clamped to the row [(0,2)]
- `abc` (0,0) `>9|` gives `    abc` (0,6) [(0,0)]
- `  abcdefgh` (0,0) `>6|` gives `      abcdefgh` (0,5) [(0,0)]
- `  abcdefgh` (0,7) `<3|` gives `abcdefgh` (0,2) [(0,7)]
- `abc / def` (0,1) `>3|` gives `    abc / def` (0,2) [(0,1)]
- `\tab\tcd` (0,0) `>6|` gives `        ab\tcd` (0,5) [(0,3)]
- `abcdefghij` (0,3) `>|` and `>1|` give (0,0) [(0,3)]; `>4|` gives (0,3) (same at the base)
- `abcdefghij` (0,6) `>3|` gives (0,2) [(0,6)]; `<3|`, which shifts nothing, also (0,2) [(0,6)]
- `abcdefghij / abcdefghij` (0,2) `2>3|` gives (0,5): the counts multiply
- `abcdefghij / abcdefghijklmnopqrstuvwxyz` (0,2) `>20|j` gives (1,13); `abc def ghi / xyz` (0,2) `>2|j` gives (1,1)
- unchanged: `abc def ghi` (0,1) `>w` gives (0,1); `>3|u` restores the text at (0,2); `>3|.` from (0,2) gives
  `        abcdefghij` at (0,2)

## Change

In `crates/fe-vim/src/operator.rs`, in the Indent/Outdent block's `carets` map, add an arm before
`(Some(rows), _) => (*rows.start(), *goal)`: when `args.target == Target::Motion(Motion::Column)`, the goal is
`args.count.unwrap_or(1).max(1) as usize - 1` (the column `|` asks for; `on_row` clamps it to the row). Nothing
else changes: `state.goals` stays cleared, so the next `j` or `k` aims at the landed column, as nvim's does. A
5-line prototype of exactly this passed every case above and all of `cargo test -p fe-vim` at 9b406468.

New integration test `crates/fe-vim/tests/shift_column_caret.rs`, top-level fn
`shift_to_a_column_leaves_the_caret_there_as_nvim_does`, through `HeadlessVim` (buffers end in `\n`), with
every case above.
