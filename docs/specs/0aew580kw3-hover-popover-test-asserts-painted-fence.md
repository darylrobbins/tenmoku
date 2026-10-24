---
task_uid: "01m5ff0yzbat1dxc0aew580kw3"
task: "T259"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test tests::lsp_hover_ra_html::\u003cname\u003e asserts the popover's laid-out text for the rust-analyzer \u003ccode\u003e fixture and, through a public colour-resolution function of the markdown renderer or highlighter that the test calls without painting (no production change), that the fence's keyword run resolves to the keyword theme colour and differs from the plain-text colour. Limit (Known limits): painted glyph colour is not asserted until gpui exposes sprite colour or the renderer records the TextRun colours it paints; the follow-up card covers that. If no such public function exists, the test asserts the laid-out text only and the spec records that the colour check waits for the follow-up"
    required: true
    manual: false
---

# Hover popover test asserts painted fence glyph colours for the rust-analyzer <code> fixture (T256 review F3 follow-up)

## Problem

T256's end-to-end hover test (Mac T1024's C4) could only assert the popover's laid-out text under GPUI's `TestAppContext`; the keyword colours of the two rust fences' glyphs came from a separately rendered document. Review (round 2, F3) wants the colours asserted on the popover's painted scene.

## Change

Adapt the fake-server hover harness in `crates/fe-workbench/src/tests/lsp_hover_ra_html.rs` to a headless context with a recording renderer (or an equivalent live-scene hook), and assert the actual popover's text and fence glyph colours before and after scrolling, for the `ra_hover_transpose.md` fixture. Test-only; touch production code only if the hook needs a small seam, and say so.

Depends on T256.

## Implementation notes

A public path exists (review F1): `fe_markdown::parse` gives each fence's code and highlight spans, and `MarkdownStyle::code_runs` / `syntax_color` resolve them to colours without painting. The test parses the fixture that way and asserts the keyword runs (`impl`, `pub`, `const`, `fn`) resolve to the theme's keyword colour, differing from the plain-text colour. It also asserts the popover's laid-out text before and after scrolling. The popover's own painted glyph colour waits for the follow-up card (gpui sprite colour, or the renderer recording the TextRun colours it paints). No production code changed.
