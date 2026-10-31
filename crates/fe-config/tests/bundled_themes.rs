//! The bundled themes `[theme] name` selects: found by title or ID, and
//! each a clean theme file of its listed appearance; and `theme.name`, a
//! user file's setting that a project file can't set.

use std::{fs, path::Path};

use fe_config::{
    Settings, SettingsStore, Severity,
    settings::PROJECT_THEME,
    theme::EXTENSIONS,
    themes::{bundled_themes, find},
};
use kiln_core::theme::{Appearance, ThemeFile};

#[test]
fn finds_by_title_or_id() {
    for name in ["Dark Modern", "vscode-dark-modern", " dark modern "] {
        let found = find(name).unwrap_or_else(|| panic!("{name:?} is found"));
        assert_eq!(found.title, "Dark Modern", "{name:?}");
        assert_eq!(found.id, "vscode-dark-modern", "{name:?}");
    }
    let ember = find("Ember").unwrap();
    assert_eq!(ember.id, "tenmoku-ember");
    assert_eq!(find("Tenmoku Ember"), Some(ember));
    assert_eq!(find("TENMOKU-EMBER"), Some(ember));
    assert_eq!(find("dark").map(|theme| theme.id), Some("tenmoku-dark"));
    assert_eq!(find("nope"), None);
    assert_eq!(find(""), None);
}

#[test]
fn every_entry_parses() {
    let themes = bundled_themes();
    let mut titles: Vec<_> = themes.iter().map(|theme| theme.title).collect();
    let mut ids: Vec<_> = themes.iter().map(|theme| theme.id).collect();
    for name in titles.iter().chain(&ids) {
        let found = find(name).unwrap_or_else(|| panic!("{name:?} is found"));
        assert!(found.title == *name || found.id == *name, "{name:?}");
    }
    titles.sort_unstable();
    titles.dedup();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(titles.len(), themes.len(), "titles are unique");
    assert_eq!(ids.len(), themes.len(), "IDs are unique");
    for theme in themes {
        let path = format!("{}.toml", theme.id);
        let (file, diagnostics) = ThemeFile::parse(Path::new(&path), theme.source, EXTENSIONS);
        assert_eq!(diagnostics, [], "{}", theme.id);
        let file = file.unwrap_or_else(|| panic!("{} parses", theme.id));
        assert_eq!(
            file.appearance.unwrap_or_default(),
            theme.appearance,
            "{}",
            theme.id
        );
        assert_eq!(theme.file(), file, "{}", theme.id);
    }
    assert_eq!(find("Tenmoku Light").unwrap().appearance, Appearance::Light);
}

#[test]
fn name_is_the_user_files_alone() {
    assert_eq!(Settings::default().theme.name, None);
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    fs::write(&user, "[theme]\nname = \"Dark Modern\"\n").unwrap();
    let store = SettingsStore::load(Some(user.clone()), None);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().theme.name.as_deref(), Some("Dark Modern"));

    let project = dir.path().join("project.toml");
    fs::write(&project, "[theme]\nname = \"Tenmoku Ember\"\n").unwrap();
    let store = SettingsStore::load(Some(user), Some(project));
    assert_eq!(store.settings().theme.name.as_deref(), Some("Dark Modern"));
    let found: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| (d.severity, d.key.as_deref(), d.line, d.message.as_str()))
        .collect();
    assert_eq!(
        found,
        [(Severity::Warning, Some("theme.name"), 2, PROJECT_THEME)]
    );
}
