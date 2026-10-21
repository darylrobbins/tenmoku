---
task_uid: "01m5dyb4vs6azb1f6drhxt3g04"
task: "T1014"
rev: 2
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-syntax test semantic_names (new file): from_capture gives type.parameter and constant.enum_member, and a theme styling only type and constant styles both"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test semantic_theme_keys (new file): the four bundled themes each define variable.parameter, type.parameter and constant.enum_member; ember_theme.rs changes only its count 25 to 28"
    required: true
    manual: false
  - id: "C3"
    text: "cargo run -p fe-gpui --example screenshots and fe-workbench --example screenshots pass against regenerated goldens; cargo nextest run -p fe-syntax -p fe-config and clippy pass"
    required: true
    manual: false
---

# Add type.parameter and constant.enum_member highlight names and theme colours for them and variable.parameter in the four bundled themes, for semantic tokens

## Problem

The user asked how syntax highlighting gets as good as VS Code's and approved semantic distinctions (2026-10-20). T946 maps language servers' semantic tokens onto fe-syntax's highlight names, but the vocabulary (`crates/fe-syntax/src/highlight.rs` `NAMES`) has no name for a type parameter or an enum member, and the bundled themes colour neither those nor `variable.parameter`, so rust-analyzer's `typeParameter`, `enumMember` and `parameter` tokens would land as the surrounding type, constant or variable colour. VS Code themes colour all three.

## Change

1. `crates/fe-syntax/src/highlight.rs`: add `type.parameter` and `constant.enum_member` to `NAMES` (kept sorted); a theme that colours only `type` or `constant` still colours them (the dotted fallback in `resolve_theme`). No `ALIASES` change.
2. `[syntax]` in `crates/fe-config/themes/fastedit-dark.toml`, `fastedit-light.toml`, `fastedit-ember.toml`, `vscode-dark-modern.toml`: set `variable.parameter`, `type.parameter` and `constant.enum_member` (VS Code Dark Modern: `#9CDCFE`, `#4EC9B0`, `#4FC1FF`; pick matching tones in the other three). Do not add a plain `variable` key: that would colour every identifier.
3. Not in this card (needs the user): VS Code underlines rust-analyzer's `mutable` modifier; fastedit's `Foreground` decoration is colour only, so that is a separate decoration-kind card.
4. Tests: new `crates/fe-syntax/tests/semantic_names.rs`: `Highlight::from_capture` of `type.parameter` and `constant.enum_member` is those names, and `resolve_theme` with only `type` and `constant` styled gives both their parent's style; new `crates/fe-config/tests/semantic_theme_keys.rs`: each of the four bundled themes defines the three keys, distinct from `type` and `constant`. `crates/fe-config/tests/ember_theme.rs` changes its capture count 25 -> 28 only.
5. Screenshot goldens: tree-sitter-rust already captures `(parameter (identifier) @variable.parameter)`, so colouring `variable.parameter` changes the highlighted-Rust goldens. Regenerate every golden that changes (`UPDATE_SCREENSHOTS=1 cargo run -p fe-gpui --example screenshots`, the same for fe-workbench) and look at each changed PNG. Mac only (Metal). Builds on T1013 (bracket colours on), which regenerates the same goldens and edits `vscode-dark-modern.toml` first.
