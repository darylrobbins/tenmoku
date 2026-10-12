//! `vim.regex_time_limit`: 2000 ms by default, at least 1.

use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn vim_regex_time_limit_setting() {
    assert_eq!(Settings::default().vim.regex_time_limit, 2000);

    let store = load("[vim]\nregex_time_limit = 500\n");
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().vim.regex_time_limit, 500);

    let store = load("[vim]\nregex_time_limit = 0\n");
    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert!(diagnostics[0].is_error(), "{:?}", diagnostics[0]);
    assert_eq!(store.settings().vim.regex_time_limit, 2000);

    let store = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());
}
