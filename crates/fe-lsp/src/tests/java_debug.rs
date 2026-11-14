//! What debugging Java needs of jdtls (debugger D21): the host's bundles
//! in its `initializationOptions`, and `workspace/executeCommand` for a
//! root's server, refused for a command the server doesn't list.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fe_buffer::Buffer;
use fe_config::settings::{LspServerSettings, LspSettings};
use lsp_types::{ExecuteCommandOptions, PositionEncodingKind};
use serde_json::{Value, json};

use super::{Events, ROOT, answer, manager};
use crate::fake::{
    Echo, FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::registry::Registry;
use crate::{
    BundlesRefused, JdtlsBundles, LspError, LspManager, ManualClock, ServerId, ServerStatus,
};

const JDTLS: &str = "jdtls";
const START: &str = "vscode.java.startDebugSession";
const MAIN_CLASS: &str = "vscode.java.resolveMainClass";
const CLASSPATH: &str = "vscode.java.resolveClasspath";
const EXECUTE: &str = "workspace/executeCommand";
const DEBUG_JAR: &str = "/opt/java-debug/com.microsoft.java.debug.plugin-0.53.1.jar";

/// A script whose server lists `commands` in `executeCommandProvider`.
fn listing(commands: &[&str]) -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF16));
    init.capabilities.execute_command_provider = Some(ExecuteCommandOptions {
        commands: commands.iter().map(|&c| c.into()).collect(),
        ..Default::default()
    });
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

/// A manager on `fake`, with a clock that never moves.
fn routed(fake: &FakeServer) -> (LspManager, Events) {
    let (mut manager, events) = manager(fake.launcher(), Some(Arc::new(ManualClock::new())));
    manager.update_settings(&LspSettings::default());
    (manager, events)
}

/// Opens a Java file under `root` (the window root), which starts jdtls
/// there; the buffer must outlive the test's use of it.
fn open_java(manager: &mut LspManager, root: &Path, buffer: &mut Buffer) -> Option<ServerId> {
    let subscription = buffer.subscribe();
    manager
        .open(
            Some(root),
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            subscription,
            &root.join("src/main/java/app/Main.java"),
            "java",
        )
        .unwrap()
}

/// `jdtls`'s settings from `value`.
fn jdtls_settings(value: Value) -> LspSettings {
    let mut settings = LspSettings::default();
    let server: LspServerSettings = serde_json::from_value(value).unwrap();
    settings.servers.insert(JDTLS.into(), server);
    settings
}

/// The `initializationOptions` launch `launch` (1-based) was initialized
/// with.
fn initialization_options(fake: &FakeServer, launch: usize) -> Value {
    fake.wait_initialized_at(launch);
    (fake.records_of(launch).into_iter())
        .find_map(|record| match record {
            Record::Request { method, params, .. } if method == "initialize" => Some(params),
            _ => None,
        })
        .expect("an initialize request")
        .get("initializationOptions")
        .cloned()
        .unwrap_or(Value::Null)
}

/// The params of each `workspace/executeCommand` the fake got.
fn executed(fake: &FakeServer) -> Vec<Value> {
    (fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. } if method == EXECUTE => Some(params),
            _ => None,
        })
        .collect()
}

fn paths(paths: &[&str]) -> Vec<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

/// A report of `roots` running without the bundles, none refused.
fn stale(roots: &[&Path]) -> JdtlsBundles {
    JdtlsBundles {
        stale: roots.iter().map(|root| root.to_path_buf()).collect(),
        refused: None,
    }
}

fn not_listed(command: &str) -> String {
    kiln_l10n::tr!("lsp-not-a-server-command", command = command).to_string()
}

#[test]
fn jdtls_initialization_options_carry_the_hosts_bundles() {
    // After the user's own bundles, which stay; one both list goes once,
    // where the user put it, and the user's other options are kept.
    let fake = FakeServer::new(Script::default());
    let (mut manager, _events) = routed(&fake);
    manager.update_settings(&jdtls_settings(json!({
        "initialization_options": {
            "bundles": ["/home/me/lombok-bundle.jar", DEBUG_JAR],
            "settings": { "java": { "home": "/opt/jdk" } },
        },
    })));
    let host = paths(&[DEBUG_JAR, "/opt/java-test/junit-bundle.jar"]);
    assert_eq!(
        manager.set_jdtls_bundles(host.clone()),
        JdtlsBundles::default()
    );
    let mut buffer = Buffer::new("class Main {}\n");
    open_java(&mut manager, Path::new(ROOT), &mut buffer).expect("jdtls takes it");
    assert_eq!(
        initialization_options(&fake, 1),
        json!({
            "bundles": [
                "/home/me/lombok-bundle.jar",
                DEBUG_JAR,
                "/opt/java-test/junit-bundle.jar",
            ],
            "settings": { "java": { "home": "/opt/jdk" } },
        })
    );
    assert_eq!(fake.launched()[0].0.name, JDTLS);

    // With no options of the user's, the bundles are all there is.
    let fake = FakeServer::new(Script::default());
    let (mut manager, _events) = routed(&fake);
    manager.set_jdtls_bundles(paths(&[DEBUG_JAR]));
    let mut buffer = Buffer::new("class Main {}\n");
    open_java(&mut manager, Path::new(ROOT), &mut buffer).expect("jdtls takes it");
    assert_eq!(
        initialization_options(&fake, 1),
        json!({ "bundles": [DEBUG_JAR] })
    );

    // The specs `open` routes by don't carry them, so a change to them
    // alone never restarts a running jdtls; options the bundles can't be
    // merged into without replacing what the user wrote are left alone;
    // and no other server gets them.
    let mut registry = Registry::default();
    registry.set_jdtls_bundles(vec![DEBUG_JAR.into()]);
    let jdtls = |registry: &Registry| {
        (registry.specs().into_iter())
            .find(|spec| spec.name == JDTLS)
            .unwrap()
    };
    assert_eq!(jdtls(&registry).initialization_options, None);
    for options in [json!("not a table"), json!({ "bundles": "/one.jar" })] {
        registry.set_settings(jdtls_settings(json!({ "initialization_options": options })));
        let (launched, reached) = registry.launched(jdtls(&registry));
        assert_eq!(launched.initialization_options, Some(options));
        assert!(reached.is_empty());
    }
    let kotlin = (registry.specs().into_iter())
        .find(|spec| spec.name == "kotlin-lsp")
        .unwrap();
    assert_eq!(registry.launched(kotlin.clone()), (kotlin, Vec::new()));
}

#[test]
fn execute_command_returns_the_servers_reply() {
    let fake = FakeServer::new(listing(&[START, MAIN_CLASS, CLASSPATH, "app.nothing"]));
    let main_classes = json!([{
        "mainClass": "app.Main",
        "projectName": "app",
        "filePath": "/fake/root/src/main/java/app/Main.java",
    }]);
    let classpath = json!([["/fake/root/build/modules"], ["/fake/root/build/classes"]]);
    for (command, reply) in [
        (START, json!(45123)),
        (MAIN_CLASS, main_classes.clone()),
        (CLASSPATH, classpath.clone()),
        ("app.nothing", Value::Null),
    ] {
        fake.set_command_reply(command, Scripted::now(Reply::Result(reply)));
    }
    let (mut manager, _events) = routed(&fake);
    manager.set_jdtls_bundles(paths(&[DEBUG_JAR]));
    let mut buffer = Buffer::new("class Main {}\n");
    open_java(&mut manager, Path::new(ROOT), &mut buffer).expect("jdtls takes it");
    let root = Path::new(ROOT);

    let port = manager.execute_command_at(JDTLS, root, START, Vec::new());
    assert_eq!(answer(port).unwrap(), json!(45123));
    fake.wait_initialized();

    let found = manager.execute_command_at(JDTLS, root, MAIN_CLASS, Vec::new());
    assert_eq!(answer(found).unwrap(), main_classes);
    let arguments = vec![json!("app.Main"), json!("app")];
    let resolved = manager.execute_command_at(JDTLS, root, CLASSPATH, arguments.clone());
    assert_eq!(answer(resolved).unwrap(), classpath);
    // A null result is a result, not nothing.
    let nothing = manager.execute_command_at(JDTLS, root, "app.nothing", Vec::new());
    assert_eq!(answer(nothing).unwrap(), Value::Null);

    assert_eq!(
        executed(&fake),
        vec![
            json!({ "command": START, "arguments": [] }),
            json!({ "command": MAIN_CLASS, "arguments": [] }),
            json!({ "command": CLASSPATH, "arguments": arguments }),
            json!({ "command": "app.nothing", "arguments": [] }),
        ]
    );

    // The server's error comes back as one.
    fake.set_command_reply(
        START,
        Scripted::now(Reply::Error {
            code: -32603,
            message: "no debug bundle".into(),
        }),
    );
    let failed = manager.execute_command_at(JDTLS, root, START, Vec::new());
    assert!(matches!(
        answer(failed),
        Err(LspError::Server { code: -32603, ref message, .. }) if message == "no debug bundle"
    ));
}

#[test]
fn execute_command_refuses_a_command_the_server_did_not_advertise() {
    // Initialize waits on the fake's clock, so commands asked for first
    // are decided as they'd be sent.
    let clock = Arc::new(ManualClock::new());
    let delay = Duration::from_millis(50);
    let fake = FakeServer::with_clock(
        Script {
            initialize_delay: delay,
            ..listing(&[START])
        },
        clock.clone(),
    );
    let (mut manager, _events) = routed(&fake);
    let root = Path::new(ROOT);

    // No instance at the root: nothing to ask.
    let down = manager.execute_command_at(JDTLS, root, START, Vec::new());
    assert!(matches!(answer(down), Err(LspError::ServerDown)));

    let mut buffer = Buffer::new("class Main {}\n");
    let server = open_java(&mut manager, root, &mut buffer).expect("jdtls takes it");
    let early = manager.execute_command_at(JDTLS, root, CLASSPATH, Vec::new());
    clock.wait_for_sleep(delay);
    clock.advance(delay);
    fake.wait_initialized();
    let err = answer(early).unwrap_err();
    assert!(matches!(&err, LspError::Unavailable(why) if *why == *not_listed(CLASSPATH)));

    // Once initialized, it's refused at once.
    let late = manager.execute_command_at(JDTLS, root, MAIN_CLASS, Vec::new());
    let err = answer(late).unwrap_err();
    assert!(matches!(&err, LspError::Unavailable(why) if *why == *not_listed(MAIN_CLASS)));
    // Another server's command name at the same root isn't jdtls's.
    let elsewhere = manager.execute_command_at("kotlin-lsp", root, START, Vec::new());
    assert!(matches!(answer(elsewhere), Err(LspError::ServerDown)));

    // Neither reached the server: a request behind them did.
    answer(manager.request::<Echo>(server, json!({}))).unwrap();
    assert_eq!(executed(&fake), Vec::<Value>::new());

    // A server listing no commands at all refuses every one.
    let fake = FakeServer::new(Script::default());
    let (mut manager, _events) = routed(&fake);
    let mut buffer = Buffer::new("class Main {}\n");
    let server = open_java(&mut manager, root, &mut buffer).expect("jdtls takes it");
    fake.wait_initialized();
    answer(manager.request::<Echo>(server, json!({}))).unwrap();
    let refused = manager.execute_command_at(JDTLS, root, START, Vec::new());
    let err = answer(refused).unwrap_err();
    assert!(matches!(&err, LspError::Unavailable(why) if *why == *not_listed(START)));
    answer(manager.request::<Echo>(server, json!({}))).unwrap();
    assert_eq!(executed(&fake), Vec::<Value>::new());
}

#[test]
fn new_bundles_apply_on_the_next_jdtls_start() {
    let fake = FakeServer::new(Script::default());
    let (mut manager, _events) = routed(&fake);
    let root = Path::new(ROOT);
    let mut buffer = Buffer::new("class Main {}\n");
    let first = open_java(&mut manager, root, &mut buffer).expect("jdtls takes it");
    assert_eq!(initialization_options(&fake, 1), Value::Null);

    // The running jdtls keeps what it launched with, and the host is told
    // which roots run without the new bundles.
    assert_eq!(
        manager.set_jdtls_bundles(paths(&[DEBUG_JAR])),
        stale(&[Path::new(ROOT)])
    );
    answer(manager.request::<Echo>(first, json!({}))).unwrap();
    assert_eq!(fake.launches(), 1);
    assert_eq!(manager.server_of(buffer.id()), Some(first));

    // A jdtls starting at another root launches with them.
    let other = tempfile::tempdir().unwrap();
    let mut elsewhere = Buffer::new("class Other {}\n");
    open_java(&mut manager, other.path(), &mut elsewhere).expect("jdtls takes it");
    assert_eq!(
        initialization_options(&fake, 2),
        json!({ "bundles": [DEBUG_JAR] })
    );
    // Only the first still lacks them.
    assert_eq!(
        manager.set_jdtls_bundles(paths(&[DEBUG_JAR])),
        stale(&[Path::new(ROOT)])
    );

    // Restarting it brings them in.
    let restarted = manager.restart(JDTLS, root).unwrap().expect("a new jdtls");
    assert_ne!(restarted, first);
    assert_eq!(
        initialization_options(&fake, 3),
        json!({ "bundles": [DEBUG_JAR] })
    );
    assert_eq!(manager.jdtls_bundles(), JdtlsBundles::default());

    // So does a restart for a change to its spec: the user's options
    // first, then the host's bundles.
    let before = fake.launches();
    manager.update_settings(&jdtls_settings(json!({
        "initialization_options": { "bundles": ["/home/me/extra.jar"] },
    })));
    super::wait_until("jdtls relaunched", || fake.launches() >= before + 2);
    for launch in before + 1..=fake.launches() {
        assert_eq!(
            initialization_options(&fake, launch),
            json!({ "bundles": ["/home/me/extra.jar", DEBUG_JAR] })
        );
    }
    // Dropping them takes effect the same way.
    assert_eq!(manager.set_jdtls_bundles(Vec::new()).stale.len(), 2);
}

#[test]
fn options_that_cant_take_the_bundles_are_reported() {
    for (options, why) in [
        (json!("not a table"), BundlesRefused::OptionsNotATable),
        (
            json!({ "bundles": "/one.jar" }),
            BundlesRefused::BundlesNotAList,
        ),
    ] {
        let fake = FakeServer::new(Script::default());
        let (mut manager, _events) = routed(&fake);
        let root = Path::new(ROOT);
        manager.update_settings(&jdtls_settings(
            json!({ "initialization_options": options }),
        ));
        let refused = JdtlsBundles {
            stale: Vec::new(),
            refused: Some(why),
        };
        assert_eq!(manager.set_jdtls_bundles(paths(&[DEBUG_JAR])), refused);

        // The instance launches with the user's options as written, and is
        // reported as running without the bundles.
        let mut buffer = Buffer::new("class Main {}\n");
        open_java(&mut manager, root, &mut buffer).expect("jdtls takes it");
        assert_eq!(initialization_options(&fake, 1), options);
        assert_eq!(
            manager.jdtls_bundles(),
            JdtlsBundles {
                stale: vec![root.to_path_buf()],
                refused: Some(why),
            }
        );

        // Options that can take them restart it with them.
        manager.update_settings(&jdtls_settings(json!({ "initialization_options": {} })));
        super::wait_until("jdtls relaunched", || fake.launches() == 2);
        assert_eq!(
            initialization_options(&fake, 2),
            json!({ "bundles": [DEBUG_JAR] })
        );
        assert_eq!(manager.jdtls_bundles(), JdtlsBundles::default());
    }
}

/// The next status of `server` that isn't about readiness.
fn next_crash_status(events: &Events, server: ServerId) -> ServerStatus {
    loop {
        match events.status(server) {
            ServerStatus::Starting | ServerStatus::Ready | ServerStatus::Indexing { .. } => {}
            status => return status,
        }
    }
}

#[test]
fn a_jdtls_that_gave_up_stays_stopped_with_its_bundles() {
    let fake = FakeServer::new(Script::default());
    let clock = Arc::new(ManualClock::new());
    let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
    let settings = jdtls_settings(json!({
        "initialization_options": { "bundles": ["/home/me/extra.jar"] },
    }));
    manager.update_settings(&settings);
    manager.set_jdtls_bundles(paths(&[DEBUG_JAR]));
    let root = Path::new(ROOT);
    let mut a = Buffer::new("class A {}\n");
    let server = open_java(&mut manager, root, &mut a).expect("jdtls takes it");
    assert_eq!(events.status(server), ServerStatus::Starting);
    fake.wait_initialized();
    for n in 1..=5u32 {
        fake.crash();
        assert_eq!(
            next_crash_status(&events, server),
            ServerStatus::Restarting(n)
        );
        let backoff = Duration::from_secs(1 << (n - 1));
        clock.wait_for_sleep(backoff);
        clock.advance(backoff);
        fake.wait_initialized_at(n as usize + 1);
    }
    // Each crash restart kept the bundles.
    for launch in 1..=6 {
        assert_eq!(
            initialization_options(&fake, launch),
            json!({ "bundles": ["/home/me/extra.jar", DEBUG_JAR] })
        );
    }
    fake.crash();
    assert!(matches!(
        next_crash_status(&events, server),
        ServerStatus::Stopped(Some(_))
    ));
    super::wait_until("the thread to end", || manager.thread_finished(server));

    // Neither another document, the same settings nor new bundles start it
    // again.
    let mut b = Buffer::new("class B {}\n");
    let subscription = b.subscribe();
    let opened = manager
        .open(
            Some(root),
            b.id(),
            b.text().clone(),
            b.version(),
            subscription,
            &root.join("src/main/java/app/B.java"),
            "java",
        )
        .unwrap();
    assert_eq!(opened, None);
    manager.update_settings(&settings);
    manager.set_jdtls_bundles(paths(&[DEBUG_JAR, "/opt/java-test/junit-bundle.jar"]));
    manager.update_settings(&settings);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fake.launches(), 6);

    // A restart does, with the bundles as they are now.
    let again = manager.restart(JDTLS, root).unwrap().expect("a new jdtls");
    assert_eq!(events.status(again), ServerStatus::Starting);
    assert_eq!(
        initialization_options(&fake, 7),
        json!({ "bundles": [
            "/home/me/extra.jar",
            DEBUG_JAR,
            "/opt/java-test/junit-bundle.jar",
        ] })
    );
}
