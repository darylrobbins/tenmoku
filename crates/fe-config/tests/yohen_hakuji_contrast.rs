//! Yohen and Hakuji keep the design's accessibility rules
//! (docs/design/design-system.md, Colour and Accessibility rules): text
//! clears 4.5:1 on every surface it sits on, code clears 6:1 in Yohen and
//! 4.5:1 in Hakuji, Yohen's signals stay apart, and the colours that must be
//! told apart differ in lightness too; the values T1512 retuned moved in
//! lightness alone; and the jump labels read as the bundled themes' do.
//! Contrast is WCAG 2's, from relative luminance; distance and lightness
//! are OKLab's.

use std::{fs, path::Path};

use fe_config::{Color, EditorPalette, themes};
use kiln_core::theme::ThemeData;

/// The surfaces text sits on: the design's name, then where the theme
/// keeps it.
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

/// The three text steps.
const TEXT: [(&str, &str); 3] = [
    ("ink", "text.primary"),
    ("muted", "text.muted"),
    ("faint", "text.faint"),
];

/// The signal roles.
const SIGNALS: [(&str, &str); 7] = [
    ("accent", "accent.base"),
    ("agent", "agent.base"),
    ("celadon", "status.success"),
    ("shinsha", "status.error"),
    ("kiseto", "status.warning"),
    ("ruri", "status.modified"),
    ("jun", "editor.vim_mode_visual"),
];

/// The pairs the design says must be told apart: added and removed,
/// warning and error, accent and agent.
const PAIRS: [(&str, &str); 3] = [
    ("status.success", "status.error"),
    ("status.warning", "status.error"),
    ("accent.base", "agent.base"),
];

struct Theme {
    id: &'static str,
    data: ThemeData,
    palette: EditorPalette,
}

impl Theme {
    fn load(id: &'static str) -> Theme {
        let data = ThemeData::from_file(&themes::find(id).unwrap().file());
        let (palette, warnings) = EditorPalette::for_theme(&data);
        assert_eq!(warnings, Vec::<String>::new(), "{id}");
        Theme { id, data, palette }
    }

    /// The opaque colour at `place`: a token, or `editor.<key>`.
    fn color(&self, place: &str) -> Color {
        let color = match place.strip_prefix("editor.") {
            Some(key) => self.palette.editor.get(key),
            None => self.data.tokens.get(place).copied(),
        }
        .unwrap_or_else(|| panic!("{}: no {place}", self.id));
        assert_eq!(color.a(), 0xff, "{}: {place} {color} is opaque", self.id);
        color
    }

    /// The code colours: every capture's, and the foreground (variables).
    fn code(&self) -> Vec<(String, Color)> {
        let mut code = self.palette.syntax.clone();
        code.push(("variable".to_owned(), self.palette.editor.foreground));
        code
    }
}

fn yohen() -> Theme {
    Theme::load("yohen")
}

fn hakuji() -> Theme {
    Theme::load("hakuji")
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

/// OKLab ΔE on the hundred scale the design quotes (its 5.4).
fn delta_e(a: Color, b: Color) -> f64 {
    let (a, b) = (oklab(a), oklab(b));
    100. * a
        .iter()
        .zip(b)
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f64>()
        .sqrt()
}

#[test]
fn text_steps_clear_4_5_on_every_surface() {
    for theme in [yohen(), hakuji()] {
        for (step, text) in TEXT {
            for (surface, place) in SURFACES {
                let ratio = contrast(theme.color(text), theme.color(place));
                assert!(
                    ratio >= 4.5,
                    "{}: {step} on {surface} is {ratio:.2}:1",
                    theme.id
                );
            }
        }
        // The editor's text is ink.
        assert_eq!(
            theme.color("editor.foreground"),
            theme.color("text.primary")
        );
    }
}

fn code_clears(theme: &Theme, least: f64) {
    for surface in ["editor.background", "editor.active_line"] {
        let ground = theme.color(surface);
        for (capture, color) in theme.code() {
            assert_eq!(color.a(), 0xff, "{}: {capture}", theme.id);
            let ratio = contrast(color, ground);
            assert!(
                ratio >= least,
                "{}: {capture} {color} on {surface} is {ratio:.2}:1",
                theme.id
            );
        }
    }
}

#[test]
fn yohen_code_clears_6_on_editor_and_cursor_line() {
    let theme = yohen();
    assert!(theme.code().len() > 20, "every capture is checked");
    code_clears(&theme, 6.);
}

#[test]
fn hakuji_code_clears_4_5() {
    let theme = hakuji();
    assert!(theme.code().len() > 20, "every capture is checked");
    code_clears(&theme, 4.5);
}

#[test]
fn closest_signal_pair_is_at_least_5_delta_e() {
    let theme = yohen();
    let signals = SIGNALS.map(|(name, place)| (name, theme.color(place)));
    let mut closest = (f64::MAX, "", "");
    for (i, &(a, x)) in signals.iter().enumerate() {
        for &(b, y) in &signals[i + 1..] {
            let distance = delta_e(x, y);
            if distance < closest.0 {
                closest = (distance, a, b);
            }
        }
    }
    let (distance, a, b) = closest;
    assert!(distance >= 5., "Yohen: {a} and {b} are {distance:.2} apart");
}

#[test]
fn paired_signals_differ_in_lightness() {
    for theme in [yohen(), hakuji()] {
        for (a, b) in PAIRS {
            let apart = (oklab(theme.color(a))[0] - oklab(theme.color(b))[0]).abs();
            assert!(
                apart >= 0.05,
                "{}: {a} and {b} differ by {apart:.3} in OKLab L",
                theme.id
            );
        }
    }
}

/// The oracle's own numbers: white against black, and one OKLab value.
#[test]
fn the_measures_match_known_values() {
    let (white, black) = (Color::rgb(0xffffff), Color::rgb(0x000000));
    assert!((contrast(white, black) - 21.).abs() < 1e-9);
    let [l, a, b] = oklab(white);
    assert!((l - 1.).abs() < 1e-4 && a.abs() < 1e-4 && b.abs() < 1e-4);
    assert!((delta_e(white, black) - 100.).abs() < 1e-2);
}

/// The design's note on the values T1512 retuned: each `#old` → `#new`
/// keeps its OKLab a and b, so hue and chroma, to within 8 bits a channel's
/// rounding, and moves in L.
#[test]
fn retuned_values_move_in_lightness_only() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/design/design-system.md");
    let doc = fs::read_to_string(&path).unwrap();
    let note = (doc.lines())
        .skip_while(|line| !line.starts_with("T1512 (rev 2)"))
        .take_while(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut moves = Vec::new();
    let mut rest = note.as_str();
    const ARROW: &str = "` → `";
    while let Some(at) = rest.find(ARROW) {
        let (old, new) = (&rest[at - 7..at], &rest[at + ARROW.len()..][..7]);
        moves.push((old.parse::<Color>().unwrap(), new.parse::<Color>().unwrap()));
        rest = &rest[at + ARROW.len() + 7..];
    }
    assert_eq!(moves.len(), 5, "the note's retuned values: {note}");
    for (old, new) in moves {
        let ([l0, a0, b0], [l1, a1, b1]) = (oklab(old), oklab(new));
        let drift = ((a1 - a0).powi(2) + (b1 - b0).powi(2)).sqrt();
        assert!(drift <= 0.002, "{old} → {new} moves a and b by {drift:.4}");
        assert!((l1 - l0).abs() >= 0.003, "{old} → {new} moves L");
    }
}

/// Hue in degrees, 0 to 360.
fn hue(color: Color) -> f64 {
    let [r, g, b] = [color.r(), color.g(), color.b()].map(|c| f64::from(c) / 255.);
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

/// A vim jump's labels, held to jump_label_contrast.rs's rules: opaque
/// boxes in two hues 60 degrees apart, each 3:1 on the editor, with a glyph
/// 7:1 on the live label and 4.5:1 on the preview.
#[test]
fn jump_labels_readable() {
    for theme in [yohen(), hakuji()] {
        let id = theme.id;
        let live = theme.color("editor.jump_label");
        let preview = theme.color("editor.jump_label_preview");
        let glyph = theme.color("editor.jump_label_foreground");
        let background = theme.color("editor.background");
        let on_live = contrast(glyph, live);
        assert!(on_live >= 7., "{id}: glyph on live label {on_live:.2}");
        let on_preview = contrast(glyph, preview);
        assert!(on_preview >= 4.5, "{id}: glyph on preview {on_preview:.2}");
        for (key, color) in [("jump_label", live), ("jump_label_preview", preview)] {
            let ratio = contrast(color, background);
            assert!(ratio >= 3., "{id}: {key} on background {ratio:.2}");
        }
        let apart = (hue(live) - hue(preview)).abs();
        let apart = apart.min(360. - apart);
        assert!(apart >= 60., "{id}: label hues {apart:.0} degrees apart");
    }
}
