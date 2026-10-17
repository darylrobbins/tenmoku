---
task_uid: "01m55ce5ey3nqn2fmmcgrd4cqp"
task: "T146"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The integration test ex_undo_matches_neovim (top level, in crates/fe-vim/tests/ex_undo_oracle.rs) passes on crates/fe-vim/tests/corpus/ex_undo.toml, at least 12 cases recorded with nvim 0.12.x, among them (asserted by name) undo_once, undo_abbreviated_u, redo_after_undo, redo_abbreviated_red, undo_zero_undoes_all, undo_with_nothing_to_undo_fails, undo_range_fails, undo_bang_fails, dot_after_undo, undo_from_global (:g/x/s/x/y/ then :undo)."
    required: true
    manual: false
  - id: "C2"
    text: ":undo {N} for N above 0 is refused with a message and changes nothing, asserted in ex_undo_oracle.rs through HeadlessVim; both commands live in crates/fe-vim/src/ex/undo.rs, reached only through crates/fe-vim/src/ex/table.rs."
    required: true
    manual: false
---

# fe-vim: :u[ndo] and :red[o] undo and redo as u and <C-r> do, and :undo 0 undoes every change, as nvim does, instead of E492 (Mac T824)

Filed on argon from Mac T824 (card ~/coordination/argon-cards/zzk/T824.json rev 2; note zzk, the Vim breadth track). Mac task uid 01m55bh0t6sf45vsr720zzrh0r. Base: argon trunk (mac/main 76125d0 synced). Behaviour probed with nvim --clean --headless 0.12.6. Argon dep: T145 (the table chain adds its entries to ex/table.rs one at a time). Title the host commit "... (Mac T824)".

## Problem

`:u[ndo]` and `:red[o]` are E492 in fe-vim at Mac main 92c57c57 (the crate docs list `:undo` as not
planned). The user approved them on 2026-10-17 in the "Vim breadth" track. In `nvim --clean --headless`
0.12.6, `:undo` is `u`, `:redo` is `<C-r>`, and `:undo 0` undoes every change (`abc`, `xx:undo 0<CR>`
leaves the text as it was loaded); `:undo {N}` with N above 0 jumps in nvim's undo tree by change number
(E830 for a number it hasn't got), which fastedit's linear history doesn't number.

## Change

New `crates/fe-vim/src/ex/undo.rs` with two entries in `crates/fe-vim/src/ex/table.rs` (the seam Mac T822
adds; both edit): `undo`, shortest `u`, and `redo`, shortest `red`. `:u` and `:red` do what `u` and `<C-r>`
do, through the same code (cursor, marks and the failed flag as theirs: running out of changes fails as
`u` does); `:undo 0` undoes until nothing is left to undo. `:undo {N}` for N above 0 is refused with a
message (":undo {N}: not supported", nothing changed). A range or a `!` is nvim's error for each (probe
them: `:2u`, `:u!`). `.` doesn't repeat them, and neither starts a transaction of its own.

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/ex_undo.toml` (header comment as the
other corpora; a key that fails is a case's last key), recorded into `ex_undo.expected.json` by the ignored
`record_corpus_with_neovim` in `tests/oracle.rs`, and a new `crates/fe-vim/tests/ex_undo_oracle.rs`
modelled on `tests/ex_lines_oracle.rs`. Touch nothing in ex.rs, lib.rs or fe-gpui.
