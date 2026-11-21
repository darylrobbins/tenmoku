//! `terminal.history_suggestions` (on): the faint rest of a command at the prompt.

use fe_config::{Settings, SettingsStore};

#[test]
fn terminal_history_suggestions_defaults_true() {
    assert!(Settings::default().terminal.history_suggestions);

    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, "[terminal]\nhistory_suggestions = false\n").unwrap();
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(store.diagnostics(), []);
    assert!(!store.settings().terminal.history_suggestions);
}
