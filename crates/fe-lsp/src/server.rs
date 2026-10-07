//! One thread per server, named `fe-lsp <name>`, running
//! `async_io::block_on(join3(main_loop, actor, stderr))`: §2.2's
//! `join(main_loop, actor)`, plus a drain for the server's stderr.
//!
//! - The **main loop** is async-lsp's `MainLoop` over the launcher's byte
//!   streams, answering the server through [`client_router`].
//! - The **actor** owns the session: it starts and stops the server
//!   (§3.4) and issues every request. async-lsp numbers requests from 0 in
//!   the order they reach its loop and never says which number it gave, so
//!   the actor, as the only issuer, mirrors that counter (§2.1 gap 2 (a)).
//!   What it hands async-lsp can't be taken back, so it keeps at most
//!   [`QUEUED_MAX`] requests in async-lsp's queue and holds the rest: a
//!   stop drops those, and `shutdown` goes next.
//! - The actor also keeps each open document in sync (§4.1, `document`),
//!   draining its subscription on a nudge and on a [`BACKSTOP`] tick. Its
//!   notifications take their turn in the same window as requests, so they
//!   stay in order with them, and a drain only runs when the window has
//!   room and what the last one made has gone.
//! - A request on a document is admitted there too (§4.1): the actor
//!   drains the document first, and sends the request right behind what
//!   that queued, only if the server has the version it was computed on.
//! - Every open it performs activates its identity first (identity.rs),
//!   and when the session ends, however it ends, its identities are
//!   retired before anything else is reported.

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use async_lsp::{MainLoop, ServerSocket};
use fe_buffer::{BufferId, Version};
use futures::channel::{mpsc, oneshot};
use futures::future::{BoxFuture, Either};
use futures::stream::FuturesUnordered;
use futures::{
    AsyncRead, AsyncReadExt, AsyncWrite, Future, FutureExt, StreamExt, pin_mut, select_biased,
};
use lsp_types::notification::{Exit, Initialized};
use lsp_types::request::{Initialize, Request, Shutdown};
use lsp_types::{
    ClientCapabilities, ClientInfo, CompletionClientCapabilities, CompletionItemCapability,
    CompletionItemCapabilityResolveSupport, DiagnosticTag, DynamicRegistrationClientCapabilities,
    GeneralClientCapabilities, GotoCapability, HoverClientCapabilities, InitializeParams,
    InitializeResult, InitializedParams, MarkupKind, PositionEncodingKind,
    PublishDiagnosticsClientCapabilities, ServerCapabilities, TagSupport,
    TextDocumentClientCapabilities, TextDocumentSyncClientCapabilities, Url,
    WindowClientCapabilities, WorkspaceClientCapabilities, WorkspaceFolder,
    WorkspaceSymbolClientCapabilities,
};

use crate::clock::Clock;
use crate::document::{Admitted, Document, Link, Note, Opening, SyncPolicy};
use crate::encoding::PositionEncoding;
use crate::event::{LspError, LspEvent, NOT_OPEN, ServerId, ServerStatus};
use crate::identity::{EndSession, Identities, SessionId};
use crate::launcher::{ServerIo, ServerLauncher, ServerProcess, ServerSpec};
use crate::router::{ClientState, client_router};

/// How long `initialize` may take before the start counts as failed.
pub(crate) const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);
/// How long `shutdown` may take before `exit` is sent anyway.
pub(crate) const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
/// How long the server has to go after `exit` before it's killed.
pub(crate) const EXIT_TIMEOUT: Duration = Duration::from_secs(1);
/// How many of the actor's requests may wait in async-lsp's queue for its
/// loop to take them. More wait with the actor, so `shutdown` follows at
/// most this many, plus the one the loop is writing.
pub(crate) const QUEUED_MAX: u64 = 32;
/// How often the actor drains every open document's subscription, so an
/// event the host never nudged about (a save after the edits were already
/// sent, say) still reaches the server.
pub(crate) const BACKSTOP: Duration = Duration::from_millis(250);

/// The manager's event sink, shared by every server thread.
pub(crate) type Sink = Arc<Mutex<dyn FnMut(LspEvent) + Send>>;

/// Issues one request on the server's socket, given the id async-lsp will
/// number it with. It must send before returning (see [`send_request`]),
/// and the future it returns answers the caller.
pub(crate) type Issue = Box<dyn FnOnce(&ServerSocket, i32) -> BoxFuture<'static, ()> + Send>;

/// Admits a request on a document, given what it's admitted with: on
/// success, the [`Issue`] that sends it; on failure, it answers the caller
/// with the error and returns `None`.
pub(crate) type Admit = Box<dyn FnOnce(Result<Admitted, LspError>) -> Option<Issue> + Send>;

/// What waits in the actor's outbox for room in async-lsp's queue.
pub(crate) enum Outgoing {
    Note(Note),
    /// An admitted request, sent right behind the notifications that
    /// brought its document to the version it names.
    Request(Issue),
}

/// What the manager asks of the actor. Anything that must keep its order
/// with requests (document sync, say) comes through here and
/// [`QUEUED_MAX`]'s window, or it would overtake the requests the actor
/// holds back. Only `shutdown` and `exit` skip it, on purpose.
pub(crate) enum Command {
    Request(Issue),
    /// Start syncing a document: `didOpen`.
    Open(Box<Opening>),
    /// Drain a document's subscription.
    Changed(BufferId),
    /// Drain a document one last time, then `didClose`.
    Close(BufferId),
    /// A request on a document, computed on buffer version `version`.
    Admit {
        buffer: BufferId,
        version: Version,
        admit: Admit,
    },
    /// The LSP and buffer versions in a document's ring.
    #[cfg(test)]
    Ring(BufferId, oneshot::Sender<Vec<(i32, Version)>>),
}

/// The event the actor queues behind each request or notification it hands
/// async-lsp. The router counts it when the loop gets to it, by which time
/// the loop has taken the message.
pub(crate) struct Taken;

/// How many of the actor's requests async-lsp's loop has taken from its
/// queue, shared by the router, counting [`Taken`], and the actor, waiting
/// for room.
#[derive(Default)]
pub(crate) struct Queue {
    taken: Cell<u64>,
    /// The actor, waiting for room.
    waiting: Cell<Option<Waker>>,
}

impl Queue {
    /// Counts a request the loop has taken, waking the actor if it waits.
    pub(crate) fn took(&self) {
        self.taken.set(self.taken.get() + 1);
        if let Some(actor) = self.waiting.take() {
            actor.wake();
        }
    }

    /// Ready once fewer than [`QUEUED_MAX`] of the `sent` requests wait to
    /// be taken.
    fn poll_room(&self, sent: u64, cx: &Context<'_>) -> Poll<()> {
        if sent - self.taken.get() < QUEUED_MAX {
            return Poll::Ready(());
        }
        self.waiting.set(Some(cx.waker().clone()));
        Poll::Pending
    }
}

/// What the session learned from `initialize`, readable from the manager.
#[derive(Default)]
pub(crate) struct Session {
    pub(crate) capabilities: Option<ServerCapabilities>,
    pub(crate) encoding: Option<PositionEncoding>,
}

/// Everything a server thread starts with.
pub(crate) struct Server {
    pub(crate) id: ServerId,
    pub(crate) spec: ServerSpec,
    pub(crate) root: PathBuf,
    pub(crate) launcher: Arc<dyn ServerLauncher>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) sink: Sink,
    pub(crate) session: Arc<Mutex<Session>>,
    /// This run's id, and the table its documents' identities go in.
    pub(crate) session_id: SessionId,
    pub(crate) identities: Identities,
    pub(crate) commands: mpsc::UnboundedReceiver<Command>,
    /// Resolves (either way) when the manager wants the server stopped.
    /// Apart from the commands, so a stream of requests can't delay it.
    pub(crate) stop: oneshot::Receiver<()>,
}

impl Server {
    pub(crate) fn spawn(self) -> io::Result<JoinHandle<()>> {
        thread::Builder::new()
            .name(format!("fe-lsp {}", self.spec.name))
            .spawn(move || self.run())
    }

    fn run(self) {
        let status = StatusSink {
            id: self.id,
            name: self.spec.name.clone(),
            sink: self.sink.clone(),
        };
        if catch_unwind(AssertUnwindSafe(|| self.serve())).is_err() {
            status.send(ServerStatus::Stopped(Some(
                "fe-lsp's server thread panicked".into(),
            )));
        }
    }

    fn serve(mut self) {
        let status = StatusSink {
            id: self.id,
            name: self.spec.name.clone(),
            sink: self.sink.clone(),
        };
        status.send(ServerStatus::Starting);
        let folder = match workspace_folder(&self.root) {
            Ok(folder) => folder,
            Err(err) => {
                self.commands.close();
                return status.send(ServerStatus::Stopped(Some(err)));
            }
        };
        let ServerIo {
            input,
            output,
            stderr,
            process,
        } = match self.launcher.launch(&self.spec, &self.root) {
            Ok(io) => io,
            Err(err) => {
                self.commands.close();
                return status.send(ServerStatus::Stopped(Some(err.to_string())));
            }
        };

        let queue = Rc::new(Queue::default());
        let (main_loop, socket) = MainLoop::new_client(|socket| {
            client_router(ClientState {
                name: self.spec.name.clone(),
                folder: folder.clone(),
                settings: self.spec.settings.clone(),
                queue: queue.clone(),
                _socket: socket,
            })
        });
        // The loop reports how it ended to the actor; the actor's end (its
        // `session_over` sender dropping) ends the loop and the stderr drain.
        let (loop_ended, loop_done) = oneshot::channel();
        let (session_over, over) = oneshot::channel::<()>();
        let over = over.shared();
        let main_loop = {
            let over = over.clone();
            async move {
                // async-lsp's loop has no budget of its own: with the
                // server's output always ready, or a long queue to send,
                // one poll of it could last as long as the flood or the
                // queue, starving the actor and its deadlines. Its reads
                // and writes share an `IoBudget`, refilled each time the
                // loop is polled.
                let budget = IoBudget::default();
                let run = main_loop.run_buffered(budget.meter(output), budget.meter(input));
                pin_mut!(run);
                let run = futures::future::poll_fn(|cx| {
                    budget.refill();
                    run.as_mut().poll(cx)
                })
                .fuse();
                pin_mut!(run);
                let mut over = over.fuse();
                select_biased! {
                    _ = over => {}
                    ended = run => { let _ = loop_ended.send(ended); }
                }
            }
        };
        let stderr = drain_stderr(self.spec.name.clone(), stderr, over);
        let actor = Actor {
            _end: EndSession(self.identities.clone(), self.session_id),
            status,
            spec: self.spec,
            folder,
            clock: self.clock,
            session: self.session,
            socket,
            commands: self.commands,
            stop: self.stop,
            next_id: 0,
            queue,
            sent: 0,
            pending: FuturesUnordered::new(),
            backlog: VecDeque::new(),
            documents: HashMap::new(),
            link: Link {
                policy: SyncPolicy::from_capabilities(&ServerCapabilities::default()),
                encoding: PositionEncoding::Utf16,
                identities: self.identities,
                server: self.id,
                session: self.session_id,
                versions: HashMap::new(),
                outbox: VecDeque::new(),
            },
            tick: None,
            drain_due: false,
            stopping: false,
        };
        async_io::block_on(futures::future::join3(
            main_loop,
            actor.run(process, loop_done, session_over),
            stderr,
        ));
    }
}

#[derive(Clone)]
struct StatusSink {
    id: ServerId,
    name: String,
    sink: Sink,
}

impl StatusSink {
    fn send(&self, status: ServerStatus) {
        let mut sink = self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        sink(LspEvent::Status {
            server: self.id,
            name: self.name.clone(),
            status,
        });
    }
}

type LoopDone = oneshot::Receiver<async_lsp::Result<()>>;

struct Actor {
    /// Retires the session's identities if the actor goes some way that
    /// doesn't (a panic).
    _end: EndSession,
    status: StatusSink,
    spec: ServerSpec,
    folder: WorkspaceFolder,
    clock: Arc<dyn Clock>,
    session: Arc<Mutex<Session>>,
    socket: ServerSocket,
    commands: mpsc::UnboundedReceiver<Command>,
    stop: oneshot::Receiver<()>,
    /// The id async-lsp gives the next request.
    next_id: i32,
    queue: Rc<Queue>,
    /// Requests and notifications handed to async-lsp since `initialize`,
    /// each followed by [`Taken`].
    sent: u64,
    /// Requests in flight; each answers its caller.
    pending: FuturesUnordered<BoxFuture<'static, ()>>,
    /// Commands made while starting, carried out once initialized, ahead
    /// of new ones.
    backlog: VecDeque<Command>,
    documents: HashMap<BufferId, Document>,
    /// How the server asked for documents to be synced, and the outbox.
    link: Link,
    /// The next [`BACKSTOP`] tick, while documents are open.
    tick: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
    /// A tick came: drain every document once there's room to send what
    /// that makes. Ticks while it waits add nothing, so a server that
    /// doesn't read can't make the outbox grow.
    drain_due: bool,
    /// The manager asked for a stop.
    stopping: bool,
}

enum Started {
    Running,
    /// Asked to stop, and the server never got as far as answering
    /// `initialize`.
    Stopped,
    Failed(String),
}

enum Step {
    Next(Next),
    Stop,
    Tick,
    Answered,
    LoopEnded(Result<async_lsp::Result<()>, oneshot::Canceled>),
}

impl Actor {
    async fn run(
        mut self,
        mut process: Box<dyn ServerProcess>,
        mut loop_done: LoopDone,
        _session_over: oneshot::Sender<()>,
    ) {
        match self.initialize(&mut loop_done).await {
            Started::Running => {}
            Started::Stopped => {
                process.kill();
                return self.stopped(None);
            }
            Started::Failed(why) => {
                process.kill();
                return self.stopped(Some(why));
            }
        }
        let mut budget = Budget::default();
        while !self.stopping {
            let step = select_biased! {
                _ = &mut self.stop => Step::Stop,
                ended = loop_done => Step::LoopEnded(ended),
                () = self.pending.select_next_some() => Step::Answered,
                () = wait_tick(&mut self.tick).fuse() => Step::Tick,
                next = next_command(
                    &self.queue,
                    self.sent,
                    &mut self.link.outbox,
                    self.drain_due,
                    &mut self.backlog,
                    &mut self.commands,
                ).fuse() => Step::Next(next),
            };
            match step {
                Step::Next(Next::Out(Outgoing::Note(note))) => self.notify(note),
                Step::Next(Next::Out(Outgoing::Request(issue))) => self.issue(issue),
                Step::Next(Next::Command(Some(command))) => self.command(command),
                Step::Next(Next::Command(None)) | Step::Stop => break,
                Step::Next(Next::DrainAll) => {
                    self.drain_due = false;
                    let ids: Vec<BufferId> = self.documents.keys().copied().collect();
                    for id in ids {
                        self.drain(id);
                    }
                }
                Step::Tick => {
                    self.tick = None;
                    self.drain_due = true;
                    self.arm_tick();
                }
                Step::Answered => {}
                Step::LoopEnded(ended) => {
                    process.kill();
                    let why = format!("{} stopped: {}", self.spec.name, loop_end(ended));
                    return self.stopped(Some(why));
                }
            }
            budget.spend().await;
        }
        self.shutdown(&mut *process).await;
    }

    /// The id for a request about to be sent.
    fn take_id(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn issue(&mut self, issue: Issue) {
        let id = self.take_id();
        self.pending.push(issue(&self.socket, id));
        // If the loop has gone, `loop_done` says so.
        let _ = self.socket.emit(Taken);
        self.sent += 1;
    }

    fn notify(&mut self, note: Note) {
        // If the loop has gone, `loop_done` says so.
        let _ = note.send(&self.socket);
        let _ = self.socket.emit(Taken);
        self.sent += 1;
    }

    /// Carries out a command once initialized. A document command only
    /// queues notifications (and an admitted request) in the outbox, which
    /// goes out ahead of the next command.
    fn command(&mut self, command: Command) {
        match command {
            Command::Request(issue) => self.issue(issue),
            Command::Open(opening) => {
                let buffer = opening.buffer;
                // The manager closes a buffer before opening it again, so
                // its document has gone by now; replace it if it hasn't.
                if let Some(old) = self.documents.remove(&buffer) {
                    old.close(&mut self.link);
                }
                // A stale open is dropped, and its subscription with it.
                if let Some(document) = Document::open(*opening, &mut self.link) {
                    self.documents.insert(buffer, document);
                }
                self.arm_tick();
            }
            Command::Changed(buffer) => self.drain(buffer),
            Command::Close(buffer) => {
                if let Some(document) = self.documents.remove(&buffer) {
                    document.close(&mut self.link);
                }
                if self.documents.is_empty() {
                    self.tick = None;
                    self.drain_due = false;
                }
            }
            Command::Admit {
                buffer,
                version,
                admit,
            } => {
                if let Some(issue) = admit(self.admit(buffer, version)) {
                    self.link.outbox.push_back(Outgoing::Request(issue));
                }
            }
            #[cfg(test)]
            Command::Ring(buffer, answer) => {
                let ring = self.documents.get(&buffer).map(Document::ring);
                let _ = answer.send(ring.unwrap_or_default());
            }
        }
    }

    fn drain(&mut self, buffer: BufferId) {
        if let Some(document) = self.documents.get_mut(&buffer) {
            document.drain(&mut self.link);
        }
    }

    /// Drains `buffer`'s document, then admits a request computed on
    /// `version` if the server now has that version.
    fn admit(&mut self, buffer: BufferId, version: Version) -> Result<Admitted, LspError> {
        self.drain(buffer);
        let document = self
            .documents
            .get(&buffer)
            .ok_or_else(|| LspError::Unavailable(NOT_OPEN.into()))?;
        let admitted = document.admit(version)?;
        let others = self
            .documents
            .iter()
            .filter(|(other, _)| **other != buffer)
            .filter_map(|(_, other)| other.current())
            .collect();
        Ok(Admitted {
            encoding: self.link.encoding,
            document: admitted,
            others,
        })
    }

    /// Starts the [`BACKSTOP`] timer if documents are open and it isn't
    /// running.
    fn arm_tick(&mut self) {
        if self.tick.is_none() && !self.documents.is_empty() {
            self.tick = Some(self.clock.sleep(BACKSTOP));
        }
    }

    async fn initialize(&mut self, loop_done: &mut LoopDone) -> Started {
        let params = initialize_params(&self.spec, &self.folder);
        self.take_id();
        let reply = send_request::<Initialize>(&self.socket, params).fuse();
        let timeout = self.clock.sleep(INITIALIZE_TIMEOUT).fuse();
        pin_mut!(reply, timeout);
        // A stop while starting waits for the answer: the protocol allows
        // nothing to be sent before it, and `shutdown` after it.
        let mut budget = Budget::default();
        let failed = |stopping: bool, why: String| {
            if stopping {
                Started::Stopped
            } else {
                Started::Failed(why)
            }
        };
        let result = loop {
            select_biased! {
                result = reply => break result,
                _ = &mut self.stop => self.stopping = true,
                ended = &mut *loop_done => {
                    return failed(self.stopping, format!(
                        "{} stopped before answering initialize: {}",
                        self.spec.name,
                        loop_end(ended),
                    ));
                }
                () = timeout => {
                    return failed(self.stopping, format!(
                        "{} didn't answer initialize within {} s",
                        self.spec.name,
                        INITIALIZE_TIMEOUT.as_secs(),
                    ));
                }
                command = self.commands.next() => match command {
                    Some(command) => self.backlog.push_back(command),
                    None => self.stopping = true,
                },
            }
            budget.spend().await;
        };
        let result = match result {
            Ok(result) => result,
            // The loop has gone, taking the reply's channel with it: report
            // why it went.
            Err(async_lsp::Error::ServiceStopped) => {
                return failed(
                    self.stopping,
                    format!(
                        "{} stopped before answering initialize: {}",
                        self.spec.name,
                        loop_end(loop_done.await),
                    ),
                );
            }
            Err(err) => {
                let why = format!("{}: initialize failed: {err}", self.spec.name);
                return failed(self.stopping, why);
            }
        };
        self.record(result);
        if self
            .socket
            .notify::<Initialized>(InitializedParams {})
            .is_err()
        {
            return failed(
                self.stopping,
                format!(
                    "{} stopped after answering initialize: {}",
                    self.spec.name,
                    loop_end(loop_done.await),
                ),
            );
        }
        Started::Running
    }

    fn record(&mut self, result: InitializeResult) {
        let encoding = match result.capabilities.position_encoding.as_ref() {
            None => PositionEncoding::Utf16,
            Some(kind) if *kind == PositionEncodingKind::UTF8 => PositionEncoding::Utf8,
            Some(kind) if *kind == PositionEncodingKind::UTF16 => PositionEncoding::Utf16,
            // Including utf-32, which isn't offered.
            Some(kind) => {
                log::warn!(
                    "{}: position encoding {:?} wasn't offered; assuming utf-16",
                    self.spec.name,
                    kind.as_str()
                );
                PositionEncoding::Utf16
            }
        };
        self.link.policy = SyncPolicy::from_capabilities(&result.capabilities);
        self.link.encoding = encoding;
        let mut session = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        session.capabilities = Some(result.capabilities);
        session.encoding = Some(encoding);
    }

    /// `shutdown` (up to 2 s), `exit`, up to 1 s for the process to go,
    /// then `kill`. Whatever the server does after `exit` (a non-zero code,
    /// closing its output) is a normal stop.
    async fn shutdown(&mut self, process: &mut dyn ServerProcess) {
        self.link.identities.end_session(self.link.session);
        // Callers of requests in flight or kept back get `ServerDown` now,
        // and those of requests still unread once the session is over.
        self.pending = FuturesUnordered::new();
        self.backlog.clear();
        self.link.outbox.clear();
        self.documents.clear();
        self.tick = None;
        self.drain_due = false;
        self.take_id();
        let reply = send_request::<Shutdown>(&self.socket, ()).fuse();
        let timeout = self.clock.sleep(SHUTDOWN_TIMEOUT).fuse();
        pin_mut!(reply, timeout);
        select_biased! {
            result = reply => if let Err(err) = result {
                log::info!("{}: shutdown: {err}", self.spec.name);
            },
            () = timeout => log::info!(
                "{}: no answer to shutdown within {} s",
                self.spec.name,
                SHUTDOWN_TIMEOUT.as_secs(),
            ),
        }
        if let Err(err) = self.socket.notify::<Exit>(()) {
            log::info!("{}: exit: {err}", self.spec.name);
        }
        let exited = {
            let exited = process.exited().fuse();
            let timeout = self.clock.sleep(EXIT_TIMEOUT).fuse();
            pin_mut!(exited, timeout);
            select_biased! {
                exit = exited => {
                    log::info!("{} exited: {exit:?}", self.spec.name);
                    true
                }
                () = timeout => false,
            }
        };
        if !exited {
            log::info!(
                "{} still running {} s after exit; killing it",
                self.spec.name,
                EXIT_TIMEOUT.as_secs(),
            );
            process.kill();
        }
        self.stopped(None);
    }

    /// Reports the session over. The manager sees the command channel
    /// closed first, so a `start` prompted by this status starts afresh.
    fn stopped(&mut self, why: Option<String>) {
        self.link.identities.end_session(self.link.session);
        self.commands.close();
        self.status.send(ServerStatus::Stopped(why));
    }
}

/// Sends `R` now and returns its reply. async-lsp's request future only
/// sends when first polled, which would let ids follow poll order rather
/// than issue order, so poll it once here.
pub(crate) fn send_request<R: Request>(
    socket: &ServerSocket,
    params: R::Params,
) -> impl Future<Output = async_lsp::Result<R::Result>> + Send + 'static {
    let socket = socket.clone();
    let mut reply = Box::pin(async move { socket.request::<R>(params).await });
    match reply
        .as_mut()
        .poll(&mut Context::from_waker(futures::task::noop_waker_ref()))
    {
        Poll::Ready(result) => Either::Left(futures::future::ready(result)),
        Poll::Pending => Either::Right(reply),
    }
}

fn loop_end(ended: Result<async_lsp::Result<()>, oneshot::Canceled>) -> String {
    match ended {
        Ok(Ok(())) => "the connection closed".into(),
        Ok(Err(async_lsp::Error::Eof)) => "the server closed its output (EOF)".into(),
        Ok(Err(err)) => err.to_string(),
        Err(oneshot::Canceled) => "the connection stopped".into(),
    }
}

/// Logs the server's stderr until it closes or the session ends. Card 4
/// keeps it for `lsp.show_log`.
async fn drain_stderr(
    name: String,
    stderr: Option<Pin<Box<dyn AsyncRead + Send>>>,
    over: futures::future::Shared<oneshot::Receiver<()>>,
) {
    let Some(mut stderr) = stderr else { return };
    let drain = async {
        let mut buf = vec![0; 4096];
        let mut budget = Budget::default();
        while let Ok(n @ 1..) = stderr.read(&mut buf).await {
            log::debug!(
                "{name} stderr: {}",
                String::from_utf8_lossy(&buf[..n]).trim_end()
            );
            budget.spend().await;
        }
    }
    .fuse();
    pin_mut!(drain);
    let mut over = over.fuse();
    select_biased! {
        _ = over => {}
        () = drain => {}
    }
}

enum Next {
    Out(Outgoing),
    /// Drain every document: a backstop tick came.
    DrainAll,
    /// `None` once the manager has gone.
    Command(Option<Command>),
}

/// What to do next, once async-lsp's queue has room: send what's first in
/// the outbox, else a due backstop drain, else carry out the
/// startup backlog, then commands. So the outbox only fills when it's
/// empty and there's room, from one drain or one command at a time.
async fn next_command(
    queue: &Queue,
    sent: u64,
    outbox: &mut VecDeque<Outgoing>,
    drain_due: bool,
    backlog: &mut VecDeque<Command>,
    commands: &mut mpsc::UnboundedReceiver<Command>,
) -> Next {
    futures::future::poll_fn(|cx| queue.poll_room(sent, cx)).await;
    if let Some(out) = outbox.pop_front() {
        return Next::Out(out);
    }
    if drain_due {
        return Next::DrainAll;
    }
    match backlog.pop_front() {
        Some(command) => Next::Command(Some(command)),
        None => Next::Command(commands.next().await),
    }
}

/// Resolves when the backstop tick is due; never, while it isn't running.
async fn wait_tick(tick: &mut Option<Pin<Box<dyn Future<Output = ()> + Send>>>) {
    match tick {
        Some(tick) => tick.await,
        None => futures::future::pending().await,
    }
}

/// How many steps a task on the server thread takes before it lets the
/// others joined with it run (for async-lsp's loop, reads and writes: see
/// [`IoBudget`]). Each task's input can be always ready (a server flooding
/// its output or stderr, a host flooding requests), and `block_on(join(..))`
/// only switches tasks when one returns `Pending`.
pub(crate) const STEPS_PER_YIELD: u32 = 32;

#[derive(Default)]
struct Budget(u32);

impl Budget {
    /// Counts a step, yielding once the budget is spent.
    async fn spend(&mut self) {
        self.0 += 1;
        if self.0 >= STEPS_PER_YIELD {
            self.0 = 0;
            YieldNow(false).await;
        }
    }
}

/// Returns `Pending` once, having asked to be polled again.
struct YieldNow(bool);

impl Future for YieldNow {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// The reads and writes async-lsp's main loop has left in the current poll
/// of it: [`STEPS_PER_YIELD`], refilled each time the loop is polled.
///
/// Once it's spent, every read or write returns `Pending` (having woken
/// the task) until the loop is polled again. Whatever path the loop takes
/// to retry one within the same poll (its `feed` retries a write it was
/// flushing, for one), that poll ends, and the actor gets its turn. With
/// nothing more to read or write, the loop does little else: what is left
/// of its read buffer, and at most a couple of messages taken from its
/// queue, with the [`Taken`] events between them.
#[derive(Clone, Default)]
struct IoBudget(Rc<Cell<u32>>);

impl IoBudget {
    fn refill(&self) {
        self.0.set(STEPS_PER_YIELD);
    }

    /// Takes one read or write, or wakes the task to say there's none left.
    fn take(&self, cx: &Context<'_>) -> bool {
        let left = self.0.get();
        if left == 0 {
            cx.waker().wake_by_ref();
            return false;
        }
        self.0.set(left - 1);
        true
    }

    fn meter<T>(&self, inner: T) -> Metered<T> {
        Metered {
            inner,
            budget: self.clone(),
        }
    }
}

/// A reader or writer whose every read or write spends its [`IoBudget`].
struct Metered<T> {
    inner: T,
    budget: IoBudget,
}

impl<R: AsyncRead + Unpin> AsyncRead for Metered<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if !self.budget.take(cx) {
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for Metered<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if !self.budget.take(cx) {
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}

fn workspace_folder(root: &Path) -> Result<WorkspaceFolder, String> {
    let uri = Url::from_file_path(root)
        .map_err(|()| format!("{} isn't an absolute path", root.display()))?;
    let name = root.file_name().map_or_else(
        || root.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    Ok(WorkspaceFolder { uri, name })
}

/// `initialize`'s parameters (§3.4): the single root, the spec's
/// `initializationOptions`, and what M4 uses (§7) with every
/// `dynamicRegistration` off (§3.6).
#[allow(deprecated)] // `root_uri`: still what servers read first.
pub(crate) fn initialize_params(spec: &ServerSpec, folder: &WorkspaceFolder) -> InitializeParams {
    InitializeParams {
        process_id: Some(std::process::id()),
        root_uri: Some(folder.uri.clone()),
        workspace_folders: Some(vec![folder.clone()]),
        client_info: Some(ClientInfo {
            name: "fastedit".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
        initialization_options: spec.initialization_options.clone(),
        capabilities: client_capabilities(),
        ..InitializeParams::default()
    }
}

fn client_capabilities() -> ClientCapabilities {
    let dynamic = Some(false);
    let symbol_kinds = serde_json::from_value(serde_json::json!({
        "valueSet": (1..=26).collect::<Vec<_>>(),
    }))
    .ok();
    ClientCapabilities {
        workspace: Some(WorkspaceClientCapabilities {
            did_change_configuration: Some(DynamicRegistrationClientCapabilities {
                dynamic_registration: dynamic,
            }),
            symbol: Some(WorkspaceSymbolClientCapabilities {
                dynamic_registration: dynamic,
                symbol_kind: symbol_kinds,
                ..Default::default()
            }),
            workspace_folders: Some(true),
            configuration: Some(true),
            ..Default::default()
        }),
        text_document: Some(TextDocumentClientCapabilities {
            synchronization: Some(TextDocumentSyncClientCapabilities {
                dynamic_registration: dynamic,
                will_save: Some(false),
                will_save_wait_until: Some(false),
                did_save: Some(true),
            }),
            completion: Some(CompletionClientCapabilities {
                dynamic_registration: dynamic,
                completion_item: Some(CompletionItemCapability {
                    snippet_support: Some(false),
                    insert_replace_support: Some(false),
                    resolve_support: Some(CompletionItemCapabilityResolveSupport {
                        properties: vec!["documentation".into(), "detail".into()],
                    }),
                    label_details_support: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            hover: Some(HoverClientCapabilities {
                dynamic_registration: dynamic,
                content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
            }),
            definition: Some(GotoCapability {
                dynamic_registration: dynamic,
                link_support: Some(true),
            }),
            references: Some(DynamicRegistrationClientCapabilities {
                dynamic_registration: dynamic,
            }),
            publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                related_information: Some(true),
                tag_support: Some(TagSupport {
                    value_set: vec![DiagnosticTag::UNNECESSARY, DiagnosticTag::DEPRECATED],
                }),
                version_support: Some(true),
                code_description_support: Some(true),
                data_support: None,
            }),
            ..Default::default()
        }),
        window: Some(WindowClientCapabilities {
            work_done_progress: Some(true),
            ..Default::default()
        }),
        general: Some(GeneralClientCapabilities {
            position_encodings: Some(vec![
                PositionEncodingKind::UTF8,
                PositionEncodingKind::UTF16,
            ]),
            ..Default::default()
        }),
        experimental: Some(serde_json::json!({ "serverStatusNotification": true })),
    }
}
