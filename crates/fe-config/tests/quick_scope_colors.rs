//! Quick-scope's underline colours: every bundled theme sets both, distinct
//! and at 3:1 or more against its background, and a user theme without them
//! gets the bundled palette's.

use std::path::Path;

use fe_config::{Color, EditorColors, EditorPalette, theme::EXTENSIONS, themes::bundled_themes};
use kiln_core::theme::{ThemeData, ThemeFile};

const KEYS: [&str; 2] = ["quick_scope_primary", "quick_scope_secondary"];

/// WCAG relative luminance.
fn luminance(color: Color) -> f64 {
    let [r, g, b] = [color.r(), color.g(), color.b()].map(|c| {
        let c = f64::from(c) / 255.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG contrast ratio of `a` against `b`.
fn contrast(a: Color, b: Color) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[test]
fn the_keys_follow_the_yank_highlight() {
    let at = (EditorColors::KEYS.iter())
        .position(|&key| key == "vim_yank_highlight")
        .unwrap();
    assert_eq!(EditorColors::KEYS[at + 1..at + 3], KEYS);
}

#[test]
fn every_bundled_theme_sets_both_readably() {
    let themes = bundled_themes();
    assert_eq!(
        std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/themes"))
            .unwrap()
            .count(),
        themes.len()
    );
    for theme in themes {
        let name = theme.title;
        let (file, diagnostics) = ThemeFile::parse(Path::new("t.toml"), theme.source, EXTENSIONS);
        assert_eq!(diagnostics, [], "{name}");
        let file = file.unwrap();
        for key in KEYS {
            assert!(file.extensions["editor"].contains_key(key), "{name}: {key}");
        }
        let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&file));
        assert_eq!(warnings, Vec::<String>::new(), "{name}");
        let editor = palette.editor;
        assert_ne!(
            editor.quick_scope_primary, editor.quick_scope_secondary,
            "{name}"
        );
        for (key, color) in [
            (KEYS[0], editor.quick_scope_primary),
            (KEYS[1], editor.quick_scope_secondary),
        ] {
            let ratio = contrast(color, editor.background);
            assert!(ratio >= 3., "{name}: {key} is {ratio:.2}:1");
        }
    }
}

#[test]
fn quick_scope_s_own_colours_in_tenmoku_dark() {
    let source = include_str!("../themes/tenmoku-dark.toml");
    let (file, _) = ThemeFile::parse(Path::new("tenmoku-dark.toml"), source, EXTENSIONS);
    let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&file.unwrap()));
    assert_eq!(warnings, Vec::<String>::new());
    let editor = &palette.editor;
    assert_eq!(editor.quick_scope_primary, "#afff5f".parse().unwrap());
    assert_eq!(editor.quick_scope_secondary, "#5fffff".parse().unwrap());
}

#[test]
fn a_user_theme_without_them_gets_the_bundled_ones() {
    let (file, _) = ThemeFile::parse(
        Path::new("t.toml"),
        "appearance = \"light\"\n\n[editor]\nbackground = \"#fefefe\"\n",
        EXTENSIONS,
    );
    let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&file.unwrap()));
    assert_eq!(warnings, Vec::<String>::new());
    let bundled = &EditorPalette::fastedit_light().editor;
    assert_eq!(
        palette.editor.quick_scope_primary,
        bundled.quick_scope_primary
    );
    assert_eq!(
        palette.editor.quick_scope_secondary,
        bundled.quick_scope_secondary
    );
}
