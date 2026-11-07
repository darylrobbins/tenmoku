//! `editor.format_on_type` and `editor.format_on_paste`: off by default,
//! parsed, documented.

use fe_config::settings::EditorSettings;
use fe_config::{SettingsStore, USER_SETTINGS_TEMPLATE, settings_schema};

fn load(body: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, body).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn both_default_off_and_parse() {
    let defaults = EditorSettings::default();
    assert!(!defaults.format_on_type);
    assert!(!defaults.format_on_paste);
    let store = load("");
    assert!(!store.settings().editor.format_on_type);
    assert!(!store.settings().editor.format_on_paste);

    let store = load("[editor]\nformat_on_type = true\nformat_on_paste = true\n");
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().editor.format_on_type);
    assert!(store.settings().editor.format_on_paste);

    // Each on its own, and a wrong type is a diagnostic that leaves both off.
    let store = load("[editor]\nformat_on_paste = true\n");
    assert!(!store.settings().editor.format_on_type);
    assert!(store.settings().editor.format_on_paste);
    for key in ["format_on_type", "format_on_paste"] {
        let store = load(&format!("[editor]\n{key} = \"yes\"\n"));
        assert_eq!(store.diagnostics().len(), 1, "{key}");
        assert_eq!(store.settings().editor, defaults, "{key}");
    }

    let schema = settings_schema();
    let committed: serde_json::Value =
        serde_json::from_str(include_str!("../settings.schema.json")).unwrap();
    for key in ["format_on_type", "format_on_paste"] {
        for schema in [&schema, &committed] {
            let property = &schema["$defs"]["EditorSettings"]["properties"][key];
            assert_eq!(property["type"], "boolean", "{key}");
            assert_eq!(property["default"], false, "{key}");
        }
        assert!(
            USER_SETTINGS_TEMPLATE.contains(&format!("\n# {key} = false\n")),
            "{key}"
        );
    }
}
