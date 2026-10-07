//! [`LspManager`]: starts and stops servers, and carries requests to them.
//!
//! Documents opened with [`LspManager::open`] are routed (§3.2): each goes
//! to the first enabled server listing its language (registry.rs), one
//! instance per (server, root), started by the first document that needs
//! it and left running when its last document closes. One with no server
//! to go to waits, parked, until settings or a host spec give it one.
//!
//! A server that crashes restarts itself (restart.rs); the manager keeps
//! each (server, root)'s crash list and log ring across the instances it
//! starts there, so a server that gave up stays stopped until
//! [`LspManager::restart`] or a new spec, and its log outlives it.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use fe_buffer::{BufferId, Subscription, Version};
use fe_config::settings::LspSettings;
use fe_text::Text;
use futures::channel::{mpsc, oneshot};
use futures::future::FutureExt;
use lsp_types::request::Request;
use lsp_types::{ServerCapabilities, Url};

use crate::clock::{Clock, SystemClock};
use crate::document::{Admitted, Attached, Opening, Subscribed, language_id};
use crate::encoding::PositionEncoding;
use crate::event::{LspError, LspEvent, NOT_OPEN, ServerId, SharedLog};
use crate::identity::{Identities, IncarnationId, SessionId};
use crate::launcher::{ServerLauncher, ServerSpec};
use crate::registry::{Registry, needs_restart, server_for};
use crate::requests::{Reply, call, timeout, within};
use crate::restart::SharedCrashes;
use crate::server::{Admit, Command, Done, Place, Routes, Server, Session, Sink};

/// A reply, with the id the actor mirrored for its request.
pub(crate) type WithId<T> = (i32, Result<Reply<T>, LspError>);

/// The default [`LspManager::max_file_bytes`]: 50 MB, the size past which
/// fastedit highlights only the viewport.
pub const DEFAULT_MAX_FILE_BYTES: usize = 50 * 1024 * 1024;

/// Runs language servers, one thread each, and reports on them through an
/// event sink.
pub struct LspManager {
    launcher: Arc<dyn ServerLauncher>,
    clock: Arc<dyn Clock>,
    sink: Sink,
    servers: HashMap<ServerId, Handle>,
    /// Every open document: where it's synced, and its subscription,
    /// shared with that server's thread.
    documents: HashMap<BufferId, Placed>,
    /// Where routed documents are, and those waiting for a server, shared
    /// with the servers' threads, which hand theirs on when they end.
    routes: Routes,
    /// For each (name, root) whose server was stopped, a signal its thread
    /// has ended, which the next instance there waits for before it
    /// launches.
    retiring: HashMap<(String, PathBuf), Done>,
    registry: Registry,
    /// Which identity of each open buffer is active on each server,
    /// shared with the servers' threads.
    identities: Identities,
    /// The crashes of the servers started at each (name, root).
    crashes: HashMap<(String, PathBuf), SharedCrashes>,
    /// The log ring of the servers started at each (name, root).
    logs: HashMap<(String, PathBuf), SharedLog>,
    max_file_bytes: usize,
    next_id: u64,
}

struct Handle {
    name: String,
    root: PathBuf,
    /// What it was started with, or last configured with.
    spec: ServerSpec,
    /// Started by [`LspManager::open`], so settings changes apply to it.
    routed: bool,
    commands: mpsc::UnboundedSender<Command>,
    stop: oneshot::Sender<()>,
    /// Resolves when its thread has ended.
    done: Done,
    session: Arc<Mutex<Session>>,
    thread: JoinHandle<()>,
}

impl Handle {
    /// Asks it to stop; the next instance of the same name and root waits
    /// for its thread to end.
    fn stop(self, retiring: &mut HashMap<(String, PathBuf), Done>) {
        let _ = self.stop.send(());
        retiring.insert((self.name, self.root), self.done);
    }
}

/// An open document.
struct Placed {
    subscribed: Subscribed,
    kind: Kind,
}

enum Kind {
    /// Opened on a server of the host's choosing ([`LspManager::open_on`]).
    On(ServerId),
    /// Routed by [`LspManager::open`]: where it is is in `routes`.
    Routed(Route),
}

struct Route {
    root: PathBuf,
    /// fe-syntax's name.
    language: String,
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
        Self {
            launcher,
            clock,
            sink: Arc::new(Mutex::new(sink)),
            servers: HashMap::new(),
            documents: HashMap::new(),
            routes: Routes::default(),
            retiring: HashMap::new(),
            registry: Registry::default(),
            identities: Identities::default(),
            crashes: HashMap::new(),
            logs: HashMap::new(),
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            next_id: 0,
        }
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
    /// A stopped or restarted server's documents go to the server they'd
    /// get if opened now, at the text it last drained, or wait for one.
    /// Documents waiting get a server if one now applies. A document stays
    /// on a server that keeps running, even if another would now come
    /// first for its language. `max_file_bytes` applies to documents
    /// opened from now on.
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

    /// Registers a host's `spec`, which wins over a built-in or settings
    /// spec of the same name; settings may still turn it off. Applies as
    /// [`update_settings`](Self::update_settings) does.
    pub fn register_spec(&mut self, spec: ServerSpec) {
        self.registry.register(spec);
        self.reconcile();
    }

    /// The server `buffer` is synced to, if any.
    pub fn server_of(&self, buffer: BufferId) -> Option<ServerId> {
        match self.documents.get(&buffer)?.kind {
            Kind::On(server) => Some(server),
            Kind::Routed(_) => self.place_of(buffer),
        }
    }

    /// The server a routed document is placed on, if any.
    fn place_of(&self, buffer: BufferId) -> Option<ServerId> {
        match self
            .routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .places
            .get(&buffer)?
        {
            Place::Server(server, _) => Some(*server),
            Place::Waiting => None,
        }
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
    /// clears them.
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
        let mut routes = self.routes.lock().unwrap_or_else(PoisonError::into_inner);
        self.documents.retain(|&buffer, placed| match placed.kind {
            Kind::On(server) => {
                let live = !gone.contains(&server);
                if !live {
                    identities.close(buffer);
                }
                live
            }
            Kind::Routed(_) => {
                if let Some(Place::Server(server, _)) = routes.places.get(&buffer)
                    && gone.contains(server)
                {
                    routes.places.insert(buffer, Place::Waiting);
                }
                true
            }
        });
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
            routes: self.routes.clone(),
            after,
            done: Some(done_sender),
            crashes,
            epoch,
            log,
        }
        .spawn()?;
        self.retiring.remove(&key);
        self.servers.insert(
            id,
            Handle {
                name,
                root: root.to_owned(),
                spec,
                routed,
                commands,
                stop,
                done: done.shared(),
                session,
                thread,
            },
        );
        Ok(id)
    }

    /// Shuts `server` down (§3.4) on its own thread, or ends its wait to
    /// restart; the sink hears `Stopped(None)` when it has gone. Requests still waiting get
    /// [`LspError::ServerDown`]. Its documents go with it, routed ones too:
    /// they're no longer tracked, as if closed, and a host wanting them
    /// synced again opens them again. (One that a crash or a settings
    /// change leaves without a server waits for one instead.)
    pub fn stop(&mut self, server: ServerId) {
        // Its documents go with it.
        let gone: Vec<BufferId> = self
            .documents
            .keys()
            .copied()
            .filter(|&buffer| self.server_of(buffer) == Some(server))
            .collect();
        for buffer in gone {
            self.forget(buffer);
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
    /// there's no such server to start. Errs only if its thread can't be
    /// made.
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

    /// Stops tracking `buffer`: retires its identity, drops its
    /// subscription, its place and any opening waiting for it. Returns the
    /// server it was synced to.
    fn forget(&mut self, buffer: BufferId) -> Option<ServerId> {
        let server = self.server_of(buffer);
        let placed = self.documents.remove(&buffer)?;
        self.identities.close(buffer);
        placed.subscribed.detach();
        let mut routes = self.routes.lock().unwrap_or_else(PoisonError::into_inner);
        routes.places.remove(&buffer);
        let parked = routes.lot.remove(&buffer);
        drop(routes);
        drop(parked);
        server
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
    /// (§4.1). The server's thread brings the document up to date first,
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
        let (answer, answered) = oneshot::channel();
        let admit: Admit = Box::new(move |admission| {
            let admitted = match admission {
                Ok(admitted) => admitted,
                Err(err) => {
                    let _ = answer.send(Err(err));
                    return None;
                }
            };
            let params = params(&admitted);
            Some(call::<R, _>(answer, params, move |_, reply| {
                reply.map(|reply| (reply, admitted))
            }))
        });
        match self.server_of(buffer) {
            Some(server) => self.send(
                server,
                Command::Admit {
                    buffer,
                    version,
                    admit,
                },
            ),
            None => {
                admit(Err(LspError::Unavailable(NOT_OPEN.into())));
            }
        }
        let answered = answered.map(|answer| answer.unwrap_or(Err(LspError::ServerDown)));
        within(
            answered,
            self.timeout::<R>(),
            &*self.clock,
            Err(LspError::Timeout),
        )
    }

    /// Starts syncing buffer `buffer`, at `path`, to the server for
    /// fe-syntax's `language` and the workspace at `root` (§3.2), starting
    /// that server if it isn't running, and returns which it is. As
    /// [`open_on`](Self::open_on) otherwise.
    ///
    /// A document with no root, or whose path isn't under it, gets no
    /// server, and isn't kept. One with no server for its language (none
    /// listing it, or `[lsp]` turning it off) is kept, without a server,
    /// until [`update_settings`](Self::update_settings) or
    /// [`register_spec`](Self::register_spec) give it one; its subscription
    /// is drained at each [`changed`](Self::changed) meanwhile.
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
            LspError::Unavailable(format!("{} isn't an absolute path", path.display()))
        })?;
        self.close(buffer);
        let Some(root) = root.filter(|root| is_under(path, root)) else {
            return Ok(None);
        };
        let subscribed = Subscribed::new(subscription);
        let opening = Opening {
            buffer,
            text,
            version,
            subscription: Attached(subscribed.clone()),
            uri,
            language_id: language_id(language, path),
            generation: self.identities.open(buffer),
            max_bytes: self.max_file_bytes,
        };
        self.documents.insert(
            buffer,
            Placed {
                subscribed,
                kind: Kind::Routed(Route {
                    root: root.to_owned(),
                    language: language.to_owned(),
                }),
            },
        );
        let specs = self.registry.specs();
        let server = self.place(opening, &specs);
        // Documents waiting for the same server (after it failed, say) go
        // to it too.
        if server.is_some()
            && let Some(spec) = server_for(&specs, language)
        {
            let only = (spec.name.clone(), root.to_owned());
            self.place_waiting(&specs, Some(&only));
        }
        Ok(server)
    }

    /// Opens `opening` on the server its route gives it under `specs`,
    /// starting that if need be, or leaves it waiting.
    fn place(&mut self, opening: Opening, specs: &[ServerSpec]) -> Option<ServerId> {
        let buffer = opening.buffer;
        let (server, place) = self.place_for(buffer, specs);
        let mut routes = self.routes.lock().unwrap_or_else(PoisonError::into_inner);
        routes.places.insert(buffer, place);
        routes.forward(&self.identities, vec![opening]);
        server
    }

    /// Places routed `buffer` at `place`. If it's a server and the
    /// document's opening is waiting in the lot (handed back meanwhile),
    /// that goes there now.
    fn set_place(&self, buffer: BufferId, place: Place) {
        let mut routes = self.routes.lock().unwrap_or_else(PoisonError::into_inner);
        let to_server = matches!(place, Place::Server(..));
        routes.places.insert(buffer, place);
        if to_server && let Some(opening) = routes.lot.remove(&buffer) {
            routes.forward(&self.identities, vec![opening]);
        }
    }

    /// The server for `buffer` under `specs`, started if need be, and its
    /// place, or none and [`Place::Waiting`].
    fn place_for(&mut self, buffer: BufferId, specs: &[ServerSpec]) -> (Option<ServerId>, Place) {
        self.target(buffer, specs)
            .and_then(|server| {
                let commands = self.servers.get(&server)?.commands.clone();
                Some((Some(server), Place::Server(server, commands)))
            })
            .unwrap_or((None, Place::Waiting))
    }

    /// The server `buffer`'s route gives it under `specs`, started if it
    /// isn't running. `None` for a document without a route, with no
    /// server for its language, or whose server's thread can't be made.
    fn target(&mut self, buffer: BufferId, specs: &[ServerSpec]) -> Option<ServerId> {
        let Kind::Routed(route) = &self.documents.get(&buffer)?.kind else {
            return None;
        };
        let spec = server_for(specs, &route.language)?.clone();
        let root = route.root.clone();
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
        self.place_waiting(&specs, None);
    }

    /// Stops routed server `id`, first placing each of its documents on
    /// the server it gets under `specs` (started if need be: a new
    /// instance of it waits for it to end), or to wait, so its thread
    /// forwards them there. Documents opened on it by
    /// [`open_on`](Self::open_on) go with it.
    fn retire(&mut self, id: ServerId, specs: &[ServerSpec]) {
        let Some(handle) = self.servers.remove(&id) else {
            return;
        };
        let (name, root) = (handle.name.clone(), handle.root.clone());
        self.retiring.insert((name, root), handle.done);
        let buffers: Vec<BufferId> = self
            .documents
            .keys()
            .copied()
            .filter(|&buffer| self.server_of(buffer) == Some(id))
            .collect();
        for buffer in buffers {
            if matches!(self.documents[&buffer].kind, Kind::On(_)) {
                self.forget(buffer);
                continue;
            }
            let (_, place) = self.place_for(buffer, specs);
            self.set_place(buffer, place);
        }
        let _ = handle.stop.send(());
    }

    /// Opens each waiting document whose route now gives it a server
    /// (only `only`, a server name and root, if given), with the size limit
    /// it was opened with. One its old server hasn't handed back yet is
    /// forwarded there when it is.
    fn place_waiting(&mut self, specs: &[ServerSpec], only: Option<&(String, PathBuf)>) {
        let started = self.next_id;
        for buffer in self.waiting_for(specs, only) {
            let (server, place) = self.place_for(buffer, specs);
            if server.is_some() {
                self.set_place(buffer, place);
            }
        }
        // One placed early in the pass may have come back meanwhile from an
        // instance that failed, which a later one replaced: it goes to the
        // instance running now rather than wait beside it. None is started
        // for this, so a server failing at once can't make it a loop.
        if self.next_id != started {
            for buffer in self.waiting_for(specs, only) {
                if let Some(place) = self.running_place(buffer, specs) {
                    self.set_place(buffer, place);
                }
            }
        }
    }

    /// The place of the instance running for routed `buffer`'s server and
    /// root under `specs`, if there is one; none is started.
    fn running_place(&self, buffer: BufferId, specs: &[ServerSpec]) -> Option<Place> {
        let Kind::Routed(route) = &self.documents.get(&buffer)?.kind else {
            return None;
        };
        let spec = server_for(specs, &route.language)?;
        self.servers
            .iter()
            .find(|(_, handle)| handle.name == spec.name && handle.root == route.root)
            .map(|(&id, handle)| Place::Server(id, handle.commands.clone()))
    }

    /// The routed documents waiting for a server that `specs` give them
    /// one of (only `only`, a server name and root, if given).
    fn waiting_for(&self, specs: &[ServerSpec], only: Option<&(String, PathBuf)>) -> Vec<BufferId> {
        self.documents
            .iter()
            .filter_map(|(&buffer, placed)| {
                let Kind::Routed(route) = &placed.kind else {
                    return None;
                };
                let spec = server_for(specs, &route.language)?;
                only.is_none_or(|(name, root)| spec.name == *name && route.root == *root)
                    .then_some(buffer)
            })
            .filter(|&buffer| self.place_of(buffer).is_none())
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
    /// `server`: settings changes don't move it.
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
            LspError::Unavailable(format!("{} isn't an absolute path", path.display()))
        })?;
        if !self.servers.contains_key(&server) {
            return Err(LspError::ServerDown);
        }
        // Closed before the open is sent, so a server dropping the open
        // drops nothing the manager still tracks.
        self.close(buffer);
        let subscription = Subscribed::new(subscription);
        let opening = Opening {
            buffer,
            text,
            version,
            subscription: Attached(subscription.clone()),
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
            },
        );
        Ok(())
    }

    /// Stops syncing `buffer`: what it did until now that its server
    /// hasn't been sent yet is sent, in order, then `didClose`. The
    /// subscription is dropped here, on the calling thread, so edits made
    /// after this returns send nothing, and its identity is retired here
    /// too, before `didClose` is sent.
    pub fn close(&mut self, buffer: BufferId) {
        if let Some(server) = self.forget(buffer) {
            self.send(server, Command::Close(buffer));
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

    /// The LSP and buffer versions in `buffer`'s ring on its server,
    /// oldest first, once the commands before this one are done.
    #[cfg(test)]
    pub(crate) fn ring(
        &self,
        buffer: BufferId,
    ) -> impl Future<Output = Vec<(i32, Version)>> + use<> {
        let (answer, answered) = oneshot::channel();
        if let Some(server) = self.server_of(buffer) {
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

    /// Says `buffer` emitted events (edits or a save), so its server's
    /// thread drains them now rather than at the next backstop tick; a
    /// document waiting for a server is drained here. Cheap: call it once
    /// a frame for each buffer that changed.
    pub fn changed(&self, buffer: BufferId) {
        if !self.documents.contains_key(&buffer) {
            return;
        }
        match self.server_of(buffer) {
            Some(server) => self.send(server, Command::Changed(buffer)),
            None => {
                let mut routes = self.routes.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(opening) = routes.lot.get_mut(&buffer) {
                    opening.catch_up();
                }
            }
        }
    }

    fn send(&self, server: ServerId, command: Command) {
        if let Some(handle) = self.servers.get(&server) {
            // A server that has gone has no documents to sync.
            let _ = handle.commands.unbounded_send(command);
        }
    }

    /// The position encoding `server` chose, once it has initialized.
    pub fn position_encoding(&self, server: ServerId) -> Option<PositionEncoding> {
        self.session(server, |session| session.encoding)
    }

    /// What `server` said it can do, once it has initialized.
    pub fn capabilities(&self, server: ServerId) -> Option<ServerCapabilities> {
        self.session(server, |session| session.capabilities.clone())
    }

    fn session<T>(&self, server: ServerId, read: impl FnOnce(&Session) -> Option<T>) -> Option<T> {
        let handle = self.servers.get(&server)?;
        read(
            &handle
                .session
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
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

impl Drop for LspManager {
    /// Asks every server to shut down, without waiting for them.
    fn drop(&mut self) {
        for (_, handle) in self.servers.drain() {
            handle.stop(&mut self.retiring);
        }
    }
}

/// Whether `path` is within the workspace at `root`, by its components:
/// a path through `..` never is.
fn is_under(path: &Path, root: &Path) -> bool {
    root.is_absolute()
        && path.starts_with(root)
        && !path
            .components()
            .any(|component| component == Component::ParentDir)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
