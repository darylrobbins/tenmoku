---
task_uid: "01m55mapzd5r9417j2n7j96sx0"
task: "T183"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test registers_list_as_neovim (top level, in crates/fe-vim/tests/ex_registers.rs) passes: through HeadlessVim, :reg, :registers a x, :di \"0, :reg b a and :regi a leave HostCommand::Pick with title \"Registers\", header \"Type Name Content\" and rows whose cells equal nvim 0.12.6's probed output (order, type c/l/b for a charwise, linewise and \u003cC-v\u003e yank, \"x name, content with ^J for line breaks and ^I, ^[ for control characters), hard-coded with the nvim version in a comment; :reg ! and :reg q with q unset leave a pick with no rows; :reg! gives E477 and :2reg E481."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test picking_a_register_puts_as_quote_p (top level, same file) passes: for a charwise, a linewise and a block register row, HeadlessVim::dispatch of the row's then.invocation() leaves the text, cursor, mode and registers as feeding \"xp does on a twin HeadlessVim."
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test workbench::vim_host_tests::pick_tests::registers_opens_a_picker_and_puts (in crates/fe-workbench/src/vim_pick_tests.rs) passes: in a real workbench with vim on, after \"ayy, typing :reg on the bar's line shows the modal vim_pick with a row for \"a; Enter on it dismisses the picker, focuses the editor and puts the line below the cursor's row, as \"ap does; :di dismissed with Escape changes nothing."
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-vim and cargo test -p fe-workbench pass, with no change under crates/fe-workbench/src other than vim_pick_tests.rs."
    required: true
    manual: false
---

# fe-vim: :reg[isters] and :di[splay] [names] list the registers in a picker with nvim's columns and order, and picking one puts it as "xp does (Mac T866)

## Problem

`:reg[isters]` and `:di[splay]` are E492 in fe-vim at Mac main 76125d07 (listed as not planned). The user
decided on 2026-10-17 that they open a picker, like the command palette, rather than print a list. The
previous card (`:marks`) adds the seam: `HostCommand::Pick`, `crates/fe-vim/src/ex/pick.rs` and the
workbench's `VimPicker` (`crates/fe-workbench/src/vim_pick.rs`), which runs a chosen row's
`PickThen::invocation()` in the editor.

nvim 0.12.6 (`--clean --headless`, `nvim_exec2('registers', {output = true})`), rows as printed:

```
Type Name Content
  c  ""   a
  c  "0   two
  l  "1   gamma three^J
  l  "a   delta^J
  c  "b   a
  l  "x     beta two^Jgamma three^J
  c  "-   a
  c  "*
  c  "+
  c  ".    ins
  c  ":   s/a/A/
  c  "%   f.txt
  c  "#   g.txt
  c  "/   two
```

- The order is `"`, `0`–`9`, `a`–`z`, `-`, `*`, `+`, `.`, `:`, `%`, `#`, `/`, whatever order an argument
  names them in (`:reg ba` and `:reg b a` both list `a` then `b`); an unset register has no row. Type is
  `c`, `l` or `b` (a block yanked with `<C-v>`: `b  "k   al^J  `). The content is the text with each line
  break as `^J` (a linewise register ends in one), other control characters as `^X` (`x^Iy^[z` for
  `x<Tab>y<Esc>z`), leading blanks kept (`"x` above), cut to the window's width.
- `:reg {names}` (blanks ignored, `"` a name) lists only those; none set, or a name that isn't a register
  (`:reg !`), prints the header alone, with no error. `:reg!` is `E477: No ! allowed`; `:2reg`
  `E481: No range allowed` (probe it). Names: `:reg[isters]` (`:re` is `:read`) and `:di[splay]`
  (`:d` is `:delete`).

The user's brief: choosing a register "puts it, as `"xp` does, or just closes (pick nvim-faithful-ish and
sensible)". nvim's `:reg` only prints, so neither is nvim's own; putting is what the pickers users know
do (telescope's and fzf-lua's registers pickers put on select) and what makes the pick worth having, so
choosing a row puts it, as `"xp`; Escape closes with nothing put.

## Change

- `crates/fe-vim/src/ex/pick.rs`: `PickThen::Put(char)`, whose invocation is `vim.put` with
  `{"register": x}` (after the caret, count 1), as `"xp` is.
- New `crates/fe-vim/src/ex/registers.rs`, with two entries in Mac T822's table, `"registers"` shortest 3
  and `"display"` shortest 2 (can't edit), and their lines in `crates/fe-vim/src/ex/table.rs`. Parse: no
  range (E481), no bang (E477), the rest of the line the names. Run: one row per register set, in nvim's
  order, for those fe-vim has, read with `Registers::get` (`"` while it points at a set register,
  `0`–`9`, `a`–`z`, `-`, `/`, and `.` and `:` once `Registers::get` gives them, Mac T836), plus `*` and `+`
  with the clipboard's text, read through the host as `Registers::read` reads them, when it holds any
  (nvim lists both always, empty when it has no clipboard; an empty row is only noise in a picker).
  fe-vim knows no file names and has no expression register: `%`, `#` and `=` have no row. Cells
  `[type, "\"" + name, content]`: `l` for linewise, `b` for a block (`block_width` set), else `c`;
  content through `printable` with line breaks as `^J` (a linewise register's text ends in one, whether
  or not fe-vim stores the final `\n`), cut after 256 characters so a 64 MiB register stays cheap (nvim cuts
  at the window's width; the picker clips what's wider). `then` `PickThen::Put(name)`. It leaves
  `HostCommand::Pick(Pick { title: "Registers", header: "Type Name Content", rows })`, rows or none.
- No change in fe-workbench's code: the seam shows the pick and runs the put.

Tests: a new `crates/fe-vim/tests/ex_registers.rs`, nvim's lists hard-coded as probed (nvim version in a
comment), on sequences that set the same registers in both (leave out `.`, `:`, `%`, `#` and the
clipboard, which differ); and a test in `crates/fe-workbench/src/vim_pick_tests.rs` (the `:marks`
card's).
