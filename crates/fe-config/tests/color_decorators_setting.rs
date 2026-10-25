//! `editor.color_decorators`.

use fe_config::SettingsStore;
use fe_config::settings::EditorSettings;

#[test]
fn color_decorators_default_on_and_parse() {
    assert!(EditorSettings::default().color_decorators);

    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, "[editor]\ncolor_decorators = false\n").unwrap();
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(store.diagnostics(), []);
    assert!(!store.settings().editor.color_decorators);
}

#[test]
fn color_decorators_in_the_template_and_schema() {
    let template = include_str!("../settings-template.toml");
    assert!(template.contains("\n# color_decorators = true\n"));
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../settings.schema.json")).unwrap();
    let setting = &schema["$defs"]["EditorSettings"]["properties"]["color_decorators"];
    assert_eq!(setting["type"], "boolean");
    assert_eq!(setting["default"], true);
}
