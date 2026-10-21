---
task_uid: "01m5e7ymfct4mw1dtktq6bx0c2"
task: "T1017"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "vscode_2026_dark_theme: parses cleanly (name VS Code Dark 2026, Dark), no EditorPalette warnings, editor, syntax and chrome values equal the spec's tables"
    required: true
    manual: false
  - id: "C2"
    text: "not_the_default: Settings::default().theme.name is None, find(\"dark\") is still fastedit-dark, find(\"Dark 2026\") is vscode-2026-dark"
    required: true
    manual: false
  - id: "C3"
    text: "NOTICE and THIRD_PARTY.md have the 2026-dark.json row at 1.142.0 commit d956e8a6ac5d904486557eea07e1d6bd62a4d36f; fastedit third_party and notice tests pass"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-config passes, bundled_themes.rs no longer hard-codes the title list, and the readability checks (jump_label_contrast, vim_mode_colors) pass for the new theme"
    required: true
    manual: false
---

# Bundle VS Code's Dark 2026 as a selectable theme, not the default

## Problem

The user (2026-10-21) compared fastedit's bundled "Dark Modern" (T994) with VS Code and found it looks nothing alike: VS Code 1.142's default dark theme is not Dark Modern but **Dark 2026**. File it as a bundled, selectable theme. It must NOT become the default: `[theme] name` unset still gives fastedit Dark, and the order of the picker's built-in entries other than the new one does not change.

Upstream: microsoft/vscode `extensions/theme-defaults/themes/2026-dark.json` at tag `1.142.0` (commit `d956e8a6ac5d904486557eea07e1d6bd62a4d36f`; the installed app's own build commit is `6d62b1d5dace93a33b3a6d3eca1317bd5e15bcb1`, same file). MIT, Copyright (c) Microsoft Corporation. It `include`s `./dark_modern.json` (which includes `dark_plus.json`, `dark_vs.json`): its `tokenColors` are laid after the included ones, so a scope it does not set keeps the Dark Modern / Dark+ colour. It has no `semanticTokenColors` of its own and no bracket colours (`editorBracketHighlight.*` absent).

## Change (names beyond these files are out of scope)

1. New `crates/fe-config/themes/vscode-2026-dark.toml`, `kiln_theme = 1`, `name = "VS Code Dark 2026"`, `appearance = "dark"`. Header like `vscode-dark-modern.toml`'s: upstream files, the commit above, MIT notice in NOTICE, which values are VS Code defaults vs chosen, which scopes were left out. Copy the `[syntax]` names and every unchanged value from `vscode-dark-modern.toml`, then override as below.
2. `crates/fe-config/src/themes.rs`: add `bundled!("Dark 2026", "vscode-2026-dark", Dark)` to `BUNDLED`, after Dark Modern. Do not touch `Settings` defaults or `theme.rs`'s `include_str!` of the fastedit themes (those are the fallbacks; this theme is only in `BUNDLED`).
3. `NOTICE`: a second VS Code line for the new file (same wording as the Dark Modern entry). `THIRD_PARTY.md`: a row `crates/fe-config/themes/vscode-2026-dark.toml` | VS Code `extensions/theme-defaults/themes/2026-dark.json`, `dark_modern.json`, `dark_plus.json`, `dark_vs.json` | `d956e8a6ac5d904486557eea07e1d6bd62a4d36f` | MIT. `crates/fastedit/tests/third_party.rs` must pass unedited.
4. `crates/fe-config/tests/bundled_themes.rs`: `every_entry_parses` hard-codes the title list; replace that assertion with invariants (titles and IDs unique, every title and ID is found by `find`, `Dark Modern` first or not, no order claim) so the next theme needs no edit there. Nothing else in that file.
5. New `crates/fe-config/tests/vscode_2026_dark_theme.rs` (model: `vscode_dark_modern_theme.rs`): parses cleanly under `EXTENSIONS` with name `VS Code Dark 2026` and Dark appearance; `EditorPalette::for_theme` has no warnings; `matches_upstream` asserts the values in the tables below for editor background/foreground/selection, `comment`, `keyword`, `function`, `string`, `tag`, and `surface.background`, `surface.panel`, `border.default`; `syntax_keys_are_known`; `not_the_default`: `Settings::default().theme.name` is None and `find("Dark 2026")` gives id `vscode-2026-dark`, and `find("dark")` still gives `fastedit-dark`.
6. If `crates/fe-workbench/src/theme_picker_tests.rs` or `crates/fastedit/src/theme_name_tests.rs` hard-code the list of bundled rows and fail, make the smallest edit to accept the new row; otherwise leave them.

Left out on purpose: `crates/fe-config/settings-template.toml`'s sentence naming the bundled themes (T1013 edits that file; a follow-up adds "Dark 2026" there). Bracket colours: T1013 adds `bracket_1..3` only to the other themes' tables; Dark 2026 sets its own below (VS Code's defaults, since the theme sets none), so `editor.bracket_pair_colors` looks right on it.

## Chrome: in, through kiln's token tables

Kiln themes can colour chrome, but by semantic token group (`[surface] [text] [border] [accent] [state] [status] [list] [tab] [scrollbar]`, as `fastedit-ember.toml` does), not per component: there is no sidebar/status-bar/activity-bar/title-bar key and this card does not add any. Dark 2026's sidebar, activity bar, status bar, title bar and inactive tabs share one colour, so they map onto `surface.panel`. A token not set is derived by kiln's rules; set only these:

| token | value | from |
|---|---|---|
| surface.background | #121314 | editor.background |
| surface.panel | #191A1B | sideBar/activityBar/statusBar/titleBar.background |
| surface.raised | #202122 | menu / editorWidget / quickInput.background |
| surface.overlay | #00000080 | (Ember's) |
| text.primary | #BFBFBF | foreground |
| text.muted | #8C8C8C | descriptionForeground |
| text.disabled | #555555 | disabledForeground |
| text.accent | #48A0C7 | textLink.foreground |
| text.on_accent | #FFFFFF | button.foreground |
| border.default | #2A2B2C | sideBar.border, widget.border |
| border.subtle | #2A2B2C | |
| border.focused | #3994BC | panelTitle.activeBorder, tab.activeBorderTop |
| accent.base | #297AA0 | button.background |
| accent.hover | #2B7DA3 | button.hoverBackground |
| accent.subtle | #3994BC26 | menu.selectionBackground |
| state.hover | #FFFFFF14 | list.hoverBackground |
| state.pressed | #FFFFFF22 | list.activeSelectionBackground |
| state.selected | #FFFFFF22 | list.activeSelectionBackground |
| state.selected_inactive | #2C2D2E | list.inactiveSelectionBackground |
| state.focus_ring | #3994BCB3 | focusBorder |
| status.error / warning / info / success | #F48771 / #E5BA7D / #3A94BC / #73C991 | errorForeground, list.warningForeground, notificationsInfoIcon, gitDecoration.addedResourceForeground |
| list.indent_guide | #8384854D | editorIndentGuide.background1 |
| list.chevron, list.icon, list.folder_icon | #8C8C8C | icon.foreground |
| tab.active / tab.inactive / tab.active_text | #121314 / #191A1B / #BFBFBF | tab.* |
| scrollbar.thumb / thumb_hover | #A8A9AA85 / #A8A9AA90 | scrollbarSlider.* |

`[editor]`: background #121314, foreground #BBBEBF, gutter_foreground #858889, gutter_active_foreground #BBBEBF, active_line #242526, selection #276782DD, cursor #BBBEBF, error #F48771, warning #CCA700, information #3A94BC, hint #EEEEEEB2 (VS Code default), popover_background #202122, popover_border #2A2B2C, change_added #72C892, change_modified #0078D4 (inherited from dark_modern; 2026 sets none), change_deleted #F28772, indent_guide #8384854D, indent_guide_active #838485, bracket_1 #FFD700, bracket_2 #DA70D6, bracket_3 #179FFF (VS Code defaults). `jump_label*` and `vim_mode_*`: no upstream value; pick from the palette (accent #3994BC, #7EE787, #D2A8FF, #FF7B72, #FFA657) with dark foreground #121314, and keep them readable: `jump_label_contrast.rs` and `vim_mode_colors.rs` check the bundled themes and must pass (add the new theme to them only if they enumerate by hand and the edit is one line each; they are in this card's scope for that).

## `[syntax]` (one colour per highlight name; rule: a name takes the general scope's colour, as T994 did)

Overridden by 2026-dark.json: `comment` #8B949E (comment); `keyword` #FF7B72 (`keyword` and `storage`/`storage.type`, which is `fn`, `let`, `struct`, `const`, `pub`, `function`, `class` in Rust/TS; the common case); `function` and `function.macro` #D2A8FF (entity.name.function, so Rust `name!` macros too); `string` #A5D6FF; `tag` #7EE787 (entity.name.tag); `variable` #C9D1D9 (variable.other, the scope an ordinary Rust/TS local has; 2026's plain `variable` #FFA657 applies only to scopes without `.other`); `variable.builtin` #79C0FF (variable.language: self, this); `markup.heading` #79C0FF; `markup.raw` #79C0FF; `punctuation.special` #FF7B72 (punctuation.section.embedded); `markup.link` and `markup.link.url` #A5D6FF (string.other.link).
Chosen against VS Code: `keyword.control` (if, match, for, return, await) stays #C586C0 in VS Code because 2026 does not override that more specific Dark+ rule; fastedit has one `keyword` name, so control flow shows #FF7B72 here. Say so in the header. Do not add a keyword.control highlight name.
Unchanged from Dark Modern (2026 sets nothing more specific): `type`, `module`, `constructor` #4EC9B0; `number` #B5CEA8; `constant` #4FC1FF; `constant.builtin` #569CD6; `operator` #D4D4D4; `property` and `attribute` #9CDCFE; `escape` #D7BA7D; `label` #C8C8C8; `string.special` #A5D6FF (2026-dark overrides `string.regexp` to #a5d6ff, the scope Dark Modern's `string.special` follows); `string.special.key` #9CDCFE; `punctuation` #CCCCCC. Copy each from `vscode-dark-modern.toml`; if one is wrong for Dark 2026 (a rule in the JSON you can see overrides it), follow the JSON and say so in the header.
Follow-ups, not in this card: `type.parameter` and `constant.enum_member` (T1014 adds the names; Dark Modern gives `#4EC9B0` and `#4FC1FF`) and `variable.parameter` (#C9D1D9 in 2026).

## Platform

Linux-OK: no screenshot golden changes (the theme is not the default, and the picker has no golden). `cargo test -p fe-config` and, if edited, `-p fe-workbench -p fastedit`.
