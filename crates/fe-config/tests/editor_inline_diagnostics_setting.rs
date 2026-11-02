//! `editor.inline_diagnostics`: `warning` by default; `off`, `error`,
//! `warning` or `all`.

use std::path::PathBuf;

use fe_config::{
    Settings, SettingsStore, Severity, USER_SETTINGS_TEMPLATE, settings::InlineDiagnostics,
    settings_schema,
};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn editor_inline_diagnostics_setting() {
    assert_eq!(
        Settings::default().editor.inline_diagnostics,
        InlineDiagnostics::Warning
    );

    for (value, parsed) in [
        ("off", InlineDiagnostics::Off),
        ("error", InlineDiagnostics::Error),
        ("warning", InlineDiagnostics::Warning),
        ("all", InlineDiagnostics::All),
    ] {
        let store = load(&format!("[editor]\ninline_diagnostics = \"{value}\"\n"));
        assert_eq!(store.diagnostics(), [], "{value}");
        assert_eq!(store.settings().editor.inline_diagnostics, parsed);
    }

    // An error at the value, the default kept.
    for bad in ["\"hint\"", "\"Warning\"", "true", "1"] {
        let store = load(&format!("[editor]\ninline_diagnostics = {bad}\n"));
        let diagnostics = store.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{bad}: {diagnostics:?}");
        assert_eq!(diagnostics[0].severity, Severity::Error, "{bad}");
        assert_eq!(
            store.settings().editor.inline_diagnostics,
            InlineDiagnostics::Warning,
            "{bad}"
        );
    }

    assert!(USER_SETTINGS_TEMPLATE.contains("\n# inline_diagnostics = \"warning\"\n"));
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
    let property = &schema["$defs"]["EditorSettings"]["properties"]["inline_diagnostics"];
    assert_eq!(property["default"], "warning");
}
