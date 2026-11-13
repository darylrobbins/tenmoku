//! `[lsp]`: its defaults, the user's overrides, and the project layer's
//! trust filter (docs/decisions/lsp-client.md §3.3).

use std::{collections::BTreeMap, fs};

use serde_json::json;

use crate::{
    Settings, SettingsStore,
    settings::{LspReadiness, LspServerSettings, LspSettings, TypescriptServer, UNTRUSTED_LSP},
};

use super::write;

const USER: &str = r#"
[lsp.servers.rust-analyzer]
enabled = false
command = "/opt/ra/bin/rust-analyzer"
args = ["--log", "x"]
env = { RA_LOG = "info" }
languages = ["rust", "ron"]
readiness = "progress"
ready_ignores_progress = ["a/", "b/"]
initialization_options = { checkOnSave = false }
[lsp.servers.rust-analyzer.settings.rust-analyzer]
cargo.features = "all"
"#;

#[test]
fn lsp_defaults_and_user_overrides() {
    let lsp = Settings::default().lsp;
    assert!(lsp.enabled);
    assert_eq!(lsp.max_file_bytes, 52_428_800);
    assert_eq!(lsp.typescript_server, TypescriptServer::TypescriptGo);
    let timeouts = &lsp.timeouts;
    assert_eq!(
        [
            timeouts.definition,
            timeouts.workspace_symbol,
            timeouts.references,
            timeouts.hover,
            timeouts.completion,
            timeouts.resolve,
        ],
        [5000, 5000, 30000, 3000, 3000, 2000]
    );
    assert!(lsp.servers.is_empty());
    assert_eq!(
        toml::Value::try_from(TypescriptServer::default()).unwrap(),
        toml::Value::String("typescript-go".into())
    );

    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "settings.toml", USER);
    let mut store = SettingsStore::load(Some(path.clone()), None);
    assert_eq!(store.diagnostics(), []);
    let expected = LspServerSettings {
        enabled: false,
        command: Some("/opt/ra/bin/rust-analyzer".into()),
        args: Some(vec!["--log".into(), "x".into()]),
        env: Some(BTreeMap::from([("RA_LOG".into(), "info".into())])),
        languages: Some(vec!["rust".into(), "ron".into()]),
        readiness: Some(LspReadiness::Progress),
        ready_ignores_progress: Some(vec!["a/".into(), "b/".into()]),
        root_markers: None,
        add_on: None,
        watch_files: None,
        watch_excludes: None,
        watch_files_from: None,
        watch_excludes_from: None,
        repoll_ms: None,
        initialization_options: Some(json!({ "checkOnSave": false })),
        settings: json!({ "rust-analyzer": { "cargo": { "features": "all" } } })
            .as_object()
            .cloned(),
    };
    let servers = &store.settings().lsp.servers;
    assert_eq!(servers.len(), 1);
    assert_eq!(servers["rust-analyzer"], expected);

    // A readiness that isn't a mode is an error, and the last good table
    // stays in effect.
    let bad = USER.replace("\"progress\"", "\"sometimes\"");
    fs::write(&path, &bad).unwrap();
    store
        .reload(std::slice::from_ref(&path))
        .expect("an error to report");
    let [diagnostic] = store.diagnostics() else {
        panic!("one diagnostic: {:#?}", store.diagnostics());
    };
    assert!(diagnostic.is_error(), "{diagnostic}");
    assert_eq!(
        diagnostic.key.as_deref(),
        Some("lsp.servers.rust-analyzer.readiness"),
        "{diagnostic}"
    );
    assert_eq!(store.settings().lsp.servers["rust-analyzer"], expected);
    // So is an empty command, and a misspelled key warns.
    fs::write(&path, USER.replace("/opt/ra/bin/rust-analyzer", "")).unwrap();
    store
        .reload(std::slice::from_ref(&path))
        .expect("an error to report");
    let [diagnostic] = store.diagnostics() else {
        panic!("one diagnostic: {:#?}", store.diagnostics());
    };
    assert!(diagnostic.is_error(), "{diagnostic}");
    assert_eq!(
        diagnostic.key.as_deref(),
        Some("lsp.servers.rust-analyzer.command")
    );
    assert_eq!(store.settings().lsp.servers["rust-analyzer"], expected);
    fs::write(
        &path,
        format!("{USER}\n[lsp.servers.pyright]\ncomand = \"x\"\n"),
    )
    .unwrap();
    store.reload(std::slice::from_ref(&path)).expect("a change");
    let found: Vec<_> = store
        .diagnostics()
        .iter()
        .map(|d| (d.is_error(), d.key.as_deref()))
        .collect();
    assert_eq!(found, [(false, Some("lsp.servers.pyright.comand"))]);
    // The other TypeScript server is a setting.
    fs::write(
        &path,
        "[lsp]\ntypescript_server = \"typescript-language-server\"\n",
    )
    .unwrap();
    store.reload(std::slice::from_ref(&path)).expect("a change");
    assert_eq!(store.diagnostics(), []);
    assert_eq!(
        store.settings().lsp.typescript_server,
        TypescriptServer::TypescriptLanguageServer
    );
    fs::write(&path, &bad).unwrap();

    // At startup the erroring file is ignored.
    let store = SettingsStore::load(Some(path), None);
    assert!(store.diagnostics()[0].is_error());
    assert_eq!(store.settings().lsp, LspSettings::default());
}

/// Project lines that may not apply, each with the key it sets.
const UNTRUSTED: [(&str, &str); 12] = [
    (
        "lsp.servers.rust-analyzer.command = \"/tmp/evil\"",
        "lsp.servers.rust-analyzer.command",
    ),
    (
        "lsp.servers.rust-analyzer.args = [\"--evil\"]",
        "lsp.servers.rust-analyzer.args",
    ),
    (
        "lsp.servers.rust-analyzer.env = { PATH = \"/tmp\" }",
        "lsp.servers.rust-analyzer.env",
    ),
    (
        "lsp.servers.rust-analyzer.settings = { rust-analyzer = { check = { overrideCommand = [\"sh\"] } } }",
        "lsp.servers.rust-analyzer.settings",
    ),
    (
        "lsp.servers.rust-analyzer.initialization_options = { a = 1 }",
        "lsp.servers.rust-analyzer.initialization_options",
    ),
    (
        "lsp.servers.pyright.languages = [\"rust\"]",
        "lsp.servers.pyright.languages",
    ),
    (
        "lsp.servers.pyright.readiness = \"server-status\"",
        "lsp.servers.pyright.readiness",
    ),
    (
        "lsp.servers.pyright.ready_ignores_progress = [\"\"]",
        "lsp.servers.pyright.ready_ignores_progress",
    ),
    ("lsp.max_file_bytes = 1", "lsp.max_file_bytes"),
    ("lsp.timeouts.hover = 1", "lsp.timeouts"),
    (
        "lsp.typescript_server = \"typescript-go\"",
        "lsp.typescript_server",
    ),
    // Turning on what the user turned off widens too.
    (
        "lsp.servers.typescript-go.enabled = true",
        "lsp.servers.typescript-go.enabled",
    ),
];

const USER_BASE: &str = "[lsp.servers.typescript-go]\nenabled = false\n\
                         [lsp.servers.rust-analyzer]\ncommand = \"ra\"\n";

#[test]
fn project_layer_may_only_narrow() {
    let dir = tempfile::tempdir().unwrap();
    for with_user in [true, false] {
        let user = with_user.then(|| write(&dir, "user.toml", USER_BASE));
        let without = SettingsStore::load(user.clone(), None);
        assert_eq!(without.diagnostics(), []);
        let without = (**without.settings()).clone();
        let with_tab = |tab_size| {
            let mut settings = without.clone();
            settings.editor.tab_size = tab_size;
            settings
        };
        assert_eq!(
            UNTRUSTED_LSP,
            "LSP settings in project files apply only in trusted folders"
        );
        let project = dir.path().join("project.toml");
        for (line, key) in UNTRUSTED {
            let check = |store: &SettingsStore, tab_size| {
                assert_eq!(**store.settings(), with_tab(tab_size), "{line}");
                let found: Vec<_> = store
                    .diagnostics()
                    .iter()
                    .map(|d| (d.is_error(), d.key.as_deref(), d.line, d.message.as_str()))
                    .collect();
                assert_eq!(
                    found,
                    [(false, Some(key), 2, UNTRUSTED_LSP)],
                    "{line}: {:#?}",
                    store.diagnostics()
                );
            };
            // At load, with the rest of the file applying.
            fs::write(&project, format!("editor.tab_size = 2\n{line}\n")).unwrap();
            let mut store = SettingsStore::load(user.clone(), Some(project.clone()));
            check(&store, 2);
            // Through a reload of the edited file.
            fs::write(&project, format!("editor.tab_size = 3\n{line}\n")).unwrap();
            store
                .reload(std::slice::from_ref(&project))
                .expect("a change");
            check(&store, 3);
            // And added by a reload to a file that had none.
            fs::write(&project, "editor.tab_size = 2\n").unwrap();
            let mut store = SettingsStore::load(user.clone(), Some(project.clone()));
            assert_eq!(store.diagnostics(), []);
            fs::write(&project, format!("editor.tab_size = 2\n{line}\n")).unwrap();
            store
                .reload(std::slice::from_ref(&project))
                .expect("a warning");
            check(&store, 2);
        }

        // Turning servers off applies, at load and through a reload.
        let mut pyright_off = without.clone();
        pyright_off
            .lsp
            .servers
            .entry("pyright".into())
            .or_default()
            .enabled = false;
        let mut all_off = pyright_off.clone();
        all_off.lsp.enabled = false;
        let narrowing = "[lsp]\nservers.pyright.enabled = false\n";
        fs::write(&project, narrowing).unwrap();
        let mut store = SettingsStore::load(user.clone(), Some(project.clone()));
        assert_eq!(store.diagnostics(), []);
        assert_eq!(**store.settings(), pyright_off);
        fs::write(&project, format!("{narrowing}enabled = false\n")).unwrap();
        let event = store
            .reload(std::slice::from_ref(&project))
            .expect("a change");
        assert_eq!(*event.settings, all_off);
        assert_eq!(store.diagnostics(), []);
        // Narrowing keys and others side by side: only the others go.
        fs::write(
            &project,
            format!("{narrowing}enabled = false\nmax_file_bytes = 1\n"),
        )
        .unwrap();
        store
            .reload(std::slice::from_ref(&project))
            .expect("a warning");
        assert_eq!(**store.settings(), all_off);
        let keys: Vec<_> = store
            .diagnostics()
            .iter()
            .map(|d| d.key.as_deref())
            .collect();
        assert_eq!(keys, [Some("lsp.max_file_bytes")]);

        // A value that isn't even valid, in a key that's ignored anyway,
        // rejects nothing: the opt-out and the rest of the file apply.
        for (bad, key) in [
            ("typescript_server = \"deno\"", "lsp.typescript_server"),
            (
                "servers.pyright.readiness = \"sometimes\"",
                "lsp.servers.pyright.readiness",
            ),
            ("servers.pyright.command = 5", "lsp.servers.pyright.command"),
            ("servers.typescript-go = \"x\"", "lsp.servers.typescript-go"),
            ("timeouts = []", "lsp.timeouts"),
            (
                "servers.pyright.enabled = \"no\"",
                "lsp.servers.pyright.enabled",
            ),
        ] {
            let text = format!("editor.tab_size = 2\n[lsp]\nenabled = false\n{bad}\n");
            fs::write(&project, &text).unwrap();
            let store = SettingsStore::load(user.clone(), Some(project.clone()));
            let found: Vec<_> = store
                .diagnostics()
                .iter()
                .map(|d| (d.is_error(), d.key.as_deref(), d.line))
                .collect();
            assert_eq!(found, [(false, Some(key), 4)], "{text}");
            let mut expected = with_tab(2);
            expected.lsp.enabled = false;
            assert_eq!(**store.settings(), expected, "{text}");
        }
        fs::write(&project, "editor.tab_size = 2\nlsp = 5\n").unwrap();
        let store = SettingsStore::load(user.clone(), Some(project.clone()));
        let found: Vec<_> = store
            .diagnostics()
            .iter()
            .map(|d| (d.is_error(), d.key.as_deref(), d.line))
            .collect();
        assert_eq!(found, [(false, Some("lsp"), 2)]);
        assert_eq!(**store.settings(), with_tab(2));
        // A real error elsewhere still rejects the file.
        fs::write(&project, "editor.tab_size = 0\n[lsp]\nenabled = false\n").unwrap();
        let store = SettingsStore::load(user.clone(), Some(project.clone()));
        assert!(store.diagnostics().iter().any(|d| d.is_error()));
        assert_eq!(**store.settings(), without);
    }
}
