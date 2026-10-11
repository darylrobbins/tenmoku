//! `theme.mode`: dark by default, and the app's, so a project file can't
//! set it.

use std::fs;

use fe_config::{Settings, SettingsStore, Severity, ThemeModeSetting, settings::PROJECT_THEME};

fn user_file(dir: &tempfile::TempDir, contents: &str) -> std::path::PathBuf {
    let user = dir.path().join("settings.toml");
    fs::write(&user, contents).unwrap();
    user
}

#[test]
fn theme_mode_key() {
    assert_eq!(Settings::default().theme.mode, ThemeModeSetting::Dark);
    let dir = tempfile::tempdir().unwrap();

    let user = user_file(&dir, "[theme]\nmode = \"system\"\n");
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().theme.mode, ThemeModeSetting::System);

    let user = user_file(&dir, "[theme]\nmode = \"blue\"\n");
    let store = SettingsStore::load(Some(user), None);
    let found: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| (d.severity, d.line))
        .collect();
    assert_eq!(found, [(Severity::Error, 2)], "{:#?}", store.diagnostics());
    assert_eq!(store.settings().theme.mode, ThemeModeSetting::Dark);
}

#[test]
fn project_file_cannot_set_the_theme() {
    let dir = tempfile::tempdir().unwrap();
    let user = user_file(&dir, "[theme]\nmode = \"system\"\n");
    let project = dir.path().join("project.toml");
    fs::write(&project, "[theme]\nmode = \"light\"\n").unwrap();
    let store = SettingsStore::load(Some(user), Some(project));
    assert_eq!(store.settings().theme.mode, ThemeModeSetting::System);
    let found: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| (d.severity, d.key.as_deref(), d.line, d.message.as_str()))
        .collect();
    assert_eq!(
        found,
        [(Severity::Warning, Some("theme.mode"), 2, PROJECT_THEME)]
    );
}
