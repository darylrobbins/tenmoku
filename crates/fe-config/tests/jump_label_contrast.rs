//! A vim jump's labels in the bundled themes: opaque boxes in two hues, each
//! readable against the editor background, with a glyph readable on either.

use std::path::Path;

use fe_config::{Color, EditorPalette, theme::EXTENSIONS};
use kiln_core::theme::{ThemeData, ThemeFile};

const EMBER: &str = include_str!("../themes/fastedit-ember.toml");
const DARK_2026: &str = include_str!("../themes/vscode-2026-dark.toml");

fn ember() -> EditorPalette {
    palette(EMBER)
}

fn palette(source: &str) -> EditorPalette {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&file.unwrap()));
    assert_eq!(warnings, Vec::<String>::new());
    palette
}

/// The channels of `color`, 0 to 1.
fn channels(color: Color) -> [f64; 3] {
    [color.r(), color.g(), color.b()].map(|c| f64::from(c) / 255.)
}

/// WCAG relative luminance.
fn luminance(color: Color) -> f64 {
    let [r, g, b] = channels(color).map(|c| {
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

/// Hue in degrees, 0 to 360.
fn hue(color: Color) -> f64 {
    let [r, g, b] = channels(color);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let delta = max - min;
    if delta == 0. {
        return 0.;
    }
    let sector = if max == r {
        ((g - b) / delta).rem_euclid(6.)
    } else if max == g {
        (b - r) / delta + 2.
    } else {
        (r - g) / delta + 4.
    };
    sector * 60.
}

#[test]
fn labels_readable_in_bundled_themes() {
    let palettes = [
        ("fastedit Dark", EditorPalette::fastedit_dark().clone()),
        ("fastedit Light", EditorPalette::fastedit_light().clone()),
        ("fastedit Ember", ember()),
        ("VS Code Dark 2026", palette(DARK_2026)),
    ];
    for (name, palette) in palettes {
        let editor = &palette.editor;
        let (live, preview, glyph) = (
            editor.jump_label,
            editor.jump_label_preview,
            editor.jump_label_foreground,
        );
        for (key, color) in [
            ("jump_label", live),
            ("jump_label_preview", preview),
            ("jump_label_foreground", glyph),
        ] {
            assert_eq!(color.a(), 0xff, "{name}: {key} {color} isn't opaque");
        }

        let on_live = contrast(glyph, live);
        assert!(on_live >= 7.0, "{name}: glyph on live label {on_live:.2}");
        let on_preview = contrast(glyph, preview);
        assert!(
            on_preview >= 4.5,
            "{name}: glyph on preview label {on_preview:.2}"
        );
        for (key, color) in [("jump_label", live), ("jump_label_preview", preview)] {
            let ratio = contrast(color, editor.background);
            assert!(ratio >= 3.0, "{name}: {key} on background {ratio:.2}");
        }

        let apart = (hue(live) - hue(preview)).abs();
        let apart = apart.min(360. - apart);
        assert!(apart >= 60., "{name}: label hues {apart:.0} degrees apart");
    }
}
