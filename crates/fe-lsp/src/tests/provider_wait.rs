//! A rename, a prepare rename, and a declaration, type definition or
//! implementation request made while the server is still initializing:
//! each is sent, or not, by what the server then says it provides.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use fe_buffer::Buffer;
use fe_nav::{NavError, NavRequest, NavigationProvider, Related};
use lsp_types::{OneOf, PositionEncodingKind, RenameOptions, ServerCapabilities};
use serde_json::{Value, json};

use super::WAIT;
use crate::fake::{
    FakeServer, InitializeReply, Record, Reply, Script, Scripted, initialize_result,
};
use crate::rename::RenameTarget;
use crate::{LspError, LspEvent, LspManager, LspNavigation, ManualClock, ServerId, ServerSpec};

const ROOT: &str = crate::fake::FAKE_ROOT;
const TEXT: &str = "fn helper() {}\n";
/// How long the fake holds `initialize`.
const DELAY: Duration = Duration::from_secs(1);

/// A server holding `initialize`, with a buffer open on it, nothing
/// waited for.
struct Starting {
    fake: FakeServer,
    clock: Arc<ManualClock>,
    fake_clock: Arc<ManualClock>,
    manager: LspManager,
    buffer: Buffer,
}

impl Starting {
    fn new(capabilities: ServerCapabilities) -> Self {
        Self::start(capabilities).0
    }

    fn start(capabilities: ServerCapabilities) -> (Self, ServerId) {
        let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
        init.capabilities = ServerCapabilities {
            text_document_sync: init.capabilities.text_document_sync,
            position_encoding: init.capabilities.position_encoding,
            ..capabilities
        };
        let fake_clock = Arc::new(ManualClock::new());
        let fake = FakeServer::with_clock(
            Script {
                initialize: InitializeReply::Result(Box::new(init)),
                initialize_delay: DELAY,
                ..Script::default()
            },
            fake_clock.clone(),
        );
        let clock = Arc::new(ManualClock::new());
        let (sender, events) = std::sync::mpsc::channel::<LspEvent>();
        let mut manager = LspManager::with_clock(fake.launcher(), clock.clone(), move |event| {
            let _ = sender.send(event);
        });
        let server = manager
            .start(ServerSpec::new("fake", "fake-server"), Path::new(ROOT))
            .unwrap();
        // The events aren't read; keep the channel open.
        std::mem::forget(events);
        let mut buffer = Buffer::new(TEXT);
        let subscription = buffer.subscribe();
        manager
            .open_on(
                server,
                buffer.id(),
                buffer.text().clone(),
                buffer.version(),
                subscription,
                &PathBuf::from(ROOT).join("a.rs"),
                "text",
            )
            .unwrap();
        let starting = Self {
            fake,
            clock,
            fake_clock,
            manager,
            buffer,
        };
        (starting, server)
    }

    /// Scripts a `null` reply to each of `methods`, so a request that is
    /// sent is answered.
    fn nulls(&self, methods: &[&str]) {
        for method in methods {
            self.fake
                .set_reply(method, Scripted::now(Reply::Result(Value::Null)));
        }
    }

    fn finish_initializing<T>(&self, calls: Vec<JoinHandle<T>>) -> Vec<T> {
        finish_initializing(&self.fake, &self.clock, &self.fake_clock, calls)
    }

    /// How many requests for `method` the fake got.
    fn sent(&self, method: &str) -> usize {
        sent(&self.fake, method)
    }
}

/// How many requests for `method` `fake` got.
fn sent(fake: &FakeServer, method: &str) -> usize {
    (fake.records().iter())
        .filter(|record| matches!(record, Record::Request { method: sent, .. } if sent == method))
        .count()
}

/// Lets `initialize` be answered, then steps the manager's clock (well
/// under any timeout) until `calls` have all finished.
fn finish_initializing<T>(
    fake: &FakeServer,
    clock: &ManualClock,
    fake_clock: &ManualClock,
    calls: Vec<JoinHandle<T>>,
) -> Vec<T> {
    fake_clock.wait_for_sleep(DELAY);
    fake_clock.advance(DELAY);
    fake.wait_initialized();
    let deadline = Instant::now() + WAIT;
    let mut steps = 0;
    while !calls.iter().all(JoinHandle::is_finished) && Instant::now() < deadline {
        if steps < 100 {
            clock.advance(Duration::from_millis(20));
            steps += 1;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert!(calls.iter().all(JoinHandle::is_finished));
    (calls.into_iter())
        .map(|call| call.join().unwrap_or_else(|_| unreachable!()))
        .collect()
}

#[test]
fn rename_waits_until_capabilities_are_known() {
    let s = Starting::new(ServerCapabilities::default());
    let methods = ["textDocument/prepareRename", "textDocument/rename"];
    s.nulls(&methods);
    let (id, version) = (s.buffer.id(), s.buffer.version());
    let prepare = s.manager.prepare_rename(id, version, 4);
    let rename = s.manager.rename(id, version, 4, "other".into());
    let calls = vec![
        thread::spawn(move || async_io::block_on(prepare).map(|_| ())),
        thread::spawn(move || async_io::block_on(rename).map(|_| ())),
    ];
    let answers = s.finish_initializing(calls);
    assert_eq!(
        answers,
        vec![
            Err(LspError::Unavailable("rename".into())),
            Err(LspError::Unavailable("rename".into()))
        ]
    );
    assert_eq!(s.sent(methods[0]), 0);
    assert_eq!(s.sent(methods[1]), 0);
}

#[test]
fn prepare_rename_asks_a_preparing_server_once_known() {
    let s = Starting::new(ServerCapabilities {
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        ..ServerCapabilities::default()
    });
    // "helper" is the word at the offset, and "renamed" isn't.
    let range = json!({
        "start": { "line": 0, "character": 3 },
        "end": { "line": 0, "character": 9 },
    });
    s.fake.set_reply(
        "textDocument/prepareRename",
        Scripted::now(Reply::Result(
            json!({ "range": range, "placeholder": "renamed" }),
        )),
    );
    let prepare = s
        .manager
        .prepare_rename(s.buffer.id(), s.buffer.version(), 4);
    let calls = vec![thread::spawn(move || async_io::block_on(prepare))];
    let answers = s.finish_initializing(calls);
    let answers: Vec<_> = (answers.into_iter())
        .map(|answer| {
            answer.map(|target| {
                target.map(|target| {
                    assert!(target.owner.is_some(), "a prepared target has its owner");
                    RenameTarget::new(target.range, target.placeholder)
                })
            })
        })
        .collect();
    assert_eq!(answers, vec![Ok(Some(RenameTarget::new(3..9, "renamed")))]);
    assert_eq!(s.sent("textDocument/prepareRename"), 1);
}

#[test]
fn related_waits_until_capabilities_are_known() {
    let s = Starting::new(ServerCapabilities::default());
    let methods = [
        "textDocument/declaration",
        "textDocument/typeDefinition",
        "textDocument/implementation",
    ];
    s.nulls(&methods);
    let req = NavRequest {
        root: PathBuf::from(ROOT),
        rel: PathBuf::from("a.rs"),
        language: Some("text"),
        text: s.buffer.text().clone(),
        offset: 4,
        buffer: Some((s.buffer.id(), s.buffer.version())),
    };
    let manager = Arc::new(Mutex::new(s.manager));
    let lsp = Arc::new(LspNavigation::new(Arc::downgrade(&manager), ROOT));
    let calls: Vec<_> = [
        Related::Declaration,
        Related::TypeDefinition,
        Related::Implementation,
    ]
    .into_iter()
    .map(|kind| {
        let (lsp, req) = (lsp.clone(), req.clone());
        thread::spawn(move || {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            lsp.related(kind, &req, &cancel).map(|_| ())
        })
    })
    .collect();
    // Each call blocks, so there's no telling when it has asked: give the
    // threads time to, before `initialize` is answered.
    thread::sleep(Duration::from_millis(300));
    let answers = finish_initializing(&s.fake, &s.clock, &s.fake_clock, calls);
    assert_eq!(answers, vec![Err(NavError::Unsupported); 3]);
    for method in methods {
        assert_eq!(sent(&s.fake, method), 0);
    }
}

#[test]
fn a_stop_while_waiting_answers_server_down() {
    let (mut s, server) = Starting::start(ServerCapabilities::default());
    let (id, version) = (s.buffer.id(), s.buffer.version());
    let highlights = s.manager.document_highlights(id, version, 4);
    let rename = s.manager.rename(id, version, 4, "other".into());
    let calls = vec![
        thread::spawn(move || async_io::block_on(highlights).map(|_| ())),
        thread::spawn(move || async_io::block_on(rename).map(|_| ())),
    ];
    s.fake.wait_for("initialize", |records| {
        records.iter().any(|r| r.method() == "initialize")
    });
    s.manager.stop(server);
    // Only the poll interval passes, far short of any timeout.
    let deadline = Instant::now() + WAIT;
    while !calls.iter().all(JoinHandle::is_finished) && Instant::now() < deadline {
        s.clock.advance(Duration::from_millis(20));
        thread::sleep(Duration::from_millis(5));
    }
    let answers: Vec<_> = (calls.into_iter())
        .map(|call| call.join().unwrap_or_else(|_| unreachable!()))
        .collect();
    assert_eq!(answers, vec![Err(LspError::ServerDown); 2]);
}

#[test]
fn a_queued_prepare_rename_times_out_while_the_server_initializes() {
    // The local fallback (no `prepareProvider`) is covered by the deadline
    // too, though it sends nothing: the wait for initialization is what's
    // timed.
    let s = Starting::new(ServerCapabilities {
        rename_provider: Some(OneOf::Left(true)),
        ..ServerCapabilities::default()
    });
    let (id, version) = (s.buffer.id(), s.buffer.version());
    let prepare = s.manager.prepare_rename(id, version, 4);
    let rename = s.manager.rename(id, version, 4, "other".into());
    let calls = vec![
        thread::spawn(move || async_io::block_on(prepare).map(|_| ())),
        thread::spawn(move || async_io::block_on(rename).map(|_| ())),
    ];
    // `initialize` is never answered; only the manager's clock moves.
    let deadline = Instant::now() + WAIT;
    while !calls.iter().all(JoinHandle::is_finished) && Instant::now() < deadline {
        s.clock.advance(crate::rename::RENAME_TIMEOUT);
        thread::sleep(Duration::from_millis(5));
    }
    let answers: Vec<_> = (calls.into_iter())
        .map(|call| call.join().unwrap_or_else(|_| unreachable!()))
        .collect();
    assert_eq!(answers, vec![Err(LspError::Timeout); 2]);
    assert_eq!(s.sent("textDocument/rename"), 0);
}
