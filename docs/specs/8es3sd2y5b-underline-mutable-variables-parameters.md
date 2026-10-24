---
task_uid: "01m5nh4s7hbjtk128es3sd2y5b"
task: "T1121"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test (new file element/syntax_underline_tests.rs): a syntax-coloured underline is placed on the Underlines layer, straight, in theme.syntax_color of its highlight name, and in theme.foreground for a name the theme does not colour"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test (new file tests/lsp_semantic_underline_colour.rs): a variable+mutable semantic token yields the new underline kind with the same highlight name as its Foreground; T1016 lsp_semantic_underline.rs assertions move to the new kind; the only other existing test edited is T1120's tests/lsp_semantic_timeout.rs, whose decoration filter moves from the old Underline kind to SyntaxUnderline with its timeout assertion unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-editor -p fe-gpui -p fe-workbench passes; clippy passes"
    required: true
    manual: false
---

# Underline mutable variables, parameters and fields in the token's own theme colour, as VS Code does, not the editor foreground

## Problem

The user asked (2026-10-23) for the mutable underline (T1016) to be drawn in the token's own theme colour, as VS Code does, instead of the editor foreground. T1016 emits `DecorationKind::Underline { wavy: false }` with `Tint::Muted`; `Tint` is a closed role enum resolved by `tint_color` (crates/fe-gpui/src/element/decorations.rs:62-74) and cannot carry a syntax name. Foreground decorations resolve their colour through `Highlight::from_capture(name)` then `theme.syntax_color` (crates/fe-gpui/src/element.rs:2909-2918).

## Change

- crates/fe-editor/src/decorations.rs: add a `DecorationKind` variant for a straight underline in the colour the theme gives a highlight name (e.g. `SyntaxUnderline(Arc<str>)`), documented like `Foreground`. A name the theme has no colour for underlines in the editor foreground.
- crates/fe-gpui/src/element/decorations.rs: place it like `Underline` (the `Highlight | Underline` arm at ~226), paint it in `Layer::Underlines` (~341), with its colour resolved through the same `Highlight::from_capture` + `theme.syntax_color` lookup Foreground uses (move or share the lookup without touching element.rs), falling back to theme.foreground. This is the only exhaustive match on DecorationKind outside tests.
- crates/fe-workbench/src/lsp_semantic_tokens.rs: a `mutable` token emits the new kind with the same highlight name as its Foreground; a `mutable` token whose type maps to no name emits it with no themed colour (foreground).

## Tests

- fe-gpui: new file crates/fe-gpui/src/element/syntax_underline_tests.rs, declared next to decorations_tests.rs: the placed underline's colour equals `theme.syntax_color` of the name, and equals theme.foreground for a name the theme doesn't colour; not wavy; on the Underlines layer.
- fe-workbench: new file crates/fe-workbench/src/tests/lsp_semantic_underline_colour.rs, declared at the end of crates/fe-workbench/src/tests/lsp_semantic_tokens.rs: a `variable`+`mutable` token yields the new kind named as its Foreground.
- T1016's crates/fe-workbench/src/tests/lsp_semantic_underline.rs asserts the old `Underline`/`Tint::Muted` kind: update those assertions to the new kind; it is the only existing test edited.

Not a screenshot golden change unless an existing golden shows a mutable token from a fake server (check; update only if so and say why).

## Implementation notes

- `tests/lsp_semantic_timeout.rs` (T1120) also filtered on the old `Underline` kind and failed once the emitter changed, so its one filter line moved to `SyntaxUnderline(_)`; the spec named only T1016, but this edit was unavoidable.
- The name lookup is `name_color` in `fe-gpui/src/element/decorations.rs`; `element.rs` still has its own copy for Foreground (out of scope).
- The T1120 timeout test is the one further edit: with the old kind no longer emitted its filter could not match, so C2's "no other existing test" cannot hold literally; the assertion it makes is unchanged.
