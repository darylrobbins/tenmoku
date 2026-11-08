//! The editor keys for the design's roles (T1509): Tenmoku Dark and Light
//! set each to the colour the editor computes for it today, to a theme
//! file's 8 bits a channel, so nothing drawn changes when a reader moves to
//! the key.

use std::path::Path;

use fe_config::{EditorColors, EditorPalette, theme::EXTENSIONS};
use kiln_core::theme::{ThemeData, ThemeFile};

const KEYS: [&str; 5] = [
    "find_match",
    "find_match_other",
    "identifier_highlight",
    "agent_lane",
    "fold_hover",
];

/// fe-gpui's alpha for a search match (`element.rs:search_match_color`)
/// and a highlight over its tint (`decorations.rs:HIGHLIGHT_ALPHA`).
const TODAY_ALPHA: f32 = 0.22;

#[test]
fn tenmoku_palettes_set_the_new_editor_keys_to_todays_colours() {
    for key in KEYS {
        assert!(EditorColors::KEYS.contains(&key), "{key} is a colour key");
    }
    let alpha = (TODAY_ALPHA * 255.).round() as u8;
    assert_eq!(alpha, 0x38);
    let sources = [
        include_str!("../themes/tenmoku-dark.toml"),
        include_str!("../themes/tenmoku-light.toml"),
    ];
    for source in sources {
        let (file, diagnostics) = ThemeFile::parse(Path::new("bundled.toml"), source, EXTENSIONS);
        assert_eq!(diagnostics, []);
        let file = file.unwrap();
        let editor = &file.extensions["editor"];
        for key in KEYS {
            assert!(editor.contains_key(key), "the file sets {key}");
        }
        let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&file));
        assert_eq!(warnings, [] as [String; 0]);
        let e = &palette.editor;
        // Find matches and the identifier's uses: the foreground, faint.
        let faint = e.foreground.with_alpha(alpha);
        assert_eq!(e.find_match, faint);
        assert_eq!(e.find_match_other, faint);
        assert_eq!(e.identifier_highlight, faint);
        assert_eq!(e.fold_hover, faint);
        // An agent's rows: a highlight's accent tint (`Tint::Accent`, the
        // cursor's colour).
        assert_eq!(e.agent_lane, e.cursor.with_alpha(alpha));
    }
}
