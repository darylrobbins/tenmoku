---
task_uid: "01m4tjjmz7emrzqm6v3txzeh06"
task: "T114"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-syntax test highlight_flat_windows::go_window_starting_on_a_blank_row_keeps_its_highlights: in the Go text of 1100 functions, the highlights of the 20 rows from blank rows 1, 3 and 1001 equal those of a query starting one row earlier, cut to the same range, and are non-empty"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test highlight_flat_windows::bash_window_starting_on_a_blank_row_keeps_its_highlights: the same holds for the bash text of 1100 echo lines"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax test highlight_flat_windows::python_window_starting_on_a_blank_row_keeps_its_highlights passes, and the existing fe-syntax tests (flat_nodes_highlight_as_one_query_would, injection tests, golden tests) pass unchanged; fold.rs is not changed"
    required: true
    manual: false
---

# Highlight a window that starts on a blank row in fe-syntax files with over 1024 top-level items, where tree-sitter's child-for-byte search gives up inside a hidden node, as in Go and bash (Mac T653)

## Problem

In a Go or bash file with more than 1024 top-level items (`FLAT_CHILDREN` in crates/fe-syntax/src/syntax.rs),
`Syntax::highlights` returns no highlights at all for a range that starts on a blank row. fe-gpui asks for the
visible rows, so scrolling a blank row to the top of the view turns the whole screen plain. Large generated Go
files (protobuf, mocks, tables) and long shell scripts hit it; blank rows between functions are everywhere.

`for_each_query_root` queries a node with many children a child at a time. It takes "no child" from
`children.goto_first_child_for_byte(range.start)` to mean nothing under the node reaches the range, and returns.
tree-sitter's child-for-goal search (`tree_cursor.c`) does not backtrack: it descends into the first hidden
child that ends after the goal and has visible children, and when no visible child inside it qualifies it
answers none without trying the later siblings. In Go and bash the hidden repeat node that holds an item plus
its newline terminator ends on the blank row below it, so the search gives up there. The split node itself is
queried with `set_max_start_depth(0)`, so none of its children is ever queried. The same walk also feeds
injection finding (crates/fe-syntax/src/injection.rs `find`), so injections in that window are lost too. This is
the same tree-sitter behaviour Mac T647 fixes for fold.rs `for_each_candidate`.

Checked 2026-10-13 at Mac e6988aba (syntax.rs is identical on helium main, blob de401a93): for
`"package main\n\n" + "func f() int { return 1 }\n\n" * 1100`, `highlights(row_start(1)..row_start(21))` is
empty, while `highlights(0..row_start(21))` has 50 spans in those rows; the same for every blank row tried
(3, 1001). For `"echo \"$HOME\" # c\n\n" * 1100` in bash, likewise. Python, Rust and markdown are unaffected.
Under 1024 items nothing is split and nothing goes wrong.

## Change

In crates/fe-syntax/src/syntax.rs `for_each_query_root` (`visit`): when `goto_first_child_for_byte(range.start)`
finds no child of a split node, fall back to `goto_first_child()` and step over the children that end at or before
`range.start` (`end_byte() <= range.start`); only if none is left is nothing under that node in range. The fast
path stays for the common case. `flat_paths` may share the same helper (its early stop only costs speed today),
but nothing else changes: results for ranges that work today stay the same, and the existing tests
(`flat_nodes_highlight_as_one_query_would` and the injection tests) pass unchanged. fold.rs is out of scope
(Mac T647).

## Tests

New file crates/fe-syntax/tests/highlight_flat_windows.rs, gated on the go, bash and python features (all
default). Each test builds the text, and for blank rows 1, 3 and 1001 compares the highlights of the 20 rows
from that blank row (a fresh `Syntax::new` + `parse_job().run()` + `finish` per query, spans cut to the range)
with those of a query starting one row earlier, cut to the same range, which must be non-empty:

- `go_window_starting_on_a_blank_row_keeps_its_highlights` (regression, fails at base: left is `[]`) for the Go text above.
- `bash_window_starting_on_a_blank_row_keeps_its_highlights` (regression, fails at base) for the bash text above.
- `python_window_starting_on_a_blank_row_keeps_its_highlights` (control, passes at base) for
  `"def f(x): return None\n\n" * 1100`.
