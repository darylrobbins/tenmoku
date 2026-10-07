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
//! message it receives, with request ids, and every reply it sends, each
//! tagged with the launch (1, 2, …) it belongs to, and it can send
//! requests and notifications of its own or crash on cue. It can also hold
//! or step the client's writes to it (see [`Writes`]).

use std::collections::HashMap;
use std::io;
use std::marker::PhantomData;
use std::ops::ControlFlow;
use std::path::Path;
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
use lsp_types::{InitializeResult, NumberOrString, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use crate::clock::{Clock, SystemClock};
use crate::launcher::{ExitInfo, ServerIo, ServerLauncher, ServerProcess, ServerSpec};

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
        }
    }
}

/// An `InitializeResult` with default capabilities and the given encoding.
pub fn initialize_result(encoding: Option<PositionEncodingKind>) -> InitializeResult {
    InitializeResult {
        capabilities: ServerCapabilities {
            position_encoding: encoding,
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
}

impl Record {
    pub fn method(&self) -> &str {
        match self {
            Self::Request { method, .. }
            | Self::Notification { method, .. }
            | Self::Replied { method, .. } => method,
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
    kills: usize,
    /// Every launch, in order: launch `n` is `sessions[n - 1]`.
    sessions: Vec<Launched>,
    writes: Writes,
    /// The client's writes waiting while they're held.
    held: Vec<Waker>,
    /// Stepped writes let through and not yet made.
    allowed: usize,
    /// A stepped write is blocked, waiting to be let through.
    write_waiting: bool,
}

/// Ends the fake's main loop.
struct Stop;

/// One launch's handle on the client and on its exit. Each session's
/// process, thread and router act only on their own.
#[derive(Clone)]
struct Launched {
    socket: ClientSocket,
    exit: ExitSender,
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
                    kills: 0,
                    sessions: Vec::new(),
                    writes: Writes::Open,
                    held: Vec::new(),
                    allowed: 0,
                    write_waiting: false,
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

    /// The name of the thread each launch ran on.
    pub fn launch_threads(&self) -> Vec<Option<String>> {
        self.lock().launch_threads.clone()
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

    /// With writes stepped, lets one more through.
    pub fn allow_write(&self) {
        self.lock().allowed += 1;
        self.shared.changed.notify_all();
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
        let socket = self.socket();
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
    fn launch(&self, spec: &ServerSpec, _root: &Path) -> io::Result<ServerIo> {
        let (client_reads, server_writes) = pipe();
        let (server_reads, client_writes) = pipe();
        let (exit, exited) = oneshot::channel();
        let exit: ExitSender = Arc::new(Mutex::new(Some(exit)));
        let mut state = self.fake.lock();
        let launch = state.sessions.len() + 1;
        let fake = self.fake.clone();
        let (main_loop, socket) =
            MainLoop::new_server(|client| fake_router(fake, launch, client, exit.clone()));
        let session = Launched { socket, exit };
        state
            .launch_threads
            .push(thread::current().name().map(str::to_owned));
        state.sessions.push(session.clone());
        drop(state);
        let ended = session.exit.clone();
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
            stderr: None,
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
        self.fake.lock().kills += 1;
        self.session.end(ExitInfo {
            code: None,
            signal: Some(9),
        });
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
            fake.record(
                launch,
                Record::Request {
                    id: req.id.clone(),
                    method: req.method.clone(),
                    params: req.params.clone(),
                },
            );
            let script = fake.lock().script.clone();
            let clock = fake.shared.clock.clone();
            async move {
                let answer = answer(&script, &*clock, &req).await;
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
            fake.record(
                session.launch,
                Record::Notification {
                    method: notif.method.clone(),
                    params: notif.params,
                },
            );
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
