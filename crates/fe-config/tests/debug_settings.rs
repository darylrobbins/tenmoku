//! The `[debug]` settings (docs/decisions/r1b9dw3g0g-debugger.md §3, card
//! D3): the user's adapters, the timeouts' defaults, and a project file
//! that can't choose what runs or turn on payload logging.

use std::collections::BTreeMap;

use fe_config::{
    Settings, SettingsModel, SettingsStore,
    debug_settings::{DebugAdapterSettings, DebugTimeouts, DebugTransport, PROJECT_DEBUG},
};

fn load(user: &str, project: Option<&str>) -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let user_path = dir.path().join("settings.toml");
    std::fs::write(&user_path, user).unwrap();
    let project_path = project.map(|contents| {
        let path = dir.path().join("project.toml");
        std::fs::write(&path, contents).unwrap();
        path
    });
    SettingsStore::load(Some(user_path), project_path)
}

#[test]
fn user_adapter_settings_parse() {
    let store = load(
        r#"
[debug]
log_payloads = true

[debug.adapters.codelldb]
command = "/opt/codelldb/adapter/codelldb"

[debug.adapters.lldb-dap]
args = ["--repl-mode", "command"]
env = { LLDB_DEBUGSERVER_PATH = "/usr/bin/lldb-server" }
path = ["/opt/llvm/bin", "~/llvm/bin"]

[debug.adapters.my_adapter]
command = "my-adapter"
args = ["--listen", "${port}"]
transport = "tcp"
languages = ["zig"]
types = ["my-adapter", "zig"]
"#,
        None,
    );
    assert_eq!(store.diagnostics(), []);
    let debug = &store.settings().debug;
    assert!(debug.log_payloads);
    let adapters: Vec<&str> = debug.adapters.keys().map(String::as_str).collect();
    assert_eq!(adapters, ["codelldb", "lldb-dap", "my_adapter"]);
    assert_eq!(
        debug.adapters["codelldb"],
        DebugAdapterSettings {
            command: Some("/opt/codelldb/adapter/codelldb".into()),
            ..Default::default()
        }
    );
    assert_eq!(
        debug.adapters["lldb-dap"],
        DebugAdapterSettings {
            args: Some(vec!["--repl-mode".into(), "command".into()]),
            env: Some(BTreeMap::from([(
                "LLDB_DEBUGSERVER_PATH".into(),
                "/usr/bin/lldb-server".into()
            )])),
            path: Some(vec!["/opt/llvm/bin".into(), "~/llvm/bin".into()]),
            ..Default::default()
        }
    );
    assert_eq!(
        debug.adapters["my_adapter"],
        DebugAdapterSettings {
            command: Some("my-adapter".into()),
            args: Some(vec!["--listen".into(), "${port}".into()]),
            transport: Some(DebugTransport::Tcp),
            languages: Some(vec!["zig".into()]),
            types: Some(vec!["my-adapter".into(), "zig".into()]),
            ..Default::default()
        }
    );

    // Bad values are errors at their keys.
    for (source, key) in [
        (
            "[debug.adapters.x]\ncommand = \"\"\n",
            "debug.adapters.x.command",
        ),
        (
            "[debug.adapters.x]\ntransport = \"pipe\"\n",
            "debug.adapters.x.transport",
        ),
        (
            "[debug.adapters.x]\nargs = \"--stdio\"\n",
            "debug.adapters.x.args",
        ),
        ("[debug]\nlog_payloads = \"yes\"\n", "debug.log_payloads"),
        ("[debug.timeouts]\nrequest = 0\n", "debug.timeouts.request"),
    ] {
        let store = load(source, None);
        let diagnostics = store.diagnostics();
        assert_eq!(diagnostics.len(), 1, "{source}: {diagnostics:#?}");
        assert!(diagnostics[0].is_error(), "{:?}", diagnostics[0]);
        assert_eq!(diagnostics[0].key.as_deref(), Some(key), "{source}");
    }
}

#[test]
fn a_project_file_cannot_set_an_adapters_command_args_or_env_or_log_payloads() {
    let user = "[debug.adapters.debugpy]\ncommand = \"/usr/bin/python3\"\n";
    let project = r#"
[debug]
log_payloads = true

[debug.timeouts]
launch = 60000

[debug.adapters.debugpy]
command = "./evil"
args = ["-c", "import os"]
env = { PYTHONPATH = "." }
path = ["."]

[debug.adapters.mine]
command = "./tools/dap"
transport = "tcp"
"#;
    let store = load(user, Some(project));
    let debug = &store.settings().debug;
    // The user's adapter is untouched, the project's own is gone, and
    // payloads stay off.
    assert_eq!(
        debug.adapters,
        BTreeMap::from([(
            "debugpy".to_owned(),
            DebugAdapterSettings {
                command: Some("/usr/bin/python3".into()),
                ..Default::default()
            }
        )])
    );
    assert!(!debug.log_payloads);
    // Its timeouts apply.
    assert_eq!(debug.timeouts.launch, 60_000);
    assert_eq!(debug.timeouts.request, DebugTimeouts::default().request);

    // narrow_debug drops each with a warning at its key.
    let mut keys: Vec<String> = store
        .diagnostics()
        .iter()
        .map(|d| {
            assert!(!d.is_error(), "{d:?}");
            assert_eq!(d.message, PROJECT_DEBUG);
            d.key.clone().unwrap()
        })
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "debug.adapters.debugpy.args",
            "debug.adapters.debugpy.command",
            "debug.adapters.debugpy.env",
            "debug.adapters.debugpy.path",
            "debug.adapters.mine.command",
            "debug.adapters.mine.transport",
            "debug.log_payloads",
        ]
    );

    // A project's `debug` that isn't a table is dropped whole, and the
    // user's stays.
    let scalar = load("[debug]\nlog_payloads = true\n", Some("debug = 1\n"));
    assert!(scalar.settings().debug.log_payloads);
    let diagnostics = scalar.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(diagnostics[0].key.as_deref(), Some("debug"));

    // The Settings editor agrees: of `debug`, a project may set only the
    // timeouts, and shows what the store loads.
    let model = SettingsModel::new(Some(user), Some(project));
    let settings = serde_json::to_value(store.settings().as_ref()).unwrap();
    let mut checked = 0;
    for entry in model.entries() {
        if entry.key.first().map(String::as_str) != Some("debug") {
            continue;
        }
        let key = entry.key_path();
        assert_eq!(
            model.workspace_allowed(entry),
            key.starts_with("debug.timeouts."),
            "{key}"
        );
        let mut expected = &settings;
        for name in &entry.key {
            expected = expected.get(name).unwrap_or(&serde_json::Value::Null);
        }
        let value = model.value(entry).value;
        if !(expected.is_null() && value.is_null()) {
            assert_eq!(&value, expected, "{key}");
        }
        checked += 1;
    }
    assert_eq!(checked, 5);
}

#[test]
fn timeouts_have_defaults() {
    let debug = Settings::default().debug;
    assert_eq!(
        debug.timeouts,
        DebugTimeouts {
            request: 10_000,
            launch: 30_000,
            connect: 5_000,
        }
    );
    assert!(debug.adapters.is_empty());
    assert!(!debug.log_payloads);

    // One set leaves the others at their defaults.
    let store = load("[debug.timeouts]\nrequest = 2500\n", None);
    assert_eq!(store.diagnostics(), []);
    let timeouts = &store.settings().debug.timeouts;
    assert_eq!(timeouts.request, 2_500);
    assert_eq!(timeouts.launch, 30_000);
    assert_eq!(timeouts.connect, 5_000);
}
