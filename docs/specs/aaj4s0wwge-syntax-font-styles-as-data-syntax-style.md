---
task_uid: "01m7d0vt9xrzs941aaj4s0wwge"
task: "T384"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in cargo nextest run -p fe-config --test syntax_styles pass: longest_prefix_wins_for_styles, a_theme_without_the_table_has_none, yohen_and_hakuji_set_italic_comments_and_bold_function_definitions"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-gpui --test syntax_styles passes: the editor theme resolves a capture to colour and style; no golden changes; crates/fe-config/tests/design_language_doc.rs passes"
    required: true
    manual: false
---

# Syntax font styles as data: [syntax_style] in themes (italic, bold) resolved with the colours, not yet drawn (Mac T1598)

# Font styles in the syntax palette: data only

Spec: `docs/design/design-system.md#syntax` ("Comment: italic", "Function:
bold at its definition"). Today a capture has a colour only
(`EditorPalette::syntax: Vec<(String, Color)>`, `crates/fe-config/src/theme.rs`)
and fe-gpui paints foreground spans as `StyledSpan<Hsla>`.

## Change (no painting)

- Theme files gain `[syntax_style]`: `comment = "italic"`, `"function.definition" =
  "bold"`, `"keyword" = "bold italic"`; same longest-prefix rule as colours.
  `EditorPalette` carries `styles: Vec<(String, FontStyle { italic, bold })>`;
  `fe-gpui`'s `EditorTheme` resolves a capture to colour and style
  (`crates/fe-gpui/src/theme.rs`); nothing draws it yet.
- Yohen and Hakuji set `comment = "italic"` and `"function.definition" = "bold"`;
  every other bundled theme sets none (unchanged look).
- `docs/design-language.md` records the key.

## Not in this card

Italic faces, shaping (next two cards), the definition capture itself.

Platform: **Linux-OK**.

## Implementation notes

- Font styles are a theme's own: `EditorPalette::for_theme` clears the bundled base's styles before laying the theme's `[syntax_style]` over it, so a theme without the table has none, kiln's bases included. The bundled `fastedit_dark()` and `fastedit_light()` still carry Yohen's and Hakuji's. Kiln-base expectations in `crates/fastedit/src/theme.rs` and `crates/fe-workbench/src/theme_picker_tests.rs` follow.
