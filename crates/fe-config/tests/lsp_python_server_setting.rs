//! `lsp.python_server`: its default, each value, the server a table picks
//! while it's unset, an unknown value warned of and left unset, and a
//! project file's narrowed as the other `lsp` keys are.

use std::fs;

use fe_config::settings::{LspSettings, PythonServer, UNTRUSTED_LSP, trusted_lsp};
use fe_config::{Settings, SettingsStore, USER_SETTINGS_TEMPLATE, settings_schema};

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

/// The store's diagnostics as (key, message, line).
fn diagnostics(store: &SettingsStore) -> Vec<(Option<String>, String, usize)> {
    store
        .diagnostics()
        .iter()
        .map(|d| (d.key.clone(), d.message.clone(), d.line))
        .collect()
}

#[test]
fn python_server_defaults_to_pyrefly() {
    let lsp = Settings::default().lsp;
    assert_eq!(lsp.python_server, None);
    assert_eq!(lsp.chosen_python_server(), PythonServer::Pyrefly);
    assert_eq!(PythonServer::default(), PythonServer::Pyrefly);
    // Unset, it isn't written out.
    let table = toml::Table::try_from(&lsp).unwrap();
    assert!(!table.contains_key("python_server"), "{table}");

    let schema = settings_schema();
    let property = &schema["$defs"]["LspSettings"]["properties"]["python_server"];
    assert!(property.is_object(), "{property}");
    let values: Vec<&str> = schema["$defs"]["PythonServer"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variant| variant["const"].as_str().unwrap())
        .collect();
    assert_eq!(values, ["pyrefly", "ty", "pyright"]);
    assert!(
        USER_SETTINGS_TEMPLATE.contains("# python_server = \"ty\""),
        "the template documents python_server"
    );
}

#[test]
fn each_value_parses() {
    for server in PythonServer::ALL {
        let name = server.name();
        let (_dir, store) = load(&format!("[lsp]\npython_server = \"{name}\"\n"), None);
        assert_eq!(store.diagnostics(), [], "{name}");
        let lsp = &store.settings().lsp;
        assert_eq!(lsp.python_server, Some(server), "{name}");
        assert_eq!(lsp.chosen_python_server(), server, "{name}");
        assert_eq!(
            toml::Value::try_from(server).unwrap(),
            toml::Value::String(name.into())
        );
    }
}

#[test]
fn unset_a_python_servers_table_picks_it() {
    let chosen = |user: &str| {
        let (_dir, store) = load(user, None);
        assert_eq!(store.diagnostics(), [], "{user}");
        store.settings().lsp.chosen_python_server()
    };
    // Today's pyright settings keep pyright.
    let pyright = "[lsp.servers.pyright]\nsettings.python.analysis.typeCheckingMode = \"strict\"\n";
    assert_eq!(chosen(pyright), PythonServer::Pyright);
    // Turned off, it's still the one: none of the built-ins runs.
    assert_eq!(
        chosen("[lsp.servers.pyright]\nenabled = false\n"),
        PythonServer::Pyright
    );
    // ty before pyright, pyrefly before both.
    let both = "[lsp.servers.pyright]\nenabled = true\n[lsp.servers.ty]\nenabled = true\n";
    assert_eq!(chosen(both), PythonServer::Ty);
    let all = format!("{both}[lsp.servers.pyrefly]\nenabled = true\n");
    assert_eq!(chosen(&all), PythonServer::Pyrefly);
    // Another server's table picks nothing.
    assert_eq!(
        chosen("[lsp.servers.ruff]\nenabled = false\n"),
        PythonServer::Pyrefly
    );
    // The setting wins over any table.
    assert_eq!(
        chosen(&format!("[lsp]\npython_server = \"pyrefly\"\n{pyright}")),
        PythonServer::Pyrefly
    );
    let mut settings = LspSettings {
        python_server: Some(PythonServer::Ty),
        ..LspSettings::default()
    };
    settings
        .servers
        .insert("pyright".into(), Default::default());
    assert_eq!(settings.chosen_python_server(), PythonServer::Ty);
}

#[test]
fn an_unknown_value_is_warned_of_and_unset() {
    for (value, written) in [("jedi", "\"jedi\""), ("Pyright", "\"Pyright\""), ("3", "3")] {
        let user = format!("[editor]\ntab_size = 2\n\n[lsp]\npython_server = {written}\n");
        let (dir, store) = load(&user, None);
        let found = diagnostics(&store);
        let message = format!(
            "{written} isn't a Python server: use \"pyrefly\", \"ty\" or \"pyright\"; ignored, \
             so the default applies"
        );
        assert_eq!(
            found,
            [(Some("lsp.python_server".into()), message, 5)],
            "{value}"
        );
        assert!(!store.diagnostics()[0].is_error(), "{value}");
        // The rest of the file applies; the server is the default.
        let settings = store.settings();
        assert_eq!(settings.editor.tab_size, 2, "{value}");
        assert_eq!(settings.lsp.python_server, None, "{value}");
        assert_eq!(
            settings.lsp.chosen_python_server(),
            PythonServer::Pyrefly,
            "{value}"
        );
        // A trusted root's layers do the same, rather than drop the
        // file's other lsp keys.
        fs::write(
            dir.path().join("user.toml"),
            format!("[lsp]\nmax_file_bytes = 7\npython_server = {written}\n"),
        )
        .unwrap();
        let lsp = trusted_lsp(Some(dir.path().join("user.toml")), None);
        assert_eq!(
            (lsp.max_file_bytes, lsp.python_server),
            (7, None),
            "{value}"
        );
    }
}

#[test]
fn a_project_files_value_is_narrowed() {
    let user = "[lsp]\npython_server = \"pyright\"\n";
    let project = "[editor]\ntab_size = 8\n\n[lsp]\npython_server = \"ty\"\n";
    let (dir, store) = load(user, Some(project));
    assert_eq!(
        diagnostics(&store),
        [(
            Some("lsp.python_server".into()),
            UNTRUSTED_LSP.to_owned(),
            5
        )]
    );
    let settings = store.settings();
    assert_eq!(settings.editor.tab_size, 8);
    assert_eq!(settings.lsp.python_server, Some(PythonServer::Pyright));

    // Alone in the project, it's still dropped: the default applies.
    let (_other, store) = load("", Some(project));
    assert_eq!(store.settings().lsp.python_server, None);
    assert_eq!(
        store.settings().lsp.chosen_python_server(),
        PythonServer::Pyrefly
    );

    // A trusted root applies it as written.
    let lsp = trusted_lsp(
        Some(dir.path().join("user.toml")),
        Some(dir.path().join("project.toml")),
    );
    assert_eq!(lsp.python_server, Some(PythonServer::Ty));
}
