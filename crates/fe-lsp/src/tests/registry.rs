//! The server registry (docs/decisions/lsp-client.md §3.2, §3.3): built-in
//! specs, `[lsp]` settings and host specs, lazy start per (server, root),
//! the project layer's trust filter end to end, `workspace/configuration`,
//! and bare commands on the login shell's `PATH`.

#[cfg(unix)]
use std::ffi::OsStr;
use std::future::Future;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use fe_buffer::{Buffer, Origin};
use fe_config::SettingsStore;
use fe_config::settings::{LspServerSettings, LspSettings, TypescriptServer};
#[cfg(unix)]
use futures::{AsyncReadExt, AsyncWriteExt};
use lsp_types::request::WorkspaceConfiguration;
use lsp_types::{ConfigurationItem, ConfigurationParams, Url};
use serde_json::{Value, json};

use super::{Events, ROOT, WAIT, answer, capture_logs, logs_from, manager, wait_until};
use crate::document::language_id;
use crate::fake::{Echo, FakeServer, Record, Script};
use crate::registry::{self, Registry, server_for};
use crate::server::BACKSTOP;
use crate::{
    Clock, LaunchCancel, LspEvent, LspManager, ManualClock, ReadinessMode, ServerId, ServerIo,
    ServerLauncher, ServerSpec, ServerStatus,
};
#[cfg(unix)]
use crate::{LocalLauncher, LoginPath};

mod gopls;
mod language_ids;
mod outside_root;
mod root_markers;
mod routed_save;

/// A manager on the fake, with a clock that never moves.
fn routed() -> (FakeServer, LspManager, Events) {
    let fake = FakeServer::new(Script::default());
    let (manager, events) = manager(fake.launcher(), Some(Arc::new(ManualClock::new())));
    (fake, manager, events)
}

fn open(
    manager: &mut LspManager,
    root: Option<&Path>,
    buffer: &mut Buffer,
    path: &str,
    language: &str,
) -> Option<ServerId> {
    let subscription = buffer.subscribe();
    manager
        .open(
            root,
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            subscription,
            Path::new(path),
            language,
        )
        .unwrap()
}

fn root() -> Option<&'static Path> {
    Some(Path::new(ROOT))
}

/// Returns once `server` has read everything sent to it before now.
fn barrier(manager: &LspManager, server: ServerId) {
    answer(manager.request::<Echo>(server, json!({}))).unwrap();
}

fn names(launched: &[(ServerSpec, PathBuf)]) -> Vec<&str> {
    launched
        .iter()
        .map(|(spec, _)| spec.name.as_str())
        .collect()
}

fn uri(path: &str) -> Url {
    Url::from_file_path(path).unwrap()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|&value| value.into()).collect()
}

fn server(settings: Value) -> LspServerSettings {
    serde_json::from_value(settings).unwrap()
}

#[test]
fn builtin_specs() {
    let specs = registry::builtin_specs();
    let summary: Vec<_> = specs
        .iter()
        .map(|spec| {
            (
                spec.name.as_str(),
                spec.command.to_str().unwrap(),
                spec.args.clone(),
                spec.languages.clone(),
                spec.readiness,
                spec.ready_ignores_progress.clone(),
            )
        })
        .collect();
    let typescript = strings(&["javascript", "typescript", "tsx"]);
    assert_eq!(
        summary,
        [
            (
                "rust-analyzer",
                "rust-analyzer",
                vec![],
                strings(&["rust"]),
                ReadinessMode::ServerStatus,
                strings(&["rust-analyzer/flycheck/"]),
            ),
            (
                "pyright",
                "pyright-langserver",
                strings(&["--stdio"]),
                strings(&["python"]),
                ReadinessMode::Progress,
                vec![],
            ),
            (
                "typescript-language-server",
                "typescript-language-server",
                strings(&["--stdio"]),
                typescript.clone(),
                ReadinessMode::Progress,
                vec![],
            ),
            (
                "typescript-go",
                "tsc",
                strings(&["--lsp", "--stdio"]),
                typescript,
                ReadinessMode::Progress,
                vec![],
            ),
            (
                "gopls",
                "gopls",
                vec![],
                strings(&["go"]),
                ReadinessMode::Progress,
                vec![],
            ),
        ]
    );

    // With default settings TypeScript and JavaScript go to typescript-go,
    // and typescript-language-server never runs.
    let mut registry = Registry::default();
    let specs = registry.specs();
    assert_eq!(
        specs
            .iter()
            .map(|spec| spec.name.as_str())
            .collect::<Vec<_>>(),
        ["rust-analyzer", "pyright", "typescript-go", "gopls"]
    );
    let chosen =
        |specs: &[ServerSpec], language| server_for(specs, language).map(|s| s.name.clone());
    for language in ["javascript", "typescript", "tsx"] {
        assert_eq!(chosen(&specs, language).as_deref(), Some("typescript-go"));
    }
    assert!(specs.iter().all(|s| s.name != "typescript-language-server"));
    assert_eq!(chosen(&specs, "rust").as_deref(), Some("rust-analyzer"));
    assert_eq!(chosen(&specs, "python").as_deref(), Some("pyright"));
    assert_eq!(chosen(&specs, "text"), None);
    registry.set_settings(LspSettings {
        typescript_server: TypescriptServer::TypescriptLanguageServer,
        ..LspSettings::default()
    });
    let specs = registry.specs();
    for language in ["javascript", "typescript", "tsx"] {
        assert_eq!(
            chosen(&specs, language).as_deref(),
            Some("typescript-language-server")
        );
    }
    assert!(specs.iter().all(|s| s.name != "typescript-go"));

    // The languageId for each language (§3.2).
    let file = |name: &str| PathBuf::from(ROOT).join(name);
    for (language, name, id) in [
        ("rust", "a.rs", "rust"),
        ("python", "a.py", "python"),
        ("javascript", "a.js", "javascript"),
        ("javascript", "a.jsx", "javascriptreact"),
        ("typescript", "a.ts", "typescript"),
        ("tsx", "a.tsx", "typescriptreact"),
    ] {
        assert_eq!(language_id(language, &file(name)), id, "{name}");
    }

    // Through the manager: a TypeScript document starts the chosen server.
    for (choice, command, args) in [
        (
            TypescriptServer::TypescriptLanguageServer,
            "typescript-language-server",
            strings(&["--stdio"]),
        ),
        (
            TypescriptServer::TypescriptGo,
            "tsc",
            strings(&["--lsp", "--stdio"]),
        ),
    ] {
        let (fake, mut manager, _events) = routed();
        manager.update_settings(&LspSettings {
            typescript_server: choice,
            ..LspSettings::default()
        });
        let mut buffer = Buffer::new("let a = 1;\n");
        let path = format!("{ROOT}/a.ts");
        open(&mut manager, root(), &mut buffer, &path, "typescript").unwrap();
        fake.wait_for_document(&uri(&path), "didOpen", |r| r.is_some());
        let launched = fake.launched();
        let [(spec, _)] = launched.as_slice() else {
            panic!("one launch: {launched:?}");
        };
        assert_eq!(
            (spec.command.to_str().unwrap(), &spec.args),
            (command, &args)
        );
        assert_eq!(
            fake.document(&uri(&path)).unwrap().language_id,
            "typescript"
        );
    }
}

#[test]
fn user_settings_and_host_specs() {
    // A user override merges onto the built-in spec.
    let mut registry = Registry::default();
    let mut settings = LspSettings::default();
    settings.servers.insert(
        "rust-analyzer".into(),
        server(json!({
            "command": "/opt/ra/rust-analyzer",
            "args": ["--verbose"],
            "settings": { "rust-analyzer": { "cargo": { "features": "all" } } },
        })),
    );
    registry.set_settings(settings.clone());
    let specs = registry.specs();
    let ra = server_for(&specs, "rust").unwrap();
    assert_eq!(ra.command, Path::new("/opt/ra/rust-analyzer"));
    assert_eq!(ra.args, ["--verbose"]);
    assert_eq!(
        ra.settings,
        json!({ "rust-analyzer": { "cargo": { "features": "all" } } })
    );
    // What it didn't set stays built in.
    assert_eq!(ra.languages, ["rust"]);
    assert_eq!(ra.readiness, ReadinessMode::ServerStatus);
    assert_eq!(ra.ready_ignores_progress, ["rust-analyzer/flycheck/"]);

    // A host spec of the same name wins over settings.
    let (fake, mut manager, _events) = routed();
    manager.update_settings(&settings);
    manager.register_spec(ServerSpec {
        languages: strings(&["rust"]),
        ..ServerSpec::new("rust-analyzer", "/host/rust-analyzer")
    });
    let mut buffer = Buffer::new("fn main() {}\n");
    let path = format!("{ROOT}/main.rs");
    let id = open(&mut manager, root(), &mut buffer, &path, "rust").unwrap();
    barrier(&manager, id);
    let launched = fake.launched();
    assert_eq!(names(&launched), ["rust-analyzer"]);
    assert_eq!(launched[0].0.command, Path::new("/host/rust-analyzer"));
    assert_eq!(launched[0].0.args, Vec::<String>::new());

    // With lsp.enabled = false, or pyright turned off, a Python document
    // launches nothing; turned back on, it gets its server.
    let mut off = LspSettings {
        enabled: false,
        ..LspSettings::default()
    };
    let mut pyright_off = LspSettings::default();
    pyright_off
        .servers
        .insert("pyright".into(), server(json!({ "enabled": false })));
    for settings in [&mut off, &mut pyright_off] {
        let (fake, mut manager, _events) = routed();
        manager.update_settings(settings);
        let mut buffer = Buffer::new("x = 1\n");
        let path = format!("{ROOT}/a.py");
        assert_eq!(
            open(&mut manager, root(), &mut buffer, &path, "python"),
            None
        );
        assert_eq!(manager.server_of(buffer.id()), None);
        // Edited while waiting: the server opens it with the latest text.
        buffer.edit([(0..0, "# hi\n")], Origin::User).unwrap();
        manager.changed(buffer.id());
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(fake.launches(), 0);

        // A new size limit is for documents opened from now on.
        manager.update_settings(&LspSettings {
            max_file_bytes: 1,
            ..LspSettings::default()
        });
        let id = manager.server_of(buffer.id()).expect("a server now");
        let replica = fake.wait_for_document(&uri(&path), "didOpen", |r| r.is_some());
        assert_eq!(replica.unwrap().text, "# hi\nx = 1\n");
        assert_eq!(names(&fake.launched()), ["pyright"]);
        let mut big = Buffer::new("y = 2\n");
        let big_path = format!("{ROOT}/big.py");
        assert_eq!(
            open(&mut manager, root(), &mut big, &big_path, "python"),
            Some(id)
        );
        barrier(&manager, id);
        assert_eq!(fake.document(&uri(&big_path)), None);
    }
}

#[test]
fn lazy_start_per_server_and_root() {
    let (fake, mut manager, events) = routed();
    manager.update_settings(&LspSettings::default());
    assert_eq!(fake.launches(), 0, "nothing before the first open");

    let mut a = Buffer::new("a = 1\n");
    let mut b = Buffer::new("b = 2\n");
    let a_path = format!("{ROOT}/a.py");
    let b_path = format!("{ROOT}/pkg/b.py");
    let pyright = open(&mut manager, root(), &mut a, &a_path, "python").unwrap();
    assert_eq!(events.status(pyright), ServerStatus::Starting);
    fake.wait_initialized();
    assert_eq!(
        open(&mut manager, root(), &mut b, &b_path, "python"),
        Some(pyright)
    );
    fake.wait_for_document(&uri(&b_path), "didOpen", |r| r.is_some());
    barrier(&manager, pyright);
    let launched = fake.launched();
    assert_eq!(names(&launched), ["pyright"]);
    assert_eq!(launched[0].1, Path::new(ROOT));

    // Closing every Python document leaves pyright running.
    manager.close(a.id());
    manager.close(b.id());
    fake.wait_for_document(&uri(&b_path), "didClose", |r| r.is_some_and(|r| !r.open));
    barrier(&manager, pyright);
    assert!(
        !fake.records().iter().any(|r| r.method() == "shutdown"),
        "{:#?}",
        fake.records()
    );

    // None for a language no server lists.
    let mut text = Buffer::new("notes\n");
    assert_eq!(
        open(
            &mut manager,
            root(),
            &mut text,
            &format!("{ROOT}/notes.txt"),
            "text"
        ),
        None
    );
    barrier(&manager, pyright);
    assert_eq!(fake.launches(), 1);

    // Another root gets its own instance.
    #[cfg(unix)]
    let (other_root, other_file) = ("/other", "/other/d.py");
    #[cfg(windows)]
    let (other_root, other_file) = (r"C:\other", r"C:\other\d.py");
    let mut other = Buffer::new("d = 4\n");
    let other_id = open(
        &mut manager,
        Some(Path::new(other_root)),
        &mut other,
        other_file,
        "python",
    )
    .unwrap();
    assert_ne!(other_id, pyright);
    barrier(&manager, other_id);
    let launched = fake.launched();
    assert_eq!(names(&launched), ["pyright", "pyright"]);
    assert_eq!(launched[1].1, Path::new(other_root));
}

const USER: &str = r#"
[lsp.servers.rust-analyzer]
command = "user-rust-analyzer"
args = ["--from-user"]
env = { FROM = "user" }
[lsp.servers.rust-analyzer.settings.rust-analyzer]
from = "user"
"#;

const PROJECT: &str = r#"
[lsp.servers.rust-analyzer]
command = "/tmp/evil"
args = ["--evil"]
env = { FROM = "project", PATH = "/tmp" }
[lsp.servers.rust-analyzer.settings]
rust-analyzer = { from = "project" }
evil = { from = "project" }
"#;

fn configuration(fake: &FakeServer, launch: usize, sections: &[Option<&str>]) -> Value {
    let items = sections
        .iter()
        .map(|section| ConfigurationItem {
            scope_uri: None,
            section: section.map(str::to_owned),
        })
        .collect();
    fake.request_at::<WorkspaceConfiguration>(launch, ConfigurationParams { items })
        .unwrap()
}

#[test]
fn project_settings_never_reach_the_server() {
    for with_user in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("user.toml");
        let project = dir.path().join("project.toml");
        std::fs::write(&user, USER).unwrap();
        std::fs::write(&project, PROJECT).unwrap();
        let mut store = SettingsStore::load(with_user.then(|| user.clone()), Some(project.clone()));
        assert!(
            store.diagnostics().iter().all(|d| !d.is_error()) && !store.diagnostics().is_empty(),
            "{:#?}",
            store.diagnostics()
        );

        let (fake, mut manager, events) = routed();
        manager.update_settings(&store.settings().lsp);
        let mut buffer = Buffer::new("fn main() {}\n");
        let path = format!("{ROOT}/main.rs");
        let id = open(&mut manager, root(), &mut buffer, &path, "rust").unwrap();
        assert_eq!(events.status(id), ServerStatus::Starting);
        fake.wait_initialized();

        let expected = if with_user {
            (
                "user-rust-analyzer",
                strings(&["--from-user"]),
                vec![("FROM".to_owned(), "user".to_owned())],
            )
        } else {
            ("rust-analyzer", vec![], vec![])
        };
        let check = |fake: &FakeServer| {
            for (spec, _) in fake.launched() {
                assert_eq!(
                    (spec.command.to_str().unwrap(), spec.args, spec.env),
                    expected.clone()
                );
            }
            let answers = configuration(fake, 1, &[Some("rust-analyzer"), Some("evil"), None]);
            let user_section = if with_user {
                json!({ "from": "user" })
            } else {
                Value::Null
            };
            assert_eq!(answers[0], user_section);
            assert_eq!(answers[1], Value::Null);
            assert!(!answers.to_string().contains("project"), "{answers}");
        };
        check(&fake);

        // A reload of the edited project file changes nothing either.
        std::fs::write(&project, PROJECT.replace("evil", "worse")).unwrap();
        let event = store.reload(std::slice::from_ref(&project));
        if let Some(event) = event {
            manager.update_settings(&event.settings.lsp);
        }
        barrier(&manager, id);
        check(&fake);
        assert_eq!(fake.launches(), 1);
        assert!(
            !fake
                .records()
                .iter()
                .any(|r| r.is_notification("workspace/didChangeConfiguration")),
        );

        // Turning the server off from the project file stops it.
        std::fs::write(
            &project,
            PROJECT.replace(
                "[lsp.servers.rust-analyzer]\n",
                "[lsp.servers.rust-analyzer]\nenabled = false\n",
            ),
        )
        .unwrap();
        let event = store
            .reload(std::slice::from_ref(&project))
            .expect("a change");
        manager.update_settings(&event.settings.lsp);
        assert_eq!(events.status(id), ServerStatus::Stopped(None));
        assert_eq!(manager.server_of(buffer.id()), None);
        let methods: Vec<String> = fake
            .records_of(1)
            .iter()
            .map(|r| r.method().to_owned())
            .collect();
        assert!(
            methods.contains(&"shutdown".to_owned()) && methods.last().is_some_and(|m| m == "exit"),
            "{methods:?}"
        );
        assert_eq!(fake.launches(), 1);
    }
}

#[test]
fn configuration_answers_and_changes() {
    let (fake, mut manager, events) = routed();
    let mut settings = LspSettings::default();
    settings.servers.insert(
        "rust-analyzer".into(),
        server(json!({
            "settings": {
                "rust-analyzer": { "cargo": { "features": "all" } },
                "a": { "b": 1 },
            },
        })),
    );
    manager.update_settings(&settings);
    let mut buffer = Buffer::new("fn main() {}\n");
    let path = format!("{ROOT}/main.rs");
    let id = open(&mut manager, root(), &mut buffer, &path, "rust").unwrap();
    assert_eq!(events.status(id), ServerStatus::Starting);
    fake.wait_initialized();
    let answers = configuration(
        &fake,
        1,
        &[
            Some("rust-analyzer"),
            Some("rust-analyzer.cargo.features"),
            Some("a.b"),
            Some("missing"),
            Some("a.b.c"),
        ],
    );
    assert_eq!(
        answers,
        json!([{ "cargo": { "features": "all" } }, "all", 1, null, null])
    );

    // Changing only the settings sends didChangeConfiguration, and the
    // answers change; nothing is launched.
    let new_settings = json!({ "rust-analyzer": { "cargo": { "features": [] } } });
    settings.servers.get_mut("rust-analyzer").unwrap().settings = new_settings.as_object().cloned();
    manager.update_settings(&settings);
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
    assert_eq!(sent, Some(new_settings));
    assert_eq!(
        configuration(&fake, 1, &[Some("rust-analyzer.cargo.features")]),
        json!([[]])
    );
    barrier(&manager, id);
    assert_eq!(fake.launches(), 1);

    // Changing args, then command, shuts it down and launches it again,
    // and the open document goes along, edits included.
    let changes: [(&str, Value); 2] = [("args", json!(["--next"])), ("command", json!("ra-next"))];
    for (launch, (key, value)) in (2..).zip(changes) {
        let server = settings.servers.get_mut("rust-analyzer").unwrap();
        match key {
            "args" => server.args = serde_json::from_value(value).unwrap(),
            _ => server.command = serde_json::from_value(value).unwrap(),
        }
        buffer.edit([(0..0, "// edited\n")], Origin::User).unwrap();
        manager.update_settings(&settings);
        fake.wait_initialized_at(launch);
        let next = manager
            .server_of(buffer.id())
            .expect("moved to the new server");
        assert_ne!(next, id);
        let text = buffer.text().to_string();
        fake.wait_for_at(launch, "the document reopened", |records| {
            records.iter().any(|r| match r {
                Record::Notification { method, params } if method == "textDocument/didOpen" => {
                    params["textDocument"]["uri"] == json!(uri(&path))
                }
                _ => false,
            })
        });
        manager.changed(buffer.id());
        fake.wait_for_document(&uri(&path), "the edit", |r| {
            r.is_some_and(|r| r.open && r.text == text)
        });
        let earlier: Vec<String> = fake
            .records_of(launch - 1)
            .iter()
            .map(|r| r.method().to_owned())
            .collect();
        assert!(earlier.contains(&"exit".to_owned()), "{earlier:?}");
        barrier(&manager, next);
    }
    let launched = fake.launched();
    assert_eq!(launched.len(), 3);
    assert_eq!(launched[1].0.args, ["--next"]);
    assert_eq!(launched[2].0.command, Path::new("ra-next"));
    assert_eq!(
        configuration(&fake, 3, &[Some("rust-analyzer.cargo.features")]),
        json!([[]])
    );

    // Restarts back to back, the second before the first has handed the
    // document on: it still reaches the last server, which launched only
    // after the one before had gone.
    let reopened = |records: &[Record]| {
        records.iter().any(|r| {
            r.is_notification("textDocument/didOpen")
                && matches!(r, Record::Notification { params, .. }
                    if params["textDocument"]["uri"] == json!(uri(&path)))
        })
    };
    for round in 0..10 {
        let last = format!("--b{round}");
        for args in [format!("--a{round}"), last.clone()] {
            settings.servers.get_mut("rust-analyzer").unwrap().args = Some(vec![args]);
            manager.update_settings(&settings);
        }
        wait_until("the last launch", || {
            fake.launched()
                .iter()
                .any(|(spec, _)| spec.args == [last.clone()])
        });
        let launch = 1 + fake
            .launched()
            .iter()
            .position(|(spec, _)| spec.args == [last.clone()])
            .unwrap();
        fake.wait_for_at(launch, "the document reopened", reopened);
        let server = manager.server_of(buffer.id()).unwrap();
        barrier(&manager, server);
        assert_eq!(fake.launches(), launch);
    }
    assert_launches_follow_exits(&fake);

    // A server restarted with no documents open is launched again too.
    manager.close(buffer.id());
    let before = fake.launches();
    settings.servers.get_mut("rust-analyzer").unwrap().args = Some(strings(&["--empty"]));
    manager.update_settings(&settings);
    wait_until("the relaunch", || fake.launches() == before + 1);
    fake.wait_initialized_at(before + 1);
    assert_eq!(fake.launched()[before].0.args, ["--empty"]);
}

/// Asserts each launch's `initialize` came after the launch before had
/// been sent `exit`. Receiving `initialize` isn't completing the
/// handshake: a launch superseded before the client consumed its reply is
/// killed without `exit`, however its `initialize` was recorded. So only a
/// predecessor that sent `initialized` is held to having been sent `exit`,
/// and only before a successor that did initialize.
fn assert_launches_follow_exits(fake: &FakeServer) {
    let timeline = fake.timeline();
    let at = |launch: usize, method: &str| {
        timeline
            .iter()
            .position(|(from, r)| *from == launch && r.method() == method)
    };
    for launch in 2..=fake.launches() {
        let (Some(initialize), Some(_)) = (at(launch, "initialize"), at(launch - 1, "initialized"))
        else {
            continue;
        };
        let exit = at(launch - 1, "exit");
        assert!(
            exit.is_some_and(|exit| exit < initialize),
            "launch {launch}: exit {exit:?}, initialize {initialize:?}"
        );
    }
}

/// A launch superseded after its `initialize` was recorded but before its
/// reply completed is killed without `exit`, and that is no violation.
#[test]
fn a_launch_superseded_mid_handshake_needs_no_exit() {
    let fake_clock = Arc::new(ManualClock::new());
    let fake = FakeServer::with_clock(
        Script {
            initialize_delay: Duration::from_secs(1),
            ..Script::default()
        },
        fake_clock.clone(),
    );
    let (mut manager, _events) = manager(fake.launcher(), Some(Arc::new(ManualClock::new())));
    let mut buffer = Buffer::new("fn main() {}\n");
    let path = format!("{ROOT}/main.rs");
    open(&mut manager, root(), &mut buffer, &path, "rust").unwrap();
    fake.wait_for("the first initialize", |records| {
        records.iter().any(|r| r.method() == "initialize")
    });
    // Both launches share the clock, and the predecessor's timer may outlive
    // its kill. Age it, so only the successor's timer has a full second left.
    fake_clock.wait_for_sleep(Duration::from_secs(1));
    fake_clock.advance(Duration::from_millis(100));
    let mut settings = LspSettings::default();
    settings.servers.insert(
        "rust-analyzer".into(),
        server(json!({ "args": ["--next"] })),
    );
    manager.update_settings(&settings);
    wait_until("the successor", || fake.launches() == 2);
    fake.wait_for_at(2, "the second initialize", |records| {
        records.iter().any(|r| r.method() == "initialize")
    });
    fake_clock.wait_for_sleep(Duration::from_secs(1));
    fake_clock.advance(Duration::from_secs(1));
    fake.wait_initialized_at(2);
    let first: Vec<_> = fake
        .records_of(1)
        .iter()
        .map(|r| r.method().to_owned())
        .collect();
    assert!(first.contains(&"initialize".to_owned()), "{first:?}");
    assert!(!first.contains(&"initialized".to_owned()), "{first:?}");
    assert!(!first.contains(&"exit".to_owned()), "{first:?}");
    assert_launches_follow_exits(&fake);
}

#[test]
fn a_restart_waits_for_the_old_server_to_go() {
    let clock = Arc::new(ManualClock::new());
    let fake = FakeServer::new(Script {
        answer_shutdown: false,
        ..Script::default()
    });
    let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
    let mut settings = LspSettings::default();
    manager.update_settings(&settings);
    let mut buffer = Buffer::new("fn main() {}\n");
    let path = format!("{ROOT}/main.rs");
    let first = open(&mut manager, root(), &mut buffer, &path, "rust").unwrap();
    assert_eq!(events.status(first), ServerStatus::Starting);
    fake.wait_initialized();

    // The old server waits for its shutdown answer: nothing is launched,
    // and a second change meanwhile replaces the first.
    let mut server = server(json!({ "args": ["--one"] }));
    settings
        .servers
        .insert("rust-analyzer".into(), server.clone());
    manager.update_settings(&settings);
    clock.wait_for_sleep(crate::server::SHUTDOWN_TIMEOUT);
    server.args = Some(strings(&["--two"]));
    settings.servers.insert("rust-analyzer".into(), server);
    manager.update_settings(&settings);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fake.launches(), 1);

    // Once it has gone, the latest spec launches, with the document.
    clock.advance(crate::server::SHUTDOWN_TIMEOUT);
    fake.wait_initialized_at(2);
    fake.wait_for_document(&uri(&path), "didOpen", |r| r.is_some_and(|r| r.launch == 2));
    assert_eq!(fake.launched()[1].0.args, ["--two"]);
    let server = manager.server_of(buffer.id()).unwrap();
    barrier(&manager, server);
    assert_eq!(fake.launches(), 2);
    assert_launches_follow_exits(&fake);
}

#[test]
fn a_restart_waits_for_a_killed_server_to_exit() {
    let clock = Arc::new(ManualClock::new());
    let fake = FakeServer::new(Script {
        on_exit: crate::fake::OnExit::Ignore,
        ..Script::default()
    });
    fake.hold_kills(true);
    let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
    let mut settings = LspSettings::default();
    manager.update_settings(&settings);
    let mut buffer = Buffer::new("fn main() {}\n");
    let path = format!("{ROOT}/main.rs");
    let first = open(&mut manager, root(), &mut buffer, &path, "rust").unwrap();
    assert_eq!(events.status(first), ServerStatus::Starting);
    fake.wait_initialized();

    // The old server ignores `exit`, so it's killed, and takes a while to
    // go: nothing is launched meanwhile.
    settings.servers.insert(
        "rust-analyzer".into(),
        server(json!({ "args": ["--next"] })),
    );
    manager.update_settings(&settings);
    clock.wait_for_sleep(crate::server::EXIT_TIMEOUT);
    clock.advance(crate::server::EXIT_TIMEOUT);
    wait_until("the kill", || fake.kills() == 1);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fake.launches(), 1);

    fake.hold_kills(false);
    fake.wait_initialized_at(2);
    fake.wait_for_document(&uri(&path), "didOpen", |r| r.is_some_and(|r| r.launch == 2));
    assert_eq!(fake.launched()[1].0.args, ["--next"]);
}

/// Refuses to launch a command named `missing`, as a launcher does when
/// it isn't installed; launches the fake otherwise.
struct MissingOrFake(Arc<crate::fake::FakeLauncher>);

impl ServerLauncher for MissingOrFake {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> std::io::Result<ServerIo> {
        assert_ne!(spec.command, Path::new("panic"), "the launcher panicked");
        if spec.command == Path::new("missing") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "missing not found",
            ));
        }
        self.0.launch(spec, root, cancel)
    }
}

#[test]
fn a_fixed_command_brings_back_the_documents_of_a_failed_server() {
    let fake = FakeServer::new(Script::default());
    let launcher = Arc::new(MissingOrFake(fake.launcher()));
    let (mut manager, events) = manager(launcher, Some(Arc::new(ManualClock::new())));
    let mut settings = LspSettings::default();
    settings
        .servers
        .insert("pyright".into(), server(json!({ "command": "missing" })));
    manager.update_settings(&settings);
    let mut a = Buffer::new("a = 1\n");
    let a_path = format!("{ROOT}/a.py");
    let failed = open(&mut manager, root(), &mut a, &a_path, "python").unwrap();
    assert_eq!(events.status(failed), ServerStatus::Starting);
    assert_eq!(
        events.status(failed),
        ServerStatus::Stopped(Some("pyright not found".into()))
    );
    // It waits, and edits meanwhile are drained as they're nudged.
    wait_until("the document to wait", || {
        manager.server_of(a.id()).is_none()
    });
    a.edit([(0..0, "# edited\n")], Origin::User).unwrap();
    manager.changed(a.id());

    // Fixing the command starts it again, with the document as it is now.
    settings.servers.get_mut("pyright").unwrap().command = Some("pyright-langserver".into());
    manager.update_settings(&settings);
    let replica = fake.wait_for_document(&uri(&a_path), "didOpen", |r| r.is_some());
    assert_eq!(replica.unwrap().text, "# edited\na = 1\n");
    let server = manager.server_of(a.id()).unwrap();
    assert_ne!(server, failed);
    barrier(&manager, server);
    assert_eq!(fake.launches(), 1);

    // After another failure, a document opened once the server can start
    // takes the one waiting along.
    let mut b = Buffer::new("b = 2\n");
    let b_path = format!("{ROOT}/b.py");
    settings.servers.get_mut("pyright").unwrap().command = Some("missing".into());
    manager.update_settings(&settings);
    let failed = manager.server_of(a.id()).unwrap();
    assert_eq!(events.status(failed), ServerStatus::Starting);
    assert_eq!(
        events.status(failed),
        ServerStatus::Stopped(Some("pyright not found".into()))
    );
    manager.register_spec(ServerSpec {
        languages: strings(&["python"]),
        ..ServerSpec::new("pyright", "host-pyright")
    });
    let server = open(&mut manager, root(), &mut b, &b_path, "python").unwrap();
    let records = fake.wait_for_at(2, "both documents", |records| {
        records
            .iter()
            .filter(|r| r.is_notification("textDocument/didOpen"))
            .count()
            == 2
    });
    assert!(!records.is_empty());
    assert_eq!(manager.server_of(a.id()), Some(server));

    // Stopping a failed server, then closing its waiting document, lets go
    // of the document.
    manager.register_spec(ServerSpec {
        languages: strings(&["python"]),
        ..ServerSpec::new("pyright", "missing")
    });
    let failed = manager.server_of(a.id()).unwrap();
    assert_eq!(events.status(failed), ServerStatus::Starting);
    assert_eq!(
        events.status(failed),
        ServerStatus::Stopped(Some("pyright not found".into()))
    );
    wait_until("the document to wait", || {
        manager.server_of(a.id()).is_none()
    });
    manager.stop(failed);
    assert!(manager.is_subscribed(a.id()), "it still waits");
    manager.close(a.id());
    assert!(!manager.is_subscribed(a.id()));
    assert!(a.edit([(0..0, "x")], Origin::User).is_some());

    // A launcher that panics: the document waits all the same, and gets
    // its server once one can start.
    let mut c = Buffer::new("c = 3\n");
    let c_path = format!("{ROOT}/c.py");
    manager.register_spec(ServerSpec {
        languages: strings(&["python"]),
        ..ServerSpec::new("pyright", "panic")
    });
    let panicked = open(&mut manager, root(), &mut c, &c_path, "python").unwrap();
    assert_eq!(events.status(panicked), ServerStatus::Starting);
    assert_eq!(
        events.status(panicked),
        ServerStatus::Stopped(Some("fe-lsp's server thread panicked".into()))
    );
    wait_until("the document to wait", || {
        manager.server_of(c.id()).is_none()
    });
    manager.register_spec(ServerSpec {
        languages: strings(&["python"]),
        ..ServerSpec::new("pyright", "pyright-langserver")
    });
    fake.wait_for_document(&uri(&c_path), "didOpen", |r| r.is_some());
    assert!(manager.server_of(c.id()).is_some());
}

/// Documents opened while their server fails to launch, or its launcher
/// panics, all reach the server that can start: an open still on its way
/// into a failing server's commands as it hands its documents on goes
/// with them. The race is in the channel, so this opens many documents
/// while instances keep failing.
#[test]
fn documents_opened_as_their_server_fails_are_handed_on() {
    const DOCUMENTS: usize = 400;
    for command in ["missing", "panic"] {
        let fake = FakeServer::new(Script::default());
        let launcher = Arc::new(MissingOrFake(fake.launcher()));
        let (mut manager, _events) = manager(launcher, Some(Arc::new(ManualClock::new())));
        manager.register_spec(ServerSpec {
            languages: strings(&["python"]),
            ..ServerSpec::new("pyright", command)
        });
        let mut buffers: Vec<Buffer> = (0..DOCUMENTS)
            .map(|i| Buffer::new(&format!("x = {i}\n")))
            .collect();
        for (i, buffer) in buffers.iter_mut().enumerate() {
            open(
                &mut manager,
                root(),
                buffer,
                &format!("{ROOT}/{i}.py"),
                "python",
            );
        }
        manager.register_spec(ServerSpec {
            languages: strings(&["python"]),
            ..ServerSpec::new("pyright", "pyright-langserver")
        });
        let opened = || {
            fake.records()
                .iter()
                .filter(|r| r.is_notification("textDocument/didOpen"))
                .count()
        };
        wait_until(&format!("every document opened ({command})"), || {
            opened() == DOCUMENTS
        });
        assert!(buffers.iter().all(|b| manager.is_subscribed(b.id())));
    }
}

/// A manual clock that can hold the thread polling one of its sleeps: the
/// `nth` poll of a sleep of `length` blocks until released. An actor held
/// there is between two branches of its loop.
struct HoldingClock {
    clock: ManualClock,
    hold: Arc<Hold>,
}

#[derive(Default)]
struct Hold {
    state: Mutex<Holding>,
    changed: Condvar,
}

#[derive(Default)]
struct Holding {
    armed: Option<(Duration, usize)>,
    polls: usize,
    held: bool,
    released: bool,
}

impl Hold {
    fn arm(&self, length: Duration, nth: usize) {
        *self.state.lock().unwrap() = Holding {
            armed: Some((length, nth)),
            ..Holding::default()
        };
    }

    /// Returns once a thread is held.
    fn wait_held(&self) {
        let state = self.state.lock().unwrap();
        let (state, waited) = self
            .changed
            .wait_timeout_while(state, WAIT, |state| !state.held)
            .unwrap();
        drop(state);
        assert!(!waited.timed_out(), "nothing was held");
    }

    fn release(&self) {
        self.state.lock().unwrap().released = true;
        self.changed.notify_all();
    }
}

impl Clock for HoldingClock {
    fn now(&self) -> Instant {
        self.clock.now()
    }

    fn sleep(&self, length: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(HeldSleep {
            sleep: self.clock.sleep(length),
            length,
            hold: self.hold.clone(),
        })
    }
}

struct HeldSleep {
    sleep: Pin<Box<dyn Future<Output = ()> + Send>>,
    length: Duration,
    hold: Arc<Hold>,
}

impl Future for HeldSleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let hold = self.hold.clone();
        let mut state = hold.state.lock().unwrap();
        let due = match state.armed {
            Some((length, nth)) if length == self.length => {
                state.polls += 1;
                state.polls == nth
            }
            _ => false,
        };
        if due {
            state.armed = None;
            state.held = true;
            hold.changed.notify_all();
            drop(
                hold.changed
                    .wait_timeout_while(state, WAIT, |state| !state.released)
                    .unwrap(),
            );
        } else {
            drop(state);
        }
        self.sleep.as_mut().poll(cx)
    }
}

/// A restart can drop the last sender of a running server's commands
/// while its actor is between looking for a stop and reading them: it
/// finds them ended rather than the stop, and still hands its documents on
/// to the new instance.
#[test]
fn a_restart_ending_the_commands_hands_the_documents_on() {
    // A server of its own, so its thread's name is this test's alone.
    let thread = "fe-lsp held-server";
    capture_logs(thread);
    let held = |args: &[&str]| ServerSpec {
        args: strings(args),
        languages: strings(&["held"]),
        ..ServerSpec::new("held-server", "held-server")
    };
    let hold = Arc::new(Hold::default());
    let fake = FakeServer::new(Script::default());
    let clock = HoldingClock {
        clock: ManualClock::new(),
        hold: hold.clone(),
    };
    let (mut manager, _events) = manager(fake.launcher(), Some(Arc::new(clock)));
    manager.register_spec(held(&[]));
    // The backstop tick is polled after the stop and before the commands:
    // the second time is in the pass after the one sending `didOpen`.
    hold.arm(BACKSTOP, 2);
    let mut a = Buffer::new("a = 1\n");
    let a_path = format!("{ROOT}/a.held");
    let first = open(&mut manager, root(), &mut a, &a_path, "held").unwrap();
    hold.wait_held();
    manager.register_spec(held(&["--next"]));
    let next = manager.server_of(a.id()).unwrap();
    assert_ne!(next, first);
    hold.release();
    fake.wait_initialized_at(2);
    a.edit([(0..0, "# edited\n")], Origin::User).unwrap();
    manager.changed(a.id());
    barrier(&manager, next);
    let replica = fake.wait_for_document(&uri(&a_path), "a on the next launch", |r| {
        r.is_some_and(|r| r.launch == 2)
    });
    assert_eq!(replica.unwrap().text, "# edited\na = 1\n");
    assert!(manager.is_subscribed(a.id()));
    // It went through the end of its commands, not its stop.
    let logs = logs_from(thread);
    assert!(
        logs.iter()
            .any(|log| log == "held-server: its commands ended"),
        "{logs:#?}"
    );
}

/// An instance stopped after it has started but before it launches
/// launches nothing, so changes made back to back launch only the last
/// spec.
#[test]
fn an_instance_stopped_before_it_launches_launches_nothing() {
    let fake = FakeServer::new(Script::default());
    let (mut manager, events) = manager(fake.launcher(), Some(Arc::new(ManualClock::new())));
    // The first instance's thread is held as it says it's starting.
    let (held, holding) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    events.probe(move |event| {
        if let LspEvent::Status {
            server: ServerId(0),
            status: ServerStatus::Starting,
            ..
        } = event
        {
            let _ = held.send(());
            let _ = released.recv_timeout(WAIT);
        }
    });
    let mut a = Buffer::new("a = 1\n");
    let a_path = format!("{ROOT}/a.py");
    assert_eq!(
        open(&mut manager, root(), &mut a, &a_path, "python"),
        Some(ServerId(0))
    );
    holding.recv_timeout(WAIT).unwrap();
    let mut settings = LspSettings::default();
    settings
        .servers
        .insert("pyright".into(), server(json!({ "args": ["--next"] })));
    manager.update_settings(&settings);
    release.send(()).unwrap();
    let replica = fake.wait_for_document(&uri(&a_path), "a opened", |r| r.is_some());
    assert_eq!(replica.unwrap().launch, 1);
    let launched = fake.launched();
    assert_eq!(names(&launched), ["pyright"]);
    assert_eq!(launched[0].0.args, strings(&["--next"]));
}

/// Hands launches to the fake, the first only once released.
struct HeldLaunch {
    fake: Arc<crate::fake::FakeLauncher>,
    entered: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl ServerLauncher for HeldLaunch {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> std::io::Result<ServerIo> {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _ = entered.send(());
            let _ = self.release.lock().unwrap().recv_timeout(WAIT);
        }
        self.fake.launch(spec, root, cancel)
    }
}

/// A stop that comes while an instance launches (a launch may block) kills
/// what it launched before anything is sent to it, so its successor
/// doesn't wait for an `initialize` answer first.
#[test]
fn an_instance_stopped_while_it_launches_is_killed_before_initialize() {
    // The fake's clock never moves, so it never answers `initialize`, and
    // the manager's never times it out: an instance that started
    // initializing would hold its successor for good.
    let fake = FakeServer::with_clock(
        Script {
            initialize_delay: Duration::from_secs(1),
            ..Script::default()
        },
        Arc::new(ManualClock::new()),
    );
    let (entered, entering) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let launcher = Arc::new(HeldLaunch {
        fake: fake.launcher(),
        entered: Mutex::new(Some(entered)),
        release: Mutex::new(released),
    });
    let (mut manager, _events) = manager(launcher, Some(Arc::new(ManualClock::new())));
    let mut a = Buffer::new("a = 1\n");
    let a_path = format!("{ROOT}/a.py");
    open(&mut manager, root(), &mut a, &a_path, "python").unwrap();
    entering.recv_timeout(WAIT).unwrap();
    let mut settings = LspSettings::default();
    settings
        .servers
        .insert("pyright".into(), server(json!({ "args": ["--next"] })));
    manager.update_settings(&settings);
    release.send(()).unwrap();
    wait_until("the next spec launched", || fake.launched().len() == 2);
    let launched = fake.launched();
    assert_eq!(launched[0].0.args, strings(&["--stdio"]));
    assert_eq!(launched[1].0.args, strings(&["--next"]));
    assert_eq!(fake.kills(), 1);
}

/// Fails its first launch, and hands the later ones to the fake.
struct FailsFirst(Arc<crate::fake::FakeLauncher>, AtomicBool);

impl ServerLauncher for FailsFirst {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> std::io::Result<ServerIo> {
        if !self.1.swap(true, Ordering::SeqCst) {
            return Err(std::io::Error::other("the first launch fails"));
        }
        self.0.launch(spec, root, cancel)
    }
}

/// Documents placed on an instance that fails while others are still
/// being placed go to the instance started after it, rather than wait
/// beside it. The first instance fails at once, so with this many
/// documents it has usually gone before they're all placed.
#[test]
fn documents_bounced_off_a_failed_instance_reach_its_replacement() {
    const DOCUMENTS: usize = 2000;
    let fake = FakeServer::new(Script::default());
    let launcher = Arc::new(FailsFirst(fake.launcher(), AtomicBool::new(false)));
    let (mut manager, _events) = manager(launcher, Some(Arc::new(ManualClock::new())));
    manager.update_settings(&LspSettings {
        enabled: false,
        ..LspSettings::default()
    });
    let mut buffers: Vec<Buffer> = (0..DOCUMENTS)
        .map(|i| Buffer::new(&format!("x = {i}\n")))
        .collect();
    for (i, buffer) in buffers.iter_mut().enumerate() {
        let path = format!("{ROOT}/{i}.py");
        assert_eq!(open(&mut manager, root(), buffer, &path, "python"), None);
    }
    manager.update_settings(&LspSettings::default());
    let replaced = buffers
        .iter()
        .any(|b| manager.server_of(b.id()).is_some_and(|s| s != ServerId(0)));
    if replaced {
        wait_until("every document opened on the second instance", || {
            fake.records()
                .iter()
                .filter(|r| r.is_notification("textDocument/didOpen"))
                .count()
                == DOCUMENTS
        });
    }
}

/// Writes an executable `#!/bin/sh` script.
#[cfg(unix)]
fn script(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Writes a stand-in shell running `body`, which first notes the file it
/// was given for its `PATH` in `files`, next to it.
#[cfg(unix)]
fn shell_script(shell: &Path, body: &str) {
    script(
        shell,
        &format!("printf '%s\\n' \"$FASTEDIT_LOGIN_PATH_FILE\" >> \"${{0%/*}}/files\"\n{body}"),
    );
}

/// Writes a stand-in login shell: it runs `profile`, then the command it's
/// given (`-l -c <command>`) with `PATH` set to `path`, then `logout`, and
/// exits as the command did.
#[cfg(unix)]
fn login_shell(shell: &Path, profile: &str, path: &str, logout: &str) {
    shell_script(
        shell,
        &format!("{profile}\nPATH='{path}' /bin/sh -c \"$3\"\nstatus=$?\n{logout}\nexit $status"),
    );
}

/// Writes `ping`, expects it back, and kills the server.
#[cfg(unix)]
fn ping(io: ServerIo) {
    let ServerIo {
        mut input,
        mut output,
        mut process,
        ..
    } = io;
    async_io::block_on(async {
        input.write_all(b"ping\n").await.unwrap();
        input.flush().await.unwrap();
        let mut echoed = [0; 5];
        output.read_exact(&mut echoed).await.unwrap();
        assert_eq!(&echoed, b"ping\n");
    });
    process.kill();
    async_io::block_on(process.exited());
}

#[test]
// Its login shells and servers are shell scripts made executable by mode.
#[cfg(unix)]
fn bare_commands_use_the_login_shell_path() {
    let thread = std::thread::current().name().unwrap().to_owned();
    capture_logs(&thread);
    let dir = tempfile::tempdir().unwrap();
    let dir = dunce::canonicalize(dir.path()).unwrap();
    let bin = dir.join("bin");
    std::fs::create_dir(&bin).unwrap();
    script(
        &bin.join("fe-test-server"),
        "printf %s \"$PATH\" > \"$PWD/server-path.txt\"; exec cat",
    );
    let login_path = format!("/nonexistent:{}:/bin:/usr/bin", bin.display());
    let shell = dir.join("login-shell");
    // What its profile and logout script print, lines that look like a
    // PATH among it, isn't taken for its PATH.
    login_shell(
        &shell,
        &format!(
            "echo run >> '{}/runs'\necho 'profile chatter'\necho /usr/local/bin",
            dir.display()
        ),
        &login_path,
        "printf '/bin\\nbye'",
    );

    // Several launches of a bare command run the shell once.
    let login = Arc::new(LoginPath::new(&shell));
    let launcher = LocalLauncher::with_login_path(login.clone());
    let spec = ServerSpec::new("test", "fe-test-server");
    for _ in 0..3 {
        ping(launcher.launch(&spec, &dir, &LaunchCancel::new()).unwrap());
    }
    let runs = std::fs::read_to_string(dir.join("runs")).unwrap();
    assert_eq!(runs.lines().count(), 1, "{runs}");
    assert_eq!(login.path(), Some(OsStr::new(&login_path)));
    assert_eq!(
        login.resolve(Path::new("fe-test-server"), None, &dir),
        Some(bin.join("fe-test-server"))
    );
    // Only that PATH: `cat` on this process's PATH isn't on it.
    let only_bin = dir.join("only-bin-shell");
    login_shell(&only_bin, "", bin.to_str().unwrap(), "");
    let narrow = LoginPath::new(&only_bin);
    assert_eq!(narrow.resolve(Path::new("cat"), None, &dir), None);
    // The server gets that PATH too.
    assert_eq!(
        std::fs::read_to_string(dir.join("server-path.txt")).unwrap(),
        login_path
    );
    // A spec's own PATH wins.
    assert_eq!(
        login.resolve(
            Path::new("fe-test-server"),
            Some(OsStr::new("/nowhere")),
            &dir
        ),
        None
    );
    // Not found anywhere: a launch error, not a lookup on some other PATH.
    let missing = ServerSpec::new("missing", "fe-no-such-server");
    match launcher.launch(&missing, &dir, &LaunchCancel::new()) {
        Err(err) => {
            assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
            assert_eq!(err.to_string(), "fe-no-such-server not found");
        }
        Ok(_) => panic!("launched a command that isn't anywhere"),
    }

    // An absolute command is used as it is, and gets the login PATH too.
    let unused = Arc::new(LoginPath::new(dir.join("missing-shell")));
    let absolute = ServerSpec::new("absolute", bin.join("fe-test-server"));
    assert_eq!(
        unused.resolve(&absolute.command, None, &dir),
        Some(absolute.command.clone())
    );
    std::fs::remove_file(dir.join("server-path.txt")).unwrap();
    ping(
        launcher
            .launch(&absolute, &dir, &LaunchCancel::new())
            .unwrap(),
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("server-path.txt")).unwrap(),
        login_path
    );
    assert_eq!(logs_from(&thread), Vec::<String>::new());

    // A leading `~` is the home directory, as bash has it, in a PATH entry
    // and in a command: never a directory named `~` under the root.
    let home = dir.join("home");
    for under in [&home, &dir.join("~")] {
        std::fs::create_dir_all(under.join("bin")).unwrap();
    }
    script(&home.join("bin/fe-home-server"), "exec cat");
    script(&dir.join("~/bin/fe-home-server"), "exit 1");
    let homed = Arc::new(LoginPath::with_home(
        dir.join("missing-shell"),
        Some(home.clone()),
    ));
    let at_home = Some(home.join("bin/fe-home-server"));
    let tilde_path = Some(OsStr::new("~/bin"));
    let name = Path::new("fe-home-server");
    assert_eq!(homed.resolve(name, tilde_path, &dir), at_home);
    let tilde = Path::new("~/bin/fe-home-server");
    assert_eq!(homed.resolve(tilde, None, &dir), at_home);
    let mut spec_at_home = ServerSpec::new("home", tilde);
    spec_at_home.env = vec![("PATH".into(), "/bin:/usr/bin".into())];
    ping(
        LocalLauncher::with_login_path(homed.clone())
            .launch(&spec_at_home, &dir, &LaunchCancel::new())
            .unwrap(),
    );
    // With no home, they're nowhere.
    let homeless = LoginPath::with_home(dir.join("missing-shell"), None);
    assert_eq!(homeless.resolve(name, tilde_path, &dir), None);
    assert_eq!(homeless.resolve(tilde, None, &dir), None);
    // Another relative command is under the root, and `.` is a name, not
    // found on a PATH as a file.
    assert_eq!(
        homed.resolve(Path::new("bin/fe-test-server"), None, &dir),
        Some(bin.join("fe-test-server"))
    );
    assert_eq!(
        homed.resolve(Path::new("."), Some(OsStr::new("/bin")), &dir),
        None
    );

    // A shell that fails, or takes over 2 s, leaves this process's PATH
    // in use, and says why.
    let failing = dir.join("failing-shell");
    shell_script(&failing, "exit 3");
    let slow = dir.join("slow-shell");
    shell_script(
        &slow,
        "sleep 10 &\necho $! > \"${0%/*}/slow-pids\"\necho $$ >> \"${0%/*}/slow-pids\"\nexec sleep 10",
    );
    // Prints what looks like a PATH, but never runs the command.
    let chatty = dir.join("chatty-shell");
    shell_script(&chatty, "printf '/bin\\nbye'");
    for (shell, why) in [
        (failing, "failed"),
        (slow, "took over 2 s"),
        (chatty, "ended without writing its PATH"),
    ] {
        let login = Arc::new(LoginPath::new(&shell));
        let started = Instant::now();
        // Not on this process's PATH, which is used now.
        assert_eq!(login.resolve(Path::new("fe-test-server"), None, &dir), None);
        let took = started.elapsed();
        assert!(took < Duration::from_secs(5), "{took:?}");
        assert_eq!(login.path(), None);
        let logs = logs_from(&thread);
        assert!(
            logs.last().is_some_and(|log| log.contains(why)
                && log.contains(shell.to_str().unwrap())
                && log.contains("Tenmoku's own PATH")),
            "{logs:#?}"
        );
        // `cat` is found on this process's PATH, which servers get.
        let launcher = LocalLauncher::with_login_path(login.clone());
        let cat = ServerSpec::new("cat", "cat");
        ping(launcher.launch(&cat, &dir, &LaunchCancel::new()).unwrap());
        ping(
            launcher
                .launch(&absolute, &dir, &LaunchCancel::new())
                .unwrap(),
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("server-path.txt")).unwrap(),
            std::env::var("PATH").unwrap()
        );
        assert_eq!(logs_from(&thread).len(), logs.len(), "the shell ran once");
    }
    // The slow shell's whole group was killed.
    let pids = std::fs::read_to_string(dir.join("slow-pids")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    for pid in pids.lines() {
        let pid = rustix::process::Pid::from_raw(pid.parse().unwrap()).unwrap();
        while rustix::process::test_kill_process(pid).is_ok() {
            assert!(Instant::now() < deadline, "{pid:?} outlived the timeout");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    // Relative and empty entries are skipped: the server's working
    // directory is the workspace, which may be untrusted.
    let relative = dir.join("relative-shell");
    let relative_path = format!("bin::{}:/bin:/usr/bin", dir.join("elsewhere").display());
    login_shell(&relative, "", &relative_path, "");
    script(
        &dir.join("local-server"),
        "printf %s \"$PATH\" > \"$PWD/server-path.txt\"; exec /bin/cat",
    );
    std::fs::create_dir(dir.join("elsewhere")).unwrap();
    script(&dir.join("elsewhere/fe-test-server"), "exit 1");
    let login = Arc::new(LoginPath::new(&relative));
    assert_eq!(login.path(), Some(OsStr::new(&relative_path)));
    // `bin` is passed over, and the absolute entry after it is found.
    assert_eq!(
        login.resolve(Path::new("fe-test-server"), None, &dir),
        Some(dir.join("elsewhere/fe-test-server"))
    );
    assert_eq!(login.resolve(Path::new("local-server"), None, &dir), None);
    let launcher = LocalLauncher::with_login_path(login);
    match launcher.launch(
        &ServerSpec::new("local", "local-server"),
        &dir,
        &LaunchCancel::new(),
    ) {
        Err(err) => assert_eq!(err.to_string(), "local-server not found"),
        Ok(_) => panic!("local-server was found through an empty entry"),
    }
    // A PATH of relative entries only is a PATH too, and finds nothing.
    let relative_only = dir.join("relative-only-shell");
    login_shell(&relative_only, "", "bin", "");
    let login = LoginPath::new(&relative_only);
    assert_eq!(login.path(), Some(OsStr::new("bin")));
    assert_eq!(login.resolve(Path::new("fe-test-server"), None, &dir), None);

    // So is an empty one: a single empty entry, which is skipped. This
    // process's PATH isn't searched.
    let empty = dir.join("empty-shell");
    login_shell(&empty, "", "", "");
    let login = Arc::new(LoginPath::new(&empty));
    assert_eq!(login.path(), Some(OsStr::new("")));
    assert_eq!(login.resolve(Path::new("cat"), None, &dir), None);
    assert_eq!(login.resolve(Path::new("fe-test-server"), None, &dir), None);
    assert_eq!(login.resolve(Path::new("local-server"), None, &dir), None);
    let launcher = LocalLauncher::with_login_path(login);
    match launcher.launch(
        &ServerSpec::new("local", "local-server"),
        &dir,
        &LaunchCancel::new(),
    ) {
        Err(err) => assert_eq!(err.to_string(), "local-server not found"),
        Ok(_) => panic!("local-server was found through an empty PATH"),
    }
    match launcher.launch(&ServerSpec::new("cat", "cat"), &dir, &LaunchCancel::new()) {
        Err(err) => assert_eq!(err.to_string(), "cat not found"),
        Ok(_) => panic!("cat was looked up on this process's PATH"),
    }
    // A file found first that this user may not run is passed over, as a
    // shell would, though it has an execute bit: one only its group may
    // run, and, on macOS, one an ACL denies. (Linux ACLs can't deny the
    // owner what its mode bits allow.)
    let mut shells = 9;
    if !rustix::process::geteuid().is_root() {
        let group_only = dir.join("group-only");
        let denied = dir.join("denied");
        let mut unrunnable = vec![&group_only];
        if cfg!(target_os = "macos") {
            unrunnable.push(&denied);
        }
        for folder in &unrunnable {
            std::fs::create_dir(folder).unwrap();
            script(&folder.join("fe-test-server"), "exit 1");
        }
        std::fs::set_permissions(
            group_only.join("fe-test-server"),
            std::fs::Permissions::from_mode(0o610),
        )
        .unwrap();
        if cfg!(target_os = "macos") {
            let acl = std::process::Command::new("/bin/chmod")
                .args(["+a", "everyone deny execute"])
                .arg(denied.join("fe-test-server"))
                .status()
                .unwrap();
            assert!(acl.success());
        }
        let passing = dir.join("passing-shell");
        let path = std::env::join_paths(unrunnable.iter().copied().chain([&bin]))
            .unwrap()
            .into_string()
            .unwrap()
            + ":/bin:/usr/bin";
        login_shell(&passing, "", &path, "");
        shells += 1;
        let login = Arc::new(LoginPath::new(&passing));
        assert_eq!(
            login.resolve(Path::new("fe-test-server"), None, &dir),
            Some(bin.join("fe-test-server"))
        );
        ping(
            LocalLauncher::with_login_path(login)
                .launch(&spec, &dir, &LaunchCancel::new())
                .unwrap(),
        );
    }
    // Line breaks in it are kept, only the one after it is dropped.
    let newlines = dir.join("newlines-shell");
    login_shell(&newlines, "", "bin\n:/x\n", "");
    let login = LoginPath::new(&newlines);
    assert_eq!(login.path(), Some(OsStr::new("bin\n:/x\n")));
    // Only the failing shells said anything.
    assert_eq!(logs_from(&thread).len(), 3);

    // Each run's file is gone, however it ended.
    let files = std::fs::read_to_string(dir.join("files")).unwrap();
    assert_eq!(files.lines().count(), shells, "{files}");
    for file in files.lines() {
        assert!(Path::new(file).is_absolute(), "{file}");
        assert!(!Path::new(file).exists(), "{file} is left");
    }
}
