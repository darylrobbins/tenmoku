//! Yohen's comment is a warm neutral grey (T1613): barely any chroma, a
//! trace of the theme's iron-brown, never green or blue; and it keeps the
//! design's contrast rules. Contrast is WCAG 2's, from relative luminance;
//! chroma and hue are OKLab's.

use fe_config::{Color, EditorPalette, themes};
use kiln_core::theme::ThemeData;

/// The surfaces text sits on, where the theme keeps each.
const SURFACES: [(&str, &str); 8] = [
    ("chrome", "surface.chrome"),
    ("tool window", "surface.panel"),
    ("editor", "editor.background"),
    ("cursor line", "editor.active_line"),
    ("hover", "state.hover"),
    ("selected", "state.selected_inactive"),
    ("focused selection", "state.selected"),
    ("overlay", "surface.raised"),
];

fn load(id: &str) -> (ThemeData, EditorPalette) {
    let data = ThemeData::from_file(&themes::find(id).unwrap().file());
    let (palette, warnings) = EditorPalette::for_theme(&data);
    assert_eq!(warnings, Vec::<String>::new(), "{id}");
    (data, palette)
}

fn comment(palette: &EditorPalette) -> Color {
    let (_, color) = palette
        .syntax
        .iter()
        .find(|(name, _)| name == "comment")
        .expect("a comment colour");
    *color
}

fn surface(data: &ThemeData, palette: &EditorPalette, place: &str) -> Color {
    match place.strip_prefix("editor.") {
        Some(key) => palette.editor.get(key),
        None => data.tokens.get(place).copied(),
    }
    .unwrap_or_else(|| panic!("no {place}"))
}

/// sRGB channels, linear, 0 to 1.
fn linear(color: Color) -> [f64; 3] {
    [color.r(), color.g(), color.b()].map(|c| {
        let c = f64::from(c) / 255.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    })
}

/// WCAG 2 relative luminance.
fn luminance(color: Color) -> f64 {
    let [r, g, b] = linear(color);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG 2 contrast ratio of `a` against `b`.
fn contrast(a: Color, b: Color) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// OKLab L, a and b (Björn Ottosson's matrices).
fn oklab(color: Color) -> [f64; 3] {
    let [r, g, b] = linear(color);
    let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
    let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
    let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
    let [l, m, s] = [l, m, s].map(f64::cbrt);
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

#[test]
fn comment_is_a_warm_neutral_grey() {
    let (_, palette) = load("yohen");
    let [_, a, b] = oklab(comment(&palette));
    let chroma = a.hypot(b);
    assert!(chroma <= 0.010, "chroma {chroma:.4}");
    if chroma >= 0.002 {
        let hue = b.atan2(a).to_degrees().rem_euclid(360.);
        assert!((40. ..=100.).contains(&hue), "hue {hue:.1} is not warm");
    }
    let (_, hakuji) = load("hakuji");
    assert_eq!(comment(&hakuji), Color::rgb(0x69727c));
}

#[test]
fn comment_clears_4_5_on_every_surface() {
    let (data, palette) = load("yohen");
    let comment = comment(&palette);
    for (name, place) in SURFACES {
        let ratio = contrast(comment, surface(&data, &palette, place));
        assert!(ratio >= 4.5, "{name}: {ratio:.2}");
        if matches!(name, "editor" | "cursor line") {
            assert!(ratio >= 6., "{name}: {ratio:.2} is under 6");
        }
    }
}
