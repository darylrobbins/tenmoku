---
task_uid: "01m57yaeam5ss4jjjt74xvztr5"
task: "T199"
rev: 1
type: "docs"
complexity: "s"
criteria:
  - id: "C1"
    text: "docs/decisions/\u003cthis task's key\u003e-per-language-styling.md exists, status proposed, with a numbered section for each of: token model, selector grammar and cascade, style roles, the semantic contribution point and its layering, rendering of the overlay, impact on T944/T945/T946/T917/T932/T988, Dark Modern, cards"
    required: true
    manual: false
  - id: "C2"
    text: "Every file, symbol and line the record cites exists at the base (highlight.rs Highlight, theme.rs EXTENSIONS and syntax_color, kiln extension_kind), or is corrected; the finish notes say so"
    required: true
    manual: false
  - id: "C3"
    text: "Each decision states its rejected alternative; section 6 says which of T944/T945/T946/T917/T932/T988 stand and which are respecced"
    required: true
    manual: false
  - id: "C4"
    text: "The record ends with its open questions for the user and the finish notes ask the USER to sign off"
    required: true
    manual: false
---

# Decide per-language styling (token model, selector cascade, style roles, the semantic contribution point) in a decision record (Mac T993)

# Decide per-language styling: token model, selector cascade, style roles and the `semantic` contribution point, in a decision record

## Problem

The user approved (2026-10-18, from the "fastedit: VS Code Parity Notes" doc, section "Generic per-language styling")
making per-language styling generic: fastedit knows token types, modifiers and rules that style them, not Rust; a
language extension contributes the meaning (rust-analyzer's `mutable` and its underline). Nothing is decided yet, and
three open cards would otherwise fix half of it by accident.

What exists (check each at the base):

- **Syntax colours are foreground-only, by dotted name.** `crates/fe-syntax/src/highlight.rs` `Highlight` (vocabulary
  `NAMES`, `ALIASES`, `from_capture` longest-prefix, `resolve_theme`); `crates/fe-config/src/theme.rs`
  `EditorPalette::syntax_color(name)` reads a theme's `[syntax]` table (colour per name, `EXTENSIONS = ["editor",
  "syntax"]`). No bold, italic, underline, strike or background per token, no modifiers, no languages in a selector.
- **Semantic tokens are ticketed with the mapping baked in.** T945 (fe-lsp decodes tokens as `type` + `modifiers`
  names from the legend), T944 (`DecorationKind::Foreground(Arc<str>)`: a highlight name, colour only), T946
  (`crates/fe-workbench/src/lsp_semantic_tokens.rs` `highlight_name`: a fixed table from LSP types to fe-syntax
  names; modifiers only pick `.builtin`/`constant`). Modifiers like `mutable` have nowhere to go.
- **Extensions** (record `docs/decisions/vmtnmkay62-extensions-and-plugins.md`, T839): kiln loads manifests, hot
  reloads and lets an app register its own contribution kinds with a schema (`extension_kind`,
  `kiln/crates/kiln/src/extensions.rs`; fastedit's `[contributes.lsp]` is T844, grammars T845). Layering: record §4.
- **Other styling already ticketed:** diagnostic tags Unnecessary/Deprecated (T917), inlay-hint chips (T932),
  bracket-pair colours (T988), indent guides (landed, `indent_guide*` editor keys).

## Change

Write `docs/decisions/<this task's key>-per-language-styling.md`, status `proposed`, in the shape of
`vmtnmkay62-extensions-and-plugins.md` (Context, Decision, numbered sections, Cards, Open questions). It decides,
each with a short rationale and the rejected alternative:

1. **Token model.** A span resolves to a type plus modifiers. Core vocabulary is LSP's (types and modifiers listed in
   the doc); extensions add types (with a supertype fallback, Rust's `lifetime` -> `typeParameter`) and modifiers.
   fe-syntax emits the same `type.modifier` names so a language with no LSP is styled by the same rules. Decide how
   today's dotted `Highlight` names (`function.method`, `markup.heading`, ...) map onto types+modifiers: bridge,
   migrate, or keep both with one resolver. Name the crate that owns the model (the doc says alongside fe-syntax and
   fe-lsp; fe-config must stay GPUI-free and cannot depend on either).
2. **Selector grammar and cascade.** `type.modifier...:language` with `*`; specificity (language, then modifier
   count, then exact type over supertype/`*`); each attribute (`fg`, `bg`, bold, italic, `underline`
   straight|dotted|wavy, `strike`) resolved independently from the most specific rule that sets it. Fix the exact
   grammar, tie-breaking by source order, and how a user's rule beats a more specific theme rule across layers.
3. **Style roles.** The abstract roles (`emphasis.underline`, `emphasis.strong`, `warning.subtle`, `deprecated`, ...):
   the closed list, the `[roles]` theme table, and how an extension maps a modifier or type onto a role.
4. **The `semantic` contribution point.** The `[semantic]` schema in `extension.toml` (`[[semantic.modifiers]]`,
   `[[semantic.types]]`, default rules) as a fastedit kind registered with `extension_kind` (schemars schema, positioned
   diagnostics), the `[semantic]`/`[roles]` tables in theme files (fastedit's theme tables beside `editor`/`syntax`),
   and the user's settings form. Which layers exist and their order: user > theme > extension > fastedit defaults.
   What lives in kiln (nothing but the generic kind plumbing) and what in fastedit.
5. **Rendering.** What a decoration carries once colour-only `Foreground(name)` (T944) is not enough (a resolved style,
   or the token); how the semantic overlay refines tree-sitter's spans after open without flicker or layout shift;
   what happens to a modifier the syntax layer cannot tag (later uses of `let mut x`).
6. **Impact on open cards.** Say for T944, T945, T946, T917, T932 and T988 which stand as written and which are
   respecced, and what each reads from the model.
7. **Dark Modern.** What the VS Code Dark Modern theme card (separate) can ship now on the colour-only model and what
   waits for the follow-ups.
8. **Cards.** The follow-up cards, each with files, order and size (expect: the model and resolver crate; fe-syntax
   emitting typed names; theme `[semantic]`/`[roles]` parsing in fe-config; the `semantic` kind and layering in
   fastedit's config; rendering of full styles; the Rust extension's manifest), drafted after sign-off.
9. **Open questions for the user**, ending with the sign-off ask.

## Do not

Change code, schemas or other docs, or write the follow-up cards' specs beyond one paragraph each.

## Lent-card note

This is a decision record; the USER signs it off. Write the record, finish, and leave it for the user's sign-off.
