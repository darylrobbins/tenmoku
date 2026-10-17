---
task_uid: "01m55asdhb1sf6s0vf6yc8wjcq"
task: "T157"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test virtual_replace_breaks_match_neovim (top level, in crates/fe-vim/tests/virtual_replace_breaks_oracle.rs) passes: every case of crates/fe-vim/tests/corpus/virtual_replace_breaks.toml (at least 10, recorded by record_corpus_with_neovim from nvim --clean --headless 0.12.x into virtual_replace_breaks.expected.json) matches nvim's buffer, cursor, mode, registers, visual anchor and marks."
    required: true
    manual: false
  - id: "C2"
    text: "The corpus has, and the test asserts by name, every case the spec's Problem lists: the rest of the row replaced, a row added on the last row (twice), the next row's indentation replacing columns, \u003cBS\u003e back over the break, a count, and ."
    required: true
    manual: false
  - id: "C3"
    text: "R's line break is unchanged: the existing replace_matches_neovim test in crates/fe-vim/tests/replace_oracle.rs still passes unchanged."
    required: true
    manual: false
---

# fe-vim: Enter in gR's virtual replace mode replaces the rest of the row and goes on replacing the next one, as nvim does (Mac T804)

## Problem

The user ruled on 2026-10-17: "yes, add those vim commands to the Day-one editor track", naming `gR`.
In virtual replace mode `<CR>` doesn't split the row as `R`'s does (`:help vreplace-mode`): "the rest of
the current line will be replaced by the <NL> (that is, they are deleted), and replacing continues on the
next line. A new line is NOT inserted unless you go past the end of the file." Probed with
`nvim --clean --headless` 0.12.6 (the oracle's settings: 'autoindent', tabstop 4, 'expandtab'):

- `gRx<CR>y<Esc>` on `abcdef\nghi` from col 1 makes `ax\nyhi`, caret (1,0).
- On the last row a row is added: `gRx<CR>y<Esc>` on `ab\ncd` from (1,1) makes `ab\ncx\ny`;
  `gRx<CR><CR>y<Esc>` on `abc\ndef` from (1,1) makes `abc\ndx\n\ny`.
- The next row gets the indentation as screen columns replaced: `gRx<CR>y<Esc>` on
  `    abcdef\n  ghijkl` from col 6 makes `    abx\n    yjkl`.
- `<BS>` back over it puts both rows back: `gRx<CR>y<BS><BS><Esc>` on `abcdef\nghi` from col 1 makes
  `axcdef\nghi`.
- `2gRx<CR>y<Esc>` on `abcdef\nghi` from col 1 makes `ax\nyx\ny`; `gRx<CR>y<Esc>j0.` on
  `abcdef\nghi\njkl` makes `ax\nyhi\nx\ny`.

## Change

In `crates/fe-vim/src/replace.rs`, `VimState::line_break` in a virtual session (Mac T803's
`gR`) deletes the rest of the caret's row and moves to the next row, where its 'autoindent' indentation
replaces that many screen columns as typed spaces would, and typing goes on replacing there; on the last
row it adds a row as `R`'s break does. Keep it as one entry for `<BS>`, which puts back the deleted
text and what the indentation replaced, and for the count's copies and `.`, which type it again. `R`'s
line break is unchanged.

The test is the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/virtual_replace_breaks.toml`
(every probed case above; no case ends in insert or replace mode) recorded into
`virtual_replace_breaks.expected.json` by the ignored `record_corpus_with_neovim` test in
`tests/oracle.rs`, and a new `crates/fe-vim/tests/virtual_replace_breaks_oracle.rs` modelled on
`tests/replace_oracle.rs`. `crates/fe-vim/src/lib.rs`'s docs are out of scope; the lead files one docs
chore after these cards land.

## Implementation notes

- `virtual_break` types the break: it deletes the rest of the row (and an unused indentation a previous break typed), and the next row's columns take the indentation as typed spaces would. The entry's `put` is `\n` plus the indentation over the row's existing newline; `was` holds the deleted rest and newline, `indent_was` what the indentation replaced, so `<BS>` puts back the indentation, then the rest. On the last row it adds a row as `R` does.
- A break that goes on to the next row isn't noted for the ledger: nvim leaves its indentation at `Esc`; an added row's goes.
- The count's copies of keys with a line break are `virtual_copies`, a row-by-row simulation; keys with a `<BS>` and a line break still use `R`'s `copies`.
- Carried T156 F5, F6, F8 fixed in `virtual_over` and `repeat_typed`; their oracle cases are in `virtual_replace_breaks.toml` (T156's corpus is outside this scope).
