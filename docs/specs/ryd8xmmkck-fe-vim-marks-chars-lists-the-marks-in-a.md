---
task_uid: "01m55manvgzbgbrpryd8xmmkck"
task: "T182"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test marks_list_as_neovim (top level, in crates/fe-vim/tests/ex_marks.rs) passes: through HeadlessVim, :marks, :marks aB and :marks b a leave HostCommand::Pick with title \"Marks\", header \"mark line  col file/text\" and rows whose cells equal nvim 0.12.6's probed :marks output (order, 1-based line, byte col, text with leading blanks skipped and control characters as ^X), hard-coded with the nvim version in a comment; :marks zq gives E283: No marks matching \"zq\" and no host command; :2marks gives E481 and :marks! E477."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test picking_a_mark_jumps_as_backtick (top level, same file) passes: for a lowercase, an uppercase, the jump mark and \u003c and \u003e rows, HeadlessVim::dispatch of the row's then.invocation() leaves the cursor, mode and jump mark ('') as feeding `x does on a twin HeadlessVim."
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test workbench::vim_host_tests::pick_tests::marks_opens_a_picker_and_jumps (new crates/fe-workbench/src/vim_pick_tests.rs, declared in crates/fe-workbench/src/vim_host_tests.rs) passes: in a real workbench with vim on, typing :marks on the bar's line after ma on row 2 shows the modal vim_pick, focused, with its rows and the header as its status; typing a query narrows the rows; Enter on mark a dismisses it, focuses the editor and puts its cursor on the mark; a second :marks dismissed with Escape (twice, for the picker's vim normal mode) focuses the editor with its cursor unmoved."
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-vim and cargo test -p fe-workbench pass, and crates/fe-workbench/src/workbench.rs is unchanged."
    required: true
    manual: false
---

# fe-vim: :marks [chars] lists the marks in a picker with nvim's columns and order, and picking one jumps to it as `x does, through a generic HostCommand::Pick (Mac T865)

## Problem

`:marks` is E492 in fe-vim at Mac main 76125d07 (the crate docs list it as not planned). The user decided on
2026-10-17 that `:marks` and `:reg` open a picker, like the command palette, rather than print a list:
choosing a mark jumps to it as `` `x `` does. fe-vim leaves file and window commands to the host as a
`HostCommand` (`crates/fe-vim/src/ex.rs`), and `:oldfiles` already answers with a picker (Open Recent,
`HostCommand::OldFiles`, run by `Workbench::run_window_command` in `crates/fe-workbench/src/vim_host.rs`).
There is no host command that carries a list to pick from, so each list command (`:marks` here, `:reg`
next, `:jumps` or `:changes` later) would need its own variant and its own arm in every exhaustive
`HostCommand` match (`vim_host.rs`, `git_commit.rs`, `git_diff.rs`).

nvim 0.12.6 (`--clean --headless`, `nvim_exec2('marks', {output = true})`), after `ggwma`, `jjmB`, `jlllmc`
and some edits on `alpha one / ··beta two / gamma three / delta` (rows as nvim printed them):

```
mark line  col file/text
 '      2   13 beta two ins
 a      1    6 alpha one
 c      3    4 delt
 B      3    6 delt
 Z      1    0 g.txt
 "      1    0 alpha one
 [      2   10 beta two ins
 ]      2   14 beta two ins
 ^      2   14 beta two ins
 .      2   10 beta two ins
 <      1    8 alpha one
 >      2   13 beta two ins
```

- The order is `'`, `a`–`z`, `A`–`Z`, `0`–`9`, `"`, `[`, `]`, `^`, `.`, `<`, `>`, whatever order an
  argument names them in (`:marks ba` and `:marks b a` both list `a` then `b`). Line is 1-based, col the
  byte column (as `getpos()`), and the text is the mark's row with its leading blanks skipped (`··beta two`
  shows `beta two`), control characters as `^X`; a mark in another file shows the file's name (`Z` above).
- `:marks {chars}` lists only the marks named; none of them set is `E283: No marks matching "zq"` (the
  argument as typed). `:2marks` is `E481: No range allowed`, `:marks!` is `E477: No ! allowed`. The name
  takes no abbreviation: `:mar` and `:mark` are `:mark` (E471 with no argument), not a listing.

## Change

A generic "pick from this list" host command, and `:marks` as its first user. `:reg` follows on this seam
(the next card), with no host change.

fe-vim:

- New `crates/fe-vim/src/ex/pick.rs` (declared `mod pick;` in `crates/fe-vim/src/ex.rs`, its types
  re-exported there and from `crates/fe-vim/src/lib.rs`'s `pub use ex::{…}` line, the one lib.rs change):
  `Pick { title: &'static str, header: &'static str, rows: Vec<PickRow> }`, `PickRow { cells: Vec<String>,
  then: PickThen }` and `enum PickThen { Jump(char) }`, all `Clone, Debug, PartialEq, Eq`.
  `PickThen::invocation(&self) -> fe_command::Invocation` is what choosing the row runs in the editor:
  `Jump(x)` is `vim.move` with `{"motion": {"mark": {"name": x, "exact": true}}}`, as `` `x `` is. Also
  a `pub(crate) fn printable(text: &str) -> String` (control characters as `^X`, DEL as `^?`, as nvim
  shows them), which the next card's `:reg` uses too.
- `crates/fe-vim/src/ex.rs`: a `HostCommand::Pick(Pick)` variant, documented as a list the host shows to
  pick from, each row's `then` run in the editor the line was typed in.
- New `crates/fe-vim/src/ex/marks.rs`, a `COMMAND` entry in Mac T822's table (`"marks"`, shortest `5`,
  can't edit), with its two lines in `crates/fe-vim/src/ex/table.rs`. Parse: no range (E481), no bang
  (E477), the rest of the line the mark names (blanks ignored). Run: one row per mark set, in nvim's order
  above, for the marks fe-vim has (`'`/`` ` ``, `a`–`z`, `A`–`Z`, `[`, `]`, `^`, `.`, `<`, `>`, through
  `VimState::mark` and `Mark::get` on the model's snapshot; a mark whose row was deleted is left out),
  cells `[name, line, col, text]` as nvim prints them (line 1-based, text from `printable` with leading
  blanks skipped), `then` `PickThen::Jump(name)`. fe-vim has no `0`–`9` or `"` marks (nvim's come from
  shada and from leaving the buffer), and keeps `A`–`Z` per view, so the text column is always a row's
  text. It leaves `HostCommand::Pick(Pick { title: "Marks", header: "mark line  col file/text", rows })`
  as the line's host command (`state.host_command`); with an argument none of whose marks is set, `E283:
  No marks matching "{arg}"` and no host command. With no argument and none set, an empty pick (nvim
  always has `'`).

fe-workbench:

- New `crates/fe-workbench/src/vim_pick.rs`, declared in `crates/fe-workbench/src/vim_host.rs` as
  `#[path = "vim_pick.rs"] mod vim_pick;`: `VimPicker`, a `PickerDelegate` (the palettes' picker,
  `crate::picker`) over a `fe_vim::Pick` and the `WeakEntity<EditorView>` it was typed in. Placeholder: the
  pick's title; `status()`: its header; each row its cells in aligned columns, in the order fe-vim gave
  them; the query filters as a case-insensitive subsequence (`toolbar_picker::subsequence`) of the row's
  cells joined by spaces, keeping the order. Confirm focuses the editor, then dispatches the row's
  `then.invocation()` on it as a `RunCommand` (as `vim_line.rs`'s `run_in` does), then dismisses; an
  editor closed by then only dismisses. Dismissed focuses the editor.
- `crates/fe-workbench/src/vim_host.rs`: `HostCommand::Pick(_)` joins the window commands' pattern in
  `run_vim_host_command`, and `run_window_command` shows a new `VimPicker` for it, focused, in the
  workbench's modal layer (`self.modals`) under `ModalId("vim_pick")`, after `dismiss_palette`. It is
  deliberately not a `Palette` variant, so `workbench.rs` (held by the typed-commands chain, Mac
  T696–T706) needs no change.
- `crates/fe-workbench/src/git_commit.rs` and `crates/fe-workbench/src/git_diff.rs`: add
  `| HostCommand::Pick(_)` to the pattern that sends `:Ex`, `:oldfiles`, `:copen`, `:cclose` and
  `:Replace` to `run_window_command`, so the commit box and a diff side open it too. These are the only
  lines they need, now and for every later list command.

Tests:

- New `crates/fe-vim/tests/ex_marks.rs`: nvim's lists above and more, hard-coded as probed (nvim
  version in a comment), on sequences that set the same marks in both (do a jump first, so `'` is set,
  and leave out `"` and file marks in other files, which fe-vim doesn't have).
- New `crates/fe-workbench/src/vim_pick_tests.rs`, declared in the existing
  `crates/fe-workbench/src/vim_host_tests.rs` by `#[path = "vim_pick_tests.rs"] mod pick_tests;`, using
  that file's `window`, `ex`, `draw` and `active`.

ex.rs is also changed by Mac T826, T830, T831 and T833 (helium's ex.rs chain), and vim_host.rs,
vim_host_tests.rs, git_commit.rs and git_diff.rs by Mac T833 (`:r`, a `HostCommand::Read`), which waits on
Mac T795, T797 and T810 for the git files: this card depends on T833 and T822, so start on a trunk with
them landed.
