//! `editor.default_color_decorators`: `auto` by default; `auto`, `always` or
//! `never`.

use fe_config::{
    SettingsStore, Severity, USER_SETTINGS_TEMPLATE,
    settings::{DefaultColorDecorators, EditorSettings},
};

fn load(contents: &str) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, contents).unwrap();
    SettingsStore::load(Some(user), None)
}

#[test]
fn default_is_auto_and_values_parse() {
    assert_eq!(
        EditorSettings::default().default_color_decorators,
        DefaultColorDecorators::Auto
    );

    for (value, parsed) in [
        ("auto", DefaultColorDecorators::Auto),
        ("always", DefaultColorDecorators::Always),
        ("never", DefaultColorDecorators::Never),
    ] {
        let store = load(&format!(
            "[editor]\ndefault_color_decorators = \"{value}\"\n"
        ));
        assert_eq!(store.diagnostics(), [], "{value}");
        assert_eq!(store.settings().editor.default_color_decorators, parsed);
    }

    // An error at the value, the default kept.
    let store = load("[editor]\ndefault_color_decorators = \"sometimes\"\n");
    let diagnostics = store.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert_eq!(
        store.settings().editor.default_color_decorators,
        DefaultColorDecorators::Auto
    );
}

#[test]
fn in_the_template_and_schema() {
    assert!(USER_SETTINGS_TEMPLATE.contains("\n# default_color_decorators = \"auto\"\n"));
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../settings.schema.json")).unwrap();
    let setting = &schema["$defs"]["EditorSettings"]["properties"]["default_color_decorators"];
    assert_eq!(setting["default"], "auto");
    let reference = setting["$ref"].as_str().unwrap();
    let name = reference.strip_prefix("#/$defs/").unwrap();
    let values: Vec<&str> = (schema["$defs"][name]["oneOf"].as_array().unwrap().iter())
        .map(|variant| variant["const"].as_str().unwrap())
        .collect();
    assert_eq!(values, ["auto", "always", "never"]);
}
