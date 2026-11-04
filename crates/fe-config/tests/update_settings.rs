//! `[update]`: the background update check's off switch.

use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE, settings_schema};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn update_defaults_to_automatic_checks() {
    assert!(Settings::default().update.automatic_checks);
    let store = load("");
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().update.automatic_checks);
    let store = load("[update]\n");
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().update.automatic_checks);
}

#[test]
fn update_automatic_checks_false_parses() {
    let store = load("[update]\nautomatic_checks = false\n");
    assert_eq!(store.diagnostics(), []);
    assert!(!store.settings().update.automatic_checks);
}

#[test]
fn unknown_update_key_is_reported() {
    let store = load("[update]\nautomatic_chekcs = false\n");
    assert!(!store.diagnostics().is_empty());
    assert!(store.settings().update.automatic_checks);
}

#[test]
fn the_template_and_the_schema_list_it() {
    assert!(USER_SETTINGS_TEMPLATE.contains("[update]\n"));
    assert!(USER_SETTINGS_TEMPLATE.contains("# automatic_checks = true\n"));
    let schema = settings_schema();
    let key = &schema["$defs"]["UpdateSettings"]["properties"]["automatic_checks"];
    assert_eq!(key["type"], "boolean");
    assert_eq!(key["default"], true);
    assert!(schema["properties"]["update"].is_object());
}
