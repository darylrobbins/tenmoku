//! `lsp.servers.<name>.add_on`: parsed, in the schema and template, and
//! ignored from an untrusted project file.

use std::fs;

use fe_config::settings::{UNTRUSTED_LSP, trusted_lsp};
use fe_config::{SettingsStore, USER_SETTINGS_TEMPLATE, settings_schema};

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
fn add_on_setting_parses() {
    let (_dir, store) = load(
        "[lsp.servers.a]\nadd_on = true\n[lsp.servers.b]\nadd_on = false\n[lsp.servers.c]\ncommand = \"c\"\n",
        None,
    );
    assert_eq!(store.diagnostics(), []);
    let servers = &store.settings().lsp.servers;
    assert_eq!(servers["a"].add_on, Some(true));
    assert_eq!(servers["b"].add_on, Some(false));
    assert_eq!(servers["c"].add_on, None);
}

#[test]
fn add_on_in_schema_and_template() {
    let schema = settings_schema();
    let property = &schema["$defs"]["LspServerSettings"]["properties"]["add_on"];
    assert!(property.is_object(), "{property}");
    assert!(
        USER_SETTINGS_TEMPLATE.contains("# add_on = true"),
        "the template documents add_on"
    );
}

#[test]
fn add_on_in_project_layer_is_filtered() {
    let user = "[lsp.servers.pyright]\ncommand = \"pyright-langserver\"\n";
    let project = "[lsp.servers.pyright]\nadd_on = true\n";
    let (dir, store) = load(user, Some(project));
    assert_eq!(store.settings().lsp.servers["pyright"].add_on, None);
    let found: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| (d.key.as_deref(), d.message.as_str()))
        .collect();
    assert_eq!(found, [(Some("lsp.servers.pyright.add_on"), UNTRUSTED_LSP)]);
    // A trusted root applies it.
    let lsp = trusted_lsp(
        Some(dir.path().join("user.toml")),
        Some(dir.path().join("project.toml")),
    );
    assert_eq!(lsp.servers["pyright"].add_on, Some(true));
}
