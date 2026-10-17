---
task_uid: "01m53yvdeyakpfrb76byh1m84q"
task: "T125"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-terminal test link_line_words::quoted_paths_take_a_line_and_column_in_words passes: a quoted path followed by \", line 12\" links with line 12 and no column, \", line 3, column 9\" and \", line 4, col 2\" carry both, each spanning from the path through its last digit; a quoted path followed by \", in \u003cmodule\u003e\" links with no line, spanning only the path; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-terminal passes with no existing test edited (tests/links.rs unchanged); cargo clippy -p fe-terminal --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Take the line and column from a quoted path's ", line N[, column M]" in terminal links, so a Python traceback opens at its row as VS Code does (Mac T734)

## Problem

The terminal's path links (`crates/fe-terminal/src/links.rs`, `path_at` and `location`) take a line and
column only from a `:line[:column]` or `(line[,column])` suffix right after the path. Python's
tracebacks, and other tools that copy them, quote the path and name the line in words:

```
  File "/Users/me/app/main.py", line 12, in <module>
  File "src/tool.py", line 3, column 9
```

At c5e93ed6, cmd-clicking `/Users/me/app/main.py` there finds the file (the path run stops at the `"`)
but `location` sees `"` after it and returns no line, so the file opens at its top instead of row 12,
and the underline stops at the path. VS Code's terminal (`terminalLinkParsing.ts` link suffixes)
recognises `"foo", line 339`, `"foo", line 339, col 12` and `"foo", line 339, column 12`.

## Change

- `crates/fe-terminal/src/links.rs` `location` (or `path_at`, its only caller): when the character before
  the path run is `"` and the run ends at a `"`, also accept, after that closing quote, `,` then any
  spaces, the word `line`, one or more spaces and a line number; then optionally `,`, any spaces,
  `column` or `col`, one or more spaces and a column number. The link's span runs from the path's first
  character through the last digit taken (the closing quote and words included); the opening quote stays
  outside it. Words are matched as written (lower case), and a suffix that doesn't complete (`", line`,
  `", line x`, `", in <module>`) leaves the path link as today, with no line and the span on the path.
  Nothing else changes: the `:` and `(` suffixes, URLs, OSC 8 and `existing_path` stay as they are, and a
  quoted path with a line still isn't dot-trimmed.
- Update the module doc's list of suffixes to include `"path", line N[, column M]`.
- New test file `crates/fe-terminal/tests/link_line_words.rs` (its own `Session`/`assert_link` helpers,
  copied from `tests/links.rs`, which it doesn't edit), one test
  `quoted_paths_take_a_line_and_column_in_words`: in a temp dir with `src/tool.py`, a `/bin/sh` session
  prints `  File "src/tool.py", line 12, in <module>`, `  File "<abs>/src/tool.py", line 3, column 9`,
  `"src/tool.py", line 4, col 2` and `  File "src/tool.py", in <module>`, and `link_at` on a cell of each
  path gives `File { line: Some(12), column: None }` spanning `src/tool.py", line 12`,
  `Some(3)/Some(9)` spanning through `9`, `Some(4)/Some(2)`, and the last one the path with no line,
  spanning only `src/tool.py`. It fails at the base.
