//! VS Code Light 2026, a bundled theme and a theme file a user can copy to
//! `theme.toml`.

use std::path::Path;

use fe_config::{EditorPalette, Settings, theme::EXTENSIONS, themes::find};
use fe_syntax::Highlight;
use kiln_core::theme::{Appearance, Color, ThemeData, ThemeFile};

const SOURCE: &str = include_str!("../themes/vscode-2026-light.toml");

fn parse() -> ThemeFile {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), SOURCE, EXTENSIONS);
    assert_eq!(diagnostics, []);
    file.unwrap()
}

#[test]
fn parses_cleanly() {
    let file = parse();
    assert_eq!(file.name.as_deref(), Some("VS Code Light 2026"));
    assert_eq!(file.appearance, Some(Appearance::Light));

    let (_, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&file));
    assert_eq!(warnings, Vec::<String>::new());
}

#[test]
fn matches_upstream() {
    let data = ThemeData::from_file(&parse());
    let (palette, _) = EditorPalette::for_theme(&data);
    assert_eq!(palette.editor.background, Color::rgb(0xFFFFFF));
    assert_eq!(palette.editor.foreground, Color::rgb(0x202020));
    assert_eq!(palette.editor.selection, Color::rgba(0x0069CC40));
    for (name, rgb) in [
        ("comment", 0x6E7781),
        ("keyword", 0xCF222E),
        ("function", 0x8250DF),
        ("string", 0x0A3069),
        ("tag", 0x116329),
    ] {
        assert_eq!(palette.syntax_color(name), Some(Color::rgb(rgb)), "{name}");
    }
    assert_eq!(data.tokens.surface.background, Color::rgb(0xFFFFFF));
    assert_eq!(data.tokens.surface.panel, Color::rgb(0xFAFAFD));
    assert_eq!(data.tokens.border.default, Color::rgb(0xE4E5E6));
}

#[test]
fn syntax_keys_are_known() {
    let file = parse();
    for key in file.extensions["syntax"].keys() {
        assert!(Highlight::from_capture(key).is_some(), "{key}");
    }
}

#[test]
fn not_the_default() {
    assert_eq!(Settings::default().theme.name, None);
    assert_eq!(find("Light 2026").unwrap().id, "vscode-2026-light");
    assert_eq!(find("light").unwrap().id, "hakuji");
    assert_eq!(find("Light 2026").unwrap().appearance, Appearance::Light);
}
