//! `workbench.start_page`: the Start page in a window's empty pane, on by
//! default.

use std::fs;

use fe_config::{Settings, SettingsStore, Severity, USER_SETTINGS_TEMPLATE, settings_schema};

fn load(text: &str) -> (SettingsStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("settings.toml");
    fs::write(&user, text).unwrap();
    (SettingsStore::load(Some(user), None), dir)
}

#[test]
fn start_page_defaults_true_and_validates() {
    assert!(Settings::default().workbench.start_page);

    let (store, _dir) = load("[workbench]\nstart_page = false\n");
    assert_eq!(store.diagnostics(), []);
    assert!(!store.settings().workbench.start_page);

    let (store, _dir) = load("[workbench]\nstart_page = true\n");
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().workbench.start_page);

    // A string isn't a bool: the schema rejects it, and the default stays.
    let (store, _dir) = load("[workbench]\nstart_page = \"no\"\n");
    let found: Vec<_> = (store.diagnostics().iter())
        .map(|d| (d.severity, d.key.as_deref(), d.line))
        .collect();
    assert_eq!(
        found,
        [(Severity::Error, Some("workbench.start_page"), 2)],
        "{:#?}",
        store.diagnostics()
    );
    assert!(store.settings().workbench.start_page);

    // Documented in the template and the schema, the committed one too.
    assert!(USER_SETTINGS_TEMPLATE.contains("\n# start_page = true\n"));
    let schema = settings_schema();
    let prop = &schema["$defs"]["WorkbenchSettings"]["properties"]["start_page"];
    assert_eq!(prop["type"], "boolean", "{prop:#}");
    assert_eq!(prop["default"], true, "{prop:#}");
    assert!(
        (prop["description"].as_str()).is_some_and(|text| text.contains("Start page")),
        "{prop:#}"
    );
    let committed: serde_json::Value =
        serde_json::from_str(include_str!("../settings.schema.json")).unwrap();
    assert_eq!(
        committed["$defs"]["WorkbenchSettings"]["properties"]["start_page"],
        *prop
    );
    assert_eq!(
        committed["properties"]["workbench"]["default"]["start_page"],
        true
    );
}
