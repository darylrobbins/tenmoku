---
task_uid: "01m55ceg5zr08ght85q9e0earc"
task: "T166"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test read_parses_to_a_host_command (top level, in crates/fe-vim/tests/ex_read_path.rs) passes: :r f.txt, :read f.txt, :3r f.txt, :0r f.txt and :r alone give HostCommand::Read with the path and row the spec says, :r !ls is refused with a message, and :reg stays E492."
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test workbench::vim_host_tests::read_tests::read_puts_a_files_lines_below_the_line (new crates/fe-workbench/src/vim_read_tests.rs, declared by a #[path] mod in the existing crates/fe-workbench/src/vim_host_tests.rs) passes: typed on the bar's line in a real workbench, :r f.txt and :0r f.txt leave the text, cursor and message nvim 0.12.x does (probed, in the spec), each undone by one u, and :r nope.txt leaves E484 and the text unchanged."
    required: true
    manual: false
---

# fe-vim: :r[ead] {file} puts a file's lines below the range's line (:0r at the top), run by the workbench as nvim does, instead of E492 (Mac T833)

## Problem

`:r[ead]` is E492 in fe-vim at Mac main 92c57c57 (listed as not planned). The user approved it on
2026-10-17 in the "Vim breadth" track. fe-vim knows no files; `:e` and `:w` already leave a `HostCommand`
that the workbench runs (`crates/fe-workbench/src/vim_host.rs`, which resolves a relative path under the
project root, else the file's folder, with `resolve`). Probed with `nvim --clean --headless` 0.12.6, a file
`f.txt` holding `one\ntwo\n`:

- `a/b` at (0,0), `:r f.txt`: `a/one/two/b`, cursor (1,0) (the first line read, at its first non-blank),
  and the message `"f.txt" 2L, 8B`.
- then `:0r f.txt`: `one/two/a/one/two/b`, cursor (0,0).
- `:r nope.txt`: `E484: Can't open file nope.txt`, nothing changed.

`:r !cmd` (a shell command's output) needs a shell, which fastedit has no safe seam for: it stays refused.

## Change

- `crates/fe-vim/src/ex.rs`: a new `HostCommand::Read { path: Option<String>, below: u32 }`: `:[line]r[ead]
  [file]` parses (by `host()`, as `:e` does) to the file (none: the buffer's own file, as nvim reads it)
  and the row it goes below, the range's last line (the cursor's by default) with line 0 meaning above the
  first (`below` then a sentinel or make it `Option<u32>`, documented). `:r !…` is refused with a message,
  unchanged text; `:r!` (a bang) is nvim's (probe it).
- `crates/fe-workbench/src/vim_host.rs`: run it for the line's editor: resolve the path as `vim_edit`
  does, read the file (UTF-8; a missing or unreadable file gives `E484: Can't open file {path}` as the
  editor's vim message), and put its lines below the row in one transaction (one `u` undoes it), the cursor
  on the first line read at its first non-blank, and nvim's `"{name}" {N}L, {B}B` message. A read-only
  editor refuses it as it refuses any edit. A file without a final line break still goes in as whole
  lines.
- `crates/fe-workbench/src/git_commit.rs` and `git_diff.rs` match every `HostCommand`: give `Read` the
  answer each gives `:e` there (the commit box's NOT_IN_THE_BOX, the diff's own).

Tests: a new `crates/fe-vim/tests/ex_read_path.rs` for the parse (with and without a file, a range, `0`,
`!cmd` refused, E492 kept for other names starting with `r` that fe-vim lacks, such as `:reg`) and a new
`crates/fe-workbench/src/vim_read_tests.rs`, declared in the existing `crates/fe-workbench/src/vim_host_tests.rs`
by `#[path = "vim_read_tests.rs"] mod read_tests;`, typing `:r f.txt` and `:0r f.txt` on the bar's line
in a real workbench (text, cursor, message, one undo) and `:r nope.txt` (E484).

git_commit.rs and git_diff.rs are held by Mac T795, T797 and T810 (git_diff.rs): this card depends on all three
and on Mac T831 (ex.rs's chain), so start on a trunk that has them.
