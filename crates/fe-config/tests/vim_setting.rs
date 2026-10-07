//! The `[vim]` table: `enabled`, off by default.

use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE};

/// The store for a user settings file holding `contents`.
fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn vim_setting() {
    assert!(!Settings::default().vim.enabled);

    let store = load("[vim]\nenabled = true\n");
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().vim.enabled);
    assert_eq!(store.settings().editor, Settings::default().editor);

    let store = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());
}
