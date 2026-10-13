---
task_uid: "01m4spvr4ka0czdbr50pbxmae6"
task: "T106"
rev: 3
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "From (0,6) on long rows, ]c lands at column 0 of the change and a following j lands at column 6; the same for [c then k (crates/fe-vim/tests/bracket_c_goal.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "A goal column set before ]c (j from column 20 onto a short row, then ]c, then j onto a long row) is kept: the caret lands at column 20"
    required: true
    manual: false
  - id: "C3"
    text: "The existing bracket_c test passes unchanged: landing column 0, counts, failing with no change that way, and visual mode"
    required: true
    manual: false
---

# fe-vim: ]c and [c keep the desired column, so j and k after them return to it as in nvim (T549 review F1) (Mac T616)

## Problem

From the codex review of T549 (approved 2026-10-12, never filed), F1, `crates/fe-vim/src/changes.rs:171`:
in normal mode `]c` and `[c` move through `motion::apply` with `Motion::MarkAt`, which resets the
desired column to `None`. Starting at column 6, `]c` lands at column 0 as nvim does, but a following `j`
stays at column 0; nvim's `]c` sets the column without touching `curswant`, so `j` returns to column 6.
The visual-mode branch (`visual::jump_cursor`) already keeps the goal, so the two modes differ.

## Change

In the normal-mode branch of the `]c`/`[c` handler in `crates/fe-vim/src/changes.rs`, take the valid
desired display column before the jump (from `motion::valid_goals`, else the caret's own column) and
restore it after moving to column 0 of the change, as `scroll::jump_to_row` in `crates/fe-vim/src/scroll.rs`
does. Landing stays at column 0; counts, failure and visual mode are unchanged.

Tests go in the new `crates/fe-vim/tests/bracket_c_goal.rs`, through `fe_vim::HeadlessVim` with
`model_mut().set_diff_base(..)` as `src/tests/changes.rs` sets it up (nvim's diff-mode behaviour checked
by hand with `:diffthis`, as the existing `bracket_c` test notes).

Note: neon's blocked T493 reworks the goal representation in `motion.rs`/`state.rs`; this card uses only
the existing goal API and touches neither file.
