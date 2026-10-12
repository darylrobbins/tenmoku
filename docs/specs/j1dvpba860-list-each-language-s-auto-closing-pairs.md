---
task_uid: "01m4rh46v95a0hqqj1dvpba860"
task: "T87"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-syntax test auto_pairs::every_language_has_an_entry: auto_pairs(name) matches the spec's table for every name language_names() yields, and markdown_inline is empty"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test auto_pairs::values: rust has () [] {} \"\" and no ''; python adds ''; javascript adds backticks; markdown has () [] {} \"\" and backticks but no ''; an unknown name gets () [] {} \"\""
    required: true
    manual: false
  - id: "C3"
    text: "lib.rs changes only by auto_pairs in the comment re-export; comment_tokens and its tests are unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-syntax, cargo clippy -p fe-syntax --all-targets -- -D warnings and scripts/check-syntax-features.sh pass"
    required: true
    manual: false
---

# List each language's auto-closing pairs in fe-syntax: auto_pairs for every language in the table, which T449's landing left out (Mac T496)

## Problem

Auto-closing brackets and quotes (user decision 2026-10-12; T477) needs each language's pairs, and nothing
provides them. T449 was to add both `comment_tokens` and `auto_pairs` to fe-syntax, but its host landing
(422a0f80, "(Mac T449) (T22)") added only `comment_tokens` (`crates/fe-syntax/src/comment.rs`), and T449 is
abandoned. T477's spec still calls `fe_syntax::auto_pairs(language)` (its "Change", and `ViewHost` answering
`auto_pairs`), and its dependency on T449 can never be satisfied. `rg auto_pairs crates` finds nothing.

## Change

In `crates/fe-syntax/src/comment.rs`, next to `comment_tokens`, and re-exported from `crates/fe-syntax/src/lib.rs`
(`pub use comment::{CommentTokens, auto_pairs, comment_tokens};`, the only change to lib.rs):

`pub fn auto_pairs(language: &str) -> &'static [(char, char)]`: the pairs typing an opener closes, by the names
`language_names()` gives (and `markdown_inline`), whether or not the language's Cargo feature is on. An unknown
name (plain text) gets `()`, `[]`, `{}`, `""`. Every language except `markdown_inline` gets `()`, `[]` and `{}`,
plus:

| Languages | quotes |
|---|---|
| rust | `""` (no `''`: lifetimes) |
| python, javascript, typescript, tsx, php, ruby, bash, lua, sql, toml, yaml, css, html, xml, dockerfile, make, scala, kotlin, swift, csharp, java, c, cpp, go, json | `""`, `''` |
| javascript, typescript, tsx, go, bash | also `` ` ` `` |
| markdown | `""` and `` ` ` `` only (no `''`: apostrophes in prose) |
| markdown_inline | none, and no brackets |

Where a language appears twice the rows add up: JavaScript has `""`, `''` and backticks. Pairs are listed
brackets first, in the order `()`, `[]`, `{}`, then quotes. The table is a `match` like `comment_tokens`, with an
arm for every name in `DEFS` (hidden ones included), so a new grammar without an entry fails the test below.

T477 is unchanged by this card; the lead points its dependency here instead of T449.

## Tests

New file `crates/fe-syntax/tests/auto_pairs.rs`:

- `every_language_has_an_entry`: for every name `language_names()` yields, `auto_pairs(name)` is not the
  unknown-name default unless the table says so (compare against the table for each name), and `markdown_inline`
  is empty.
- `values`: the rows above for rust, python, javascript, markdown, json, and an unknown name.
