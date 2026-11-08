//! VS Code Dark 2026, a bundled theme and a theme file a user can copy to
//! `theme.toml`.

use std::path::Path;

use fe_config::{EditorPalette, Settings, theme::EXTENSIONS, themes::find};
use fe_syntax::Highlight;
use kiln_core::theme::{Appearance, Color, ThemeData, ThemeFile};

const SOURCE: &str = include_str!("../themes/vscode-2026-dark.toml");

fn parse() -> ThemeFile {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), SOURCE, EXTENSIONS);
    assert_eq!(diagnostics, []);
    file.unwrap()
}

#[test]
fn parses_cleanly() {
    let file = parse();
    assert_eq!(file.name.as_deref(), Some("VS Code Dark 2026"));
    assert_eq!(file.appearance, Some(Appearance::Dark));

    let (_, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&file));
    assert_eq!(warnings, Vec::<String>::new());
}

#[test]
fn matches_upstream() {
    let data = ThemeData::from_file(&parse());
    let (palette, _) = EditorPalette::for_theme(&data);
    assert_eq!(palette.editor.background, Color::rgb(0x121314));
    assert_eq!(palette.editor.foreground, Color::rgb(0xBBBEBF));
    assert_eq!(palette.editor.selection, Color::rgba(0x276782DD));
    for (name, rgb) in [
        ("comment", 0x8B949E),
        ("keyword", 0xFF7B72),
        ("function", 0xD2A8FF),
        ("string", 0xA5D6FF),
        ("tag", 0x7EE787),
    ] {
        assert_eq!(palette.syntax_color(name), Some(Color::rgb(rgb)), "{name}");
    }
    assert_eq!(data.tokens.surface.background, Color::rgb(0x121314));
    assert_eq!(data.tokens.surface.panel, Color::rgb(0x191A1B));
    assert_eq!(data.tokens.border.default, Color::rgb(0x2A2B2C));
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
    assert_eq!(find("Dark 2026").unwrap().id, "vscode-2026-dark");
    assert_eq!(find("dark").unwrap().id, "yohen");
}
