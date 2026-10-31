//! `vim.inccommand`: true by default, a bool.

use std::path::PathBuf;

use fe_config::{Settings, SettingsStore, Severity, USER_SETTINGS_TEMPLATE, settings_schema};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn vim_inccommand_setting() {
    assert!(Settings::default().vim.inccommand);

    let store = load("[vim]\ninccommand = false\n");
    assert_eq!(store.diagnostics(), []);
    assert!(!store.settings().vim.inccommand);

    for bad in ["\"nosplit\"", "1"] {
        let store = load(&format!("[vim]\ninccommand = {bad}\n"));
        let diagnostics = store.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{bad}: {diagnostics:?}");
        assert_eq!(diagnostics[0].severity, Severity::Error, "{bad}");
        assert_eq!(
            diagnostics[0].key.as_deref(),
            Some("vim.inccommand"),
            "{bad}"
        );
        assert!(store.settings().vim.inccommand, "{bad}");
    }

    assert!(USER_SETTINGS_TEMPLATE.contains("\n# inccommand = true\n"));
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
    let property = &schema["$defs"]["VimSettings"]["properties"]["inccommand"];
    assert_eq!(property["default"], true);
    assert_eq!(property["type"], "boolean");
}
