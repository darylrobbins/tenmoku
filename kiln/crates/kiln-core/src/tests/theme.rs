// SPDX-License-Identifier: LicenseRef-Proprietary

//! Theme files: colours, tokens, derivation from a base, the alias table,
//! mistakes reported per key, and app tables kept as written.

use std::path::Path;

use crate::{
    Severity,
    theme::{
        ALIASES, Appearance, Color, RULES, THEME_VERSION, TOKEN_KEYS, ThemeData, ThemeFile, Tokens,
        resolve, rule,
    },
};

fn parse(source: &str) -> (ThemeFile, Vec<crate::Diagnostic>) {
    let (file, diagnostics) = ThemeFile::parse(Path::new("paper.toml"), source, &["editor"]);
    (file.expect("parses"), diagnostics)
}

/// (key, message) of each diagnostic, all warnings.
fn warnings(diagnostics: &[crate::Diagnostic]) -> Vec<(String, String)> {
    diagnostics
        .iter()
        .inspect(|d| assert_eq!(d.severity, Severity::Warning, "{d}"))
        .map(|d| (d.key.clone().unwrap_or_default(), d.message.clone()))
        .collect()
}

#[test]
fn colors_parse_and_print() {
    assert_eq!("#4f7cc8".parse(), Ok(Color(0x4f7cc8ff)));
    assert_eq!("#4F7CC833".parse(), Ok(Color(0x4f7cc833)));
    assert_eq!("#abc".parse(), Ok(Color::rgb(0xaabbcc)));
    for bad in ["4f7cc8", "#4f7cc", "#4f7cc8f", "#ggg", "", "#", "#+f7cc8"] {
        assert!(bad.parse::<Color>().is_err(), "{bad:?}");
    }
    assert_eq!(Color(0x4f7cc8ff).to_string(), "#4f7cc8");
    assert_eq!(Color(0x4f7cc833).to_string(), "#4f7cc833");
    assert_eq!(Color::rgb(0x102030).with_alpha(0x80), Color(0x10203080));
    let mid = Color::rgb(0x000000).mix(Color::rgb(0xffffff), 0.5);
    assert_eq!(mid, Color::rgb(0x808080));
    assert_eq!(
        Color::rgb(0x123456).mix(Color::TRANSPARENT, 0.),
        Color::rgb(0x123456)
    );
}

#[test]
fn token_keys_and_rules_are_consistent() {
    let tokens = Tokens::<Color>::default();
    for key in TOKEN_KEYS {
        assert!(tokens.get(key).is_some(), "{key}");
    }
    assert!(tokens.get("list.nothing").is_none());
    for (index, &(key, rule)) in RULES.iter().enumerate() {
        assert!(TOKEN_KEYS.contains(&key), "{key}");
        for input in rule.inputs() {
            assert!(TOKEN_KEYS.contains(&input), "{key} reads {input}");
            // An input that's derived itself comes earlier.
            if let Some(at) = RULES.iter().position(|(derived, _)| *derived == input) {
                assert!(at < index, "{key} reads {input}, derived later");
            }
        }
    }
    for (alias, token) in ALIASES {
        assert!(TOKEN_KEYS.contains(token), "{alias} -> {token}");
    }
}

#[test]
fn bundled_bases_set_every_root() {
    for (base, source) in [
        (ThemeData::dark(), crate::theme::DARK_SOURCE),
        (ThemeData::light(), crate::theme::LIGHT_SOURCE),
    ] {
        let (file, diagnostics) = parse(source);
        assert_eq!(diagnostics, []);
        for key in TOKEN_KEYS.iter().filter(|key| rule(key).is_none()) {
            assert!(file.tokens.contains_key(key), "{} sets {key}", base.name);
        }
    }
    assert_eq!(ThemeData::dark().appearance, Appearance::Dark);
    assert_eq!(ThemeData::light().appearance, Appearance::Light);
    assert_ne!(ThemeData::dark().tokens, ThemeData::light().tokens);
}

/// The dark base's chrome is the workbench's colours before tokens.
#[test]
fn dark_base_keeps_the_chrome_colors() {
    let tokens = &ThemeData::dark().tokens;
    let expected = [
        ("surface.background", 0x16181dff),
        ("surface.panel", 0x121418ff),
        ("border.default", 0x23262dff),
        ("text.primary", 0x8b8f98ff),
        ("text.muted", 0x4b5160ff),
        ("status.error", 0xe06c75ff),
        ("list.row_hover", 0xffffff0a),
        ("list.row_selected", 0x4f7cc833),
        ("list.row_selected_inactive", 0xffffff0f),
        ("list.row_focus_border", 0x4f7cc8aa),
        ("list.icon", 0x8b8f98ff),
        ("list.folder_icon", 0x7f9fd8ff),
        ("list.chevron", 0x6c717cff),
        ("list.indent_guide", 0xffffff14),
        ("tab.active", 0x16181dff),
        ("tab.active_text", 0xc8ccd4ff),
        ("tab.inactive", 0x121418ff),
        ("state.dirty", 0xd19a66ff),
        ("state.drop_target", 0x4f7cc8ff),
        ("accent.base", 0x4f7cc8ff),
    ];
    for (key, rgba) in expected {
        assert_eq!(tokens.get(key), Some(&Color(rgba)), "{key}");
    }
}

#[test]
fn parses_tokens_name_and_appearance() {
    let (file, diagnostics) = parse(
        r##"
        kiln_theme = 1
        name = "Paper"
        appearance = "light"
        list.chevron = "#101010"

        [surface]
        background = "#fafafa"

        [tab]
        active_text = "#202020"
        "##,
    );
    assert_eq!(diagnostics, []);
    assert_eq!(file.name.as_deref(), Some("Paper"));
    assert_eq!(file.appearance, Some(Appearance::Light));
    assert_eq!(file.tokens.len(), 3);
    assert_eq!(file.tokens["surface.background"], Color::rgb(0xfafafa));
    assert_eq!(file.tokens["list.chevron"], Color::rgb(0x101010));

    let theme = ThemeData::from_file(&file);
    assert_eq!(theme.name, "Paper");
    assert_eq!(theme.appearance, Appearance::Light);
    assert_eq!(theme.tokens.surface.background, Color::rgb(0xfafafa));
    assert_eq!(theme.tokens.tab.active_text, Color::rgb(0x202020));
    // Left out, so the light base's.
    assert_eq!(
        theme.tokens.status.error,
        ThemeData::light().tokens.status.error
    );
}

#[test]
fn unnamed_theme_takes_its_file_stem_and_the_dark_base() {
    let (file, _) = parse("");
    let theme = ThemeData::from_file(&file);
    assert_eq!(theme.name, "paper");
    assert_eq!(theme.appearance, Appearance::Dark);
    assert_eq!(theme.tokens, ThemeData::dark().tokens);
}

#[test]
fn unset_tokens_follow_what_they_derive_from() {
    let (file, _) = parse(
        r##"
        [accent]
        base = "#c84f7c"

        [list]
        row_selected = "#00ff0033"
        "##,
    );
    let tokens = ThemeData::from_file(&file).tokens;
    let accent = Color::rgb(0xc84f7c);
    assert_eq!(tokens.accent.base, accent);
    assert_eq!(tokens.accent.subtle, accent.with_alpha(0x33));
    assert_eq!(tokens.state.selected, accent.with_alpha(0x33));
    assert_eq!(tokens.state.focus_ring, accent.with_alpha(0xaa));
    assert_eq!(tokens.list.row_focus_border, accent.with_alpha(0xaa));
    assert_eq!(tokens.border.focused, accent);
    // Derived, but the base set its own colour, which a file's accent
    // still replaces.
    assert_eq!(tokens.list.folder_icon, accent);
    assert_eq!(tokens.status.info, accent);
    // Set by the file, so not derived.
    assert_eq!(tokens.list.row_selected, Color(0x00ff0033));
    // Nothing it reads changed: the base's.
    let dark = &ThemeData::dark().tokens;
    assert_eq!(tokens.list.row_hover, dark.list.row_hover);
    assert_eq!(tokens.text.muted, dark.text.muted);
    assert_eq!(tokens.tab.active, dark.tab.active);
}

#[test]
fn derivation_chains_through_derived_tokens() {
    let tokens = resolve(
        [("text.primary", Color::rgb(0xffffff))],
        Some(&ThemeData::dark().tokens),
    );
    let dark = &ThemeData::dark().tokens;
    let muted = Color::rgb(0xffffff).mix(dark.surface.background, 0.5);
    assert_eq!(tokens.text.muted, muted);
    assert_eq!(tokens.text.disabled, muted, "derived from a derived token");
    assert_eq!(tokens.list.icon, Color::rgb(0xffffff));
    assert_eq!(tokens.list.chevron, Color::rgb(0xffffff).mix(muted, 0.5));
}

#[test]
fn alias_table_loads_into_tokens() {
    let mut source = String::from("name = \"Old\"\nappearance = \"dark\"\n[ui]\n");
    for (index, (alias, _)) in ALIASES.iter().enumerate() {
        source.push_str(&format!("{alias} = \"#0000{index:02x}\"\n"));
    }
    let (file, diagnostics) = parse(&source);
    assert_eq!(diagnostics, []);
    for (index, (alias, token)) in ALIASES.iter().enumerate() {
        assert_eq!(
            file.tokens.get(token),
            Some(&Color::rgb(index as u32)),
            "{alias}"
        );
    }

    // The token's own key wins over its alias, wherever each is.
    let (file, _) = parse(
        r##"
        [list]
        row_selected = "#111111"
        [ui]
        row_selected = "#222222"
        accent = "#333333"
        "##,
    );
    assert_eq!(file.tokens["list.row_selected"], Color::rgb(0x111111));
    assert_eq!(file.tokens["accent.base"], Color::rgb(0x333333));
}

/// A theme file written before tokens: a flat `[ui]` table, with an app's
/// own tables beside it.
#[test]
fn older_theme_file_loads_cleanly() {
    let source = r##"
        name = "Old Dark"
        appearance = "dark"

        [ui]
        background = "#16181d"
        row_selected = "#4f7cc833"
        accent = "#4f7cc8"

        [editor]
        background = "#16181d"
        cursor = "#7aa2f7"
        "##;
    let (file, diagnostics) =
        ThemeFile::parse(Path::new("old.toml"), source, &["editor", "syntax"]);
    assert_eq!(diagnostics, []);
    let theme = ThemeData::from_file(&file.unwrap());
    assert_eq!(theme.name, "Old Dark");
    assert_eq!(theme.tokens, ThemeData::dark().tokens);
    let editor = theme.extension("editor").unwrap();
    assert_eq!(editor["cursor"].as_str(), Some("#7aa2f7"));
}

#[test]
fn bad_values_are_warnings_at_their_key() {
    let (file, diagnostics) = parse(
        r##"
        kiln_theme = "one"
        name = ""
        appearance = "sepia"
        colour_scheme = "x"

        [surface]
        background = "fafafa"
        panel = 12
        floor = "#000000"

        [ui]
        nonsense = "#000000"
        border = "#zzzzzz"

        [list]
        row_hover = "#ffffff10"

        text = "#ffffff"
        "##,
    );
    assert_eq!(
        warnings(&diagnostics),
        [
            ("kiln_theme", "expected 1, ignored"),
            (
                "name",
                "expected a name: a string that isn't empty, ignored"
            ),
            ("appearance", "expected \"dark\" or \"light\", ignored"),
            ("colour_scheme", "unknown key, ignored"),
            (
                "surface.background",
                "\"fafafa\" isn't a colour: expected \"#rrggbb\" or \"#rrggbbaa\", ignored"
            ),
            (
                "surface.panel",
                "expected a colour string, found integer, ignored"
            ),
            ("surface.floor", "unknown key, ignored"),
            ("ui.nonsense", "unknown key, ignored"),
            (
                "ui.border",
                "\"#zzzzzz\" isn't a colour: expected \"#rrggbb\" or \"#rrggbbaa\", ignored"
            ),
            ("list.text", "unknown key, ignored"),
        ]
        .map(|(key, message)| (key.to_owned(), message.to_owned()))
    );
    // Lines point at the file, so an editor can jump there.
    assert_eq!(diagnostics[4].line, 8);
    // What was right still applies.
    assert_eq!(file.tokens.len(), 1);
    assert_eq!(file.tokens["list.row_hover"], Color(0xffffff10));
    assert_eq!(file.name, None);
    assert_eq!(file.appearance, None);
}

#[test]
fn group_or_extension_of_the_wrong_type_is_a_warning() {
    let (file, diagnostics) = parse("surface = \"#000000\"\neditor = 3\nsyntax = {}\n");
    assert_eq!(
        warnings(&diagnostics),
        [
            ("surface", "expected a table, found string, ignored"),
            ("editor", "expected a table, found integer, ignored"),
            // Not an extension this reader was told of.
            ("syntax", "unknown key, ignored"),
        ]
        .map(|(key, message)| (key.to_owned(), message.to_owned()))
    );
    assert!(file.extensions.is_empty());
}

#[test]
fn newer_version_still_loads() {
    let source = format!(
        "kiln_theme = {}\n[accent]\nbase = \"#ff0000\"\n",
        THEME_VERSION + 1
    );
    let (file, diagnostics) = parse(&source);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert!(
        diagnostics[0].message.starts_with("written for version 2"),
        "{}",
        diagnostics[0].message
    );
    assert_eq!(file.tokens["accent.base"], Color::rgb(0xff0000));
}

#[test]
fn extension_tables_are_kept_as_written() {
    let (file, diagnostics) = parse(
        r##"
        [editor]
        background = "#16181d"
        nested = { deep = [1, 2, 3] }
        "not a colour" = true
        "##,
    );
    assert_eq!(diagnostics, []);
    let theme = ThemeData::from_file(&file);
    let editor = theme.extension("editor").unwrap();
    assert_eq!(editor.len(), 3);
    assert_eq!(editor["background"].as_str(), Some("#16181d"));
    assert_eq!(editor["not a colour"].as_bool(), Some(true));
    assert_eq!(editor["nested"]["deep"].as_array().map(Vec::len), Some(3));
}

#[test]
fn unreadable_files_are_errors() {
    let (file, diagnostics) = ThemeFile::parse(Path::new("broken.toml"), "[surface", &[]);
    assert!(file.is_none());
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].is_error());

    let dir = tempfile::tempdir().unwrap();
    let (file, diagnostics) = ThemeFile::load(&dir.path().join("missing.toml"), &[]);
    assert!(file.is_none());
    assert_eq!(diagnostics[0].message, "no such file");

    let path = dir.path().join("mine.toml");
    std::fs::write(&path, "[accent]\nbase = \"#00ff00\"\n").unwrap();
    let (file, diagnostics) = ThemeFile::load(&path, &[]);
    assert_eq!(diagnostics, []);
    let theme = ThemeData::from_file(&file.unwrap());
    assert_eq!(theme.name, "mine");
    assert_eq!(theme.tokens.state.drop_target, Color::rgb(0x00ff00));
}
