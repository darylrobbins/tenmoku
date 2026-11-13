//! `lsp.servers.<name>.watch_files`, `watch_excludes`, `watch_files_from`,
//! `watch_excludes_from` and `repoll_ms`: parsed, in the schema and
//! template, and ignored from an untrusted project file.

use std::fs;

use fe_config::settings::{UNTRUSTED_LSP, trusted_lsp};
use fe_config::{SettingsStore, USER_SETTINGS_TEMPLATE, settings_schema};

const KEYS: [&str; 5] = [
    "watch_files",
    "watch_excludes",
    "watch_files_from",
    "watch_excludes_from",
    "repoll_ms",
];

const TABLE: &str = "[lsp.servers.vars]
watch_files = [\"**/*.css\"]
watch_excludes = [\"**/node_modules\", \"**/dist\"]
watch_files_from = \"/cssVariables/lookupFiles\"
watch_excludes_from = \"/cssVariables/blacklistFolders\"
repoll_ms = 3000
";

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
fn watch_settings_parse() {
    let (_dir, store) = load(
        &format!("{TABLE}[lsp.servers.plain]\ncommand = \"plain\"\n"),
        None,
    );
    assert_eq!(store.diagnostics(), []);
    let servers = &store.settings().lsp.servers;
    let vars = &servers["vars"];
    assert_eq!(vars.watch_files, Some(vec!["**/*.css".to_owned()]));
    assert_eq!(
        vars.watch_excludes,
        Some(vec!["**/node_modules".to_owned(), "**/dist".to_owned()])
    );
    assert_eq!(
        vars.watch_files_from.as_deref(),
        Some("/cssVariables/lookupFiles")
    );
    assert_eq!(
        vars.watch_excludes_from.as_deref(),
        Some("/cssVariables/blacklistFolders")
    );
    assert_eq!(vars.repoll_ms, Some(3000));
    // Each defaults off.
    let plain = &servers["plain"];
    assert_eq!(
        (
            &plain.watch_files,
            &plain.watch_excludes,
            &plain.watch_files_from,
            &plain.watch_excludes_from,
            plain.repoll_ms
        ),
        (&None, &None, &None, &None, None)
    );
}

#[test]
fn watch_settings_in_schema_and_template() {
    let schema = settings_schema();
    let properties = &schema["$defs"]["LspServerSettings"]["properties"];
    for key in KEYS {
        assert!(properties[key].is_object(), "{key}: {properties}");
    }
    for line in [
        "# watch_files = [",
        "# watch_excludes = [",
        "# repoll_ms = ",
    ] {
        assert!(
            USER_SETTINGS_TEMPLATE.contains(line),
            "the template documents {line:?}"
        );
    }
}

#[test]
fn watch_settings_in_project_layer_are_filtered() {
    let user = "[lsp.servers.vars]\ncommand = \"vars\"\n";
    let (dir, store) = load(user, Some(TABLE));
    let vars = &store.settings().lsp.servers["vars"];
    assert_eq!(vars.watch_files, None);
    assert_eq!(vars.watch_excludes, None);
    assert_eq!(vars.watch_files_from, None);
    assert_eq!(vars.watch_excludes_from, None);
    assert_eq!(vars.repoll_ms, None);
    let mut found: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| (d.key.clone().unwrap_or_default(), d.message.as_str()))
        .collect();
    found.sort();
    let mut expected: Vec<_> = (KEYS.iter())
        .map(|key| (format!("lsp.servers.vars.{key}"), UNTRUSTED_LSP))
        .collect();
    expected.sort();
    assert_eq!(found, expected);
    // A trusted root applies them.
    let lsp = trusted_lsp(
        Some(dir.path().join("user.toml")),
        Some(dir.path().join("project.toml")),
    );
    assert_eq!(lsp.servers["vars"].repoll_ms, Some(3000));
    assert_eq!(
        lsp.servers["vars"].watch_files_from.as_deref(),
        Some("/cssVariables/lookupFiles")
    );
}
