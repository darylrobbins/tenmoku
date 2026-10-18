//! The trust gate ([`LspManager::set_trust`]): routed documents under an
//! untrusted root wait, and nothing starts or restarts a server there.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use fe_buffer::{Buffer, Origin};
use fe_config::settings::{LspServerSettings, LspSettings};
use lsp_types::Url;
use serde_json::json;

use crate::event::NOT_OPEN;
use crate::fake::{Echo, FakeServer, Record, Script, Writes};
use crate::{LspError, LspEvent, LspManager, ManualClock, ServerId, ServerSpec, ServerStatus};

#[path = "extension_servers_tests.rs"]
mod extension_servers;

const ROOT: &str = "/fake/root";
const OTHER: &str = "/fake/other";

/// A manager on the fake, with a manual clock that moves only when a test
/// moves it.
fn routed() -> (FakeServer, Arc<ManualClock>, LspManager) {
    let (fake, clock, manager, _) = routed_with_events();
    (fake, clock, manager)
}

/// As [`routed`], with the events it reports.
fn routed_with_events() -> (FakeServer, Arc<ManualClock>, LspManager, Receiver<LspEvent>) {
    let fake = FakeServer::new(Script::default());
    let clock = Arc::new(ManualClock::new());
    let (sender, events) = std::sync::mpsc::channel();
    let manager = LspManager::with_clock(fake.launcher(), clock.clone(), move |event| {
        let _ = sender.send(event);
    });
    (fake, clock, manager, events)
}

/// Waits for `server` to report it restarts after a crash.
fn wait_restarting(events: &Receiver<LspEvent>, server: ServerId) {
    loop {
        match events.recv_timeout(Duration::from_secs(10)) {
            Ok(LspEvent::Status {
                server: from,
                status: ServerStatus::Restarting(_),
                ..
            }) if from == server => return,
            Ok(_) => {}
            Err(error) => panic!("no restart of {server:?}: {error}"),
        }
    }
}

/// Opens `buffer` as `name` under `root`, routed to `language`'s server.
fn open(
    manager: &mut LspManager,
    root: &str,
    buffer: &mut Buffer,
    name: &str,
    language: &str,
) -> (Option<ServerId>, Url) {
    let path = PathBuf::from(root).join(name);
    let server = manager
        .open(
            Some(Path::new(root)),
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            buffer.subscribe(),
            &path,
            language,
        )
        .unwrap();
    (server, Url::from_file_path(&path).unwrap())
}

/// The names and roots of the servers launched so far, sorted.
fn launched(fake: &FakeServer) -> Vec<(String, PathBuf)> {
    let mut launched: Vec<_> = fake
        .launched()
        .into_iter()
        .map(|(spec, root)| (spec.name, root))
        .collect();
    launched.sort();
    launched
}

/// Gives the manager's threads a moment of real time to do anything they
/// would.
fn settle() {
    std::thread::sleep(Duration::from_millis(100));
}

/// Waits until the fake has `uri` open at `buffer`'s text, from launch
/// `launch`.
fn wait_synced(fake: &FakeServer, uri: &Url, buffer: &Buffer, launch: usize) {
    let text = buffer.text().to_string();
    fake.wait_for_document(uri, "the buffer's text", |replica| {
        replica.is_some_and(|r| r.open && r.launch == launch && r.text == text)
    });
}

#[test]
fn untrusted_roots_start_nothing() {
    let (fake, _clock, mut manager) = routed();
    manager.set_trust(|_| false);
    let mut rs = Buffer::new("fn main() {}\n");
    let mut py = Buffer::new("print(1)\n");
    let (server, rs_uri) = open(&mut manager, ROOT, &mut rs, "main.rs", "rust");
    assert_eq!(server, None);
    let (server, py_uri) = open(&mut manager, ROOT, &mut py, "main.py", "python");
    assert_eq!(server, None);
    settle();
    assert_eq!(fake.launches(), 0);
    assert_eq!(manager.server_of(rs.id()), None);
    assert_eq!(manager.server_of(py.id()), None);
    let request = manager.document_request::<Echo>(rs.id(), rs.version(), |_| json!({}));
    assert!(matches!(
        async_io::block_on(request),
        Err(LspError::Unavailable(why)) if why == NOT_OPEN
    ));

    rs.edit([(0..0, "// edited\n")], Origin::User).unwrap();
    manager.changed(rs.id());

    manager.set_trust(|_| true);
    for (buffer, uri) in [(&rs, &rs_uri), (&py, &py_uri)] {
        let launch = fake
            .wait_for_document(uri, "didOpen", |replica| replica.is_some_and(|r| r.open))
            .unwrap()
            .launch;
        wait_synced(&fake, uri, buffer, launch);
        assert!(manager.server_of(buffer.id()).is_some());
    }
    let root = PathBuf::from(ROOT);
    assert_eq!(
        launched(&fake),
        [
            ("pyright".into(), root.clone()),
            ("rust-analyzer".into(), root)
        ]
    );

    // A root the predicate still rejects gets no server.
    manager.set_trust(|root| root == Path::new(ROOT));
    let mut other = Buffer::new("fn other() {}\n");
    let (server, _) = open(&mut manager, OTHER, &mut other, "other.rs", "rust");
    assert_eq!(server, None);
    settle();
    assert_eq!(manager.server_of(other.id()), None);
    assert_eq!(fake.launches(), 2);
}

#[test]
fn untrusting_stops_servers() {
    let (fake, clock, mut manager, events) = routed_with_events();
    let mut a = Buffer::new("fn a() {}\n");
    let mut b = Buffer::new("fn b() {}\n");
    let (server, a_uri) = open(&mut manager, ROOT, &mut a, "a.rs", "rust");
    let server = server.unwrap();
    let (_, b_uri) = open(&mut manager, ROOT, &mut b, "b.rs", "rust");
    assert_eq!(manager.server_of(b.id()), Some(server));
    fake.wait_initialized();
    wait_synced(&fake, &a_uri, &a, 1);
    wait_synced(&fake, &b_uri, &b, 1);

    manager.set_trust(|_| false);
    assert_eq!(manager.server_of(a.id()), None);
    assert_eq!(manager.server_of(b.id()), None);
    fake.wait_for_at(1, "shutdown, then exit", |records| {
        let shutdown = records
            .iter()
            .position(|r| matches!(r, Record::Request { method, .. } if method == "shutdown"));
        let exit = records.iter().position(|r| r.is_notification("exit"));
        matches!((shutdown, exit), (Some(shutdown), Some(exit)) if shutdown < exit)
    });

    // Nothing starts it again while the root is untrusted.
    let mut settings = LspSettings::default();
    let changed: LspServerSettings =
        serde_json::from_value(json!({ "command": "/other/rust-analyzer" })).unwrap();
    settings.servers.insert("rust-analyzer".into(), changed);
    manager.update_settings(&settings);
    manager.register_spec(ServerSpec {
        languages: vec!["rust".into()],
        ..ServerSpec::new("rust-analyzer", "/host/rust-analyzer")
    });
    assert_eq!(
        manager.restart("rust-analyzer", Path::new(ROOT)).unwrap(),
        None
    );
    settle();
    assert_eq!(fake.launches(), 1);

    // Trusted again, it starts with both documents.
    manager.set_trust(|_| true);
    fake.wait_initialized_at(2);
    wait_synced(&fake, &a_uri, &a, 2);
    wait_synced(&fake, &b_uri, &b, 2);
    assert_eq!(fake.launches(), 2);

    // A crash then untrusting at once: no restart, even after the delay.
    fake.crash();
    manager.set_trust(|_| false);
    for _ in 0..3 {
        clock.advance(Duration::from_secs(2));
        settle();
    }
    assert_eq!(fake.launches(), 2);
    assert_eq!(manager.server_of(a.id()), None);

    // Untrusted while it waits out its restart's delay: none either.
    manager.set_trust(|_| true);
    fake.wait_initialized_at(3);
    let server = manager.server_of(a.id()).unwrap();
    fake.crash();
    wait_restarting(&events, server);
    manager.set_trust(|_| false);
    for _ in 0..3 {
        clock.advance(Duration::from_secs(2));
        settle();
    }
    assert_eq!(fake.launches(), 3);

    // A manager whose set_trust was never called starts servers as before.
    let (fake, _clock, mut manager) = routed();
    let mut c = Buffer::new("fn c() {}\n");
    let (server, c_uri) = open(&mut manager, ROOT, &mut c, "c.rs", "rust");
    assert!(server.is_some());
    wait_synced(&fake, &c_uri, &c, 1);
    assert_eq!(fake.launches(), 1);
}

#[test]
fn retrusting_opens_at_the_current_text() {
    let (fake, _clock, mut manager) = routed();
    let mut a = Buffer::new("fn a() {}\n");
    let (server, uri) = open(&mut manager, ROOT, &mut a, "a.rs", "rust");
    let server = server.unwrap();
    fake.wait_initialized();
    wait_synced(&fake, &uri, &a, 1);

    // The server's thread blocked on a write: retired, it can't hand the
    // document on (at the text it drained) until after it's edited and the
    // root is trusted again.
    fake.set_writes(Writes::Stepped);
    let _request = manager.request::<Echo>(server, json!({}));
    wait_until("the write to block", || fake.write_waiting());
    manager.set_trust(|_| false);
    a.edit([(0..0, "// edited\n")], Origin::User).unwrap();
    manager.changed(a.id());
    manager.set_trust(|_| true);
    fake.set_writes(Writes::Open);

    let records = fake.wait_for_at(2, "didOpen", |records| {
        records
            .iter()
            .any(|r| r.is_notification("textDocument/didOpen"))
    });
    let opened = records
        .iter()
        .find_map(|record| match record {
            Record::Notification { method, params } if method == "textDocument/didOpen" => {
                Some(params["textDocument"]["text"].clone())
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(opened, json!(a.text().to_string()));
}

/// Waits for `done`, polling; panics after 10 s naming `what`.
fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
