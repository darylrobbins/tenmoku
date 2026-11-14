//! `workbench.color_blind_mode`: off by default; on, a theme's
//! `[color_blind]` table swaps the signals that tell green from red for
//! blue and orange (docs/design/design-system.md, Colour: "A colour-blind
//! mode swaps green and red for blue and orange as its own setting").
//! Yohen and Hakuji carry the table; a theme without one is unchanged.
//! Contrast is WCAG 2's, from relative luminance; lightness and hue are
//! OKLab's.

use std::{fs, path::Path};

use fe_config::{
    Color, EditorPalette, Settings, SettingsStore, USER_SETTINGS_TEMPLATE, settings_schema,
    theme::{COLOR_BLIND, EXTENSIONS, color_blind},
    themes,
};
use kiln_core::theme::{ThemeData, ThemeFile};

/// A bundled theme as kiln resolves it, and in colour-blind mode.
struct Theme {
    id: &'static str,
    plain: ThemeData,
    swapped: ThemeData,
    palette: EditorPalette,
    swapped_palette: EditorPalette,
}

impl Theme {
    fn load(id: &'static str) -> Theme {
        let plain = ThemeData::from_file(&themes::find(id).unwrap().file());
        let (swapped, warnings) = color_blind(&plain);
        assert_eq!(warnings, [] as [String; 0], "{id}");
        let (palette, warnings) = EditorPalette::for_theme(&plain);
        assert_eq!(warnings, [] as [String; 0], "{id}");
        let (swapped_palette, warnings) = EditorPalette::for_theme(&swapped);
        assert_eq!(warnings, [] as [String; 0], "{id}");
        Theme {
            id,
            plain,
            swapped,
            palette,
            swapped_palette,
        }
    }

    /// The colour at `place` in colour-blind mode: a token, or
    /// `editor.<key>`.
    fn swapped(&self, place: &str) -> Color {
        color_at(&self.swapped, &self.swapped_palette, place)
            .unwrap_or_else(|| panic!("{}: no {place}", self.id))
    }

    /// The colour at `place` with the mode off.
    fn plain(&self, place: &str) -> Color {
        color_at(&self.plain, &self.palette, place)
            .unwrap_or_else(|| panic!("{}: no {place}", self.id))
    }

    fn blue(&self) -> Color {
        self.swapped("status.success")
    }

    fn orange(&self) -> Color {
        self.swapped("status.error")
    }
}

fn color_at(data: &ThemeData, palette: &EditorPalette, place: &str) -> Option<Color> {
    match place.strip_prefix("editor.") {
        Some(key) => palette.editor.get(key),
        None => data.tokens.get(place).copied(),
    }
}

fn both() -> [Theme; 2] {
    [Theme::load("yohen"), Theme::load("hakuji")]
}

/// Where the green meaning is drawn: added, passed (a test that passed is
/// in the success colour) and INSERT.
const GREEN: [&str; 6] = [
    "status.success",
    "vcs.added",
    "vcs.untracked",
    "editor.change_added",
    "editor.minimap_gutter_added",
    "editor.vim_mode_insert",
];

/// Where the red meaning is drawn: removed, failed and errors, and REPLACE.
const RED: [&str; 7] = [
    "status.error",
    "vcs.deleted",
    "editor.error",
    "editor.change_deleted",
    "editor.minimap_gutter_deleted",
    "editor.minimap_error",
    "editor.vim_mode_replace",
];

/// The surfaces text sits on, as yohen_hakuji_contrast.rs has them.
const SURFACES: [&str; 8] = [
    "surface.chrome",
    "surface.panel",
    "editor.background",
    "editor.active_line",
    "state.hover",
    "state.selected_inactive",
    "state.selected",
    "surface.raised",
];

/// What stays: warnings, modified, the information colour and the accent.
const KEPT: [&str; 9] = [
    "status.warning",
    "status.warning_soft",
    "status.modified",
    "status.modified_soft",
    "status.info",
    "vcs.modified",
    "accent.base",
    "editor.warning",
    "editor.change_modified",
];

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

/// WCAG 2 contrast ratio of `a` against `b`.
fn contrast(a: Color, b: Color) -> f64 {
    let luminance = |color| {
        let [r, g, b] = linear(color);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    };
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

/// OKLab hue in degrees, -180 to 180.
fn hue(color: Color) -> f64 {
    let [_, a, b] = oklab(color);
    b.atan2(a).to_degrees()
}

/// How far apart two hues are, in degrees.
fn hue_apart(a: f64, b: f64) -> f64 {
    let apart = (a - b).abs() % 360.;
    apart.min(360. - apart)
}

#[test]
fn off_by_default() {
    assert!(!Settings::default().workbench.color_blind_mode);
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    fs::write(&user, "").unwrap();
    let store = SettingsStore::load(Some(user.clone()), None);
    assert!(!store.settings().workbench.color_blind_mode);
    fs::write(&user, "[workbench]\ncolor_blind_mode = true\n").unwrap();
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().workbench.color_blind_mode);

    // Documented in the template and the schema, the committed one too.
    assert!(USER_SETTINGS_TEMPLATE.contains("\n# color_blind_mode = false\n"));
    let schema = settings_schema();
    let prop = &schema["$defs"]["WorkbenchSettings"]["properties"]["color_blind_mode"];
    assert_eq!(prop["type"], "boolean", "{prop:#}");
    assert_eq!(prop["default"], false, "{prop:#}");
    let committed: serde_json::Value =
        serde_json::from_str(include_str!("../settings.schema.json")).unwrap();
    assert_eq!(
        committed["$defs"]["WorkbenchSettings"]["properties"]["color_blind_mode"],
        *prop
    );

    // Off, the table changes nothing: the palette is the theme's own.
    for theme in both() {
        assert!(theme.plain.extension(COLOR_BLIND).is_some(), "{}", theme.id);
        assert_eq!(theme.plain("status.success"), theme.plain("vcs.added"));
        assert_ne!(theme.plain("status.success"), theme.blue(), "{}", theme.id);
    }
}

#[test]
fn on_replaces_added_passed_insert_and_removed_failed_error_in_yohen_and_hakuji() {
    for theme in both() {
        let id = theme.id;
        let (blue, orange) = (theme.blue(), theme.orange());
        let (green, red) = (theme.plain("status.success"), theme.plain("status.error"));
        for (places, was, now) in [(&GREEN[..], green, blue), (&RED[..], red, orange)] {
            for place in places {
                assert_eq!(theme.plain(place), was, "{id}: {place} before");
                assert_eq!(theme.swapped(place), now, "{id}: {place} after");
            }
        }
        // Blue and orange: Okabe-Ito's hues, kept within a few degrees.
        for (name, color, from) in [
            ("blue", blue, Color::rgb(0x0072b2)),
            ("orange", orange, Color::rgb(0xd55e00)),
        ] {
            let apart = hue_apart(hue(color), hue(from));
            assert!(apart <= 3., "{id}: {name} {color} is {apart:.1}° off");
        }
        for place in KEPT {
            assert_eq!(theme.swapped(place), theme.plain(place), "{id}: {place}");
        }
        // Nothing else moves: the swapped places put back, the palettes and
        // tokens are the theme's.
        let mut tokens = theme.swapped.tokens;
        let mut palette = theme.swapped_palette.clone();
        for place in GREEN.iter().chain(&RED).chain(&[
            "status.success_soft",
            "status.error_soft",
            "editor.diff_added_row",
            "editor.diff_removed_row",
            "editor.diff_added_word",
            "editor.diff_removed_word",
        ]) {
            let was = theme.plain(place);
            match place.strip_prefix("editor.") {
                Some(key) => *palette_slot(&mut palette, key) = was,
                None => *tokens.get_mut(place).unwrap() = was,
            }
        }
        assert_eq!(tokens, theme.plain.tokens, "{id}");
        assert_eq!(palette, theme.palette, "{id}");
        assert_eq!(theme.swapped.name, theme.plain.name);
        assert_eq!(theme.swapped.appearance, theme.plain.appearance);
        assert_eq!(theme.swapped.contrast, theme.plain.contrast);
        // The table is spent: laying it again changes nothing.
        assert!(theme.swapped.extension(COLOR_BLIND).is_none(), "{id}");
        assert_eq!(color_blind(&theme.swapped), (theme.swapped.clone(), vec![]));
    }
}

/// `palette`'s `[editor]` colour at `key`.
fn palette_slot<'a>(palette: &'a mut EditorPalette, key: &str) -> &'a mut Color {
    let editor = &mut palette.editor;
    match key {
        "error" => &mut editor.error,
        "change_added" => &mut editor.change_added,
        "change_deleted" => &mut editor.change_deleted,
        "minimap_gutter_added" => &mut editor.minimap_gutter_added,
        "minimap_gutter_deleted" => &mut editor.minimap_gutter_deleted,
        "minimap_error" => &mut editor.minimap_error,
        "vim_mode_insert" => &mut editor.vim_mode_insert,
        "vim_mode_replace" => &mut editor.vim_mode_replace,
        "diff_added_row" => &mut editor.diff_added_row,
        "diff_removed_row" => &mut editor.diff_removed_row,
        "diff_added_word" => &mut editor.diff_added_word,
        "diff_removed_word" => &mut editor.diff_removed_word,
        _ => panic!("no slot for {key}"),
    }
}

#[test]
fn soft_grounds_follow() {
    for theme in both() {
        let id = theme.id;
        for (signal, soft, row, word) in [
            (
                "status.success",
                "status.success_soft",
                "editor.diff_added_row",
                "editor.diff_added_word",
            ),
            (
                "status.error",
                "status.error_soft",
                "editor.diff_removed_row",
                "editor.diff_removed_word",
            ),
        ] {
            let (before, after) = (theme.plain(soft), theme.swapped(soft));
            let color = theme.swapped(signal);
            assert_ne!(after, before, "{id}: {soft}");
            assert_eq!(after.a(), 0xff, "{id}: {soft}");
            // The signal's hue, at the soft ground's lightness before.
            let apart = hue_apart(hue(after), hue(color));
            assert!(
                apart <= 5.,
                "{id}: {soft} {after} is {apart:.1}° off {color}"
            );
            let moved = (oklab(after)[0] - oklab(before)[0]).abs();
            assert!(moved <= 0.005, "{id}: {soft} moved {moved:.3} in L");
            // A diff's rows are the soft ground, its words the signal at
            // most a fifth.
            assert_eq!(theme.swapped(row), after, "{id}: {row}");
            let word = theme.swapped(word);
            assert_eq!(word.with_alpha(0xff), color, "{id}: {word}");
            assert!(word.a() > 0 && word.a() <= 0x33, "{id}: {word}");
            // The signal reads on its own soft ground.
            let ratio = contrast(color, after);
            assert!(ratio >= 4.5, "{id}: {signal} on {soft} is {ratio:.2}:1");
        }
    }
}

#[test]
fn a_theme_without_the_table_is_unchanged() {
    for theme in themes::bundled_themes() {
        let data = ThemeData::from_file(&theme.file());
        if ["yohen", "hakuji"].contains(&theme.id) {
            continue;
        }
        assert!(data.extension(COLOR_BLIND).is_none(), "{}", theme.id);
        assert_eq!(color_blind(&data), (data.clone(), vec![]), "{}", theme.id);
    }
    // A user's theme of its own, with no table.
    let source = "appearance = \"dark\"\n\n[status]\nsuccess = \"#00ff00\"\n";
    let (file, diagnostics) = ThemeFile::parse(Path::new("mine.toml"), source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    let data = ThemeData::from_file(&file.unwrap());
    assert_eq!(color_blind(&data), (data.clone(), vec![]));

    // One with a table of its own: its keys apply, and what isn't a token
    // or a colour is left out with a warning.
    let source = "appearance = \"dark\"\n\n[color_blind.status]\nsuccess = \"#0000ff\"\n\
                  nope = \"#000000\"\nerror = 3\n\n[color_blind.editor]\n\
                  change_added = \"#0000ff\"\n";
    let (file, diagnostics) = ThemeFile::parse(Path::new("mine.toml"), source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    let data = ThemeData::from_file(&file.unwrap());
    let (swapped, warnings) = color_blind(&data);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings[0].contains("color_blind.status.error"),
        "{warnings:?}"
    );
    assert!(
        warnings[1].contains("color_blind.status.nope"),
        "{warnings:?}"
    );
    assert_eq!(swapped.tokens.status.success, Color::rgb(0x0000ff));
    assert_eq!(swapped.tokens.vcs.added, Color::rgb(0x0000ff));
    assert_eq!(swapped.tokens.status.error, data.tokens.status.error);
    let (palette, warnings) = EditorPalette::for_theme(&swapped);
    assert_eq!(warnings, [] as [String; 0]);
    assert_eq!(palette.editor.change_added, Color::rgb(0x0000ff));
}

#[test]
fn replacements_differ_in_oklab_l_by_at_least_0_05_and_clear_4_5_on_their_surfaces() {
    for theme in both() {
        let id = theme.id;
        let (blue, orange) = (theme.blue(), theme.orange());
        let apart = (oklab(blue)[0] - oklab(orange)[0]).abs();
        assert!(apart >= 0.05, "{id}: blue and orange {apart:.3} apart in L");
        // As the design keeps warning and error apart, and added and
        // modified are told apart too.
        for (a, b) in [
            ("status.warning", "status.error"),
            ("status.modified", "status.success"),
        ] {
            let apart = (oklab(theme.swapped(a))[0] - oklab(theme.swapped(b))[0]).abs();
            assert!(apart >= 0.05, "{id}: {a} and {b} {apart:.3} apart in L");
        }
        for (name, color) in [("blue", blue), ("orange", orange)] {
            assert_eq!(color.a(), 0xff, "{id}: {name}");
            for surface in SURFACES {
                let ratio = contrast(color, theme.swapped(surface));
                assert!(ratio >= 4.5, "{id}: {name} on {surface} is {ratio:.2}:1");
            }
        }
        // The vim mode's label on INSERT and REPLACE.
        let label = theme.swapped("editor.vim_mode_foreground");
        for mode in ["editor.vim_mode_insert", "editor.vim_mode_replace"] {
            let ratio = contrast(label, theme.swapped(mode));
            assert!(ratio >= 4.5, "{id}: label on {mode} is {ratio:.2}:1");
        }
    }
}
