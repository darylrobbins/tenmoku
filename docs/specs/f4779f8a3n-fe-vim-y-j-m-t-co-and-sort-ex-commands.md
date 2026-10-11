---
task_uid: "01m4p2zb2k5hb7phf4779f8a3n"
task: "T423"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim tests in tests/ex_lines.rs: Ex::parse gives :y[ank] [x] [count], :j[oin][!] [count], :m[ove] {address}, :t and :co[py] {address}, :\u003e and :\u003c (repeated) [count] and :sor[t][!] [i] [n] [u] [r] [/pat/] with their abbreviations, and nvim's errors for bad arguments"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test ex_lines_matches_neovim (tests/ex_lines_oracle.rs): tests/corpus/ex_lines.toml has at least 45 cases (each command with no range, a range, marks and searches in addresses, counts, :m0, :m$, :t., :t0, :m into itself as the last key, :j!, :\u003e\u003e with a count, :sort with each flag and a pattern, u after each); text, cursor and registers match nvim 0.12.4"
    required: true
    manual: false
  - id: "C3"
    text: "Each command is one undo step; every earlier corpus expected file is byte-identical and every earlier fe-vim test passes; ex.rs's module docs list the commands and that marks on rows :m moves stay behind"
    required: true
    manual: false
  - id: "C4"
    text: "Redo puts the caret where nvim 0.12.4 does after :m and :t (review F4): fe-vim's history keeps a redo caret record separate from the undo one (src/history.rs), and tests/corpus/ex_lines.toml has u\u003cC-r\u003e cases for :t$, :t0, :m$ and :m0 from a middle row, each matching nvim; and the :j fix of F5 has its regression test in src/tests/ex.rs through the measurement hook"
    required: true
    manual: false
---

# fe-vim: :y :j :m :t :co :> :< and :sort ex commands, checked against nvim

# fe-vim: :y :j :m :t :co :> :< and :sort ex commands

## Problem

fe-vim's ex commands are `:{range}`, `:d`, `:s`/`:&`, `:noh` and the host's `:w :q :e` family
(`crates/fe-vim/src/ex.rs` `ExCommand`, ~85–344). The line commands nvim users reach for most are
missing; `:sort` too. Gap analysis 2026-10-11.

## Change

`ExCommand` gains the commands; their parsing and running live in `src/ex/lines.rs` (new,
`mod lines;` in `ex.rs`), with `ex.rs`'s `command()` and `execute()` calling into it. No `lib.rs`
change (`ExCommand` is already re-exported).

- `:y[ank] [x] [count]`: the range's rows (or `count` from its last) yanked linewise into `x`.
- `:j[oin][!] [count]`: joins the range (at least two rows; with one, it and the next), `!` without
  spaces, as `J`/`gJ` do (`edit.rs`'s join).
- `:m[ove] {address}`: moves the range below `{address}` (`0` for the top); E134 moving into itself.
- `:t` and `:co[py] {address}`: copies the range below `{address}`.
- `:>` and `:<`, repeated (`:>>>`) for more shifts, with `[count]`: shift as `>>`/`<<` do.
- `:sor[t][!] [i] [n] [u] [r] [/pat/]`: sorts the range (default all) as nvim does: `!` reversed,
  `i` ignoring case, `n` by the first decimal number, `u` keeping the first of equal rows, `/pat/`
  sorting by what follows the match (or the match with `r`); stable.
- Each is one undo step, leaves the cursor where nvim does, and writes registers only as nvim does.
  Abbreviations as `:help` lists them; errors as nvim's (E14, E16, E134, E488, E492…).
- `ex.rs`'s module docs list the commands; marks on rows `:m` moves don't move with them (nvim's
  do): a known limit written there.

**Clean room**: `nvim --clean` (0.12.4) and `:help :yank`, `:help :join`, `:help :move`,
`:help :copy`, `:help :>`, `:help :sort`; never source, hjkl, Zed or Helix.

### Corpus

`tests/corpus/ex_lines.toml`, at least 45 cases: each command with no range, a range, marks and
searches in addresses, counts, `:m0`, `:m$`, `:t.`, `:t0`, `:m` into itself (last key), `:j!`,
`:>>` with a count, `:sort` with each flag and a pattern, `u` after each, and the cursor and
registers after.

## Tests (new files)

- `tests/ex_lines.rs`: `Ex::parse` gives each command with its abbreviations and arguments, and the
  parse errors.
- `tests/ex_lines_oracle.rs` (`#[allow(dead_code)] mod support;`) with `ex_lines_matches_neovim`.

## Size

About 350 non-test lines.

## Out of scope

`:g`/`:v` and `:normal` (the next card), `:sort` flags `x o b f` and `:sort` with `l`, marks moving
with `:m`, `:p`/`:#`/`:=` (nothing to print to), the GPUI check.

## Conflicts

`src/ex.rs` is also in T357's scope (inbox, review follow-ups of T221 whose findings are in the
workbench): narrow T357 or let this go first. No running or ready task holds `ex.rs`, so this can
start now.
