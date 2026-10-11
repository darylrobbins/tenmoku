//! fastedit Ember, the warm dark theme file a user can copy to `theme.toml`.

use std::path::Path;

use fe_config::{EditorColors, EditorPalette, theme::EXTENSIONS};
use kiln_core::theme::{Appearance, DARK_SOURCE, ThemeData, ThemeFile};

const EMBER: &str = include_str!("../themes/fastedit-ember.toml");
const DARK: &str = include_str!("../themes/fastedit-dark.toml");

fn parse(source: &str) -> ThemeFile {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    file.unwrap()
}

#[test]
fn parses_cleanly() {
    let ember = parse(EMBER);
    assert_eq!(ember.name.as_deref(), Some("fastedit Ember"));
    assert_eq!(ember.appearance, Some(Appearance::Dark));

    let kiln_dark = parse(DARK_SOURCE);
    for key in kiln_dark.tokens.keys() {
        assert!(ember.tokens.contains_key(key), "token {key} is not set");
    }

    let mut keys = EditorColors::KEYS.to_vec();
    keys.sort();
    let editor: Vec<_> = ember.extensions["editor"]
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(editor, keys);

    let captures =
        |file: &ThemeFile| -> Vec<String> { file.extensions["syntax"].keys().cloned().collect() };
    assert_eq!(captures(&ember), captures(&parse(DARK)));
    assert_eq!(captures(&ember).len(), 25);
}

#[test]
fn looks_different() {
    let theme = ThemeData::from_file(&parse(EMBER));
    let (palette, warnings) = EditorPalette::for_theme(&theme);
    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(palette.editor.background, theme.tokens.surface.background);

    let dark = EditorPalette::fastedit_dark();
    assert_ne!(palette.editor.background, dark.editor.background);
    assert_ne!(palette.editor.foreground, dark.editor.foreground);
    assert_ne!(palette.editor.cursor, dark.editor.cursor);
    assert_ne!(palette.editor.selection, dark.editor.selection);
    for name in ["keyword", "string", "function"] {
        assert_ne!(
            palette.syntax_color(name),
            dark.syntax_color(name),
            "{name}"
        );
    }

    let base = ThemeData::dark();
    assert_ne!(
        theme.tokens.surface.background,
        base.tokens.surface.background
    );
    assert_ne!(theme.tokens.accent.base, base.tokens.accent.base);
}
