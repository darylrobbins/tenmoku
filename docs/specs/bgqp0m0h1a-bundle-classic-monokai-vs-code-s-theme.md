---
task_uid: "01m6rv5k3ss41txcbgqp0m0h1a"
task: "T1418"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "monokai_theme::loads: find(\"Monokai\") and find(\"vscode-monokai\") return it with appearance Dark; the file parses with no diagnostics under EXTENSIONS; EditorPalette::for_theme returns no warnings; its [editor] keys are exactly EditorColors::KEYS; Settings::default().theme.name is still None and find(\"dark\") is still tenmoku-dark"
    required: true
    manual: false
  - id: "C2"
    text: "monokai_theme::matches_upstream: editor background #272822, foreground #F8F8F2, selection #878B9180, cursor #F8F8F0, active line #3E3D32, surface.panel #1E1F1C, and the colours of comment #88846F, keyword #F92672, function #A6E22E, string #E6DB74, type.builtin #66D9EF, number #AE81FF and variable.parameter #FD971F equal the pinned monokai-color-theme.json"
    required: true
    manual: false
  - id: "C3"
    text: "monokai_theme::syntax_keys_are_known: every [syntax] key is a name Highlight::from_capture accepts; string.placeholder is set and differs from string; variable.parameter, type.parameter and constant.enum_member are set"
    required: true
    manual: false
  - id: "C4"
    text: "The readability checks pass with Monokai bundled: labels_readable_in_bundled_themes, mode_segments_readable_in_bundled_themes, every_bundled_theme_sets_both_readably and tests::bracket_pair_colors_default::every_bundled_theme_has_three_distinct_bracket_colors"
    required: true
    manual: false
  - id: "C5"
    text: "With 11 bundled themes: every_entry_parses, every_bundled_theme_sets_every_minimap_key, minimap_colours_are_vscode_s, every_bundled_theme_sets_vscode_s_sticky_scroll_colours, every_bundled_theme_sets_it and tests::template_theme_names::the_template_lists_every_bundled_theme pass"
    required: true
    manual: false
  - id: "C6"
    text: "theme_picker::theme_picker_tests::choosing_a_bundled_theme_applies_and_keeps_it lists Monokai (Dark) after Catppuccin Mocha and before Tenmoku Dark, and setup_wizard_preview_tests::tiles_use_each_themes_colours passes with a Monokai tile"
    required: true
    manual: false
  - id: "C7"
    text: "NOTICE has a VS Code (Microsoft, MIT) and a Colorsublime-Themes (Copyright (c) 2015 Colorsublime.com, MIT) entry for crates/fe-config/themes/vscode-monokai.toml, THIRD_PARTY.md a row with the upstream path and pinned commit; cargo nextest run -p fastedit --test notice --test third_party passes"
    required: true
    manual: false
  - id: "C8"
    text: "cargo run -p fe-workbench --example screenshots passes on the Mac with golden/setup_wizard_dark.png regenerated to show the Monokai tile; setup_wizard_light.png, workbench.png and the file_icons goldens are unchanged"
    required: true
    manual: false
  - id: "C9"
    text: "cargo nextest run -p fe-config -p fe-workbench and cargo clippy -p fe-config -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Bundle classic Monokai (VS Code's theme-monokai) as a selectable dark theme, not the default

# Bundle classic Monokai as a selectable dark theme

## Problem

The user (2026-11-06) asked for a Monokai theme next to the Catppuccin flavours. Bundle the classic
Monokai (Wimer Hazenberg's original palette) as VS Code ships it, as a built-in, selectable dark theme,
not the default: `[theme] name` unset still gives Tenmoku Dark / Tenmoku Light. **Not Monokai Pro**
(proprietary, monokai.pro): take no colour, name or file from it.

Upstream: microsoft/vscode `extensions/theme-monokai/themes/monokai-color-theme.json`, last changed at
commit `f93fdeb454a0f0fafe5f310408cec48dd3b6f58a` (2026-10-01); pin the commit you actually read in the
file header and THIRD_PARTY.md. MIT, Copyright (c) Microsoft Corporation. That extension's
`cgmanifest.json` registers the colours as derived from **Colorsublime-Themes**
(https://github.com/Colorsublime/Colorsublime-Themes, commit `c10fdd8b144486b7a4f3cb4e2251c66df222a825`,
MIT, "Copyright (c) 2015 Colorsublime.com", as VS Code's ThirdPartyNotices.txt carries it), so both
notices are owed. The file is JSONC: a `//` header naming the base palette and a trailing comment on
`tab.inactiveForeground`. It sets `semanticHighlighting: true` and no `semanticTokenColors`, so semantic
names take their TextMate scopes' colours.

How bundled themes work (read `docs/specs/cga21xm10k-bundle-catppuccin-latte-frappe-macchiato.md`, the
last card that bundled a theme, and follow it): a hand-written kiln theme TOML in
`crates/fe-config/themes/` (`kiln_theme = 1`, `name`, `appearance`, kiln token tables `[surface]` `[text]`
`[border]` `[accent]` `[state]` `[status]` `[list]` `[tab]` `[scrollbar]`, then `[editor]` with every key
of `EditorColors::KEYS`, then `[syntax]` by highlight name), registered by one `bundled!` line in
`crates/fe-config/src/themes.rs`. There is no VS Code JSON importer; do not build one (a throwaway
conversion script, not committed, is fine). Picker, `[theme] name`, the Settings tab dropdowns, the setup
wizard's tiles and kiln install all iterate `bundled_themes()` and need no code change.

## Change

1. New `crates/fe-config/themes/vscode-monokai.toml`: `name = "Monokai"`, `appearance = "dark"`. Header
   in `catppuccin-mocha.toml`'s shape, within the first 15 lines as `crates/fastedit/tests/third_party.rs`
   checks: upstream path and pinned commit, Colorsublime origin, MIT pointer to NOTICE, then what is left
   out (font styles: italic storage.type/parameters/support.type, underlined class names; `invalid`;
   markdown-only and find-in-files scopes; token.*-token) and which values are chosen from the palette.
2. `crates/fe-config/src/themes.rs`: `bundled!("Monokai", "vscode-monokai", Dark)` after
   "Catppuccin Mocha", before "Tenmoku Dark". `find("Monokai")` and `find("vscode-monokai")` then work.
3. `NOTICE`: two entries in the existing style: "VS Code (colour values, adapted in
   crates/fe-config/themes/vscode-monokai.toml)" Microsoft/MIT, and "Colorsublime-Themes (Monokai colour
   values, via VS Code's theme-monokai, adapted in crates/fe-config/themes/vscode-monokai.toml)
   Copyright (c) 2015 Colorsublime.com / MIT". `THIRD_PARTY.md`: one row in the "Code copied or adapted"
   table beside the other `crates/fe-config/themes/` rows: our path | VS Code
   `extensions/theme-monokai/themes/monokai-color-theme.json` (colours from Colorsublime-Themes per its
   `cgmanifest.json`) | the pinned commit | MIT. Check the audit paragraph's "Themes:" line still holds
   (it covers `vscode-*`). Only kiln's rows are mirrored to `kiln/THIRD_PARTY.md`; this one is not.
4. `crates/fe-config/settings-template.toml`: add "Monokai" to the `# A bundled theme by name:` comment
   (`template_theme_names` fails otherwise). Touch nothing else in the file.
5. `README.md` line ~30, the Themes bullet: add Monokai.
6. Existing tests that enumerate the bundled themes, edited minimally:
   - `crates/fe-config/tests/minimap_theme_keys.rs`: `len() == 10` to 11; Monokai sets no
     `minimapSlider.*`, so its slider marks are the default dark arm's; it does set
     `minimap.selectionHighlight` `#878b9180`, so add a `("vscode-monokai", _)` arm with the default
     slider values and that selection (or set the default arm's selection and say why in the header).
     Background, warning, gutter, error and find-match marks follow the test (editor's / VS Code defaults).
   - `crates/fe-config/tests/sticky_scroll_colors.rs`: a `want` row for `vscode-monokai` (array length 11).
     Monokai sets no `editorStickyScroll.*`, so VS Code's dark defaults apply, as in Dark Modern's row,
     on `#272822`.
   - `crates/fe-config/tests/vim_yank_highlight_colors.rs`: add the file to its `BUNDLED` list (it
     compares the list with the `themes/` directory's count).
   - `crates/fe-workbench/src/theme_picker_tests.rs`: `row("Monokai", Appearance::Dark, false)` after
     "Catppuccin Mocha" in `choosing_a_bundled_theme_applies_and_keeps_it`.
7. New test file `crates/fe-config/tests/monokai_theme.rs` (do not extend other test files for these):
   see criteria. Model it on `catppuccin_themes.rs` / `vscode_2026_dark_theme.rs`.
8. Screenshot golden: the setup wizard's Appearance page draws a tile per bundled dark theme, so
   `crates/fe-workbench/examples/screenshots/golden/setup_wizard_dark.png` gains a Monokai tile (row 3
   beside Tenmoku Ember today; Tenmoku Dark and Ember shift one place). Regenerate it with
   `UPDATE_SCREENSHOTS=1 cargo run -p fe-workbench --example screenshots` on the Mac, check by eye that
   only the dark-theme tiles changed, and leave `setup_wizard_light.png`, `workbench.png` and the
   file-icon goldens unchanged.

## Palette mapping (guidance; values are upstream's unless marked chosen)

Base palette: background `#272822`, tab well / sidebar / widgets `#1E1F1C`, line highlight and hover
`#3E3D32`, selection-ish `#414339`, focus/muted `#75715E`, foreground `#F8F8F2`; accents pink `#F92672`,
green `#A6E22E`, yellow `#E6DB74`, orange `#FD971F`, purple `#AE81FF`, cyan `#66D9EF`.

- Chrome: `surface.background` editor.background `#272822`; `panel` sideBar/titleBar/tabs well
  `#1E1F1C`; `raised` editorWidget `#1E1F1C`; `overlay` its translucent form. `text.primary` `#F8F8F2`,
  `muted` tab.inactiveForeground `#CCCCC7` or line number `#90908A` (pick for contrast), `disabled`
  `#75715E`. `border.default` panel.border `#414339`, `subtle` editorGroup.border `#34352F`; focus
  focusBorder `#99947C`. `accent` button/badge/progress `#75715E` with `#F8F8F2` on it. `list` active
  selection `#75715E`, inactive `#414339`, hover `#3E3D32`. `tab` active `#272822`, inactive `#34352F`
  with `#CCCCC7`, border `#1E1F1C`. Status bar `#414339`. Inputs/dropdowns `#414339`.
- `[editor]`: foreground `#F8F8F2`, selection `#878B9180`, cursor `#F8F8F0`, active line `#3E3D32`,
  line numbers `#90908A` / active `#C2C2BF`, whitespace and indent guide `#464741`, active guide
  `#767771`, word highlight `#4A4A7680` (strong `#6A6A9680`), selection highlight `#575B6180`, popover
  editorHoverWidget `#414339` border `#75715E`, suggest widget `#272822` border `#75715E`, diff inserted
  `#4B661680`, removed `#90274A70`. Chosen (upstream sets none): error `#F92672`, warning `#E2E22E`,
  info `#819AFF` (inputValidation borders), hint `#66D9EF`; git change bar added `#A6E22E`, modified
  `#E6DB74`, deleted `#F92672` (markup.inserted/changed/deleted); brackets three distinct accents
  (yellow, purple, cyan); jump labels pink live / cyan preview with a `#272822` glyph; vim mode segments
  cyan, green, purple, pink, orange with `#272822` labels; quick scope, yank highlight and the other
  keys from the palette, keeping `jump_label_contrast`, `vim_mode_colors`, `quick_scope_colors` and the
  bracket check green.
- `[syntax]` (one colour per highlight name; the general scope wins): `comment` `#88846F`; `string`
  `#E6DB74`; `string.placeholder` and `escape` (constant.other / constant.character) `#AE81FF`, so
  placeholders stand apart from strings; `number`, `constant`, `constant.builtin` (constant.language),
  `constant.enum_member` `#AE81FF`; `keyword`, `operator` (keyword.operator) `#F92672`; `type`,
  `type.parameter`, `constructor`, `module` (entity.name.type / class / namespace) `#A6E22E`;
  `type.builtin` (support.type, storage.type) `#66D9EF`; `function`, `attribute`
  (entity.other.attribute-name) `#A6E22E`; `function.builtin`, `function.macro` (support.function)
  `#66D9EF`; `variable`, `property`, `punctuation`, `label`, `string.special`, `string.special.key` the
  foreground `#F8F8F2` (JSON keys: upstream's `#CFCFC2` rule is JSON-only, leave it out); `variable.parameter`
  and `variable.builtin` (variable.language this/self) `#FD971F`; `tag` `#F92672`; `punctuation.special`
  (template-expression punctuation) `#F92672`; `markup.heading` `#A6E22E`, `markup.inserted` `#A6E22E`,
  `markup.deleted` `#F92672`, `markup.raw` `#FD971F`, `markup.link` `#AE81FF`, `markup.link.url`
  `#E6DB74`. Set the names `catppuccin-mocha.toml` sets plus `string.placeholder`, each one a name
  `Highlight::from_capture` accepts; the header lists any name left to its dotted parent.

## Not here

Monokai Pro or any other Monokai variant (Dimmed, Pro filters), a light Monokai, font styles
(italic/underline), a VS Code JSON importer, making Monokai the default, terminal ANSI colours (no theme
sets them), any wizard or picker code change, `docs/design-language.md`, `TESTING.md`.

## Platform

Mac-only: the `setup_wizard_dark.png` golden must be regenerated, and only macOS renders and compares
the workbench screenshots (TESTING.md, Screenshots). Everything else is headless.
