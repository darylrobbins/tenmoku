//! Sticky scroll's colours: every bundled theme sets the four
//! `sticky_scroll_*` keys with VS Code's values (editorStickyScroll.* and
//! editorStickyScrollHover.background), and a user theme without them gets
//! the bundled palette's for its appearance.

use std::path::Path;

use fe_config::{Color, EditorColors, EditorPalette, theme::EXTENSIONS, themes::bundled_themes};
use kiln_core::theme::{ThemeData, ThemeFile};

const KEYS: [&str; 4] = [
    "sticky_scroll_background",
    "sticky_scroll_border",
    "sticky_scroll_shadow",
    "sticky_scroll_hover",
];

fn palette(file: &ThemeFile) -> EditorPalette {
    let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(file));
    assert_eq!(warnings, Vec::<String>::new());
    palette
}

fn colors(colors: &EditorColors) -> [Color; 4] {
    [
        colors.sticky_scroll_background,
        colors.sticky_scroll_border,
        colors.sticky_scroll_shadow,
        colors.sticky_scroll_hover,
    ]
}

#[test]
fn every_bundled_theme_sets_vscode_s_sticky_scroll_colours() {
    // Right after the indent guides' keys.
    let at = (EditorColors::KEYS.iter())
        .position(|&key| key == "indent_guide_active")
        .unwrap();
    assert_eq!(EditorColors::KEYS[at + 1..at + 5], KEYS);
    let want: [(&str, [u32; 4]); 11] = [
        ("vscode-2026-dark", [0x121314FF, 0, 0x191B1D4D, 0x2A2D2EFF]),
        (
            "vscode-dark-modern",
            [0x1F1F1FFF, 0, 0x000000FF, 0x2A2D2EFF],
        ),
        ("vscode-2026-light", [0xFFFFFFFF, 0, 0, 0xF0F0F0FF]),
        ("catppuccin-latte", [0xEFF1F5FF, 0, 0xDCE0E8FF, 0xCCD0DAFF]),
        ("catppuccin-frappe", [0x303446FF, 0, 0x232634FF, 0x414559FF]),
        (
            "catppuccin-macchiato",
            [0x24273AFF, 0, 0x181926FF, 0x363A4FFF],
        ),
        ("catppuccin-mocha", [0x1E1E2EFF, 0, 0x11111BFF, 0x313244FF]),
        ("vscode-monokai", [0x272822FF, 0, 0x000000FF, 0x2A2D2EFF]),
        ("tenmoku-dark", [0x16181DFF, 0, 0x000000FF, 0x2A2D2EFF]),
        ("tenmoku-ember", [0x1C1814FF, 0, 0x000000FF, 0x2A2D2EFF]),
        ("tenmoku-light", [0xFFFFFFFF, 0, 0xDDDDDDFF, 0xF0F0F0FF]),
    ];
    assert_eq!(bundled_themes().len(), want.len());
    for (id, want) in want {
        let theme = fe_config::themes::find(id).unwrap();
        let file = theme.file();
        for key in KEYS {
            assert!(
                file.extensions["editor"].contains_key(key),
                "{id} sets {key}"
            );
        }
        let editor = palette(&file).editor;
        assert_eq!(colors(&editor), want.map(Color::rgba), "{id}");
        // The band's background is the editor's.
        assert_eq!(editor.sticky_scroll_background, editor.background, "{id}");
    }
}

#[test]
fn a_user_theme_without_them_gets_the_bundled_palette_s() {
    for (appearance, bundled) in [
        ("dark", EditorPalette::fastedit_dark()),
        ("light", EditorPalette::fastedit_light()),
    ] {
        let source =
            format!("appearance = \"{appearance}\"\n\n[editor]\nbackground = \"#123456\"\n");
        let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), &source, EXTENSIONS);
        assert_eq!(diagnostics, []);
        let editor = palette(&file.unwrap()).editor;
        assert_eq!(colors(&editor), colors(&bundled.editor), "{appearance}");
    }

    // And one that sets them has its own.
    let source = "appearance = \"dark\"\n\n[editor]\nsticky_scroll_border = \"#ff000080\"\n\
                  sticky_scroll_hover = \"#010203\"\n";
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    let editor = palette(&file.unwrap()).editor;
    assert_eq!(editor.sticky_scroll_border, Color::rgba(0xFF000080));
    assert_eq!(editor.sticky_scroll_hover, Color::rgb(0x010203));
    let dark = &EditorPalette::fastedit_dark().editor;
    assert_eq!(editor.sticky_scroll_shadow, dark.sticky_scroll_shadow);
}
