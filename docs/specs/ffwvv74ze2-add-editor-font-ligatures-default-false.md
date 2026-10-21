---
task_uid: "01m5f7awwk813cj0ffwvv74ze2"
task: "T1026"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "editor.font_ligatures defaults to false and appears in the settings template"
    required: true
    manual: false
  - id: "C2"
    text: "With the default, the editor's row font has calt disabled; with true it doesn't"
    required: true
    manual: false
  - id: "C3"
    text: "Changing the setting reshapes open editors live"
    required: true
    manual: false
  - id: "C4"
    text: "Hover, completion and signature-help code use the same features"
    required: true
    manual: false
---

# Add editor.font_ligatures, default false as VS Code's editor.fontLigatures, and shape the editor font with calt off unless it is set

## Problem (user report: fastedit vs VS Code, both JetBrains Mono)

fastedit draws `=>` as ⇒ and `->` as →: JetBrains Mono's `calt` ligatures are on, because the
editor font is `gpui::font(theme.font_family)` with default features (`crates/fe-gpui/src/element.rs`
~638 and the jump labels ~2641, `crates/fe-gpui/src/view.rs` ~1479). There is no setting for it.
VS Code's `editor.fontLigatures` defaults to false, and the user's VS Code shows `=>` and `->` as two
characters.

## Fix

- Add `editor.font_ligatures: bool` to `EditorSettings` in `crates/fe-config/src/settings.rs`,
  default `false` as in VS Code, with a doc comment and schema. Add it to
  `crates/fe-config/settings-template.toml` next to `font_family`.
- `EditorTheme` (`crates/fe-gpui/src/lib.rs`) gets `font_features: gpui::FontFeatures`, set in
  `set_metrics`:
  - `FontFeatures::disable_ligatures()` (calt=0) when false;
  - `FontFeatures::default()` when true.
- Use it everywhere the editor font is built: element.rs and view.rs `Font`s. Set
  `.font_features(...)` once on the editor view's root so code drawn in the editor's font (hover,
  completion and signature-help code blocks) inherits it. Pass it to the markdown view's code font
  only through `MarkdownStyle` (`crates/fe-markdown-view/src/style.rs`) if inheritance doesn't
  reach it.
- Live: changing the setting re-shapes open editors, as `font_family` does.
- Out of scope: the terminal, which already keeps ligatures within cells, and VS Code's
  string-of-features form of the setting.

## Tests

- New `crates/fe-config/tests/font_ligatures_setting.rs`:
  - default is false;
  - `[editor] font_ligatures = true` parses;
  - the template documents it.
- New `crates/fe-gpui/src/font_ligatures_tests.rs`, registered in lib.rs:
  - with default settings the editor's row font has `is_calt_enabled() == Some(false)`;
  - with `font_ligatures = true` it doesn't disable calt;
  - flipping the setting changes the shaped font of an open editor.
- On the Mac, regenerate any screenshot golden whose text contains `=>`/`->`/`!=` (Mac-only:
  CoreText shapes the ligatures).
