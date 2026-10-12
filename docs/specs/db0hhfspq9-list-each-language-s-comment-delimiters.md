---
task_uid: "01m4pzqvfwcbf86pdb0hhfspq9"
task: "T22"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-syntax test comment_tokens::every_listed_language_has_tokens: comment_tokens(name) is Some for every name language_names() yields and for markdown_inline, and None for an unknown name"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test comment_tokens::values: rust is // with /* */; python # with no block; lua -- with --[[ ]]; sql -- with /* */; css and html have no line token and /* */ and \u003c!-- --\u003e blocks; json has neither"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-syntax, cargo clippy -p fe-syntax --all-targets -- -D warnings and scripts/check-syntax-features.sh pass"
    required: true
    manual: false
---

# List each language's comment delimiters in fe-syntax: comment_tokens gives the line and block comment tokens of every language in the table (Mac T449)

Filed on helium from Mac T449 (~/coordination/helium-cards/T449.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-i.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

There is no way to know a language's comment syntax. `fe_syntax::Language` and its table (`Def`,
`crates/fe-syntax/src/language.rs` :148) hold a grammar and queries only. A toggle-comment command (the next card,
M6 "Editing polish", plan line 110) needs each language's line comment and block comment delimiters. The plan's reuse
map names lapce-core's language table as a source for language data (line 42); comment delimiters are plain facts
about each language, so this card writes the table itself.

## Change

New module `crates/fe-syntax/src/comment.rs` (`mod comment;` and `pub use comment::{CommentTokens, comment_tokens};`
in lib.rs):

- `#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub struct CommentTokens { pub line: Option<&'static str>, pub block:
  Option<(&'static str, &'static str)> }`.
- `pub fn comment_tokens(language: &str) -> Option<CommentTokens>`, by the names `language_names()` gives: `Some` for
  every language in the table (whether or not its Cargo feature is on), `None` for an unknown name.

The table:

| Languages | line | block |
|---|---|---|
| rust, javascript, typescript, tsx, go, c, cpp, java, php, csharp, scala, swift, kotlin | `//` | `/*` `*/` |
| python, toml, bash, yaml, ruby, dockerfile, make | `#` | none |
| lua | `--` | `--[[` `]]` |
| sql | `--` | `/*` `*/` |
| css | none | `/*` `*/` |
| html, xml, markdown | none | `<!--` `-->` |
| json, markdown_inline | none | none |

A table entry exists for every name in `DEFS`, including hidden ones, so a new grammar without an entry fails the
test below.

## Tests

New file `crates/fe-syntax/tests/comment_tokens.rs`.
