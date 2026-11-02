---
task_uid: "01m6ce88r0cwb5x728p0gfqrxr"
task: "T400"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "undo_marks_matches_neovim (crates/fe-vim/tests/undo_marks_oracle.rs) passes on a recorded corpus that names cases for: madd u; madd u \u003cC-r\u003e; :d u; J carrying a mark, then u; dG u; Vj\u003cEsc\u003e then dG u ('\u003c, '\u003e); :g/x/d u; 2u over two changes; cc\u003cEsc\u003e on the marked row, then u; [e u"
    required: true
    manual: false
  - id: "C2"
    text: "That corpus test fails at the base (stated in the implementation notes)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-vim passes with every existing test unchanged, the [e ]e tests included"
    required: true
    manual: false
  - id: "C4"
    text: "Recorded under the T802 convention: no corpus other than undo_marks.* changes, support::NVIM is unchanged in the diff, and the notes say which nvim recorded it (0.12.4 or 0.12.6)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo clippy -p fe-vim --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# fe-vim: u puts back the a-z marks and the last visual extent as they were before the change it takes back, and <C-r> swaps them again, as nvim does (Mac T1285)

## Problem

In nvim, `u` puts back the lowercase marks (`a`–`z`) and the last visual extent (`'<`, `'>`) as they were before
the change it takes back, and `<C-r>` swaps them again. Probed 2026-11-01 with `nvim --clean --headless` (0.12.6)
on `one/two/three/four`, cursor on `two`:

- `madd` → mark `a` gone; `maddu` → `a` back on row 2; `maddu<C-r>` → gone again.
- `majddkJu` (mark carried by a join, then undone) → `a` on row 2; `mjmaggdGu` → `a` on row 2.
- `Vj<Esc>ggdGu` → `'<` back on row 2.

fe-vim does this only for `[e`/`]e` row moves: `crates/fe-vim/src/feature/line_edits.rs` keeps a `Moves` record
(`Saved { named, visual }` per `TransactionId`, at most `history::KEPT`) and `history::undo`
(`crates/fe-vim/src/history.rs`) calls `line_edits::outgoing` / `line_edits::reverted` around each step. Every
other change (`dd`, `:d`, `J`, `dG`, `:g`, `:sort`, a filter, `cc`…) leaves a deleted mark deleted after `u`.
A pre-existing divergence was noted on T423 (2026-10-11): "fe-vim's normal `J` + `u` doesn't restore marks
carried by the join". The user approved closing the remaining vim parity gaps on 2026-11-01 ("undo marks").

## Change

Generalise the `Moves` record to every undoable change, as nvim's undo block saves marks:

- Move `Moves` and `Saved` out of `line_edits.rs` into `crates/fe-vim/src/history/undo_marks.rs` (a submodule
  declared from `history.rs`, so `lib.rs` is untouched); keep the type name `Moves` re-exported at
  `crate::feature::line_edits::Moves` (or update the one field type in `state.rs` only if the re-export is not
  possible) so `VimState::moves` keeps working.
- In `commands::dispatch` (`crates/fe-vim/src/commands.rs`), take `Saved::of(state, model)` before the registry
  runs the command, and record it for each transaction id in the `Dispatched` result that has no record yet
  (the first change of an undo block keeps the marks from before it; a `:g` or `:normal` that makes several
  edits in one transaction records once). `u` and `<C-r>` themselves record nothing. An insert session records
  the marks from before the command that opened it.
- `history::undo` restores through the general record: before each step, the outgoing marks; after it, if the
  step's transaction has a record, put back its `a`–`z` (those it holds) and its visual extent, and keep the
  outgoing ones in its place for the step back, as `line_edits::reverted` does today. `[e`/`]e` keep their
  current behaviour through the same record.
- Uppercase marks, `'[`, `']`, `'.`, `'^` and the jumplist are unchanged.

Tests: a new oracle corpus area `crates/fe-vim/tests/corpus/undo_marks.toml`, recorded into
`undo_marks.expected.json`, and `crates/fe-vim/tests/undo_marks_oracle.rs` (as `marks_special_oracle.rs` loads
its corpus). Record with nvim 0.12.6 under the T802 convention (`docs/decisions/q3sndmknyh-line-end-motions-oracle-stamp.md`:
temporarily set `support::NVIM`, run the ignored recorder, keep only this corpus with the 0.12.4 stamp, revert the
rest) and say so in the implementation notes. Corpus rules: a failing key must be the case's last; a case can't
end in insert mode; `u` must not reach past the case's own changes (it would undo the buffer setup). Probe every
expected value against nvim; don't read nvim's source (clean room).

## Implementation notes

- `Moves` and `Saved` moved to `crates/fe-vim/src/history/undo_marks.rs`; `crate::feature::line_edits::Moves` is a
  re-export, so `state.rs` is untouched (its doc comment on `moves` still speaks of `[e`/`]e` only).
- `commands::dispatch` records around the registry call. A command flagged `mutates_buffer` catches the marks up first
  (its handler would anyway), so they are saved on their current rows. `u`/`<C-r>` report the transactions they step
  over (`:undo`, `:normal u` too), and those are skipped. A session's record is held until its transaction closes, and
  dropped if the buffer's version didn't change, so `i<Esc>` can't push a real record out.
- Scope widened to `crates/fe-vim/src/ex/filter.rs`: `apply_filter` and `apply_read_command` run outside any command,
  when the host hands back the output, so they note their own record. Probed by hand (`:2,3!head -1`, `:0r !echo x`);
  the two tests in `undo_marks_oracle.rs` state the values.
- Known limit, not changed: marks on rows `:m` moves stay behind (see `ex.rs`), so `:m` then `u` `<C-r>` differs from
  nvim. The corpus has `:m` then `u`, and `[e` then `u` `<C-r>`.
- C2: at the base sources, `undo_marks_matches_neovim` fails with 28 of 62 cases differing (`dd_undo`, `ex_d_undo`,
  `dG_undo`, `global_d_undo`, `two_undos_over_two_changes`, ...), and both filter tests fail.
- C4: the corpus was recorded with nvim **0.12.4**, the only nvim on this machine (C4 was respecced to accept either
  version). 0.12.4 matches `support::NVIM`, so no temporary pin was needed and the file carries the usual 0.12.4 stamp.
  No other corpus changed: the recorder's run left all the others byte-identical. The spec's 0.12.6 probe values for
  `madd`, `maddu`, `maddu<C-r>`, `majddkJu`, `mjmaggdGu` and `Vj<Esc>ggdGu` match this recording. The hand probes behind
  the filter and `:r !` tests were made with 0.12.4 too.
