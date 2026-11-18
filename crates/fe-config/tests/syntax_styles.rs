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

const ITALIC: FontStyle = FontStyle::ITALIC;
const BOLD: FontStyle = FontStyle::BOLD;

/// Every theme's styles, [`fe_config::theme::MARKUP_STYLES`], as a palette
/// lists them.
fn markup() -> Vec<(String, FontStyle)> {
    vec![
        ("comment.documentation.italic".into(), ITALIC),
        (
            "comment.documentation.strikethrough".into(),
            FontStyle::STRIKETHROUGH,
        ),
        ("comment.documentation.strong".into(), BOLD),
        ("markup.heading".into(), BOLD),
        ("markup.italic".into(), ITALIC),
        ("markup.strikethrough".into(), FontStyle::STRIKETHROUGH),
        ("markup.strong".into(), BOLD),
    ]
}

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
            bold: true,
            strikethrough: false,
        })
    );
    assert_eq!(palette.style_for("string"), None);
}

#[test]
fn a_theme_without_the_table_has_the_markup_styles_alone() {
    for source in ["", "[syntax]\ncomment = \"#999999\"\n"] {
        assert_eq!(palette(source).styles, markup());
    }
}

#[test]
fn a_theme_can_set_the_markup_styles_otherwise() {
    let palette = palette(
        "[syntax_style]\n\"markup.strong\" = \"normal\"\n\"markup.quote\" = \"italic strikethrough\"\n",
    );
    assert_eq!(
        palette.style_for("markup.strong"),
        Some(FontStyle::default())
    );
    assert_eq!(
        palette.style_for("markup.quote"),
        Some(FontStyle {
            italic: true,
            bold: false,
            strikethrough: true,
        })
    );
    assert_eq!(palette.style_for("markup.italic"), Some(ITALIC));
    assert_eq!(palette.style_for("markup.heading"), Some(BOLD));
}

/// A doc comment's emphasis adds its face to the comment's style, unless the
/// theme sets it itself.
#[test]
fn doc_comment_emphasis_adds_to_the_comment_style() {
    let palette = palette(
        "[syntax_style]\ncomment = \"italic\"\n\"comment.documentation.strikethrough\" = \"bold\"\n",
    );
    let bold_italic = BOLD.union(ITALIC);
    assert_eq!(
        palette.style_for("comment.documentation.strong"),
        Some(bold_italic)
    );
    assert_eq!(
        palette.style_for("comment.documentation.italic"),
        Some(ITALIC)
    );
    assert_eq!(
        palette.style_for("comment.documentation.strikethrough"),
        Some(BOLD)
    );
    // Yohen's comments are italic.
    let yohen = EditorPalette::fastedit_dark();
    assert_eq!(
        yohen.style_for("comment.documentation.strong"),
        Some(bold_italic)
    );
    assert_eq!(
        yohen.style_for("comment.documentation.strikethrough"),
        Some(FontStyle::STRIKETHROUGH.union(ITALIC))
    );
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
    assert_eq!(palette.styles, markup());
    assert_eq!(
        warnings,
        [
            "syntax_style.comment: expected a font style (italic, bold, strikethrough), found integer",
            "syntax_style.keyword: expected a font style (italic, bold, strikethrough), found wavy"
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
        assert_eq!(palette.styles.len(), markup().len() + 2);
    }
}

/// The markup captures fe-syntax added for Markdown (T1924).
const MARKUP_CAPTURES: [&str; 6] = [
    "markup.strong",
    "markup.italic",
    "markup.strikethrough",
    "markup.quote",
    "markup.list",
    "markup.link.label",
];

/// Every bundled theme, and Kiln's own, draws strong text bold, emphasis
/// italic, headings bold and struck text struck through; each bundled theme
/// colours the new markup captures itself, so none takes the bundled
/// palette's colour for its appearance.
#[test]
fn every_theme_styles_and_colours_markup() {
    let struck = FontStyle::STRIKETHROUGH;
    let mut palettes: Vec<(&str, EditorPalette)> = vec![
        ("kiln dark", EditorPalette::for_theme(ThemeData::dark()).0),
        ("kiln light", EditorPalette::for_theme(ThemeData::light()).0),
    ];
    for theme in fe_config::themes::bundled_themes() {
        let file = theme.file();
        let syntax = &file.extensions["syntax"];
        for name in MARKUP_CAPTURES {
            assert!(syntax.contains_key(name), "{} colours {name}", theme.id);
        }
        let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&file));
        assert_eq!(warnings, [] as [String; 0], "{}", theme.id);
        palettes.push((theme.id, palette));
    }
    for (id, palette) in palettes {
        assert_eq!(palette.style_for("markup.strong"), Some(BOLD), "{id}");
        assert_eq!(palette.style_for("markup.italic"), Some(ITALIC), "{id}");
        assert_eq!(palette.style_for("markup.heading"), Some(BOLD), "{id}");
        assert_eq!(
            palette.style_for("markup.strikethrough"),
            Some(struck),
            "{id}"
        );
        for name in MARKUP_CAPTURES {
            assert!(palette.syntax_color(name).is_some(), "{id}: {name}");
        }
    }
}
