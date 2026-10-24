---
task_uid: "01m5nqx2fz422yb9cga21xm10k"
task: "T304"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "catppuccin_themes::each_flavour_loads: find() by title and by id returns each of the four; each file parses with no diagnostics under EXTENSIONS; EditorPalette::for_theme returns no warnings; appearance is Light for Latte and Dark for the other three; Settings::default().theme.name is still None"
    required: true
    manual: false
  - id: "C2"
    text: "catppuccin_themes::matches_upstream: for each flavour editor background, foreground, selection and cursor, surface.background, and the colours of comment, keyword, function, string, type and number equal the pinned upstream theme's values (Mocha base #1E1E2E, Latte base #EFF1F5 among them)"
    required: true
    manual: false
  - id: "C3"
    text: "catppuccin_themes::syntax_keys_are_known: every [syntax] key of the four files is a name Highlight::from_capture accepts, and each sets variable.parameter, type.parameter and constant.enum_member"
    required: true
    manual: false
  - id: "C4"
    text: "catppuccin_themes::readable: for each flavour the jump labels and the five vim mode segments have a label/fill contrast ratio of at least 3, and the three bracket colours are distinct"
    required: true
    manual: false
  - id: "C5"
    text: "The theme picker lists Catppuccin Latte (Light), Catppuccin Frappe, Catppuccin Macchiato and Catppuccin Mocha (Dark) after Light 2026: cargo nextest run -p fe-workbench theme_picker passes"
    required: true
    manual: false
  - id: "C6"
    text: "NOTICE has a Catppuccin MIT entry for each of the four files and THIRD_PARTY.md a row each with the upstream path and commit; cargo nextest run -p fastedit --test notice --test third_party passes"
    required: true
    manual: false
  - id: "C7"
    text: "cargo nextest run -p fe-config passes, including template_theme_names, bracket_pair_colors_default, minimap_theme_keys and sticky_scroll_colors with ten bundled themes"
    required: true
    manual: false
---

# Bundle Catppuccin Latte, Frappe, Macchiato and Mocha as selectable themes (Mac T1122)

# Bundle Catppuccin's four flavours as selectable themes

## Problem

The user (2026-10-23) asked to introduce Catppuccin themes to fastedit. Bundle all four official flavours as built-in, selectable themes, none the default: Latte (light), Frappe, Macchiato, Mocha (dark). `fastedit Dark` / `fastedit Light` stay the defaults when `[theme] name` is unset.

Upstream: catppuccin/vscode (MIT, Copyright (c) Catppuccin), the built theme files (`themes/latte.json`, `frappe.json`, `macchiato.json`, `mocha.json` as published in the repo / the extension package). The implementer pins the commit read and records it in each file header and in THIRD_PARTY.md.

How bundled themes work today (read before starting): each theme is a hand-written kiln theme TOML in `crates/fe-config/themes/` (`kiln_theme = 1`, `name`, `appearance`, kiln token tables for chrome, `[editor]` colours, `[syntax]` colours by dotted highlight name), registered by one `bundled!(title, id, Appearance)` line in `crates/fe-config/src/themes.rs`. There is NO VS Code JSON importer; do not build one. Convert by hand (or with a throwaway script kept out of the repo), as the Dark 2026 / Light 2026 cards did (see `docs/specs/*tktq6bx0c2*` and `*jv35ak122t*` for the shape, and `crates/fe-config/themes/vscode-2026-light.toml` for header wording and structure). The model is one colour per highlight name (names of `fe_syntax::Highlight::all()`), so a TextMate scope with no highlight name is left out and the header says which; `semanticTokenColors` map onto the same highlight names (e.g. `variable.parameter`, `type.parameter`, `constant.enum_member`) and the `*.mutable` / style rules are left out (T1121 owns underline). Picker, `[theme] name`, hot reload and kiln install (`crates/fastedit/src/theme.rs`) iterate `bundled_themes()`, so they need no change.

## Change

1. Four new files in `crates/fe-config/themes/`: `catppuccin-latte.toml` (name "Catppuccin Latte", light), `catppuccin-frappe.toml`, `catppuccin-macchiato.toml`, `catppuccin-mocha.toml` (dark). Header: upstream file and pinned commit, MIT notice pointer to NOTICE, what is left out. Colours from the upstream theme's `colors` (workbench), `tokenColors` and `semanticTokenColors`: chrome through kiln's token groups (surface, text, border, accent, state, status, list, tab, scrollbar); `[editor]` background, foreground, gutter, active line, selection, cursor, diagnostics, popover, git change colours, indent guides, bracket colours, and every key `EditorColors::KEYS` lists (minimap_*, sticky scroll, jump_label*, vim_mode_*); where upstream sets none, choose from the flavour's palette. Chosen values must keep the existing readability and distinctness checks green.
2. `crates/fe-config/src/themes.rs`: add four `bundled!` lines after "Light 2026": "Catppuccin Latte" (Light), "Catppuccin Frappe", "Catppuccin Macchiato", "Catppuccin Mocha" (Dark); ids as the file stems. Titles use plain ASCII "Frappe" (document in the header that upstream is "Frappé"); `find` also takes the id.
3. `NOTICE`: one Catppuccin entry per file (Copyright (c) Catppuccin, MIT) in the existing VS Code entries' style. `THIRD_PARTY.md`: four rows (file, upstream repo and path, commit, MIT) so `cargo nextest run -p fastedit --test notice --test third_party` passes. If upstream's licence text names a different copyright holder or year than assumed, use the LICENSE file's exact line.
4. `crates/fe-config/settings-template.toml`: add the four titles to the `# A bundled theme by name:` comment (test `template_theme_names` fails otherwise).
5. Existing tests that enumerate the bundled themes and will fail with ten themes, edited minimally: `crates/fe-config/tests/minimap_theme_keys.rs` (`len() == 6`; the slider/selection `match` falls to the VS Code default arm, so either set those colours in the new files to the default arm's values or add arms keyed by id; minimap background/warning/gutter/error/find-match must equal what the test requires) and `crates/fe-config/tests/sticky_scroll_colors.rs` (the `want` table of 6 and its length assert: add four rows). `crates/fe-workbench/src/theme_picker_tests.rs`: add the four rows to the expected picker list after "Light 2026" and before "Fastedit Dark", in the same order as `BUNDLED`.
6. New test file `crates/fe-config/tests/catppuccin_themes.rs` (do not extend other test files for these): see criteria.

## Not here

Semantic-token styling rules (underline, italic), an importer for VS Code theme JSON, making any Catppuccin flavour the default or auto-pairing flavours to light/dark mode, extension-installed Catppuccin, goldens.

## Platform

Linux-OK: headless tests only (`cargo nextest run -p fe-config`, `-p fe-workbench theme_picker`, `-p fastedit --test notice --test third_party`). No screenshot goldens exist for themes; do not add any.

Lend note (helium lead): full JSON from the Mac (y56).

## Implementation notes

- Upstream: catppuccin/vscode commit befc9e6fc41980f4241408f7049755d47c06ff45
  (tag catppuccin-vsc-v3.19.0). The built `packages/catppuccin-vsc/themes/*.json`
  are gitignored there, so the values come from the published
  Catppuccin.catppuccin-vsc 3.19.0 package (Open VSX), which that tag builds.
  The LICENSE line is "Copyright (c) 2021 Catppuccin", used in NOTICE and the
  headers.
- Converted with a throwaway script (not committed): each highlight name takes
  the most specific single-scope tokenColors rule for a representative scope,
  as each file's header lists; `constant.enum_member` is semanticTokenColors'
  `enumMember`. Names the files set beyond fastedit Dark/Light's list:
  `function.builtin`, `type.builtin` (upstream's general built-in rules) and
  `variable`.
- Chosen from the palette where upstream sets nothing: hint teal; text.muted
  subtext0. Jump labels: pink live / teal preview with crust glyph in the dark
  flavours; mauve / teal with base glyph in Latte, whose pink, green and peach
  are too light for any palette glyph to reach 3:1. Vim modes: blue, green,
  mauve, red, peach on crust (dark); Latte uses teal for insert and maroon for
  command, its only other fills above 3:1 with a base label.
- The minimap's background, warning, gutter, error and find-match marks follow
  the existing minimap test (editor's / VS Code defaults), not upstream's
  translucent minimap.* values; the slider and selection marks are upstream's,
  with id-keyed arms added to `minimap_colours_are_vscode_s`.
