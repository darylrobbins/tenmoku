//! The git change bar's colours: a kiln theme's `[editor]` can set them.

use std::path::Path;

use fe_config::{Color, EditorPalette, theme::EXTENSIONS};
use kiln_core::theme::{ThemeData, ThemeFile};

fn color(text: &str) -> Color {
    text.parse().unwrap()
}

fn parse(source: &str) -> ThemeFile {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    file.unwrap()
}

#[test]
fn a_theme_sets_the_change_colours() {
    let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&parse(
        "appearance = \"dark\"\n\n[editor]\nchange_added = \"#123456\"\n",
    )));
    let bundled = EditorPalette::fastedit_dark();
    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(palette.editor.change_added, color("#123456"));
    assert_eq!(
        palette.editor.change_modified,
        bundled.editor.change_modified
    );
    assert_eq!(palette.editor.change_deleted, bundled.editor.change_deleted);

    let (palette, warnings) =
        EditorPalette::for_theme(&ThemeData::from_file(&parse("appearance = \"dark\"\n")));
    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(palette.editor.change_added, bundled.editor.change_added);
    assert_eq!(
        palette.editor.change_modified,
        bundled.editor.change_modified
    );
    assert_eq!(palette.editor.change_deleted, bundled.editor.change_deleted);
}
