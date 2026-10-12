//! The git change bar's colours: the bundled themes keep today's derived
//! colours, and a kiln theme's `[editor]` can set them.

use std::path::Path;

use fe_config::{Color, EditorPalette, theme::EXTENSIONS};
use kiln_core::theme::{ThemeData, ThemeFile};

const THEMES: [(&str, &str); 3] = [
    ("dark", include_str!("../themes/fastedit-dark.toml")),
    ("light", include_str!("../themes/fastedit-light.toml")),
    ("ember", include_str!("../themes/fastedit-ember.toml")),
];

fn color(text: &str) -> Color {
    text.parse().unwrap()
}

fn parse(source: &str) -> ThemeFile {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    file.unwrap()
}

fn channels(color: Color) -> [f64; 3] {
    [color.r(), color.g(), color.b()].map(f64::from)
}

/// `color` with its hue set to `hue` degrees, saturation and lightness kept (HSL).
fn with_hue(color: Color, hue: f64) -> [f64; 3] {
    let [r, g, b] = channels(color).map(|c| c / 255.);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let lightness = (max + min) / 2.;
    let chroma = max - min;
    let saturation = if chroma == 0. {
        0.
    } else {
        chroma / (1. - (2. * lightness - 1.).abs())
    };
    let chroma = (1. - (2. * lightness - 1.).abs()) * saturation;
    let sector = hue / 60.;
    let x = chroma * (1. - (sector % 2. - 1.).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, x, 0.),
        1 => (x, chroma, 0.),
        2 => (0., chroma, x),
        3 => (0., x, chroma),
        4 => (x, 0., chroma),
        _ => (chroma, 0., x),
    };
    let m = lightness - chroma / 2.;
    [r + m, g + m, b + m].map(|c| c * 255.)
}

fn close(actual: Color, expected: [f64; 3], name: &str) {
    for (a, e) in channels(actual).into_iter().zip(expected) {
        assert!((a - e).abs() <= 1., "{name}: {actual:?} vs {expected:?}");
    }
}

#[test]
fn bundled_themes_keep_the_derived_colours() {
    for (name, source) in THEMES {
        let file = parse(source);
        let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&file));
        assert_eq!(warnings, Vec::<String>::new(), "{name}");
        let editor = &palette.editor;
        assert_eq!(editor.change_modified, editor.information, "{name}");
        assert_eq!(editor.change_deleted, editor.error, "{name}");
        close(
            editor.change_added,
            with_hue(editor.information, 130.),
            name,
        );
    }
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
