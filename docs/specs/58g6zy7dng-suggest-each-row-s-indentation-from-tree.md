---
task_uid: "01m4s6vs6vkwqy7e58g6zy7dng"
task: "T96"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "indent: for rust, javascript, typescript, go, c, cpp, java, json, lua and css, a new row after an opener ({, [, () suggests the opener row plus one level, a row inside a block the block's level, and a row after a finished one-line statement the same level"
    required: true
    manual: false
  - id: "C2"
    text: "indent: in python a row after a def/if/for line ending in : suggests one level more, and a row after return or pass suggests one level less than that line (@indent.dedent)"
    required: true
    manual: false
  - id: "C3"
    text: "indent: closer_outdents for a row starting with } (and ] or )) names its opener's row; for a row starting with else in C-like languages it names the if's row (@indent.branch)"
    required: true
    manual: false
  - id: "C4"
    text: "indent: no tree, a language without an indents query, or a row in an injected layer gives None; each indents file names its nvim-treesitter source, commit and licence and has a THIRD_PARTY.md row"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-syntax and cargo clippy -p fe-syntax --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Suggest each row's indentation from tree-sitter indent queries in fe-syntax: indents.scm for twelve languages, adapted from nvim-treesitter (Mac T519)

## Problem

A new line copies the previous line's indentation (`EditorModel::newline`, crates/fe-editor/src/lib.rs:992), so
after `{`, `:` or `(` the user indents by hand, and `}` isn't outdented. Day-one audit 2026-10-12: language-aware
auto-indent is table stakes. fe-syntax has no indent rules; this card adds them.

## Change

New module `crates/fe-syntax/src/indent.rs`, exported from `crates/fe-syntax/src/lib.rs`, with indent queries in
`crates/fe-syntax/queries/indents/<language>.scm` for rust, python, javascript, typescript, tsx, go, c, cpp, java,
json, lua and css, loaded per language with `include_str!` as outline.rs does (language.rs untouched).

- Queries are adapted from nvim-treesitter's `queries/<lang>/indents.scm` (Apache-2.0), predicates translated,
  each file headed with its source, commit and licence, with rows in `THIRD_PARTY.md`. Never Helix or Zed.
- Captures supported: `@indent.begin`, `@indent.end`, `@indent.branch`, `@indent.dedent`, `@indent.auto`,
  `@indent.ignore`, `@indent.zero`. `@indent.align` isn't (known limit in the docs).
- `suggest_indent(syntax: &Syntax, text: &Text, row: u32) -> Option<IndentSuggestion>`: `IndentSuggestion::Relative {
  basis_row: u32, delta: i32 }` ("this row's indentation is `basis_row`'s plus `delta` levels") or
  `IndentSuggestion::Zero` (`@indent.zero`). Computed from the tree as it stands (the caller passes a syntax that has seen the edit; for a new
  empty row, the row before ends the context). `None` without a tree or a query for the language, so callers keep
  copying the indentation.
- `closer_outdents(syntax, text, row) -> Option<u32>`: when the row's first non-blank char is a node captured
  `@indent.end` or `@indent.branch`, the row it should align with (its opener's).
- Injected layers aren't looked at (known limit, as `bracket_pair` documents).

## Tests

New file `crates/fe-syntax/tests/indent.rs`: per language, rows after an opener, inside a block, at a closer, after
a one-line statement, and Python's `:` blocks.

Lead note (rev 2): three things before finishing.
1. Rebase onto main 9bcd335. It carries mac/main 3eec614a and Mac T456's provenance test, which checks every query file's source header and its THIRD_PARTY.md row, so every new queries/indents/*.scm must pass it.
2. Fix review F8 (C1): track bracket continuation, the inherited body level and a loop's own body level separately. Inside a multi-line loop header's brackets, keep the inherited level (a blank row after `for (` inside an unbraced if-body gets +1, and the closing paren keeps its level). Turn on the loop-body level only after the header closes. Add the C example from F8 as a regression test.
3. Fix review F12: the ERROR-node scan must be linear, as documented. Track only the three closer kinds in a fixed-size set or bit mask, not a growing list with linear `contains`. Add a wide malformed-container regression: N `(`, `;`, N `)`, N `,`.

## Implementation notes

- Queries are from nvim-treesitter commit `1a4f47b03dfe92d28de7d5654c67327ef008c9e6`. JavaScript's is the
  shared `ecma` file (`javascript.scm`) plus `jsx.scm`; TypeScript adds `typescript.scm`; TSX all three, with
  `tsx.scm` holding only its header. C++ is `c.scm` then `cpp.scm`.
- The level is nvim-treesitter's walk; the basis row is where the nearest node around the row's anchor (its first
  node, or for a blank row the last node of the row before) starts, among nodes starting on an earlier row at the
  first non-blank char. Inside an ERROR the previous non-blank row is tried first, since error trees are flat.
- Brackets are handled in `indent.rs`, for every language: a node with an opening `(`, `[` or `{` child begins from
  the opener's row to its closer, the closer is a branch and `closer_outdents` names the opener's row; an unclosed
  opener in an error begins every row after it. The scan is linear in the children, with no cap. An error's
  brackets are matched in order with a stack per kind, so each closer names its own opener and only the openers
  still open at a row indent it, a level per row. The rows already counted are hash sets, so many openers on many
  rows stay linear (`wide_malformed_brackets_on_many_rows`). A begin capture on such a node begins only the rows after its brackets close (a C `for`'s
  unbraced body), while a same-line begin inherited from the parent (an unbraced `if` body) holds inside them too,
  so a loop header over several rows keeps the `if` body's level and the loop's starts after its `)`. A dedent capture on it (Go's
  switches) keeps the brackets from indenting, though `closer_outdents` still names the opener. The queries'
  captures on header nodes whose bodies are bracketed (Rust items, TypeScript `enum`/`interface`), and on openers as
  branches, are dropped, so a `{` on a row of its own lines up with its header.
- After an `@indent.end` token the walk goes on from the construct it ends, not the node at the blank row, so the
  blocks still open around it hold in a file whose `}` isn't typed yet.
- Other differences from nvim-treesitter, in `indent.rs`'s module docs: a node that ended on an earlier row doesn't
  begin unless immediate (Python blocks, switch cases) or itself unfinished: an error, a token loose in one, or a
  construct whose end (its last child, recursively) is missing or an error. An error elsewhere, in its parent or
  before its end (a malformed loop header), doesn't count; a dedent applies inside its node, or with `indent.immediate`
  only after it (Python's `return`, `pass`, `break`, C's `break` in a case); rows add and take away a level at most
  once each; a begin with `start_at_same_line` covers the node's rows from its first, counted from its parent's row
  (unbraced `if`/`else` bodies, so a nested unbraced loop adds its own level, and `else`, which is a branch and a
  same-row begin so it closes without changing its level); `indent.immediate` on an `else` or a condition's `)`
  indents a new row after it when the body is unbraced; `@indent.ignore` keeps the previous row rather than
  zero; `@indent.zero` applies only to a row starting with the node.
- Captures come from the query's matches rooted at each node on the walk (`set_max_start_depth(0)`), not from a
  byte range, so a long row costs no more than a short one. An error with over 256 children skips the patterns rooted
  at an error: tree-sitter's matching of their children is quadratic, and a malformed file's flat error can hold
  every token (`wide_malformed_brackets`: 150,000 tokens, about 20 ms a row in release).
- `indent.rs` has a THIRD_PARTY.md row of its own, for the walk ported from nvim-treesitter's `indent.lua`, so the
  provenance test (Mac T456) passes for it and every `queries/indents/*.scm`.
- Injected rows are found with the grammar crates' own injections queries (JavaScript's for JS/TS/TSX, under either
  feature; Lua's), counting only injections whose language is in this build. The TypeScript-only build is covered by
  a unit test in `indent.rs`: `cargo test -p fe-syntax --no-default-features --features typescript,css --lib indent`.
- `tests/indent.rs` checks well-indented samples per language, with brackets of every kind on the opener's row and
  on rows of their own: each row's own suggestion matches its indentation, and a blank row inserted after each row
  gets the next row's indentation (a level more before a closer), except a few listed ambiguous rows.
