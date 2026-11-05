//! `workbench.icon_theme`: the coloured set by default, and the user's, so a
//! project file can't set it.

use std::fs;

use fe_config::{
    Settings, SettingsModel, SettingsStore, Severity, USER_SETTINGS_TEMPLATE,
    settings::{IconTheme, PROJECT_ICON_THEME},
    settings_schema,
};

#[test]
fn default_is_tenmoku_and_project_file_ignored() {
    assert_eq!(Settings::default().workbench.icon_theme, IconTheme::Tenmoku);
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let path = dir.path().join(name);
        fs::write(&path, text).unwrap();
        path
    };

    let minimal = "[workbench]\nicon_theme = \"minimal\"\n";
    let store = SettingsStore::load(Some(write("user.toml", minimal)), None);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().workbench.icon_theme, IconTheme::Minimal);

    let user = write("user.toml", "[workbench]\nicon_theme = \"neon\"\n");
    let store = SettingsStore::load(Some(user), None);
    let found: Vec<_> = (store.diagnostics().iter())
        .map(|d| (d.severity, d.key.as_deref(), d.line))
        .collect();
    assert_eq!(
        found,
        [(Severity::Error, Some("workbench.icon_theme"), 2)],
        "{:#?}",
        store.diagnostics()
    );
    assert_eq!(store.settings().workbench.icon_theme, IconTheme::Tenmoku);

    // A project's value is dropped, with a warning, its other keys kept.
    let project = "[workbench]\npreview_tabs = false\nicon_theme = \"minimal\"\n";
    let store = SettingsStore::load(None, Some(write("project.toml", project)));
    assert_eq!(store.settings().workbench.icon_theme, IconTheme::Tenmoku);
    assert!(!store.settings().workbench.preview_tabs);
    let found: Vec<_> = (store.diagnostics().iter())
        .map(|d| (d.severity, d.key.as_deref(), d.line, d.message.as_str()))
        .collect();
    assert_eq!(
        found,
        [(
            Severity::Warning,
            Some("workbench.icon_theme"),
            3,
            PROJECT_ICON_THEME
        )]
    );

    // A project's value doesn't override the user's either.
    let user = write("user.toml", "[workbench]\nicon_theme = \"minimal\"\n");
    let project = write("project.toml", "[workbench]\nicon_theme = \"tenmoku\"\n");
    let store = SettingsStore::load(Some(user), Some(project));
    assert_eq!(store.settings().workbench.icon_theme, IconTheme::Minimal);

    // The Settings tab won't write one there, and reads the same way.
    let model = SettingsModel::new(None, Some("[workbench]\nicon_theme = \"minimal\"\n"));
    let entry = (model.entries().iter())
        .find(|entry| entry.key_path() == "workbench.icon_theme")
        .expect("listed");
    assert!(!model.workspace_allowed(entry));
    assert_eq!(model.value(entry).value, "tenmoku");

    // Documented in the template and the schema.
    assert!(USER_SETTINGS_TEMPLATE.contains("\n# icon_theme = \"tenmoku\"\n"));
    let schema = settings_schema();
    let prop = &schema["$defs"]["WorkbenchSettings"]["properties"]["icon_theme"];
    assert_eq!(prop["default"], "tenmoku");
    assert!(
        (prop["description"].as_str()).is_some_and(|text| text.contains("A project file")),
        "{prop:#}"
    );
    let values = (schema["$defs"]["IconTheme"]["oneOf"].as_array())
        .expect("its values")
        .iter()
        .map(|value| value["const"].as_str().expect("a value"))
        .collect::<Vec<_>>();
    assert_eq!(values, ["tenmoku", "minimal"]);
}
