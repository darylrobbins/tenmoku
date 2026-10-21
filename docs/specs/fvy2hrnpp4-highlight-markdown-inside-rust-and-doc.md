---
task_uid: "01m5dyp7k66fqn40fvy2hrnpp4"
task: "T237"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-syntax test rust_doc_markdown (new file): backtick spans in ///, //! and /** */ comments are markup.raw, [`x`] links are markup.link, surrounding prose stays comment.documentation, a // comment is untouched, macro bodies are still highlighted as Rust, an unterminated backtick does not leak past its comment"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax and clippy pass; the existing fe-syntax golden and injection tests pass with ONLY the Rust golden regenerated, and its diff changes nothing but the spans of the doc-comment line (the backtick span in line 29's /// comment); no other golden file changes"
    required: true
    manual: false
---

# Highlight Markdown inside Rust /// and //! doc comments: code spans and links via an injected markdown_inline layer, prose keeping the comment colour (Mac T1015)

## Problem

The user asked how syntax highlighting gets as good as VS Code's and approved Markdown in Rust doc comments (2026-10-20). VS Code colours a backtick code span, a [`link`] and emphasis inside `///` and `//!` comments; fastedit paints the whole comment `comment.documentation`. fe-syntax already has an injection engine (`crates/fe-syntax/src/injection.rs`) that Markdown, HTML, JS and PHP use, and a hidden `markdown_inline` language; Rust's `injections` is `None` (`crates/fe-syntax/src/language.rs`, the `rust` Def), so doc comments get no injection.

## Change

1. New `crates/fe-syntax/queries/rust-injections.scm`, used by the `rust` Def in place of tree-sitter-rust's, as `php-injections.scm` is for PHP: header comment naming tree-sitter-rust 0.24.2 `queries/injections.scm` (MIT) and the change; keep its two token-tree patterns (macro bodies stay Rust) and add
   `((doc_comment) @injection.content (#set! injection.language "markdown_inline") (#set! injection.combined))`
   so the lines of one comment run parse as one inline-Markdown layer (a code span may wrap lines). Check the node names against the grammar (`line_comment`/`block_comment` > `doc_comment`, between the `///` marker and the end).
2. `crates/fe-syntax/src/language.rs`: the `rust` Def's `injections` is `Some(include_str!("../queries/rust-injections.scm"))`.
3. Only the injected spans change colour: prose keeps `comment.documentation`; code spans take `markup.raw`, links `markup.link`/`markup.link.url`, as in Markdown files. If the inline grammar paints plain doc text (check its `HIGHLIGHT_QUERY_INLINE` captures), clear it so prose stays the comment colour. Rust files without doc comments highlight exactly as before (existing golden `tests/golden/` Rust sample, if any, unchanged).
4. Tests: new `crates/fe-syntax/tests/rust_doc_markdown.rs`: for `/// Uses `Vec` and [`Option`].`, `//! Crate `x`.` and a `/** ... `y` */` block, the backtick bytes are `markup.raw` and the surrounding prose is `comment.documentation`; a `//` plain comment is untouched; a macro body is still highlighted as Rust; an unterminated backtick does not leak past its comment.

## Lead decisions (2026-10-21, after the first worker run)

- **The golden moves, by design.** `crates/fe-syntax/tests/golden/rust.rs` line 29 has a `///` comment with a backtick span (`offset`), so the new injection correctly changes `rust.rs.golden` (the backticks and `offset` inside the doc comment). Regenerate it with `UPDATE_GOLDEN=1` and check the diff is ONLY the doc-comment line's spans; no other golden line may change. The golden files are in scope.
- **Do not take tree-sitter-rust's two token-tree injection patterns.** Injecting macro bodies as Rust changes about twenty golden lines and the outer parse already highlights macros as Rust. The new `rust-injections.scm` has only the `doc_comment` pattern; "macro bodies are still Rust" is satisfied by the outer parse and pinned by the test.
