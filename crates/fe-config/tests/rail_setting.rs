//! `workbench.rail`: which edge of the window the activity rail, and the
//! sidebar beside it, are on; the left by default.

use std::fs;

use fe_config::{
    Settings, SettingsStore, Severity, USER_SETTINGS_TEMPLATE, settings::RailSide, settings_schema,
};

/// Settings from a user file of `user` and a project file of `project`.
fn load(user: &str, project: Option<&str>) -> (SettingsStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let user_path = dir.path().join("settings.toml");
    fs::write(&user_path, user).unwrap();
    let project = project.map(|text| {
        let path = dir.path().join("project.toml");
        fs::write(&path, text).unwrap();
        path
    });
    (SettingsStore::load(Some(user_path), project), dir)
}

#[test]
fn rail_side_parses_and_defaults_left() {
    assert_eq!(Settings::default().workbench.rail, RailSide::Left);
    let (store, _dir) = load("", None);
    assert_eq!(store.settings().workbench.rail, RailSide::Left);

    for (text, side) in [("left", RailSide::Left), ("right", RailSide::Right)] {
        let (store, _dir) = load(&format!("[workbench]\nrail = \"{text}\"\n"), None);
        assert_eq!(store.diagnostics(), [], "{text}");
        assert_eq!(store.settings().workbench.rail, side, "{text}");
    }

    // A project file sets it too, over the user's.
    let (store, _dir) = load(
        "[workbench]\nrail = \"left\"\n",
        Some("[workbench]\nrail = \"right\"\n"),
    );
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().workbench.rail, RailSide::Right);

    // Neither side: the schema rejects it, and the default stays.
    let (store, _dir) = load("[workbench]\nrail = \"bottom\"\n", None);
    let found: Vec<_> = (store.diagnostics().iter())
        .map(|d| (d.severity, d.key.as_deref(), d.line))
        .collect();
    assert_eq!(
        found,
        [(Severity::Error, Some("workbench.rail"), 2)],
        "{:#?}",
        store.diagnostics()
    );
    assert_eq!(store.settings().workbench.rail, RailSide::Left);

    // Documented in the template and the schema, the committed one too.
    assert!(USER_SETTINGS_TEMPLATE.contains("\n# rail = \"left\"\n"));
    let schema = settings_schema();
    let prop = &schema["$defs"]["WorkbenchSettings"]["properties"]["rail"];
    assert_eq!(prop["default"], "left", "{prop:#}");
    assert!(
        (prop["description"].as_str()).is_some_and(|text| text.contains("activity rail")),
        "{prop:#}"
    );
    let committed: serde_json::Value =
        serde_json::from_str(include_str!("../settings.schema.json")).unwrap();
    assert_eq!(
        committed["$defs"]["WorkbenchSettings"]["properties"]["rail"],
        *prop
    );
    assert_eq!(
        committed["properties"]["workbench"]["default"]["rail"],
        "left"
    );
}
