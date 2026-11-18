//! `lsp.python_environment`: parsed from the user's file, in the schema,
//! the template and the Settings editor's text, and dropped from an
//! untrusted project file with the untrusted-LSP warning.

use std::fs;

use super::{UNTRUSTED_LSP, trusted_lsp};
use crate::{SettingsStore, USER_SETTINGS_TEMPLATE, settings_schema};

const SET: &str = "[lsp]\npython_environment = \"~/envs/app\"\n";

fn load(user: &str, project: Option<&str>) -> (tempfile::TempDir, SettingsStore) {
    let dir = tempfile::tempdir().unwrap();
    let user_path = dir.path().join("user.toml");
    fs::write(&user_path, user).unwrap();
    let project_path = project.map(|text| {
        let path = dir.path().join("project.toml");
        fs::write(&path, text).unwrap();
        path
    });
    let store = SettingsStore::load(Some(user_path), project_path);
    (dir, store)
}

#[test]
fn the_user_file_sets_it() {
    let (_dir, store) = load(SET, None);
    assert_eq!(store.diagnostics(), []);
    let lsp = &store.settings().lsp;
    assert_eq!(lsp.python_environment.as_deref(), Some("~/envs/app"));
    let (_dir, store) = load("", None);
    assert_eq!(store.settings().lsp.python_environment, None);
}

#[test]
fn in_the_schema_template_and_settings_text() {
    let schema = settings_schema();
    let property = &schema["$defs"]["LspSettings"]["properties"]["python_environment"];
    assert_eq!(property["type"][0], "string", "{property}");
    let description = property["description"].as_str().unwrap();
    assert!(description.contains("A user setting"), "{description}");
    assert!(
        USER_SETTINGS_TEMPLATE.contains("# [lsp]\n# python_environment = "),
        "the template documents it"
    );
    let ftl = include_str!("../l10n/en-US/settings.ftl");
    assert!(ftl.contains("settings-lsp-python_environment =\n    The Python environment"));
}

#[test]
fn a_project_file_s_value_is_dropped() {
    let (dir, store) = load("", Some(SET));
    assert_eq!(store.settings().lsp.python_environment, None);
    let found: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| (d.key.as_deref(), d.message.as_str()))
        .collect();
    assert_eq!(found, [(Some("lsp.python_environment"), UNTRUSTED_LSP)]);

    // In a trusted folder its language servers get it.
    let lsp = trusted_lsp(None, Some(dir.path().join("project.toml")));
    assert_eq!(lsp.python_environment.as_deref(), Some("~/envs/app"));
}
