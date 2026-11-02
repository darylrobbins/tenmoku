//! `[formatters.<language>]`: an external formatter command per language.

use fe_config::{FormatterPrefer, Settings, SettingsStore};

fn load(user: &str, project: Option<&str>) -> (SettingsStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let user_path = dir.path().join("settings.toml");
    std::fs::write(&user_path, user).unwrap();
    let project_path = project.map(|text| {
        let path = dir.path().join("project.toml");
        std::fs::write(&path, text).unwrap();
        path
    });
    (SettingsStore::load(Some(user_path), project_path), dir)
}

#[test]
fn defaults_and_a_plain_command() {
    assert!(Settings::default().formatters.is_empty());
    let (store, _dir) = load("", None);
    assert!(store.settings().formatters.is_empty());

    let (store, _dir) = load(
        "[formatters.rust]\ncommand = [\"rustfmt\", \"--emit\", \"stdout\"]\n",
        None,
    );
    assert_eq!(store.diagnostics(), []);
    let rust = &store.settings().formatters["rust"];
    assert_eq!(rust.command, ["rustfmt", "--emit", "stdout"]);
    assert_eq!(rust.prefer, FormatterPrefer::External);
    assert!(rust.stdin);
    assert_eq!(rust.timeout_ms, 3000);
}

#[test]
fn file_placeholder_and_prefer_parse() {
    let (store, _dir) = load(
        "[formatters.typescript]\n\
         command = [\"prettier\", \"--stdin-filepath\", \"{file}\"]\n\
         prefer = \"lsp\"\n\
         [formatters.go]\ncommand = [\"gofmt\", \"{file}\"]\nstdin = false\ntimeout_ms = 60000\n",
        None,
    );
    assert_eq!(store.diagnostics(), []);
    let formatters = &store.settings().formatters;
    let ts = &formatters["typescript"];
    assert_eq!(ts.command, ["prettier", "--stdin-filepath", "{file}"]);
    assert_eq!(ts.prefer, FormatterPrefer::Lsp);
    assert!(ts.stdin);
    let go = &formatters["go"];
    assert!(!go.stdin);
    assert_eq!(go.timeout_ms, 60000);
}

#[test]
fn bad_values_are_settings_diagnostics() {
    let (store, _dir) = load(
        "[formatters.rust]\ncommand = [\"rustfmt\"]\n\
         [formatters.a]\ncommand = []\n\
         [formatters.b]\ncommand = \"black -\"\n\
         [formatters.c]\ncommand = [\"x\"]\nprefer = \"both\"\n\
         [formatters.d]\ncommand = [\"x\"]\ntimeout_ms = 0\n\
         [formatters.e]\ncommand = [\"x\"]\ntimeout_ms = 60001\n\
         [formatters.python]\ncommand = [\"black\", \"-\"]\n\
         [editor]\nauto_close_pairs = false\n",
        None,
    );
    let formatters = &store.settings().formatters;
    assert_eq!(
        formatters.keys().collect::<Vec<_>>(),
        ["python", "rust"],
        "{:?}",
        store.diagnostics()
    );
    let keys: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| d.key.clone().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "formatters.a.command",
            "formatters.b.command",
            "formatters.c.prefer",
            "formatters.d.timeout_ms",
            "formatters.e.timeout_ms",
        ]
    );
    assert!(store.diagnostics().iter().all(|d| !d.is_error()));
    assert!(
        !store.settings().editor.auto_close_pairs,
        "the file applied"
    );
}

#[test]
fn a_formatters_value_that_is_not_a_table_is_dropped() {
    let (store, _dir) = load("formatters = 3\n[editor]\nauto_close_pairs = false\n", None);
    assert!(store.settings().formatters.is_empty());
    assert_eq!(store.diagnostics().len(), 1);
    assert_eq!(store.diagnostics()[0].key.as_deref(), Some("formatters"));
}

#[test]
fn a_project_file_cannot_set_formatters() {
    let (store, _dir) = load(
        "[formatters.rust]\ncommand = [\"rustfmt\"]\n",
        Some("[formatters.python]\ncommand = [\"evil\"]\n"),
    );
    let formatters = &store.settings().formatters;
    assert_eq!(formatters.keys().collect::<Vec<_>>(), ["rust"]);
    assert_eq!(store.diagnostics().len(), 1);
}
