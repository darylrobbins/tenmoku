---
task_uid: "01m55cef6g9k2q7vxy3rdhzz11"
task: "T165"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The integration test completions_match_neovim_for_commands_fe_vim_runs (top level, in crates/fe-vim/tests/ex_complete.rs) passes: for at least 10 prefixes (among them noh, sor, norm, x, s, d, w, a range '\u003c,'\u003enoh, and an argument s/x giving None), Ex::completions gives nvim 0.12.x's getcompletion(prefix, 'cmdline') list, hard-coded as probed, filtered to the names Ex::parse doesn't answer E492, with the offset of the name."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test every_completion_is_a_command (top level, in crates/fe-vim/tests/ex_complete.rs) passes: each name complete.rs lists, and each crates/fe-vim/src/ex/table.rs entry's name, parses as a command (not E492)."
    required: true
    manual: false
---

# fe-vim: Ex::completions lists the ex commands fe-vim runs that a typed name abbreviates, in nvim's order, for the command line's Tab (Mac T831)

## Problem

fe-vim's command line has no completion at Mac main 92c57c57 (the crate docs list "the line's
completion" as not planned), and its host has nothing to ask for one. The user approved it on 2026-10-17
in the "Vim breadth" track. nvim (0.12.6, `--clean --headless`) completes an ex command's name with `<Tab>`
(`:help cmdline-completion`); `getcompletion(prefix, 'cmdline')` gives its list, in the order `<Tab>`
cycles through it:

- `noh` gives `nohlsearch`; `sor` `sort`; `norm` `normal`; `pu` `put`; `marks` `marks`; `reg`
  `registers`.
- `x` gives `xall, xit, xmap, xmapclear, xmenu, xnoremap, xnoremenu, xunmap, xunmenu`.

fastedit should offer only the commands fe-vim runs: nvim's list without the names fe-vim doesn't take
(for `x`, only `xit`).

## Change

New `crates/fe-vim/src/ex/complete.rs` (declared `mod complete;` in `crates/fe-vim/src/ex.rs`), with a
public inherent fn on `Ex` (so lib.rs needs no new export): `Ex::completions(line: &str) -> Option<(usize,
Vec<&'static str>)>`. For a line whose text after the range (`:`s, blanks and line specifiers skipped, as
`split` reads them) is a run of letters ending the line, it returns the byte offset where that run starts
and the full names of the commands fe-vim runs that the run abbreviates, in nvim's order (as
`getcompletion` lists them: probe the order nvim uses for ties and mixed case); otherwise (the caret past
the name, in an argument) `None`. The names: a const list in complete.rs of the commands ex.rs, lines.rs,
global.rs and normal.rs take, by full name (`delete`, `nohlsearch`, `substitute`, `yank`, `join`, `move`,
`copy`, `t`, `sort`, `global`, `vglobal`, `normal`, the host's `write`, `quit`, `wq`, `xit`, `exit`,
`edit`, `saveas`, `qall`, `quitall`, `wall`, `diffget`, `diffput`, `diffsplit`, `Explore`, `oldfiles`,
`copen`, `lopen`, `cclose`, `lclose`, `Replace`, `browse`, and any other `command()` takes), plus every
entry's name in `ex/table.rs`'s `COMMANDS` (Mac T822's seam), so a table command is offered once it's
there.

Test: a new `crates/fe-vim/tests/ex_complete.rs` with the probed lists above and more (at least 10
prefixes, among them one with a range, `'<,'>noh`, and an argument, `s/x`), each asserting
`Ex::completions` gives nvim's list filtered to the names whose `Ex::parse` isn't E492 (hard-code nvim's
lists as probed, with the nvim version in a comment); and one asserting every name the const list holds
parses as a command (not E492), so the list can't hold a name fe-vim has dropped. The host's `<Tab>` is a
separate card (Mac T832).

## Implementation notes

- `completions` returns `None` for an empty name (nvim lists nothing for `''` either) and sorts by byte, which is the order `getcompletion` gives (capitals first).
- Not done: the T164 F1/F2 carried findings (`*` range handling at ex.rs:983/985). This task's only change to ex.rs is the `mod complete;` line; fixing them needs resolver changes plus new nvim-recorded corpus cases outside this task's scope, so they stay open for a card of their own.
