//! `editor.cursor_line`: true by default, a bool.

use std::path::PathBuf;

use fe_config::{Settings, SettingsStore, Severity, USER_SETTINGS_TEMPLATE, settings_schema};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn editor_cursor_line_setting() {
    assert!(Settings::default().editor.cursor_line);

    let store = load("[editor]\ncursor_line = false\n");
    assert_eq!(store.diagnostics(), []);
    assert!(!store.settings().editor.cursor_line);

    for bad in ["\"x\"", "1", "2.5"] {
        let store = load(&format!("[editor]\ncursor_line = {bad}\n"));
        let diagnostics = store.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{bad}: {diagnostics:?}");
        assert_eq!(diagnostics[0].severity, Severity::Error, "{bad}");
        // Located at the `cursor_line` value.
        assert_eq!(
            diagnostics[0].key.as_deref(),
            Some("editor.cursor_line"),
            "{bad}"
        );
        assert_eq!(
            (diagnostics[0].line, diagnostics[0].column),
            (2, 15),
            "{bad}"
        );
        assert!(store.settings().editor.cursor_line, "{bad}");
    }

    assert!(USER_SETTINGS_TEMPLATE.contains("\n# cursor_line = true\n"));
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
    let property = &schema["$defs"]["EditorSettings"]["properties"]["cursor_line"];
    assert_eq!(property["default"], true);
    assert_eq!(property["type"], "boolean");
}
