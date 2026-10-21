use kiln_core::theme::ThemeData;

use crate::{EditorPalette, Settings, SettingsStore, themes};

use super::write;

#[test]
fn on_by_default() {
    assert!(Settings::default().editor.bracket_pair_colors);
}

#[test]
fn a_settings_file_turns_it_off() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(
        &dir,
        "settings.toml",
        "[editor]\nbracket_pair_colors = false\n",
    );
    let store = SettingsStore::load(Some(path), None);
    assert!(store.diagnostics().is_empty());
    assert!(!store.settings().editor.bracket_pair_colors);
}

#[test]
fn every_bundled_theme_has_three_distinct_bracket_colors() {
    for theme in themes::bundled_themes() {
        let (palette, warnings) = EditorPalette::for_theme(&ThemeData::from_file(&theme.file()));
        assert_eq!(warnings, Vec::<String>::new(), "{}", theme.id);
        let EditorPalette { editor, .. } = palette;
        let colors = [editor.bracket_1, editor.bracket_2, editor.bracket_3];
        for (i, a) in colors.iter().enumerate() {
            for b in &colors[i + 1..] {
                assert_ne!(a, b, "{}: {colors:?}", theme.id);
            }
        }
    }
}
