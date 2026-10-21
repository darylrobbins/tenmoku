---
task_uid: "01m5f7cbry8jbzzsw6833xfr73"
task: "T257"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "In ///, //! and /** */ comments, code spans and their backticks are comment.documentation, with no markup.raw, punctuation.delimiter or escape spans"
    required: true
    manual: false
  - id: "C2"
    text: "Intra-doc link text is markup.link"
    required: true
    manual: false
  - id: "C3"
    text: "Code spans in .md files are still markup.raw"
    required: true
    manual: false
  - id: "C4"
    text: "rust.rs.golden is updated"
    required: true
    manual: false
---

# Keep Rust doc-comment code spans, backticks and escapes in the comment colour as VS Code does, highlighting only link text (Mac T1025)

## Problem (user bug: "doesn't look like VS Code", fastedit vs VS Code screenshots of crates/fastedit/src/config.rs)

T1015 (dde62977) injects `markdown_inline` into Rust doc comments
(`crates/fe-syntax/queries/rust-injections.scm`). The tree-sitter-md inline query then colours:
- code spans (`text.literal` → `markup.raw`: Dark Modern #CE9178 orange, Dark 2026 #79C0FF);
- backticks and link brackets (`punctuation.delimiter`);
- escapes (`string.escape`), emphasis and strong;
- link text (`text.reference` → `markup.link`).

Pixels sampled from the user's screenshots:
- fastedit: doc prose is #6A9955 (comment, correct) and code spans are #CE9178.
- VS Code with rust-analyzer: doc prose AND code spans with their backticks are the comment colour
  (Dark 2026 #8B949E). Only intra-doc links (`[`ExtensionGrammar`]`) are coloured, by the kind of
  symbol they resolve to (#4EC9B0, a struct), from RA semantic tokens, which T946 brings.

## Fix

Inside Rust doc comments, only link text is highlighted. Everything else keeps the
`comment.documentation`/`comment` colour.

- Add a hidden language, `rustdoc_inline`, that uses the `markdown_inline` grammar with a
  fastedit highlights query in a new file, `crates/fe-syntax/queries/rustdoc-inline-highlights.scm`.
  The query captures only `(link_text)`, `(link_label)` and `(shortcut_link (link_text))` as
  `@text.reference`.
- Point `rust-injections.scm` at that language.
- Keep `markdown_inline` unchanged, so `.md` files still colour code spans.
- Until T946 lands, link text uses `markup.link` as a stand-in. With semantic tokens, RA's
  tokens colour it by kind, over it.
- Lead's alternative, if preferred: drop the injection altogether, which makes everything comment
  colour. VS Code without semantic tokens looks like that.

## Tests

- New `crates/fe-syntax/tests/rust_doc_comment_colours.rs`:
  - `` /// Uses `Vec` and [`Option`], \_ *e*. `` has no `markup.raw`, `punctuation.delimiter`,
    `escape`, `markup.italic`/`emphasis` spans.
  - `Vec` and the backticks are `comment.documentation`.
  - `Option` is `markup.link`.
  - A `.md` buffer still has `markup.raw` for a code span.
- Update the `markup.raw` expectations in `crates/fe-syntax/tests/rust_doc_markdown.rs` and the
  golden `crates/fe-syntax/tests/golden/rust.rs.golden`.

## Implementation notes

- `rustdoc_inline` is a hidden language on the `markdown_inline` grammar with no injections (so no HTML in doc comments).
- Link text includes its backticks (`[`Option`]` is one `link_text`), so those are `markup.link` too.
- Known limit (T237 F1): consecutive doc lines are separate layers. The injection engine combines siblings of one parent, and each `doc_comment` has its own `line_comment` parent; grouping a run needs an engine change in `src/injection.rs`, outside this scope. With code spans in the comment colour, only a link wrapping across lines is affected.
