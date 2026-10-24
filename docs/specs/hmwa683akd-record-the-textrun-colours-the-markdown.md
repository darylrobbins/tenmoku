---
task_uid: "01m5qd8r4wgeecmnhmwa683akd"
task: "T317"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-markdown-view exports a PaintedRuns gpui Global; when installed, document_element's code blocks and inline text push (text, TextRun list) for each StyledText they build, and nothing is recorded or changed when it is absent. Tests records_keyword_colour_of_a_fence, nothing_recorded_without_the_global"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Record the TextRun colours the Markdown renderer builds, behind an opt-in test Global (seam for T309)

## Problem
T259 could assert hover fence colours only through `MarkdownStyle::code_runs`, not what is painted: gpui-pre 0.3.9's TestAppContext exposes no painted-sprite colour. T309 (the painted assertion) needs a small production seam in the Markdown renderer. Mac decision: take a seam in the renderer (no gpui bump).

Note: the renderer that draws hover and completion Markdown is `crates/fe-markdown-view` (`document_element`, `MarkdownRenderer` in fe-gpui's hover.rs only declares the trait), so the seam lives there.

## Change
- `crates/fe-markdown-view/src/blocks.rs` (code block at the `StyledText::new(code).with_runs(runs)` site) and `crates/fe-markdown-view/src/inlines.rs` (the inline `StyledText` site): when a `PaintedRuns` gpui Global is present, push `(text, Vec<TextRun>)` for each styled text it builds. No cost and no behaviour change when the Global is absent.
- `crates/fe-markdown-view/src/lib.rs`: export `PaintedRuns` (set/take/clear methods; documented as a test seam).

## Tests
`crates/fe-markdown-view/tests/painted_runs.rs`: a fenced rust block records runs whose keyword run carries the theme's keyword colour; with no Global installed nothing is recorded.
