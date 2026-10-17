---
task_uid: "01m54f2npakr9rk45basqh7eer"
task: "T130"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test visual_put_of_a_block_sets_the_visual_marks_as_nvim (tests/visual_block_put_marks.rs, top level) passes: after v_p/v_P of a blockwise register, '\u003e is the last byte put on the block's last row (that row's start when nothing went there) and '\u003c the put's first byte, unsnapped, as nvim --clean does (\"héllo wörld ünï\\n日本語 テスト 12\\nabc\" at (0,0): \u003cC-v\u003ejly0viwp gives '\u003c (0,0) '\u003e (1,5), \u003cC-v\u003ejlyjvep '\u003c (1,0) '\u003e (2,5); \"日本語\\nテスト\\nabc\\ndef\" at (0,0) \u003cC-v\u003ejlyGvp '\u003e (4,5); \"ab日本cd\\nüü\\n\\nxyz w\" at (2,0) \u003cC-v\u003e2jyjwv$p '\u003c (3,4) '\u003e (4,4)); it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "The text, the caret and the registers of every visual put are unchanged, and so are the marks of an ASCII block put (\"ab\\ncd\\nef\" at (0,0) \u003cC-v\u003ejlyGvp: '\u003c (2,0) '\u003e (3,1)), of a block whose last row is empty (\"ab日本cd\\nüü\\n\\nxyz w\" at (1,0) \u003cC-v\u003ejlyjvep: (2,0), (3,0)) and of charwise and linewise registers"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: a visual put of a blockwise register sets '< at the put's first byte and '> at the last byte put on its last row, as nvim does, instead of at character starts (Mac T755)

Filed on argon from Mac T755 (card ~/coordination/argon-cards/zy/T755.json rev 1; note zy). Mac task uid 01m54et94syv8r9yk3kjs4jjbt. Base: argon trunk d52c23b (= mac/main 5837b41 synced); the file it changes is byte-identical on Mac main 36627a7c. Title the host commit "... (Mac T755)".

## Problem

After a visual-mode put (`v_p`/`v_P`) of a **blockwise** register, nvim sets `'<` to the byte where the put
block starts and `'>` to the **last byte** put on the block's last row (not the start of its last
character), or to that row's start when nothing was put there. fe-vim snaps both to character starts, so
with a multibyte last character `'>` lands 1 to 2 bytes early, and with an empty first block row `'<` lands
on the row's last character instead of past it.

`crates/fe-vim/src/visual/put.rs` `put`: the block arm (line 268 at Mac 80e6c76d) returns `last + len` as the
end, and the marks are then set from `start = Cursor::at(&after, start)` and
`end = Cursor::at(&after, end - 1)` (lines ~331 to 346), both snapped to a grapheme start.

Reproduced at Mac 80e6c76d with `HeadlessVim` against `nvim --clean --headless` 0.12.6 (the oracle's
settings). Buffer rows joined by `\n`, positions (row, byte column); text and caret already match:

| buffer | cursor | keys | nvim `'<`, `'>` | fe-vim `'<`, `'>` |
|---|---|---|---|---|
| `héllo wörld ünï\n日本語 テスト 12\nabc` | (0,0) | `<C-v>jly0viwp` | (0,0), (1,5) | (0,0), (1,3) |
| same | (0,0) | `<C-v>jlyjvep` | (1,0), (2,5) | (1,0), (2,3) |
| same | (0,0) | `<C-v>jlyjviwP` | (1,0), (2,5) | (1,0), (2,3) |
| `日本語\nテスト\nabc\ndef` | (0,0) | `<C-v>jlyGvp` | (3,0), (4,5) | (3,0), (4,3) |
| same | (0,0) | `<C-v>jly2jviwp` | (2,0), (3,5) | (2,0), (3,3) |
| `ab日本cd\nüü\n\nxyz w` | (2,0) | `<C-v>2jyjwv$p` | (3,4), (4,4) | (3,3), (4,4) |
| `ab日本cd\nüü\n\nxyz w` | (0,2) | `<C-v>jlyGvp` (last block row empty, padded row) | (3,2), (4,1) | same |
| `ab日本cd\nüü\n\nxyz w` | (1,0) | `<C-v>jlyjvep` (last block row empty) | (2,0), (3,0) | same |
| `ab\ncd\nef` | (0,0) | `<C-v>jlyGvp` (ASCII) | (2,0), (3,1) | same |

So `'>` = max(last block row's start, (where the last row's put text starts) + (its byte length, padding
included) − 1), and `'<` = where the first row's put text starts, unsnapped (past the row's end in the
`v$p` row above). Charwise and linewise registers put in a visual mode are unchanged (nvim's `'>` there is
the last character's start, as fe-vim does).

## Change

In `visual/put.rs` `put`, for a blockwise register put over a charwise or blockwise selection (the
`(Some(width), _)` arm), set the visual marks from the raw byte positions instead of the snapped ones:
`'<` at the put's start offset as `put_block` gives it, and `'>` at `(last + len).saturating_sub(1)` clamped to
no earlier than the start of `last`'s row (`last` and `len` from `put_block`'s `[_, last, len]`). The caret and
the other arms stay as they are. A prototype of exactly this (about 12 lines) took a 3,000-case random fuzz
of block yanks over multibyte text put with `v`/`V`/`<C-v>` `p`/`P` from 271 mismatches to 2 (both a wide
character split by padding, where the text differs too; out of scope) with no new mismatch, and
`cargo test -p fe-vim` passed unchanged.

Regression test: a new integration test `crates/fe-vim/tests/visual_block_put_marks.rs`, at the top level of
the file, `visual_put_of_a_block_sets_the_visual_marks_as_nvim`, driving `fe_vim::HeadlessVim` through the
table's rows and asserting the text, the caret and `vim.mark('<')`/`vim.mark('>')`. Probe any added row
with `nvim --clean --headless` first.
