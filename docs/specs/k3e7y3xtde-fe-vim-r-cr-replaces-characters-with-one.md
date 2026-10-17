---
task_uid: "01m55ascf972sbvxk3e7y3xtde"
task: "T155"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test replace_break_and_gi_match_neovim (top level, in crates/fe-vim/tests/replace_break_and_gi_oracle.rs) passes: every case of crates/fe-vim/tests/corpus/replace_break_and_gi.toml (at least 20, recorded by record_corpus_with_neovim from nvim --clean --headless 0.12.x into replace_break_and_gi.expected.json) matches nvim's buffer, cursor, mode, registers, visual anchor and marks."
    required: true
    manual: false
  - id: "C2"
    text: "The corpus has, and the test asserts by name, every case the spec's Problem lists: r\u003cCR\u003e, r\u003cC-j\u003e and a register before r, the blank after the break dropped, indented and tab-indented rows, a count, too few left (failing), the row's last character, . and u; gi with no insert yet, after A, with a count, after rows put in above, after cw and after R, and . after gi."
    required: true
    manual: false
---

# fe-vim: r<CR> replaces characters with one indented line break and gi inserts where insert mode was last left, as nvim does (Mac T800)

## Problem

The user ruled on 2026-10-17: "yes, add those vim commands to the Day-one editor track", naming `r<CR>`
(replace a character with a line break) and `gi` (insert where insert mode was last left). At Mac main
92c57c57 both are missing: `reducer.rs`'s `Prefix::Replace` takes only a char (`<CR>` cancels) and
`edit.rs`'s `replace_char` refuses `\n`/`\r`; `g` then `i` cancels. Probed with `nvim --clean
--headless` 0.12.6 (the oracle's settings: 'autoindent', 'expandtab', tabstop 4):

- `r<CR>` on `foo bar` from col 3 makes `foo\nbar`, caret (1,0); `r<C-j>` and `"ar<CR>` the same. On
  `foo  bar` from col 3 it makes `foo\nbar` (the blank after the break dropped); from col 4 `foo \nbar`.
  On `    foo bar` from col 7 it makes `    foo\n    bar`, caret (1,3); on `\tfoo bar` from col 4,
  `\tfoo\n    bar`. `3r<CR>` on `foo bar baz` from col 3 makes `foo\nr baz` (one break for the count);
  `9r<CR>` with too few left fails; on the row's last char `foo bar` col 6 makes `foo ba\n`. `r<CR>w.` on
  `foo bar baz` makes `foo\nbar \naz`; `u` undoes it in one step.
- `gi` with no insert yet inserts at the caret as `i` does (`giZ<Esc>` on `foo bar` col 2: `foZo bar`).
  After `AX<Esc>j`, `giZ<Esc>` makes `foo barXZ` (past the row's last char, where `` `^ `` stops on it);
  after `iX<Esc>gg` from (1,1), `3giZ<Esc>` makes `bXZZZaz`; after rows put in above (`ggOnew<Esc>`) it
  follows the mark; after `cwQQ<Esc>` and after `RQQ<Esc>` it inserts after `QQ`. `.` after `giZ<Esc>`
  repeats it as `i`: `giZ<Esc>j0.` makes `Zbaz` on the second row.

## Change

- `crates/fe-vim/src/reducer.rs`: `Prefix::Replace` (not in a visual mode) takes `VimKey::Enter` and
  `<C-j>` as `vim.replace_char {"char": "\n"}` with the count; in a visual mode `r<CR>` still cancels
  (nvim writes CR characters there, which fe-text can't hold; T344). `Prefix::G` takes `i` with no
  operator pending and outside a visual mode as a new `vim.insert_last {"count"}`; after an operator or
  in a visual mode `gi` cancels as now (nvim moves the cursor to the mark and fails).
- `crates/fe-vim/src/edit.rs` `replace_char`: for `\n`, the `count` graphemes from each caret become one
  line break, the new row indented as insert mode's `<CR>` indents it (copying the indentation and
  dropping the blanks moved after the break), the caret where nvim leaves it; too few graphemes left
  fails as now. One transaction, `.` repeating it with its count.
- `crates/fe-vim/src/commands.rs` registers `vim.insert_last` (normal mode), and
  `crates/fe-vim/src/insert.rs` runs it: put the caret at the `^` mark exactly (at the row's end if the
  mark is past its last character; the `^` mark is the one `` `^ `` already reads), then open the session
  as `insert::insert` with `InsertAt::Before` and the count, so `.` repeats it as `i`; with the `^` mark
  unset (or deleted with its row) it is `i` at the caret. Don't touch `args.rs` (the motion cards hold it).

The test is the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/replace_break_and_gi.toml` (every
probed case above; no case ends in insert mode; a failing key last) recorded into
`replace_break_and_gi.expected.json` by the ignored `record_corpus_with_neovim` test in `tests/oracle.rs`,
and a new `crates/fe-vim/tests/replace_break_and_gi_oracle.rs` modelled on `tests/line_motions_oracle.rs`.

reducer.rs and commands.rs belong to Mac T770 (g&) and Mac T769 to ex.rs, both landing in Mac host stack 24:
sync a mac/main that carries stack 24 before starting. `crates/fe-vim/src/lib.rs`'s docs are out of
scope; the lead files one docs chore after these cards land.

## Implementation notes

- `gi` at a mark past the row's last character opens the session as `a` would (a caret can't stand there) but records `i` for `.`: `insert::insert_as` takes the args to run and the args to record.
- `r<CR>` copies the row's whole indentation as spaces; it doesn't consult the language's indent hook.
- Carried finding T120 F6 (multiline replay indentation in insert.rs) is not addressed: it is a redesign of replay indentation, unrelated to this card's change.
