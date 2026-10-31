//! `editor.render_whitespace` and `editor.rulers`, and the `whitespace` and
//! `ruler` colours every bundled theme sets.

use std::path::Path;

use fe_config::{
    Color, EditorColors, EditorPalette, EditorSettings, Settings, USER_SETTINGS_TEMPLATE,
    settings::RenderWhitespace, settings_schema, theme::EXTENSIONS,
};
use kiln_core::theme::{ThemeData, ThemeFile};

#[test]
fn whitespace_shows_in_selections_and_there_are_no_rulers_by_default() {
    let settings = EditorSettings::default();
    assert_eq!(settings.render_whitespace, RenderWhitespace::Selection);
    assert_eq!(settings.rulers, Vec::<i32>::new());
}

#[test]
fn every_value_and_a_rulers_list_parse() {
    for (name, value) in [
        ("none", RenderWhitespace::None),
        ("boundary", RenderWhitespace::Boundary),
        ("selection", RenderWhitespace::Selection),
        ("trailing", RenderWhitespace::Trailing),
        ("all", RenderWhitespace::All),
    ] {
        let source = format!("[editor]\nrender_whitespace = \"{name}\"\n");
        let settings: Settings = toml::from_str(&source).unwrap();
        assert_eq!(settings.editor.render_whitespace, value, "{name}");
    }
    let settings: Settings = toml::from_str("[editor]\nrulers = [80, 120]\n").unwrap();
    assert_eq!(settings.editor.rulers, [80, 120]);
    assert!(toml::from_str::<Settings>("[editor]\nrender_whitespace = \"some\"\n").is_err());
}

#[test]
fn the_template_and_schema_list_both() {
    assert!(USER_SETTINGS_TEMPLATE.contains("# render_whitespace = \"selection\""));
    assert!(USER_SETTINGS_TEMPLATE.contains("# rulers = []"));
    let schema = settings_schema().to_string();
    for word in ["render_whitespace", "rulers"] {
        assert!(schema.contains(word), "the schema lists {word}");
    }
    for value in ["none", "boundary", "selection", "trailing", "all"] {
        assert!(schema.contains(&format!("\"{value}\"")), "{value}");
    }
}

fn parse(source: &str) -> ThemeFile {
    let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
    assert_eq!(diagnostics, []);
    file.unwrap()
}

#[test]
fn every_bundled_theme_sets_the_whitespace_and_ruler_colours() {
    for key in ["whitespace", "ruler"] {
        assert!(EditorColors::KEYS.contains(&key), "{key} is a colour key");
    }
    for (name, source) in [
        ("dark", include_str!("../themes/tenmoku-dark.toml")),
        ("light", include_str!("../themes/tenmoku-light.toml")),
        ("ember", include_str!("../themes/tenmoku-ember.toml")),
        ("2026 dark", include_str!("../themes/vscode-2026-dark.toml")),
        (
            "2026 light",
            include_str!("../themes/vscode-2026-light.toml"),
        ),
        (
            "dark modern",
            include_str!("../themes/vscode-dark-modern.toml"),
        ),
    ] {
        let file = parse(source);
        let editor = &file.extensions["editor"];
        let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&file));
        assert_eq!(warnings, Vec::<String>::new());
        for key in ["whitespace", "ruler"] {
            let set: Color = editor[key].as_str().unwrap().parse().unwrap();
            assert_eq!(palette.editor.get(key), Some(set), "{name} sets {key}");
        }
        let background = palette.editor.background;
        assert_ne!(palette.editor.whitespace, background, "{name}");
        assert_ne!(palette.editor.ruler, background, "{name}");
    }
}
