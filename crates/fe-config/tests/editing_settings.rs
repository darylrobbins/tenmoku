//! `editor.format_on_save` and `editor.auto_close_pairs`.

use fe_config::{Settings, SettingsStore};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn defaults_and_parse() {
    let defaults = Settings::default();
    assert!(!defaults.editor.format_on_save);
    assert!(defaults.editor.auto_close_pairs);

    let store = load("[editor]\nformat_on_save = true\nauto_close_pairs = false\n");
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().editor.format_on_save);
    assert!(!store.settings().editor.auto_close_pairs);

    for key in ["format_on_save", "auto_close_pairs"] {
        let store = load(&format!("[editor]\n{key} = \"yes\"\n"));
        assert_eq!(store.diagnostics().len(), 1, "{key}");
        assert_eq!(store.settings().editor, defaults.editor, "{key}");
    }
}
