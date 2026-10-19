//! `editor.inline_blame`.

use fe_config::SettingsStore;
use fe_config::settings::EditorSettings;

#[test]
fn inline_blame_default_off_and_parse() {
    assert!(!EditorSettings::default().inline_blame);

    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, "[editor]\ninline_blame = true\n").unwrap();
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().editor.inline_blame);
}
