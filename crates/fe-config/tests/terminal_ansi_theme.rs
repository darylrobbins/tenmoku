//! The terminal's ANSI colours come from a theme's `[terminal]` table.
//! Yohen and Hakuji set theirs from their palette, each readable on the
//! editor, and being the default palettes, a theme without the table takes
//! theirs. Tenmoku Dark and Light keep the two sets the terminal drew before
//! themes.

use fe_config::{Color, EditorPalette, themes::bundled_themes};
use kiln_core::theme::{Appearance, ThemeData};

const DARK: [u32; 16] = [
    0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x3b6fe8, 0xcd00cd, 0x00cdcd, 0xe5e5e5, 0x7f7f7f,
    0xff4040, 0x40ff40, 0xffff40, 0x7090ff, 0xff40ff, 0x40ffff, 0xffffff,
];
const LIGHT: [u32; 16] = [
    0x000000, 0xb00000, 0x007a00, 0x8a7000, 0x0000c8, 0xa000a0, 0x007a8a, 0x8a8a8a, 0x5a5a5a,
    0xd02020, 0x10a010, 0xa08a00, 0x3050e0, 0xc020c0, 0x0090a0, 0x606060,
];

fn palette(id: &str) -> (EditorPalette, Appearance) {
    let theme = bundled_themes().iter().find(|t| t.id == id).unwrap();
    let data = ThemeData::from_file(&theme.file());
    let (palette, warnings) = EditorPalette::for_theme(&data);
    assert_eq!(warnings, Vec::<String>::new(), "{id}");
    (palette, data.appearance)
}

#[test]
fn a_theme_without_the_table_inherits_the_defaults() {
    let (yohen, _) = palette("yohen");
    let (hakuji, _) = palette("hakuji");
    assert_eq!(
        EditorPalette::bundled(Appearance::Dark).terminal,
        yohen.terminal
    );
    assert_eq!(
        EditorPalette::bundled(Appearance::Light).terminal,
        hakuji.terminal
    );
    let mut inheriting = 0;
    for theme in bundled_themes() {
        let (palette, appearance) = palette(theme.id);
        let want = match (theme.id, appearance) {
            ("tenmoku-dark", _) => DARK.map(Color::rgb),
            ("tenmoku-light", _) => LIGHT.map(Color::rgb),
            ("yohen" | "hakuji", _) => continue,
            (_, Appearance::Dark) => {
                inheriting += 1;
                yohen.terminal.ansi
            }
            (_, Appearance::Light) => {
                inheriting += 1;
                hakuji.terminal.ansi
            }
        };
        assert_eq!(palette.terminal.ansi, want, "{}", theme.id);
        assert_eq!(palette.terminal.foreground, None, "{}", theme.id);
    }
    // Ember and the eight themes from elsewhere have no table.
    assert_eq!(inheriting, bundled_themes().len() - 4);
}

/// WCAG 2 contrast ratio.
fn contrast(a: Color, b: Color) -> f64 {
    let luminance = |c: Color| {
        let lin = |v: u8| {
            let v = f64::from(v) / 255.;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    };
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[test]
fn yohen_and_hakuji_ansi_clear_4_5_on_the_editor() {
    for id in ["yohen", "hakuji"] {
        let (palette, _) = palette(id);
        let ground = palette.editor.background;
        for (index, color) in palette.terminal.ansi.iter().enumerate() {
            assert_eq!(color.a(), 255, "{id} ansi_{index} is opaque");
            if index == 0 || index == 8 {
                continue;
            }
            let ratio = contrast(*color, ground);
            assert!(ratio >= 4.5, "{id} ansi_{index} is {ratio:.2}:1");
        }
        let today = match palette_appearance(id) {
            Appearance::Dark => DARK,
            Appearance::Light => LIGHT,
        };
        assert_ne!(palette.terminal.ansi, today.map(Color::rgb), "{id}");
    }
}

fn palette_appearance(id: &str) -> Appearance {
    palette(id).1
}
