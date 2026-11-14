---
task_uid: "01m7c017v77psx8v837j75dnbh"
task: "T377"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-config --test property_theme_colour passes: property_is_not_plain_text_in_listed_themes, hakuji_and_yohen_property_is_the_key_colour"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-config passes, including yohen_hakuji_values (the design doc's Property row now holds Lapis's hexes) and monokai_theme"
    required: true
    manual: false
  - id: "C3"
    text: "vscode-monokai.toml: property is upstream Monokai's property colour, or unchanged with the header sentence and the finish notes saying upstream colours none"
    required: true
    manual: false
---

# Colour the property highlight in Hakuji, Yohen and the Catppuccin themes (Monokai if upstream does) (Mac T1817)

# Give `property` a visible colour in Hakuji, Yohen and the Catppuccin themes

Decision 2026-11-13. `syntax.property` is the plain text colour in Hakuji, Yohen, the four Catppuccin themes and
VS Code Monokai, so names that label a value (member access, field declarations, named
arguments; the following cards move more languages onto `@property`) are invisible. Fix the colours; the
queries are other cards.

1. Hakuji and Yohen: `"property"` takes the value of their `"string.special.key"` (Hakuji `#2457c5`, Yohen
   `#8db1f2`, Lapis). The design table in `docs/design/design-system.md` (rows ~81-82, paragraph ~88) says Property is
   Shino and Key is Lapis: change the Property row to Lapis's two hexes and reword the paragraph to the new rule
   (a name that labels a value is Property; `string.special.key` is only for keys of data files; in Yohen and Hakuji
   both are Lapis). `crates/fe-config/tests/yohen_hakuji_values.rs` reads that doc's rows and pins `("Property",
   &["syntax.property"])` against it, so doc and themes must change together. Do not edit that test.
2. Catppuccin Mocha, Macchiato, Frappe, Latte: `"property"` follows upstream Catppuccin's property colour
   (`@property` in catppuccin/nvim, `variable.other.property` in catppuccin/vscode: Lavender). Check against the
   palette the themes already cite in their headers before writing; expected: Mocha `#B4BEFE`, Macchiato `#B7BDF8`,
   Frappe `#BABBF1`, Latte `#7287FD`. `string.special.key` stays blue, so property and key stay distinct. Fix the
   header comments that say `property` takes the text colour (mocha line ~35 and its siblings).
3. VS Code Monokai: read upstream's `monokai-color-theme.json` tokenColors (see
   docs/specs/bgqp0m0h1a-bundle-classic-monokai-vs-code-s-theme.md for where the repo took it from). If a scope for
   properties (`variable.other.property`, `meta.object-literal.key`, `support.type.property-name`) is coloured,
   use that colour for `"property"` in `vscode-monokai.toml`; if none is, leave it and write that in the file header's
   existing sentence (line ~149) and in the finish notes.

Do not touch the tenmoku-* or vscode-2026/dark-modern themes (their `property` is already coloured). No new key, so
no Ember change; `design_role_tokens`/theme_picker pins (`rg theme_picker crates/fe-workbench`) do not mention
`property`, confirm with `cargo nextest run -p fe-config -p fe-workbench theme`.

**Test.** New file `crates/fe-config/tests/property_theme_colour.rs`, patterned on
`tests/format_specifier_theme_key.rs` (parse each theme with `ThemeFile::parse(.., EXTENSIONS)` and read
`extensions["syntax"]`). `property_is_not_plain_text_in_listed_themes`: in hakuji, yohen and the four Catppuccin files
`syntax.property` differs from `editor.foreground` (the theme's text colour; read it the way
`yohen_hakuji_values.rs` does) and from `syntax.variable` where that exists. `hakuji_and_yohen_property_is_the_key_colour`:
equals `syntax.string.special.key`. Base fails.

Conflicts: blocked T1581 (colour-blind mode) also lists hakuji.toml and yohen.toml; this card goes first,
T1581 rebases. Linux-OK (text-only token tests).
