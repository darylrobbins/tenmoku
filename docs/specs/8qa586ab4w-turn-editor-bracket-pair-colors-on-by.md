---
task_uid: "01m5dyawgtm2ncjb8qa586ab4w"
task: "T1013"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config unit test module bracket_pair_colors_default (new file crates/fe-config/src/tests/bracket_pair_colors_default.rs): EditorSettings::default().bracket_pair_colors is true, a settings file can set it false, and all four bundled themes give bracket_1..3 three distinct colours"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test bracket_colors passes with only its default-is-false assertion changed to true; fe-config schema_matches_golden and template tests pass"
    required: true
    manual: false
  - id: "C3"
    text: "cargo run -p fe-gpui --example screenshots and fe-workbench --example screenshots pass against regenerated goldens; cargo nextest run -p fe-config -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# Turn editor.bracket_pair_colors on by default, as VS Code does: settings default, template, schema, vscode-dark-modern bracket keys and the screenshot goldens

## Problem

The user asked how syntax highlighting gets as good as VS Code's and approved turning on bracket-pair colours (2026-10-20). The feature exists and is tested: `editor.bracket_pair_colors` colours brackets by nesting depth in the theme's `bracket_1..3` (`crates/fe-gpui/src/element.rs` `styled_highlights`, pairs from `fe_syntax::bracket_depths`), landed as helium T195 for Mac T988. T988 is abandoned because that merge delivered it, not because the work failed. It shipped off "so existing screenshot goldens don't change"; VS Code has it on by default (`editor.bracketPairColorization.enabled`).

## Change

1. `crates/fe-config/src/settings.rs`: `EditorSettings::default()` sets `bracket_pair_colors: true` (line ~447). Nothing else in the file changes.
2. `crates/fe-config/settings-template.toml`: the commented example reads `# bracket_pair_colors = true`.
3. `crates/fe-config/settings.schema.json`: regenerate with `UPDATE_GOLDEN=1` (the schema golden test); both `default`s (the key's own and the `editor` default object) become true.
4. `crates/fe-config/themes/vscode-dark-modern.toml`: add `bracket_1..3` to `[editor]` with VS Code Dark Modern's `#ffd700`, `#da70d6`, `#179fff` (the other three bundled themes already set them).
5. `crates/fe-gpui/tests/bracket_colors.rs`: the one line in `off_until_the_setting_turns_it_on` asserting the default is false now asserts true (rename the test `on_until_the_setting_turns_it_off` if the body still checks both ways); no other edit.
6. Screenshot goldens: regenerate every golden that changes (`UPDATE_SCREENSHOTS=1 cargo run -p fe-gpui --example screenshots`, the same for fe-workbench). Mac only (Metal); look at each changed PNG for sane colours.
7. Tests: new unit-test module `crates/fe-config/src/tests/bracket_pair_colors_default.rs` (declared in `crates/fe-config/src/tests/mod.rs`; not under `crates/fe-config/tests/`, which running T1009 holds): `Settings::default().editor.bracket_pair_colors` is true; a settings file with `editor.bracket_pair_colors = false` turns it off; all four bundled themes resolve `bracket_1..3` to three distinct colours.

Mac-only for the goldens.
