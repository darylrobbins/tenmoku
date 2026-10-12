//! `vim.jump_on_s`: off by default.

use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn vim_jump_on_s_setting() {
    assert!(!Settings::default().vim.jump_on_s);

    let store = load("[vim]\njump_on_s = true\n");
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().vim.jump_on_s);

    let store = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());
}
