//! The editor's colours: the bundled dark and light palettes, and a kiln
//! theme's `[editor]` and `[syntax]` tables laid over them.

use std::path::Path;

use fe_config::{Color, EditorColors, EditorPalette, theme::EXTENSIONS};
use kiln_core::theme::{ThemeData, ThemeFile};

fn color(text: &str) -> Color {
    text.parse().unwrap()
}

fn theme(source: &str) -> (ThemeData, usize) {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    (ThemeData::from_file(&file.unwrap()), diagnostics.len())
}

#[test]
fn bundled_palettes() {
    let dark = EditorPalette::fastedit_dark();
    let light = EditorPalette::fastedit_light();
    assert_eq!(
        EditorPalette::for_theme(ThemeData::dark()),
        (dark.clone(), vec![])
    );
    assert_eq!(
        EditorPalette::for_theme(ThemeData::light()),
        (light.clone(), vec![])
    );

    assert_eq!(dark.editor.background, color("#16181d"));
    assert_eq!(dark.editor.cursor, color("#7aa2f7"));
    assert_eq!(dark.editor.popover_background, color("#21252b"));
    assert_eq!(dark.syntax_color("keyword"), Some(color("#c678dd")));
    assert_eq!(
        light.editor.background,
        ThemeData::light().tokens.surface.background
    );

    let sources = [
        include_str!("../themes/fastedit-dark.toml"),
        include_str!("../themes/fastedit-light.toml"),
    ];
    let mut captures = Vec::new();
    for source in sources {
        let (file, diagnostics) = ThemeFile::parse(Path::new("bundled.toml"), source, EXTENSIONS);
        assert_eq!(diagnostics, []);
        let file = file.unwrap();
        let editor: Vec<_> = file.extensions["editor"]
            .keys()
            .map(String::as_str)
            .collect();
        let mut keys = EditorColors::KEYS.to_vec();
        keys.sort();
        assert_eq!(editor, keys);
        assert_eq!(EditorColors::KEYS.len(), 30);
        captures.push(
            file.extensions["syntax"]
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(captures[0].len(), 25);
    assert_eq!(captures[0], captures[1]);
    let names = |palette: &EditorPalette| -> Vec<String> {
        palette
            .syntax
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    };
    assert_eq!(names(dark), names(light));
}

#[test]
fn extension_tables_override() {
    let (theme, diagnostics) = theme(
        r##"
appearance = "light"

[editor]
background = "#fafafa"
cursor = 3

[syntax]
keyword = "#d00000"
"function.method" = "#123456"
"##,
    );
    assert_eq!(diagnostics, 0);
    let (palette, warnings) = EditorPalette::for_theme(&theme);
    assert_eq!(
        warnings,
        ["editor.cursor: expected a colour string, found integer"]
    );

    let light = EditorPalette::fastedit_light();
    let mut expected = light.clone();
    expected.editor.background = color("#fafafa");
    let keyword = expected
        .syntax
        .iter_mut()
        .find(|(name, _)| name == "keyword")
        .unwrap();
    keyword.1 = color("#d00000");
    expected
        .syntax
        .push(("function.method".to_owned(), color("#123456")));
    assert_eq!(palette, expected);
    assert_eq!(palette.editor.cursor, light.editor.cursor);
    assert_eq!(
        palette.syntax.last(),
        Some(&("function.method".to_owned(), color("#123456")))
    );
}
