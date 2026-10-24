//! `extensions.disabled`: the installed extensions that contribute nothing,
//! none by default, and the user's, so a project file can't set it
//! (extensions record §10).

use std::fs;

use fe_config::{
    PROJECT_EXTENSIONS, Settings, SettingsStore, Severity, USER_SETTINGS_TEMPLATE, settings_schema,
};

#[test]
fn disabled_key() {
    assert_eq!(
        Settings::default().extensions.disabled,
        Vec::<String>::new()
    );
    let dir = tempfile::tempdir().unwrap();

    let user = dir.path().join("settings.toml");
    fs::write(&user, "[extensions]\ndisabled = [\"acme.solarized\"]\n").unwrap();
    let store = SettingsStore::load(Some(user.clone()), None);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().extensions.disabled, ["acme.solarized"]);

    let project = dir.path().join("project.toml");
    fs::write(&project, "[extensions]\ndisabled = []\n").unwrap();
    let store = SettingsStore::load(Some(user), Some(project));
    assert_eq!(store.settings().extensions.disabled, ["acme.solarized"]);
    let found: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| (d.severity, d.key.as_deref(), d.line, d.message.as_str()))
        .collect();
    assert_eq!(
        found,
        [(
            Severity::Warning,
            Some("extensions.disabled"),
            2,
            PROJECT_EXTENSIONS
        )]
    );

    // A project file alone can't disable one either.
    let project = dir.path().join("only.toml");
    fs::write(&project, "[extensions]\ndisabled = [\"acme.zig\"]\n").unwrap();
    let store = SettingsStore::load(None, Some(project));
    assert!(store.settings().extensions.disabled.is_empty());

    // Documented in the template and the schema.
    let (_, section) = USER_SETTINGS_TEMPLATE.split_once("[extensions]\n").unwrap();
    assert!(section.contains("# disabled = []"), "{section}");
    let schema = settings_schema();
    let disabled = &schema["$defs"]["ExtensionsSettings"]["properties"]["disabled"];
    assert_eq!(disabled["type"], "array");
    assert_eq!(disabled["items"]["type"], "string");
    assert_eq!(disabled["default"], serde_json::json!([]));
    assert!(
        (disabled["description"].as_str()).is_some_and(|text| text.contains("contribute nothing")),
        "{disabled:#}"
    );
}
