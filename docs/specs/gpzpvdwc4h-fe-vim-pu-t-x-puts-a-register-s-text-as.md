---
task_uid: "01m55ce56ef16xf9gpzpvdwc4h"
task: "T145"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test ex_put_matches_neovim (top level, in crates/fe-vim/tests/ex_put_oracle.rs) passes on crates/fe-vim/tests/corpus/ex_put.toml, at least 20 cases recorded with nvim 0.12.x into ex_put.expected.json."
    required: true
    manual: false
  - id: "C2"
    text: "ex_put_matches_neovim asserts these cases are in the corpus: put_unnamed_below, put_bang_charwise_above, put_line_zero, put_last_line, put_named_register_with_address (cursor on the first non-blank), put_numbered_register (:pu 3), put_multiline_register, put_block_register, put_empty_register_fails, put_then_undo, put_then_redo, put_from_global (:g/x/pu a)."
    required: true
    manual: false
  - id: "C3"
    text: ":pu lives in crates/fe-vim/src/ex/put.rs, reached only through its entry in crates/fe-vim/src/ex/table.rs (no change to ex.rs, lib.rs or fe-gpui); :pu = is refused with a message and changes nothing, asserted in ex_put_oracle.rs through HeadlessVim."
    required: true
    manual: false
---

# fe-vim: :pu[t][!] [x] puts a register's text as lines below the range's line (above with !, at the top with :0pu), as nvim does, instead of E492 (Mac T823)

Filed on argon from Mac T823 (card ~/coordination/argon-cards/zzk/T823.json rev 1; note zzk, the Vim breadth track). Mac task uid 01m55bgzgdj42k4yq4fa6tnbh6. Base: argon trunk (mac/main 76125d0 synced). Behaviour probed with nvim --clean --headless 0.12.6. Held in the inbox: waits until helium's Mac T822 (the ex command table, crates/fe-vim/src/ex/table.rs) is on argon's trunk; the Mac lead clears it. Title the host commit "... (Mac T823)".

## Problem

`:pu[t]` is E492 in fe-vim at Mac main 92c57c57 (the crate docs list it as not planned). The user approved
it on 2026-10-17 in the "Vim breadth" track. Probed with `nvim --clean --headless` 0.12.6 (ts=4 sw=4 et,
the oracle's settings):

- `a/b/c` at (0,0), `yy:pu<CR>`: `a/a/b/c`, cursor (1,0).
- `a/b/c` at (1,0), `yiw:pu!<CR>`: `a/b/b/c` (a charwise register goes in as a line, above), cursor (1,0).
- `a/b/c` at (1,0), `yy:0pu<CR>`: `b/a/b/c`, cursor (0,0).
- `a/  b/c` at (1,0), `"ayy:3pu a<CR>`: `a/  b/c/  b`, cursor (3,2), the first non-blank.

## Change

New `crates/fe-vim/src/ex/put.rs`, an entry in `crates/fe-vim/src/ex/table.rs` (the seam Mac T822 adds;
`put`, shortest `pu`, edits): `:[line]pu[t][!] [x]` (`:help :put`). The register `x` (by default `"`), as
`Registers::read` gives it (the clipboard registers and `/` included), always goes in as whole lines: below
the range's last line (the cursor's by default), or above it with `!`; line 0 puts above the first. A
block register's rows are the lines. The cursor goes to the last new line's first non-blank. It is one
transaction, one `u` undoes it (cursor where nvim's undo leaves it), `.` doesn't repeat it, and it writes no
register. An empty or unset register fails with nvim's error and changes nothing. `:pu =` (the expression
register) is refused with a message ("the = register: not supported", fe-vim has no expressions). A
register name after blanks is optional (`:pu a`, `:pu  a`); `:pu 3` names register 3, not a count.

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/ex_put.toml` (a header comment as
the other corpora have; a key that fails is a case's last key), recorded into `ex_put.expected.json` by the
ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a new `crates/fe-vim/tests/ex_put_oracle.rs`
modelled on `tests/ex_lines_oracle.rs` (load, a minimum case count, the case names below asserted present,
`check` on each). Touch nothing in ex.rs, lib.rs or fe-gpui.
