---
task_uid: "01m55cedcc34rtwvds8a18gbat"
task: "T163"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test ex_bar_matches_neovim (top level, in crates/fe-vim/tests/ex_bar_oracle.rs) passes on crates/fe-vim/tests/corpus/ex_bar.toml, at least 15 cases recorded with nvim 0.12.x, among them (asserted by name) bar_two_substitutes_then_undo, bar_error_stops_the_rest, bar_in_global_command, bar_in_normal_keys, bar_escaped_in_pattern, bar_after_comment, bar_delete_then_yank, bar_range_then_delete, bar_after_host_command_unchanged."
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::diff_seams_tests::read_only_bar_tests::read_only_refuses_a_bar_line_that_edits (new crates/fe-gpui/src/view/read_only_bar_tests.rs, declared by a #[path] mod in the existing crates/fe-gpui/src/view/diff_seams_tests.rs) passes: on a read-only editor :y | d is refused with E21 and changes nothing, and :y | noh runs; ex_edits in host.rs reads every piece of the line through a pure pub fn on fe_vim::Ex."
    required: true
    manual: false
---

# fe-vim: | separates ex commands on one line, run in turn as one undo step and stopping at the first that fails, as nvim does, instead of E488 or E492 (Mac T826)

## Problem

fe-vim runs one command per ex line at Mac main 92c57c57: a `|` after a command is E488 (trailing
characters) or E492, so `:s/a/A/ | s/c/C/` fails (the crate docs list `|` between commands as not
planned). The user approved it on 2026-10-17 in the "Vim breadth" track. Probed with `nvim --clean
--headless` 0.12.6 (`:help :bar`):

- `abc`, `:s/a/A/ | s/c/C/<CR>`: `AbC`; then `u` gives `abc` back (the whole line is one undo step).
- `abc`, `:s/a/A/ | s/z/Z/ | s/c/C/<CR>`: `Abc` and E486: the command that fails stops the rest.
- `a1/b/a2`, `:g/a/s/a/x/ | s/x/y/<CR>`: `y1/b/y2`: `:g` takes the `|` and what follows as its command.
- `a/b`, `:norm Ax | y<CR>`: `ax | y/b`: `:normal` takes it as keys.
- `a|b`, `:s/a\|b/X/g<CR>`: `X|X`: `\|` in a pattern is the pattern's.
- `abc`, `:s/a/A/ " | s/c/C/<CR>`: `Abc`: a comment runs to the end, bars and all.
- `a/b/c`, `:2d | 1y<CR>p`: `a/c/a`. `a/b/c`, `:2|d<CR>`: `a/c` (`:2`, then `:d` on line 2).

## Change

In `crates/fe-vim/src/ex.rs`, a pure splitter that cuts a line into its commands where nvim does: at a `|`
that isn't escaped (`\|` stays, as the command's), not inside the command's own argument where nvim gives
it the rest of the line (`:g`, `:v`, `:normal`, and every host command, `:w`, `:e`, the capitalised user
commands, `:Replace`: those keep today's behaviour, their argument running to the end, so a `|` after
`:w` is still its path's, or E488 as now), not inside `:s`'s pattern and replacement (up to their
delimiters, as `search::split_search` reads a pattern), and not after a `"` comment. `ex::run` runs the
pieces in order through `execute`, each as now, and stops at the first that fails, leaving its error. The
whole line is one undo step when more than one piece changes the text (reuse `ex/global.rs`'s `Group`,
as `:g` and `:normal` do), and `u`/`<C-r>` put the cursor where nvim's do. A piece that is empty (`:|d`)
is nvim's (probe it). `@:` and the history repeat the whole line.

In `crates/fe-gpui/src/view/host.rs`, `ex_edits` answers true if any piece of the line edits (expose the
splitter as a `pub fn` on `Ex`, for example `Ex::commands(line) -> Vec<&str>`). Test it in a new
`crates/fe-gpui/src/view/read_only_bar_tests.rs`, declared in the existing
`crates/fe-gpui/src/view/diff_seams_tests.rs` by `#[path = "read_only_bar_tests.rs"] mod read_only_bar_tests;`
(reusing its read-only editor helpers): `:y | d` is refused with E21, `:y | noh` runs.

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/ex_bar.toml` (header comment as the
other corpora; an error ends a case), recorded into `ex_bar.expected.json` by the ignored
`record_corpus_with_neovim` in `tests/oracle.rs`, and a new `crates/fe-vim/tests/ex_bar_oracle.rs`
modelled on `tests/ex_lines_oracle.rs`. Leave substitute.rs alone (it is the `:s` flag cards'); lib.rs's
docs are the lead's closing chore.
