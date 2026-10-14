//! The indent guides' colours: each bundled theme sets them faint, and a
//! kiln theme's `[editor]` can set them.

use std::path::Path;

use fe_config::{Color, EditorColors, EditorPalette, theme::EXTENSIONS};
use kiln_core::theme::{ThemeData, ThemeFile};

fn parse(source: &str) -> ThemeFile {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    file.unwrap()
}

fn alpha(color: Color) -> f32 {
    f32::from(color.a()) / 255.
}

#[test]
fn bundled_themes_set_faint_guides() {
    assert_eq!(EditorColors::KEYS.len(), 27);
    for (name, source) in [
        ("dark", include_str!("../themes/fastedit-dark.toml")),
        ("light", include_str!("../themes/fastedit-light.toml")),
        ("ember", include_str!("../themes/fastedit-ember.toml")),
    ] {
        let editor = &parse(source).extensions["editor"];
        for key in ["indent_guide", "indent_guide_active"] {
            assert!(editor.contains_key(key), "{name} sets {key}");
        }
        let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&parse(source)));
        assert_eq!(warnings, Vec::<String>::new());
        let (guide, active) = (
            alpha(palette.editor.indent_guide),
            alpha(palette.editor.indent_guide_active),
        );
        assert!(guide < 0.3 && active < 0.3, "{name}: {guide}, {active}");
        assert!(active > guide, "{name}: {guide}, {active}");
    }
}

#[test]
fn a_theme_sets_the_guide_colour() {
    let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&parse(
        "appearance = \"dark\"\n\n[editor]\nindent_guide = \"#123456\"\n",
    )));
    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(palette.editor.indent_guide, "#123456".parse().unwrap());
    assert_eq!(
        palette.editor.indent_guide_active,
        EditorPalette::fastedit_dark().editor.indent_guide_active
    );
}
