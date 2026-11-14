//! Font styles for captures: a theme's `[syntax_style]` table.

use std::path::Path;

use fe_config::{EditorPalette, FontStyle, theme::EXTENSIONS};
use kiln_core::theme::{ThemeData, ThemeFile};

fn palette(source: &str) -> EditorPalette {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    let theme = ThemeData::from_file(&file.unwrap_or_default());
    let (palette, warnings) = EditorPalette::for_theme(&theme);
    assert_eq!(warnings, [] as [String; 0]);
    palette
}

const ITALIC: FontStyle = FontStyle {
    italic: true,
    bold: false,
};
const BOLD: FontStyle = FontStyle {
    italic: false,
    bold: true,
};

#[test]
fn longest_prefix_wins_for_styles() {
    let palette = palette(
        "[syntax_style]\ncomment = \"italic\"\n\"comment.todo\" = \"bold\"\nkeyword = \"bold italic\"\n",
    );
    assert_eq!(palette.style_for("comment"), Some(ITALIC));
    assert_eq!(palette.style_for("comment.documentation"), Some(ITALIC));
    assert_eq!(palette.style_for("comment.todo"), Some(BOLD));
    assert_eq!(
        palette.style_for("keyword.return"),
        Some(FontStyle {
            italic: true,
            bold: true
        })
    );
    assert_eq!(palette.style_for("string"), None);
}

#[test]
fn a_theme_without_the_table_has_none() {
    for source in ["", "[syntax]\ncomment = \"#999999\"\n"] {
        assert_eq!(palette(source).styles, []);
    }
}

#[test]
fn a_bad_style_is_a_warning() {
    let (file, _) = ThemeFile::parse(
        Path::new("theme.toml"),
        "[syntax_style]\ncomment = 3\nkeyword = \"wavy\"\n",
        EXTENSIONS,
    );
    let theme = ThemeData::from_file(&file.unwrap_or_default());
    let (palette, warnings) = EditorPalette::for_theme(&theme);
    assert_eq!(palette.styles, []);
    assert_eq!(
        warnings,
        [
            "syntax_style.comment: expected a font style (italic, bold), found integer",
            "syntax_style.keyword: expected a font style (italic, bold), found wavy"
        ]
    );
}

#[test]
fn yohen_and_hakuji_set_italic_comments_and_bold_function_definitions() {
    for palette in [
        EditorPalette::fastedit_dark(),
        EditorPalette::fastedit_light(),
    ] {
        assert_eq!(palette.syntax_style("comment"), Some(ITALIC));
        assert_eq!(palette.syntax_style("function.definition"), Some(BOLD));
        assert_eq!(palette.styles.len(), 2);
    }
}
