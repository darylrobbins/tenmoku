//! One thread per server, named `fe-lsp <name>`, running
//! `async_io::block_on(join4(main_loop, actor, stderr, pump))`: §2.2's
//! `join(main_loop, actor)`, plus a drain for the server's stderr and a
//! pump that reads its stdout whatever the main loop is doing.
//!
//! - The **main loop** is async-lsp's `MainLoop` over the launcher's byte
//!   streams, answering the server through [`client_router`]. It reads the
//!   server's output from a [`Backlog`], which the **pump** fills: while
//!   the loop waits for a write, it reads nothing, and a server that stops
//!   reading until its output drains would never take that write.
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
//!   retired before anything else is reported; then each open document's
//!   diagnostics are reported cleared.
//! - It decodes the diagnostics the server publishes (diagnostics.rs)
//!   against its documents' rings, as the router reads them.
//! - A session ending in a crash (restart.rs) keeps its documents and the
//!   manager's commands for the next session on the same thread, after a
//!   backoff. Otherwise, however the session ends, the documents the
//!   manager routed to it ([`Routes`]), open or still queued, go on to
//!   wherever the manager now places them: another server (a restart's new
//!   instance), or its lot to wait. The rest are dropped.
//! - A stop cancels a launch in progress (the instance's
//!   [`LaunchCancel`], passed to every launch it makes), and kills a server
//!   that hasn't answered `initialize`. The manager's kill switch also
//!   cuts `shutdown`, `exit` and a restart's backoff short: the process is
//!   killed at once, and nothing more is launched.
//! - The actor watches the process too: an exit while not shutting down
//!   ends the session as a crash, even if the server's output stays open
//!   (a descendant holding it, say).
//! - Readiness (readiness.rs) is tracked from what the router hears, with
//!   its timers on the actor's clock, and every change of what it shows is
//!   a status event.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use async_lsp::{MainLoop, ServerSocket};
use fe_buffer::{BufferId, Version};
use futures::channel::{mpsc, oneshot};
use futures::future::{BoxFuture, Either};
use futures::stream::FuturesUnordered;
use futures::{
    AsyncRead, AsyncReadExt, AsyncWrite, Future, FutureExt, StreamExt, pin_mut, select_biased,
};
use lsp_types::notification::{Cancel, DidChangeConfiguration, Exit, Initialized};
use lsp_types::request::{Initialize, Request, Shutdown};
use lsp_types::{
    CancelParams, ClientCapabilities, ClientInfo, CodeActionCapabilityResolveSupport,
    CodeActionClientCapabilities, CodeActionKind, CodeActionKindLiteralSupport,
    CodeActionLiteralSupport, CompletionClientCapabilities, CompletionItemCapability,
    CompletionItemCapabilityResolveSupport, DiagnosticClientCapabilities, DiagnosticTag,
    DidChangeConfigurationParams, DynamicRegistrationClientCapabilities, FailureHandlingKind,
    GeneralClientCapabilities, GotoCapability, HoverClientCapabilities, InitializeParams,
    InitializeResult, InitializedParams, InlayHintClientCapabilities,
    InlayHintResolveClientCapabilities, InlayHintWorkspaceClientCapabilities, MarkupKind,
    NumberOrString, ParameterInformationSettings, PositionEncodingKind,
    PublishDiagnosticsClientCapabilities, PublishDiagnosticsParams, RenameClientCapabilities,
    SemanticTokensClientCapabilities, SemanticTokensClientCapabilitiesRequests,
    SemanticTokensFullOptions, SemanticTokensWorkspaceClientCapabilities, ServerCapabilities,
    SignatureHelpClientCapabilities, SignatureInformationSettings, TagSupport,
    TextDocumentClientCapabilities, TextDocumentSyncClientCapabilities, TokenFormat, Url,
    WindowClientCapabilities, WorkspaceClientCapabilities, WorkspaceEditClientCapabilities,
    WorkspaceFolder, WorkspaceSymbolClientCapabilities,
};

use crate::clock::Clock;
use crate::diagnostics::{FileDiagnostic, Inbox};
use crate::document::{Admitted, Document, Link, Note, Opened, Opening, SentDocument, SyncPolicy};
use crate::encoding::PositionEncoding;
use crate::event::{LspError, LspEvent, NOT_OPEN, ServerId, ServerStatus, SharedLog};
use crate::identity::{EndSession, Identities, SessionId};
use crate::launcher::{
    ExitInfo, Flag, LaunchCancel, ServerIo, ServerLauncher, ServerProcess, ServerSpec,
    UnversionedDiagnostics,
};
use crate::pull::{Pulled, Puller};
use crate::readiness::Readiness;
use crate::registration::Registrations;
use crate::requests::{Issue, Orphans, Out, Settled};
use crate::restart::{Carry, Kill, SharedCrashes, Verdict, Waited};
use crate::router::{ClientState, Signal, client_router};
use crate::semantic_tokens;
use crate::workspace_edit::{Asked, Edits, HandedReply, decode};
use lsp_types::WorkspaceFileOperationsClientCapabilities;

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
/// The most commands a backstop drain lets go ahead of it.
pub(crate) const DRAIN_BEHIND_MAX: usize = 1024;

/// A sleep on the actor's clock.
type Timer = Pin<Box<dyn Future<Output = ()> + Send>>;

/// The manager's event sink, shared by every server thread.
pub(crate) type Sink = Arc<Mutex<dyn FnMut(LspEvent) + Send>>;

/// Admits requests on a document, given what they're admitted with: on
/// success, the [`Issue`]s that send them, in order; on failure, it answers
/// the callers with the error and returns none.
pub(crate) type Admit = Box<dyn FnOnce(Result<Admitted, LspError>) -> Vec<Issue> + Send>;

/// As [`Admit`], for a request on no document: given the documents the
/// server has open as it's sent.
pub(crate) type WithDocuments = Box<dyn FnOnce(Result<Opened, LspError>) -> Option<Issue> + Send>;

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
    /// A request on no document in particular, sent with the documents
    /// the server has open then, brought up to date first.
    WithDocuments(WithDocuments),
    /// Called with the documents the server has open now, at the versions
    /// handed to async-lsp ([`Handed`]), without bringing them up to date
    /// (workspace_edit.rs).
    Documents(Box<dyn FnOnce(Opened) + Send>),
    /// New `settings` for the spec: they answer `workspace/configuration`
    /// from now on, and `workspace/didChangeConfiguration` carries them.
    Configure(serde_json::Value),
    /// A notification on no document in particular, sent in order with the
    /// requests and document sync queued before it (notify.rs).
    // Nothing calls them until the watched-files and file-operation cards.
    #[allow(dead_code)]
    Notify {
        method: &'static str,
        params: serde_json::Value,
    },
    /// Drain every open document: a backstop tick queued it, behind the
    /// commands already waiting, so drains can't starve them.
    Drain,
    /// The LSP and buffer versions in a document's ring.
    #[cfg(test)]
    Ring(BufferId, oneshot::Sender<Vec<(i32, Version)>>),
}

/// Where routed documents are, shared by the manager and every server
/// thread behind one mutex. The manager updates a document's place before
/// it stops the server holding it, and an ending server closes its command
/// channel before it takes what's queued there. So a document forwarded to
/// a server that has since stopped either lands in its queue in time to be
/// forwarded again, or fails to send and goes where its place now says.
pub(crate) type Routes = Arc<Mutex<Places>>;

#[derive(Default)]
pub(crate) struct Places {
    /// Where each document [`LspManager::open`](crate::LspManager::open)
    /// routed is, or is going. A document not here isn't forwarded.
    pub(crate) places: HashMap<BufferId, Place>,
    /// Documents waiting for a server, as openings at the text last
    /// drained.
    pub(crate) lot: HashMap<BufferId, Opening>,
}

#[derive(Clone)]
pub(crate) enum Place {
    /// Synced to this server, or on its way there.
    Server(ServerId, mpsc::UnboundedSender<Command>),
    /// Waiting in the lot, or on its way there.
    Waiting,
}

impl Places {
    /// Sends each of `openings` still current where its place says: to its
    /// server, or, if that has gone (its place then says it waits) or it has
    /// none, to the lot. One with no place is dropped. Each is first brought
    /// up to its buffer's latest edit, so a server's `didOpen` carries the
    /// text as it is now, even for one handed on late (by a retired server,
    /// say) after edits nothing drained.
    pub(crate) fn forward(&mut self, identities: &Identities, openings: Vec<Opening>) {
        for mut opening in openings {
            if !identities.is_current(opening.buffer, opening.generation) {
                continue;
            }
            opening.catch_up();
            let opening = match self.places.get(&opening.buffer) {
                None => continue,
                Some(Place::Waiting) => opening,
                Some(Place::Server(_, commands)) => {
                    match commands.unbounded_send(Command::Open(Box::new(opening))) {
                        Ok(()) => continue,
                        Err(err) => match err.into_inner() {
                            Command::Open(opening) => *opening,
                            _ => continue,
                        },
                    }
                }
            };
            self.places.insert(opening.buffer, Place::Waiting);
            self.lot.insert(opening.buffer, opening);
        }
    }
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

/// What the current session learned, readable from the manager. Each
/// session starts it afresh.
#[derive(Default)]
pub(crate) struct Session {
    pub(crate) capabilities: Option<ServerCapabilities>,
    pub(crate) encoding: Option<PositionEncoding>,
    pub(crate) registrations: Registrations,
    /// How many orphans the actor holds, and the most it has held.
    #[cfg(test)]
    pub(crate) orphans: (usize, usize),
    /// Once a test sets it: each request and document notification the
    /// actor hands async-lsp, in order.
    #[cfg(test)]
    pub(crate) handed: Option<Vec<String>>,
}

/// Resolves when a server thread has ended.
pub(crate) type Done = futures::future::Shared<oneshot::Receiver<()>>;

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
    /// Passed to every launch; the manager fires it with the stop.
    pub(crate) cancel: LaunchCancel,
    /// The manager's kill switch, which `cancel` is linked to.
    pub(crate) switch: Arc<Flag>,
    pub(crate) routes: Routes,
    /// Resolves when the thread of the instance this one replaces (same
    /// name and root) has ended, its process gone: nothing is launched
    /// before.
    pub(crate) after: Option<Done>,
    /// Dropped when this thread ends, for the instance replacing it.
    pub(crate) done: Option<oneshot::Sender<()>>,
    /// The crashes of servers with this name and root, and the epoch of
    /// them this instance counts in.
    pub(crate) crashes: SharedCrashes,
    pub(crate) epoch: u64,
    /// Where its stderr and `window/logMessage` go.
    pub(crate) log: SharedLog,
    /// The latest status of the servers with this name and root.
    pub(crate) latest: LatestStatus,
}

/// The latest status the servers started at one (name, root) reported, and
/// which reported it: the newest instance's, since an instance a restart
/// replaced may report its end after its successor has started.
pub(crate) type LatestStatus = Arc<Mutex<Option<(ServerId, ServerStatus)>>>;

/// Records `status` as `server`'s in `latest`, unless a newer instance
/// there has reported already.
pub(crate) fn latest(latest: &LatestStatus, server: ServerId, status: &ServerStatus) {
    let mut latest = lock(latest);
    if latest.as_ref().is_none_or(|(newest, _)| *newest <= server) {
        *latest = Some((server, status.clone()));
    }
}

/// Whether `open` is `published`: the same URI, or both `file` URIs for the
/// same decoded path (`published_path`, decoded once), so a server that
/// escapes `[ ] ( )` still matches the URI fe-lsp built.
fn same_file(open: &Url, published: &Url, published_path: &Option<PathBuf>) -> bool {
    open == published
        || (published_path.is_some()
            && open.scheme() == "file"
            && open.to_file_path().ok() == *published_path)
}

impl Server {
    pub(crate) fn spawn(self) -> io::Result<JoinHandle<()>> {
        thread::Builder::new()
            .name(format!("fe-lsp {}", self.spec.name))
            .spawn(move || self.run())
    }

    fn run(mut self) {
        let _done = self.done.take();
        let status = StatusSink {
            id: self.id,
            name: self.spec.name.clone(),
            sink: self.sink.clone(),
            latest: self.latest.clone(),
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
            latest: self.latest.clone(),
        };
        // Hands the queued documents on however the thread ends, a panic
        // included, unless a session's actor has them.
        let (_, spent) = oneshot::channel();
        let mut carry = Carry {
            commands: std::mem::replace(&mut self.commands, mpsc::unbounded().1),
            stop: std::mem::replace(&mut self.stop, spent),
            kill: Kill::new(self.switch.clone()),
            openings: Vec::new(),
            settings: self.spec.settings.clone(),
            routes: self.routes.clone(),
            identities: self.identities.clone(),
        };
        status.send(ServerStatus::Starting);
        if let Some(mut after) = self.after.take() {
            // A stop or a kill first: one that came while waiting wins over
            // the end.
            let stopped = async_io::block_on(async {
                select_biased! {
                    () = &mut carry.kill => true,
                    _ = &mut carry.stop => true,
                    _ = &mut after => false,
                }
            });
            if stopped {
                // Stopped before it launched. Whatever replaces it must still
                // wait for the one before, so this thread ends after that.
                carry.hand_over();
                status.send(ServerStatus::Stopped(None));
                let _ = async_io::block_on(after);
                return;
            }
        }
        let folder = match workspace_folder(&self.root) {
            Ok(folder) => folder,
            Err(err) => {
                carry.hand_over();
                return status.send(ServerStatus::Stopped(Some(err)));
            }
        };
        let mut session_id = self.session_id;
        // The last session's process, kept until this thread ends or launches
        // again, so a kill thrown meanwhile (during a backoff, or from a
        // status callback) still reaches what's left of it ([`Watched`]).
        let mut last: Option<Watched>;
        loop {
            // A stop or kill that came after the wait, or with none to wait
            // for, wins too: nothing is launched for a spec already replaced,
            // nor once the manager's servers are being killed.
            if carry.stopping() {
                carry.hand_over();
                return status.send(ServerStatus::Stopped(None));
            }
            // The last session's process goes before its replacement is
            // launched: it has exited, and once its group has gone too, its
            // id may be reused, so a later kill mustn't signal it.
            last = None;
            let launched = self.launcher.launch(&self.spec, &self.root, &self.cancel);
            // A stop or kill that came while it launched (which may block: a
            // login shell, a container), which `cancel` asked it to cut
            // short. Nothing has been sent to what it started, so that's
            // killed rather than initialized, and its successor needn't wait
            // for an `initialize` answer first. A launch that gave up is a
            // stop, not a failure.
            if self.cancel.is_cancelled() || carry.stopping() {
                if let Ok(io) = launched {
                    let mut process = io.process;
                    async_io::block_on(kill(&mut *process));
                }
                carry.hand_over();
                return status.send(ServerStatus::Stopped(None));
            }
            let io = match launched {
                Ok(io) => io,
                // Not a crash (§3.5), even on a restart: nothing ran, and
                // trying again is for the user (`lsp.restart`).
                Err(err) => {
                    carry.hand_over();
                    return status
                        .send(ServerStatus::Stopped(Some(launch_error(&self.spec, &err))));
                }
            };
            let (ended, process) = self.session(io, session_id, &folder, &status, carry);
            let process = last.insert(process);
            let (why, at, kept) = match ended {
                Ended::Stopped => return process.reap(),
                Ended::Crashed { why, at, carry } => (why, at, carry),
            };
            carry = kept;
            // A stop or kill that came while it crashed (during a recycle's
            // shutdown, say) is a stop, not a crash.
            if carry.stopping() {
                process.reap();
                carry.hand_over();
                return status.send(ServerStatus::Stopped(None));
            }
            // What it gives up with is the spec as it ran: its settings as
            // last configured.
            let ran = ServerSpec {
                settings: carry.settings.clone(),
                ..self.spec.clone()
            };
            let verdict = lock(&self.crashes).crash(at, self.epoch, &ran);
            let (n, after) = match verdict {
                Verdict::Restart { n, after } => (n, after),
                Verdict::GiveUp => {
                    log::warn!("{why}; it crashed too often, so it stays stopped");
                    carry.hand_over();
                    return status.send(ServerStatus::Stopped(Some(why)));
                }
                Verdict::Superseded => {
                    log::info!("{why}; a restart replaced it");
                    carry.hand_over();
                    return status.send(ServerStatus::Stopped(None));
                }
            };
            log::warn!("{why}; restarting in {} s", after.as_secs());
            status.send(ServerStatus::Restarting(n));
            if let Waited::Stopped = async_io::block_on(carry.wait_out(&*self.clock, after)) {
                process.reap();
                return status.send(ServerStatus::Stopped(None));
            }
            session_id = self.identities.new_session();
            status.send(ServerStatus::Starting);
        }
    }

    /// Runs one session on `io`, until it stops or crashes, and hands its
    /// process back.
    fn session(
        &self,
        io: ServerIo,
        session_id: SessionId,
        folder: &WorkspaceFolder,
        status: &StatusSink,
        mut carry: Carry,
    ) -> (Ended, Watched) {
        let ServerIo {
            input,
            output,
            stderr,
            process,
        } = io;
        let mut process = Watched {
            process,
            killed: false,
            switch: self.switch.clone(),
        };
        *lock(&self.session) = Session::default();
        let queue = Rc::new(Queue::default());
        let settings = Rc::new(RefCell::new(carry.settings.clone()));
        let readiness = Arc::new(Mutex::new(Readiness::new(&self.spec)));
        let signal = Rc::new(Signal::default());
        let refresh = Rc::new(Signal::default());
        let diagnostics = Rc::new(Inbox::default());
        let edits = Rc::new(Edits::default());
        let (main_loop, socket) = MainLoop::new_client(|socket| {
            client_router(ClientState {
                name: self.spec.name.clone(),
                folder: folder.clone(),
                settings: settings.clone(),
                queue: queue.clone(),
                server: self.id,
                sink: self.sink.clone(),
                clock: self.clock.clone(),
                readiness: readiness.clone(),
                signal: signal.clone(),
                refresh: refresh.clone(),
                log: self.log.clone(),
                diagnostics: diagnostics.clone(),
                edits: edits.clone(),
                session: self.session.clone(),
                _socket: socket,
            })
        });
        // The loop reports how it ended to the actor; the actor's end (its
        // `session_over` sender dropping) ends the loop and the stderr drain.
        let (loop_ended, loop_done) = oneshot::channel();
        let (session_over, over) = oneshot::channel::<()>();
        let over = over.shared();
        // The server's output is read whatever the loop is doing (see
        // [`Backlog`]), against the same budget as the loop's reads and
        // writes.
        let budget = IoBudget::default();
        let (drained, overflowed, pump) = Backlog::split(output, BACKLOG_MAX, budget.clone());
        let main_loop = {
            let over = over.clone();
            let budget = budget.clone();
            async move {
                // async-lsp's loop has no budget of its own: with the
                // server's output always ready, or a long queue to send,
                // one poll of it could last as long as the flood or the
                // queue, starving the actor and its deadlines. Its reads
                // and writes share an `IoBudget`, refilled each time the
                // loop is polled.
                let run = main_loop.run_buffered(budget.meter(drained), budget.meter(input));
                pin_mut!(run);
                // An overflowing backlog ends the loop here, not only through
                // its reader: a loop waiting for a write reads nothing.
                let run = futures::future::poll_fn(|cx| {
                    budget.refill();
                    if let Poll::Ready(err) = overflowed.poll(cx) {
                        return Poll::Ready(Err(async_lsp::Error::Io(err)));
                    }
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
        let stderr = drain_stderr(
            self.spec.name.clone(),
            stderr,
            self.log.clone(),
            self.clock.clone(),
            over,
            self.switch.clone(),
        );
        let backlog = carry.reopen();
        let actor = Actor {
            _end: EndSession(self.identities.clone(), session_id),
            status: status.clone(),
            spec: self.spec.clone(),
            settings,
            folder: folder.clone(),
            clock: self.clock.clone(),
            session: self.session.clone(),
            socket,
            routes: self.routes.clone(),
            next_id: 0,
            queue,
            sent: 0,
            pending: FuturesUnordered::new(),
            orphans: Orphans::default(),
            backlog,
            documents: HashMap::new(),
            link: Link {
                policy: SyncPolicy::from_capabilities(&ServerCapabilities::default()),
                encoding: PositionEncoding::Utf16,
                identities: self.identities.clone(),
                server: self.id,
                session: session_id,
                versions: HashMap::new(),
                outbox: VecDeque::new(),
                sink: self.sink.clone(),
                puller: Puller::new(self.spec.pull_diagnostics, self.clock.clone()),
            },
            diagnostics,
            edits,
            asked: Vec::new(),
            handed: Handed::default(),
            tick: None,
            drain_queued: false,
            moving: None,
            readiness,
            signal,
            refresh,
            due: None,
            shown: ServerStatus::Starting,
            carry,
        };
        // The pump goes last, so the loop has the budget first each poll.
        let (_, ended, (), ()) = async_io::block_on(futures::future::join4(
            main_loop,
            actor.run(&mut process, loop_done, session_over),
            stderr,
            pump,
        ));
        (ended, process)
    }
}

/// A session's process, and whether it has been killed. Dropped, or
/// [`reap`](Self::reap)ed, once the kill switch is thrown, it kills what's
/// left of the process if nothing has yet: the server exited, but its group
/// may still have members.
struct Watched {
    process: Box<dyn ServerProcess>,
    killed: bool,
    switch: Arc<Flag>,
}

impl Watched {
    fn reap(&mut self) {
        if self.switch.fired() && !self.killed {
            async_io::block_on(kill(self));
        }
    }
}

/// However the thread ends: a terminal status's sink may throw the switch.
impl Drop for Watched {
    fn drop(&mut self) {
        self.reap();
    }
}

impl ServerProcess for Watched {
    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>> {
        self.process.exited()
    }

    fn kill(&mut self) {
        self.killed = true;
        self.process.kill();
    }
}

/// How a session ended.
enum Ended {
    /// Asked to stop, or it couldn't go on: its end has been reported, and
    /// its documents handed on.
    Stopped,
    /// It crashed, and nothing has been reported yet: its documents are in
    /// the carry, for the next session.
    /// `at` is when it was found, before any cleanup.
    Crashed {
        why: String,
        at: Instant,
        carry: Carry,
    },
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a failed launch stops the server with: "<name> not found" when
/// there's nothing to run (Q2 (a)), else the launcher's error.
fn launch_error(spec: &ServerSpec, err: &io::Error) -> String {
    if err.kind() == io::ErrorKind::NotFound {
        format!("{} not found", spec.name)
    } else {
        err.to_string()
    }
}

#[derive(Clone)]
struct StatusSink {
    id: ServerId,
    name: String,
    sink: Sink,
    latest: LatestStatus,
}

impl StatusSink {
    fn send(&self, status: ServerStatus) {
        // Set first, so whoever hears of it reads it here too.
        latest(&self.latest, self.id, &status);
        let mut sink = self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        sink(LspEvent::Status {
            server: self.id,
            name: self.name.clone(),
            status,
        });
    }
}

type LoopDone = oneshot::Receiver<async_lsp::Result<()>>;
/// Resolves when the server's process has gone.
type Exited<'a> = futures::future::Fuse<Pin<Box<dyn Future<Output = ExitInfo> + Send + 'a>>>;

struct Actor {
    /// Retires the session's identities if the actor goes some way that
    /// doesn't (a panic).
    _end: EndSession,
    status: StatusSink,
    spec: ServerSpec,
    /// The spec's `settings` as they are now, shared with the router.
    settings: Rc<RefCell<serde_json::Value>>,
    folder: WorkspaceFolder,
    clock: Arc<dyn Clock>,
    session: Arc<Mutex<Session>>,
    socket: ServerSocket,
    routes: Routes,
    /// The id async-lsp gives the next request.
    next_id: i32,
    queue: Rc<Queue>,
    /// Requests and notifications handed to async-lsp since `initialize`,
    /// each followed by [`Taken`].
    sent: u64,
    /// Requests in flight; each answers its caller.
    pending: FuturesUnordered<BoxFuture<'static, Settled>>,
    /// Requests abandoned and not yet answered (requests.rs).
    orphans: Orphans,
    /// Commands carried out once initialized, ahead of new ones: those
    /// made while starting, and those a backstop tick moved here ahead of
    /// its [`Command::Drain`].
    backlog: VecDeque<Command>,
    documents: HashMap<BufferId, Document>,
    /// How the server asked for documents to be synced, and the outbox.
    link: Link,
    /// The next [`BACKSTOP`] tick, while documents are open.
    tick: Option<Timer>,
    /// A [`Command::Drain`] waits in the backlog: ticks meanwhile add
    /// nothing, so a server that doesn't read can't make the outbox grow.
    drain_queued: bool,
    /// A drain is being queued: how many waiting commands have been moved
    /// ahead of it so far ([`Actor::move_for_drain`]).
    moving: Option<usize>,
    readiness: Arc<Mutex<Readiness>>,
    /// Raised by the router when `readiness` changed.
    signal: Rc<Signal>,
    /// Raised by the router at `workspace/diagnostic/refresh`.
    refresh: Rc<Signal>,
    /// Publications the router read, to decode.
    diagnostics: Rc<Inbox>,
    /// `workspace/applyEdit`s the router read, to decode.
    edits: Rc<Edits>,
    /// The replies of those handed to the host this session: answered
    /// failed when it ends, so none of them is applied after.
    asked: Vec<HandedReply>,
    /// What the server has of each open document.
    handed: Handed,
    /// When readiness next has something to do on its own, and the timer.
    due: Option<(Instant, Timer)>,
    /// The status last reported.
    shown: ServerStatus,
    /// The manager's commands and stop, kept across sessions.
    carry: Carry,
}

/// However the actor ends, a panic included, its documents go on, unless
/// they're carried into the next session, and their diagnostics are
/// reported cleared once their identities are retired. Ended any other way,
/// it has done so already, and its documents have none left to report.
impl Drop for Actor {
    fn drop(&mut self) {
        self.end_session();
        self.hand_over();
    }
}

enum Started {
    Running,
    /// Asked to stop (or killed) before the server answered `initialize`.
    Stopped,
    Crashed(String),
}

/// Why a session ends.
enum End {
    /// Asked to stop: shut it down, unless the kill switch is thrown.
    Stop,
    /// Asked to stop before it initialized: kill it, as the protocol
    /// allows nothing to be sent to it, `shutdown` included.
    Kill,
    Crash(String),
    /// Too many orphans: shut it down, then restart it as after a crash.
    Recycle(String),
}

enum Step {
    Next(Next),
    Stop,
    Tick,
    Settled(Settled),
    Orphaned,
    Readiness,
    /// The router read diagnostics.
    Published(Vec<PublishDiagnosticsParams>),
    /// The router read `workspace/applyEdit`s.
    ApplyEdits(Vec<Asked>),
    /// A pull's reply came (pull.rs).
    Pulled(Pulled),
    /// The server asked for every open document to be pulled again.
    Refresh,
    /// Readiness's timer came due.
    Due,
    LoopEnded(Result<async_lsp::Result<()>, oneshot::Canceled>),
    Exited(ExitInfo),
}

impl Actor {
    async fn run(
        mut self,
        process: &mut dyn ServerProcess,
        mut loop_done: LoopDone,
        _session_over: oneshot::Sender<()>,
    ) -> Ended {
        let end = {
            let mut exited = process.exited().fuse();
            match self.initialize(&mut loop_done, &mut exited).await {
                Started::Running => self.serve(&mut loop_done, &mut exited).await,
                Started::Stopped => End::Kill,
                Started::Crashed(why) => End::Crash(why),
            }
        };
        // When a crash was found: cleaning up (a recycle's shutdown, say)
        // takes time that mustn't count.
        let at = self.clock.now();
        match end {
            End::Stop => {
                self.shutdown(process, false).await;
                Ended::Stopped
            }
            End::Kill => {
                self.shutdown(process, true).await;
                Ended::Stopped
            }
            End::Crash(why) => {
                // Callers waiting get `ServerDown` now.
                self.pending = FuturesUnordered::new();
                // Retired before the wait, so the dead server's
                // diagnostics stop counting while the process goes;
                // `crashed` ends the session again, to no effect.
                self.end_session();
                kill(process).await;
                self.crashed(why, at)
            }
            End::Recycle(why) => {
                self.pending = FuturesUnordered::new();
                log::warn!("{why}; recycling the session");
                // As for a crash: before the shutdown's wait.
                self.end_session();
                self.shut_down(process).await;
                self.crashed(why, at)
            }
        }
    }

    /// Carries out commands until the session ends.
    async fn serve(&mut self, loop_done: &mut LoopDone, exited: &mut Exited<'_>) -> End {
        let mut budget = Budget::default();
        loop {
            self.move_for_drain();
            let step = select_biased! {
                // `shutdown` sees it thrown, and kills the process.
                () = &mut self.carry.kill => Step::Stop,
                _ = &mut self.carry.stop => Step::Stop,
                ended = &mut *loop_done => Step::LoopEnded(ended),
                exit = &mut *exited => Step::Exited(exit),
                settled = self.pending.select_next_some() => Step::Settled(settled),
                () = self.orphans.answered().fuse() => Step::Orphaned,
                () = futures::future::poll_fn(|cx| self.signal.poll_raised(cx)).fuse() => {
                    Step::Readiness
                }
                published = futures::future::poll_fn(|cx| self.diagnostics.poll_take(cx)).fuse() => {
                    Step::Published(published)
                }
                asked = futures::future::poll_fn(|cx| self.edits.poll_take(cx)).fuse() => {
                    Step::ApplyEdits(asked)
                }
                pulled = self.link.puller.next().fuse() => Step::Pulled(pulled),
                () = futures::future::poll_fn(|cx| self.refresh.poll_raised(cx)).fuse() => {
                    Step::Refresh
                }
                () = wait_due(&mut self.due).fuse() => Step::Due,
                () = wait_tick(&mut self.tick).fuse() => Step::Tick,
                next = next_command(
                    &self.queue,
                    self.sent,
                    &mut self.link.outbox,
                    &mut self.backlog,
                    &mut self.carry.commands,
                ).fuse() => Step::Next(next),
            };
            match step {
                Step::Next(Next::Out(Outgoing::Note(note))) => self.notify(note),
                Step::Next(Next::Out(Outgoing::Request(issue))) => self.issue(issue),
                Step::Next(Next::Command(Some(command))) => self.command(command),
                // Every sender gone means the manager retired it (its stop
                // is on its way) or has gone itself: a stop either way.
                Step::Next(Next::Command(None)) => {
                    log::debug!("{}: its commands ended", self.spec.name);
                    return End::Stop;
                }
                Step::Stop => return End::Stop,
                Step::Tick => {
                    self.tick = None;
                    self.queue_drain();
                    self.arm_tick();
                }
                Step::Settled(Settled::Answered) | Step::Orphaned => {}
                Step::Settled(Settled::Abandoned { id, orphan }) => {
                    self.cancel(id);
                    if !self.orphans.adopt(orphan) {
                        return End::Recycle(format!(
                            "{} left more than {} cancelled requests unanswered",
                            self.spec.name,
                            crate::requests::ORPHANS_MAX,
                        ));
                    }
                }
                Step::Readiness => self.readiness_changed(),
                Step::Published(published) => {
                    for params in published {
                        self.published(params);
                    }
                }
                Step::ApplyEdits(asked) => {
                    for asked in asked {
                        self.apply_edit(asked);
                    }
                }
                Step::Pulled(pulled) => self.pulled(pulled),
                Step::Refresh => self.repull(),
                Step::Due => {
                    self.due = None;
                    self.readiness_changed();
                }
                Step::LoopEnded(ended) => {
                    return End::Crash(format!("{} stopped: {}", self.spec.name, loop_end(ended)));
                }
                Step::Exited(exit) => {
                    return End::Crash(format!("{} {}", self.spec.name, describe(exit)));
                }
            }
            #[cfg(test)]
            {
                let held = self.orphans.len();
                let mut session = lock(&self.session);
                session.orphans = (held, session.orphans.1.max(held));
            }
            budget.spend().await;
        }
    }

    /// The id for a request about to be sent.
    fn take_id(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Sends `issue`, unless its caller has gone already.
    fn issue(&mut self, issue: Issue) {
        if issue.gone() {
            return;
        }
        #[cfg(test)]
        self.handed(issue.method().to_owned());
        let id = self.take_id();
        self.pending.push(issue.issue(Out {
            socket: &self.socket,
            id,
            readiness: &self.readiness,
        }));
        // If the loop has gone, `loop_done` says so.
        let _ = self.socket.emit(Taken);
        self.sent += 1;
        // A tsserver pull's other requests, right behind its first, with
        // nothing between them (tsserver.rs). They may take the queue a
        // little past `QUEUED_MAX`.
        for follower in self.link.puller.released() {
            self.issue(follower);
        }
    }

    fn notify(&mut self, note: Note) {
        #[cfg(test)]
        self.handed(note.describe());
        self.handed.hand(&note);
        // If the loop has gone, `loop_done` says so.
        let ended = note.send(&self.socket);
        let _ = self.socket.emit(Taken);
        self.sent += 1;
        if let Some(ended) = ended {
            self.link.cleared(ended);
        }
    }

    /// Records what was handed to async-lsp, if the test asked.
    #[cfg(test)]
    fn handed(&self, what: String) {
        if let Some(handed) = &mut lock(&self.session).handed {
            handed.push(what);
        }
    }

    /// `$/cancelRequest` for request `id`. Its order with requests doesn't
    /// matter (that request has been sent), so it skips the window.
    fn cancel(&mut self, id: i32) {
        let _ = self.socket.notify::<Cancel>(CancelParams {
            id: NumberOrString::Number(id),
        });
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
                // A stale open (one handed over late, say) must not replace
                // the document a newer open made.
                if !self.link.identities.is_current(buffer, opening.generation) {
                    return;
                }
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
                }
            }
            Command::Admit {
                buffer,
                version,
                admit,
            } => {
                for issue in admit(self.admit(buffer, version)) {
                    self.link.outbox.push_back(Outgoing::Request(issue));
                }
            }
            Command::WithDocuments(admit) => {
                let ids: Vec<BufferId> = self.documents.keys().copied().collect();
                for id in ids {
                    self.drain(id);
                }
                if let Some(issue) = admit(Ok(self.opened())) {
                    self.link.outbox.push_back(Outgoing::Request(issue));
                }
            }
            Command::Documents(with) => with(Opened {
                encoding: self.link.encoding,
                documents: self.handed.documents(),
            }),
            Command::Configure(settings) => {
                self.settings.replace(settings.clone());
                self.carry.settings = settings.clone();
                // If the loop has gone, `loop_done` says so.
                let _ = self
                    .socket
                    .notify::<DidChangeConfiguration>(DidChangeConfigurationParams { settings });
                let _ = self.socket.emit(Taken);
                self.sent += 1;
            }
            Command::Notify { method, params } => {
                if crate::notify::send(&self.socket, method, params) {
                    let _ = self.socket.emit(Taken);
                    self.sent += 1;
                }
            }
            Command::Drain => {
                self.drain_queued = false;
                let ids: Vec<BufferId> = self.documents.keys().copied().collect();
                for id in ids {
                    self.drain(id);
                }
            }
            #[cfg(test)]
            Command::Ring(buffer, answer) => {
                let ring = self.documents.get(&buffer).map(Document::ring);
                let _ = answer.send(ring.unwrap_or_default());
            }
        }
    }

    /// Starts queueing a drain of every document behind the commands
    /// waiting now, unless one is queued or on its way.
    fn queue_drain(&mut self) {
        if !self.drain_queued && self.moving.is_none() {
            self.moving = Some(0);
        }
    }

    /// Moves up to [`STEPS_PER_YIELD`] waiting commands into the backlog
    /// for a drain being queued, and queues it once none are left, or once
    /// [`DRAIN_BEHIND_MAX`] have gone ahead of it: a step at a time, so a
    /// flood of commands can't keep the actor from its stop and deadlines,
    /// nor put the drain off for ever.
    fn move_for_drain(&mut self) {
        let Some(moved) = self.moving.as_mut() else {
            return;
        };
        for _ in 0..STEPS_PER_YIELD {
            if *moved >= DRAIN_BEHIND_MAX {
                break;
            }
            match self.carry.commands.try_recv() {
                Ok(command) => {
                    self.backlog.push_back(command);
                    *moved += 1;
                }
                Err(_) => {
                    *moved = DRAIN_BEHIND_MAX;
                    break;
                }
            }
        }
        if *moved >= DRAIN_BEHIND_MAX {
            self.moving = None;
            self.backlog.push_back(Command::Drain);
            self.drain_queued = true;
        }
    }

    /// Forwards the routed documents, open or still queued, to where the
    /// manager now places them ([`Routes`]), at the text drained so far:
    /// what the buffer did since waits in its subscription for the next
    /// server's drains. Closes the command channel first, and takes every
    /// command still in it; the others are dropped with the session.
    fn hand_over(&mut self) {
        self.carry.openings.extend(
            self.documents
                .drain()
                .map(|(_, document)| document.into_opening()),
        );
        // Opened later than those, so they go after them.
        self.carry
            .openings
            .extend(self.backlog.drain(..).filter_map(|command| match command {
                Command::Open(opening) => Some(*opening),
                _ => None,
            }));
        self.tick = None;
        self.drain_queued = false;
        self.moving = None;
        self.carry.hand_over();
    }

    /// The session crashed: its identities are retired, and its documents
    /// and commands go to the next session ([`Carry`]).
    fn crashed(mut self, why: String, at: Instant) -> Ended {
        self.end_session();
        {
            let mut session = lock(&self.session);
            session.capabilities = None;
            session.encoding = None;
            session.registrations = Registrations::default();
        }
        let documents: Vec<Opening> = self
            .documents
            .drain()
            .map(|(_, document)| document.into_opening())
            .collect();
        self.carry.openings.extend(documents);
        for command in std::mem::take(&mut self.backlog) {
            self.carry.hold(command);
        }
        self.link.outbox.clear();
        let (_, stop) = oneshot::channel();
        let spent = Carry {
            commands: mpsc::unbounded().1,
            stop,
            kill: self.carry.kill.again(),
            openings: Vec::new(),
            settings: serde_json::Value::Null,
            routes: self.routes.clone(),
            identities: self.link.identities.clone(),
        };
        let carry = std::mem::replace(&mut self.carry, spent);
        Ended::Crashed { why, at, carry }
    }

    /// Retires the session's identities, then reports the diagnostics of
    /// each document the server had open cleared, and of each whose
    /// `didClose` is still in the outbox, which is never sent now.
    fn end_session(&mut self) {
        for reply in self.asked.drain(..) {
            reply.fail("the server's session ended");
        }
        let mut ended: Vec<_> = (self.documents.iter_mut())
            .filter_map(|(&buffer, document)| Some((buffer, document.end_session()?)))
            .collect();
        ended.extend(self.link.outbox.iter().filter_map(|out| match out {
            Outgoing::Note(note) => note.ends(),
            Outgoing::Request(_) => None,
        }));
        self.link.outbox.clear();
        self.link.identities.end_session(self.link.session);
        for ended in ended {
            self.link.cleared(ended);
        }
    }

    /// Decodes a publication for the URI's current incarnation, or drops it
    /// (diagnostics.rs).
    fn published(&mut self, params: PublishDiagnosticsParams) {
        // `to_file_path` ignores the scheme, so a `custom:` URI of an open file's
        // path would otherwise match it.
        let path = (params.uri.scheme() == "file")
            .then(|| params.uri.to_file_path().ok())
            .flatten();
        if !(self.documents.values()).any(|document| same_file(document.uri(), &params.uri, &path))
        {
            // No text to decode against here: the host does, with the file.
            let items = (params.diagnostics.into_iter())
                .map(FileDiagnostic::new)
                .collect();
            self.link.report(LspEvent::FileDiagnostics {
                server: self.status.id,
                uri: params.uri,
                path,
                encoding: self.link.encoding,
                items,
            });
            return;
        }
        let mut current = (self.documents.values_mut()).filter(|document| {
            document.is_synced() && same_file(document.uri(), &params.uri, &path)
        });
        let (Some(document), None) = (current.next(), current.next()) else {
            log::debug!(
                "{}: dropped diagnostics for {}, still opening or several",
                self.spec.name,
                params.uri,
            );
            return;
        };
        let Some(version) = params.version else {
            match self.spec.unversioned_diagnostics {
                UnversionedDiagnostics::Drop => {
                    log::debug!(
                        "{}: dropped unversioned diagnostics for {}",
                        self.spec.name,
                        params.uri,
                    );
                    return;
                }
            }
        };
        if let Some(event) = document.publish(version, params.diagnostics, &self.link) {
            self.link.report(event);
        }
    }

    /// The documents the server has open, as last sent.
    fn opened(&self) -> Opened {
        Opened {
            encoding: self.link.encoding,
            documents: (self.documents.values())
                .filter_map(Document::current)
                .collect(),
        }
    }

    /// Decodes a `workspace/applyEdit` against the documents as handed to
    /// the server ([`Handed`]), and hands it to the host to answer,
    /// or answers it not applied if it's refused (workspace_edit.rs). One
    /// already answered (it timed out) is dropped.
    fn apply_edit(&mut self, Asked { params, reply }: Asked) {
        if reply.is_answered() {
            return;
        }
        match decode(params.edit, &self.handed.documents(), self.link.encoding) {
            Err(why) => {
                log::info!("{}: refused workspace/applyEdit: {why}", self.spec.name);
                reply.failed(why);
            }
            Ok(edits) if edits.is_empty() => reply.applied(),
            Ok(edits) => {
                self.asked.retain(HandedReply::is_open);
                self.asked.push(HandedReply::of(&reply));
                self.link.report(LspEvent::ApplyEdit {
                    server: self.link.server,
                    label: params.label,
                    edits,
                    reply,
                });
            }
        }
    }

    /// Takes a pull's reply for its document, or drops it (pull.rs).
    fn pulled(&mut self, pulled: Pulled) {
        let Some(document) = self.documents.get_mut(&pulled.buffer) else {
            log::debug!(
                "{}: dropped pulled diagnostics for a closed document",
                self.spec.name
            );
            return;
        };
        // The reply is decoded in the link's encoding: for the one call,
        // the reply's own if it has one (tsserver's).
        let session = self.link.encoding;
        if let Some(encoding) = pulled.encoding() {
            self.link.encoding = encoding;
        }
        let event = document.pulled(pulled, &mut self.link);
        self.link.encoding = session;
        if let Some(event) = event {
            self.link.report(event);
        }
    }

    /// Pulls every open document again (pull.rs).
    fn repull(&mut self) {
        for document in self.documents.values_mut() {
            document.repull(&mut self.link);
        }
    }

    /// Drains `buffer`'s document, then pulls it if that sent something.
    fn drain(&mut self, buffer: BufferId) {
        if let Some(document) = self.documents.get_mut(&buffer) {
            document.drain(&mut self.link);
            document.pull(&mut self.link);
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

    /// Brings readiness up to now: applies what time changed, reports what
    /// it shows if that changed, and arms its next timer.
    fn readiness_changed(&mut self) {
        let now = self.clock.now();
        let mut readiness = lock(&self.readiness);
        readiness.tick(now);
        if readiness.take_fallback() {
            log::warn!(
                "{}: no experimental/serverStatus within {} s of initialize; routing by \
                 progress, and its empty answers are never final",
                self.spec.name,
                crate::readiness::STATUS_WAIT.as_secs(),
            );
        }
        let view = readiness.view();
        let next = readiness.next_due();
        drop(readiness);
        if view != self.shown {
            self.shown = view.clone();
            let ready = view == ServerStatus::Ready;
            self.status.send(view);
            if ready {
                self.repull();
            }
        }
        match next {
            Some(at) if self.due.as_ref().is_none_or(|(armed, _)| *armed != at) => {
                let sleep = self.clock.sleep(at.saturating_duration_since(now));
                self.due = Some((at, sleep));
            }
            Some(_) => {}
            None => self.due = None,
        }
    }

    async fn initialize(&mut self, loop_done: &mut LoopDone, exited: &mut Exited<'_>) -> Started {
        let params = initialize_params(&self.spec, &self.folder);
        self.take_id();
        let reply = send_request::<InitializeJson>(&self.socket, initialize_json(params)).fuse();
        let timeout = self.clock.sleep(INITIALIZE_TIMEOUT).fuse();
        pin_mut!(reply, timeout);
        // A stop while starting ends it at once: the protocol allows nothing
        // to be sent before the answer, `shutdown` included, so it's killed
        // rather than waited for (up to the timeout).
        let mut budget = Budget::default();
        let result = loop {
            select_biased! {
                result = reply => break result,
                () = &mut self.carry.kill => return Started::Stopped,
                _ = &mut self.carry.stop => return Started::Stopped,
                ended = &mut *loop_done => {
                    return Started::Crashed(format!(
                        "{} stopped before answering initialize: {}",
                        self.spec.name,
                        loop_end(ended),
                    ));
                }
                exit = &mut *exited => {
                    return Started::Crashed(format!(
                        "{} {} before answering initialize",
                        self.spec.name,
                        describe(exit),
                    ));
                }
                () = timeout => {
                    return Started::Crashed(format!(
                        "{} didn't answer initialize within {} s",
                        self.spec.name,
                        INITIALIZE_TIMEOUT.as_secs(),
                    ));
                }
                command = self.carry.commands.next() => match command {
                    // So a configuration request during the start gets
                    // them; the notification waits.
                    Some(Command::Configure(settings)) => {
                        self.settings.replace(settings.clone());
                        self.backlog.push_back(Command::Configure(settings));
                    }
                    Some(command) => self.backlog.push_back(command),
                    None => return Started::Stopped,
                },
            }
            budget.spend().await;
        };
        let result = match result {
            Ok(result) => result,
            // The loop has gone, taking the reply's channel with it: report
            // why it went.
            Err(async_lsp::Error::ServiceStopped) => {
                return Started::Crashed(format!(
                    "{} stopped before answering initialize: {}",
                    self.spec.name,
                    loop_end(loop_done.await),
                ));
            }
            Err(err) => {
                return Started::Crashed(format!("{}: initialize failed: {err}", self.spec.name));
            }
        };
        self.record(result);
        if self
            .socket
            .notify::<Initialized>(InitializedParams {})
            .is_err()
        {
            return Started::Crashed(format!(
                "{} stopped after answering initialize: {}",
                self.spec.name,
                loop_end(loop_done.await),
            ));
        }
        lock(&self.readiness).initialized(self.clock.now());
        self.readiness_changed();
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
        self.link.puller.set_capabilities(&result.capabilities);
        self.link.encoding = encoding;
        let mut session = lock(&self.session);
        session.capabilities = Some(result.capabilities);
        session.encoding = Some(encoding);
    }

    /// Stops the session at the manager's request: its documents go on,
    /// its identities are retired and its callers answered, then
    /// [`shut_down`](Self::shut_down), or, if `force`, [`kill`].
    async fn shutdown(&mut self, process: &mut dyn ServerProcess, force: bool) {
        // However it came to stop, its documents go on first, their
        // identities retired.
        self.end_session();
        self.hand_over();
        // Callers of requests in flight or kept back get `ServerDown` now,
        // and those of requests still unread once the session is over.
        self.pending = FuturesUnordered::new();
        self.backlog.clear();
        self.link.outbox.clear();
        self.documents.clear();
        self.tick = None;
        self.drain_queued = false;
        if force {
            kill(process).await;
        } else {
            self.shut_down(process).await;
        }
        self.stopped(None);
    }

    /// `shutdown` (up to 2 s), `exit`, up to 1 s for the process to go,
    /// then `kill`. Whatever the server does after `exit` (a non-zero code,
    /// closing its output) is a normal stop. Once the kill switch is
    /// thrown, it's killed at once, waiting for nothing more.
    async fn shut_down(&mut self, process: &mut dyn ServerProcess) {
        if self.carry.kill.thrown() {
            return kill(process).await;
        }
        self.take_id();
        let reply = send_request::<Shutdown>(&self.socket, ()).fuse();
        let timeout = self.clock.sleep(SHUTDOWN_TIMEOUT).fuse();
        pin_mut!(reply, timeout);
        select_biased! {
            () = &mut self.carry.kill => return kill(process).await,
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
            // The kill first: an exit seen as it's thrown may be only the
            // group's leader.
            select_biased! {
                () = &mut self.carry.kill => {
                    log::info!("{}: killed before it exited", self.spec.name);
                    false
                }
                exit = exited => {
                    log::info!("{} exited: {exit:?}", self.spec.name);
                    true
                }
                () = timeout => {
                    log::info!(
                        "{} still running {} s after exit; killing it",
                        self.spec.name,
                        EXIT_TIMEOUT.as_secs(),
                    );
                    false
                }
            }
        };
        if !exited {
            kill(process).await;
        }
    }

    /// Reports the session over. The manager sees the command channel
    /// closed first, so a `start` prompted by this status starts afresh.
    fn stopped(&mut self, why: Option<String>) {
        // A session ending on its own (a failed start) hands its documents
        // back too.
        self.end_session();
        self.hand_over();
        self.status.send(ServerStatus::Stopped(why));
    }
}

/// How a process went, for a crash's message.
fn describe(exit: ExitInfo) -> String {
    match exit {
        ExitInfo {
            code: Some(code), ..
        } => format!("exited with code {code}"),
        ExitInfo {
            signal: Some(signal),
            ..
        } => format!("was killed by signal {signal}"),
        ExitInfo { .. } => "exited".into(),
    }
}

/// Kills `process` and waits until it has gone, so the instance replacing
/// it never runs beside it. [`ServerProcess::exited`] must resolve once a
/// killed process has gone.
async fn kill(process: &mut dyn ServerProcess) {
    process.kill();
    process.exited().await;
}

/// Closes `commands`, then forwards `openings` and every opening still
/// queued there, after them, as [`Places::forward`] does.
///
/// It takes commands until the channel has ended, not just until it's
/// empty: a send the channel counted before the close may still be on its
/// way into the queue, and an open there would be dropped with the
/// receiver. Sends the close comes before fail, and their documents wait.
pub(crate) fn hand_over(
    commands: &mut mpsc::UnboundedReceiver<Command>,
    mut openings: Vec<Opening>,
    routes: &Routes,
    identities: &Identities,
) {
    commands.close();
    loop {
        match commands.try_recv() {
            Ok(Command::Open(opening)) => openings.push(*opening),
            Ok(_) => {}
            Err(mpsc::TryRecvError::Closed) => break,
            // Another thread is part-way through a send, as futures'
            // receiver waits for when it's dropped.
            Err(mpsc::TryRecvError::Empty) => thread::yield_now(),
        }
    }
    if !openings.is_empty() {
        routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .forward(identities, openings);
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

/// Keeps the server's stderr in its log ring, and logs it, until it
/// closes or the session ends. Then it reads on, a step at a time, for as
/// long as there's something ready to read, however much, so a crashing
/// server's last words are kept, the ring holding the newest. It stops at
/// the end of the stream, when nothing more is ready, or, for a stream
/// that never stops being ready (something else keeps writing it),
/// [`STDERR_GRACE`] on the clock after the session ended, or once the
/// kill `switch` is thrown.
async fn drain_stderr(
    name: String,
    stderr: Option<Pin<Box<dyn AsyncRead + Send>>>,
    ring: SharedLog,
    clock: Arc<dyn Clock>,
    over: futures::future::Shared<oneshot::Receiver<()>>,
    switch: Arc<Flag>,
) {
    let Some(mut stderr) = stderr else { return };
    let mut buf = vec![0; 4096];
    let keep = |bytes: &[u8]| {
        lock(&ring).push(bytes);
        log::debug!(
            "{name} stderr: {}",
            String::from_utf8_lossy(bytes).trim_end()
        );
    };
    {
        let drain = async {
            let mut budget = Budget::default();
            while let Ok(n @ 1..) = stderr.read(&mut buf).await {
                keep(&buf[..n]);
                budget.spend().await;
            }
        }
        .fuse();
        pin_mut!(drain);
        let mut over = over.fuse();
        select_biased! {
            _ = over => {}
            () = drain => return,
        }
    }
    let mut grace = clock.sleep(STDERR_GRACE);
    let mut budget = Budget::default();
    while !switch.fired()
        && grace.as_mut().now_or_never().is_none()
        && let Some(Ok(n @ 1..)) = stderr.read(&mut buf).now_or_never()
    {
        keep(&buf[..n]);
        budget.spend().await;
    }
}

/// How long stderr that never stops being ready is read after its
/// session ended.
pub(crate) const STDERR_GRACE: Duration = Duration::from_secs(1);

enum Next {
    Out(Outgoing),
    /// `None` once every sender has gone: the manager retired the server
    /// (its places point elsewhere, and its stop is on its way) or has
    /// gone itself.
    Command(Option<Command>),
}

/// What to do next, once async-lsp's queue has room: send what's first in
/// the outbox, else carry out the backlog (where a backstop tick queues
/// its drain, behind the commands waiting then), then new commands. So
/// the outbox only fills when it's empty and there's room, from one drain
/// or one command at a time, and commands and drains take turns in the
/// order they came.
async fn next_command(
    queue: &Queue,
    sent: u64,
    outbox: &mut VecDeque<Outgoing>,
    backlog: &mut VecDeque<Command>,
    commands: &mut mpsc::UnboundedReceiver<Command>,
) -> Next {
    futures::future::poll_fn(|cx| queue.poll_room(sent, cx)).await;
    if let Some(out) = outbox.pop_front() {
        return Next::Out(out);
    }
    match backlog.pop_front() {
        Some(command) => Next::Command(Some(command)),
        None => Next::Command(commands.next().await),
    }
}

/// The version of each open document last handed to async-lsp: what the
/// server has, as far as the actor can know, to read its
/// `workspace/applyEdit`s against (workspace_edit.rs). Kept as notes are
/// handed, not from the documents' rings, which run ahead of it while notes
/// wait in the outbox, and lose versions to newer ones or to a close.
#[derive(Default)]
pub(crate) struct Handed(HashMap<BufferId, SentDocument>);

impl Handed {
    /// `note` is being handed to async-lsp.
    pub(crate) fn hand(&mut self, note: &Note) {
        match note {
            Note::Open(_, sent) | Note::Change(_, sent) => {
                self.0.insert(sent.buffer, (**sent).clone());
            }
            Note::Close(_, (buffer, identity)) => {
                if self
                    .0
                    .get(buffer)
                    .is_some_and(|sent| sent.identity == *identity)
                {
                    self.0.remove(buffer);
                }
            }
            Note::Save(_) => {}
        }
    }

    /// Every document the server has open, at the version it has.
    pub(crate) fn documents(&self) -> Vec<SentDocument> {
        self.0.values().cloned().collect()
    }
}

/// Resolves when readiness's timer is due; never, while it isn't running.
async fn wait_due(due: &mut Option<(Instant, Timer)>) {
    match due {
        Some((_, timer)) => timer.await,
        None => futures::future::pending().await,
    }
}

/// Resolves when the backstop tick is due; never, while it isn't running.
async fn wait_tick(tick: &mut Option<Timer>) {
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

/// The most server output a [`Backlog`] holds for the main loop before
/// the session ends with an error.
pub(crate) const BACKLOG_MAX: usize = 64 << 20;

/// The most the pump reads at once.
const PUMP_CHUNK: usize = 64 << 10;

/// The server's output, read by the pump as it comes and held until
/// async-lsp's main loop reads it.
///
/// The loop reads nothing while it waits for a write to finish (its
/// `feed`, in async-lsp 0.2.4's `MainLoop::run`). A server that, like
/// rust-analyzer's `lsp-server`, reads its input only while its output
/// isn't blocked would then never take that write: each would wait for the
/// other. The pump, a task of its own beside the loop, keeps reading.
///
/// The end of the output, or an error reading it, reaches the loop after
/// the bytes before it, as it would have read them. The pump's reads spend
/// the loop's [`IoBudget`], as the loop's own reads and writes do.
///
/// **Bound.** Once more than `max` bytes ([`BACKLOG_MAX`] in a session)
/// wait for the loop, the pump stops reading, and the loop's reader yields
/// an error ("server output backlog over 64 MiB") after those bytes. The
/// loop may be waiting for a write, reading nothing, so [`Overflowed`]
/// tells the session's wrapper of the loop too, which ends the loop with
/// that error at once. The session then ends as on any transport error (a
/// crash, the server killed and restarted, as a broken pipe would be),
/// rather than holding a server's output without limit.
#[derive(Default)]
struct Backlog {
    bytes: RefCell<VecDeque<u8>>,
    /// How the output ended, once it has: handed to the loop's reader
    /// after the bytes.
    end: RefCell<Option<io::Result<()>>>,
    /// Set once the pump has stopped reading, for good.
    stopped: Cell<bool>,
    /// Set once the loop's reader has yielded `end`.
    ended: Cell<bool>,
    /// The bound, once the pump has gone past it.
    overflowed: Cell<Option<usize>>,
    /// Set when the loop's reader is dropped: the pump then stops.
    closed: Cell<bool>,
    reader: RefCell<Option<Waker>>,
    pump: RefCell<Option<Waker>>,
    watcher: RefCell<Option<Waker>>,
}

impl Backlog {
    /// The loop's reader of `output`, what says the backlog overflowed,
    /// and the pump that fills it, holding at most `max` bytes unread.
    fn split<R: AsyncRead + Unpin>(
        output: R,
        max: usize,
        budget: IoBudget,
    ) -> (Drained, Overflowed, impl Future<Output = ()>) {
        let backlog = Rc::new(Self::default());
        let pump = {
            let backlog = backlog.clone();
            let mut output = budget.meter(output);
            let mut chunk = vec![0; PUMP_CHUNK];
            futures::future::poll_fn(move |cx| backlog.pump(cx, &mut output, &mut chunk, max))
        };
        (Drained(backlog.clone()), Overflowed(backlog), pump)
    }

    /// Reads `output` until it's pending or has ended.
    fn pump(
        &self,
        cx: &mut Context<'_>,
        mut output: impl AsyncRead + Unpin,
        chunk: &mut [u8],
        max: usize,
    ) -> Poll<()> {
        loop {
            if self.closed.get() || self.stopped.get() {
                return Poll::Ready(());
            }
            let held = self.bytes.borrow().len();
            if held > max {
                self.overflowed.set(Some(max));
                if let Some(waker) = self.watcher.borrow_mut().take() {
                    waker.wake();
                }
                self.finish(Err(overflow(max)));
                return Poll::Ready(());
            }
            // No further past `max` than a byte.
            let room = chunk.len().min(max + 1 - held);
            match Pin::new(&mut output).poll_read(cx, &mut chunk[..room]) {
                Poll::Pending => {
                    *self.pump.borrow_mut() = Some(cx.waker().clone());
                    return Poll::Pending;
                }
                Poll::Ready(Ok(0)) => self.finish(Ok(())),
                Poll::Ready(Ok(n)) => {
                    self.bytes.borrow_mut().extend(&chunk[..n]);
                    self.wake_reader();
                }
                Poll::Ready(Err(err)) => self.finish(Err(err)),
            }
        }
    }

    fn finish(&self, end: io::Result<()>) {
        self.stopped.set(true);
        *self.end.borrow_mut() = Some(end);
        self.wake_reader();
    }

    fn wake_reader(&self) {
        if let Some(waker) = self.reader.borrow_mut().take() {
            waker.wake();
        }
    }
}

/// The error a [`Backlog`] past `max` ends the loop with.
fn overflow(max: usize) -> io::Error {
    let over = if max >= 1 << 20 {
        format!("{} MiB", max >> 20)
    } else {
        format!("{max} bytes")
    };
    io::Error::other(format!("server output backlog over {over}"))
}

/// Says when a [`Backlog`] has gone past its bound, whether or not the
/// main loop is reading.
struct Overflowed(Rc<Backlog>);

impl Overflowed {
    fn poll(&self, cx: &Context<'_>) -> Poll<io::Error> {
        match self.0.overflowed.get() {
            Some(max) => Poll::Ready(overflow(max)),
            None => {
                *self.0.watcher.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// The main loop's reader of a [`Backlog`]: what the pump has read, then
/// how the output ended, then nothing.
struct Drained(Rc<Backlog>);

impl AsyncRead for Drained {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let backlog = &*self.0;
        let mut bytes = backlog.bytes.borrow_mut();
        if !bytes.is_empty() {
            return Poll::Ready(io::Read::read(&mut *bytes, buf));
        }
        if backlog.ended.get() {
            return Poll::Ready(Ok(0));
        }
        match backlog.end.borrow_mut().take() {
            Some(end) => {
                backlog.ended.set(true);
                Poll::Ready(end.map(|()| 0))
            }
            None => {
                *backlog.reader.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl Drop for Drained {
    fn drop(&mut self) {
        self.0.closed.set(true);
        if let Some(waker) = self.0.pump.borrow_mut().take() {
            waker.wake();
        }
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

/// `initialize` with its parameters as JSON ([`initialize_json`]).
enum InitializeJson {}

impl Request for InitializeJson {
    type Params = serde_json::Value;
    type Result = InitializeResult;
    const METHOD: &'static str = Initialize::METHOD;
}

/// `params` as sent: with `workspace.diagnostics { refreshSupport: true }`
/// (pull.rs), which lsp-types 0.95.1 would name `workspace.diagnostic`.
fn initialize_json(params: InitializeParams) -> serde_json::Value {
    let mut params = serde_json::to_value(params).expect("InitializeParams serialize");
    params["capabilities"]["workspace"]["diagnostics"] =
        serde_json::json!({ "refreshSupport": true });
    params
}

/// `initialize`'s parameters (§3.4): the single root, the spec's
/// `initializationOptions`, and what M4 uses (§7) plus rename, code
/// actions, formatting and signature help, with every `dynamicRegistration`
/// off (§3.6).
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
            inlay_hint: Some(InlayHintWorkspaceClientCapabilities {
                refresh_support: Some(true),
            }),
            // Reported by the refresh-events card.
            semantic_tokens: Some(SemanticTokensWorkspaceClientCapabilities {
                refresh_support: Some(true),
            }),
            workspace_folders: Some(true),
            configuration: Some(true),
            // No willCreate or willDelete: their edits would need the same
            // flow for operations fastedit can't hold.
            file_operations: Some(WorkspaceFileOperationsClientCapabilities {
                dynamic_registration: dynamic,
                did_create: Some(true),
                will_rename: Some(true),
                did_rename: Some(true),
                did_delete: Some(true),
                ..Default::default()
            }),
            // Answered by the host, through `LspEvent::ApplyEdit`
            // (workspace_edit.rs), which takes edits to files' text only.
            apply_edit: Some(true),
            workspace_edit: Some(WorkspaceEditClientCapabilities {
                document_changes: Some(true),
                resource_operations: Some(Vec::new()),
                failure_handling: Some(FailureHandlingKind::Abort),
                ..Default::default()
            }),
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
                    snippet_support: Some(true),
                    commit_characters_support: Some(true),
                    insert_replace_support: Some(false),
                    resolve_support: Some(CompletionItemCapabilityResolveSupport {
                        properties: vec![
                            "documentation".into(),
                            "detail".into(),
                            "additionalTextEdits".into(),
                        ],
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
            declaration: Some(GotoCapability {
                dynamic_registration: dynamic,
                link_support: Some(true),
            }),
            type_definition: Some(GotoCapability {
                dynamic_registration: dynamic,
                link_support: Some(true),
            }),
            implementation: Some(GotoCapability {
                dynamic_registration: dynamic,
                link_support: Some(true),
            }),
            document_highlight: Some(DynamicRegistrationClientCapabilities {
                dynamic_registration: dynamic,
            }),
            inlay_hint: Some(InlayHintClientCapabilities {
                dynamic_registration: dynamic,
                resolve_support: Some(InlayHintResolveClientCapabilities {
                    properties: ["tooltip", "textEdits", "label.tooltip"]
                        .map(String::from)
                        .to_vec(),
                }),
            }),
            // Full and delta; ranges only help with huge files.
            semantic_tokens: Some(SemanticTokensClientCapabilities {
                dynamic_registration: dynamic,
                requests: SemanticTokensClientCapabilitiesRequests {
                    range: None,
                    full: Some(SemanticTokensFullOptions::Delta { delta: Some(true) }),
                },
                token_types: semantic_tokens::TOKEN_TYPES.to_vec(),
                token_modifiers: semantic_tokens::TOKEN_MODIFIERS.to_vec(),
                formats: vec![TokenFormat::RELATIVE],
                overlapping_token_support: Some(false),
                multiline_token_support: Some(false),
                server_cancel_support: None,
                augments_syntax_tokens: Some(true),
            }),
            references: Some(DynamicRegistrationClientCapabilities {
                dynamic_registration: dynamic,
            }),
            rename: Some(RenameClientCapabilities {
                dynamic_registration: dynamic,
                prepare_support: Some(true),
                ..Default::default()
            }),
            code_action: Some(CodeActionClientCapabilities {
                dynamic_registration: dynamic,
                code_action_literal_support: Some(CodeActionLiteralSupport {
                    code_action_kind: CodeActionKindLiteralSupport {
                        value_set: [
                            CodeActionKind::EMPTY,
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::REFACTOR,
                            CodeActionKind::REFACTOR_EXTRACT,
                            CodeActionKind::REFACTOR_INLINE,
                            CodeActionKind::REFACTOR_REWRITE,
                            CodeActionKind::SOURCE,
                            CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
                            CodeActionKind::SOURCE_FIX_ALL,
                        ]
                        .map(|kind| kind.as_str().to_owned())
                        .into(),
                    },
                }),
                is_preferred_support: Some(true),
                data_support: Some(true),
                resolve_support: Some(CodeActionCapabilityResolveSupport {
                    properties: vec!["edit".into()],
                }),
                ..Default::default()
            }),
            formatting: Some(DynamicRegistrationClientCapabilities {
                dynamic_registration: dynamic,
            }),
            range_formatting: Some(DynamicRegistrationClientCapabilities {
                dynamic_registration: dynamic,
            }),
            signature_help: Some(SignatureHelpClientCapabilities {
                dynamic_registration: dynamic,
                signature_information: Some(SignatureInformationSettings {
                    documentation_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                    parameter_information: Some(ParameterInformationSettings {
                        label_offset_support: Some(true),
                    }),
                    active_parameter_support: Some(true),
                }),
                context_support: Some(true),
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
            diagnostic: Some(DiagnosticClientCapabilities {
                dynamic_registration: dynamic,
                related_document_support: Some(false),
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

#[cfg(test)]
mod pump_tests {
    use super::*;

    /// Serves `bytes`, then `end`, counting what it has served.
    struct Source {
        bytes: Vec<u8>,
        at: Rc<Cell<usize>>,
        end: Option<io::Error>,
    }

    impl AsyncRead for Source {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            let at = self.at.get();
            if at == self.bytes.len() {
                return Poll::Ready(self.end.take().map_or(Ok(0), Err));
            }
            let n = buf.len().min(self.bytes.len() - at);
            buf[..n].copy_from_slice(&self.bytes[at..at + n]);
            self.at.set(at + n);
            Poll::Ready(Ok(n))
        }
    }

    fn source(len: usize, end: Option<io::Error>) -> (Source, Rc<Cell<usize>>, Vec<u8>) {
        let bytes: Vec<u8> = (0..len).map(|n| (n % 251) as u8).collect();
        let at = Rc::new(Cell::new(0));
        let source = Source {
            bytes: bytes.clone(),
            at: at.clone(),
            end,
        };
        (source, at, bytes)
    }

    fn cx() -> Context<'static> {
        Context::from_waker(futures::task::noop_waker_ref())
    }

    /// Runs the pump with a fresh budget until it stops.
    fn pump_out(pump: impl Future<Output = ()>, budget: &IoBudget) {
        pin_mut!(pump);
        for _ in 0..100 {
            budget.refill();
            if pump.as_mut().poll(&mut cx()).is_ready() {
                return;
            }
        }
        panic!("the pump didn't stop");
    }

    /// What `drained` yields, 300 bytes at a time, until it's pending, has
    /// ended or has failed, and how it ended (`None` when pending).
    fn read(drained: &mut Drained) -> (Vec<u8>, Option<io::Result<()>>) {
        let mut got = Vec::new();
        let mut buf = [0; 300];
        loop {
            match Pin::new(&mut *drained).poll_read(&mut cx(), &mut buf) {
                Poll::Ready(Ok(0)) => return (got, Some(Ok(()))),
                Poll::Ready(Ok(n)) => got.extend_from_slice(&buf[..n]),
                Poll::Ready(Err(err)) => return (got, Some(Err(err))),
                Poll::Pending => return (got, None),
            }
        }
    }

    #[test]
    fn the_output_backlog_bound_is_an_error() {
        let (source, at, bytes) = source(4096, None);
        let budget = IoBudget::default();
        let (mut drained, overflowed, pump) = Backlog::split(source, 1024, budget.clone());
        assert!(overflowed.poll(&cx()).is_pending());
        // The reader isn't polled: the pump stops a byte past the bound.
        pump_out(pump, &budget);
        assert_eq!(at.get(), 1025);
        let Poll::Ready(err) = overflowed.poll(&cx()) else {
            panic!("no overflow");
        };
        assert_eq!(err.to_string(), "server output backlog over 1024 bytes");
        let (got, end) = read(&mut drained);
        assert_eq!(got, bytes[..1025]);
        let err = end.unwrap().unwrap_err();
        assert_eq!(err.to_string(), "server output backlog over 1024 bytes");
        // Then nothing.
        let (got, end) = read(&mut drained);
        assert!(got.is_empty());
        assert!(end.unwrap().is_ok());
        assert_eq!(at.get(), 1025);
        assert_eq!(BACKLOG_MAX, 64 << 20);
    }

    #[test]
    fn the_end_of_the_output_and_its_errors_follow_its_bytes() {
        for fails in [false, true] {
            let end = fails.then(|| io::Error::from(io::ErrorKind::ConnectionReset));
            let (source, at, bytes) = source(3000, end);
            let budget = IoBudget::default();
            let (mut drained, overflowed, pump) =
                Backlog::split(source, BACKLOG_MAX, budget.clone());
            pump_out(pump, &budget);
            assert_eq!(at.get(), 3000);
            assert!(overflowed.poll(&cx()).is_pending());
            let (got, end) = read(&mut drained);
            assert_eq!(got, bytes);
            match end.unwrap() {
                Ok(()) => assert!(!fails),
                Err(err) => {
                    assert!(fails);
                    assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
                }
            }
            assert!(read(&mut drained).0.is_empty());
        }
    }

    #[test]
    fn the_reader_waits_for_the_pump_and_the_pump_spends_the_budget() {
        let (source, at, _) = source(2 * STEPS_PER_YIELD as usize * PUMP_CHUNK, None);
        let budget = IoBudget::default();
        let (mut drained, _, pump) = Backlog::split(source, BACKLOG_MAX, budget.clone());
        // Nothing pumped yet.
        assert!(read(&mut drained).1.is_none());
        pin_mut!(pump);
        budget.refill();
        assert!(pump.as_mut().poll(&mut cx()).is_pending());
        assert_eq!(at.get(), STEPS_PER_YIELD as usize * PUMP_CHUNK);
        assert_eq!(read(&mut drained).0.len(), at.get());
        // Dropping the reader stops the pump.
        drop(drained);
        budget.refill();
        assert!(pump.as_mut().poll(&mut cx()).is_ready());
    }
}
