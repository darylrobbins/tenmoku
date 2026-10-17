---
task_uid: "01m55bwqzw57rphtrc19mh5s7y"
task: "T138"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-terminal test bracketed_and_dotted_locations_link_to_their_line_and_column (new crates/fe-terminal/tests/link_bracket_suffixes.rs, top-level fn, base API only, one-row snapshots as tests/file_url_lines.rs builds them) passes: with an existing \u003ctemp\u003e/src/main.rs, link_at over the path in 'error at src/main.rs{suffix} here' gives LinkTarget::File { path: that file, line, column } covering the path plus the suffix for '(12, 5)' '(12,5)' '(12:5)' '[12,5]' '[12, 5]' '[12:5]' ':12.5' ':12:5' (line 12, column 5) and '(12)' '[12]' (line 12, no column); ':12.' gives line 12 covering ':12' only; '(12,  5)' (two blanks), '[12' and '[]' give the path alone with no line; it fails at the base ('(12, 5)' links the path alone)"
    required: true
    manual: false
  - id: "C2"
    text: "Each bracket closes only with its own closer; the forms the base already takes (:N, :N:M, (N), (N,M), a quoted path's \", line N[, column M]\", file:// URLs with :N[:M]) give the targets and spans they give at the base; no I/O is added; the module doc lists the new forms"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-terminal passes with no existing test edited (tests/links.rs, file_url_lines.rs, link_marks.rs and link_line_words.rs among them); cargo clippy -p fe-terminal --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Take a path's line and column from (N, M), [N], [N,M], [N, M], [N:M] and :N.M in terminal links, as VS Code's terminal does, instead of linking the path alone (Mac T806)

Filed on argon from Mac T806 (card ~/coordination/argon-cards/zzj/T806.json rev 1; note zzj). Mac task uid 01m55ahhg8a7068045ymedb6ns. Base: argon trunk e9d82a2 (= mac/main 76125d0 synced); the files it changes are byte-identical on Mac main 92c57c57. Title the host commit "... (Mac T806)".

## Problem

VS Code's terminal reads a path's line and column from more suffix forms than fe-terminal's links do.
Its link-suffix regex (workbench.desktop.main.js in the installed VS Code, the `ZKn` builder) has the clause
`:? ?[\[\(]ROW(?:(?:, ?|:)COL)?[\]\)]`, so `foo(339, 12)`, `foo(339:12)`, `foo[339]`, `foo[339,12]`,
`foo[339, 12]` and `foo[339:12]` all open at their line and column. A second clause, `[:.]COL`, makes
`foo:339.12` line 339, column 12.

`crates/fe-terminal/src/links.rs` `location()` takes only `:N[:M]` and `(N[,M])` with no blank after the comma.
At Mac main 92c57c57, on a one-row snapshot of `error at src/main.rs(12, 5) here` (where src/main.rs exists),
`link_at` gives a link over `src/main.rs` alone with no line. This is because the `(` branch fails on the blank
and drops the whole suffix. `[12]` and `[12,5]` are ignored the same way. `src/main.rs:12.5` links to line 12
with no column, and the link stops before `.5`. MSVC prints `file.cpp(12, 5)` with no blank but some tools
add one, and Rust's and Go's test output use `file:line:col`. Square brackets come from several linters and
from Delphi/FPC.

links.rs is byte-identical at mac/main ecdf8603 and Mac main 92c57c57, so no sync is needed.

## Change

In `location()` in `crates/fe-terminal/src/links.rs`:

- Handle `[` like `(`, each closed by its own bracket only (`)` for `(`, `]` for `[`; a mismatched or
  missing closer is no location, as now).
- Inside either bracket, the column may follow `,`, `, ` (exactly one blank) or `:`.
- After `:N`, a `.M` gives the column as `:M` does. A `.` with no digit after it ends the location at `N`, so
  `see src/main.rs:12.` still links `src/main.rs:12`.

The link covers the path and the whole suffix, closing bracket included, as `(N,M)` does today. Update the
module doc's list of forms. Don't add VS Code's other clauses (`#N`, ` N`, `, N`, `on line N`, ranges, a blank
before the bracket). file:// URLs go through `location()` too (`with_url_location`), so `file:///x.mjs:3.9`
gains the column as well.
