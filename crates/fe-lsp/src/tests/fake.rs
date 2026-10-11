//! A fake language server for tests (docs/decisions/lsp-client.md §6.1).
//!
//! It is async-lsp's server side, run on a thread of its own and wired to
//! the client through an in-memory duplex, with no subprocess.
//! [`FakeLauncher`] hands that duplex to [`LspManager`](crate::LspManager) as
//! its [`ServerIo`], so the launch seam is exercised too.
//!
//! A [`Script`] says how it answers `initialize`, `shutdown`, `exit` and
//! any other method; a method it doesn't script is echoed back (see
//! [`Echo`]). Its delays are measured on a [`Clock`]. It records every
//! message it receives, with request ids, including the client's answers
//! to its own requests, and every reply it sends, each tagged with the
//! launch (1, 2, …) it belongs to. It can send requests and notifications
//! of its own, write to stderr, crash or exit on cue, and answer the
//! client's `$/cancelRequest`s. It can also hold or step the client's
//! writes to it (see [`Writes`]).
//!
//! It keeps a [`Replica`] of each document the client syncs to it,
//! applying `didOpen`, `didChange`, `didSave` and `didClose` with a
//! position converter of its own (§4.4), never fe-lsp's, and records what
//! it can't apply as [`FakeServer::sync_errors`].

use std::collections::HashMap;
use std::io;
use std::marker::PhantomData;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker, ready};
use std::thread;
use std::time::{Duration, Instant};

use async_lsp::router::Router;
use async_lsp::{AnyNotification, AnyRequest, ClientSocket, MainLoop, ResponseError};
use futures::channel::{mpsc, oneshot};
use futures::{AsyncRead, AsyncWrite, Future, FutureExt, StreamExt};
use lsp_types::notification::Notification;
use lsp_types::request::Request;
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, InitializeResult, NumberOrString, Position, PositionEncodingKind,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, Url,
};
use serde_json::{Value, json};

use crate::clock::{Clock, SystemClock};
use crate::launcher::{
    ExitInfo, LaunchCancel, ServerIo, ServerLauncher, ServerProcess, ServerSpec,
};

/// How the fake behaves.
#[derive(Clone, Debug)]
pub struct Script {
    pub initialize: InitializeReply,
    /// How long to wait, on the fake's clock, before answering
    /// `initialize`.
    pub initialize_delay: Duration,
    /// Whether `shutdown` gets an answer at all.
    pub answer_shutdown: bool,
    pub on_exit: OnExit,
    /// Answers to other requests, by method. Unscripted methods are
    /// echoed.
    pub replies: HashMap<String, Scripted>,
    /// Whether a `$/cancelRequest` for a request still being answered
    /// answers it at once with `RequestCancelled` (-32800), as a compliant
    /// server does. Otherwise it's only recorded.
    pub answer_cancels: bool,
}

/// A scripted answer to a request, given after `delay` on the fake's
/// clock.
#[derive(Clone, Debug)]
pub struct Scripted {
    pub reply: Reply,
    pub delay: Duration,
}

impl Scripted {
    pub fn now(reply: Reply) -> Self {
        Self {
            reply,
            delay: Duration::ZERO,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Reply {
    Result(Value),
    Error {
        code: i32,
        message: String,
    },
    /// Never answered.
    Never,
}

impl Default for Script {
    /// Answers `initialize` choosing utf-8, answers `shutdown`, and exits
    /// with code 0 on `exit`.
    fn default() -> Self {
        Self {
            initialize: InitializeReply::Result(Box::new(initialize_result(Some(
                PositionEncodingKind::UTF8,
            )))),
            initialize_delay: Duration::ZERO,
            answer_shutdown: true,
            on_exit: OnExit::Exit(0),
            replies: HashMap::new(),
            answer_cancels: false,
        }
    }
}

/// An `InitializeResult` with the given encoding, incremental sync with
/// `didOpen`, `didClose` and `didSave` (without the text), and otherwise
/// default capabilities.
pub fn initialize_result(encoding: Option<PositionEncodingKind>) -> InitializeResult {
    InitializeResult {
        capabilities: ServerCapabilities {
            position_encoding: encoding,
            text_document_sync: Some(TextDocumentSyncCapability::Options(
                TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(TextDocumentSyncKind::INCREMENTAL),
                    will_save: None,
                    will_save_wait_until: None,
                    save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                },
            )),
            ..ServerCapabilities::default()
        },
        server_info: None,
    }
}

#[derive(Clone, Debug)]
pub enum InitializeReply {
    Result(Box<InitializeResult>),
    Error {
        code: i32,
        message: String,
    },
    /// Never answered.
    Never,
}

#[derive(Clone, Copy, Debug)]
pub enum OnExit {
    /// Close the transport (EOF) and exit with this code.
    Exit(i32),
    /// Close the transport (EOF), but keep running until killed.
    CloseOnly,
    /// Carry on as if nothing was said, until killed.
    Ignore,
}

/// A document as the fake has it, built from the client's notifications.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replica {
    pub language_id: String,
    /// From the latest `didOpen` or `didChange`.
    pub version: i32,
    pub text: String,
    /// Each `didSave`, in order.
    pub saves: Vec<Saved>,
    /// Until `didClose`.
    pub open: bool,
    /// The launch that opened it: each launch is a server of its own, so
    /// another's notifications don't apply to it, and its `didOpen`
    /// replaces it.
    pub launch: usize,
}

/// A `didSave` as the fake got it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    /// The document's version when it came.
    pub version: i32,
    /// The text it carried, if any.
    pub text: Option<String>,
}

/// Something the fake received or sent, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Record {
    Request {
        id: NumberOrString,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    /// The fake answered request `id`.
    Replied {
        id: NumberOrString,
        method: String,
    },
    /// The client answered the fake's request `id`: its result, or its
    /// error's code and message.
    Response {
        id: NumberOrString,
        result: Result<Value, (i32, String)>,
    },
}

impl Record {
    pub fn method(&self) -> &str {
        match self {
            Self::Request { method, .. }
            | Self::Notification { method, .. }
            | Self::Replied { method, .. } => method,
            Self::Response { .. } => "",
        }
    }

    pub fn is_notification(&self, name: &str) -> bool {
        matches!(self, Self::Notification { method, .. } if method == name)
    }
}

/// The default answer: a method the script doesn't name is answered
/// `{"id": <its id>, "params": <its params>}`, after `params.delay_ms` on
/// the fake's clock, or never if `params.never` is true.
pub enum Echo {}

impl Request for Echo {
    type Params = Value;
    type Result = Value;
    const METHOD: &'static str = "fake/echo";
}

/// How the client's writes to the fake get through, for every launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writes {
    /// As they're made.
    Open,
    /// Not for now: each waits (`Pending`), as when a server stops reading
    /// its input, until writes are open or stepped.
    Held,
    /// One at a time: each blocks the client's thread until
    /// [`FakeServer::allow_write`] lets it through, so the server reads
    /// whatever is sent, and the test sees what the client did in between.
    /// Meant for one launch's writes at a time.
    Stepped,
    /// One at a time, as a slow reader takes them: each waits (`Pending`)
    /// until [`FakeServer::allow_write`] lets one through, without blocking
    /// the client's thread meanwhile.
    Paced,
}

/// An error a client answered the fake's request with.
#[derive(Clone, Debug, PartialEq)]
pub struct FakeError {
    /// `None` when the request got no answer at all.
    pub code: Option<i32>,
    pub message: String,
}

/// The fake server's script, record and controls. Clones share them.
#[derive(Clone)]
pub struct FakeServer {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    clock: Arc<dyn Clock>,
}

struct State {
    script: Script,
    /// Each with the launch it belongs to.
    records: Vec<(usize, Record)>,
    launch_threads: Vec<Option<String>>,
    /// What each launch was asked to start, and where.
    launched: Vec<(ServerSpec, PathBuf)>,
    kills: usize,
    /// While set, a kill closes the transport but the exit waits in
    /// `killed` until [`FakeServer::hold_kills`] clears it.
    hold_kills: bool,
    killed: Vec<ExitSender>,
    /// Every launch, in order: launch `n` is `sessions[n - 1]`.
    sessions: Vec<Launched>,
    writes: Writes,
    /// The client's writes waiting while they're held.
    held: Vec<Waker>,
    /// Stepped writes let through and not yet made.
    allowed: usize,
    /// A stepped write is blocked, waiting to be let through.
    write_waiting: bool,
    documents: HashMap<Url, Replica>,
    /// Document notifications the fake couldn't apply, and why.
    sync_errors: Vec<String>,
    /// Answers `RequestCancelled` to the client's requests being answered,
    /// by launch and id, while `answer_cancels` is set.
    cancels: HashMap<(usize, String), oneshot::Sender<()>>,
}

/// Ends the fake's main loop.
struct Stop;

/// One launch's handle on the client and on its exit. Each session's
/// process, thread and router act only on their own.
#[derive(Clone)]
struct Launched {
    socket: ClientSocket,
    exit: ExitSender,
    stderr: mpsc::UnboundedSender<Vec<u8>>,
}

type ExitSender = Arc<Mutex<Option<oneshot::Sender<ExitInfo>>>>;

impl Launched {
    /// Closes the transport, and exits if it hasn't yet.
    fn end(&self, exit: ExitInfo) {
        let _ = self.socket.emit(Stop);
        send_exit(&self.exit, exit);
    }
}

fn send_exit(sender: &ExitSender, exit: ExitInfo) {
    let sender = sender.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(sender) = sender {
        let _ = sender.send(exit);
    }
}

const WAIT: Duration = Duration::from_secs(10);

/// How a killed launch exits.
const KILLED: ExitInfo = ExitInfo {
    code: None,
    signal: Some(9),
};

impl FakeServer {
    /// A fake whose delays are real time.
    pub fn new(script: Script) -> Self {
        Self::with_clock(script, Arc::new(SystemClock))
    }

    /// A fake whose delays are measured on `clock`.
    pub fn with_clock(script: Script, clock: Arc<dyn Clock>) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    script,
                    records: Vec::new(),
                    launch_threads: Vec::new(),
                    launched: Vec::new(),
                    kills: 0,
                    hold_kills: false,
                    killed: Vec::new(),
                    sessions: Vec::new(),
                    writes: Writes::Open,
                    held: Vec::new(),
                    allowed: 0,
                    write_waiting: false,
                    documents: HashMap::new(),
                    sync_errors: Vec::new(),
                    cancels: HashMap::new(),
                }),
                changed: Condvar::new(),
                clock,
            }),
        }
    }

    pub fn launcher(&self) -> Arc<FakeLauncher> {
        Arc::new(FakeLauncher { fake: self.clone() })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared.lock()
    }

    /// Everything recorded, across launches.
    pub fn records(&self) -> Vec<Record> {
        records(&self.lock(), None)
    }

    /// Everything recorded, in order, each with its launch.
    pub fn timeline(&self) -> Vec<(usize, Record)> {
        self.lock().records.clone()
    }

    /// What launch `launch` (from 1) recorded.
    pub fn records_of(&self, launch: usize) -> Vec<Record> {
        records(&self.lock(), Some(launch))
    }

    /// How many times it has been launched.
    pub fn launches(&self) -> usize {
        self.lock().sessions.len()
    }

    /// Blocks until `done` holds for the record across launches, and
    /// returns it. Panics after 10 s, naming `what`.
    pub fn wait_for(&self, what: &str, done: impl Fn(&[Record]) -> bool) -> Vec<Record> {
        self.wait_for_in(None, what, done)
    }

    /// As [`wait_for`](Self::wait_for), for launch `launch`'s record only.
    pub fn wait_for_at(
        &self,
        launch: usize,
        what: &str,
        done: impl Fn(&[Record]) -> bool,
    ) -> Vec<Record> {
        self.wait_for_in(Some(launch), what, done)
    }

    fn wait_for_in(
        &self,
        launch: Option<usize>,
        what: &str,
        done: impl Fn(&[Record]) -> bool,
    ) -> Vec<Record> {
        let deadline = Instant::now() + WAIT;
        let mut state = self.lock();
        loop {
            let records = records(&state, launch);
            if done(&records) {
                return records;
            }
            let now = Instant::now();
            assert!(
                now < deadline,
                "fake: timed out waiting for {what}: {records:#?}"
            );
            state = self
                .shared
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Blocks until the client has sent `initialized` to the first launch.
    pub fn wait_initialized(&self) {
        self.wait_initialized_at(1);
    }

    /// Blocks until the client has sent `initialized` to launch `launch`.
    pub fn wait_initialized_at(&self, launch: usize) {
        self.wait_for_at(launch, "initialized", |records| {
            records.iter().any(|r| r.is_notification("initialized"))
        });
    }

    /// The spec and root each launch was given, in order.
    pub fn launched(&self) -> Vec<(ServerSpec, PathBuf)> {
        self.lock().launched.clone()
    }

    /// The name of the thread each launch ran on.
    pub fn launch_threads(&self) -> Vec<Option<String>> {
        self.lock().launch_threads.clone()
    }

    /// While `hold` is set, a killed launch closes its transport at once
    /// but exits only when this is called again with `false`, as a process
    /// takes a while to go after `SIGKILL`.
    pub fn hold_kills(&self, hold: bool) {
        let killed = {
            let mut state = self.lock();
            state.hold_kills = hold;
            if hold {
                Vec::new()
            } else {
                std::mem::take(&mut state.killed)
            }
        };
        for exit in killed {
            send_exit(&exit, KILLED);
        }
    }

    /// How many times the client killed it.
    pub fn kills(&self) -> usize {
        self.lock().kills
    }

    /// Sets how the client's writes get through from now on, dropping any
    /// stepped writes allowed and not yet made.
    pub fn set_writes(&self, writes: Writes) {
        let mut state = self.lock();
        state.writes = writes;
        state.allowed = 0;
        let held = std::mem::take(&mut state.held);
        drop(state);
        self.shared.changed.notify_all();
        for waker in held {
            waker.wake();
        }
    }

    /// Whether, with writes stepped, the client's thread is blocked on a
    /// write until [`allow_write`](Self::allow_write).
    pub fn write_waiting(&self) -> bool {
        let state = self.lock();
        state.writes == Writes::Stepped && state.write_waiting && state.allowed == 0
    }

    /// With writes stepped or paced, lets one more through.
    pub fn allow_write(&self) {
        let mut state = self.lock();
        state.allowed += 1;
        let held = std::mem::take(&mut state.held);
        drop(state);
        self.shared.changed.notify_all();
        for waker in held {
            waker.wake();
        }
    }

    /// With writes paced, blocks until the writes let through have been
    /// made and the client waits to write again. Panics after 10 s.
    pub fn wait_paced(&self) {
        let deadline = Instant::now() + WAIT;
        let mut state = self.lock();
        while state.allowed > 0 || state.held.is_empty() {
            let now = Instant::now();
            assert!(now < deadline, "fake: the paced writes weren't made");
            state = self
                .shared
                .changed
                .wait_timeout(state, (deadline - now).min(Duration::from_millis(10)))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Whether a client write may go now. While writes are stepped, it
    /// blocks the client's thread until the write is let through; it panics
    /// after 10 s.
    fn pass_write(&self, cx: &mut Context<'_>) -> Poll<()> {
        let deadline = Instant::now() + WAIT;
        let mut state = self.lock();
        loop {
            state.write_waiting = false;
            match state.writes {
                Writes::Open => return Poll::Ready(()),
                Writes::Held => {
                    if !state.held.iter().any(|held| held.will_wake(cx.waker())) {
                        state.held.push(cx.waker().clone());
                    }
                    return Poll::Pending;
                }
                Writes::Paced if state.allowed > 0 => {
                    state.allowed -= 1;
                    return Poll::Ready(());
                }
                Writes::Paced => {
                    if !state.held.iter().any(|held| held.will_wake(cx.waker())) {
                        state.held.push(cx.waker().clone());
                    }
                    return Poll::Pending;
                }
                Writes::Stepped if state.allowed > 0 => {
                    state.allowed -= 1;
                    return Poll::Ready(());
                }
                Writes::Stepped => {
                    let now = Instant::now();
                    assert!(
                        now < deadline,
                        "fake: a write waited {WAIT:?} to be let through"
                    );
                    state.write_waiting = true;
                    self.shared.changed.notify_all();
                    state = self
                        .shared
                        .changed
                        .wait_timeout(state, deadline - now)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
            }
        }
    }

    /// The latest launch.
    fn session(&self) -> Launched {
        self.lock()
            .sessions
            .last()
            .cloned()
            .expect("fake: not launched")
    }

    fn socket(&self) -> ClientSocket {
        self.session().socket
    }

    /// Sends a notification to the client, from the latest launch.
    pub fn notify<N: Notification>(&self, params: N::Params) {
        self.socket()
            .notify::<N>(params)
            .expect("fake: the session has ended");
    }

    /// Sends a request to the client, from the latest launch, and blocks
    /// for its answer, as JSON.
    pub fn request<R: Request>(&self, params: R::Params) -> Result<Value, FakeError> {
        self.request_at::<R>(self.launches(), params)
    }

    /// As [`request`](Self::request), from launch `launch`.
    pub fn request_at<R: Request>(
        &self,
        launch: usize,
        params: R::Params,
    ) -> Result<Value, FakeError> {
        let socket = self.lock().sessions[launch - 1].socket.clone();
        let reply = async move { socket.request::<Raw<R>>(params).await }.fuse();
        // A watchdog for the test, so real time whatever the fake's clock.
        let timeout = SystemClock.sleep(WAIT).fuse();
        futures::pin_mut!(reply, timeout);
        async_io::block_on(async {
            futures::select_biased! {
                reply = reply => reply.map_err(|err| match err {
                    async_lsp::Error::Response(err) => FakeError {
                        code: Some(err.code.0),
                        message: err.message,
                    },
                    err => FakeError { code: None, message: err.to_string() },
                }),
                _ = timeout => Err(FakeError {
                    code: None,
                    message: format!("no answer to {} in {WAIT:?}", R::METHOD),
                }),
            }
        })
    }

    /// Changes the script's answer to `method` from now on: for every
    /// request not yet recorded.
    pub fn set_reply(&self, method: &str, reply: Scripted) {
        self.lock().script.replies.insert(method.to_owned(), reply);
    }

    /// Writes `bytes` to the latest launch's stderr.
    pub fn stderr(&self, bytes: &[u8]) {
        let _ = self.session().stderr.unbounded_send(bytes.to_vec());
    }

    /// Exits the latest launch with `code`, leaving its transport open, as
    /// when a descendant still holds the server's stdout.
    pub fn exit_keeping_output(&self, code: i32) {
        send_exit(
            &self.session().exit,
            ExitInfo {
                code: Some(code),
                signal: None,
            },
        );
    }

    /// Crashes the latest launch: closes the transport (the client reads
    /// EOF) and exits with code 1.
    pub fn crash(&self) {
        self.crash_at(self.launches());
    }

    /// Crashes launch `launch`, as [`crash`](Self::crash).
    pub fn crash_at(&self, launch: usize) {
        let session = self.lock().sessions[launch - 1].clone();
        session.end(ExitInfo {
            code: Some(1),
            signal: None,
        });
    }

    fn record(&self, launch: usize, record: Record) {
        self.lock().records.push((launch, record));
        self.shared.changed.notify_all();
    }

    /// The replica of the document at `uri`, if it was ever opened.
    pub fn document(&self, uri: &Url) -> Option<Replica> {
        self.lock().documents.get(uri).cloned()
    }

    /// Blocks until `done` holds for the replica of `uri` (`None` until
    /// it's opened), and returns it. Panics after 10 s, naming `what`.
    pub fn wait_for_document(
        &self,
        uri: &Url,
        what: &str,
        done: impl Fn(Option<&Replica>) -> bool,
    ) -> Option<Replica> {
        let deadline = Instant::now() + WAIT;
        let mut state = self.lock();
        loop {
            let replica = state.documents.get(uri);
            if done(replica) {
                return replica.cloned();
            }
            let now = Instant::now();
            assert!(
                now < deadline,
                "fake: timed out waiting for {what}: {replica:#?}"
            );
            state = self
                .shared
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Every document notification the fake couldn't apply, and why.
    pub fn sync_errors(&self) -> Vec<String> {
        self.lock().sync_errors.clone()
    }

    /// Records a notification, applying it to the replicas first, under
    /// one lock, so a test that sees the record sees its effect too.
    fn record_notification(&self, launch: usize, method: &str, params: &Value) {
        let mut state = self.lock();
        let utf16 = !negotiated_utf8(&state.script);
        if let Err(err) = apply(&mut state.documents, launch, method, params, utf16) {
            state.sync_errors.push(format!("{method}: {err}"));
        }
        state.records.push((
            launch,
            Record::Notification {
                method: method.to_owned(),
                params: params.clone(),
            },
        ));
        drop(state);
        self.shared.changed.notify_all();
    }
}

/// Whether the script's `initialize` answer chose utf-8. Anything else
/// (no answer, no field) is utf-16.
fn negotiated_utf8(script: &Script) -> bool {
    matches!(
        &script.initialize,
        InitializeReply::Result(result)
            if result.capabilities.position_encoding == Some(PositionEncodingKind::UTF8)
    )
}

/// Applies a document notification to `documents`; other methods are
/// ignored.
fn apply(
    documents: &mut HashMap<Url, Replica>,
    launch: usize,
    method: &str,
    params: &Value,
    utf16: bool,
) -> Result<(), String> {
    fn parse<T: serde::de::DeserializeOwned>(params: &Value) -> Result<T, String> {
        serde_json::from_value(params.clone()).map_err(|err| err.to_string())
    }
    fn open<'a>(
        documents: &'a mut HashMap<Url, Replica>,
        uri: &Url,
        launch: usize,
    ) -> Result<&'a mut Replica, String> {
        documents
            .get_mut(uri)
            .filter(|replica| replica.open && replica.launch == launch)
            .ok_or_else(|| format!("{uri} isn't open"))
    }
    match method {
        "textDocument/didOpen" => {
            let DidOpenTextDocumentParams { text_document } = parse(params)?;
            if documents
                .get(&text_document.uri)
                .is_some_and(|r| r.open && r.launch == launch)
            {
                return Err(format!("{} is already open", text_document.uri));
            }
            documents.insert(
                text_document.uri,
                Replica {
                    language_id: text_document.language_id,
                    version: text_document.version,
                    text: text_document.text,
                    saves: Vec::new(),
                    open: true,
                    launch,
                },
            );
        }
        "textDocument/didChange" => {
            let DidChangeTextDocumentParams {
                text_document,
                content_changes,
            } = parse(params)?;
            let replica = open(documents, &text_document.uri, launch)?;
            if text_document.version <= replica.version {
                return Err(format!(
                    "version {} after {}",
                    text_document.version, replica.version
                ));
            }
            replica.version = text_document.version;
            for change in content_changes {
                let Some(range) = change.range else {
                    replica.text = change.text;
                    continue;
                };
                let start = reference_offset(&replica.text, range.start, utf16)?;
                let end = reference_offset(&replica.text, range.end, utf16)?;
                if end < start {
                    return Err(format!("{range:?} ends before it starts"));
                }
                replica.text.replace_range(start..end, &change.text);
            }
        }
        "textDocument/didSave" => {
            let DidSaveTextDocumentParams {
                text_document,
                text,
            } = parse(params)?;
            let replica = open(documents, &text_document.uri, launch)?;
            replica.saves.push(Saved {
                version: replica.version,
                text,
            });
        }
        "textDocument/didClose" => {
            let DidCloseTextDocumentParams { text_document } = parse(params)?;
            open(documents, &text_document.uri, launch)?.open = false;
        }
        _ => {}
    }
    Ok(())
}

/// The byte offset of `position` in `text`, as the fake reads LSP: a line
/// ends at `\n`, `\r\n` or `\r`, and a column counts UTF-16 code units
/// (`utf16`) or bytes, never including the line break. Written over
/// `char_indices` and `char::len_utf16`, apart from fe-lsp's conversion
/// through fe-text. It's strict: a position past its line's end, past the
/// last line, or inside a char is an error, since fe-lsp never sends one.
pub fn reference_offset(text: &str, position: Position, utf16: bool) -> Result<usize, String> {
    let mut chars = text.char_indices().peekable();
    let mut line_start = 0;
    for _ in 0..position.line {
        loop {
            match chars.next() {
                None => return Err(format!("{position:?} is past the last line")),
                Some((at, '\n')) => {
                    line_start = at + 1;
                    break;
                }
                Some((at, '\r')) => {
                    line_start = at + 1;
                    if let Some(&(_, '\n')) = chars.peek() {
                        chars.next();
                        line_start += 1;
                    }
                    break;
                }
                Some(_) => {}
            }
        }
    }
    let mut column = 0;
    for (at, c) in text[line_start..].char_indices() {
        if column == position.character {
            return Ok(line_start + at);
        }
        if c == '\n' || c == '\r' {
            return Err(format!("{position:?} is past its line's end"));
        }
        column += if utf16 { c.len_utf16() } else { c.len_utf8() } as u32;
        if column > position.character {
            return Err(format!("{position:?} is inside {c:?}"));
        }
    }
    if column == position.character {
        // The last line's end.
        return Ok(text.len());
    }
    Err(format!("{position:?} is past its line's end"))
}

fn records(state: &State, launch: Option<usize>) -> Vec<Record> {
    state
        .records
        .iter()
        .filter(|(from, _)| launch.is_none_or(|launch| *from == launch))
        .map(|(_, record)| record.clone())
        .collect()
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Launches the fake: a new session per launch, all sharing one record.
pub struct FakeLauncher {
    fake: FakeServer,
}

impl ServerLauncher for FakeLauncher {
    fn launch(&self, spec: &ServerSpec, root: &Path, _: &LaunchCancel) -> io::Result<ServerIo> {
        let (client_reads, server_writes) = pipe();
        let (server_reads, client_writes) = pipe();
        let (stderr_reads, stderr_writes) = pipe();
        let (exit, exited) = oneshot::channel();
        let exit: ExitSender = Arc::new(Mutex::new(Some(exit)));
        let mut state = self.fake.lock();
        let launch = state.sessions.len() + 1;
        let fake = self.fake.clone();
        let (main_loop, socket) =
            MainLoop::new_server(|client| fake_router(fake, launch, client, exit.clone()));
        let session = Launched {
            socket,
            exit,
            stderr: stderr_writes.0,
        };
        state
            .launch_threads
            .push(thread::current().name().map(str::to_owned));
        state.launched.push((spec.clone(), root.to_owned()));
        state.sessions.push(session.clone());
        drop(state);
        let ended = session.exit.clone();
        let server_reads = Tap {
            pipe: server_reads,
            fake: self.fake.clone(),
            launch,
            seen: Vec::new(),
        };
        thread::Builder::new()
            .name(format!("fake {}", spec.name))
            .spawn(move || {
                let ran = async_io::block_on(main_loop.run_buffered(server_reads, server_writes));
                // The client hung up (EOF) rather than the fake ending the
                // session: exit as a real server would.
                if ran.is_err() {
                    send_exit(&ended, ExitInfo::default());
                }
            })?;
        Ok(ServerIo {
            input: Box::pin(Gated {
                pipe: client_writes,
                fake: self.fake.clone(),
            }),
            output: Box::pin(client_reads),
            stderr: Some(Box::pin(stderr_reads)),
            process: Box::new(FakeProcess {
                fake: self.fake.clone(),
                session,
                exited: Some(exited),
                exit: None,
            }),
        })
    }
}

struct FakeProcess {
    fake: FakeServer,
    session: Launched,
    exited: Option<oneshot::Receiver<ExitInfo>>,
    exit: Option<ExitInfo>,
}

impl ServerProcess for FakeProcess {
    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>> {
        Box::pin(async move {
            if let Some(exited) = self.exited.as_mut() {
                let exit = exited.await.unwrap_or_default();
                self.exited = None;
                self.exit = Some(exit);
            }
            self.exit.unwrap_or_default()
        })
    }

    fn kill(&mut self) {
        let mut state = self.fake.lock();
        state.kills += 1;
        if state.hold_kills {
            state.killed.push(self.session.exit.clone());
            drop(state);
            let _ = self.session.socket.emit(Stop);
            return;
        }
        drop(state);
        self.session.end(KILLED);
    }
}

struct Session {
    fake: FakeServer,
    launch: usize,
    exit: ExitSender,
    /// Keeps the main loop's event channel open while it runs.
    _client: ClientSocket,
}

fn fake_router(
    fake: FakeServer,
    launch: usize,
    client: ClientSocket,
    exit: ExitSender,
) -> Router<Session> {
    let mut router = Router::new(Session {
        fake,
        launch,
        exit,
        _client: client,
    });
    router
        .unhandled_request(|session, req: AnyRequest| {
            let fake = session.fake.clone();
            let launch = session.launch;
            // Taken before the request is recorded, so a test that saw it
            // recorded knows what it will be answered.
            let script = fake.lock().script.clone();
            fake.record(
                launch,
                Record::Request {
                    id: req.id.clone(),
                    method: req.method.clone(),
                    params: req.params.clone(),
                },
            );
            let clock = fake.shared.clock.clone();
            let cancelled = script.answer_cancels.then(|| {
                let (cancel, cancelled) = oneshot::channel();
                fake.lock()
                    .cancels
                    .insert((launch, id_key(&req.id)), cancel);
                cancelled
            });
            async move {
                let answer = {
                    let answer = answer(&script, &*clock, &req).fuse();
                    let cancelled = async {
                        let cancelled = match cancelled {
                            Some(cancelled) => cancelled.await.is_ok(),
                            None => false,
                        };
                        if !cancelled {
                            futures::future::pending::<()>().await;
                        }
                    }
                    .fuse();
                    futures::pin_mut!(answer, cancelled);
                    futures::select_biased! {
                        answer = answer => answer,
                        () = cancelled => Err(ResponseError::new(
                            async_lsp::ErrorCode::REQUEST_CANCELLED,
                            "cancelled",
                        )),
                    }
                };
                fake.lock().cancels.remove(&(launch, id_key(&req.id)));
                fake.record(
                    launch,
                    Record::Replied {
                        id: req.id,
                        method: req.method,
                    },
                );
                answer
            }
        })
        .unhandled_notification(|session, notif: AnyNotification| {
            let fake = &session.fake;
            fake.record_notification(session.launch, &notif.method, &notif.params);
            if notif.method == "$/cancelRequest"
                && let Some(id) = notif.params.get("id")
                && let Ok(id) = serde_json::from_value::<NumberOrString>(id.clone())
            {
                let cancel = fake.lock().cancels.remove(&(session.launch, id_key(&id)));
                if let Some(cancel) = cancel {
                    let _ = cancel.send(());
                }
            }
            if notif.method == "exit" {
                let on_exit = fake.lock().script.on_exit;
                match on_exit {
                    OnExit::Exit(code) => {
                        send_exit(
                            &session.exit,
                            ExitInfo {
                                code: Some(code),
                                signal: None,
                            },
                        );
                        return ControlFlow::Break(Ok(()));
                    }
                    OnExit::CloseOnly => return ControlFlow::Break(Ok(())),
                    OnExit::Ignore => {}
                }
            }
            ControlFlow::Continue(())
        })
        .event::<Stop>(|_, Stop| ControlFlow::Break(Ok(())));
    router
}

async fn answer(
    script: &Script,
    clock: &dyn Clock,
    req: &AnyRequest,
) -> Result<Value, ResponseError> {
    if let Some(Scripted { reply, delay }) = script.replies.get(&req.method) {
        if !delay.is_zero() {
            clock.sleep(*delay).await;
        }
        return match reply {
            Reply::Result(result) => Ok(result.clone()),
            Reply::Error { code, message } => Err(ResponseError::new((*code).into(), message)),
            Reply::Never => futures::future::pending().await,
        };
    }
    match req.method.as_str() {
        "initialize" => {
            if !script.initialize_delay.is_zero() {
                clock.sleep(script.initialize_delay).await;
            }
            match &script.initialize {
                InitializeReply::Result(result) => {
                    Ok(serde_json::to_value(result).expect("serialize InitializeResult"))
                }
                InitializeReply::Error { code, message } => {
                    Err(ResponseError::new((*code).into(), message))
                }
                InitializeReply::Never => futures::future::pending().await,
            }
        }
        "shutdown" if !script.answer_shutdown => futures::future::pending().await,
        "shutdown" => Ok(Value::Null),
        _ => {
            if req.params.get("never") == Some(&Value::Bool(true)) {
                return futures::future::pending().await;
            }
            let delay = req.params.get("delay_ms").and_then(Value::as_u64);
            if let Some(delay) = delay.filter(|&ms| ms > 0) {
                clock.sleep(Duration::from_millis(delay)).await;
            }
            Ok(json!({ "id": req.id, "params": req.params }))
        }
    }
}

fn id_key(id: &NumberOrString) -> String {
    match id {
        NumberOrString::Number(n) => n.to_string(),
        NumberOrString::String(s) => format!("{s:?}"),
    }
}

/// The fake's input, read through: records each response the client
/// sends (to the fake's own requests), which the main loop consumes
/// without telling the router.
struct Tap {
    pipe: PipeReader,
    fake: FakeServer,
    launch: usize,
    /// Bytes read and not yet parsed into whole messages.
    seen: Vec<u8>,
}

impl AsyncRead for Tap {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        let n = ready!(Pin::new(&mut this.pipe).poll_read(cx, out))?;
        this.seen.extend_from_slice(&out[..n]);
        while let Some((message, used)) = frame(&this.seen) {
            this.seen.drain(..used);
            let is_response = message.get("method").is_none();
            if let (true, Some(id)) = (is_response, message.get("id")) {
                let id = serde_json::from_value(id.clone()).unwrap_or(NumberOrString::Number(-1));
                let result = match message.get("error") {
                    Some(error) => Err((
                        error["code"].as_i64().unwrap_or(0) as i32,
                        error["message"].as_str().unwrap_or("").to_owned(),
                    )),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                this.fake
                    .record(this.launch, Record::Response { id, result });
            }
        }
        Poll::Ready(Ok(n))
    }
}

/// The first whole `Content-Length` framed message in `bytes`, and how
/// many bytes it took.
fn frame(bytes: &[u8]) -> Option<(Value, usize)> {
    let header_end = bytes.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
    let header = std::str::from_utf8(&bytes[..header_end]).ok()?;
    let length: usize = header.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())?
    })?;
    let body = bytes.get(header_end..header_end + length)?;
    let message = serde_json::from_slice(body).unwrap_or(Value::Null);
    Some((message, header_end + length))
}

/// `R`, answered as plain JSON.
struct Raw<R>(PhantomData<fn() -> R>);

impl<R: Request> Request for Raw<R> {
    type Params = R::Params;
    type Result = Value;
    const METHOD: &'static str = R::METHOD;
}

/// One direction of the in-memory duplex.
fn pipe() -> (PipeReader, PipeWriter) {
    let (sender, receiver) = mpsc::unbounded();
    (
        PipeReader {
            chunks: receiver,
            chunk: Vec::new(),
            read: 0,
        },
        PipeWriter(sender),
    )
}

struct PipeWriter(mpsc::UnboundedSender<Vec<u8>>);

impl AsyncWrite for PipeWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(match self.0.unbounded_send(buf.to_vec()) {
            Ok(()) => Ok(buf.len()),
            Err(_) => Err(io::ErrorKind::BrokenPipe.into()),
        })
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.0.close_channel();
        Poll::Ready(Ok(()))
    }
}

/// The client's end of its writes to the fake, let through as
/// [`FakeServer::set_writes`] says.
struct Gated {
    pipe: PipeWriter,
    fake: FakeServer,
}

impl AsyncWrite for Gated {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        ready!(self.fake.pass_write(cx));
        Pin::new(&mut self.pipe).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.pipe).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.pipe).poll_close(cx)
    }
}

struct PipeReader {
    chunks: mpsc::UnboundedReceiver<Vec<u8>>,
    chunk: Vec<u8>,
    read: usize,
}

impl AsyncRead for PipeReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        while this.read == this.chunk.len() {
            match ready!(this.chunks.poll_next_unpin(cx)) {
                Some(chunk) => {
                    this.chunk = chunk;
                    this.read = 0;
                }
                // Every writer has gone: EOF.
                None => return Poll::Ready(Ok(0)),
            }
        }
        let n = out.len().min(this.chunk.len() - this.read);
        out[..n].copy_from_slice(&this.chunk[this.read..this.read + n]);
        this.read += n;
        Poll::Ready(Ok(n))
    }
}
