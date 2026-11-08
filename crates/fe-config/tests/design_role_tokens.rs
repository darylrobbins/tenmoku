//! The design's new roles (T1509) are plumbing: each new token follows its
//! rule, or is the base's colour, and every token that was there before
//! resolves as it did, in Kiln Dark, Kiln Light and every theme Tenmoku
//! bundles. So nothing drawn changes until a theme sets them.

use std::{fs, path::Path};

use kiln_core::theme::{Color, Rule, SOFT, TOKEN_KEYS, ThemeData, ThemeFile, Tokens, rule};

/// The tokens this card added, and how each is found when a theme leaves
/// it out: a rule, or `None` for a root the bases set.
const NEW: &[(&str, Option<Rule>)] = &[
    ("surface.chrome", Some(Rule::Same("surface.panel"))),
    (
        "text.faint",
        Some(Rule::Mix("text.muted", "surface.background", 0.4)),
    ),
    ("border.control", Some(Rule::Same("border.default"))),
    ("status.modified", None),
    (
        "status.success_soft",
        Some(Rule::Alpha("status.success", SOFT)),
    ),
    ("status.error_soft", Some(Rule::Alpha("status.error", SOFT))),
    (
        "status.warning_soft",
        Some(Rule::Alpha("status.warning", SOFT)),
    ),
    (
        "status.modified_soft",
        Some(Rule::Alpha("status.modified", SOFT)),
    ),
    ("agent.base", None),
    ("agent.soft", Some(Rule::Alpha("agent.base", SOFT))),
    ("vcs.added", Some(Rule::Same("status.success"))),
    ("vcs.modified", Some(Rule::Same("status.modified"))),
    ("vcs.deleted", Some(Rule::Same("status.error"))),
    ("vcs.untracked", Some(Rule::Same("status.success"))),
    ("vcs.ignored", Some(Rule::Same("text.muted"))),
    ("vcs.conflict", Some(Rule::Same("status.warning"))),
];

/// Every theme's tokens as they resolved before this card: its name, then
/// an indented line per token. Written from the task base (3f2869279).
const BEFORE: &str = "
Kiln Dark
    surface.background #16181d
    surface.panel #121418
    surface.raised #1f2227
    surface.overlay #00000080
    text.primary #8b8f98
    text.muted #4b5160
    text.disabled #4b5160
    text.accent #4f7cc8
    text.on_accent #c8ccd4
    border.default #23262d
    border.subtle #1d1f25
    border.focused #4f7cc8
    accent.base #4f7cc8
    accent.hover #4f7cc8d9
    accent.subtle #4f7cc833
    state.hover #ffffff0a
    state.pressed #ffffff14
    state.selected #4f7cc833
    state.selected_inactive #ffffff0f
    state.focus_ring #4f7cc8aa
    state.drop_target #4f7cc8
    state.dirty #d19a66
    state.disabled #20232a
    status.error #e06c75
    status.warning #d19a66
    status.info #61afef
    status.success #98c379
    list.row_hover #ffffff0a
    list.row_selected #4f7cc833
    list.row_selected_inactive #ffffff0f
    list.row_focus_border #4f7cc8aa
    list.indent_guide #ffffff14
    list.chevron #6c717c
    list.icon #8b8f98
    list.folder_icon #7f9fd8
    tab.active #16181d
    tab.active_text #c8ccd4
    tab.inactive #121418
    scrollbar.thumb #4b516080
    scrollbar.thumb_hover #8b8f9899
Kiln Light
    surface.background #ffffff
    surface.panel #f3f4f6
    surface.raised #f1f1f2
    surface.overlay #0000004d
    text.primary #4a4f59
    text.muted #9aa0aa
    text.disabled #9aa0aa
    text.accent #3b6fd4
    text.on_accent #ffffff
    border.default #dcdfe4
    border.subtle #eeeff2
    border.focused #3b6fd4
    accent.base #3b6fd4
    accent.hover #3b6fd4d9
    accent.subtle #3b6fd433
    state.hover #0000000a
    state.pressed #00000014
    state.selected #3b6fd433
    state.selected_inactive #0000000f
    state.focus_ring #3b6fd4aa
    state.drop_target #3b6fd4
    state.dirty #b8760f
    state.disabled #dddfe3
    status.error #d93f4c
    status.warning #b8760f
    status.info #2f74d0
    status.success #3f8f3a
    list.row_hover #0000000a
    list.row_selected #3b6fd433
    list.row_selected_inactive #0000000f
    list.row_focus_border #3b6fd4aa
    list.indent_guide #00000014
    list.chevron #727882
    list.icon #4a4f59
    list.folder_icon #3b6fd4
    tab.active #ffffff
    tab.active_text #1f2329
    tab.inactive #f3f4f6
    scrollbar.thumb #9aa0aa80
    scrollbar.thumb_hover #4a4f5999
Catppuccin Frappe
    surface.background #303446
    surface.panel #292c3c
    surface.raised #292c3c
    surface.overlay #292c3c80
    text.primary #c6d0f5
    text.muted #a5adce
    text.disabled #737994
    text.accent #8caaee
    text.on_accent #232634
    border.default #626880
    border.subtle #51576d
    border.focused #ca9ee6
    accent.base #ca9ee6
    accent.hover #d9baed
    accent.subtle #ca9ee666
    state.hover #41455980
    state.pressed #414559
    state.selected #414559
    state.selected_inactive #414559
    state.focus_ring #ca9ee6
    state.drop_target #ca9ee633
    state.dirty #e5c890
    state.disabled #484c61
    status.error #e78284
    status.warning #ef9f76
    status.info #8caaee
    status.success #a6d189
    list.row_hover #41455980
    list.row_selected #414559
    list.row_selected_inactive #414559
    list.row_focus_border #ca9ee6
    list.indent_guide #949cbb
    list.chevron #ca9ee6
    list.icon #ca9ee6
    list.folder_icon #ca9ee6
    tab.active #303446
    tab.active_text #ca9ee6
    tab.inactive #292c3c
    scrollbar.thumb #62688080
    scrollbar.thumb_hover #737994
Catppuccin Latte
    surface.background #eff1f5
    surface.panel #e6e9ef
    surface.raised #e6e9ef
    surface.overlay #e6e9ef80
    text.primary #4c4f69
    text.muted #6c6f85
    text.disabled #9ca0b0
    text.accent #1e66f5
    text.on_accent #dce0e8
    border.default #acb0be
    border.subtle #bcc0cc
    border.focused #8839ef
    accent.base #8839ef
    accent.hover #9c5af2
    accent.subtle #8839ef66
    state.hover #ccd0da80
    state.pressed #ccd0da
    state.selected #ccd0da
    state.selected_inactive #ccd0da
    state.focus_ring #8839ef
    state.drop_target #8839ef33
    state.dirty #df8e1d
    state.disabled #c8cbd5
    status.error #d20f39
    status.warning #fe640b
    status.info #1e66f5
    status.success #40a02b
    list.row_hover #ccd0da80
    list.row_selected #ccd0da
    list.row_selected_inactive #ccd0da
    list.row_focus_border #8839ef
    list.indent_guide #7c7f93
    list.chevron #8839ef
    list.icon #8839ef
    list.folder_icon #8839ef
    tab.active #eff1f5
    tab.active_text #8839ef
    tab.inactive #e6e9ef
    scrollbar.thumb #acb0be80
    scrollbar.thumb_hover #9ca0b0
Catppuccin Macchiato
    surface.background #24273a
    surface.panel #1e2030
    surface.raised #1e2030
    surface.overlay #1e203080
    text.primary #cad3f5
    text.muted #a5adcb
    text.disabled #6e738d
    text.accent #8aadf4
    text.on_accent #181926
    border.default #5b6078
    border.subtle #494d64
    border.focused #c6a0f6
    accent.base #c6a0f6
    accent.hover #dac1f9
    accent.subtle #c6a0f666
    state.hover #363a4f80
    state.pressed #363a4f
    state.selected #363a4f
    state.selected_inactive #363a4f
    state.focus_ring #c6a0f6
    state.drop_target #c6a0f633
    state.dirty #eed49f
    state.disabled #404357
    status.error #ed8796
    status.warning #f5a97f
    status.info #8aadf4
    status.success #a6da95
    list.row_hover #363a4f80
    list.row_selected #363a4f
    list.row_selected_inactive #363a4f
    list.row_focus_border #c6a0f6
    list.indent_guide #939ab7
    list.chevron #c6a0f6
    list.icon #c6a0f6
    list.folder_icon #c6a0f6
    tab.active #24273a
    tab.active_text #c6a0f6
    tab.inactive #1e2030
    scrollbar.thumb #5b607880
    scrollbar.thumb_hover #6e738d
Catppuccin Mocha
    surface.background #1e1e2e
    surface.panel #181825
    surface.raised #181825
    surface.overlay #18182580
    text.primary #cdd6f4
    text.muted #a6adc8
    text.disabled #6c7086
    text.accent #89b4fa
    text.on_accent #11111b
    border.default #585b70
    border.subtle #45475a
    border.focused #cba6f7
    accent.base #cba6f7
    accent.hover #dec7fa
    accent.subtle #cba6f766
    state.hover #31324480
    state.pressed #313244
    state.selected #313244
    state.selected_inactive #313244
    state.focus_ring #cba6f7
    state.drop_target #cba6f733
    state.dirty #f9e2af
    state.disabled #3c3d4e
    status.error #f38ba8
    status.warning #fab387
    status.info #89b4fa
    status.success #a6e3a1
    list.row_hover #31324480
    list.row_selected #313244
    list.row_selected_inactive #313244
    list.row_focus_border #cba6f7
    list.indent_guide #9399b2
    list.chevron #cba6f7
    list.icon #cba6f7
    list.folder_icon #cba6f7
    tab.active #1e1e2e
    tab.active_text #cba6f7
    tab.inactive #181825
    scrollbar.thumb #585b7080
    scrollbar.thumb_hover #6c7086
Tenmoku Dark
    surface.background #16181d
    surface.panel #121418
    surface.raised #1f2227
    surface.overlay #00000080
    text.primary #8b8f98
    text.muted #4b5160
    text.disabled #4b5160
    text.accent #4f7cc8
    text.on_accent #c8ccd4
    border.default #23262d
    border.subtle #1d1f25
    border.focused #4f7cc8
    accent.base #4f7cc8
    accent.hover #4f7cc8d9
    accent.subtle #4f7cc833
    state.hover #ffffff0a
    state.pressed #ffffff14
    state.selected #4f7cc833
    state.selected_inactive #ffffff0f
    state.focus_ring #4f7cc8aa
    state.drop_target #4f7cc8
    state.dirty #d19a66
    state.disabled #20232a
    status.error #e06c75
    status.warning #d19a66
    status.info #61afef
    status.success #98c379
    list.row_hover #ffffff0a
    list.row_selected #4f7cc833
    list.row_selected_inactive #ffffff0f
    list.row_focus_border #4f7cc8aa
    list.indent_guide #ffffff14
    list.chevron #6c717c
    list.icon #8b8f98
    list.folder_icon #7f9fd8
    tab.active #16181d
    tab.active_text #c8ccd4
    tab.inactive #121418
    scrollbar.thumb #4b516080
    scrollbar.thumb_hover #8b8f9899
Tenmoku Ember
    surface.background #1c1814
    surface.panel #16120f
    surface.raised #27221d
    surface.overlay #00000080
    text.primary #a8998a
    text.muted #5e544a
    text.disabled #5e544a
    text.accent #d08b3e
    text.on_accent #1c1814
    border.default #2e2722
    border.subtle #25201b
    border.focused #d08b3e
    accent.base #d08b3e
    accent.hover #d08b3ed9
    accent.subtle #d08b3e33
    state.hover #ffffff0a
    state.pressed #ffffff14
    state.selected #d08b3e33
    state.selected_inactive #ffffff0f
    state.focus_ring #d08b3eaa
    state.drop_target #d08b3e
    state.dirty #e0a84e
    state.disabled #28231e
    status.error #e0705f
    status.warning #e0a84e
    status.info #7fa6b8
    status.success #a3b56f
    list.row_hover #ffffff0a
    list.row_selected #d08b3e33
    list.row_selected_inactive #ffffff0f
    list.row_focus_border #d08b3eaa
    list.indent_guide #ffffff14
    list.chevron #85766a
    list.icon #a8998a
    list.folder_icon #d6a066
    tab.active #1c1814
    tab.active_text #e3d6c3
    tab.inactive #16120f
    scrollbar.thumb #5e544a80
    scrollbar.thumb_hover #a8998a99
Tenmoku Light
    surface.background #ffffff
    surface.panel #f3f4f6
    surface.raised #f1f1f2
    surface.overlay #0000004d
    text.primary #4a4f59
    text.muted #9aa0aa
    text.disabled #9aa0aa
    text.accent #3b6fd4
    text.on_accent #ffffff
    border.default #dcdfe4
    border.subtle #eeeff2
    border.focused #3b6fd4
    accent.base #3b6fd4
    accent.hover #3b6fd4d9
    accent.subtle #3b6fd433
    state.hover #0000000a
    state.pressed #00000014
    state.selected #3b6fd433
    state.selected_inactive #0000000f
    state.focus_ring #3b6fd4aa
    state.drop_target #3b6fd4
    state.dirty #b8760f
    state.disabled #dddfe3
    status.error #d93f4c
    status.warning #b8760f
    status.info #2f74d0
    status.success #3f8f3a
    list.row_hover #0000000a
    list.row_selected #3b6fd433
    list.row_selected_inactive #0000000f
    list.row_focus_border #3b6fd4aa
    list.indent_guide #00000014
    list.chevron #727882
    list.icon #4a4f59
    list.folder_icon #3b6fd4
    tab.active #ffffff
    tab.active_text #1f2329
    tab.inactive #f3f4f6
    scrollbar.thumb #9aa0aa80
    scrollbar.thumb_hover #4a4f5999
VS Code Dark 2026
    surface.background #121314
    surface.panel #191a1b
    surface.raised #202122
    surface.overlay #00000080
    text.primary #bfbfbf
    text.muted #8c8c8c
    text.disabled #555555
    text.accent #48a0c7
    text.on_accent #ffffff
    border.default #2a2b2c
    border.subtle #2a2b2c
    border.focused #3994bc
    accent.base #297aa0
    accent.hover #2b7da3
    accent.subtle #3994bc26
    state.hover #ffffff14
    state.pressed #ffffff22
    state.selected #ffffff22
    state.selected_inactive #2c2d2e
    state.focus_ring #3994bcb3
    state.drop_target #297aa0
    state.dirty #e5ba7d
    state.disabled #363737
    status.error #f48771
    status.warning #e5ba7d
    status.info #3a94bc
    status.success #73c991
    list.row_hover #ffffff14
    list.row_selected #ffffff22
    list.row_selected_inactive #2c2d2e
    list.row_focus_border #3994bcb3
    list.indent_guide #8384854d
    list.chevron #8c8c8c
    list.icon #8c8c8c
    list.folder_icon #8c8c8c
    tab.active #121314
    tab.active_text #bfbfbf
    tab.inactive #191a1b
    scrollbar.thumb #a8a9aa85
    scrollbar.thumb_hover #a8a9aa90
VS Code Light 2026
    surface.background #ffffff
    surface.panel #fafafd
    surface.raised #fafafd
    surface.overlay #00000040
    text.primary #202020
    text.muted #606060
    text.disabled #bbbbbb
    text.accent #0069cc
    text.on_accent #ffffff
    border.default #e4e5e6
    border.subtle #f0f1f2
    border.focused #0069cc
    accent.base #0069cc
    accent.hover #0069ccd9
    accent.subtle #0069cc33
    state.hover #00000014
    state.pressed #00000025
    state.selected #00000025
    state.selected_inactive #dadada99
    state.focus_ring #0069cc
    state.drop_target #0069cc
    state.dirty #b69500
    state.disabled #d4d4d6
    status.error #ad0707
    status.warning #b69500
    status.info #0069cc
    status.success #587c0c
    list.row_hover #00000014
    list.row_selected #00000025
    list.row_selected_inactive #dadada99
    list.row_focus_border #0069cc
    list.indent_guide #00000014
    list.chevron #606060
    list.icon #606060
    list.folder_icon #606060
    tab.active #ffffff
    tab.active_text #202020
    tab.inactive #fafafd
    scrollbar.thumb #646464c0
    scrollbar.thumb_hover #646464d0
VS Code Dark Modern
    surface.background #1f1f1f
    surface.panel #181818
    surface.raised #202020
    surface.overlay #00000080
    text.primary #cccccc
    text.muted #9d9d9d
    text.disabled #6e7681
    text.accent #4daafc
    text.on_accent #ffffff
    border.default #2b2b2b
    border.subtle #2b2b2b
    border.focused #0078d4
    accent.base #0078d4
    accent.hover #026ec1
    accent.subtle #0078d426
    state.hover #ffffff14
    state.pressed #ffffff22
    state.selected #ffffff22
    state.selected_inactive #37373d
    state.focus_ring #0078d4b3
    state.drop_target #0078d4
    state.dirty #cca700
    state.disabled #393939
    status.error #f85149
    status.warning #cca700
    status.info #3794ff
    status.success #2ea043
    list.row_hover #ffffff14
    list.row_selected #ffffff22
    list.row_selected_inactive #37373d
    list.row_focus_border #0078d4b3
    list.indent_guide #404040
    list.chevron #cccccc
    list.icon #cccccc
    list.folder_icon #cccccc
    tab.active #1f1f1f
    tab.active_text #ffffff
    tab.inactive #181818
    scrollbar.thumb #79797966
    scrollbar.thumb_hover #646464b3
Monokai
    surface.background #272822
    surface.panel #1e1f1c
    surface.raised #1e1f1c
    surface.overlay #1e1f1ce6
    text.primary #f8f8f2
    text.muted #ccccc7
    text.disabled #75715e
    text.accent #a6e22e
    text.on_accent #272822
    border.default #414339
    border.subtle #34352f
    border.focused #99947c
    accent.base #75715e
    accent.hover #8a866f
    accent.subtle #75715e66
    state.hover #3e3d32
    state.pressed #414339
    state.selected #75715e
    state.selected_inactive #414339
    state.focus_ring #99947c
    state.drop_target #41433980
    state.dirty #e6db74
    state.disabled #4a4a47
    status.error #f92672
    status.warning #e2e22e
    status.info #819aff
    status.success #a6e22e
    list.row_hover #3e3d32
    list.row_selected #75715e
    list.row_selected_inactive #414339
    list.row_focus_border #99947c
    list.indent_guide #464741
    list.chevron #ccccc7
    list.icon #ccccc7
    list.folder_icon #ccccc7
    tab.active #272822
    tab.active_text #f8f8f2
    tab.inactive #34352f
    scrollbar.thumb #75715e66
    scrollbar.thumb_hover #75715e99";

/// Kiln's two bases, then every theme crates/fe-config bundles, by file
/// name, each over the base for its appearance, as Tenmoku loads them.
fn themes() -> Vec<ThemeData> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
    let mut paths: Vec<_> = (fs::read_dir(&dir).unwrap())
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 11, "the bundled themes: {paths:?}");
    let mut themes = vec![ThemeData::dark().clone(), ThemeData::light().clone()];
    for path in paths {
        let (file, diagnostics) = ThemeFile::load(&path, &["editor", "syntax"]);
        assert_eq!(diagnostics, [], "{}", path.display());
        // A theme may spell out a new token, as Ember, which sets every
        // root, does; the checks below hold it to today's colour all the same.
        themes.push(ThemeData::from_file(&file.unwrap()));
    }
    themes
}

/// `rule` applied to `tokens`, as kiln-core derives a token.
fn apply(rule: Rule, tokens: &Tokens) -> Color {
    let get = |key| *tokens.get(key).unwrap();
    match rule {
        Rule::Same(key) => get(key),
        Rule::Alpha(key, alpha) => get(key).with_alpha(alpha),
        Rule::Mix(from, to, amount) => get(from).mix(get(to), amount),
    }
}

#[test]
fn new_roles_resolve_to_todays_colours() {
    // The new tokens are exactly those not in the snapshot, each with the
    // rule the spec gives it.
    let mut before: Vec<(&str, Vec<(&str, &str)>)> = Vec::new();
    for line in BEFORE.lines().filter(|line| !line.is_empty()) {
        match line.strip_prefix("    ") {
            None => before.push((line, Vec::new())),
            Some(token) => (before.last_mut().unwrap().1).push(token.split_once(' ').unwrap()),
        }
    }
    let old: Vec<&str> = before[0].1.iter().map(|&(key, _)| key).collect();
    let added: Vec<&str> = TOKEN_KEYS
        .iter()
        .copied()
        .filter(|key| !old.contains(key))
        .collect();
    let mut new: Vec<&str> = NEW.iter().map(|&(key, _)| key).collect();
    new.sort();
    let mut sorted = added.clone();
    sorted.sort();
    assert_eq!(sorted, new, "the new tokens");
    for &(key, expected) in NEW {
        assert_eq!(rule(key), expected, "{key}'s rule");
    }

    let themes = themes();
    let names: Vec<&str> = themes.iter().map(|theme| theme.name.as_str()).collect();
    let before_names: Vec<&str> = before.iter().map(|&(name, _)| name).collect();
    assert_eq!(names, before_names, "the themes");
    for (theme, (name, tokens)) in themes.iter().zip(&before) {
        assert_eq!(tokens.len(), old.len(), "{name}: every old token");
        for &(key, color) in tokens {
            let now = theme.tokens.get(key).map(Color::to_string);
            assert_eq!(now.as_deref(), Some(color), "{name}: {key}");
        }
        let base = ThemeData::base(theme.appearance);
        for &(key, rule) in NEW {
            let now = *theme.tokens.get(key).unwrap();
            match rule {
                Some(rule) => assert_eq!(now, apply(rule, &theme.tokens), "{name}: {key}"),
                None => assert_eq!(Some(&now), base.tokens.get(key), "{name}: {key}"),
            }
        }
    }

    // The roots keep today's colours: modified is the bases' accent.
    for base in [ThemeData::dark(), ThemeData::light()] {
        let tokens = &base.tokens;
        assert_eq!(tokens.status.modified, tokens.accent.base, "{}", base.name);
        assert_eq!(tokens.surface.chrome, tokens.surface.panel, "{}", base.name);
    }
    let dark = &ThemeData::dark().tokens;
    assert_eq!(dark.text.faint, Color::rgb(0x363a45));
    assert_eq!(dark.status.error_soft, Color::rgba(0xe06c7526));
}

#[test]
fn agent_never_resolves_to_the_accent() {
    for theme in themes() {
        let tokens = &theme.tokens;
        assert_ne!(tokens.agent.base, tokens.accent.base, "{}", theme.name);
        assert_ne!(tokens.agent.soft, tokens.accent.subtle, "{}", theme.name);
        assert_ne!(tokens.agent.base, tokens.status.modified, "{}", theme.name);
    }
}
