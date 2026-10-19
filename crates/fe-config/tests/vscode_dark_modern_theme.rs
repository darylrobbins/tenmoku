//! VS Code Dark Modern, the theme file a user can copy to `theme.toml`.

use std::path::Path;

use fe_config::{EditorPalette, theme::EXTENSIONS};
use fe_syntax::Highlight;
use kiln_core::theme::{Appearance, Color, ThemeData, ThemeFile};

const SOURCE: &str = include_str!("../themes/vscode-dark-modern.toml");

fn parse() -> ThemeFile {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), SOURCE, EXTENSIONS);
    assert_eq!(diagnostics, []);
    file.unwrap()
}

#[test]
fn parses_cleanly() {
    let file = parse();
    assert_eq!(file.name.as_deref(), Some("VS Code Dark Modern"));
    assert_eq!(file.appearance, Some(Appearance::Dark));

    let (_, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&file));
    assert_eq!(warnings, Vec::<String>::new());
}

#[test]
fn matches_upstream() {
    let (palette, _) = EditorPalette::for_theme(&ThemeData::from_file(&parse()));
    // dark_modern.json: editor.background, editor.foreground. VS Code's
    // default selection, as neither file sets one.
    assert_eq!(palette.editor.background, Color::rgb(0x1F1F1F));
    assert_eq!(palette.editor.foreground, Color::rgb(0xCCCCCC));
    assert_eq!(palette.editor.selection, Color::rgb(0x264F78));
    // dark_vs.json and dark_plus.json token colours.
    for (name, rgb) in [
        ("keyword", 0x569CD6),
        ("function", 0xDCDCAA),
        ("string", 0xCE9178),
        ("comment", 0x6A9955),
        ("type", 0x4EC9B0),
        ("module", 0x4EC9B0),
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
}
