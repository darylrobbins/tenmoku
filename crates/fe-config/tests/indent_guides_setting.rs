//! `editor.indent_guides`.

use fe_config::SettingsStore;
use fe_config::settings::EditorSettings;

#[test]
fn indent_guides_default_on_and_parse() {
    assert!(EditorSettings::default().indent_guides);

    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, "[editor]\nindent_guides = false\n").unwrap();
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(store.diagnostics(), []);
    assert!(!store.settings().editor.indent_guides);
}
