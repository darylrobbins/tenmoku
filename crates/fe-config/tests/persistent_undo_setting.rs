//! `editor.persistent_undo`: on by default, parsed, documented.

use fe_config::settings::EditorSettings;
use fe_config::{SettingsStore, USER_SETTINGS_TEMPLATE, settings_schema};

fn load(body: &str) -> (SettingsStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, body).unwrap();
    (SettingsStore::load(Some(user), None), dir)
}

#[test]
fn persistent_undo_defaults_to_true_and_parses() {
    assert!(EditorSettings::default().persistent_undo);
    let (store, _dir) = load("");
    assert!(store.settings().editor.persistent_undo);

    let (store, _dir) = load("[editor]\npersistent_undo = false\n");
    assert_eq!(store.diagnostics(), []);
    assert!(!store.settings().editor.persistent_undo);

    let (store, _dir) = load("[editor]\npersistent_undo = 1\n");
    assert!(!store.diagnostics().is_empty(), "a non-bool is reported");
    assert!(
        store.settings().editor.persistent_undo,
        "and the default kept"
    );

    let schema = settings_schema();
    let property = &schema["$defs"]["EditorSettings"]["properties"]["persistent_undo"];
    assert_eq!(property["type"], "boolean");
    assert_eq!(property["default"], true);
    assert!(USER_SETTINGS_TEMPLATE.contains("\n# persistent_undo = true\n"));
}
