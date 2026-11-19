//! The test-coverage gutter marks' colours (T701, cov2): every bundled
//! theme sets `coverage_covered`, `coverage_uncovered` and
//! `coverage_partial` in its `[editor]` table, each at least 3:1 against
//! the editor's background, covered never the git green; and a theme with
//! an `[editor]` table that leaves them out gets its own colours
//! [`COVERAGE_FALLBACKS`] names, as the debugger's keys do
//! ([`fe_config::theme::DEBUG_FALLBACKS`], `debug_tints.rs`).

use std::path::Path;

use fe_config::{
    Color, EditorColors, EditorPalette,
    theme::{COVERAGE_FALLBACKS, EXTENSIONS},
};
use kiln_core::theme::{ThemeData, ThemeFile, contrast_ratio, over};

/// The keys, in the order they're declared.
const KEYS: [&str; 3] = ["coverage_covered", "coverage_uncovered", "coverage_partial"];

/// The marks' least contrast against the editor's background: WCAG's for
/// graphics.
const MARK_CONTRAST: f32 = 3.;

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

/// Yohen's and Hakuji's: seiji (the type colour, a teal apart from
/// celadon's added green), shinsha (failed) and kiseto (flaky), from
/// `docs/design/design-system.md`'s roles.
const YOHEN: [&str; 3] = ["#7fc9be", "#f2716f", "#e8b440"];
const HAKUJI: [&str; 3] = ["#0d6e66", "#a1100e", "#776a00"];

fn parse(source: &str) -> ThemeData {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    ThemeData::from_file(&file.unwrap())
}

fn color(hex: &str) -> Color {
    hex.parse().unwrap()
}

/// `palette`'s coverage colours, in [`KEYS`]' order.
fn coverage_colors(palette: &EditorPalette) -> [Color; 3] {
    KEYS.map(|key| palette.editor.get(key).expect("a colour key"))
}

#[test]
fn coverage_keys_set_in_every_bundled_theme_with_contrast() {
    assert_eq!(
        std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/themes"))
            .unwrap()
            .count(),
        BUNDLED.len()
    );
    // Before the minimap's, which come last.
    let at = |key: &str| EditorColors::KEYS.iter().position(|k| *k == key);
    for key in KEYS {
        let at_key = at(key).unwrap_or_else(|| panic!("{key} is a colour key"));
        assert!(at_key < at("minimap_background").unwrap(), "{key}");
    }
    for (id, source) in BUNDLED {
        // Each theme spells them out, rather than leaning on the fallbacks.
        let (file, _) = ThemeFile::parse(Path::new("t.toml"), source, EXTENSIONS);
        let editor = &file.unwrap().extensions["editor"];
        for key in KEYS {
            assert!(editor.contains_key(key), "{id} sets {key}");
        }
        let (palette, warnings) = EditorPalette::for_theme(&parse(source));
        assert_eq!(warnings, [] as [String; 0], "{id}");
        let e = &palette.editor;
        let background = over(e.background, Color::rgb(0));
        let [covered, uncovered, partial] = coverage_colors(&palette);
        for (key, mark) in KEYS.into_iter().zip([covered, uncovered, partial]) {
            assert_eq!(mark.a(), 0xff, "{id}: {key} is opaque");
            let ratio = contrast_ratio(mark, background);
            assert!(ratio >= MARK_CONTRAST, "{id}: {key} is {ratio:.2}:1");
        }
        // Three marks apart, and apart from the git bars beside them:
        // covered is never the added green, nor any mark the modified bar.
        assert_ne!(covered, uncovered, "{id}");
        assert_ne!(covered, partial, "{id}");
        assert_ne!(uncovered, partial, "{id}");
        assert_ne!(covered, e.change_added, "{id}: covered is not git's added");
        for (key, mark) in KEYS.into_iter().zip([covered, uncovered, partial]) {
            assert_ne!(mark, e.change_modified, "{id}: {key} is not git's modified");
        }
    }
}

#[test]
fn yohen_and_hakuji_take_the_design_roles() {
    for (id, want) in [("yohen", YOHEN), ("hakuji", HAKUJI)] {
        let source = BUNDLED.iter().find(|(name, _)| *name == id).unwrap().1;
        let (palette, _) = EditorPalette::for_theme(&parse(source));
        assert_eq!(coverage_colors(&palette), want.map(color), "{id}");
        let syntax = palette.syntax_color("type").expect("a type colour");
        let e = &palette.editor;
        // Seiji, shinsha and kiseto: the type colour, removed's and the
        // warnings'.
        assert_eq!(
            coverage_colors(&palette),
            [syntax, e.change_deleted, e.warning],
            "{id}"
        );
    }
    // The bundled palettes are theirs, so the editor's default theme has
    // them too.
    assert_eq!(
        coverage_colors(EditorPalette::fastedit_dark()),
        YOHEN.map(color)
    );
    assert_eq!(
        coverage_colors(EditorPalette::fastedit_light()),
        HAKUJI.map(color)
    );
}

/// A theme of `appearance` that sets the fallbacks' colours but none of
/// the coverage keys.
fn without_the_keys(appearance: &str) -> String {
    format!(
        "appearance = \"{appearance}\"\n\n[editor]\ninformation = \"#123456\"\n\
         change_added = \"#00ff00\"\nchange_deleted = \"#ab0102\"\nwarning = \"#778899\"\n"
    )
}

#[test]
fn a_theme_without_them_gets_the_fallbacks() {
    // Covered falls back to the information colour, not the added one.
    assert_eq!(
        COVERAGE_FALLBACKS,
        [
            ("coverage_covered", "information"),
            ("coverage_uncovered", "change_deleted"),
            ("coverage_partial", "warning"),
        ]
    );
    for appearance in ["dark", "light"] {
        let (palette, warnings) = EditorPalette::for_theme(&parse(&without_the_keys(appearance)));
        assert_eq!(warnings, [] as [String; 0]);
        let want = ["#123456", "#ab0102", "#778899"].map(color);
        assert_eq!(coverage_colors(&palette), want, "{appearance}");
        assert_ne!(coverage_colors(&palette)[0], palette.editor.change_added);
    }
    // One with no `[editor]` table, as Kiln's own themes, draws the editor
    // in the bundled palette, its coverage colours too.
    for (source, bundled) in [
        ("appearance = \"dark\"\n", EditorPalette::fastedit_dark()),
        ("appearance = \"light\"\n", EditorPalette::fastedit_light()),
    ] {
        let (palette, _) = EditorPalette::for_theme(&parse(source));
        assert_eq!(coverage_colors(&palette), coverage_colors(bundled));
    }
    // One that sets a key keeps it; the others still fall back.
    let source = format!(
        "{}coverage_partial = \"#010203\"\n",
        without_the_keys("dark")
    );
    let (palette, _) = EditorPalette::for_theme(&parse(&source));
    assert_eq!(
        coverage_colors(&palette),
        ["#123456", "#ab0102", "#010203"].map(color)
    );
}
