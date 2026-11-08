---
task_uid: "01m6vvs31bwjc7kg2n4cnm5ngh"
task: "T1517"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test tests::tinted_icon_set::types_and_folder_roles_take_their_tint (cargo nextest run -p kiln-ui) passes: a .rs file takes file_icon.rust, Cargo.toml file_icon.toml, README.md the Markdown icon in file_icon.markdown, src/ tests/ docs/ their folder roles; a theme that sets none draws them in list.icon"
    required: true
    manual: false
  - id: "C2"
    text: "Test material_and_minimal_parse (cargo nextest run -p fe-config --test icon_theme_setting) passes: tenmoku, material and minimal parse; a project file's value is still dropped with a warning"
    required: true
    manual: false
  - id: "C3"
    text: "Hakuji's folder tints are one neutral grey (crates/fe-config/tests/yohen_hakuji_values.rs case added or a new case here); design_language_doc passes"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (workbench.png, setup_wizard_*.png, file_icons_*.png)"
    required: true
    manual: false
---

# File-type icons in the palette: workbench.icon_theme tenmoku tints line icons by file type and folders by role from the theme; Material's set kept as material

# File-type icons in the palette

Spec: `docs/design/design-system.md#workbench-anatomy` (Icons: file-type icons
sit in the palette, Rust in muted kaki, TOML grey, Markdown lapis; folders tint
by role, source, tests, docs, and go neutral grey in Hakuji). Audit finding CH8
(README's info disc).

Today `workbench.icon_theme = "tenmoku"` is Material Icon Theme's coloured set
(`kiln/crates/kiln-ui/src/file_icons_coloured.rs`) and `"minimal"` is
monochrome Lucide.

## Change

- `"tenmoku"` becomes line icons tinted from the theme: a new `file_icon`
  token group in kiln-core (`rust`, `toml`, `markdown`, `config`, `data`,
  `script`, `web`, `image`, `default`; folder roles `folder_source`,
  `folder_tests`, `folder_docs`, `folder`; every rule = `list.icon`, so
  themes that don't set them stay monochrome), set in Yohen and Hakuji
  (Hakuji's folders neutral grey). New
  `kiln/crates/kiln-ui/src/file_icons_tinted.rs`: the `IconSet` mapping
  names/extensions to (Lucide icon, tint token), README to the Markdown icon.
- Material's coloured set stays as `workbench.icon_theme = "material"`;
  `"minimal"` unchanged (`crates/fe-config/src/settings.rs` `IconTheme`,
  schema, template, `crates/fe-config/tests/icon_theme_setting.rs`,
  `crates/fe-workbench/src/workbench_root.rs` where the set is applied).
- `docs/design-language.md`: `### file_icon` heading (the doc test).
- Regenerate `file_icons_{dark,light}.png`, `setup_wizard_{dark,light}.png` and
  `workbench.png` on the Mac.

Platform: **Mac-only** (goldens).
