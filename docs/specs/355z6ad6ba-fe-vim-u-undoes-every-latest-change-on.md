---
task_uid: "01m58zfwsecxbn3m355z6ad6ba"
task: "T213"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test line_undo_matches_neovim (top level, in crates/fe-vim/tests/line_undo_oracle.rs) passes on crates/fe-vim/tests/corpus/line_undo.toml, at least 12 cases recorded with nvim 0.12.x, among them (asserted by name) U_after_x, U_after_several, UU_redoes, U_then_u, U_after_insert, U_after_other_row_change, U_with_nothing_fails, U_after_count_x."
    required: true
    manual: false
---

# fe-vim: U undoes every latest change on the last changed line, and U again redoes them, as nvim does (Mac T904)

## Problem

At Mac main 6f032b12 fe-vim has no `U` (undo all the latest changes on one line, `:help U`): it cancels.
IdeaVim has it. The user approved it in the "Vim parity" track on 2026-10-17. fastedit's undo history is
linear per origin (`crates/fe-buffer/src/buffer.rs`: "A new edit from an origin discards that origin's
redos"), so nvim's undo-tree `g-`/`g+` aren't carded; `U` doesn't need the tree.

## Change

New feature module `crates/fe-vim/src/feature/line_undo.rs`, declared in `crates/fe-vim/src/feature.rs` and
listed in `FEATURES`, its `key` hook taking `U` in normal mode:

- `VimState` (`crates/fe-vim/src/state.rs`) keeps the "U line": the row (anchored) of the latest change and
  its text from before the first of the run of changes on it; a change on another row (or one spanning rows:
  probe) starts a new run.
- `U` replaces that row's text with the kept text as a new change (so `u` undoes it), keeping the text it
  replaced so a second `U` puts it back, as nvim's does; the cursor goes where nvim's goes (probe). With no
  U line, or its row deleted, it fails as nvim's does (probe).
- `.` doesn't repeat `U` (probe what nvim's `.` does after `U`).

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/line_undo.toml` (changes on one row
then `U`, `UU`, `Uu`, a change on another row in between, an insert session, `x` with a count), recorded into
`line_undo.expected.json` by the ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a new
`crates/fe-vim/tests/line_undo_oracle.rs` modelled on `tests/line_motions_oracle.rs`.

## Implementation notes

- The U line is kept by a buffer subscription started and drained in `marks::follow` (before each command) and in `U` itself; the batches' edit shapes tell nvim's clears (an opened/split row, whole rows deleted) from what keeps it (`J`, `p`). Only a batch's first edit sets the line (nvim's `:s` over a range leaves its first row).
- The caret column is the transaction's recorded caret if on the row, else 0; with none recorded (insert sessions) the edit's start.
- Probed against nvim 0.12.4: the harness loads a buffer as a change on row 0 that nvim keeps as a U line, so the corpus changes rows from 1 on and clears the line (`oracle_keys`) in the "nothing" cases.
- `U` was the example of an unbound key in two reducer tests; they use `K` now.
