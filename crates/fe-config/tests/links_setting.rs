//! `editor.links`.

use fe_config::SettingsStore;
use fe_config::settings::EditorSettings;

#[test]
fn links_default_on_and_parse() {
    assert!(EditorSettings::default().links);

    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, "[editor]\nlinks = false\n").unwrap();
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(store.diagnostics(), []);
    assert!(!store.settings().editor.links);
}

#[test]
fn links_in_the_template_and_schema() {
    let template = include_str!("../settings-template.toml");
    assert!(template.contains("\n# links = true\n"));
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../settings.schema.json")).unwrap();
    let links = &schema["$defs"]["EditorSettings"]["properties"]["links"];
    assert_eq!(links["type"], "boolean");
    assert_eq!(links["default"], true);
}
