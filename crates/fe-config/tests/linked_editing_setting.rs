//! `editor.linked_editing`: off by default, parsed, documented.

use fe_config::settings::EditorSettings;
use fe_config::{SettingsStore, USER_SETTINGS_TEMPLATE, settings_schema};

fn load(body: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, body).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn default_is_false() {
    assert!(!EditorSettings::default().linked_editing);
    let store = load("");
    assert!(!store.settings().editor.linked_editing);
}

#[test]
fn parses_true() {
    let store = load("[editor]\nlinked_editing = true\n");
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().editor.linked_editing);
}

#[test]
fn appears_in_the_schema_and_template() {
    let schema = settings_schema();
    let property = &schema["$defs"]["EditorSettings"]["properties"]["linked_editing"];
    assert_eq!(property["type"], "boolean");
    assert_eq!(property["default"], false);
    assert!(USER_SETTINGS_TEMPLATE.contains("# linked_editing = false"));
}
