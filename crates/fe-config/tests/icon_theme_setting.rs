//! `workbench.icon_theme`: the tinted line icons by default, Material's
//! coloured set or plain line icons by choice, and the user's, so a project
//! file can't set it; and the bundled themes' `file_icon` tints.

use std::fs;

use fe_config::{
    Settings, SettingsModel, SettingsStore, Severity, USER_SETTINGS_TEMPLATE,
    settings::{IconTheme, PROJECT_ICON_THEME},
    settings_schema, themes,
};
use kiln_core::theme::{TOKEN_KEYS, ThemeData};

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
    assert_eq!(values, ["tenmoku", "material", "minimal"]);
}

#[test]
fn material_and_minimal_parse() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let path = dir.path().join(name);
        fs::write(&path, text).unwrap();
        path
    };
    for (value, theme) in [
        ("tenmoku", IconTheme::Tenmoku),
        ("material", IconTheme::Material),
        ("minimal", IconTheme::Minimal),
    ] {
        let user = format!("[workbench]\nicon_theme = \"{value}\"\n");
        let store = SettingsStore::load(Some(write("user.toml", &user)), None);
        assert_eq!(store.diagnostics(), [], "{value}");
        assert_eq!(store.settings().workbench.icon_theme, theme, "{value}");

        // A project's value is still dropped, with a warning.
        let project = write("project.toml", &user);
        let store = SettingsStore::load(None, Some(project));
        assert_eq!(store.settings().workbench.icon_theme, IconTheme::Tenmoku);
        let found: Vec<_> = (store.diagnostics().iter())
            .map(|d| (d.severity, d.key.as_deref(), d.message.as_str()))
            .collect();
        assert_eq!(
            found,
            [(
                Severity::Warning,
                Some("workbench.icon_theme"),
                PROJECT_ICON_THEME
            )],
            "{value}"
        );
    }
}

/// Every `file_icon` token's colour in the bundled theme `id`.
fn file_icon_tints(id: &str) -> Vec<(&'static str, String)> {
    let theme = ThemeData::from_file(&themes::find(id).unwrap().file());
    (TOKEN_KEYS.iter())
        .filter(|key| key.starts_with("file_icon."))
        .map(|&key| (key, theme.tokens.get(key).unwrap().to_string()))
        .collect()
}

#[test]
fn hakuji_folders_are_one_neutral_grey() {
    let hakuji = file_icon_tints("hakuji");
    let folders: Vec<&str> = (hakuji.iter())
        .filter(|(key, _)| key.starts_with("file_icon.folder"))
        .map(|(_, color)| color.as_str())
        .collect();
    assert_eq!(folders.len(), 4, "{hakuji:?}");
    // One grey, Hakuji's muted text, as its list.folder_icon is.
    assert!(
        folders.iter().all(|color| *color == "#4b525b"),
        "{hakuji:?}"
    );
    let theme = ThemeData::from_file(&themes::find("hakuji").unwrap().file());
    assert_eq!(theme.tokens.list.folder_icon.to_string(), "#4b525b");
    assert_eq!(theme.tokens.text.muted.to_string(), "#4b525b");

    // Yohen's folders tint by role: source, tests and docs apart from the
    // rest.
    let yohen = file_icon_tints("yohen");
    let tint = |key: &str| &yohen.iter().find(|(k, _)| *k == key).unwrap().1;
    let roles = ["folder_source", "folder_tests", "folder_docs", "folder"]
        .map(|role| tint(&format!("file_icon.{role}")).clone());
    for (i, a) in roles.iter().enumerate() {
        for b in &roles[i + 1..] {
            assert_ne!(a, b, "{yohen:?}");
        }
    }
    // The design's file types in both: Rust, TOML and Markdown apart, the
    // Markdown icon in lapis.
    for (theme, lapis) in [(yohen.clone(), "#8db1f2"), (hakuji.clone(), "#2457c5")] {
        let tint = |key: &str| &theme.iter().find(|(k, _)| *k == key).unwrap().1;
        assert_eq!(tint("file_icon.markdown"), lapis);
        assert_ne!(tint("file_icon.rust"), tint("file_icon.toml"));
    }
}
