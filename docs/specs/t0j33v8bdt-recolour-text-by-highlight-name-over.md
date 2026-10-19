---
task_uid: "01m55v398x2xem15t0j33v8bdt"
task: "T173"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui unit test view::reveal_tests::foreground::foreground_decorations_recolour_over_tree_sitter passes: variable.parameter over input recolours its run while parse keeps the function colour; function.method over part of value splits its run; an unknown name and removing the provider change nothing; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "With no Foreground decorations the frame's spans are as at the base: the existing fe-editor and fe-gpui tests and screenshot scenes pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-editor -p fe-gpui passes with no existing test edited beyond what the spec names; cargo clippy -p fe-editor -p fe-gpui --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Recolour text by highlight name over tree-sitter's colours with a Foreground decoration, for language servers' semantic tokens (Mac T944)

Filed on argon from Mac T944 (card ~/coordination/argon-cards/zzr/T944.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55txyfp940etfphm68qq59e. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Held in the inbox: waits until Mac T917, Mac T816, Mac T858 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T944)".

## Problem

LSP completeness track, item 6 (user-approved 2026-10-17; `~/.local/share/fastedit-lead/lsp-gap-audit.md`:
"Semantic tokens ✗ ... Overlay on tree-sitter highlights in fe-gpui element.rs"). VS Code colours semantic tokens
over its syntax colouring: a token's type (and modifiers) pick a theme colour that replaces the grammar's for that
range, and text no token covers keeps the grammar's colour. fastedit colours text only from tree-sitter:
`styled_highlights` (`crates/fe-gpui/src/element.rs:2229`, handed to `layout_frame_*` at `:684-698`) maps
`Syntax::highlights` through `theme.syntax_color(Highlight)`. A host can add decorations to an editor
(`EditorModel::add_decoration_provider`, `crates/fe-editor/src/lib.rs:386`; anchored `DecorationSet`,
`crates/fe-editor/src/decorations.rs:168`), but no `DecorationKind` (`:53-66`) recolours text.

This card adds a decoration that recolours text by highlight name, laid over tree-sitter's colours. Feeding it
semantic tokens is the workbench card; fe-editor stays free of fe-syntax and of LSP.

## Change

1. `crates/fe-editor/src/decorations.rs`: new `DecorationKind::Foreground(Arc<str>)`: the range's text drawn in
   the colour the editor's theme gives that highlight name (`"function.method"`, `"variable.parameter"`), over
   whatever colour the syntax gives it. Its doc says an unknown name leaves the syntax colour.
2. `crates/fe-gpui/src/element.rs`: the frame's highlight spans become tree-sitter's with the frame's
   `Foreground` decorations laid over them: a decoration's name resolves through
   `fe_syntax::Highlight::from_capture` (longest known prefix, as captures resolve) and
   `theme.syntax_color`; where it resolves, its range takes that colour, splitting tree-sitter spans at its
   bounds and colouring uncovered text too; where it doesn't, nothing changes. Later decorations win where they
   overlap. Other decoration kinds ignore `Foreground` (it paints no background or underline). With no
   `Foreground` decorations every span is as today, and the per-frame cost is one merge of sorted spans.
3. Tests: new `crates/fe-gpui/src/view/foreground_decoration_tests.rs`, declared at the end of
   `crates/fe-gpui/src/view/reveal_tests.rs` as `#[path = "foreground_decoration_tests.rs"] mod foreground;`, in
   a headless window with Rust highlighting as `view/language_tests.rs` sets it up:
   `foreground_decorations_recolour_over_tree_sitter`: on `fn main() { let value = parse(input); }`:
   - a `Foreground("variable.parameter")` over `input` makes the laid-out row's run over `input` the theme's
     `variable.parameter` colour, and `parse` keeps tree-sitter's `function` colour;
   - a `Foreground("function.method")` over part of `value` splits its run: that part in the method colour,
     the rest as before;
   - a `Foreground("no.such.name")` changes no run; removing the provider restores every run.

Order: T917 (diagnostic tags) edits the same span-building closure, and T816/T858 hold `element.rs`; this
follows them. Linux-OK (laid-out runs checked, no goldens).
