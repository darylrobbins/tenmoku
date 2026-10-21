//! The vim mode's status bar segments in the bundled themes: five opaque,
//! distinct fills, each with its label readable on it.

use std::path::Path;

use fe_config::{Color, EditorPalette, theme::EXTENSIONS};
use kiln_core::theme::{ThemeData, ThemeFile};

const EMBER: &str = include_str!("../themes/fastedit-ember.toml");
const DARK_2026: &str = include_str!("../themes/vscode-2026-dark.toml");
const LIGHT_2026: &str = include_str!("../themes/vscode-2026-light.toml");

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
fn mode_segments_readable_in_bundled_themes() {
    let sources = [
        (
            "fastedit Dark",
            include_str!("../themes/fastedit-dark.toml"),
        ),
        (
            "fastedit Light",
            include_str!("../themes/fastedit-light.toml"),
        ),
        ("fastedit Ember", EMBER),
        ("VS Code Dark 2026", DARK_2026),
        ("VS Code Light 2026", LIGHT_2026),
    ];
    for (name, source) in sources {
        let (file, _) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
        let editor = &file.unwrap().extensions["editor"];
        for key in [
            "vim_mode_normal",
            "vim_mode_insert",
            "vim_mode_visual",
            "vim_mode_replace",
            "vim_mode_command",
            "vim_mode_foreground",
        ] {
            assert!(editor.contains_key(key), "{name} leaves out {key}");
        }
    }

    let palettes = [
        ("fastedit Dark", EditorPalette::fastedit_dark().clone()),
        ("fastedit Light", EditorPalette::fastedit_light().clone()),
        ("fastedit Ember", ember()),
        ("VS Code Dark 2026", palette(DARK_2026)),
        ("VS Code Light 2026", palette(LIGHT_2026)),
    ];
    for (name, palette) in palettes {
        let editor = &palette.editor;
        let modes = [
            ("vim_mode_normal", editor.vim_mode_normal),
            ("vim_mode_insert", editor.vim_mode_insert),
            ("vim_mode_visual", editor.vim_mode_visual),
            ("vim_mode_replace", editor.vim_mode_replace),
            ("vim_mode_command", editor.vim_mode_command),
        ];
        let label = editor.vim_mode_foreground;
        assert_eq!(label.a(), 0xff, "{name}: vim_mode_foreground isn't opaque");
        for (i, &(key, color)) in modes.iter().enumerate() {
            assert_eq!(color.a(), 0xff, "{name}: {key} {color} isn't opaque");
            let ratio = contrast(label, color);
            assert!(ratio >= 4.5, "{name}: label on {key} {ratio:.2}");
            for &(other, other_color) in &modes[i + 1..] {
                assert_ne!(color, other_color, "{name}: {key} is {other}");
            }
        }
    }
}
