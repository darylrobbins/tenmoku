//! A trusted root's `lsp` settings: `fe_config::settings::trusted_lsp`
//! applies the project file's `lsp` keys as written, over the user file's,
//! while `SettingsStore` still narrows them.

use std::{collections::BTreeMap, fs};

use fe_config::SettingsStore;
use fe_config::settings::{LspLayer, LspSettings, UNTRUSTED_LSP, trusted_lsp};
use serde_json::json;

const USER: &str = r#"
[lsp.servers.rust-analyzer]
command = "ra-user"
args = ["--user"]
env = { FROM = "user" }

[lsp.servers.rust-analyzer.settings.rust-analyzer]
cargo = { features = "user" }
"#;

const PROJECT: &str = r#"
editor.tab_size = 2

[lsp.servers.rust-analyzer]
command = "ra-project"
args = ["--project"]
env = { FROM = "project" }

[lsp.servers.rust-analyzer.settings.rust-analyzer]
cargo = { features = ["project"] }

[lsp.servers.mine]
command = "my-server"
languages = ["rust"]

[theme]
mode = "light"
"#;

#[test]
fn project_lsp_keys_apply_unfiltered() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("user.toml");
    let project = dir.path().join("project.toml");
    fs::write(&user, USER).unwrap();
    fs::write(&project, PROJECT).unwrap();

    // Trusted: the project's values win over the user's.
    let lsp = trusted_lsp(Some(user.clone()), Some(project.clone()));
    let ra = &lsp.servers["rust-analyzer"];
    assert_eq!(ra.command.as_deref(), Some("ra-project"));
    assert_eq!(ra.args, Some(vec!["--project".to_owned()]));
    assert_eq!(
        ra.env,
        Some(BTreeMap::from([("FROM".to_owned(), "project".to_owned())]))
    );
    assert_eq!(
        ra.settings.clone().map(serde_json::Value::Object),
        Some(json!({ "rust-analyzer": { "cargo": { "features": ["project"] } } }))
    );
    let mine = &lsp.servers["mine"];
    assert_eq!(mine.command.as_deref(), Some("my-server"));
    assert_eq!(mine.languages, Some(vec!["rust".to_owned()]));

    // With no project file, the user's; with neither, the defaults.
    let lsp = trusted_lsp(Some(user.clone()), None);
    assert_eq!(
        lsp.servers["rust-analyzer"].command.as_deref(),
        Some("ra-user")
    );
    assert_eq!(
        trusted_lsp(None, Some(dir.path().join("missing.toml"))),
        Default::default()
    );

    // The store, on the same files, still drops them, with the warning.
    let store = SettingsStore::load(Some(user), Some(project));
    let ra = &store.settings().lsp.servers["rust-analyzer"];
    assert_eq!(ra.command.as_deref(), Some("ra-user"));
    assert_eq!(ra.args, Some(vec!["--user".to_owned()]));
    assert_eq!(
        ra.settings.clone().map(serde_json::Value::Object),
        Some(json!({ "rust-analyzer": { "cargo": { "features": "user" } } }))
    );
    assert!(!store.settings().lsp.servers.contains_key("mine"));
    assert_eq!(store.settings().editor.tab_size, 2);
    assert_eq!(
        UNTRUSTED_LSP,
        "LSP settings in project files apply only in trusted folders"
    );
    let warned: Vec<_> = store
        .diagnostics()
        .iter()
        .filter(|d| d.message == UNTRUSTED_LSP)
        .map(|d| d.key.clone().unwrap_or_default())
        .collect();
    for key in [
        "lsp.servers.rust-analyzer.command",
        "lsp.servers.rust-analyzer.args",
        "lsp.servers.rust-analyzer.env",
        "lsp.servers.rust-analyzer.settings",
        "lsp.servers.mine.command",
    ] {
        assert!(warned.iter().any(|k| k == key), "{key}: {warned:#?}");
    }
}

#[test]
fn project_lsp_keeps_its_last_good_table() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project.toml");
    let filtered = fe_config::settings::LspSettings::default();
    let mut layer = LspLayer::project(project.clone());
    // Missing: nothing over the filtered settings.
    layer.read();
    assert_eq!(layer.apply(&filtered), filtered);

    fs::write(&project, PROJECT).unwrap();
    layer.read();
    let command = |layer: &LspLayer| {
        layer.apply(&filtered).servers["rust-analyzer"]
            .command
            .clone()
    };
    assert_eq!(command(&layer).as_deref(), Some("ra-project"));

    // A syntax error, a type error and a schema error each keep it.
    for broken in [
        "[lsp.servers.rust-analyzer\ncommand = ",
        "[lsp.servers.rust-analyzer]\ncommand = 3\n",
        "[lsp.servers.rust-analyzer]\ncommand = \"\"\n",
    ] {
        fs::write(&project, broken).unwrap();
        layer.read();
        assert_eq!(command(&layer).as_deref(), Some("ra-project"), "{broken}");
    }

    // Laid over the filtered settings: the user's values it doesn't set
    // stay.
    let user = fe_config::settings::LspSettings {
        enabled: false,
        max_file_bytes: 7,
        ..Default::default()
    };
    let applied = layer.apply(&user);
    assert!(!applied.enabled);
    assert_eq!(applied.max_file_bytes, 7);

    // Fixed, it reads again; emptied, it has nothing.
    fs::write(
        &project,
        "[lsp.servers.rust-analyzer]\ncommand = \"ra-fixed\"\n",
    )
    .unwrap();
    layer.read();
    assert_eq!(command(&layer).as_deref(), Some("ra-fixed"));
    fs::write(&project, "").unwrap();
    layer.read();
    assert_eq!(layer.apply(&filtered), filtered);
}

#[test]
fn trusted_settings_are_the_project_over_the_user_over_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("user.toml");
    let project = dir.path().join("project.toml");
    let enabled = |lsp: &LspSettings| (lsp.enabled, lsp.servers["rust-analyzer"].enabled);
    let both = |user_text: &str, project_text: &str| {
        fs::write(&user, user_text).unwrap();
        fs::write(&project, project_text).unwrap();
        trusted_lsp(Some(user.clone()), Some(project.clone()))
    };

    // The project's disabling flags are the host store's too, but they
    // aren't in the trusted settings once the file stops setting them.
    let off = "lsp.enabled = false\n[lsp.servers.rust-analyzer]\nenabled = false\n";
    assert_eq!(enabled(&both("", off)), (false, false));
    let removed = "[lsp.servers.rust-analyzer]\ncommand = \"ra\"\n";
    assert_eq!(enabled(&both("", removed)), (true, true));

    // The user's own flag holds when the project doesn't set it.
    assert_eq!(enabled(&both(off, removed)), (false, false));
}

#[test]
fn layers_keep_their_last_good_tables_with_flags_changed_in_an_invalid_file() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project.toml");
    let mut layer = LspLayer::project(project.clone());
    fs::write(&project, "[lsp.servers.rust-analyzer]\ncommand = \"ra\"\n").unwrap();
    layer.read();

    // An invalid command with a disabling flag beside it: the file is
    // rejected whole, the flag with it.
    fs::write(
        &project,
        "lsp.enabled = false\n[lsp.servers.rust-analyzer]\ncommand = \"\"\nenabled = false\n",
    )
    .unwrap();
    layer.read();
    let applied = layer.apply(&LspSettings::default());
    assert!(applied.enabled);
    assert_eq!(
        applied.servers["rust-analyzer"].command.as_deref(),
        Some("ra")
    );
    assert!(applied.servers["rust-analyzer"].enabled);
}

#[test]
fn a_user_layer_keeps_its_last_good_table() {
    let dir = tempfile::tempdir().unwrap();
    let user = dir.path().join("user.toml");
    fs::write(&user, "lsp.enabled = false\n").unwrap();
    let mut layer = LspLayer::user(user.clone());
    layer.read();
    assert!(!layer.apply(&LspSettings::default()).enabled);
    fs::write(&user, "lsp.enabled = \n").unwrap();
    layer.read();
    assert!(!layer.apply(&LspSettings::default()).enabled);
    fs::write(&user, "").unwrap();
    layer.read();
    assert!(layer.apply(&LspSettings::default()).enabled);
}
