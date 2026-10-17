---
task_uid: "01m54f2jappgtj6hxpgwsh652s"
task: "T130"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test word_objects_over_one_visual_position_run_out_to_the_end_as_nvim (tests/visual_word_run_out.rs, top level) passes: a counted aw/aW from one visual position that runs out selects from the run under the caret (the caret itself when one aw finds only blanks to the end) to the text's end, keeping v, V or block mode, and a run-out iw/iW keeps V and block mode, as nvim --clean does (\"x y z\" at (0,2) v3aw: anchor (0,2), cursor (0,5); \"x  y  z\" at (0,2) v3aw: (0,1), (0,7); \"ab cd\\nef\" at (0,3) v4aw: (0,3), (1,2); \"a  \" at (0,2) vaw: (0,2), (0,3); \"x\\ny\\nz\" at (2,0) V3aw: V, (2,1); at (0,0) V9iw: V, (2,1); v3awy on \"x y z\" at (0,2) yanks \"y z\"); it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "A run-out still fails as an error does (state().failed() is true, so a macro stops there); counts that don't run out (v2aw on \"x y z\" at (0,2): anchor (0,1), cursor (0,4)), backward selections and vaw on an empty buffer are unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: aw and aW typed over one visual position that run past the text's end select to it as nvim does, instead of failing, and iw and iW keep V and block mode there (Mac T754)

## Problem

A counted word object typed over one visual position (a fresh `v`, `V` or `<C-v>`) whose count runs past the
end of the text fails in fe-vim and leaves the selection as it was. nvim still selects: from the run under the
caret to the text's end (one past the last character, or the start of an empty last row), keeps the visual
mode, and fails the command only as an error does (a macro stops there).

- `aw`/`aW`: `crates/fe-vim/src/visual/object.rs` `select_object`, the `ends.anchor == ends.cursor` branch
  (line 190 at Mac 80e6c76d): `Some(_) if count > 1 => match object(ends.cursor, count) { .., None => return
  failed(state) }`, and `None => return failed(state)` when even one `aw` finds nothing (blanks running to
  the text's end).
- `iw`/`iW` there already select to the end, but set `kind` to charwise, so `V9iw` and `<C-v>9iW` leave `V`
  and block mode; nvim keeps them.

Reproduced at Mac 80e6c76d with `HeadlessVim` against `nvim --clean --headless` 0.12.6 (the oracle's
settings). Buffer rows joined by `\n`, cursor and anchor (row, byte column):

| buffer | cursor | keys | nvim anchor, cursor, mode | fe-vim |
|---|---|---|---|---|
| `x y z` | (0,2) | `v3aw` | (0,2), (0,5), v | unchanged (0,2), fails |
| `x y z` | (0,3) | `v2aw` | (0,3), (0,5), v | unchanged |
| `x  y  z` | (0,2) | `v3aw` | (0,1), (0,7), v | unchanged |
| `ab cd` | (0,4) | `v2aw` | (0,3), (0,5), v | unchanged |
| `ab cd\nef` | (0,3) | `v4aw` | (0,3), (1,2), v | unchanged |
| `ab cd\n` | (0,3) | `v2aw` | (0,3), (1,0), v | unchanged |
| `end; of [it]` | (0,11) | `v3aW` | (0,8), (0,12), v | unchanged |
| `a  ` | (0,2) | `vaw` | (0,2), (0,3), v | unchanged |
| `    \n    ` | (0,2) | `vaw` | (0,2), (1,4), v | unchanged |
| `x  \ny` | (0,2) | `v2aw` | (0,1), (1,1), v | unchanged |
| `x\ny\nz` | (2,0) | `V3aw` | (2,0), (2,1), V | unchanged |
| `x\ny\nz` | (2,0) | `<C-v>3aw` | (2,0), (2,1), block | unchanged |
| `x\ny\nz` | (0,0) | `V9iw` | (0,0), (2,1), V | (0,0), (2,1), v |
| `x\ny\nz` | (0,0) | `<C-v>9iW` | (0,0), (2,1), block | (0,0), (2,1), v |
| `x y z` | (0,2) | `v3awy` | yanks `y z`; `'<` (0,2), `'>` (0,5) | yanks `y` |
| `x y z` | (0,2) | `v3awd` | `x `, caret (0,1) | `x  z` |

The rule, fitted from ~3,000 random probes: the anchor goes to the start of the run of one class (word or
blanks) under the caret on its row, except that when even one `aw` finds nothing (only blanks up to the
text's end) it stays on the caret; the cursor goes to the text's end position, as the `iw` branch's
`position(&text, text.len())` already does. A run-out still fails as an error does: nvim's
`:let @q="v3aw\eiQ"` then `@q` stops before `iQ` and stays in visual mode. Counts that don't run out
(`v2aw` on `x y z` at (0,2): (0,1), (0,4)) and the empty buffer (`vaw` fails, as now) are unchanged.

## Change

In `select_object`'s one-position branch, for `AWord`/`ABigWord`:

- when `object(ends.cursor, count)` is `None` but `object(ends.cursor, 1)` is `Some`, select from the start of
  the caret's run on its row (word run or blank run, `big` classes for `aW`; an empty row is its own start)
  to `position(&text, text.len())`;
- when `object(ends.cursor, 1)` is `None` on a non-empty text, select from the caret to
  `position(&text, text.len())`;
- in both, keep `ends.kind` and set `ran_out` (so `state.failed`), as the other run-out branches do.

In the `iw`/`iW` branch above it, keep `ends.kind` when `inner_words` runs out. A prototype of exactly this
(about 25 lines in `visual/object.rs`) took a 3,000-case random fuzz of `v`/`V`/`<C-v>` with `iw iW aw aW`
counts from 648 mismatches to 11 (all backward `vh{count}aw`, a separate rule, out of scope) with no new
mismatch, and `cargo test -p fe-vim` passed unchanged.

Regression test: a new integration test `crates/fe-vim/tests/visual_word_run_out.rs`, at the top level of the
file, `word_objects_over_one_visual_position_run_out_to_the_end_as_nvim`, driving `fe_vim::HeadlessVim`
through the table's rows and asserting the anchor (`visual_anchor`), cursor, mode, `state().failed()`, and for
the `y`/`d` rows the text, register and `'<`/`'>` (`vim.mark`). Probe any added row with
`nvim --clean --headless` first.
