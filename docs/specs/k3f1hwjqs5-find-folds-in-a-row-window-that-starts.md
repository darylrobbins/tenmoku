---
task_uid: "01m4tej1hgp7mg5hk3f1hwjqs5"
task: "T113"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-syntax test fold_windows::go_window_starting_on_a_blank_row_keeps_its_folds: in the Go sample, fold_ranges(1..11) equals fold_ranges(0..11) and is non-empty, and fold_ranges(7..11) equals fold_ranges(8..11) and is non-empty"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test fold_windows::bash_and_markdown_windows_starting_on_a_blank_row_keep_their_folds: in the bash and markdown samples, fold_ranges(3..7) equals the non-empty fold_ranges(4..7)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax test fold_windows::rust_window_starting_on_a_blank_row_keeps_its_folds passes; crates/fe-syntax/src/syntax.rs is unchanged and the existing fold and golden tests pass unchanged"
    required: true
    manual: false
---

# Find folds in a row window that starts on a blank row in fe-syntax, where tree-sitter's child-for-point search gives up inside a hidden node, as in Go, bash and markdown (Mac T647)

## Problem

`fold_ranges` (crates/fe-syntax/src/fold.rs) finds no fold at all in a row window that starts on a blank
row after a terminated statement, in Go, bash and markdown. fe-gpui's `fold_headers` calls it with the
visible rows, so scrolling such a blank row to the top of the view drops every fold chevron in the view.

The candidate walk `for_each_candidate` takes "no child" from `cursor.goto_first_child_for_point(start)`
to mean no child overlaps `rows`, and climbs out. tree-sitter 0.27's implementation (`tree_cursor.c`
`ts_tree_cursor_goto_first_child_for_point`) does not backtrack: it descends into the first hidden child
that ends after the goal point and has visible children, and when no visible child inside it qualifies it
returns none, without trying the later siblings. In Go that hidden node is a `source_file_repeat1` holding
`package_clause` plus its newline terminator, lexed on the blank row, so it ends at the next row and
swallows the search; `goto_first_child_for_point((1,0))` on the root returns none while `(2,0)` returns
`import_declaration`. Rust, Python, JS and the other default grammars are unaffected.

Checked 2026-10-13 at Mac e6988aba (fold.rs is identical on helium main df738a8): for the Go text
`package p\n\nfunc f() {\n\ta()\n\n\tb()\n}\n\nfunc g() {\n\tc()\n}\n`, `fold_ranges(0..11)` is
`[2..=5, 8..=9]` but `fold_ranges(1..11)` is `[]`; for the bash text `f() {\n  echo a\n}\n\ng() {\n  echo b\n}\n`,
`fold_ranges(4..7)` is `[4..=5]` but `fold_ranges(3..7)` is `[]`. Markdown shows the same with
`# A\n\ntext\n\n## B\n\nmore\n` (per a brute-force walk over the golden samples, every window starting
on such a blank row fails in Go, bash and markdown).

## Change

In crates/fe-syntax/src/fold.rs `for_each_candidate`: when `goto_first_child_for_point(start)` finds no
child of a node that has children, fall back to `goto_first_child()` and step over the children that end
at or before `start` (`end_position() <= start`); only if none is left is nothing in `rows` under that
node. The fast path for the common case stays. Results for windows that work today do not change; the
existing fold tests (in-crate and golden) pass unchanged. syntax.rs (`for_each_query_root`, which uses
`goto_first_child_for_byte` the same way) is out of scope.

## Tests

New file crates/fe-syntax/tests/fold_windows.rs, gated on the go, bash, markdown and rust features (all
default), parsing with `Syntax::new` + `parse_job().run()` + `finish`:

- `go_window_starting_on_a_blank_row_keeps_its_folds` (regression, fails at base): for the Go text above,
  `fold_ranges(1..11)` equals `fold_ranges(0..11)` and is non-empty, and `fold_ranges(7..11)` equals
  `fold_ranges(8..11)` and is non-empty.
- `bash_and_markdown_windows_starting_on_a_blank_row_keep_their_folds` (regression, fails at base): for
  the bash and markdown texts above, `fold_ranges(3..7)` equals `fold_ranges(4..7)`, which is non-empty.
- `rust_window_starting_on_a_blank_row_keeps_its_folds` (control): the same holds in Rust for
  `fn f() {\n    a();\n}\n\nfn g() {\n    b();\n}\n`.
