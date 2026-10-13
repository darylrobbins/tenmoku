---
task_uid: "01m4saf3k1b55b53ry576xawhm"
task: "T67"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor test comment::tests::line_comments: with // over \"  a\\n\\n    b\", toggle gives \"  // a\\n\\n  //   b\" and toggling again restores the text exactly; a caret after the inserted token stays on the same character"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test comment::tests::mixed_rows_comment_all: over \"// a\\nb\" toggle gives \"// // a\\n// b\""
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test comment::tests::block_only_and_none: with only /* */ the rows become \"/* a */\" and toggle back; with no tokens nothing changes and the command reports Effect::Nothing; one undo reverts a toggle"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test commands::comment_host_tests::tokens_follow_the_language: editor.toggle_comment comments with # in a Python view and // in a Rust view, and does nothing in plain text"
    required: true
    manual: false
  - id: "C5"
    text: "crates/fe-editor/src/tests/commands.rs lists editor.toggle_comment as mutating, and cargo test -p fe-editor -p fe-gpui and cargo clippy on both --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Toggle line comments over the selected rows with editor.toggle_comment, from the buffer's language, as one undo step (Mac T459)

Filed on helium from Mac T459 (~/coordination/helium-cards/T459.json; Mac lead message ~/coordination/from-mac-lead-2026-10-13-helium-zf.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

There is no toggle-comment command: the editor registry (`crates/fe-editor/src/commands.rs` `registry()`, :236)
has none, and fe-editor doesn't know the buffer's language (it doesn't depend on fe-syntax). Commenting rows out is
basic editing for a code editor; the plan's M6 is "Editing polish" (plan line 110). The previous cards add row
commands (rows.rs) and each language's comment delimiters (`fe_syntax::comment_tokens`).

## Change

fe-editor:

- `pub struct CommentTokens { pub line: Option<&'static str>, pub block: Option<(&'static str, &'static str)> }` in
  a new module `crates/fe-editor/src/comment.rs` (`mod comment;`, exported from lib.rs). fe-editor still doesn't
  depend on fe-syntax.
- `EditorHost` (commands.rs :27) gets `fn comment_tokens(&self) -> Option<CommentTokens> { None }`.
- `EditorModel::toggle_comment(tokens: CommentTokens)` and command `editor.toggle_comment` ("Toggle Line Comment",
  mutating), registered by `comment::register` from `commands::registry()`, which asks the host for the tokens. No
  tokens, or neither delimiter: no edit, `Effect::Nothing`.
- The rows are those the selections touch, as rows.rs defines it; blank rows are skipped. All edits are one
  transaction, and selections stay on the same text (anchored), as in rows.rs.
- With a line token: if every non-blank row, after its leading whitespace, starts with the token, each loses the
  token and one space after it if there is one. Otherwise every non-blank row gets `token + " "` inserted at the
  smallest leading-whitespace length (in chars) among those rows, so the comment markers line up. Rows that already
  start with the token are commented again (`// // a`), so toggling twice restores the text exactly.
- With only a block pair (CSS, HTML, XML, Markdown): each non-blank row's text after its indentation becomes
  `open + " " + text + " " + close`; if every non-blank row already starts with `open` and ends with `close`
  (ignoring surrounding whitespace), they're unwrapped, with one space inside each delimiter removed where present.

fe-gpui: `ViewHost` (`crates/fe-gpui/src/commands.rs` :178) implements `comment_tokens` from
`self.syntax.map(|s| fe_syntax::comment_tokens(s.language().name()))`, converting to fe-editor's type. A view whose
language is still loading, or plain text, has none.

Not in this card: key bindings and menus (a later card binds `editor.toggle_comment` to secondary-/).

## Tests

New files `crates/fe-editor/src/comment_tests.rs` (declared from comment.rs with `#[path]`, using
`crate::tests::{parse, render}`) and `crates/fe-gpui/src/comment_host_tests.rs` (declared from commands.rs with
`#[path]`). Update the expected command list in `crates/fe-editor/src/tests/commands.rs`.
