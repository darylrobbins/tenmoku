//! `extensions.plugins_preview`: code plugins run only when it is on, off by
//! default, and the user's, so a project file can't set it.

use std::fs;

use fe_config::{
    PROJECT_EXTENSIONS, Settings, SettingsStore, Severity, USER_SETTINGS_TEMPLATE, settings_schema,
};

#[test]
fn plugins_preview_key() {
    assert!(!Settings::default().extensions.plugins_preview);
    let dir = tempfile::tempdir().unwrap();

    let user = dir.path().join("settings.toml");
    fs::write(&user, "[extensions]\nplugins_preview = true\n").unwrap();
    let store = SettingsStore::load(Some(user), None);
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().extensions.plugins_preview);

    // A project file can't turn it on.
    let project = dir.path().join("project.toml");
    fs::write(&project, "[extensions]\nplugins_preview = true\n").unwrap();
    let store = SettingsStore::load(None, Some(project));
    assert!(!store.settings().extensions.plugins_preview);
    let found: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| (d.severity, d.key.as_deref(), d.line, d.message.as_str()))
        .collect();
    assert_eq!(
        found,
        [(
            Severity::Warning,
            Some("extensions.plugins_preview"),
            2,
            PROJECT_EXTENSIONS
        )]
    );

    // Documented in the template and the schema.
    let (_, section) = USER_SETTINGS_TEMPLATE.split_once("[extensions]\n").unwrap();
    assert!(section.contains("# plugins_preview = false"), "{section}");
    let schema = settings_schema();
    let prop = &schema["$defs"]["ExtensionsSettings"]["properties"]["plugins_preview"];
    assert_eq!(prop["type"], "boolean");
    assert_eq!(prop["default"], false);
    assert!(
        (prop["description"].as_str()).is_some_and(|text| text.contains("A preview")),
        "{prop:#}"
    );
}
