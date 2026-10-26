//! `editor.line_numbers`: `"on"` by default, or `"off"`, `"relative"` or
//! `"hybrid"`.

use std::path::PathBuf;

use fe_config::settings::LineNumbersSetting;
use fe_config::{Settings, SettingsStore, Severity, USER_SETTINGS_TEMPLATE, settings_schema};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn editor_line_numbers_setting() {
    assert_eq!(
        Settings::default().editor.line_numbers,
        LineNumbersSetting::On
    );

    for (name, want) in [
        ("on", LineNumbersSetting::On),
        ("off", LineNumbersSetting::Off),
        ("relative", LineNumbersSetting::Relative),
        ("hybrid", LineNumbersSetting::Hybrid),
    ] {
        let store = load(&format!("[editor]\nline_numbers = \"{name}\"\n"));
        assert_eq!(store.diagnostics(), [], "{name}");
        assert_eq!(store.settings().editor.line_numbers, want, "{name}");
    }

    let store = load("[editor]\ntab_size = 2\nline_numbers = \"absolute\"\n");
    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    // Located at the `line_numbers` value.
    assert_eq!((diagnostics[0].line, diagnostics[0].column), (3, 16));
    assert_eq!(store.settings().editor.line_numbers, LineNumbersSetting::On);

    assert!(USER_SETTINGS_TEMPLATE.contains("\n# line_numbers = \"on\"\n"));
    let store = load(USER_SETTINGS_TEMPLATE);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(**store.settings(), Settings::default());

    // The committed schema is the settings' (read at run time: a shared
    // target can hold a test binary built for another checkout), and lists
    // the four values with the default.
    let path =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("settings.schema.json");
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let schema = settings_schema();
    assert_eq!(
        golden, schema,
        "settings.schema.json is out of date: rerun schema_matches_golden with UPDATE_GOLDEN=1"
    );
    let property = &schema["$defs"]["EditorSettings"]["properties"]["line_numbers"];
    assert_eq!(property["default"], "on");
    let values = schema["$defs"]["LineNumbersSetting"].to_string();
    for value in ["on", "off", "relative", "hybrid"] {
        assert!(
            values.contains(&format!("\"{value}\"")),
            "{value}: {values}"
        );
    }
}
