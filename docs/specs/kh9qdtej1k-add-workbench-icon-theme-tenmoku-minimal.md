---
task_uid: "01m6j4gtevsr1s80kh9qdtej1k"
task: "T1362"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "icon_theme_setting::default_is_tenmoku_and_project_file_ignored: default is tenmoku, minimal parses, an unknown value errors naming the key, a project file value is ignored"
    required: true
    manual: false
  - id: "C2"
    text: "tests::icon_theme_install::reload_switches_set: a .rs tree row draws an image by default, the Lucide file-code svg after reload with minimal, and an image again after tenmoku"
    required: true
    manual: false
  - id: "C3"
    text: "cargo run -p fe-workbench --example screenshots passes on the Mac with the new file_icons_dark and file_icons_light goldens"
    required: true
    manual: false
  - id: "C4"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Add workbench.icon_theme (tenmoku | minimal), default the coloured set, install it on startup and settings reload, with light and dark file-icon screenshot goldens

## Problem

After cards A to C the coloured set exists but nothing installs it, and Lucide is the only look. Users should pick: coloured by default, Lucide as the minimal option.

## Change

- `crates/fe-config/src/settings.rs`: add `workbench.icon_theme` (a new `WorkbenchSettings` struct, or the nearest existing home if one exists; the implementer checks) as an enum `IconTheme { Tenmoku, Minimal }`, serde lower-case, default `Tenmoku`, with a doc comment (it shows in the generated schema and the Settings tab). A project file can't set it (appearance is the user's, as `theme`).
- `crates/fe-workbench`: on startup and on `reload_settings`, install `ColouredIcons` for `tenmoku` and `LucideIcons` (`reset_icon_set`) for `minimal` through `set_icon_set`, which already redraws every window.
- Screenshot goldens: add file-icon rows to the workbench screenshots example (new `crates/fe-workbench/examples/screenshots/icons_scene.rs`, wired from `main.rs` / `scenes.rs`): the project tree with a Rust, TOML, Makefile, justfile, Markdown, a `src` and a `node_modules` folder (one expanded), dark and light, goldens `golden/file_icons_dark.png`, `golden/file_icons_light.png`. Existing goldens and the token probe must not change except where the tree icons now show (regenerate and say so). Per TESTING.md the scenes render only under `cargo run` on macOS.

## Tests

New `crates/fe-config/tests/icon_theme_setting.rs` (default is tenmoku; 'minimal' parses; unknown value is an error naming the key; project file value ignored) and new `crates/fe-workbench/src/tests/icon_theme_install.rs` (default window's tree shows an image icon for a .rs file; after `reload_settings` with 'minimal' it shows the Lucide 'file-code' svg; and back).
