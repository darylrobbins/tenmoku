//! The minimap's colours: every bundled theme sets the eleven `minimap_*`
//! keys with VS Code's values for its appearance (the vscode-* themes with
//! their own), and a user theme's `[editor]` can set them.

use std::path::Path;

use fe_config::{Color, EditorColors, EditorPalette, theme::EXTENSIONS, themes::bundled_themes};
use kiln_core::theme::{Appearance, ThemeData, ThemeFile};

const KEYS: [&str; 11] = [
    "minimap_background",
    "minimap_slider",
    "minimap_slider_hover",
    "minimap_slider_active",
    "minimap_selection",
    "minimap_find_match",
    "minimap_error",
    "minimap_warning",
    "minimap_gutter_added",
    "minimap_gutter_modified",
    "minimap_gutter_deleted",
];

fn palette(file: &ThemeFile) -> EditorPalette {
    let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(file));
    assert_eq!(warnings, Vec::<String>::new());
    palette
}

#[test]
fn every_bundled_theme_sets_every_minimap_key() {
    for key in KEYS {
        assert!(EditorColors::KEYS.contains(&key), "{key} is a colour key");
    }
    // Appended at the end of the table.
    assert_eq!(EditorColors::KEYS[EditorColors::KEYS.len() - 11..], KEYS);
    assert_eq!(bundled_themes().len(), 6);
    for theme in bundled_themes() {
        let file = theme.file();
        let editor = &file.extensions["editor"];
        for key in KEYS {
            assert!(editor.contains_key(key), "{} sets {key}", theme.id);
        }
        palette(&file);
    }
}

#[test]
fn minimap_colours_are_vscode_s() {
    for theme in bundled_themes() {
        let colors = palette(&theme.file()).editor;
        let id = theme.id;
        // minimap.background is the editor's; the markers' defaults follow
        // editorWarning.foreground and editorGutter.*.
        assert_eq!(colors.minimap_background, colors.background, "{id}");
        assert_eq!(colors.minimap_warning, colors.warning, "{id}");
        assert_eq!(colors.minimap_gutter_added, colors.change_added, "{id}");
        assert_eq!(
            colors.minimap_gutter_modified, colors.change_modified,
            "{id}"
        );
        assert_eq!(colors.minimap_gutter_deleted, colors.change_deleted, "{id}");
        assert_eq!(colors.minimap_error, Color::rgba(0xFF1212B3), "{id}");
        assert_eq!(colors.minimap_find_match, Color::rgb(0xD18616), "{id}");

        let slider = [
            colors.minimap_slider,
            colors.minimap_slider_hover,
            colors.minimap_slider_active,
            colors.minimap_selection,
        ];
        let want = match (id, theme.appearance) {
            // 2026-dark.json and 2026-light.json set minimapSlider.* and
            // editor.selectionBackground.
            ("vscode-2026-dark", _) => [0xA8A9AA85, 0xA8A9AA90, 0xA8A9AA9C, 0x276782DD],
            ("vscode-2026-light", _) => [0x646464C0, 0x646464D0, 0x646464E0, 0x0069CC40],
            // scrollbarSlider.* at half alpha, editor.selectionBackground.
            (_, Appearance::Dark) => [0x79797933, 0x64646459, 0xBFBFBF33, 0x264F78FF],
            (_, Appearance::Light) => [0x64646433, 0x64646459, 0x0000004D, 0xADD6FFFF],
        };
        assert_eq!(slider, want.map(Color::rgba), "{id}");
    }
}

#[test]
fn a_user_theme_sets_the_minimap_slider() {
    let source = "appearance = \"dark\"\n\n[editor]\nminimap_slider = \"#12345678\"\n";
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    let colors = palette(&file.unwrap()).editor;
    assert_eq!(colors.minimap_slider, Color::rgba(0x12345678));
    let dark = &EditorPalette::fastedit_dark().editor;
    assert_eq!(colors.minimap_slider_hover, dark.minimap_slider_hover);
    assert_eq!(colors.minimap_background, dark.minimap_background);
}

/// Dark Modern's diagnostics are VS Code's editorError.foreground and
/// editorInfo.foreground defaults, which dark_modern.json doesn't set: its
/// #F85149 is the general errorForeground (T200 F1).
#[test]
fn dark_modern_diagnostics_match_upstream() {
    let theme = fe_config::themes::find("vscode-dark-modern").unwrap();
    let colors = palette(&theme.file()).editor;
    assert_eq!(colors.error, Color::rgb(0xF14C4C));
    assert_eq!(colors.warning, Color::rgb(0xCCA700));
    assert_eq!(colors.information, Color::rgb(0x59A4F9));
    assert_eq!(colors.minimap_warning, Color::rgb(0xCCA700));
}
