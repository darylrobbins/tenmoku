//! Servers extensions contribute ([`LspManager::set_extension_specs`]): a
//! tier between the built-ins and the user's `[lsp]` settings.

use std::path::PathBuf;

use fe_buffer::Buffer;
use fe_config::settings::{LspServerSettings, LspSettings};
use lsp_types::Url;
use serde_json::json;

use super::{ROOT, launched, open, routed, settle, wait_synced};
use crate::fake::{FakeServer, Record};
use crate::{LspManager, ServerSpec, extension_spec};

/// The spec an extension's `[contributes.lsp.<name>]` table `table` gives.
fn contributed(name: &str, table: serde_json::Value) -> ServerSpec {
    let settings: LspServerSettings = serde_json::from_value(table).unwrap();
    extension_spec(name, &settings).unwrap()
}

fn zls() -> ServerSpec {
    contributed(
        "zls",
        json!({ "command": "zls", "args": ["--stdio"], "languages": ["zig", "rust"] }),
    )
}

/// `settings` with `[lsp.servers.<name>]` set to `table`.
fn with_server(mut settings: LspSettings, name: &str, table: serde_json::Value) -> LspSettings {
    settings
        .servers
        .insert(name.into(), serde_json::from_value(table).unwrap());
    settings
}

/// The command and args the server named `name` was last launched with.
fn launched_with(fake: &FakeServer, name: &str) -> Option<(PathBuf, Vec<String>)> {
    fake.launched()
        .into_iter()
        .rev()
        .find(|(spec, _)| spec.name == name)
        .map(|(spec, _)| (spec.command, spec.args))
}

/// Opens a document of `language` under [`ROOT`] and waits for the server
/// it's routed to to have it, from launch `launch`: its URI.
fn open_synced(
    fake: &FakeServer,
    manager: &mut LspManager,
    buffer: &mut Buffer,
    name: &str,
    language: &str,
    launch: usize,
) -> Url {
    let (server, uri) = open(manager, ROOT, buffer, name, language);
    assert!(server.is_some(), "{language} has a server");
    wait_synced(fake, &uri, buffer, launch);
    uri
}

#[test]
fn an_extension_server_routes_its_language() {
    assert_eq!(
        extension_spec("none", &LspServerSettings::default()),
        None,
        "no command, no spec"
    );
    let (fake, _clock, mut manager) = routed();
    assert_eq!(manager.route_for("zig"), None);

    manager.set_extension_specs(vec![zls()]);
    assert_eq!(manager.route_for("zig").as_deref(), Some("zls"));
    // A built-in that lists the language comes first.
    assert_eq!(manager.route_for("rust").as_deref(), Some("rust-analyzer"));

    let mut zig = Buffer::new("const a = 1;\n");
    open_synced(&fake, &mut manager, &mut zig, "a.zig", "zig", 1);
    assert_eq!(launched(&fake), [("zls".to_owned(), PathBuf::from(ROOT))]);
    assert_eq!(
        launched_with(&fake, "zls"),
        Some(("zls".into(), vec!["--stdio".to_owned()]))
    );

    // A server only the user's settings define comes before it.
    let settings = with_server(
        LspSettings::default(),
        "zig-user",
        json!({ "command": "zig-user", "languages": ["zig"] }),
    );
    manager.update_settings(&settings);
    assert_eq!(manager.route_for("zig").as_deref(), Some("zig-user"));

    // Taken away, its language has none.
    manager.update_settings(&LspSettings::default());
    manager.set_extension_specs(Vec::new());
    assert_eq!(manager.route_for("zig"), None);
}

#[test]
fn user_settings_apply_over_an_extension_server() {
    let (fake, _clock, mut manager) = routed();
    manager.set_extension_specs(vec![zls()]);
    let settings = with_server(
        LspSettings::default(),
        "zls",
        json!({ "command": "/opt/zls", "args": ["--debug"] }),
    );
    manager.update_settings(&settings);
    assert_eq!(manager.route_for("zig").as_deref(), Some("zls"));
    let mut zig = Buffer::new("const a = 1;\n");
    open_synced(&fake, &mut manager, &mut zig, "a.zig", "zig", 1);
    assert_eq!(
        launched_with(&fake, "zls"),
        Some(("/opt/zls".into(), vec!["--debug".to_owned()]))
    );

    // Turned off, it's gone: stopped, and its language has no server.
    let off = with_server(settings, "zls", json!({ "enabled": false }));
    manager.update_settings(&off);
    assert_eq!(manager.route_for("zig"), None);
    assert_eq!(manager.server_of(zig.id()), None);

    // One named like a built-in is ignored: the built-in keeps its command
    // and languages.
    manager.update_settings(&LspSettings::default());
    manager.set_extension_specs(vec![contributed(
        "pyright",
        json!({ "command": "evil", "languages": ["python", "zig"] }),
    )]);
    assert_eq!(manager.route_for("zig"), None);
    assert_eq!(manager.route_for("python").as_deref(), Some("pyright"));
    let mut py = Buffer::new("print(1)\n");
    open_synced(&fake, &mut manager, &mut py, "a.py", "python", 2);
    assert_eq!(
        launched_with(&fake, "pyright"),
        Some(("pyright-langserver".into(), vec!["--stdio".to_owned()]))
    );

    // So is one named like a host spec, which wins.
    manager.register_spec(ServerSpec {
        languages: vec!["odin".into()],
        ..ServerSpec::new("ols", "/host/ols")
    });
    manager.set_extension_specs(vec![contributed(
        "ols",
        json!({ "command": "evil", "languages": ["odin", "zig"] }),
    )]);
    assert_eq!(manager.route_for("zig"), None);
    assert_eq!(manager.route_for("odin").as_deref(), Some("ols"));
}

#[test]
fn changing_extension_specs_restarts_only_changed_servers() {
    let (fake, _clock, mut manager) = routed();
    let ols =
        |command: &str| contributed("ols", json!({ "command": command, "languages": ["odin"] }));
    manager.set_extension_specs(vec![zls(), ols("ols")]);
    let mut zig = Buffer::new("const a = 1;\n");
    open_synced(&fake, &mut manager, &mut zig, "a.zig", "zig", 1);
    let mut odin = Buffer::new("package main\n");
    let odin_uri = open_synced(&fake, &mut manager, &mut odin, "a.odin", "odin", 2);
    fake.wait_initialized_at(1);
    fake.wait_initialized_at(2);
    let (zls_server, ols_server) = (
        manager.server_of(zig.id()).unwrap(),
        manager.server_of(odin.id()).unwrap(),
    );

    // Its settings alone: configured, not restarted.
    let zls_settings = contributed(
        "zls",
        json!({
            "command": "zls",
            "args": ["--stdio"],
            "languages": ["zig", "rust"],
            "settings": { "zls": { "semantic_tokens": "full" } },
        }),
    );
    manager.set_extension_specs(vec![zls_settings.clone(), ols("ols")]);
    let records = fake.wait_for_at(1, "didChangeConfiguration", |records| {
        records
            .iter()
            .any(|r| r.is_notification("workspace/didChangeConfiguration"))
    });
    let sent = records.iter().find_map(|r| match r {
        Record::Notification { method, params } if method == "workspace/didChangeConfiguration" => {
            Some(params["settings"].clone())
        }
        _ => None,
    });
    assert_eq!(sent, Some(json!({ "zls": { "semantic_tokens": "full" } })));
    settle();
    assert_eq!(fake.launches(), 2);
    assert_eq!(manager.server_of(zig.id()), Some(zls_server));
    assert_eq!(manager.server_of(odin.id()), Some(ols_server));

    // A changed command: that server restarts, with its document, and the
    // other keeps running.
    manager.set_extension_specs(vec![zls_settings, ols("/opt/ols")]);
    fake.wait_initialized_at(3);
    wait_synced(&fake, &odin_uri, &odin, 3);
    settle();
    assert_eq!(fake.launches(), 3);
    assert_eq!(
        launched_with(&fake, "ols"),
        Some(("/opt/ols".into(), Vec::new()))
    );
    assert_eq!(manager.server_of(zig.id()), Some(zls_server));
    assert_ne!(manager.server_of(odin.id()), Some(ols_server));
    assert!(manager.server_of(odin.id()).is_some());
}
