---
task_uid: "01m5e6dsjymrm7vm01wa5wf23p"
task: "T242"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test tests::lsp_aliases::hover_markdown::hover_markdown_renders_rich passes: the hover popover holds md-block-0 (the fence, fn in the keyword colour) and md-block-2 (the paragraph); the link opens https://example.com and no tab; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; fe-workbench/Cargo.toml changes only if T848 has not added fe-markdown-view, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Render language servers' hover and completion documentation as Markdown, code highlighted in the editor's theme and links opening, as VS Code does (Mac T957)

{
  "mode": "inline",
  "body": "## Problem\n\nLSP completeness track, item 9 (user-approved 2026-10-17). Once the cards this depends on land, fe-markdown-view\nlays out a Markdown document compactly (`markdown_element`, `MarkdownStyle::compact`) and fe-gpui draws hover\nand completion documentation through a host-set `MarkdownRenderer`. The workbench sets none, so docs stay\nnear-plain text. T848 (inbox) adds fe-markdown-view to fe-workbench for the preview tab and opens links from it\n(files, the browser, anchors).\n\n## Change\n\n1. New `crates/fe-workbench/src/lsp_markdown.rs`, declared from `crates/fe-workbench/src/lsp.rs` as\n   `#[path = \"lsp_markdown.rs\"] mod markdown;`: a `MarkdownRenderer` that parses with fe-markdown (the document\n   path being the editor's file, the root the window's, for relative links) and draws `markdown_element` with\n   `MarkdownStyle::new(theme, ui).compact()`. A link click opens it as the preview tab opens links (T848's\n   handling: `file:` and relative paths in an editor, at a line if the link has `#L<n>`; `http(s)`/`mailto` in\n   the browser; anything else ignored). Parsing is cached by the markdown string for the popover's lifetime,\n   so a redraw doesn't re-parse.\n2. `crates/fe-workbench/src/lsp.rs` `sync_lsp`: every editor gets the renderer, as it gets the hover provider\n   (`lsp.rs:1117-1137`).\n3. `crates/fe-workbench/Cargo.toml`: fe-markdown and fe-markdown-view if T848 hasn't added them (it does;\n   this card follows it).\n4. Tests: new `crates/fe-workbench/src/tests/lsp_hover_markdown.rs`, declared at the end of\n   `crates/fe-workbench/src/tests/lsp_aliases.rs` as `#[path = \"lsp_hover_markdown.rs\"] mod hover_markdown;`,\n   with the fake of `tests/lsp.rs`: `hover_markdown_renders_rich`: the fake's hover is\n   `` ```rust\\nfn f()\\n```\\n---\\nSee [docs](https://example.com). ``; after the hover delay the popover holds\n   `md-block-0` (the fence, its `fn` in the theme's keyword colour) and `md-block-2` (the paragraph); clicking\n   the link asks the platform to open `https://example.com` (as T848's tests observe opened URLs) and opens no\n   tab.\n\nLinux-OK.",
  "ref": "",
  "ref_blob": ""
}
