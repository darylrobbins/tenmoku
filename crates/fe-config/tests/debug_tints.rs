//! The debugger's editor colours (`docs/design/design-system.md`, Debugger,
//! Colour roles; D0's decision file): the stopped line's band, a caller
//! frame's, and the enabled and disabled breakpoints. Every bundled theme
//! sets them, Yohen and Hakuji to D0's values, and a theme without them
//! gets its own colours [`DEBUG_FALLBACKS`] names. And the TODO-comment
//! badges (T1497 F1): a theme without a badge's name gets its kind's
//! diagnostic tint, not the bundled palette's colour for it.

use std::path::Path;

use fe_config::{
    Color, EditorColors, EditorPalette,
    theme::{DEBUG_FALLBACKS, EXTENSIONS},
};
use kiln_core::theme::{ThemeData, ThemeFile, contrast_ratio, over};

/// The keys, in the order they're declared.
const KEYS: [&str; 5] = [
    "debug_current_line",
    "debug_frame_line",
    "debug_breakpoint",
    "debug_breakpoint_disabled",
    "debug_frame_arrow",
];

const BUNDLED: &[(&str, &str)] = &[
    ("tenmoku-dark", include_str!("../themes/tenmoku-dark.toml")),
    (
        "tenmoku-light",
        include_str!("../themes/tenmoku-light.toml"),
    ),
    (
        "tenmoku-ember",
        include_str!("../themes/tenmoku-ember.toml"),
    ),
    (
        "catppuccin-latte",
        include_str!("../themes/catppuccin-latte.toml"),
    ),
    (
        "catppuccin-frappe",
        include_str!("../themes/catppuccin-frappe.toml"),
    ),
    (
        "catppuccin-macchiato",
        include_str!("../themes/catppuccin-macchiato.toml"),
    ),
    (
        "catppuccin-mocha",
        include_str!("../themes/catppuccin-mocha.toml"),
    ),
    (
        "vscode-2026-dark",
        include_str!("../themes/vscode-2026-dark.toml"),
    ),
    (
        "vscode-2026-light",
        include_str!("../themes/vscode-2026-light.toml"),
    ),
    (
        "vscode-dark-modern",
        include_str!("../themes/vscode-dark-modern.toml"),
    ),
    (
        "vscode-monokai",
        include_str!("../themes/vscode-monokai.toml"),
    ),
    ("yohen", include_str!("../themes/yohen.toml")),
    ("hakuji", include_str!("../themes/hakuji.toml")),
];

/// D0's values (`docs/decisions/skgc7fe5z9-debugger-design.md`): current
/// line, caller frame, breakpoint, disabled breakpoint, and the frame
/// arrows in the accent (hare's fur).
const YOHEN: [&str; 5] = ["#351501", "#2a1a11", "#f2716f", "#827468", "#e0874c"];
const HAKUJI: [&str; 5] = ["#fff4ed", "#f6f7f9", "#a1100e", "#7c828a", "#a84a16"];

fn parse(source: &str) -> ThemeData {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    ThemeData::from_file(&file.unwrap())
}

fn color(hex: &str) -> Color {
    hex.parse().unwrap()
}

/// `palette`'s debugger colours, in [`KEYS`]' order.
fn debug_colors(palette: &EditorPalette) -> [Color; 5] {
    KEYS.map(|key| palette.editor.get(key).expect("a colour key"))
}

#[test]
fn every_bundled_theme_sets_the_debug_keys() {
    assert_eq!(
        std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/themes"))
            .unwrap()
            .count(),
        BUNDLED.len()
    );
    // Before the minimap's, which come last.
    let at = |key: &str| EditorColors::KEYS.iter().position(|k| *k == key).unwrap();
    for key in KEYS {
        assert!(at(key) < at("minimap_background"), "{key}");
    }
    for (id, source) in BUNDLED {
        let (file, _) = ThemeFile::parse(Path::new("t.toml"), source, EXTENSIONS);
        let editor = &file.unwrap().extensions["editor"];
        for key in KEYS {
            assert!(editor.contains_key(key), "{id} sets {key}");
        }
        let (palette, warnings) = EditorPalette::for_theme(&parse(source));
        assert_eq!(warnings, [] as [String; 0], "{id}");
        let [current, frame, breakpoint, disabled, _] = debug_colors(&palette);
        // Bands are opaque: a selection, a match and the identifier's uses
        // draw over them, and they draw instead of the cursor line.
        assert_eq!((current.a(), frame.a()), (255, 255), "{id}");
        assert_ne!(current, frame, "{id}: the two bands differ");
        assert_ne!(breakpoint, disabled, "{id}");
    }
}

#[test]
fn yohen_and_hakuji_have_d0s_values() {
    for (id, want) in [("yohen", YOHEN), ("hakuji", HAKUJI)] {
        let source = BUNDLED.iter().find(|(name, _)| *name == id).unwrap().1;
        let (palette, _) = EditorPalette::for_theme(&parse(source));
        assert_eq!(debug_colors(&palette), want.map(color), "{id}");
    }
    // The bundled palettes are theirs, so the editor's default theme has
    // them too.
    assert_eq!(
        debug_colors(EditorPalette::fastedit_dark()),
        YOHEN.map(color)
    );
    assert_eq!(
        debug_colors(EditorPalette::fastedit_light()),
        HAKUJI.map(color)
    );
}

/// A theme of `appearance` that sets the fallbacks' colours but none of
/// the debugger's.
fn without_the_keys(appearance: &str) -> String {
    format!(
        "appearance = \"{appearance}\"\n\n[editor]\nactive_line = \"#123456\"\n\
         error = \"#ab0102\"\ngutter_foreground = \"#778899\"\ncursor = \"#334455\"\n"
    )
}

#[test]
fn a_theme_without_them_gets_the_fallbacks() {
    assert_eq!(
        DEBUG_FALLBACKS,
        [
            ("debug_current_line", "active_line"),
            ("debug_frame_line", "active_line"),
            ("debug_breakpoint", "error"),
            ("debug_breakpoint_disabled", "gutter_foreground"),
            ("debug_frame_arrow", "cursor"),
        ]
    );
    for appearance in ["dark", "light"] {
        let (palette, warnings) = EditorPalette::for_theme(&parse(&without_the_keys(appearance)));
        assert_eq!(warnings, [] as [String; 0]);
        let want = ["#123456", "#123456", "#ab0102", "#778899", "#334455"].map(color);
        assert_eq!(debug_colors(&palette), want, "{appearance}");
    }
    // One with no `[editor]` table, as Kiln's own themes, draws the editor
    // in the bundled palette, its debug colours too.
    for (source, bundled) in [
        ("appearance = \"dark\"\n", EditorPalette::fastedit_dark()),
        ("appearance = \"light\"\n", EditorPalette::fastedit_light()),
    ] {
        let (palette, _) = EditorPalette::for_theme(&parse(source));
        assert_eq!(debug_colors(&palette), debug_colors(bundled));
    }
    // One that sets a key keeps it; the others still fall back.
    let source = format!(
        "{}debug_frame_line = \"#010203\"\n",
        without_the_keys("dark")
    );
    let (palette, _) = EditorPalette::for_theme(&parse(&source));
    assert_eq!(
        debug_colors(&palette),
        ["#123456", "#010203", "#ab0102", "#778899", "#334455"].map(color)
    );
}

/// The TODO-comment badges' names (`todo_comments.rs`'s `highlight_name`).
const BADGES: [&str; 6] = [
    "comment.fixme",
    "comment.hack",
    "comment.note",
    "comment.perf",
    "comment.todo",
    "comment.warn",
];

/// T1497 F1: a custom theme with diagnostic colours of its own and no
/// badge names lists none of the bundled palette's badge colours, in
/// either appearance, so each badge takes its kind's tint (the theme's
/// error, warning…) as a theme without one does in the editor. One
/// without an `[editor]` table is the bundled palette's, badges too.
#[test]
fn a_theme_without_badge_names_gets_none_of_the_bundled_ones() {
    for (appearance, bundled) in [
        ("dark", EditorPalette::fastedit_dark()),
        ("light", EditorPalette::fastedit_light()),
    ] {
        for badge in BADGES {
            assert!(
                bundled.syntax_color(badge).is_some(),
                "{appearance} {badge}"
            );
        }
        let source = format!(
            "appearance = \"{appearance}\"\n\n[editor]\nerror = \"#ff0001\"\n\
             warning = \"#ff0002\"\ninformation = \"#ff0003\"\nhint = \"#ff0004\"\n\n\
             [syntax]\ncomment = \"#808080\"\n"
        );
        let (palette, warnings) = EditorPalette::for_theme(&parse(&source));
        assert_eq!(warnings, [] as [String; 0]);
        for badge in BADGES {
            assert_eq!(palette.syntax_color(badge), None, "{appearance} {badge}");
        }
        assert_eq!(palette.syntax_color("comment"), Some(color("#808080")));
        // The rest of the bundled captures are still there.
        assert_eq!(
            palette.syntax_color("keyword"),
            bundled.syntax_color("keyword")
        );
    }
    let (palette, _) = EditorPalette::for_theme(&parse("appearance = \"dark\"\n"));
    for badge in BADGES {
        let bundled = EditorPalette::fastedit_dark().syntax_color(badge);
        assert_eq!(palette.syntax_color(badge), bundled, "{badge}");
    }
    // A badge a theme sets, quoted or through a nested table, is its own.
    let editor = "appearance = \"dark\"\n\n[editor]\nerror = \"#ff0001\"\n\n";
    let quoted = format!("{editor}[syntax]\n\"comment.todo\" = \"#010101\"\n");
    let nested = format!("{editor}[syntax.comment]\nfixme = \"#020202\"\n");
    let (palette, _) = EditorPalette::for_theme(&parse(&quoted));
    assert_eq!(palette.syntax_color("comment.todo"), Some(color("#010101")));
    assert_eq!(palette.syntax_color("comment.fixme"), None);
    let (palette, _) = EditorPalette::for_theme(&parse(&nested));
    assert_eq!(
        palette.syntax_color("comment.fixme"),
        Some(color("#020202"))
    );
    assert_eq!(palette.syntax_color("comment.todo"), None);
}

/// A colour's OKLab lightness.
fn lightness(color: Color) -> f32 {
    let linear = |c: u8| {
        let c = f32::from(c) / 255.;
        match c <= 0.04045 {
            true => c / 12.92,
            false => ((c + 0.055) / 1.055).powf(2.4),
        }
    };
    let (r, g, b) = (linear(color.r()), linear(color.g()), linear(color.b()));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s
}

/// D0's contrast floor, in every bundled theme: each glyph (an enabled
/// breakpoint, a disabled one, a frame arrow) at least 3:1 wherever it
/// sits over the line numbers (the gutter, the cursor line and the hover
/// tile), the arrow's colour 3:1 on the current line's band too, where it's
/// the band's edge; and the disabled grey at least 0.1 OKLab L from the
/// red, so they differ by more than hue.
#[test]
fn every_bundled_themes_glyphs_keep_the_contrast_floor() {
    for (id, source) in BUNDLED {
        let (palette, _) = EditorPalette::for_theme(&parse(source));
        let e = &palette.editor;
        let ground = over(e.background, Color::rgb(0));
        let grounds = [
            ("gutter", ground),
            ("cursor line", over(e.active_line, ground)),
            ("hover tile", over(e.fold_hover, ground)),
        ];
        let glyphs = [
            ("breakpoint", e.debug_breakpoint),
            ("disabled breakpoint", e.debug_breakpoint_disabled),
            ("frame arrow", e.debug_frame_arrow),
        ];
        for (glyph, color) in glyphs {
            for (name, under) in grounds {
                let ratio = contrast_ratio(over(color, under), under);
                assert!(ratio >= 3., "{id}: {glyph} on the {name}: {ratio:.2}:1");
            }
        }
        let band = over(e.debug_current_line, ground);
        let edge = contrast_ratio(e.debug_frame_arrow, band);
        assert!(edge >= 3., "{id}: the current line's edge: {edge:.2}:1");
        let apart = (lightness(e.debug_breakpoint) - lightness(e.debug_breakpoint_disabled)).abs();
        assert!(
            apart >= 0.1,
            "{id}: disabled and enabled {apart:.3} OKLab L apart"
        );
    }
}
