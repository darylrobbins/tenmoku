---
task_uid: "01m55mj7xkkkywy40zv5atkjqq"
task: "T159"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test ex_delmarks_matches_neovim (top level, in crates/fe-vim/tests/ex_delmarks_oracle.rs) passes on crates/fe-vim/tests/corpus/ex_delmarks.toml, at least 10 cases recorded with nvim 0.12.x, among them (asserted by name) delmarks_one, delmarks_range, delmarks_spaced, delmarks_bang, delmarks_jump_mark, delmarks_visual_marks, clearjumps_then_ctrl_o_fails."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test delmarks_messages (top level, same file) passes: :delmarks alone leaves E471 and :delmarks 1 (or a bad range) E475 with nvim 0.12.x's exact text, changing no mark."
    required: true
    manual: false
---

# fe-vim: :delmarks (and :delm!) delete marks and :clearjumps empties the jumplist, as nvim does, instead of E492 (Mac T906)

Filed on argon from Mac T906 (card ~/coordination/argon-cards/zzp/T906.json rev 1; note zzp, the Vim parity track part 2). Mac task uid 01m55mey63nbt32bktybgwrkrz. Base: argon trunk (mac/main synced). Corpus cases are recorded with nvim 0.12.x; unit tests are named by full module path, integration tests by bare fn name. Argon deps: T147 (Mac T825). Held in the inbox: waits until Mac T866 (another host's or the Mac's) is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T906)".

## Problem

At Mac main 6f032b12 `:delmarks` and `:clearjumps` are E492 in fe-vim (`crates/fe-vim/src/lib.rs` Known
limits). The user approved them in the "Vim parity" track on 2026-10-17.

## Change

Two ex commands, each a module under `crates/fe-vim/src/ex/` with its entry in `crates/fe-vim/src/ex/table.rs`
(T822's table):

- `crates/fe-vim/src/ex/delmarks.rs`: `:delm[arks] {marks}` deletes the named marks: letters, ranges
  (`a-d`), spaces between, and the marks fe-vim has (`'`, `<`, `>`, `.` once the changelist card lands);
  `:delm[arks]!` deletes every lowercase mark; no argument is E471, a bad one E475, as nvim's (probe the
  messages).
- `crates/fe-vim/src/ex/clearjumps.rs`: `:cle[arjumps]` empties the jumplist.

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/ex_delmarks.toml` (each then a `'x`
that fails, `<C-o>` after `:clearjumps`), recorded into `ex_delmarks.expected.json` by the ignored
`record_corpus_with_neovim` in `tests/oracle.rs`, and a new `crates/fe-vim/tests/ex_delmarks_oracle.rs`
modelled on `tests/line_motions_oracle.rs`.

table.rs is changed by T822–T825 and T865–T866: this card depends on T825 and T866.

## Implementation notes

Probed on nvim 0.12.4: `:delmarks 1` is not an error (nvim has marks `0`-`9`; fe-vim has none, so digits delete nothing), and `:delmarks '` is E475, so C2's bad-argument cases are `a-`, `c-a`, `a-B`, `a,b` and `'`. Deleting only one of `'<`/`'>` makes both read the other end, as nvim's do (`LastVisual::delete`). `:delmarks!` also deletes `'^ '[ '] '.`.
