//! [`LspManager`]: starts and stops servers, and carries requests to them.
//!
//! Documents opened with [`LspManager::open`] are routed (§3.2): each goes
//! to its language's servers (registry.rs): the lead, the first enabled
//! server listing it that isn't an add-on, and every add-on listing it
//! (docs/decisions/q5j03ax2af-lsp-add-on-servers.md §3, §4). Each is one
//! instance per (server, root), its root the window root for a file under
//! it and the file's marker root for that server otherwise
//! (`registry::server_root`), started by the first document that needs it
//! and left running when its last document closes. A document is placed on
//! each of its servers apart, keyed by server name: under that server's
//! root's trust, crash budget and launch. A server it can't go to leaves
//! it waiting, parked, there alone, until settings, an extension, a host
//! spec, trust or a restart give it one; its other servers sync it
//! meanwhile.
//!
//! Buffers of one URI whose languages have one lead (aliases) share one
//! *holder*, chosen apart from placement: the first opened, then the next
//! earliest as it closes, and, where groups merge on a settings change, the
//! earliest of their holders. Only the holder is placed, on its own route;
//! the others wait on every server until it closes. A request goes to the
//! servers its feature routes to (§5): a provider feature to the first
//! placed server advertising it ([`LspManager::provider_of`]), a
//! first-that-answers one to every placed server advertising it, and a
//! follow-up on an item to the item's [`Owner`] alone.
//!
//! A server that crashes restarts itself (restart.rs); the manager keeps
//! each (server, root)'s crash list and log ring across the instances it
//! starts there, so a server that gave up stays stopped until
//! [`LspManager::restart`] or a new spec, and its log outlives it.
//!
//! The host says which workspace roots may have servers
//! ([`LspManager::set_trust`]), since a server can run the project's code.
//! A document routed under a root it doesn't trust waits, as one with no
//! server for its language does. [`LspManager::start`] and
//! [`LspManager::open_on`] aren't gated: there the host names the server.

use kiln_l10n::tr;
use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use fe_buffer::{BufferId, Subscription, Version};
use fe_config::settings::LspSettings;
use fe_text::Text;
use futures::channel::{mpsc, oneshot};
use futures::future::{BoxFuture, FutureExt};
use lsp_types::request::Request;
use lsp_types::{ServerCapabilities, Url};

use crate::clock::{Clock, SystemClock};
use crate::document::{Admitted, Opened, Opening, Subscribed, language_id};
use crate::encoding::PositionEncoding;
use crate::event::{
    LspError, LspEvent, ServerId, ServerProgress, ServerStatus, SharedLog, Unavailable,
    WorkProgress, not_open,
};
use crate::identity::{Generation, Identities, IncarnationId, SessionId};
use crate::launcher::{Flag, LaunchCancel, ServerLauncher, ServerSpec};
use crate::registry::{Registry, needs_restart, server_for, server_root, servers_for};
use crate::requests::{Reply, call, timeout, within};
use crate::restart::SharedCrashes;
use crate::server::{
    Admit, Command, Done, LatestStatus, Place, Routes, Server, Session, Sink, WithDocuments,
};

/// A reply, with the id the actor mirrored for its request.
pub(crate) type WithId<T> = (i32, Result<Reply<T>, LspError>);

/// A request's answer, to await anywhere.
pub(crate) type Answer<T> = BoxFuture<'static, Result<T, LspError>>;

/// A reply to a request on a document, with what it was admitted with and
/// the server that answered.
pub(crate) type Provided<T> = (Reply<T>, Admitted, ServerId);

/// A server a request may go to: what its capabilities are read from, and
/// what it's sent commands by.
type Target = (
    ServerId,
    Arc<Mutex<Session>>,
    mpsc::UnboundedSender<Command>,
);

/// A feature one server of a document answers, its *provider*
/// (docs/decisions/q5j03ax2af-lsp-add-on-servers.md §5): the lead if it's
/// placed and advertises the feature, else the first placed add-on that
/// does. Named here so that callers needn't name `lsp_types` capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Feature {
    /// Full semantic tokens (and their deltas).
    SemanticTokens,
    Folding,
    DocumentSymbols,
    LinkedEditing,
    /// Formatting the whole document.
    Formatting,
    /// Formatting a range of it.
    RangeFormatting,
    /// On-type formatting, by `documentOnTypeFormattingProvider` or a
    /// registration taking the document.
    OnTypeFormatting,
    SelectionRange,
}

impl Feature {
    /// Whether `capabilities` advertise the feature.
    pub(crate) fn advertised(self, capabilities: &ServerCapabilities) -> bool {
        match self {
            Self::SemanticTokens => crate::semantic_tokens::provides_full(capabilities),
            Self::Folding => crate::folding_range::provides_folding(capabilities),
            Self::DocumentSymbols => crate::document_symbol::provides_symbols(capabilities),
            Self::LinkedEditing => crate::linked_editing::provided(capabilities),
            Self::Formatting => {
                crate::formatting::offers(capabilities.document_formatting_provider.clone())
            }
            Self::RangeFormatting => {
                crate::formatting::offers(capabilities.document_range_formatting_provider.clone())
            }
            Self::OnTypeFormatting => capabilities.document_on_type_formatting_provider.is_some(),
            Self::SelectionRange => !matches!(
                capabilities.selection_range_provider,
                None | Some(lsp_types::SelectionRangeProviderCapability::Simple(false))
            ),
        }
    }
}

/// The server that produced an item, and the document as that server's
/// session had it open then (§5, owners). A follow-up on the item (a
/// colour's presentations, a link's resolve, a hierarchy item's calls or
/// types, a prepared rename) goes to that session alone: never to another
/// server, nor to a session the server restarted into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Owner {
    pub server: ServerId,
    /// The document the item was asked about.
    pub buffer: BufferId,
    /// The opening of `buffer` on `server` the item came from.
    pub(crate) identity: (SessionId, IncarnationId),
}

impl Owner {
    /// `server`, answering a request admitted as `at`.
    pub(crate) fn of(server: ServerId, at: &Admitted) -> Self {
        Self {
            server,
            buffer: at.document.buffer,
            identity: at.document.identity,
        }
    }

    /// Whether `opened` (what the server has open as a request is sent)
    /// has the owner's document open as it had it.
    pub(crate) fn has_open(&self, opened: &Opened) -> bool {
        (opened.documents.iter())
            .any(|document| document.buffer == self.buffer && document.identity == self.identity)
    }
}

/// Why a follow-up isn't sent: its owner's session has ended.
pub(crate) fn restarted() -> Unavailable {
    Unavailable::Other(tr!("lsp-unavailable-restarted").to_string())
}

/// The capabilities `session` has, once known.
fn known(session: &Mutex<Session>) -> Option<ServerCapabilities> {
    lock(session).capabilities.clone()
}

/// The default [`LspManager::max_file_bytes`]: 50 MB, the size past which
/// fastedit highlights only the viewport.
pub const DEFAULT_MAX_FILE_BYTES: usize = 50 * 1024 * 1024;

/// Runs language servers, one thread each, and reports on them through an
/// event sink.
pub struct LspManager {
    launcher: Arc<dyn ServerLauncher>,
    pub(crate) clock: Arc<dyn Clock>,
    sink: Sink,
    servers: HashMap<ServerId, Handle>,
    /// Every open document: where it's synced, and its subscription hub,
    /// whose tap that server's thread holds.
    documents: HashMap<BufferId, Placed>,
    /// Where routed documents are, and those waiting for a server, shared
    /// with the servers' threads, which hand theirs on when they end.
    routes: Routes,
    /// For each (name, root) whose server was stopped, a signal its thread
    /// has ended, which the next instance there waits for before it
    /// launches.
    retiring: HashMap<(String, PathBuf), Done>,
    /// The servers [`reconcile`](Self::reconcile) restarted since it was
    /// last drained, for a spec that changed.
    respecced: Vec<(String, PathBuf)>,
    registry: Registry,
    /// Which identity of each open buffer is active on each server,
    /// shared with the servers' threads.
    identities: Identities,
    /// The crashes of the servers started at each (name, root).
    crashes: HashMap<(String, PathBuf), SharedCrashes>,
    /// The log ring of the servers started at each (name, root).
    logs: HashMap<(String, PathBuf), SharedLog>,
    /// The latest status of the servers started at each (name, root).
    statuses: HashMap<(String, PathBuf), LatestStatus>,
    /// Each server's open work-done progress, as the sink last heard it.
    progress: Progress,
    max_file_bytes: usize,
    next_id: u64,
    /// The count of routed opens, which orders aliases for their holder.
    next_open: u64,
    /// Whether routed servers may start for a root: see
    /// [`set_trust`](LspManager::set_trust).
    trusted: Trusted,
    /// Thrown by [`KillSwitch::kill`]; shared with every server thread.
    switch: Arc<Flag>,
    /// Shared with the [`KillSwitch`]es.
    threads: Threads,
}

/// Force-stops every server an [`LspManager`] started, from
/// [`LspManager::kill_switch`]. Clones share it, and it outlives the
/// manager, so a host can throw it after dropping the manager (whose drop
/// only asks its servers to shut down) once it has waited long enough.
#[derive(Clone)]
pub struct KillSwitch {
    flag: Arc<Flag>,
    /// The threads of every server the manager started, until seen ended.
    threads: Threads,
}

/// A reply, with the documents the server had open as it was sent.
pub(crate) type WithOpened<T> = (Reply<T>, Opened);

type Threads = Arc<Mutex<Vec<Done>>>;

impl KillSwitch {
    /// For every server the manager started whose thread hasn't ended
    /// (running, launching, waiting to restart, or still shutting down
    /// after a stop or the manager's drop): cancels its launch
    /// ([`LaunchCancel`]) and kills its process at once, without
    /// `shutdown` or `exit`. Each then launches and restarts nothing more,
    /// hands its documents on as a stop does, and reports
    /// `Stopped(None)`. Returns at once, without waiting for any of that.
    ///
    /// Once thrown it stays thrown: a server the manager starts afterwards
    /// launches nothing, and reports `Stopped(None)`.
    pub fn kill(&self) {
        self.flag.fire();
    }

    /// How many of the servers the manager started have threads that
    /// haven't ended: running, launching, waiting to restart, or still
    /// cleaning up after reporting `Stopped` (reaping their process
    /// group). Counts each server once, whatever its status, and still
    /// works after the manager is dropped.
    pub fn unfinished(&self) -> usize {
        let mut threads = lock(&self.threads);
        threads.retain(|done| done.clone().now_or_never().is_none());
        threads.len()
    }
}

impl std::fmt::Debug for KillSwitch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KillSwitch")
            .field("thrown", &self.flag.fired())
            .finish()
    }
}

type Trusted = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

struct Handle {
    name: String,
    root: PathBuf,
    /// What it was started with, or last configured with.
    spec: ServerSpec,
    /// Started by [`LspManager::open`], so settings changes apply to it.
    routed: bool,
    commands: mpsc::UnboundedSender<Command>,
    stop: oneshot::Sender<()>,
    /// Passed to its launches: fired with the stop.
    cancel: LaunchCancel,
    /// Resolves when its thread has ended.
    done: Done,
    session: Arc<Mutex<Session>>,
    thread: JoinHandle<()>,
    /// Whether a routed document has been closed on it alone
    /// ([`LspManager::release`]): its stop then waits for that `didClose`.
    closed: bool,
}

/// How long a retired server that a document was just closed on gets to
/// send that `didClose` before its stop goes ahead anyway, so one that
/// has stopped reading its input can't hold up its shutdown (and a
/// restart waiting for it) for longer. Real time, not the manager's
/// clock: a bound no test clock can hold back.
const CLOSE_GRACE: Duration = Duration::from_secs(2);

impl Handle {
    /// Asks it to stop, cutting a launch in progress short; the next
    /// instance of the same name and root waits for its thread to end.
    fn stop(self, retiring: &mut HashMap<(String, PathBuf), Done>) {
        let _ = self.stop.send(());
        self.cancel.cancel();
        retiring.insert((self.name, self.root), self.done);
    }

    /// As [`stop`](Self::stop), except that one a document was closed on
    /// ([`closed`](Self::closed)) stops only once its thread has carried
    /// out the commands queued before now, so that `didClose` goes ahead
    /// of `shutdown` (§4: a removed add-on is told), or after
    /// [`CLOSE_GRACE`], whichever is first. A launch in progress is still
    /// cut short at once.
    fn retire(self, retiring: &mut HashMap<(String, PathBuf), Done>) {
        if !self.closed {
            return self.stop(retiring);
        }
        self.cancel.cancel();
        retiring.insert((self.name.clone(), self.root.clone()), self.done);
        let stop = Arc::new(Mutex::new(Some(self.stop)));
        // Run behind the queued commands, or dropped unrun (a server
        // between sessions drops it, as does a closed channel): the stop
        // goes either way.
        let queued = StopOnDrop(stop.clone());
        let behind = Command::Documents(Box::new(move |_| drop(queued)));
        let _ = self.commands.unbounded_send(behind);
        let grace = StopOnDrop(stop);
        // With no thread, the grace drops at once, and the stop goes now.
        let _ = (std::thread::Builder::new())
            .name(format!("fe-lsp {} close grace", self.name))
            .spawn(move || {
                std::thread::sleep(CLOSE_GRACE);
                drop(grace);
            });
    }
}

/// Sends a server's stop once the first of its holders drops.
struct StopOnDrop(Arc<Mutex<Option<oneshot::Sender<()>>>>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        if let Some(stop) = lock(&self.0).take() {
            let _ = stop.send(());
        }
    }
}

/// An open document.
struct Placed {
    subscribed: Subscribed,
    kind: Kind,
    /// Its URI and language id, as its server has them: what a
    /// registration's document selector is matched against.
    filtered_as: (Url, String),
}

enum Kind {
    /// Opened on a server of the host's choosing ([`LspManager::open_on`]).
    On(ServerId),
    /// Routed by [`LspManager::open`]: where it is is in `routes`.
    Routed(Box<Route>),
}

struct Route {
    /// Its servers, the lead first, then its language's add-ons in order,
    /// each with where it runs: [`server_root`] of `path`, `window` and
    /// that server's markers. Kept up to date by
    /// [`LspManager::reconcile`]; one whose language no server lists keeps
    /// the ones it had, and waits on each.
    roots: Vec<(String, PathBuf)>,
    /// Its language's configured lead (§3), placed or not, while a server
    /// lists the language. Buffers of one URI with one lead share a holder.
    lead: Option<String>,
    /// Whether the lead is a primary rather than the first add-on.
    primary: bool,
    /// The window root it was opened under, if any.
    window: Option<PathBuf>,
    path: PathBuf,
    /// The document's URI: a server has one document per URI.
    uri: Url,
    /// fe-syntax's name.
    language: String,
    /// When it was opened, by the manager's count, which picks a group's
    /// next holder.
    opened: u64,
    /// Whether it holds its path for its lead (§4): one buffer of each
    /// (URI, lead) does, chosen apart from placement. Only the holder is
    /// placed; the others wait on every server.
    holder: bool,
    /// The open generation its openings carry.
    generation: Generation,
    /// The size limit it was opened with.
    max_bytes: usize,
}

impl Route {
    /// Sets its servers to those `specs` give its language, unless none
    /// does: then it keeps its roots and has no lead. Its lead stays
    /// `keep`, if given and still a primary listing the language, rather
    /// than one that would now come first: a document stays on a lead that
    /// keeps running (§4). Returns the names it had and the names it has
    /// now.
    fn reroot(&mut self, specs: &[ServerSpec], keep: Option<&str>) -> (Vec<String>, Vec<String>) {
        let before = self.names();
        let mut servers = servers_for(specs, &self.language);
        if let Some(kept) = keep.and_then(|keep| {
            (specs.iter()).find(|spec| {
                spec.name == keep && !spec.add_on && spec.languages.contains(&self.language)
            })
        }) {
            servers.primary = Some(kept);
        }
        self.primary = servers.primary.is_some();
        let configured: Vec<&ServerSpec> =
            servers.primary.into_iter().chain(servers.add_ons).collect();
        if configured.is_empty() {
            self.lead = None;
            return (before.clone(), before);
        }
        self.roots = configured
            .iter()
            .map(|spec| {
                let root = server_root(&self.path, self.window.as_deref(), &spec.root_markers);
                (spec.name.clone(), root)
            })
            .collect();
        self.lead = Some(self.roots[0].0.clone());
        (before, self.names())
    }

    fn names(&self) -> Vec<String> {
        self.roots.iter().map(|(name, _)| name.clone()).collect()
    }

    fn root_of(&self, name: &str) -> Option<&Path> {
        (self.roots.iter())
            .find(|(server, _)| server == name)
            .map(|(_, root)| root.as_path())
    }
}

/// One of a document's configured servers ([`LspManager::route_servers`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteServer {
    pub name: String,
    /// Where it runs for the document.
    pub root: PathBuf,
    /// Whether its spec is an add-on.
    pub add_on: bool,
    /// The instance the document is placed on now, if any.
    pub placed: Option<ServerId>,
}

impl LspManager {
    /// `sink` gets every [`LspEvent`], on the servers' threads.
    pub fn new(
        launcher: Arc<dyn ServerLauncher>,
        sink: impl FnMut(LspEvent) + Send + 'static,
    ) -> Self {
        Self::with_clock(launcher, Arc::new(SystemClock), sink)
    }

    /// As [`new`](Self::new), with every timeout measured on `clock`.
    pub fn with_clock(
        launcher: Arc<dyn ServerLauncher>,
        clock: Arc<dyn Clock>,
        sink: impl FnMut(LspEvent) + Send + 'static,
    ) -> Self {
        let progress = Progress::default();
        Self {
            launcher,
            clock,
            sink: Arc::new(Mutex::new(progress.tap(sink))),
            progress,
            servers: HashMap::new(),
            documents: HashMap::new(),
            routes: Routes::default(),
            retiring: HashMap::new(),
            respecced: Vec::new(),
            registry: Registry::default(),
            identities: Identities::default(),
            crashes: HashMap::new(),
            logs: HashMap::new(),
            statuses: HashMap::new(),
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            next_id: 0,
            next_open: 0,
            trusted: Arc::new(|_| true),
            switch: Arc::default(),
            threads: Arc::default(),
        }
    }

    /// The switch that force-stops every server this manager starts, for a
    /// host that can't wait for them to shut down (a quit's deadline, say).
    pub fn kill_switch(&self) -> KillSwitch {
        KillSwitch {
            flag: self.switch.clone(),
            threads: self.threads.clone(),
        }
    }

    /// Sets the host's answer to whether servers may start for a workspace
    /// root, since a server can run the project's code (rust-analyzer runs
    /// build scripts and proc macros). Every root is trusted until this is
    /// called. Applies as [`update_settings`](Self::update_settings) does:
    ///
    /// - a document [`open`](Self::open) routes under a root `trusted`
    ///   rejects waits, as one with no server for its language does;
    /// - each routed server running for a root it now rejects is shut down,
    ///   and its documents wait; nothing starts one there again (settings,
    ///   specs, a crash restart, [`restart`](Self::restart)) until the root
    ///   is trusted;
    /// - documents waiting under a root it now trusts get their servers,
    ///   which open them at the buffers' current text.
    ///
    /// [`start`](Self::start) and [`open_on`](Self::open_on) aren't gated:
    /// there the host names the server. So an instance the host started
    /// keeps running, with any routed document `open` placed on it while
    /// its root was trusted.
    pub fn set_trust(&mut self, trusted: impl Fn(&Path) -> bool + Send + Sync + 'static) {
        self.trusted = Arc::new(trusted);
        self.reconcile();
    }

    /// Applies the effective `[lsp]` settings (§3.3), whose project layer
    /// fe-config has narrowed. For each server [`open`](Self::open)
    /// started:
    ///
    /// - turned off (or `lsp.enabled` false, or the other TypeScript server
    ///   chosen): it's stopped;
    /// - a change to anything but its `settings` (`command`, `args`, `env`,
    ///   `languages`, `readiness`, `initialization_options` …): it's shut
    ///   down and launched again;
    /// - a change to its `settings` only: it answers
    ///   `workspace/configuration` with them from now on, and is sent
    ///   `workspace/didChangeConfiguration`.
    ///
    /// A stopped or restarted server's documents go to the instance of it
    /// they'd get if opened now, at the text it last drained, or wait for
    /// one; their other servers keep them. Documents waiting get a server
    /// if one now applies. A document stays on a lead that keeps running,
    /// even if another would now come first for its language; an add-on
    /// it no longer has closes it (`didClose`, before that server stops if
    /// it does), and one it gains opens it at its current text, as does a
    /// server whose root for it moved (its markers changed), after closing
    /// it at the old root. Buffers of one path whose languages now share a lead keep the
    /// earlier holder: the other is closed on each server it was on, and
    /// waits. `max_file_bytes` applies to documents opened from now on.
    ///
    /// A server that stopped on its own (a launch error such as a missing
    /// command, or a crash) hands its documents back to wait, and they get
    /// a server again here, or when another document starts theirs.
    /// A request on a document still on its way to its new server answers
    /// `Unavailable("not open on a language server")`, as for one not yet
    /// opened.
    pub fn update_settings(&mut self, settings: &LspSettings) {
        self.max_file_bytes = usize::try_from(settings.max_file_bytes).unwrap_or(usize::MAX);
        if self.registry.settings() != settings {
            self.registry.set_settings(settings.clone());
        }
        self.reconcile();
    }

    /// Sets the specs extensions contribute (in extension order), a tier
    /// over the built-ins: the `[lsp]` settings apply over each as over a
    /// built-in, and may turn it off. One named like a built-in or a host
    /// spec is ignored, with a warning. Applies as
    /// [`update_settings`](Self::update_settings) does: only servers whose
    /// spec changed restart, and none for a change to its `settings` alone.
    /// Returns the name and root of each server it restarted, which a
    /// caller mustn't restart again.
    pub fn set_extension_specs(&mut self, specs: Vec<ServerSpec>) -> Vec<(String, PathBuf)> {
        self.respecced.clear();
        if self.registry.set_extension_specs(specs) {
            self.reconcile();
        }
        std::mem::take(&mut self.respecced)
    }

    /// Registers a host's `spec`, which wins over a built-in, extension or
    /// settings spec of the same name; settings may still turn it off.
    /// Applies as [`update_settings`](Self::update_settings) does.
    pub fn register_spec(&mut self, spec: ServerSpec) {
        self.registry.register(spec);
        self.reconcile();
    }

    /// `buffer`'s lead server, if it's placed there:
    /// [`lead_of`](Self::lead_of), which it stays an alias of until its
    /// last caller moves. Requests a feature routes to other servers (§5)
    /// don't go by it.
    pub fn server_of(&self, buffer: BufferId) -> Option<ServerId> {
        self.lead_of(buffer)
    }

    /// The server of `buffer`'s configured lead (§3), if the document is
    /// placed on it; never an add-on in its place, whatever runs. One
    /// opened with [`open_on`](Self::open_on): the server it's on.
    pub fn lead_of(&self, buffer: BufferId) -> Option<ServerId> {
        match &self.documents.get(&buffer)?.kind {
            Kind::On(server) => Some(*server),
            Kind::Routed(route) => self.placed_on(buffer, route.lead.as_deref()?),
        }
    }

    /// The servers `buffer` is placed on now, in its route's order: the
    /// lead first when it's placed, then its add-ons.
    pub fn servers_of(&self, buffer: BufferId) -> Vec<ServerId> {
        match self.documents.get(&buffer).map(|placed| &placed.kind) {
            Some(Kind::On(server)) => vec![*server],
            Some(Kind::Routed(_)) => (self.placements(buffer).into_iter())
                .map(|(_, server)| server)
                .collect(),
            None => Vec::new(),
        }
    }

    /// Whether `buffer` is placed on any server: for a routed document,
    /// whether it holds its path for its lead (§4).
    pub fn is_served(&self, buffer: BufferId) -> bool {
        !self.servers_of(buffer).is_empty()
    }

    /// The active identity of `buffer` on each server it's placed on, in
    /// [`servers_of`](Self::servers_of)'s order: a slot can tell a restart
    /// or reopen of any one of them. A server yet to open it is left out.
    pub fn served_identities(&self, buffer: BufferId) -> Vec<(ServerId, SessionId, IncarnationId)> {
        (self.servers_of(buffer).into_iter())
            .filter_map(|server| {
                let (session, incarnation) = self.active_identity(buffer, server)?;
                Some((server, session, incarnation))
            })
            .collect()
    }

    /// `buffer`'s configured servers: the lead first, placed or not, then
    /// its add-ons, each by name and root with the instance it's placed on.
    /// Configured identity, apart from live ids, for status, restart and the
    /// log. Empty for a document no server lists; one opened with
    /// [`open_on`](Self::open_on) has the server it's on.
    pub fn route_servers(&self, buffer: BufferId) -> Vec<RouteServer> {
        let Some(placed) = self.documents.get(&buffer) else {
            return Vec::new();
        };
        let route = match &placed.kind {
            Kind::On(server) => {
                return (self.servers.get(server).into_iter())
                    .map(|handle| RouteServer {
                        name: handle.name.clone(),
                        root: handle.root.clone(),
                        add_on: handle.spec.add_on,
                        placed: Some(*server),
                    })
                    .collect();
            }
            Kind::Routed(route) if route.lead.is_some() => route,
            Kind::Routed(_) => return Vec::new(),
        };
        let routes = self.routes.lock();
        (route.roots.iter().enumerate())
            .map(|(at, (name, root))| RouteServer {
                name: name.clone(),
                root: root.clone(),
                add_on: at > 0 || !route.primary,
                placed: match routes.places.get(&(buffer, name.clone())) {
                    Some(Place::Server(server, _)) => Some(*server),
                    _ => None,
                },
            })
            .collect()
    }

    /// The name of the lead server `language` is routed to under the
    /// current settings, trusted root or not: the server a document of it
    /// opened now would wait for or go to first.
    pub fn route_for(&self, language: &str) -> Option<String> {
        let specs = self.registry.specs();
        Some(server_for(&specs, language)?.name.clone())
    }

    /// The name of `buffer`'s lead: its language's configured lead, placed
    /// or waiting (under an untrusted root, or for a server). One opened
    /// with [`open_on`](Self::open_on): the server it's on.
    pub fn route_of(&self, buffer: BufferId) -> Option<String> {
        match &self.documents.get(&buffer)?.kind {
            Kind::On(server) => self.servers.get(server).map(|handle| handle.name.clone()),
            Kind::Routed(route) => route.lead.clone(),
        }
    }

    /// The routed document `buffer`'s route.
    fn route(&self, buffer: BufferId) -> Option<&Route> {
        match &self.documents.get(&buffer)?.kind {
            Kind::Routed(route) => Some(route),
            Kind::On(_) => None,
        }
    }

    /// The instance routed `buffer` is placed on for server `name`, if
    /// any.
    fn placed_on(&self, buffer: BufferId, name: &str) -> Option<ServerId> {
        match self.routes.lock().places.get(&(buffer, name.to_owned()))? {
            Place::Server(server, _) => Some(*server),
            Place::Waiting => None,
        }
    }

    /// Each server routed `buffer` is placed on, by name, in its route's
    /// order.
    fn placements(&self, buffer: BufferId) -> Vec<(String, ServerId)> {
        let Some(route) = self.route(buffer) else {
            return Vec::new();
        };
        let routes = self.routes.lock();
        (route.roots.iter())
            .filter_map(
                |(name, _)| match routes.places.get(&(buffer, name.clone())) {
                    Some(Place::Server(server, _)) => Some((name.clone(), *server)),
                    _ => None,
                },
            )
            .collect()
    }

    /// The largest text, in bytes, a document may have and be synced
    /// (§4.1): one over it isn't opened on its server, and one that grows
    /// past it is closed there until it's back under. Requests on it
    /// answer `Unavailable("too large for the language server")`.
    /// [`DEFAULT_MAX_FILE_BYTES`] unless set.
    pub fn max_file_bytes(&self) -> usize {
        self.max_file_bytes
    }

    /// Sets [`max_file_bytes`](Self::max_file_bytes) for documents opened
    /// from now on.
    pub fn set_max_file_bytes(&mut self, bytes: usize) {
        self.max_file_bytes = bytes;
    }

    /// Starts `spec`'s server for the workspace at `root`, an absolute
    /// path, on a thread of its own, unless one with the same name is
    /// already running there. The launcher runs on that thread, and the
    /// sink hears how the start went. Errs only if the thread can't be
    /// made.
    ///
    /// It starts even a server that stopped after too many crashes, but
    /// with the crashes counted so far: only [`restart`](Self::restart)
    /// clears them. It isn't gated by [`set_trust`](Self::set_trust): the
    /// host names the server.
    pub fn start(&mut self, spec: ServerSpec, root: &Path) -> io::Result<ServerId> {
        self.start_server(spec, root, false)
    }

    /// Forgets servers that have gone. Their routed documents wait for a
    /// server (the server hands them back), and the others go with them.
    /// Only servers found gone here count: one retired a moment ago (no
    /// longer listed, still handing its documents on) doesn't.
    fn prune(&mut self) {
        // A thread whose end has been seen needn't be waited for.
        self.retiring
            .retain(|_, done| done.clone().now_or_never().is_none());
        let mut gone = Vec::new();
        let dead: Vec<ServerId> = self
            .servers
            .iter()
            .filter(|(_, handle)| handle.commands.is_closed() || handle.thread.is_finished())
            .map(|(&id, _)| id)
            .collect();
        for id in dead {
            let handle = self.servers.remove(&id).expect("listed above");
            // Its thread may still be ending (its process being killed): a
            // new instance there waits for it.
            self.retiring
                .insert((handle.name, handle.root), handle.done);
            gone.push(id);
        }
        if gone.is_empty() {
            return;
        }
        let identities = &self.identities;
        self.documents.retain(|&buffer, placed| match placed.kind {
            Kind::On(server) => {
                let live = !gone.contains(&server);
                if !live {
                    identities.close(buffer);
                }
                live
            }
            Kind::Routed(_) => true,
        });
        // Only the (document, server) places on those wait: the documents'
        // other servers keep them.
        for place in self.routes.lock().places.values_mut() {
            if let Place::Server(server, _) = place
                && gone.contains(server)
            {
                *place = Place::Waiting;
            }
        }
    }

    fn start_server(
        &mut self,
        spec: ServerSpec,
        root: &Path,
        routed: bool,
    ) -> io::Result<ServerId> {
        self.prune();
        if let Some((&id, _)) = self
            .servers
            .iter()
            .find(|(_, handle)| handle.name == spec.name && handle.root == root)
        {
            return Ok(id);
        }
        let id = ServerId(self.next_id);
        self.next_id += 1;
        let (commands, receiver) = mpsc::unbounded();
        let (stop, stop_requested) = oneshot::channel();
        let (done_sender, done) = oneshot::channel();
        let key = (spec.name.clone(), root.to_owned());
        // Removed once the thread is made: if it can't be, the next try
        // still waits.
        let after = self.retiring.get(&key).cloned();
        let session = Arc::new(Mutex::new(Session::default()));
        let name = spec.name.clone();
        let crashes = self.crashes.entry(key.clone()).or_default().clone();
        let epoch = {
            let mut crashes = lock(&crashes);
            crashes.last = Some((spec.clone(), routed));
            crashes.epoch
        };
        let log = self.logs.entry(key.clone()).or_default().clone();
        let cancel = LaunchCancel::linked(self.switch.clone());
        let latest = self.statuses.entry(key.clone()).or_default().clone();
        // As its thread will report first: an earlier instance's status
        // no longer stands for the server there, unless no thread is made.
        let before = lock(&latest).clone();
        crate::server::latest(&latest, id, &ServerStatus::Starting);
        let unmade = latest.clone();
        let thread = Server {
            id,
            spec: spec.clone(),
            root: root.to_owned(),
            launcher: self.launcher.clone(),
            clock: self.clock.clone(),
            sink: self.sink.clone(),
            session: session.clone(),
            session_id: self.identities.new_session(),
            identities: self.identities.clone(),
            commands: receiver,
            stop: stop_requested,
            cancel: cancel.clone(),
            switch: self.switch.clone(),
            routes: self.routes.named(&spec.name),
            after,
            done: Some(done_sender),
            crashes,
            epoch,
            log,
            latest,
        }
        .spawn()
        .inspect_err(|_| *lock(&unmade) = before)?;
        self.retiring.remove(&key);
        let done = done.shared();
        {
            let mut threads = lock(&self.threads);
            threads.retain(|done| done.clone().now_or_never().is_none());
            threads.push(done.clone());
        }
        self.servers.insert(
            id,
            Handle {
                name,
                root: root.to_owned(),
                spec,
                routed,
                commands,
                stop,
                cancel,
                done,
                session,
                thread,
                closed: false,
            },
        );
        Ok(id)
    }

    /// Shuts `server` down (§3.4) on its own thread, kills it if it hasn't
    /// answered `initialize`, cancels its launch in progress
    /// ([`LaunchCancel`]), or ends its wait to restart; the sink hears
    /// `Stopped(None)` when it has gone. Requests still waiting get
    /// [`LspError::ServerDown`]. Its documents go with it, routed ones too:
    /// they're no longer tracked, as if closed (each is closed on its other
    /// servers), and a host wanting them synced again opens them again.
    /// (One that a crash or a settings change leaves without a server waits
    /// for one instead.)
    pub fn stop(&mut self, server: ServerId) {
        // Its documents go with it.
        let gone: Vec<BufferId> = self
            .documents
            .keys()
            .copied()
            .filter(|&buffer| self.servers_of(buffer).contains(&server))
            .collect();
        for buffer in gone {
            for other in self.forget(buffer) {
                if other != server {
                    self.send(other, Command::Close(buffer));
                }
            }
        }
        if let Some(handle) = self.servers.remove(&server) {
            handle.stop(&mut self.retiring);
        }
    }

    /// Restarts the server `name` for the workspace at `root`
    /// (`lsp.restart`): forgets its crashes, shuts down the instance
    /// running there if there is one, and starts it again, even from
    /// `Stopped` after too many crashes. A server [`open`](Self::open)
    /// routes documents to starts with its spec as settings and host specs
    /// give it now, and its documents go to the new instance; one started
    /// with [`start`](Self::start) starts with the spec it last ran with,
    /// and documents opened on it go with the old instance, as with
    /// [`stop`](Self::stop). Returns the new instance, or `None` if
    /// there's no such server to start, or if it's one `open` routes to
    /// and [`set_trust`](Self::set_trust) rejects `root`: its crashes are
    /// still forgotten, so it starts once `root` is trusted. Errs only if
    /// its thread can't be made.
    pub fn restart(&mut self, name: &str, root: &Path) -> io::Result<Option<ServerId>> {
        self.prune();
        let key = (name.to_owned(), root.to_owned());
        let last = self.crashes.get(&key).and_then(|crashes| {
            let mut crashes = lock(crashes);
            crashes.clear();
            crashes.last.clone()
        });
        let specs = self.registry.specs();
        let configured = specs.iter().find(|spec| spec.name == name).cloned();
        let running = self
            .servers
            .iter()
            .find(|(_, handle)| handle.name == name && handle.root == root)
            .map(|(&id, handle)| (id, handle.routed, handle.spec.clone()));
        let (spec, routed) = match running {
            Some((id, true, _)) => {
                self.retire(id, &specs);
                self.regroup();
                match configured {
                    Some(spec) => (spec, true),
                    None => return Ok(None),
                }
            }
            Some((id, false, spec)) => {
                self.stop(id);
                (spec, false)
            }
            None => match (last, configured) {
                (Some((_, true)) | None, Some(spec)) => (spec, true),
                (Some((spec, false)), _) => (spec, false),
                (_, None) => return Ok(None),
            },
        };
        if routed && !(self.trusted)(root) {
            return Ok(None);
        }
        let id = self.start_server(spec, root, routed)?;
        if routed {
            self.place_waiting(&specs, Some(&key));
        }
        Ok(Some(id))
    }

    /// The last 64 KiB of what the server `name` for the workspace at
    /// `root` wrote to stderr or sent as `window/logMessage`, across its
    /// sessions and instances (`lsp.show_log`).
    pub fn log(&self, name: &str, root: &Path) -> String {
        self.logs
            .get(&(name.to_owned(), root.to_owned()))
            .map(|log| lock(log).text())
            .unwrap_or_default()
    }

    /// Stops tracking `buffer`: retires its identities, drops its
    /// subscription, its places and any opening waiting for it. Returns the
    /// servers it was synced to.
    fn forget(&mut self, buffer: BufferId) -> Vec<ServerId> {
        let servers = self.servers_of(buffer);
        let Some(placed) = self.documents.remove(&buffer) else {
            return servers;
        };
        self.identities.close(buffer);
        placed.subscribed.detach();
        let mut names = match placed.kind {
            Kind::Routed(route) => route.names(),
            Kind::On(_) => Vec::new(),
        };
        // An opening for no server, if it has one.
        names.push(String::new());
        let mut routes = self.routes.lock();
        let parked: Vec<Opening> = (names.into_iter())
            .filter_map(|name| {
                let key = (buffer, name);
                routes.places.remove(&key);
                routes.lot.remove(&key)
            })
            .collect();
        drop(routes);
        // Its subscription's hub is locked as each drops: not under the
        // places' lock.
        drop(parked);
        servers
    }

    /// Sends `R` to `server`. A request made while the server starts is
    /// sent once it has initialized; one made while it waits to restart,
    /// or in flight when it crashes, answers [`LspError::ServerDown`]. The
    /// future may be awaited anywhere.
    ///
    /// Dropping the future cancels the request (`$/cancelRequest`), or,
    /// if it hasn't been sent yet, keeps it from being sent. One not
    /// answered within its method's `lsp.timeouts` of the call is
    /// cancelled too, and answers [`LspError::Timeout`].
    pub fn request<R: Request>(
        &self,
        server: ServerId,
        params: R::Params,
    ) -> impl Future<Output = Result<Reply<R::Result>, LspError>> + Send + 'static {
        self.request_with_id::<R>(server, params)
            .map(|answer| answer.and_then(|(_, result)| result))
    }

    /// As [`request`](Self::request), with the id the actor mirrored for
    /// the request.
    pub(crate) fn request_with_id<R: Request>(
        &self,
        server: ServerId,
        params: R::Params,
    ) -> impl Future<Output = Result<WithId<R::Result>, LspError>> + Send + 'static {
        let (answer, answered) = oneshot::channel();
        let issue = call::<R, _>(answer, params, |id, reply| (id, reply));
        if let Some(handle) = self.servers.get(&server) {
            // A server that has gone drops the request, and with it `answer`.
            let _ = handle.commands.unbounded_send(Command::Request(issue));
        }
        let answered =
            answered.map(|answer| answer.map_err(|oneshot::Canceled| LspError::ServerDown));
        within(
            answered,
            self.timeout::<R>(),
            &*self.clock,
            Err(LspError::Timeout),
        )
    }

    /// How long `R` may take, from `lsp.timeouts`.
    fn timeout<R: Request>(&self) -> Option<std::time::Duration> {
        timeout(R::METHOD, &self.registry.settings().timeouts)
    }

    /// Sends `R` about open buffer `buffer`, computed on its `version`
    /// (§4.1), to its lead. The server's thread brings the document up to
    /// date first,
    /// then:
    ///
    /// - if the server now has `version`, builds the request's params with
    ///   `params` and sends it, answering with the reply and what it was
    ///   [`Admitted`] with, to decode the reply against;
    /// - if the buffer has moved on, answers
    ///   `Unavailable("buffer changed")`: the request isn't rebased;
    /// - if the document is over [`max_file_bytes`](Self::max_file_bytes),
    ///   `Unavailable("too large for the language server")`.
    ///
    /// Cancelled and timed out as [`request`](Self::request) is. The future
    /// may be awaited anywhere.
    pub fn document_request<R: Request>(
        &self,
        buffer: BufferId,
        version: Version,
        params: impl FnOnce(&Admitted) -> R::Params + Send + 'static,
    ) -> impl Future<Output = Result<(Reply<R::Result>, Admitted), LspError>> + Send + 'static {
        self.document_request_to::<R>(self.server_of(buffer), buffer, version, params)
    }

    /// As [`document_request`](Self::document_request), aimed at `server`
    /// alone (one `buffer` is placed on), whatever it advertises.
    pub(crate) fn document_request_on<R: Request>(
        &self,
        server: ServerId,
        buffer: BufferId,
        version: Version,
        params: impl FnOnce(&Admitted) -> R::Params + Send + 'static,
    ) -> impl Future<Output = Result<(Reply<R::Result>, Admitted), LspError>> + Send + 'static {
        self.document_request_to::<R>(Some(server), buffer, version, params)
    }

    /// `R` about `buffer`, to `server` (none: `Unavailable(not open)`).
    fn document_request_to<R: Request>(
        &self,
        server: Option<ServerId>,
        buffer: BufferId,
        version: Version,
        params: impl FnOnce(&Admitted) -> R::Params + Send + 'static,
    ) -> impl Future<Output = Result<(Reply<R::Result>, Admitted), LspError>> + Send + 'static {
        let (answer, answered) = oneshot::channel();
        let admit: Admit = Box::new(move |admission| {
            let admitted = match admission {
                Ok(admitted) => admitted,
                Err(err) => {
                    let _ = answer.send(Err(err));
                    return Vec::new();
                }
            };
            let params = params(&admitted);
            vec![call::<R, _>(answer, params, move |_, reply| {
                reply.map(|reply| (reply, admitted))
            })]
        });
        self.admit_on(server, buffer, version, admit);
        let answered = answered.map(|answer| answer.unwrap_or(Err(LspError::ServerDown)));
        within(
            answered,
            self.timeout::<R>(),
            &*self.clock,
            Err(LspError::Timeout),
        )
    }

    /// Hands `admit` to `server`, or answers it `Unavailable` if none.
    fn admit_on(&self, server: Option<ServerId>, buffer: BufferId, version: Version, admit: Admit) {
        match server {
            Some(server) => self.send(
                server,
                Command::Admit {
                    buffer,
                    version,
                    admit,
                },
            ),
            None => {
                admit(Err(LspError::Unavailable(not_open())));
            }
        }
    }

    /// Sends `R` to `server`, as [`request`](Self::request) does, with
    /// the documents the server has open as it's sent ([`Opened`]),
    /// brought up to date first: to decode a reply naming open documents
    /// (workspace symbols, say) against the text the server had.
    pub fn request_with_documents<R: Request>(
        &self,
        server: ServerId,
        params: R::Params,
    ) -> impl Future<Output = Result<(Reply<R::Result>, Opened), LspError>> + Send + 'static {
        let (answer, answered) = oneshot::channel();
        let admit: WithDocuments = Box::new(move |opened| {
            let opened = match opened {
                Ok(opened) => opened,
                Err(err) => {
                    let _ = answer.send(Err(err));
                    return None;
                }
            };
            Some(call::<R, _>(answer, params, move |_, reply| {
                reply.map(|reply| (reply, opened))
            }))
        });
        // A server that has gone drops the command, and `answer` with it.
        self.send(server, Command::WithDocuments(admit));
        let answered = answered.map(|answer| answer.unwrap_or(Err(LspError::ServerDown)));
        within(
            answered,
            self.timeout::<R>(),
            &*self.clock,
            Err(LspError::Timeout),
        )
    }

    /// As [`request_with_documents`](Self::request_with_documents), but
    /// only if `send` agrees, given the documents the server has open as
    /// it's sent: `Ok(None)` if it doesn't.
    pub(crate) fn request_with_documents_if<R: Request>(
        &self,
        server: ServerId,
        params: R::Params,
        send: impl FnOnce(&Opened) -> bool + Send + 'static,
    ) -> impl Future<Output = Result<Option<WithOpened<R::Result>>, LspError>> + Send + 'static
    {
        let (answer, answered) = oneshot::channel();
        let admit: WithDocuments = Box::new(move |opened| match opened {
            Ok(opened) if send(&opened) => Some(call::<R, _>(answer, params, move |_, reply| {
                reply.map(|reply| Some((reply, opened)))
            })),
            Ok(_) => {
                let _ = answer.send(Ok(None));
                None
            }
            Err(err) => {
                let _ = answer.send(Err(err));
                None
            }
        });
        // A server that has gone drops the command, and `answer` with it.
        self.send(server, Command::WithDocuments(admit));
        let answered = answered.map(|answer| answer.unwrap_or(Err(LspError::ServerDown)));
        within(
            answered,
            self.timeout::<R>(),
            &*self.clock,
            Err(LspError::Timeout),
        )
    }

    /// Sends `R` to `server` as [`request`](Self::request) does, but only
    /// if `send` agrees, given the documents the server has open as it's
    /// sent ([`Opened`]): `Ok(None)` if it doesn't. To keep a request to a
    /// session that may have ended (an item it gave, say) from reaching
    /// its successor.
    pub(crate) fn request_if<R: Request>(
        &self,
        server: ServerId,
        params: R::Params,
        send: impl FnOnce(&Opened) -> bool + Send + 'static,
    ) -> impl Future<Output = Result<Option<Reply<R::Result>>, LspError>> + Send + 'static {
        let (answer, answered) = oneshot::channel();
        let admit: WithDocuments = Box::new(move |opened| match opened {
            Ok(opened) if send(&opened) => {
                Some(call::<R, _>(answer, params, |_, reply| reply.map(Some)))
            }
            Ok(_) => {
                let _ = answer.send(Ok(None));
                None
            }
            Err(err) => {
                let _ = answer.send(Err(err));
                None
            }
        });
        // A server that has gone drops the command, and `answer` with it.
        self.send(server, Command::WithDocuments(admit));
        let answered = answered.map(|answer| answer.unwrap_or(Err(LspError::ServerDown)));
        within(
            answered,
            self.timeout::<R>(),
            &*self.clock,
            Err(LspError::Timeout),
        )
    }

    /// The lead server documents of fe-syntax's `language` under `root` go
    /// to now, by name, with the latest status of one started there
    /// (`None` if none has been); `None` if they'd go to none: no server
    /// lists the language, `[lsp]` turns it off, or `root` isn't trusted.
    /// The first of [`language_servers`](Self::language_servers), running
    /// or not, without its add-on flag.
    pub fn language_server(
        &self,
        root: &Path,
        language: &str,
    ) -> Option<(String, Option<ServerStatus>)> {
        let (name, _, status) = self.language_servers(root, language).into_iter().next()?;
        Some((name, status))
    }

    /// Every server documents of fe-syntax's `language` under `root` go to
    /// now, the lead first, then its add-ons, each by name, with whether
    /// it's an add-on and the latest status of one started there (`None` if
    /// none has been). Empty if they'd go to none: no server lists the
    /// language, `[lsp]` turns it off, or `root` isn't trusted.
    pub fn language_servers(
        &self,
        root: &Path,
        language: &str,
    ) -> Vec<(String, bool, Option<ServerStatus>)> {
        if !(self.trusted)(root) {
            return Vec::new();
        }
        let specs = self.registry.specs();
        let servers = servers_for(&specs, language);
        (servers.primary.into_iter().chain(servers.add_ons))
            .map(|spec| {
                let status = self.status_at(&spec.name, root);
                (spec.name.clone(), spec.add_on, status)
            })
            .collect()
    }

    /// The latest status of the server started at (`name`, `root`), or
    /// `None` if none ever started there: a missing command reports
    /// `Stopped(Some(…))`, and an untrusted root has none.
    pub fn status_at(&self, name: &str, root: &Path) -> Option<ServerStatus> {
        let latest = self.statuses.get(&(name.to_owned(), root.to_owned()))?;
        lock(latest).as_ref().map(|(_, status)| status.clone())
    }

    /// The running servers' open work-done progress (`$/progress`), oldest
    /// server first, each server's oldest begun first: as of the latest
    /// [`LspEvent::Progress`] each sent, which the sink hears after it's
    /// here. A server with none open isn't listed, nor one whose session
    /// ended (its `Status` went `Restarting` or `Stopped`).
    pub fn progress(&self) -> Vec<ServerProgress> {
        let progress = lock(&self.progress.0);
        let mut listed: Vec<ServerProgress> = (progress.iter())
            .filter_map(|(&server, (name, work))| {
                let handle = self.servers.get(&server)?;
                Some(ServerProgress {
                    server,
                    name: name.clone(),
                    root: handle.root.clone(),
                    work: work.clone(),
                })
            })
            .collect();
        listed.sort_by_key(|progress| progress.server);
        listed
    }

    /// The servers running for `root`, oldest first, each with its latest
    /// status.
    pub(crate) fn servers_at(&self, root: &Path) -> Vec<(ServerId, ServerStatus)> {
        let mut servers: Vec<(ServerId, ServerStatus)> = (self.servers.iter())
            .filter(|(_, handle)| handle.root == root)
            .filter_map(|(&id, handle)| {
                let key = (handle.name.clone(), handle.root.clone());
                let latest = lock(self.statuses.get(&key)?).clone();
                latest.filter(|(newest, _)| *newest == id)
            })
            .collect();
        servers.sort_by_key(|(id, _)| *id);
        servers
    }

    /// Starts syncing buffer `buffer`, at `path`, to the servers of
    /// fe-syntax's `language`, its lead and its add-ons (§3), each at the
    /// folder [`server_root`] gives it from the window root `root` and that
    /// server's root markers (§3.2), starting each that isn't running
    /// there, and returns the instance its lead was placed on, if it was
    /// ([`lead_of`](Self::lead_of) once it's there). As
    /// [`open_on`](Self::open_on) otherwise.
    ///
    /// So a document under `root` goes to the window root's servers; one
    /// outside it, or opened with no root, to servers at their marker roots
    /// (or its own folder, with no marker above), under the same trust rule
    /// as the window root. Each server places it apart: one it can't go to
    /// (none listing its language, `[lsp]` turning it off, a root
    /// [`set_trust`](Self::set_trust) rejects, a command missing or crashed
    /// out) leaves it waiting there alone, until settings, a spec, trust or
    /// a [`restart`](Self::restart) give it one. Its subscription is drained
    /// at each [`changed`](Self::changed) meanwhile. An alias of a buffer
    /// that holds `path` for the same lead waits on every server until that
    /// one closes.
    ///
    /// Errs if `path` isn't absolute.
    #[allow(clippy::too_many_arguments)] // Each is one thing the document needs.
    pub fn open(
        &mut self,
        root: Option<&Path>,
        buffer: BufferId,
        text: Text,
        version: Version,
        subscription: Subscription,
        path: &Path,
        language: &str,
    ) -> Result<Option<ServerId>, LspError> {
        let uri = Url::from_file_path(path).map_err(|()| {
            LspError::Unavailable(
                tr!("lsp-not-absolute", path = path.display().to_string())
                    .to_string()
                    .into(),
            )
        })?;
        self.close(buffer);
        let specs = self.registry.specs();
        let mut route = Route {
            roots: Vec::new(),
            lead: None,
            primary: false,
            window: root.map(Path::to_owned),
            path: path.to_owned(),
            uri: uri.clone(),
            language: language.to_owned(),
            opened: self.next_open,
            holder: false,
            generation: self.identities.open(buffer),
            max_bytes: self.max_file_bytes,
        };
        self.next_open += 1;
        let (_, names) = route.reroot(&specs, None);
        // The group's holder keeps its path, placed or not.
        route.holder = route.lead.is_some() && self.holder_of(&route.uri, &route.lead).is_none();
        self.documents.insert(
            buffer,
            Placed {
                subscribed: Subscribed::new(subscription, text, version),
                filtered_as: (uri, language_id(language, path)),
                kind: Kind::Routed(Box::new(route)),
            },
        );
        if names.is_empty() {
            // No server lists it: an opening under no name keeps its
            // subscription drained until one does.
            let opening = self.opening(buffer).expect("just opened");
            self.routes
                .lock()
                .lot
                .insert((buffer, String::new()), opening);
        }
        // One opening per server, each with its own tap.
        let lead = names.first().cloned();
        let mut on_lead = None;
        let mut placed = Vec::new();
        for name in names {
            let opening = self.opening(buffer).expect("just opened");
            let (server, place) = self.place_for(buffer, &name, &specs);
            let mut routes = self.routes.lock();
            routes.places.insert((buffer, name.clone()), place);
            routes.forward(&self.identities, &name, vec![opening]);
            drop(routes);
            if server.is_some() {
                if Some(&name) == lead.as_ref() {
                    on_lead = server;
                }
                placed.push(name);
            }
        }
        // Documents waiting for the same servers (after one failed, say) go
        // to them too.
        for name in placed {
            let Some(root) = self.route(buffer).and_then(|route| route.root_of(&name)) else {
                continue;
            };
            let only = (name, root.to_owned());
            self.place_waiting(&specs, Some(&only));
        }
        // Where the lead was placed, even if it has failed since.
        Ok(on_lead)
    }

    /// A new opening of routed `buffer`, for one of its servers, at its
    /// hub's latest text, with a tap of its own.
    fn opening(&self, buffer: BufferId) -> Option<Opening> {
        let placed = self.documents.get(&buffer)?;
        let Kind::Routed(route) = &placed.kind else {
            return None;
        };
        let (subscription, text, version) = placed.subscribed.tap();
        Some(Opening {
            buffer,
            text,
            version,
            subscription,
            uri: route.uri.clone(),
            language_id: placed.filtered_as.1.clone(),
            generation: route.generation,
            max_bytes: route.max_bytes,
        })
    }

    /// Places routed `buffer` at `place` on server `name`. If it's an
    /// instance and the document's opening for it is waiting in the lot
    /// (handed back meanwhile), that goes there now.
    fn set_place(&self, buffer: BufferId, name: &str, place: Place) {
        let mut routes = self.routes.lock();
        let key = (buffer, name.to_owned());
        let to_server = matches!(place, Place::Server(..));
        routes.places.insert(key.clone(), place);
        if to_server && let Some(opening) = routes.lot.remove(&key) {
            routes.forward(&self.identities, name, vec![opening]);
        }
    }

    /// The instance of server `name` for routed `buffer` under `specs`,
    /// started if need be, and its place there, or none and
    /// [`Place::Waiting`]: so too for a buffer that doesn't hold its path.
    fn place_for(
        &mut self,
        buffer: BufferId,
        name: &str,
        specs: &[ServerSpec],
    ) -> (Option<ServerId>, Place) {
        if !self.holds_path(buffer) {
            return (None, Place::Waiting);
        }
        self.target(buffer, name, specs)
            // A server has one document per URI, however documents came to
            // be routed to it (two leads' holders meeting on one add-on,
            // settings or trust moving several there): the others wait
            // there until it closes.
            .filter(|&server| !self.holds_uri(buffer, name, server))
            .and_then(|server| {
                let commands = self.servers.get(&server)?.commands.clone();
                Some((Some(server), Place::Server(server, commands)))
            })
            .unwrap_or((None, Place::Waiting))
    }

    /// Whether routed `buffer` holds its path for its lead (§4). One with
    /// no lead holds nothing.
    fn holds_path(&self, buffer: BufferId) -> bool {
        self.route(buffer)
            .is_some_and(|route| route.lead.is_some() && route.holder)
    }

    /// The buffer holding `uri` for `lead`, if one does.
    fn holder_of(&self, uri: &Url, lead: &Option<String>) -> Option<BufferId> {
        self.documents
            .iter()
            .find_map(|(&buffer, placed)| match &placed.kind {
                Kind::Routed(r) if r.holder && r.uri == *uri && r.lead == *lead => Some(buffer),
                _ => None,
            })
    }

    /// Whether a routed document other than `buffer`, of the same URI, is
    /// placed on `server` (as server `name`).
    fn holds_uri(&self, buffer: BufferId, name: &str, server: ServerId) -> bool {
        let Some(route) = self.route(buffer) else {
            return false;
        };
        let routes = self.routes.lock();
        self.documents.iter().any(|(&other, placed)| {
            other != buffer
                && matches!(&placed.kind, Kind::Routed(r) if r.uri == route.uri)
                && matches!(
                    routes.places.get(&(other, name.to_owned())),
                    Some(Place::Server(on, _)) if *on == server
                )
        })
    }

    /// The instance of server `name` that `buffer`'s route gives it under
    /// `specs`, started if it isn't running. `None` for a document without
    /// a route, whose route hasn't `name`, or whose language `name` doesn't
    /// serve under `specs` (as the first primary or an add-on), under
    /// an untrusted root (that server's), whose server there gave up after
    /// too many crashes, or whose server's thread can't be made.
    fn target(&mut self, buffer: BufferId, name: &str, specs: &[ServerSpec]) -> Option<ServerId> {
        let route = self.route(buffer)?;
        let root = route.root_of(name)?.to_owned();
        // A lead kept while its instance runs is never started again.
        let servers = servers_for(specs, &route.language);
        let spec = (servers.primary.into_iter().chain(servers.add_ons))
            .find(|spec| spec.name == name)?
            .clone();
        if !(self.trusted)(&root) {
            return None;
        }
        if let Some(crashes) = self.crashes.get(&(spec.name.clone(), root.clone())) {
            let mut crashes = lock(crashes);
            match &crashes.given_up {
                // It stays stopped until a restart.
                Some(given_up) if *given_up == spec => return None,
                // A new spec gets a fresh budget.
                Some(_) => crashes.clear(),
                None => {}
            }
        }
        match self.start_server(spec, &root, true) {
            Ok(server) => Some(server),
            Err(err) => {
                log::warn!("no thread for a language server: {err}");
                None
            }
        }
    }

    /// Brings running servers and waiting documents in line with the
    /// registry: see [`update_settings`](Self::update_settings).
    fn reconcile(&mut self) {
        self.prune();
        let specs = self.registry.specs();
        self.reroot(&specs);
        self.regroup();
        let routed: Vec<ServerId> = self
            .servers
            .iter()
            .filter(|(_, handle)| handle.routed)
            .map(|(&id, _)| id)
            .collect();
        for id in routed {
            // Retiring one may have found another gone.
            let Some(handle) = self.servers.get_mut(&id) else {
                continue;
            };
            if !(self.trusted)(&handle.root) {
                // Its documents wait: none starts under an untrusted root.
                self.retire(id, &specs);
                continue;
            }
            match specs.iter().find(|spec| spec.name == handle.name) {
                Some(spec) if !needs_restart(&handle.spec, spec) => {
                    if spec.settings != handle.spec.settings {
                        handle.spec.settings = spec.settings.clone();
                        let configure = Command::Configure(spec.settings.clone());
                        // A server that has gone has nothing to configure.
                        let _ = handle.commands.unbounded_send(configure);
                    }
                }
                Some(spec) => {
                    let (spec, root) = (spec.clone(), handle.root.clone());
                    self.respecced.push((spec.name.clone(), root.clone()));
                    self.retire(id, &specs);
                    // Launched again even with no documents, as it was
                    // running; its documents may have started it already.
                    if let Err(err) = self.start_server(spec, &root, true) {
                        log::warn!("no thread for a language server: {err}");
                    }
                }
                None => self.retire(id, &specs),
            }
        }
        // A lead that was kept above but has since been retired gives way
        // to the one its language has now.
        self.reroot(&specs);
        self.regroup();
        self.place_waiting(&specs, None);
    }

    /// Gives each routed document the servers its language has under
    /// `specs`, and the root each has for it, whose markers may have
    /// changed. A lead it's placed on that's running and still lists the
    /// language stays its lead, even if another would now come first. A
    /// server it gains waits for it, at its hub's latest text; one it
    /// loses closes it there. One whose language no server lists keeps its
    /// roots, and waits on each.
    fn reroot(&mut self, specs: &[ServerSpec]) {
        let buffers: Vec<BufferId> = self.documents.keys().copied().collect();
        for buffer in buffers {
            let keep = (self.route(buffer).and_then(|route| route.lead.clone())).filter(|lead| {
                self.placed_on(buffer, lead)
                    .is_some_and(|server| self.servers.contains_key(&server))
            });
            self.reroot_one(buffer, specs, keep.as_deref());
        }
    }

    /// Gives routed `buffer` the servers its language has under `specs`,
    /// keeping lead `keep` if given (see [`Route::reroot`]): a server it
    /// gains waits for it, at its hub's latest text; one it loses closes it
    /// there; one it keeps at another root closes it on the instance at
    /// the old root, and waits for the new root's, at its latest text.
    fn reroot_one(&mut self, buffer: BufferId, specs: &[ServerSpec], keep: Option<&str>) {
        let Some(Placed {
            kind: Kind::Routed(route),
            ..
        }) = self.documents.get_mut(&buffer)
        else {
            return;
        };
        let roots = route.roots.clone();
        let (before, after) = route.reroot(specs, keep);
        // Kept, but at another root (its markers changed): closed on the
        // instance at the old one, it waits for the new one's.
        let moved: Vec<String> = (route.roots.iter())
            .filter(|&kept| (roots.iter()).any(|(name, root)| *name == kept.0 && *root != kept.1))
            .map(|(name, _)| name.clone())
            .collect();
        for name in moved {
            self.release(buffer, &name, true);
        }
        // Gained first, so the document keeps a tap throughout.
        for name in after.iter().filter(|name| !before.contains(name)) {
            let spare = self.routes.lock().lot.remove(&(buffer, String::new()));
            let Some(opening) = spare.or_else(|| self.opening(buffer)) else {
                continue;
            };
            let mut routes = self.routes.lock();
            routes.places.insert((buffer, name.clone()), Place::Waiting);
            routes.lot.insert((buffer, name.clone()), opening);
        }
        for name in before.iter().filter(|name| !after.contains(name)) {
            self.release(buffer, name, false);
        }
    }

    /// Closes routed `buffer` on server `name` alone, retiring its identity
    /// there; its other servers keep it. If `keep`, it waits there again,
    /// at its hub's latest text; otherwise `name` is no longer one of its
    /// servers.
    fn release(&mut self, buffer: BufferId, name: &str, keep: bool) {
        let Some(route) = self.route(buffer) else {
            return;
        };
        let generation = route.generation;
        let key = (buffer, name.to_owned());
        let mut routes = self.routes.lock();
        let placed = routes.places.get(&key).cloned();
        let parked = match (keep, &placed) {
            (true, Some(Place::Server(..))) => {
                let fresh = self.opening(buffer);
                routes.places.insert(key.clone(), Place::Waiting);
                fresh.and_then(|fresh| routes.lot.insert(key, fresh))
            }
            (true, _) => None,
            (false, _) => {
                routes.places.remove(&key);
                routes.lot.remove(&key)
            }
        };
        drop(routes);
        drop(parked);
        let Some(Place::Server(server, commands)) = placed else {
            return;
        };
        if let Some(handle) = self.servers.get_mut(&server) {
            handle.closed = true;
        }
        let identities = self.identities.clone();
        let retire = move || {
            if let Some(identity) = identities.active(buffer, server) {
                identities.retire(buffer, generation, server, identity);
            }
        };
        retire();
        let _ = commands.unbounded_send(Command::Close(buffer));
        // An open still on its way there activates after that: it's
        // retired behind its close.
        let _ = commands.unbounded_send(Command::Documents(Box::new(move |_| retire())));
    }

    /// Gives each (URI, lead) group one holder after leads changed (§4):
    /// of groups that merged, the earliest opened of their holders keeps
    /// the path, whether or not it's placed now, and the others are closed
    /// on every server they're on, to wait; a group with no holder (one
    /// split off) gets its earliest opened.
    fn regroup(&mut self) {
        // Each member: whether it held its path, when it was opened, and it.
        type Member = (bool, u64, BufferId);
        let mut groups: HashMap<(Url, String), Vec<Member>> = HashMap::new();
        for (&buffer, placed) in &self.documents {
            if let Kind::Routed(route) = &placed.kind
                && let Some(lead) = &route.lead
            {
                (groups.entry((route.uri.clone(), lead.clone())).or_default()).push((
                    route.holder,
                    route.opened,
                    buffer,
                ));
            }
        }
        let mut lost = Vec::new();
        for members in groups.into_values() {
            // Holders first, then the earliest opened.
            let keeper = (members.iter())
                .min_by_key(|&&(holder, opened, _)| (!holder, opened))
                .map(|&(_, _, buffer)| buffer);
            for (holder, _, buffer) in members {
                let holds = Some(buffer) == keeper;
                if let Some(Placed {
                    kind: Kind::Routed(route),
                    ..
                }) = self.documents.get_mut(&buffer)
                {
                    route.holder = holds;
                }
                if holder && !holds {
                    lost.push(buffer);
                }
            }
        }
        for buffer in lost {
            for (name, _) in self.placements(buffer) {
                self.release(buffer, &name, true);
            }
        }
    }

    /// Stops routed server `id`, first placing each of its documents on
    /// the instance of that server they get under `specs` (started if need
    /// be: a new instance of it waits for it to end), or to wait there, so
    /// its thread forwards them. Their other servers keep them. Documents
    /// opened on it by [`open_on`](Self::open_on) go with it.
    fn retire(&mut self, id: ServerId, specs: &[ServerSpec]) {
        let Some(handle) = self.servers.remove(&id) else {
            return;
        };
        let (name, root) = (handle.name.clone(), handle.root.clone());
        self.retiring
            .insert((name.clone(), root), handle.done.clone());
        let buffers: Vec<BufferId> = (self.documents.iter())
            .filter(|&(&buffer, placed)| match placed.kind {
                Kind::On(server) => server == id,
                Kind::Routed(_) => self.placed_on(buffer, &name) == Some(id),
            })
            .map(|(&buffer, _)| buffer)
            .collect();
        for buffer in buffers {
            if matches!(self.documents[&buffer].kind, Kind::On(_)) {
                self.forget(buffer);
                continue;
            }
            let (server, place) = self.place_for(buffer, &name, specs);
            self.set_place(buffer, &name, place);
            // A lead kept only while that instance ran (another primary
            // comes first now) gives way to the language's own.
            let Some(route) = self.route(buffer) else {
                continue;
            };
            let kept = route.lead.as_deref() == Some(name.as_str())
                && server_for(specs, &route.language).is_none_or(|spec| spec.name != name);
            if server.is_none() && kept {
                self.reroot_one(buffer, specs, None);
                self.regroup();
                for other in self.route(buffer).map(Route::names).unwrap_or_default() {
                    if self.placed_on(buffer, &other).is_none() {
                        let (server, place) = self.place_for(buffer, &other, specs);
                        if server.is_some() {
                            self.set_place(buffer, &other, place);
                        }
                    }
                }
            }
        }
        // A document closed on it alone (it left the route) hears so first.
        handle.retire(&mut self.retiring);
    }

    /// Opens each document waiting for a server its route now gives it
    /// (only `only`, a server name and root, if given), with the size limit
    /// it was opened with. One its old instance hasn't handed back yet is
    /// forwarded there when it is.
    fn place_waiting(&mut self, specs: &[ServerSpec], only: Option<&(String, PathBuf)>) {
        let started = self.next_id;
        for (buffer, name) in self.waiting_for(only) {
            let (server, place) = self.place_for(buffer, &name, specs);
            if server.is_some() {
                self.set_place(buffer, &name, place);
            }
        }
        // One placed early in the pass may have come back meanwhile from an
        // instance that failed, which a later one replaced: it goes to the
        // instance running now rather than wait beside it. None is started
        // for this, so a server failing at once can't make it a loop.
        if self.next_id != started {
            for (buffer, name) in self.waiting_for(only) {
                if let Some(place) = self.running_place(buffer, &name) {
                    self.set_place(buffer, &name, place);
                }
            }
        }
    }

    /// The place of the instance running for routed `buffer`'s server
    /// `name` and its root there, if there is one and the buffer holds its
    /// path; none is started.
    fn running_place(&self, buffer: BufferId, name: &str) -> Option<Place> {
        if !self.holds_path(buffer) {
            return None;
        }
        let root = self.route(buffer)?.root_of(name)?;
        self.servers
            .iter()
            .find(|(_, handle)| handle.name == name && handle.root == root)
            .filter(|(id, _)| !self.holds_uri(buffer, name, **id))
            .map(|(&id, handle)| Place::Server(id, handle.commands.clone()))
    }

    /// The (routed document, server name) pairs waiting for a server their
    /// route gives them, under a trusted root (only `only`, a server name
    /// and root, if given), the earliest opened first.
    fn waiting_for(&self, only: Option<&(String, PathBuf)>) -> Vec<(BufferId, String)> {
        let mut candidates: Vec<(u64, BufferId, &str, &Path)> = Vec::new();
        for (&buffer, placed) in &self.documents {
            let Kind::Routed(route) = &placed.kind else {
                continue;
            };
            // None of its servers lists its language any more.
            if route.lead.is_none() {
                continue;
            }
            for (name, root) in &route.roots {
                if only.is_none_or(|(only, at)| only == name && at == root) {
                    candidates.push((route.opened, buffer, name, root));
                }
            }
        }
        candidates.sort_by_key(|&(opened, ..)| opened);
        candidates.retain(|&(_, _, _, root)| (self.trusted)(root));
        let routes = self.routes.lock();
        candidates
            .into_iter()
            .filter(|&(_, buffer, name, _)| {
                !matches!(
                    routes.places.get(&(buffer, name.to_owned())),
                    Some(Place::Server(..))
                )
            })
            .map(|(_, buffer, name, _)| (buffer, name.to_owned()))
            .collect()
    }

    /// Starts syncing buffer `buffer` to `server` (§4.1): `didOpen` with
    /// `text`, the buffer's text at `version`, with the `languageId` for
    /// fe-syntax's `language` (§3.2). Its LSP version is 0, or, for a URI
    /// the server had open before in this session, the last one it was
    /// sent plus 1. A text over [`max_file_bytes`](Self::max_file_bytes)
    /// isn't opened until edits bring it under.
    ///
    /// The open activates a new identity ([`active_identity`]) before its
    /// `didOpen` is sent, unless the buffer was closed meanwhile.
    ///
    /// [`active_identity`]: Self::active_identity
    ///
    /// From then on `server`'s thread drains `subscription`, which must
    /// have been made no later than `version` (edits up to it, and saves
    /// of earlier versions, are skipped): on [`changed`](Self::changed), and every 250 ms in case a
    /// nudge was missed. Each run of edits becomes a `didChange` and each
    /// save a `didSave`, in the buffer's order. Sent while the server
    /// starts, it goes once the server has initialized.
    ///
    /// A buffer already open is closed first. Errs if `path` isn't
    /// absolute, or `server` isn't running. The document stays on
    /// `server`: settings changes don't move it. It isn't gated by
    /// [`set_trust`](Self::set_trust): the host names the server.
    #[allow(clippy::too_many_arguments)] // Each is one thing the document needs.
    pub fn open_on(
        &mut self,
        server: ServerId,
        buffer: BufferId,
        text: Text,
        version: Version,
        subscription: Subscription,
        path: &Path,
        language: &str,
    ) -> Result<(), LspError> {
        let uri = Url::from_file_path(path).map_err(|()| {
            LspError::Unavailable(
                tr!("lsp-not-absolute", path = path.display().to_string())
                    .to_string()
                    .into(),
            )
        })?;
        if !self.servers.contains_key(&server) {
            return Err(LspError::ServerDown);
        }
        // Closed before the open is sent, so a server dropping the open
        // drops nothing the manager still tracks.
        self.close(buffer);
        let subscription = Subscribed::new(subscription, text, version);
        let (tap, text, version) = subscription.tap();
        let filtered_as = (uri.clone(), language_id(language, path));
        let opening = Opening {
            buffer,
            text,
            version,
            subscription: tap,
            uri,
            language_id: language_id(language, path),
            generation: self.identities.open(buffer),
            max_bytes: self.max_file_bytes,
        };
        if self.servers[&server]
            .commands
            .unbounded_send(Command::Open(Box::new(opening)))
            .is_err()
        {
            self.identities.close(buffer);
            return Err(LspError::ServerDown);
        }
        self.documents.insert(
            buffer,
            Placed {
                subscribed: subscription,
                kind: Kind::On(server),
                filtered_as,
            },
        );
        Ok(())
    }

    /// Stops syncing `buffer`: what it did until now that its servers
    /// haven't been sent yet is sent, in order, then `didClose`. The
    /// subscription is dropped here, on the calling thread, so edits made
    /// after this returns send nothing, and its identities are retired here
    /// too, before `didClose` is sent. An alias it held its path from opens
    /// on its servers now.
    pub fn close(&mut self, buffer: BufferId) {
        let uri = self.route(buffer).map(|route| route.uri.clone());
        for server in self.forget(buffer) {
            self.send(server, Command::Close(buffer));
        }
        let Some(uri) = uri else {
            return;
        };
        let mut held: Vec<(u64, BufferId)> = (self.documents.iter())
            .filter_map(|(&other, placed)| match &placed.kind {
                Kind::Routed(route) if route.uri == uri => Some((route.opened, other)),
                _ => None,
            })
            .collect();
        if held.is_empty() {
            return;
        }
        held.sort_by_key(|&(opened, _)| opened);
        // The next earliest opened holds the path if it held it.
        self.regroup();
        let specs = self.registry.specs();
        for (_, other) in held {
            let names = self.route(other).map(Route::names).unwrap_or_default();
            for name in names {
                if self.placed_on(other, &name).is_some() {
                    continue;
                }
                let (server, place) = self.place_for(other, &name, &specs);
                if server.is_some() {
                    self.set_place(other, &name, place);
                }
            }
        }
    }

    /// The `(session, incarnation)` of `buffer` that `server` has open now
    /// (§4.1), if any. It's set before the incarnation's `didOpen` is
    /// sent, and cleared when [`close`](Self::close) returns, when the
    /// document outgrows [`max_file_bytes`](Self::max_file_bytes), and
    /// when the session ends, before its end is reported.
    pub fn active_identity(
        &self,
        buffer: BufferId,
        server: ServerId,
    ) -> Option<(SessionId, IncarnationId)> {
        self.identities.active(buffer, server)
    }

    /// The table of identities, shared with the servers' threads.
    #[cfg(test)]
    pub(crate) fn identities(&self) -> &Identities {
        &self.identities
    }

    /// Holds the next document open to reach its server's thread before it
    /// activates, for tests of what runs in that window.
    #[cfg(any(test, feature = "test-support"))]
    pub fn hold_next_activation(&self) {
        self.identities
            .pauses()
            .arm(crate::identity::Point::Activate);
    }

    /// Blocks until a thread is held by [`hold_next_activation`](Self::hold_next_activation).
    #[cfg(any(test, feature = "test-support"))]
    pub fn wait_activation_held(&self) {
        self.identities
            .pauses()
            .wait_held(crate::identity::Point::Activate);
    }

    /// Lets a held activation go on.
    #[cfg(any(test, feature = "test-support"))]
    pub fn release_activation(&self) {
        self.identities.pauses().resume();
    }

    /// The LSP and buffer versions in `buffer`'s ring on its server,
    /// oldest first, once the commands before this one are done.
    #[cfg(test)]
    pub(crate) fn ring(
        &self,
        buffer: BufferId,
    ) -> impl Future<Output = Vec<(i32, Version)>> + use<> {
        let server = self.server_of(buffer);
        self.ring_on(buffer, server)
    }

    /// As [`ring`](Self::ring), on `server` (`None`: on none).
    #[cfg(test)]
    pub(crate) fn ring_on(
        &self,
        buffer: BufferId,
        server: Option<ServerId>,
    ) -> impl Future<Output = Vec<(i32, Version)>> + use<> {
        let (answer, answered) = oneshot::channel();
        if let Some(server) = server {
            self.send(server, Command::Ring(buffer, answer));
        }
        answered.map(Result::unwrap_or_default)
    }

    /// Whether `buffer` still queues events for fe-lsp.
    #[cfg(test)]
    pub(crate) fn is_subscribed(&self, buffer: BufferId) -> bool {
        self.documents
            .get(&buffer)
            .is_some_and(|placed| placed.subscribed.is_attached())
    }

    /// Says `buffer` emitted events (edits or a save), so its servers'
    /// threads drain them now rather than at the next backstop tick; a
    /// document waiting for a server is drained here. Cheap: call it once
    /// a frame for each buffer that changed.
    pub fn changed(&self, buffer: BufferId) {
        let names = match self.documents.get(&buffer).map(|placed| &placed.kind) {
            None => return,
            Some(Kind::On(server)) => return self.send(*server, Command::Changed(buffer)),
            Some(Kind::Routed(route)) => route.names(),
        };
        let mut servers = Vec::new();
        {
            let mut routes = self.routes.lock();
            // With the opening one no server lists keeps.
            for name in names.into_iter().chain([String::new()]) {
                let key = (buffer, name);
                match routes.places.get(&key) {
                    Some(Place::Server(server, _)) => servers.push(*server),
                    _ => {
                        if let Some(opening) = routes.lot.get_mut(&key) {
                            opening.catch_up();
                        }
                    }
                }
            }
        }
        for server in servers {
            self.send(server, Command::Changed(buffer));
        }
    }

    /// Every server this manager has running, in id order.
    #[allow(dead_code)] // Nothing calls it until the watched-files and file-operation cards.
    /// Whether instance `server` still runs (or starts, or restarts in
    /// place): it hasn't been stopped or replaced.
    pub(crate) fn runs(&self, server: ServerId) -> bool {
        self.servers.contains_key(&server)
    }

    pub(crate) fn server_ids(&self) -> Vec<ServerId> {
        let mut ids: Vec<ServerId> = self.servers.keys().copied().collect();
        ids.sort();
        ids
    }

    pub(crate) fn send(&self, server: ServerId, command: Command) {
        if let Some(handle) = self.servers.get(&server) {
            // A server that has gone has no documents to sync.
            let _ = handle.commands.unbounded_send(command);
        }
    }

    /// The position encoding `server` chose, once it has initialized.
    pub fn position_encoding(&self, server: ServerId) -> Option<PositionEncoding> {
        self.session(server, |session| session.encoding)
    }

    /// As [`document_request`](Self::document_request), but the request is
    /// queued only once the capabilities of `buffer`'s lead are known, and
    /// only if `provides` says it has `feature`: otherwise nothing is sent
    /// and the answer is `Unavailable(feature)`. (A server still
    /// initializing is waited for, on the manager's clock, so a request
    /// isn't queued for one that turns out not to have the feature; the
    /// method's timeout covers the wait too.) The buffer's `version` is
    /// checked when the request is queued, not before.
    pub(crate) fn document_request_if_provided<R: Request>(
        &self,
        buffer: BufferId,
        version: Version,
        feature: &'static str,
        provides: fn(&ServerCapabilities) -> bool,
        params: impl FnOnce(&Admitted) -> R::Params + Send + 'static,
    ) -> impl Future<Output = Result<(Reply<R::Result>, Admitted), LspError>> + Send + 'static {
        let lead = self.server_of(buffer).into_iter().collect();
        self.document_request_among::<R>(lead, buffer, version, feature, provides, params)
            .map(|answer| answer.map(|(reply, admitted, _)| (reply, admitted)))
    }

    /// As [`document_request_if_provided`](Self::document_request_if_provided),
    /// sent to `server` alone, whether or not it leads `buffer`: one server
    /// of a request [`ask_all`](Self::ask_all) aims at each of them.
    pub(crate) fn document_request_if_provided_on<R: Request>(
        &self,
        server: ServerId,
        buffer: BufferId,
        version: Version,
        feature: &'static str,
        provides: impl Fn(&ServerCapabilities) -> bool + Send + 'static,
        params: impl FnOnce(&Admitted) -> R::Params + Send + 'static,
    ) -> impl Future<Output = Result<(Reply<R::Result>, Admitted), LspError>> + Send + 'static {
        self.document_request_among::<R>(vec![server], buffer, version, feature, provides, params)
            .map(|answer| answer.map(|(reply, admitted, _)| (reply, admitted)))
    }

    /// As [`document_request_if_provided`](Self::document_request_if_provided),
    /// sent to `buffer`'s provider of the feature (§5): the first server
    /// it's placed on, in order (the lead first), that `provides` says has
    /// it, each waited for in turn while it initializes. With the server it
    /// went to. `Unavailable(feature)` if none has it.
    pub(crate) fn provider_request<R: Request>(
        &self,
        buffer: BufferId,
        version: Version,
        feature: &'static str,
        provides: impl Fn(&ServerCapabilities) -> bool + Send + 'static,
        params: impl FnOnce(&Admitted) -> R::Params + Send + 'static,
    ) -> impl Future<Output = Result<Provided<R::Result>, LspError>> + Send + 'static {
        let servers = self.servers_of(buffer);
        self.document_request_among::<R>(servers, buffer, version, feature, provides, params)
    }

    /// `R`, sent to the first of `servers` that `provides` says has
    /// `feature` (see [`admit_to_first`](Self::admit_to_first)).
    fn document_request_among<R: Request>(
        &self,
        servers: Vec<ServerId>,
        buffer: BufferId,
        version: Version,
        feature: &'static str,
        provides: impl Fn(&ServerCapabilities) -> bool + Send + 'static,
        params: impl FnOnce(&Admitted) -> R::Params + Send + 'static,
    ) -> impl Future<Output = Result<Provided<R::Result>, LspError>> + Send + 'static {
        let timeout = self.timeout::<R>();
        let unprovided = LspError::Unavailable(feature.into());
        self.admit_to_first(
            servers,
            (buffer, version),
            timeout,
            (provides, unprovided),
            move |server, _, answer| {
                Ok(Box::new(move |admission| {
                    let admitted = match admission {
                        Ok(admitted) => admitted,
                        Err(err) => {
                            let _ = answer.send(Err(err));
                            return Vec::new();
                        }
                    };
                    let params = params(&admitted);
                    vec![call::<R, _>(answer, params, move |_, reply| {
                        reply.map(|reply| (reply, admitted, server))
                    })]
                }))
            },
        )
    }

    /// As [`document_request_if_provided_on`](Self::document_request_if_provided_on),
    /// for a follow-up on an item `owner` produced (§5, owners): sent only
    /// to `owner`'s server, and only while its session still has the
    /// document open as it had it then. Otherwise (the server stopped or
    /// restarted, or the document was reopened there) nothing is sent and
    /// the answer is `Unavailable("the language server restarted")`. The
    /// buffer's `version` is checked as for any document request.
    pub(crate) fn owner_request<R: Request>(
        &self,
        owner: Owner,
        version: Version,
        feature: &'static str,
        provides: impl Fn(&ServerCapabilities) -> bool + Send + 'static,
        params: impl FnOnce(&Admitted) -> R::Params + Send + 'static,
    ) -> impl Future<Output = Result<(Reply<R::Result>, Admitted), LspError>> + Send + 'static {
        let timeout = self.timeout::<R>();
        self.admit_to_owner(owner, version, timeout, move |capabilities, answer| {
            if !provides(capabilities) {
                return Err(LspError::Unavailable(feature.into()));
            }
            Ok(Box::new(move |admission| {
                let admitted = match admission {
                    Ok(admitted) => admitted,
                    Err(err) => {
                        let _ = answer.send(Err(err));
                        return Vec::new();
                    }
                };
                let params = params(&admitted);
                vec![call::<R, _>(answer, params, move |_, reply| {
                    reply.map(|reply| (reply, admitted))
                })]
            }))
        })
    }

    /// Admits `owner`'s document at `version` on `owner`'s server alone,
    /// once its capabilities are known, as
    /// [`admit_to_first`](Self::admit_to_first) does, for `decide`'s
    /// [`Admit`]. That is handed the admission only if the session has the
    /// document open as `owner` had it; otherwise it's handed
    /// `Unavailable("the language server restarted")`, and sends nothing.
    /// So is the answer at once if the server has stopped, or no longer
    /// has that opening active.
    pub(crate) fn admit_to_owner<T: Send + 'static>(
        &self,
        owner: Owner,
        version: Version,
        timeout: Option<Duration>,
        decide: impl FnOnce(
            &ServerCapabilities,
            oneshot::Sender<Result<T, LspError>>,
        ) -> Result<Admit, LspError>
        + Send
        + 'static,
    ) -> BoxFuture<'static, Result<T, LspError>> {
        let current = self.servers.contains_key(&owner.server)
            && self.active_identity(owner.buffer, owner.server) == Some(owner.identity);
        if !current {
            return async { Err(LspError::Unavailable(restarted())) }.boxed();
        }
        self.admit_on_one(
            vec![owner.server],
            owner.buffer,
            version,
            timeout,
            move |capabilities, answer| {
                let admit = decide(capabilities, answer)?;
                // Checked again as the request is admitted: the session may
                // have ended, or reopened the document, since.
                Ok(Box::new(move |admission: Result<Admitted, LspError>| {
                    admit(admission.and_then(|admitted| {
                        if admitted.document.identity == owner.identity {
                            Ok(admitted)
                        } else {
                            Err(LspError::Unavailable(restarted()))
                        }
                    }))
                }))
            },
        )
        .boxed()
    }

    /// Admits `buffer` at `version` on `server` alone once its capabilities
    /// are known, as [`admit_to_first`](Self::admit_to_first) does: `decide`
    /// is given them, with the sender the answer goes to, and builds the
    /// [`Admit`] that answers it, or the error that does instead (nothing
    /// is then admitted). No server: `Unavailable(not open)`.
    pub(crate) fn admit_on_one<T: Send + 'static>(
        &self,
        server: Vec<ServerId>,
        buffer: BufferId,
        version: Version,
        timeout: Option<Duration>,
        decide: impl FnOnce(
            &ServerCapabilities,
            oneshot::Sender<Result<T, LspError>>,
        ) -> Result<Admit, LspError>
        + Send
        + 'static,
    ) -> impl Future<Output = Result<T, LspError>> + Send + 'static {
        self.admit_to_first(
            server,
            (buffer, version),
            timeout,
            (
                |_: &ServerCapabilities| true,
                LspError::Unavailable(not_open()),
            ),
            move |_, capabilities, answer| decide(capabilities, answer),
        )
    }

    /// Admits `buffer` at `version` on the first of `servers` (in their
    /// order) whose capabilities `provides` accepts (`accepts` is
    /// `(provides, unprovided)`), each waited for in turn while unknown: `decide` is given that server and its
    /// capabilities, with the sender the answer goes to, and builds the
    /// [`Admit`] that answers it, or the error that does instead (nothing
    /// is then admitted). None accepting answers `unprovided`; no servers,
    /// `Unavailable(not open)`.
    ///
    /// A server still initializing is waited for on the manager's clock,
    /// and `timeout` (if any) covers the wait too. `decide` runs at once if
    /// the capabilities it needs are known already, so the request is
    /// queued as [`document_request`](Self::document_request) queues it.
    /// A server that stops while waited for is passed over; if none then
    /// accepts, the answer is `ServerDown`.
    pub(crate) fn admit_to_first<T: Send + 'static>(
        &self,
        servers: Vec<ServerId>,
        (buffer, version): (BufferId, Version),
        timeout: Option<Duration>,
        accepts: (
            impl Fn(&ServerCapabilities) -> bool + Send + 'static,
            LspError,
        ),
        decide: impl FnOnce(
            ServerId,
            &ServerCapabilities,
            oneshot::Sender<Result<T, LspError>>,
        ) -> Result<Admit, LspError>
        + Send
        + 'static,
    ) -> impl Future<Output = Result<T, LspError>> + Send + 'static {
        /// How often the capabilities are looked at while unknown.
        const POLL: Duration = Duration::from_millis(20);
        let (provides, unprovided) = accepts;
        let (answer, answered) = oneshot::channel();
        let mut targets: Vec<Target> = (servers.into_iter())
            .filter_map(|server| {
                let handle = self.servers.get(&server)?;
                Some((server, handle.session.clone(), handle.commands.clone()))
            })
            .collect();
        let clock = self.clock.clone();
        let mut chosen = Some((decide, answer));
        // Hands the request to `server`: decided, and its admission queued.
        let mut send = move |server: ServerId,
                             commands: &mpsc::UnboundedSender<Command>,
                             capabilities: &ServerCapabilities|
              -> Result<(), LspError> {
            let (decide, answer) = chosen.take().expect("a request is sent once");
            let admit = decide(server, capabilities, answer)?;
            // A server that has gone drops the command, and `answer` with it.
            let _ = commands.unbounded_send(Command::Admit {
                buffer,
                version,
                admit,
            });
            Ok(())
        };
        // Those known now are looked at now, so one that accepts is queued
        // now, as `document_request` queues, polled or not. From the first
        // still unknown on, the rest are waited for in turn.
        let mut refused = targets
            .is_empty()
            .then(|| LspError::Unavailable(not_open()));
        let mut sent = false;
        while refused.is_none() && !sent {
            let Some((server, session, commands)) = targets.first() else {
                refused = Some(unprovided.clone());
                break;
            };
            match known(session) {
                None => break,
                Some(capabilities) if provides(&capabilities) => {
                    if let Err(err) = send(*server, commands, &capabilities) {
                        refused = Some(err);
                    }
                    sent = true;
                }
                Some(_) => {
                    targets.remove(0);
                }
            }
        }
        let gated = async move {
            if let Some(err) = refused {
                return Err(err);
            }
            if !sent {
                let mut down = false;
                let mut accepted = None;
                for (server, session, commands) in targets {
                    let capabilities = loop {
                        match known(&session) {
                            Some(capabilities) => break Some(capabilities),
                            None if commands.is_closed() => break None,
                            None => clock.sleep(POLL).await,
                        }
                    };
                    match capabilities {
                        None => down = true,
                        Some(capabilities) if provides(&capabilities) => {
                            accepted = Some(send(server, &commands, &capabilities));
                            break;
                        }
                        Some(_) => {}
                    }
                }
                match accepted {
                    Some(sent) => sent?,
                    None if down => return Err(LspError::ServerDown),
                    None => return Err(unprovided),
                }
            }
            answered.await.unwrap_or(Err(LspError::ServerDown))
        };
        within(gated, timeout, &*self.clock, Err(LspError::Timeout))
    }

    /// Each server `buffer` is placed on, in order (the lead first), with
    /// `ask(server)`: one request aimed at each, all sent at once.
    pub(crate) fn ask_all<T>(
        &self,
        buffer: BufferId,
        mut ask: impl FnMut(ServerId) -> Answer<T>,
    ) -> Vec<(ServerId, Answer<T>)> {
        (self.servers_of(buffer).into_iter())
            .map(|server| (server, ask(server)))
            .collect()
    }

    /// `buffer`'s provider of `feature` (§5), as its servers' capabilities
    /// are known now: the first server it's placed on, in order (the lead
    /// first), that advertises it. On-type formatting counts the triggers
    /// a registration taking the document gives too. `None` if none does,
    /// or none that does is known yet.
    pub fn provider_of(&self, buffer: BufferId, feature: Feature) -> Option<ServerId> {
        (self.servers_of(buffer).into_iter()).find(|&server| match feature {
            Feature::OnTypeFormatting => !self.on_type_triggers_on(server, buffer).is_empty(),
            _ => (self.capabilities(server))
                .is_some_and(|capabilities| feature.advertised(&capabilities)),
        })
    }

    /// What `server` said it can do, once it has initialized.
    pub fn capabilities(&self, server: ServerId) -> Option<ServerCapabilities> {
        self.session(server, |session| session.capabilities.clone())
    }

    pub(crate) fn session<T>(
        &self,
        server: ServerId,
        read: impl FnOnce(&Session) -> Option<T>,
    ) -> Option<T> {
        let handle = self.servers.get(&server)?;
        read(
            &handle
                .session
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }

    /// The characters that ask `buffer`'s provider of on-type formatting
    /// (see [`provider_of`](Self::provider_of)) for it when typed: its
    /// `documentOnTypeFormattingProvider`'s, and those of each
    /// `textDocument/onTypeFormatting` registration whose document selector
    /// takes the buffer (§5: the provider's alone). None for a buffer on no
    /// server, or whose servers' capabilities aren't known yet.
    pub fn on_type_formatting_triggers(&self, buffer: BufferId) -> Vec<char> {
        (self.servers_of(buffer).into_iter())
            .map(|server| self.on_type_triggers_on(server, buffer))
            .find(|triggers| !triggers.is_empty())
            .unwrap_or_default()
    }

    /// The characters that ask `server` for on-type formatting of `buffer`.
    pub(crate) fn on_type_triggers_on(&self, server: ServerId, buffer: BufferId) -> Vec<char> {
        let Some(placed) = self.documents.get(&buffer) else {
            return Vec::new();
        };
        let (uri, language_id) = &placed.filtered_as;
        self.session(server, |session| {
            let provider = (session.capabilities.as_ref())
                .and_then(|capabilities| capabilities.document_on_type_formatting_provider.clone());
            let mut chars = provider.map_or_else(Vec::new, |provider| {
                crate::registration::trigger_chars(
                    &provider.first_trigger_character,
                    provider.more_trigger_character.as_deref(),
                )
            });
            for ch in session.registrations.on_type_triggers(uri, language_id) {
                if !chars.contains(&ch) {
                    chars.push(ch);
                }
            }
            Some(chars)
        })
        .unwrap_or_default()
    }

    /// The ids of `server`'s current session's registrations.
    #[cfg(test)]
    pub(crate) fn registrations(&self, server: ServerId) -> Vec<String> {
        self.session(server, |session| Some(session.registrations.ids()))
            .unwrap_or_default()
    }

    /// How many orphans `server`'s current session holds, and the most it
    /// has held.
    #[cfg(test)]
    pub(crate) fn orphans(&self, server: ServerId) -> (usize, usize) {
        self.session(server, |session| Some(session.orphans))
            .unwrap_or_default()
    }

    /// Starts recording what `server`'s current session hands async-lsp.
    #[cfg(test)]
    pub(crate) fn record_handed(&self, server: ServerId) {
        if let Some(handle) = self.servers.get(&server) {
            lock(&handle.session).handed = Some(Vec::new());
        }
    }

    /// What `server`'s current session handed async-lsp since
    /// [`record_handed`](Self::record_handed).
    #[cfg(test)]
    pub(crate) fn handed(&self, server: ServerId) -> Vec<String> {
        self.session(server, |session| session.handed.clone())
            .unwrap_or_default()
    }

    /// Whether `server`'s thread has ended.
    #[cfg(test)]
    pub(crate) fn thread_finished(&self, server: ServerId) -> bool {
        self.servers
            .get(&server)
            .is_none_or(|handle| handle.thread.is_finished())
    }
}

#[cfg(test)]
#[path = "trust_tests.rs"]
mod trust_tests;

impl Drop for LspManager {
    /// Asks every server to shut down, without waiting for them; a
    /// [`KillSwitch`] taken beforehand can still force them.
    fn drop(&mut self) {
        for (_, handle) in self.servers.drain() {
            handle.stop(&mut self.retiring);
        }
    }
}

/// Each server's open work-done progress, by its name, as the sink last
/// heard it.
#[derive(Clone, Default)]
struct Progress(Arc<Mutex<HashMap<ServerId, Named>>>);

/// A server's name, and its open work.
type Named = (String, Vec<WorkProgress>);

impl Progress {
    /// `sink`, recording each server's progress before it hears of it,
    /// and clearing it as its session ends: when a server whose progress
    /// was open goes `Restarting` or `Stopped`, `sink` hears an empty
    /// [`LspEvent::Progress`] first.
    fn tap(
        &self,
        mut sink: impl FnMut(LspEvent) + Send + 'static,
    ) -> impl FnMut(LspEvent) + Send + 'static {
        let progress = self.clone();
        move |event| {
            let ended = match &event {
                LspEvent::Progress { server, name, work } => {
                    let mut progress = lock(&progress.0);
                    if work.is_empty() {
                        progress.remove(server);
                    } else {
                        progress.insert(*server, (name.clone(), work.clone()));
                    }
                    None
                }
                LspEvent::Status {
                    server,
                    status: ServerStatus::Restarting(_) | ServerStatus::Stopped(_),
                    ..
                } => lock(&progress.0)
                    .remove(server)
                    .map(|(name, _)| (*server, name)),
                _ => None,
            };
            if let Some((server, name)) = ended {
                sink(LspEvent::Progress {
                    server,
                    name,
                    work: Vec::new(),
                });
            }
            sink(event);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
