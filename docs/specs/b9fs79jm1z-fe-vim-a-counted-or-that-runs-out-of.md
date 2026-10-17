---
task_uid: "01m54a6595z87va5b9fs79jm1z"
task: "T127"
rev: 2
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test counted_paragraph_motions_that_run_out_set_the_goal_column_to_the_caret (tests/paragraph_run_out_goal.rs, top level) passes: a counted } or { that runs out leaves the caret where it is and the next j or k uses the caret's column, as nvim --clean 0.12.4 does (\"abcdef\\nxy\" at (0,4), j3}k ends at (0,1); \"abcdef\\nxyz\" at (0,4), j$3}k at (0,2); \"abcdef\\n日本\" at (0,4), j3}k at (0,2); visual \"abcdef\\nxy\\n\\nabcd\" at (0,4), vj9}k at (0,2); jd3}k on \"abcdef\\nxy\" at (0,1)); it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "The run-out { or } still fails as an error does (a macro stops there) and leaves the caret where it was; ( and ) and counts that don't run out are unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: a counted { or } that runs out of paragraphs sets the goal column to the caret's, as nvim does, instead of keeping the old one (Mac T747)

## Problem

A counted `}` or `{` that runs out of paragraphs leaves the caret where it was in both fe-vim and nvim,
but nvim also resets the goal column (curswant) to the caret's own column, so the next `j`/`k` lands
under the caret. fe-vim keeps the goal column from before, so the next vertical move jumps back out to
the old column.

`crates/fe-vim/src/prose.rs` `motion()` (line 140 at Mac c092189b):
`let row = paragraph(text, cursor.row, count, forward).ok_or(goal)?;` returns `Err(goal)`, which keeps
the old goal (`motion::apply` and `visual::move_cursor` keep whatever an `Err` carries).

Reproduced at Mac c092189b with `HeadlessVim`, against `nvim --clean --headless` (0.12.4 on neon, the
oracle's pinned version, and 0.12.6 on the Mac agree). Buffer rows joined by `\n`, cursor (row, byte
column), then the keys:

| buffer | cursor | keys | nvim cursor | fe-vim cursor |
|---|---|---|---|---|
| `abcdef\nxy` | (0,4) | `j3}k` | (0,1) | (0,4) |
| `xy\nabcdef` | (1,4) | `k3{j` | (1,1) | (1,4) |
| `abcdef\nab\n\ncd\n\nabcdef` | (0,4) | `j9}k` | (0,1) | (0,4) |
| `abcdef\nxyz` | (0,4) | `j$3}k` | (0,2) | (0,5) |
| `abcdef\n日本` | (0,4) | `j3}k` | (0,2) | (0,4) |
| `abcdef\nxy\n\nabcd` | (0,4) | `vj9}k` | (0,2), visual | (0,4), visual |
| `xy\nabcdef` | (1,4) | `vk3{j` | (1,2), visual | (1,4), visual |
| `abcdef\nxy` | (0,4) | `jd3}k` | (0,1) | (0,4) |

In each, nvim leaves the caret where the failed `{`/`}` found it (`j3}` alone stays at (1,1); a visual
`vj` onto `xy` is at (1,2), past the last char, and `9}` leaves it there) and the following vertical move
uses that column. A count that doesn't run out (`j2}k` on `abcdef\nxy\n\nzw`) already matches. A counted
`}` that runs out still fails as an error does (it stops a macro: nvim's `@q` with `j3}kx` stops before
`k`), as fe-vim's `Err` already makes it.

## Change

In `prose.rs` `motion()`, a paragraph motion that runs out returns `Err(None)` instead of `Err(goal)`:
the caret stays put, the motion still fails (`errs` still reports it, so a macro still stops), and the
goal column is the caret's own, in normal, operator-pending and visual modes alike (`visual::move_cursor`
already turns an `Err(None)` into the cursor's column). Sentence motions (`(`, `)`) are unchanged: their
run-out rule differs in nvim and isn't part of this card.

Regression test: a new integration test `crates/fe-vim/tests/paragraph_run_out_goal.rs`, at the top level
of the file, `counted_paragraph_motions_that_run_out_set_the_goal_column_to_the_caret`, driving
`fe_vim::HeadlessVim` through the table's rows and asserting the cursor and mode nvim gives (probe any
added row with `nvim --clean --headless` first).
## Rev 2 (neon lead, 2026-10-17): operator.rs in scope

C1's operator row (`jd3}k` on `abcdef`/`xy` from (0,4) ends at (0,1)) needs the operator path to use the run-out
motion's goal too: `operator::motion_span` discards the `Err` payload and `operator::move_only` keeps the old goal.
`crates/fe-vim/src/operator.rs` is now in scope for that small change (the worker's verified ~10-line fix), as review
round 1 asked. Nothing else changes.
