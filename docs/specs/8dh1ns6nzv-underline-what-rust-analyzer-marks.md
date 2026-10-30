---
task_uid: "01m62qtvt421rgm58dh1ns6nzv"
task: "T1229"
rev: 5
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Cause (user report with screenshot, release 0699e7f4): rust-analyzer sends self as a selfKeyword token with the mutable modifier; highlight_name (lsp_semantic_tokens.rs) maps no name for it, so its SyntaxUnderline gets an empty name and element/decorations.rs paints it in theme.foreground while the text keeps the tree-sitter self colour (pink in fastedit-dark). Function names map to 'function' and already match."
    required: true
    manual: false
  - id: "C2"
    text: "A SyntaxUnderline whose token has no semantic name is painted in the colour of the text under it (the syntax highlight colour there), not theme.foreground; where that range spans runs of different colours, each run's underline takes its own run's colour."
    required: true
    manual: false
  - id: "C3"
    text: "selfKeyword and selfTypeKeyword keep mapping to no highlight name (the existing assertion in lsp_semantic_tokens_tests.rs stays), so the tree-sitter self/Self colour is kept and C2 gives their mutable underline that colour; covered in crates/fe-workbench/src/tests/lsp_semantic_mutable_underline.rs, which records the drawn scene's underline primitives (in the manner of the lsp_hover_markdown.rs scene recorder, without refactoring it) and asserts only colours, no position mapping: the '\u0026mut self' line's underline colour equals self's painted text colour and differs from theme.foreground. It uses main's API only."
    required: true
    manual: false
  - id: "C4"
    text: "A render-level test in a new file crates/fe-gpui/src/element/syntax_underline_color_tests.rs asserts that '\u0026mut self' with a mutable underline gets an underline colour equal to the painted self colour, and a mutable function name's underline equals the function colour, with fastedit-dark and vscode-2026-dark. Red at base."
    required: true
    manual: false
  - id: "C5"
    text: "Existing tests pass unedited; clippy passes."
    required: true
    manual: false
  - id: "C6"
    text: "Regression test: the C3 test is named with --regression-test (tests::lsp_diagnostics::semantic_tokens::mutable_underline::\u003cname\u003e). It fails at base with an assert_eq!/assert_ne! panic, proved in the lane by temporarily reverting only the decorations.rs fix, and passes with it. If the scene cannot show underline colours with main's API, block rather than add API."
    required: true
    manual: false
---

# Underline what rust-analyzer marks mutable in the colour its text is painted, so &mut self's underline matches the self keyword instead of falling back to the default foreground
