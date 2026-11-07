//! Classic Monokai (VS Code's theme-monokai), a bundled dark theme that is
//! not the default.

use std::path::Path;

use fe_config::{EditorColors, EditorPalette, Settings, theme::EXTENSIONS, themes::find};
use fe_syntax::Highlight;
use kiln_core::theme::{Appearance, Color, ThemeData, ThemeFile};

fn parse() -> ThemeFile {
    let theme = find("vscode-monokai").expect("Monokai is bundled");
    let (file, diagnostics) =
        ThemeFile::parse(Path::new("vscode-monokai.toml"), theme.source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    file.unwrap()
}

#[test]
fn loads() {
    let by_title = find("Monokai").unwrap();
    assert_eq!(by_title.id, "vscode-monokai");
    assert_eq!(by_title.appearance, Appearance::Dark);
    assert_eq!(find("vscode-monokai"), Some(by_title));
    let file = parse();
    assert_eq!(file.name.as_deref(), Some("Monokai"));
    assert_eq!(file.appearance, Some(Appearance::Dark));
    let (_, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&file));
    assert_eq!(warnings, Vec::<String>::new());
    let keys: Vec<&str> = file.extensions["editor"]
        .keys()
        .map(String::as_str)
        .collect();
    let mut want = EditorColors::KEYS.to_vec();
    let mut keys = keys;
    want.sort_unstable();
    keys.sort_unstable();
    assert_eq!(keys, want);
    assert_eq!(Settings::default().theme.name, None);
    assert_eq!(find("dark").unwrap().id, "tenmoku-dark");
}

/// monokai-color-theme.json at microsoft/vscode f93fdeb.
#[test]
fn matches_upstream() {
    let data = ThemeData::from_file(&parse());
    let (palette, _) = EditorPalette::for_theme(&data);
    let editor = &palette.editor;
    assert_eq!(editor.background, Color::rgb(0x272822));
    assert_eq!(editor.foreground, Color::rgb(0xF8F8F2));
    assert_eq!(editor.selection, Color::rgba(0x878B9180));
    assert_eq!(editor.cursor, Color::rgb(0xF8F8F0));
    assert_eq!(editor.active_line, Color::rgb(0x3E3D32));
    assert_eq!(data.tokens.surface.panel, Color::rgb(0x1E1F1C));
    for (name, rgb) in [
        ("comment", 0x88846F),
        ("keyword", 0xF92672),
        ("function", 0xA6E22E),
        ("string", 0xE6DB74),
        ("type.builtin", 0x66D9EF),
        ("number", 0xAE81FF),
        ("variable.parameter", 0xFD971F),
    ] {
        assert_eq!(palette.syntax_color(name), Some(Color::rgb(rgb)), "{name}");
    }
}

#[test]
fn syntax_keys_are_known() {
    let file = parse();
    for key in file.extensions["syntax"].keys() {
        assert!(Highlight::from_capture(key).is_some(), "{key}");
    }
    let (palette, _) = EditorPalette::for_theme(&ThemeData::from_file(&file));
    let string = palette.syntax_color("string");
    let placeholder = palette.syntax_color("string.placeholder");
    assert!(placeholder.is_some());
    assert_ne!(placeholder, string);
    for name in [
        "variable.parameter",
        "type.parameter",
        "constant.enum_member",
    ] {
        assert!(file.extensions["syntax"].contains_key(name), "{name}");
    }
}
