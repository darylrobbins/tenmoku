---
task_uid: "01m4rhrnzvmesjys5sdteyjcy7"
task: "T549"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test tests::changes::bracket_c: with a diff base giving changes at rows 3, 10 and 20, ']c' from row 0 lands on row 3, '2]c' on row 10, '[c' from row 15 on row 10; ']c' from row 20 fails without moving; in visual mode ']c' extends the selection to row 3"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::changes::jump_and_cancel: ']c' sets the '' mark to the start row (\"''\" returns there); ']x' and '[q' cancel and change nothing; 'd]c' cancels"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test tests::changes::do_dp: 'do' hands the host DiffGet { rows: r..r+1 } and 'dp' DiffPut for the cursor row r, the text unchanged by fe-vim; '3do' is the same; 'dw' still deletes a word"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-vim passes, oracle tests included; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: ]c and [c jump to the next and previous changed rows, and do and dp ask the host to get or put the change

## Problem

The vim-mode requirement (coordinator, 2026-10-12): in the diff view `]c`/`[c` go to the next/previous hunk (Vim's
diff-mode keys), `do`/`dp` get and put a change, and gitsigns users expect `]c`/`[c` to jump between git changes in
any editor. fe-vim takes no `[` or `]` key (no feature module answers them; `crates/fe-vim/src/feature.rs`), and `d`
followed by `o` or `p` cancels.

## Change

New feature module `crates/fe-vim/src/changes.rs` (in `FEATURES` in `crates/fe-vim/src/feature.rs`, `mod` in
`crates/fe-vim/src/lib.rs`), with its module docs saying later `[`/`]` keys (`[(`, `]]`, `[m`) join its prefix:

- `]c` / `[c` with a count, in normal and visual mode: commit `vim.change {"forward", "count"}` (`mutates_buffer`
  false), which moves the caret to the first row of the `count`-th next/previous change in `EditorModel::row_changes()`
  (a deleted-rows change: its row as `RowChange` gives it), at the column nvim's `]c` lands on in diff mode (checked by
  hand against nvim with `:diffthis` on two windows; noted in the test). No change in that direction: no move, the key
  fails like a failed motion, no wrap (as Vim). It's a jump (`''` and the jumplist when it exists, T419). In an editor
  with a git diff base these are gitsigns' `]c`/`[c`; in the diff view each side's base is the other side, so they
  are diff mode's. Not with an operator (`d]c` cancels): stated, not built.
- `[` or `]` followed by any other key cancels, as today.
- `do` / `dp` in normal mode (operator `d` pending, then `o` or `p`; a count is ignored): run what `:diffget` and
  `:diffput` run for the cursor's row (`HostCommand::DiffGet`/`DiffPut`, the vim ex-host card), so the host decides:
  outside a diff view it answers `E99: Current buffer is not in diff mode`.

## Tests

New file `crates/fe-vim/src/tests/changes.rs` (a `mod` line in `crates/fe-vim/src/tests/mod.rs`), using a headless
model with `set_diff_base`.
