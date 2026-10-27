//! `editor.cursor_surrounding_lines`: 3 by default, 0 to 100.

use std::path::PathBuf;

use fe_config::{Settings, SettingsStore, Severity, USER_SETTINGS_TEMPLATE, settings_schema};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn editor_scroll_off_setting() {
    assert_eq!(Settings::default().editor.cursor_surrounding_lines, 3);

    for value in [0, 8, 100] {
        let store = load(&format!("[editor]\ncursor_surrounding_lines = {value}\n"));
        assert_eq!(store.diagnostics(), [], "{value}");
        assert_eq!(store.settings().editor.cursor_surrounding_lines, value);
    }

    // Out of range or not a whole number: an error at the value, the default
    // kept.
    for bad in ["101", "-1", "\"x\"", "2.5"] {
        let store = load(&format!("[editor]\ncursor_surrounding_lines = {bad}\n"));
        let diagnostics = store.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{bad}: {diagnostics:?}");
        assert_eq!(diagnostics[0].severity, Severity::Error, "{bad}");
        assert_eq!(store.settings().editor.cursor_surrounding_lines, 3, "{bad}");
    }

    assert!(USER_SETTINGS_TEMPLATE.contains("\n# cursor_surrounding_lines = 3\n"));
    let store = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());

    let path =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("settings.schema.json");
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let schema = settings_schema();
    assert_eq!(
        golden, schema,
        "settings.schema.json is out of date: rerun schema_matches_golden with UPDATE_GOLDEN=1"
    );
    let property = &schema["$defs"]["EditorSettings"]["properties"]["cursor_surrounding_lines"];
    assert_eq!(property["default"], 3);
    assert_eq!(property["minimum"], 0);
    assert_eq!(property["maximum"], 100);
}
