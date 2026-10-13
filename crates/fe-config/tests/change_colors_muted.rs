//! The bundled themes' git change colours are muted on purpose: low
//! saturation, moderate lightness, and still in their hue family.

use std::path::Path;

use fe_config::{Color, EditorPalette, theme::EXTENSIONS};
use kiln_core::theme::{ThemeData, ThemeFile};

const THEMES: [(&str, &str); 3] = [
    ("dark", include_str!("../themes/fastedit-dark.toml")),
    ("light", include_str!("../themes/fastedit-light.toml")),
    ("ember", include_str!("../themes/fastedit-ember.toml")),
];

/// `(hue in degrees, saturation, lightness)` in HSL.
fn hsl(color: Color) -> (f64, f64, f64) {
    let [r, g, b] = [color.r(), color.g(), color.b()].map(|c| f64::from(c) / 255.);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let lightness = (max + min) / 2.;
    let chroma = max - min;
    if chroma == 0. {
        return (0., 0., lightness);
    }
    let saturation = chroma / (1. - (2. * lightness - 1.).abs());
    let hue = if max == r {
        60. * ((g - b) / chroma)
    } else if max == g {
        60. * ((b - r) / chroma + 2.)
    } else {
        60. * ((r - g) / chroma + 4.)
    };
    (hue.rem_euclid(360.), saturation, lightness)
}

#[test]
fn bundled_change_colours_are_muted() {
    for (name, source) in THEMES {
        let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
        assert_eq!(diagnostics, []);
        let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&file.unwrap()));
        assert_eq!(warnings, Vec::<String>::new(), "{name}");
        let editor = &palette.editor;
        let lightness = match name {
            "dark" => Some(0.35..=0.55),
            "light" => Some(0.55..=0.80),
            _ => None,
        };
        let colours = [
            ("added", editor.change_added, 100. ..=160.),
            ("modified", editor.change_modified, 190. ..=230.),
            // Deleted wraps around red: 340-360 or 0-10.
            ("deleted", editor.change_deleted, 340. ..=370.),
        ];
        for (kind, colour, hues) in colours {
            let (hue, saturation, l) = hsl(colour);
            let hue = if kind == "deleted" && hue < 180. {
                hue + 360.
            } else {
                hue
            };
            assert!(saturation <= 0.45, "{name} {kind}: saturation {saturation}");
            assert!(hues.contains(&hue), "{name} {kind}: hue {hue}");
            if let Some(range) = &lightness {
                assert!(range.contains(&l), "{name} {kind}: lightness {l}");
            }
        }
    }
}
