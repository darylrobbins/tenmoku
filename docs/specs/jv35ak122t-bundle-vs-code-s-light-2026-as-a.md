---
task_uid: "01m5e7yvfyv3d4c9jv35ak122t"
task: "T1018"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "vscode_2026_light_theme: parses cleanly (name VS Code Light 2026, Light), no EditorPalette warnings, values equal the spec's tables"
    required: true
    manual: false
  - id: "C2"
    text: "not_the_default: find(\"light\") is still fastedit-light and find(\"Light 2026\") is vscode-2026-light"
    required: true
    manual: false
  - id: "C3"
    text: "NOTICE and THIRD_PARTY.md have the 2026-light.json row at the same commit; cargo test -p fe-config passes"
    required: true
    manual: false
---

# Bundle VS Code's Light 2026 as a selectable theme, not the default

## Problem

The user (2026-10-21) asked for VS Code's **Light 2026** as well as Dark 2026. File it as a bundled, selectable theme, NOT the default: fastedit Light stays the light theme when `[theme] name` is unset.

Upstream: microsoft/vscode `extensions/theme-defaults/themes/2026-light.json` at tag `1.142.0` (commit `d956e8a6ac5d904486557eea07e1d6bd62a4d36f`). MIT, Copyright (c) Microsoft Corporation. It `include`s `./light_modern.json` (-> `light_plus.json`, `light_vs.json`); its `tokenColors` come after those, so a scope it does not set keeps the Light Modern / Light+ colour. No `semanticTokenColors`, no bracket colours.

This card DEPENDS ON the Dark 2026 card (same shape; it edits the same shared files after it: `themes.rs`, `NOTICE`, `THIRD_PARTY.md`). Follow the dark card's file `crates/fe-config/themes/vscode-2026-dark.toml` for header wording and structure. `bundled_themes.rs` is already an invariant check by then; do not edit it. `settings-template.toml` is left alone (T1013 holds it).

## Change

1. New `crates/fe-config/themes/vscode-2026-light.toml`, `kiln_theme = 1`, `name = "VS Code Light 2026"`, `appearance = "light"`, header as the dark one's.
2. `crates/fe-config/src/themes.rs`: add `bundled!("Light 2026", "vscode-2026-light", Light)` after the Dark 2026 entry. `NOTICE` and `THIRD_PARTY.md`: the same two additions as the dark card, for the light file and `2026-light.json`, `light_modern.json`, `light_plus.json`, `light_vs.json`.
3. New `crates/fe-config/tests/vscode_2026_light_theme.rs`: parses cleanly, name and Light appearance, no `EditorPalette` warnings, the values in the tables for editor background/foreground/selection, `comment`, `keyword`, `function`, `string`, `surface.background`, `surface.panel`; `syntax_keys_are_known`; `not_the_default`: `find("light")` still gives `fastedit-light`, `find("Light 2026")` gives `vscode-2026-light`, and its `appearance` is Light so it is the light theme only when named.
4. Same one-line fix-ups as the dark card's item 6 and the `jump_label_contrast.rs` / `vim_mode_colors.rs` readability checks (jump labels and vim segments need dark text on light fills or the reverse; choose to pass).

## Chrome: in, through kiln's token tables (as the dark card: no per-component keys, sidebar/status bar/tabs map onto `surface.panel`)

surface.background #FFFFFF; surface.panel #FAFAFD; surface.raised #FAFAFD; surface.overlay #00000040; text.primary #202020; text.muted #606060; text.disabled #BBBBBB; text.accent #0069CC; text.on_accent #FFFFFF; border.default #E4E5E6; border.subtle #F0F1F2; border.focused #0069CC; accent.base #0069CC; state.hover #00000014; state.pressed #00000025; state.selected #00000025; state.selected_inactive #DADADA99; state.focus_ring #0069CC; status.error #AD0707, warning #B69500, info #0069CC, success #587C0C; list.chevron/icon/folder_icon #606060; tab.active #FFFFFF, tab.inactive #FAFAFD, tab.active_text #202020; scrollbar.thumb #646464C0, thumb_hover #646464D0. Tokens not listed derive by kiln's rules.

`[editor]`: background #FFFFFF, foreground #202020, gutter_foreground #606060, gutter_active_foreground #202020, active_line #EAEAEA40, selection #0069CC40, cursor #202020, error #AD0707, warning #B69500, information #0069CC, hint #6C6C6CB2 (VS Code default light hint), popover_background #FAFAFD, popover_border #E4E5E6, change_added #587C0C, change_modified #005FB8 (inherited from light_modern), change_deleted #AD0707, indent_guide #F7F7F740, indent_guide_active #EEEEEE, bracket_1 #0431FA, bracket_2 #319331, bracket_3 #7B3814 (VS Code's light defaults). `jump_label*` and `vim_mode_*`: chosen from the palette, readable.

## `[syntax]` (general-scope rule, as the dark card)

From 2026-light.json: `comment` #6E7781; `keyword` #CF222E (keyword, storage, storage.type: the common case in Rust/TS; `keyword.control` stays #AF00DB in VS Code, which fastedit has no separate name for; say so in the header); `function`, `function.macro` #8250DF; `string` #0A3069; `tag` #116329; `variable` #1F2328 (variable.other); `variable.builtin` #0550AE (variable.language); `markup.heading`, `markup.raw` #0550AE; `punctuation.special` #CF222E; `markup.link`, `markup.link.url` #0A3069.
Unchanged from Light Modern / Light+ (2026 sets nothing more specific): `number` #098658; `constant` #0070C1; `constant.builtin` #0000FF; `type`, `module`, `constructor` #267F99; `operator` #000000; `property`, `attribute` #001080 for property keys; `escape` #EE0000 (light_plus `constant.character.escape`); `string.special` #0A3069 (2026-light overrides `string.regexp` to #0a3069, the scope Dark Modern's `string.special` follows); `label` #000000. The lead checked type, escape and string.special against VS Code 1.142.0's light_plus.json, light_vs.json and 2026-light.json. Names to copy from `fastedit-light.toml` if neither file speaks: `punctuation`, `punctuation.bracket`.
Follow-ups, not here: `type.parameter`, `constant.enum_member`, `variable.parameter` once T1014 lands.

## Platform

Linux-OK: no golden changes. `cargo test -p fe-config`.
