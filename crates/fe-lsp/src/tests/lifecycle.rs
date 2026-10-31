//! Starting a server (`initialize`) and stopping it (`shutdown`, `exit`).

use std::io;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures::channel::oneshot;
use futures::{AsyncRead, AsyncWrite, Future, FutureExt};
use lsp_types::request::Request;
use lsp_types::{PositionEncodingKind, Url};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};

use super::{Events, WAIT, answer, manager, wait_until};
use crate::fake::{
    Echo, FakeServer, InitializeReply, OnExit, Record, Script, Writes, initialize_result,
};
use crate::server::{QUEUED_MAX, STDERR_GRACE, STEPS_PER_YIELD};
use crate::{
    ExitInfo, LaunchCancel, LspError, LspManager, ManualClock, PositionEncoding, ServerId,
    ServerIo, ServerLauncher, ServerProcess, ServerSpec, ServerStatus,
};

fn start(
    manager: &mut LspManager,
    events: &Events,
    name: &str,
    root: &std::path::Path,
) -> ServerId {
    let id = manager
        .start(ServerSpec::new(name, "fake-server"), root)
        .unwrap();
    assert_eq!(events.status(id), ServerStatus::Starting);
    id
}

/// Every `dynamicRegistration` anywhere in `value`, with its path.
fn dynamic_registrations(value: &Value, path: &str, found: &mut Vec<(String, Value)>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let path = format!("{path}.{key}");
                if key == "dynamicRegistration" {
                    found.push((path.clone(), value.clone()));
                }
                dynamic_registrations(value, &path, found);
            }
        }
        Value::Array(items) => {
            for item in items {
                dynamic_registrations(item, path, found);
            }
        }
        _ => {}
    }
}

#[test]
fn initialize_advertises_m4_capabilities() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let fake = FakeServer::new(Script {
        initialize_delay: Duration::from_millis(30),
        ..Script::default()
    });
    let (mut manager, events) = manager(fake.launcher(), None);
    let mut spec = ServerSpec::new("init-utf8", "fake-server");
    spec.initialization_options = Some(json!({ "cargo": { "features": "all" } }));
    let id = manager.start(spec, &root).unwrap();
    assert_eq!(events.status(id), ServerStatus::Starting);
    fake.wait_initialized();

    let records = fake.records();
    let initializes: Vec<_> = records
        .iter()
        .filter_map(|record| match record {
            Record::Request { method, params, .. } if method == "initialize" => Some(params),
            _ => None,
        })
        .collect();
    assert_eq!(initializes.len(), 1, "{records:#?}");
    let params = initializes[0];
    let uri = Url::from_file_path(&root).unwrap();
    assert_eq!(params["processId"], json!(std::process::id()));
    assert_eq!(params["rootUri"], json!(uri));
    assert_eq!(
        params["workspaceFolders"],
        json!([{ "uri": uri, "name": "project" }])
    );
    assert_eq!(params["clientInfo"]["name"], "Tenmoku");
    assert_eq!(
        params["initializationOptions"],
        json!({ "cargo": { "features": "all" } })
    );
    let caps = &params["capabilities"];
    assert_eq!(
        caps["general"]["positionEncodings"],
        json!(["utf-8", "utf-16"])
    );
    assert_eq!(caps["window"]["workDoneProgress"], true);
    assert_eq!(caps["workspace"]["configuration"], true);
    assert_eq!(caps["workspace"]["workspaceFolders"], true);
    assert_eq!(caps["experimental"]["serverStatusNotification"], true);
    let mut dynamic = Vec::new();
    dynamic_registrations(caps, "capabilities", &mut dynamic);
    let paths: Vec<&str> = dynamic.iter().map(|(path, _)| path.as_str()).collect();
    for expected in [
        "capabilities.workspace.didChangeConfiguration.dynamicRegistration",
        "capabilities.workspace.symbol.dynamicRegistration",
        "capabilities.textDocument.synchronization.dynamicRegistration",
        "capabilities.textDocument.completion.dynamicRegistration",
        "capabilities.textDocument.hover.dynamicRegistration",
        "capabilities.textDocument.definition.dynamicRegistration",
        "capabilities.textDocument.references.dynamicRegistration",
    ] {
        assert!(
            paths.contains(&expected),
            "{expected} missing from {paths:?}"
        );
    }
    // Watched files are registered dynamically (watched_files.rs).
    for (path, value) in &dynamic {
        assert_eq!(
            value,
            &Value::Bool(
                path == "capabilities.workspace.didChangeWatchedFiles.dynamicRegistration"
            ),
            "{path}"
        );
    }

    // `initialized` follows the reply.
    let replied = records
        .iter()
        .position(|r| matches!(r, Record::Replied { method, .. } if method == "initialize"))
        .unwrap();
    let initialized = records
        .iter()
        .position(|r| r.is_notification("initialized"))
        .unwrap();
    assert!(replied < initialized, "{records:#?}");

    assert_eq!(manager.position_encoding(id), Some(PositionEncoding::Utf8));
    assert_eq!(fake.launch_threads(), [Some("fe-lsp init-utf8".to_owned())]);

    // A server that doesn't say is utf-16.
    let fake = FakeServer::new(Script {
        initialize: InitializeReply::Result(Box::new(initialize_result(None))),
        ..Script::default()
    });
    let (mut manager, events) = super::manager(fake.launcher(), None);
    let id = start(&mut manager, &events, "init-utf16", &root);
    fake.wait_initialized();
    assert_eq!(manager.position_encoding(id), Some(PositionEncoding::Utf16));
    assert_eq!(
        manager.capabilities(id).unwrap().position_encoding,
        None::<PositionEncodingKind>
    );
}

#[test]
fn initialize_advertises_navigation_and_highlight_capabilities() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let fake = FakeServer::new(Script::default());
    let (mut manager, _events) = manager(fake.launcher(), None);
    manager
        .start(ServerSpec::new("init-utf8", "fake-server"), &root)
        .unwrap();
    fake.wait_initialized();

    let records = fake.records();
    let params = records
        .iter()
        .find_map(|record| match record {
            Record::Request { method, params, .. } if method == "initialize" => Some(params),
            _ => None,
        })
        .unwrap();
    let text_document = &params["capabilities"]["textDocument"];
    for goto in ["declaration", "typeDefinition", "implementation"] {
        assert_eq!(
            text_document[goto],
            json!({ "dynamicRegistration": false, "linkSupport": true }),
            "{goto}"
        );
    }
    assert_eq!(
        text_document["documentHighlight"],
        json!({ "dynamicRegistration": false })
    );
}

#[test]
fn new_requests_time_out_as_their_neighbours() {
    let timeouts = fe_config::settings::LspTimeouts::default();
    let of = |method| crate::requests::timeout(method, &timeouts);
    assert!(of("textDocument/definition").is_some());
    for method in [
        "textDocument/declaration",
        "textDocument/typeDefinition",
        "textDocument/implementation",
    ] {
        assert_eq!(of(method), of("textDocument/definition"), "{method}");
    }
    assert_eq!(
        of("textDocument/documentHighlight"),
        of("textDocument/hover")
    );
}

fn shutdown_methods(fake: &FakeServer) -> Vec<String> {
    fake.records()
        .iter()
        .filter(|r| matches!(r.method(), "shutdown" | "exit"))
        .map(|r| match r {
            Record::Replied { method, .. } => format!("reply {method}"),
            r => r.method().to_owned(),
        })
        .collect()
}

#[test]
fn shutdown_sequence() {
    let root = tempfile::tempdir().unwrap();

    // shutdown, its reply, exit; the fake exits and closes its output
    // (EOF): a normal stop, with no kill and no wait on the clock.
    let clock = Arc::new(ManualClock::new());
    let fake = FakeServer::new(Script::default());
    let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
    let id = start(&mut manager, &events, "stop-clean", root.path());
    fake.wait_initialized();
    manager.stop(id);
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    assert_eq!(
        shutdown_methods(&fake),
        ["shutdown", "reply shutdown", "exit"]
    );
    assert_eq!(fake.kills(), 0);

    // A non-zero exit code after `exit` is a normal stop too.
    let fake = FakeServer::new(Script {
        on_exit: OnExit::Exit(1),
        ..Script::default()
    });
    let (mut manager, events) = super::manager(fake.launcher(), Some(clock.clone()));
    let id = start(&mut manager, &events, "stop-exit-1", root.path());
    fake.wait_initialized();
    manager.stop(id);
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    assert_eq!(fake.kills(), 0);

    // `shutdown` unanswered: `exit` after 2 s on the clock.
    let clock = Arc::new(ManualClock::new());
    let fake = FakeServer::new(Script {
        answer_shutdown: false,
        ..Script::default()
    });
    let (mut manager, events) = super::manager(fake.launcher(), Some(clock.clone()));
    let id = start(&mut manager, &events, "stop-no-reply", root.path());
    fake.wait_initialized();
    manager.stop(id);
    clock.wait_for_sleep(Duration::from_secs(2));
    clock.advance(Duration::from_millis(1999));
    events.assert_quiet();
    assert_eq!(shutdown_methods(&fake), ["shutdown"]);
    clock.advance(Duration::from_millis(1));
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    assert_eq!(shutdown_methods(&fake), ["shutdown", "exit"]);
    assert_eq!(fake.kills(), 0);

    // Still running 1 s after `exit`: killed.
    let clock = Arc::new(ManualClock::new());
    let fake = FakeServer::new(Script {
        on_exit: OnExit::Ignore,
        ..Script::default()
    });
    let (mut manager, events) = super::manager(fake.launcher(), Some(clock.clone()));
    let id = start(&mut manager, &events, "stop-kill", root.path());
    fake.wait_initialized();
    manager.stop(id);
    fake.wait_for("exit", |records| {
        records.iter().any(|r| r.is_notification("exit"))
    });
    clock.wait_for_sleep(Duration::from_secs(1));
    clock.advance(Duration::from_millis(999));
    events.assert_quiet();
    assert_eq!(fake.kills(), 0);
    clock.advance(Duration::from_millis(1));
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    assert_eq!(fake.kills(), 1);
    assert_eq!(
        shutdown_methods(&fake),
        ["shutdown", "reply shutdown", "exit"]
    );

    // EOF after `exit` with the process still there: a normal stop, once
    // it's killed 1 s later.
    let clock = Arc::new(ManualClock::new());
    let fake = FakeServer::new(Script {
        on_exit: OnExit::CloseOnly,
        ..Script::default()
    });
    let (mut manager, events) = super::manager(fake.launcher(), Some(clock.clone()));
    let id = start(&mut manager, &events, "stop-eof", root.path());
    fake.wait_initialized();
    manager.stop(id);
    fake.wait_for("exit", |records| {
        records.iter().any(|r| r.is_notification("exit"))
    });
    clock.wait_for_sleep(Duration::from_secs(1));
    events.assert_quiet();
    clock.advance(Duration::from_secs(1));
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    assert_eq!(fake.kills(), 1);
}

#[test]
fn start_after_a_stop_starts_afresh() {
    let root = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new());
    let fake = FakeServer::new(Script::default());
    let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
    let mut id = start(&mut manager, &events, "restart", root.path());
    // Crashes restart it, until the 6th in 3 minutes stops it.
    for (n, launch) in (1..=5).zip(1..) {
        fake.wait_initialized_at(launch);
        fake.crash_at(launch);
        assert_eq!(events.status(id), ServerStatus::Restarting(n));
        let backoff = Duration::from_secs(1 << (n - 1));
        clock.wait_for_sleep(backoff);
        clock.advance(backoff);
        assert_eq!(events.status(id), ServerStatus::Starting);
    }
    for launch in 6..=20 {
        fake.wait_initialized_at(launch);
        fake.crash_at(launch);
        // A start doesn't renew the budget, so each crash now stops it.
        assert!(matches!(events.status(id), ServerStatus::Stopped(Some(_))));
        // Started as soon as the stop is heard: never the dead server.
        let again = start(&mut manager, &events, "restart", root.path());
        assert_ne!(again, id);
        id = again;
    }
}

#[test]
fn initialize_failure_restarts_the_server() {
    let root = tempfile::tempdir().unwrap();
    // A crash: it restarts, having logged why on its thread.
    let stopped = |manager: &LspManager, events: &Events, id, name: &str| {
        assert_eq!(events.status(id), ServerStatus::Restarting(1));
        assert!(!manager.thread_finished(id));
        super::logs_from(&format!("fe-lsp {name}"))
            .into_iter()
            .find(|line| line.ends_with("restarting in 1 s"))
            .expect("the crash logged")
    };
    for name in ["init-error", "init-timeout", "init-eof"] {
        super::capture_logs(&format!("fe-lsp {name}"));
    }

    // An error answer.
    let fake = FakeServer::new(Script {
        initialize: InitializeReply::Error {
            code: -32603,
            message: "Could not find a valid TypeScript installation".into(),
        },
        ..Script::default()
    });
    let (mut manager, events) = manager(fake.launcher(), None);
    let id = start(&mut manager, &events, "init-error", root.path());
    let why = stopped(&manager, &events, id, "init-error");
    assert!(why.contains("initialize failed"), "{why}");
    assert!(
        why.contains("Could not find a valid TypeScript installation"),
        "{why}"
    );

    // No answer within 30 s on the clock.
    let clock = Arc::new(ManualClock::new());
    let fake = FakeServer::new(Script {
        initialize: InitializeReply::Never,
        ..Script::default()
    });
    let (mut manager, events) = super::manager(fake.launcher(), Some(clock.clone()));
    let id = start(&mut manager, &events, "init-timeout", root.path());
    clock.wait_for_sleep(Duration::from_secs(30));
    clock.advance(Duration::from_millis(29_999));
    events.assert_quiet();
    clock.advance(Duration::from_millis(1));
    let why = stopped(&manager, &events, id, "init-timeout");
    assert!(
        why.contains("didn't answer initialize within 30 s"),
        "{why}"
    );

    // EOF before the answer.
    let fake = FakeServer::new(Script {
        initialize: InitializeReply::Never,
        ..Script::default()
    });
    let (mut manager, events) = super::manager(fake.launcher(), None);
    let id = start(&mut manager, &events, "init-eof", root.path());
    fake.wait_for("initialize", |records| {
        records.iter().any(|r| r.method() == "initialize")
    });
    fake.crash();
    let why = stopped(&manager, &events, id, "init-eof");
    assert!(why.contains("before answering initialize"), "{why}");
    // The fake closes its output and exits together: either may come first.
    assert!(
        why.contains("EOF") || why.contains("exited with code 1"),
        "{why}"
    );
}

#[test]
fn manager_and_request_futures_are_send() {
    fn send<T: Send>(_: &T) {}
    let fake = FakeServer::new(Script::default());
    let (manager, _events) = manager(fake.launcher(), None);
    send(&manager);
    send(&manager.request::<lsp_types::request::Shutdown>(ServerId(0), ()));
}

/// A server whose stdout and stderr are always ready: an endless stream of
/// notifications, with `initialize`'s answer slipped in once the client
/// has written something (if `answer`), and endless stderr. It answers
/// nothing else, and goes only when killed.
struct Flood {
    answer: bool,
    kills: Arc<AtomicUsize>,
}

impl ServerLauncher for Flood {
    fn launch(&self, _: &ServerSpec, _: &Path, _: &LaunchCancel) -> io::Result<ServerIo> {
        let wrote = Arc::new(AtomicBool::new(false));
        let (killed, exited) = oneshot::channel();
        Ok(ServerIo {
            input: Box::pin(Written(wrote.clone())),
            output: Box::pin(Flooding {
                answer: self.answer.then_some(wrote),
                frame: Vec::new(),
                at: 0,
            }),
            stderr: Some(Box::pin(futures::io::repeat(b'!'))),
            process: Box::new(Killable {
                kills: self.kills.clone(),
                killed: Some(killed),
                exited: exited.shared(),
            }),
        })
    }
}

struct Written(Arc<AtomicBool>);

impl AsyncWrite for Written {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.0.store(true, Ordering::SeqCst);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct Flooding {
    /// Set once the client has written; then `initialize` is answered.
    answer: Option<Arc<AtomicBool>>,
    frame: Vec<u8>,
    at: usize,
}

fn frame(body: &Value) -> Vec<u8> {
    let body = body.to_string();
    format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

impl AsyncRead for Flooding {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        if this.at == this.frame.len() {
            let answer = this
                .answer
                .take_if(|wrote| wrote.load(Ordering::SeqCst))
                .is_some();
            this.frame = if answer {
                frame(&json!({ "jsonrpc": "2.0", "id": 0, "result": { "capabilities": {} } }))
            } else {
                frame(&json!({ "jsonrpc": "2.0", "method": "flood/notification" }))
            };
            this.at = 0;
        }
        let n = out.len().min(this.frame.len() - this.at);
        out[..n].copy_from_slice(&this.frame[this.at..this.at + n]);
        this.at += n;
        Poll::Ready(Ok(n))
    }
}

struct Killable {
    kills: Arc<AtomicUsize>,
    killed: Option<oneshot::Sender<()>>,
    exited: futures::future::Shared<oneshot::Receiver<()>>,
}

impl ServerProcess for Killable {
    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>> {
        let exited = self.exited.clone();
        Box::pin(async move {
            let _ = exited.await;
            ExitInfo::default()
        })
    }

    fn kill(&mut self) {
        self.kills.fetch_add(1, Ordering::SeqCst);
        self.killed.take();
    }
}

#[test]
fn floods_do_not_hold_up_stop_or_deadlines() {
    let root = tempfile::tempdir().unwrap();

    // Always-ready stdout and stderr, `initialize` never answered: the
    // 30 s deadline on the clock still ends the start.
    let clock = Arc::new(ManualClock::new());
    let kills = Arc::new(AtomicUsize::new(0));
    let launcher = Arc::new(Flood {
        answer: false,
        kills: kills.clone(),
    });
    let (mut manager, events) = manager(launcher, Some(clock.clone()));
    let id = start(&mut manager, &events, "flood-init", root.path());
    clock.wait_for_sleep(Duration::from_secs(30));
    clock.advance(Duration::from_secs(30));
    // Its stderr never stops being ready: read for 1 s more on the clock.
    clock.wait_for_sleep(STDERR_GRACE);
    clock.advance(STDERR_GRACE);
    // A crash: stopped while it waits to restart.
    assert_eq!(events.status(id), ServerStatus::Restarting(1));
    manager.stop(id);
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    wait_until("the server thread to end", || manager.thread_finished(id));
    assert_eq!(kills.load(Ordering::SeqCst), 1);

    // Initialized, then flooding: a stop still goes through `shutdown`'s
    // 2 s and `exit`'s 1 s on the clock to the kill.
    let clock = Arc::new(ManualClock::new());
    let kills = Arc::new(AtomicUsize::new(0));
    let launcher = Arc::new(Flood {
        answer: true,
        kills: kills.clone(),
    });
    let (mut manager, events) = super::manager(launcher, Some(clock.clone()));
    let id = start(&mut manager, &events, "flood-stop", root.path());
    wait_until("initialize's answer", || {
        manager.position_encoding(id).is_some()
    });
    manager.stop(id);
    clock.wait_for_sleep(Duration::from_secs(2));
    clock.advance(Duration::from_secs(2));
    clock.wait_for_sleep(Duration::from_secs(1));
    clock.advance(Duration::from_secs(1));
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    assert_eq!(kills.load(Ordering::SeqCst), 1);
    clock.wait_for_sleep(STDERR_GRACE);
    clock.advance(STDERR_GRACE);
    wait_until("the server thread to end", || manager.thread_finished(id));

    // A host flooding requests from another thread doesn't hold up a stop.
    // The clock is manual and never advanced: on the system clock, readiness's
    // 500 ms settle can pass under load before the flood is recorded, so `Ready`
    // would come before `Stopped`. The stop itself needs no clock time.
    let fake = FakeServer::new(Script::default());
    let (manager, events) = super::manager(fake.launcher(), Some(Arc::new(ManualClock::new())));
    let manager = Arc::new(Mutex::new(manager));
    let id = manager
        .lock()
        .unwrap()
        .start(
            ServerSpec::new("flood-requests", "fake-server"),
            root.path(),
        )
        .unwrap();
    assert_eq!(events.status(id), ServerStatus::Starting);
    fake.wait_initialized();
    let flooding = Arc::new(AtomicBool::new(true));
    let flood = std::thread::spawn({
        let manager = manager.clone();
        let flooding = flooding.clone();
        move || {
            // Kept, since a request whose caller has gone isn't sent.
            let mut kept = Vec::new();
            while flooding.load(Ordering::SeqCst) {
                kept.push(manager.lock().unwrap().request::<Echo>(id, json!({})));
            }
        }
    });
    fake.wait_for("the flood", |records| records.len() > 1000);
    manager.lock().unwrap().stop(id);
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    flooding.store(false, Ordering::SeqCst);
    flood.join().unwrap();
}

#[test]
fn a_stop_while_starting_kills_it_ahead_of_queued_requests() {
    let root = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new());
    let fake = FakeServer::with_clock(
        Script {
            initialize_delay: Duration::from_secs(5),
            answer_shutdown: false,
            ..Script::default()
        },
        clock.clone(),
    );
    let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
    let id = start(&mut manager, &events, "stop-starting", root.path());
    fake.wait_for("initialize", |records| {
        records.iter().any(|r| r.method() == "initialize")
    });
    // Requests pile up while `initialize` is slow; then a stop.
    let queued: Vec<_> = (0..10_000)
        .map(|n| manager.request::<Echo>(id, json!({ "n": n })))
        .collect();
    manager.stop(id);

    // Nothing may be sent before the answer, `shutdown` included, so the
    // stop kills it at once, with no time passing on the clock, and sends
    // none of the backlog.
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    let records = fake.records();
    assert!(!records.iter().any(|r| r.method() == "shutdown"));
    assert!(!records.iter().any(|r| r.method() == "fake/echo"));
    assert_eq!(fake.kills(), 1);
    for reply in queued {
        assert_eq!(async_io::block_on(reply), Err(LspError::ServerDown));
    }
}

/// A request whose params note when they're serialized, which is when the
/// actor hands the request to async-lsp.
enum Marked {}

impl Request for Marked {
    type Params = Marker;
    type Result = Value;
    const METHOD: &'static str = "test/marked";
}

#[derive(Default)]
struct Marker(Arc<AtomicBool>);

impl Serialize for Marker {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.store(true, Ordering::SeqCst);
        serializer.serialize_unit()
    }
}

impl<'de> Deserialize<'de> for Marker {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        <()>::deserialize(deserializer).map(|()| Self::default())
    }
}

/// With the fake's writes stepped, lets the client's writes through one at
/// a time until `done` holds, and says how many it let through. `done` is
/// checked while the server thread is blocked on a write, so it sees all
/// that thread did before the write.
fn writes_until(fake: &FakeServer, what: &str, done: impl Fn() -> bool) -> u32 {
    let deadline = Instant::now() + WAIT;
    let mut writes = 0;
    loop {
        let waiting = fake.write_waiting();
        if done() {
            return writes;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        if waiting {
            fake.allow_write();
            writes += 1;
        } else {
            std::thread::sleep(Duration::from_micros(50));
        }
    }
}

#[test]
fn a_long_send_queue_does_not_hold_up_stop_or_deadlines() {
    let root = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new());
    let fake = FakeServer::with_clock(
        Script {
            initialize_delay: Duration::from_secs(5),
            answer_shutdown: false,
            ..Script::default()
        },
        clock.clone(),
    );
    let (mut manager, events) = manager(fake.launcher(), Some(clock.clone()));
    let id = start(&mut manager, &events, "send-queue", root.path());
    fake.wait_for("initialize", |records| {
        records.iter().any(|r| r.method() == "initialize")
    });

    // While `initialize` is slow, requests pile up, and the server stops
    // reading. Once it's answered, the actor fills async-lsp's queue, which
    // can't send, and keeps the rest.
    fake.set_writes(Writes::Held);
    // The queue holds more than one budget of writes (two a message), so
    // the stop is heard in time only if the budget holds for a whole poll.
    assert!(2 * QUEUED_MAX > u64::from(STEPS_PER_YIELD));
    let queued_max = usize::try_from(QUEUED_MAX).unwrap();
    let in_flight: Vec<_> = (1..queued_max)
        .map(|n| manager.request::<Echo>(id, json!({ "n": n })))
        .collect();
    let last_in = Marker::default();
    let filled = last_in.0.clone();
    let first_out = Marker::default();
    let overfilled = first_out.0.clone();
    let marked = [
        manager.request::<Marked>(id, last_in),
        manager.request::<Marked>(id, first_out),
    ];
    let mut held: Vec<_> = (0..500)
        .map(|n| manager.request::<Echo>(id, json!({ "n": n })))
        .collect();
    clock.wait_for_sleep(Duration::from_secs(5));
    clock.advance(Duration::from_secs(5));
    wait_until("async-lsp's queue to fill", || {
        filled.load(Ordering::SeqCst)
    });
    // These wait unread in the actor's command channel.
    held.extend((0..100).map(|n| manager.request::<Echo>(id, json!({ "late": n }))));

    // Then the server reads as fast as the client writes. The stop is heard
    // within one budget of writes, and so is `shutdown`'s 2 s on the clock,
    // passing while the queue ahead of `shutdown` still sends.
    fake.set_writes(Writes::Stepped);
    manager.stop(id);
    let writes = writes_until(&fake, "the stop", || {
        clock.sleeping().contains(&Duration::from_secs(2))
    });
    assert!(writes <= STEPS_PER_YIELD, "the stop waited {writes} writes");
    // The requests handed to async-lsp are answered at once.
    let [last_in, first_out] = marked;
    for reply in in_flight {
        assert_eq!(reply.now_or_never(), Some(Err(LspError::ServerDown)));
    }
    assert_eq!(last_in.now_or_never(), Some(Err(LspError::ServerDown)));
    clock.advance(Duration::from_secs(2));
    let writes = writes_until(&fake, "shutdown's deadline", || {
        clock.sleeping().contains(&Duration::from_secs(1))
    });
    assert!(
        writes <= STEPS_PER_YIELD,
        "the deadline waited {writes} writes"
    );
    let exited = || fake.records().iter().any(|r| r.is_notification("exit"));
    writes_until(&fake, "exit", exited);
    fake.set_writes(Writes::Open);
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    assert_eq!(fake.kills(), 0);

    // The server got the queue's worth, then `shutdown` and `exit`; the
    // requests the actor kept were dropped unsent.
    let received: Vec<_> = fake
        .records()
        .iter()
        .filter(|r| !matches!(r, Record::Replied { .. }))
        .map(|r| r.method().to_owned())
        .collect();
    let mut expected = vec!["initialize", "initialized"];
    expected.extend(std::iter::repeat_n(Echo::METHOD, queued_max - 1));
    expected.extend([Marked::METHOD, "shutdown", "exit"]);
    assert_eq!(received, expected);
    assert!(!overfilled.load(Ordering::SeqCst));
    for reply in held {
        assert_eq!(answer(reply), Err(LspError::ServerDown));
    }
    assert_eq!(answer(first_out), Err(LspError::ServerDown));
}
