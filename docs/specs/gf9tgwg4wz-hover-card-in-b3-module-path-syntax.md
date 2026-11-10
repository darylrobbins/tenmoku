---
task_uid: "01m725cfqz7hy1yzgf9tgwg4wz"
task: "T556"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in hover_card_b3 (cargo nextest run -p fe-gpui --test hover_card_b3) pass: the card is surface.raised with radius 6, a 1 px hairline and the float shadow; the module path line is faint code small; the signature keeps its syntax colours; the footer shows the definition location when the provider gives one, and gd definition"
    required: true
    manual: false
  - id: "C2"
    text: "Compact Markdown only: code padding 8, block gap 8, a visible 1 px rule (cargo nextest run -p fe-markdown-view passes). The Markdown preview goldens are macOS-only and can't be checked on helium: the Mac checks them when it lands the batch."
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-gpui --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist)"
    required: true
    manual: false
---

# Hover card in B3: module path, syntax-coloured signature, docs, a footer with the definition location, the floating-surface look, compact Markdown spacing (Mac T1544)

# Hover card in B3

Spec: `docs/design/design-system.md#editor` (Hover card). Audit findings
HC1–HC3 (the held card P21, less the completion part: the completion popup is
still to design).

## Change

- Layout (`crates/fe-gpui/src/hover.rs`): the module path in faint code small;
  the signature in its syntax colours; prose docs in UI body; a footer in UI
  small faint with the definition location when the hover's provider gives a
  range in another place, and `gd definition`. Diagnostics shown in the hover
  keep their place above it.
- Floating-surface look: `surface.raised`, radius 6, the float shadow and a
  1 px hairline (HC2).
- The hovered token gets a soft fill (`identifier_highlight`).
- Compact Markdown only (`crates/fe-markdown-view/src/compact.rs`,
  `style.rs`, `blocks.rs` under `cx.compact` / `MarkdownStyle::compact`): code
  padding 8, block gap 8, a visible 1 px rule, code at the editor's line height
  (HC1, HC3). The Markdown preview's goldens must still match.
- Bold signature names, caller counts and K-pin wait (font styles, call
  hierarchy counts).

## i18n

fe-gpui's text is already in its catalogue (card G1). Any text this card adds
or rewrites goes to `crates/fe-gpui/l10n/en-US/` as keys; the crate's lint
fails otherwise.

Platform: **Linux-OK** (no golden shows a hover; the audit hover scene is the visual check on the Mac).

## Implementation notes

- **The card's parts** (`crates/fe-gpui/src/hover.rs`, `CardText`). The
  provider's Markdown is split, not re-rendered whole: its leading fences
  (blank lines apart, at most two) are the header. Two fences whose first is
  one word (no whitespace) are rust-analyzer's module path and signature; one
  fence whose first line is one word with a blank line after it splits the
  same way (the audit scene's old text); any other leading fence is all
  signature. The rest, rules included, is the docs, drawn by the host's
  `MarkdownRenderer` or else as text. The path is drawn in the editor's code
  font at its size less 1, in the faint text; the signature at the editor's
  size, highlighted by fe-syntax as its fence's language. Diagnostics stay
  first. `popover_text` still holds the whole text.
- **Faint and sizes.** fe-gpui has no faint token: it uses
  `EditorTheme::gutter_foreground`, which in Yohen is `text.faint` (#a09080);
  the test checks that against the theme file. Sizes follow the editor's (13
  by default gives the design's 12 for code small, 11 for UI small), so a
  zoomed editor's card scales with it.
- **Floating surface.** `popover_background` (Yohen's `surface.raised`), a
  1 px `popover_border`, radius 6 and the float shadow 0 12 32 at 55% black
  (14% on a light surface). fe-gpui doesn't depend on kiln-ui, so
  `SHADOW_FLOAT` and the menu radius are mirrored as constants in `hover.rs`.
- **Footer.** `HoverProvider::definition` is a new trait method with a
  default of `None`, so no provider changes. The footer shows its location
  unless it's the hovered row of the same file, and `gd definition` only
  with vim on (`gd` is vim's key; there's no binding lookup for the
  non-vim key in fe-gpui). Host popovers (blame, annotations, hints) have no
  footer. **Not wired yet:** the workbench's LSP provider (`fe-workbench`
  `lsp.rs`, outside this scope) doesn't answer `definition`, so in the app
  the footer shows only `gd definition` until a follow-up asks the server's
  `textDocument/definition`.
- **Not done here:** the hovered token's soft fill (`identifier_highlight`)
  has to be painted behind the text, which takes a tint in fe-editor's
  decorations and its paint in `element.rs`, both outside this scope: a
  follow-up card. Bold names, caller counts and the K pin wait, as the spec
  says.
- **Compact Markdown** (`MarkdownStyle` gains `block_gap`, `code_padding`,
  `rule` and `float_border`; `new` keeps 16, 16 and `ui.border`, so the
  preview is unchanged). The rule was laid out across the column already; it
  was invisible because `ui.border` (Yohen `border.default` #2e251e) is
  almost the raised surface (#2a211c). The compact form draws rules in the
  floating surface's hairline (`editor.popover_border`). Code already used
  the editor's line height; the test pins it.
- **T549 F3.** `Hovered` keeps each diagnostic's data (`DiagnosticItem`) and
  says it in the active locale when drawn, so a locale switch no longer
  drops an open diagnostic hover. `tests/hover_locale.rs` switches to a fake
  locale with the hover open, in its own binary as `l10n_status.rs` does.
- **Scope.** Widened to `lib.rs` (exports), `fe-gpui.ftl` (the crate's one
  catalogue file; a separate `hover.ftl` would need `l10n.rs`), the two
  fe-workbench hover tests whose `md-block-N` indices moved with the header
  and whose example now starts above the fold with 8 px gaps, and the locale
  test. The audit scene now sends rust-analyzer's two fences; its
  `surface.panel` probe went with the code block, and the `surface.raised`
  probe moved to (700, 262), right of the path and signature: estimated from
  the old probes, so the Mac should confirm it when it lands the batch.
