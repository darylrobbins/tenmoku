---
task_uid: "01m5qnbgg852pdj7j16d5e1eyc"
task: "T1124"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "New fe-syntax test file tests/rust_doc_code_spans.rs: in ///, //! and /** */ comments a plain backtick span (backticks included) and the code inside [`Config`] take a new highlight distinct from comment.documentation, while prose, list markers and [ ] brackets keep comment.documentation; .md files unchanged. Test a_backtick_span_in_a_doc_comment_is_not_the_prose_colour"
    required: true
    manual: false
  - id: "C2"
    text: "T1025's doc_comment_code_span_keeps_comment_colour in tests/rust_doc_comment_colours.rs is reversed or removed (the user's 2026-10-24 screenshot supersedes its pixel sample). Existing assertions in tests/rust_doc_comment_colours.rs and tests/rust_doc_markdown.rs that pin a backtick span or intra-doc code to comment.documentation (review F1: rust_doc_comment_colours.rs:65,74; rust_doc_markdown.rs:44,55,63) are updated to the new highlight, with their other expectations unchanged; no other existing test edited"
    required: true
    manual: false
  - id: "C3"
    text: "The new highlight resolves in every bundled theme to a colour visibly brighter than its comment colour, close to the default foreground as in the VS Code screenshot; rust.rs.golden updated, other goldens unedited; clippy passes"
    required: true
    manual: false
---

# Colour backtick code spans in Rust doc comments brighter than the prose, as VS Code does, reversing T1025's comment-coloured spans

## Problem
The user (2026-10-24) compared crates/fastedit/src/config.rs lines 27-59 side by side: in VS Code (screenshot ~/.local/share/fastedit-lead/doc-comments-vscode-2026-10-24.png) the doc-comment prose is grey but every backtick span (`settings`, `theme.mode`, `theme.toml`) and the code inside intra-doc links renders in a brighter, code-like foreground; in fastedit (~/.local/share/fastedit-lead/doc-comments-fastedit-2026-10-24.png) all of it is the same comment grey.

## Cause
T1025 (commit 55951fd0) narrowed the injected rustdoc_inline layer (crates/fe-syntax/queries/rustdoc-inline-highlights.scm) so plain code spans keep the comment colour, from a pixel sample that this screenshot contradicts. Its test doc_comment_code_span_keeps_comment_colour (crates/fe-syntax/tests/rust_doc_comment_colours.rs) pins the old behaviour; the user's screenshot is the authority, so that test is reversed here.

## Change
Capture backtick code spans (backticks included) in the rustdoc_inline layer to a new highlight distinct from comment.documentation, coloured in every bundled theme like VS Code's code span (the default foreground in the screenshot, not the orange markup.raw that T1015 used). Prose, list markers and [ ] brackets stay comment.documentation. Intra-doc link colouring from rust-analyzer is a separate card.
