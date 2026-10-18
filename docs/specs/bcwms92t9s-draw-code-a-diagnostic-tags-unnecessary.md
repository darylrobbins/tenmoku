---
task_uid: "01m55sbpjvr82wqjbcwms92t9s"
task: "T169"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test element::tests::diagnostic_tags_tests::unnecessary_fades_and_deprecated_strikes passes (new crates/fe-gpui/src/element/diagnostic_tags_tests.rs, declared in element/tests.rs), plain and Rust-highlighted: under an Unnecessary hint the row's decoration runs have 0.67 times their untagged alpha, the text around is unchanged and no squiggle covers it; a Deprecated warning has one painted_strikes box over its text and keeps its squiggle; an untagged hint keeps its squiggle"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::diagnostics_tests::squiggles_gutter_and_hover passes unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-gpui passes with no existing test edited (only the mod line added to element/tests.rs); cargo clippy -p fe-gpui --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Draw code a diagnostic tags Unnecessary faded and Deprecated struck through, as VS Code does, with no squiggle for an Unnecessary hint (Mac T917)

## Problem

LSP gap audit (2026-10-17, main c7b6a04a): diagnostic tags are "decoded but nothing in fe-gpui draws
Unnecessary or Deprecated". Re-verified at main 40608138:

- fe-lsp advertises `tagSupport: [Unnecessary, Deprecated]` (`crates/fe-lsp/src/server.rs:2109-2111`) and
  decodes them (`crates/fe-lsp/src/diagnostics.rs:137`, `tag`).
- fe-workbench copies them onto fe-editor's `Diagnostic.tags` (`crates/fe-workbench/src/lsp.rs:1983-1988`).
- fe-gpui never reads `tags`. `diagnostic_marks` (`crates/fe-gpui/src/element.rs:1494-1600`) draws every
  diagnostic as a wavy underline in its severity's colour, and nothing else in fe-gpui mentions
  `DiagnosticTag`.

VS Code draws Unnecessary code (unused variables and imports, unreachable code; TypeScript sends them as
Hint diagnostics tagged Unnecessary) faded at `editorUnnecessaryCode.opacity` (`#000000aa`, 0.67), with no
squiggle for a Hint. It draws Deprecated as struck through. fastedit draws a hint squiggle and nothing
more. This is a missing rendering feature, not a defect in what is drawn.

## Change

`crates/fe-gpui/src/element.rs`, with the frame's diagnostics (`view.model.diagnostics()`):

1. **Unnecessary, faded.** Text covered by a diagnostic tagged Unnecessary is drawn at its colour's alpha
   × `UNNECESSARY_OPACITY` (a new const, 0.67, VS Code's). That covers syntax-highlighted spans and plain
   text in `theme.foreground`. Do it where the frame's style spans are made (the `styled_highlights`
   closure handed to `layout_frame_*`): split spans at the diagnostic's bounds, and add foreground spans
   for uncovered text inside it. Overlapping Unnecessary diagnostics fade once.
2. **Deprecated, struck.** For each piece of a Deprecated diagnostic on a shown row (the pieces
   `diagnostic_marks` computes), paint a 1 px straight line through the text's middle (half the
   ascent above the baseline) in the text's colour.
3. **No squiggle for Unnecessary hints.** A Hint tagged Unnecessary draws no squiggle; fading shows it.
   Its gutter mark stays. Every other diagnostic keeps its squiggle, tagged or not.
4. `crates/fe-gpui/src/view.rs`: record the painted strike lines as `painted_strikes: Vec<Bounds<Pixels>>`,
   beside `painted_squiggles`, for tests.
5. Tests: new `crates/fe-gpui/src/element/diagnostic_tags_tests.rs`, declared in
   `crates/fe-gpui/src/element/tests.rs` with `#[path = "diagnostic_tags_tests.rs"]`, in a headless
   window like `view/diagnostics_tests.rs`. `unnecessary_fades_and_deprecated_strikes`, on
   `fn main() {\n    let unused = old();\n}` (plain text, then again with Rust highlighting):
   - a Hint tagged Unnecessary over `unused`: the laid-out row's decoration runs over `unused` have
     alpha 0.67 × their untagged alpha, the text around them is unchanged, and no squiggle covers
     `unused`;
   - a Warning tagged Deprecated over `old`: one `painted_strikes` box spans `old`'s x range on its row,
     and its squiggle is still painted;
   - an untagged Hint draws its squiggle as before.

Pixel goldens aren't needed: the painted state is checked. Linux-OK.
