---
task_uid: "01m5dygzj3k5zhvgvd64nkshmz"
task: "T1016"
rev: 2
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::lsp_diagnostics::semantic_tokens::underline (new file lsp_semantic_underline.rs): a variable+mutable token gets one Underline (Tint::Muted, not wavy) and one Foreground over its bytes; without mutable, no Underline; an unmapped type with mutable gets an Underline alone; editor.semantic_highlighting = false removes every Underline"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited; clippy passes"
    required: true
    manual: false
---

# Underline mutable variables, parameters and fields from language servers' semantic tokens, as VS Code does for rust-analyzer's mutable modifier, with the existing Underline decoration

## Problem

The user approved VS Code's `mutable` underline (2026-10-21). rust-analyzer marks a mutable variable, parameter or field with the `mutable` semantic token modifier and VS Code underlines it (an extension default, not a theme rule). T946 maps token types and modifiers to colours only (`DecorationKind::Foreground`) and ignores `mutable`.

## Reuse, no new kind

fe-editor already has `DecorationKind::Underline { wavy }` (`crates/fe-editor/src/decorations.rs`), painted by fe-gpui's `Layer::Underlines` (`element/decorations.rs`), coloured by a `Tint` role resolved from the theme. A straight underline in `Tint::Muted` (the theme's editor foreground) needs no change to fe-editor, fe-gpui or fe-config, and no theme key: the underline is the server's modifier, as in VS Code, not a theme colour (the theme model's `[syntax]` values are colours only, so a flag would mean a model change). Cost: the underline is the foreground colour, not the token's own colour. No screenshot golden changes.

## Change

In `crates/fe-workbench/src/lsp_semantic_tokens.rs` (T946's file), where the per-buffer `DecorationSet` is built: a token whose modifiers include `mutable` (looked up in the server's legend) gets, besides its `Foreground` decoration (or alone when its type maps to none), a `DecorationKind::Underline { wavy: false }` decoration with `Tint::Muted` over the same bytes. Tokens without `mutable` get none. Both go in the same provider, so they follow edits, refresh and the `editor.semantic_highlighting` switch together.

Tests: new `crates/fe-workbench/src/tests/lsp_semantic_underline.rs`, declared at the end of `crates/fe-workbench/src/tests/lsp_semantic_tokens.rs` as `#[path = "lsp_semantic_underline.rs"] mod underline;`, with the fake advertising `mutable` in its legend: a `variable`+`mutable` token yields one Underline and one Foreground over its bytes, a `variable` without it yields no Underline, an unmapped type with `mutable` yields an Underline alone, and `editor.semantic_highlighting = false` removes every Underline.

Linux-OK.
