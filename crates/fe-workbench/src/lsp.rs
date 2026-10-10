//! Language servers for the workbench (docs/decisions/lsp-client.md §8,
//! card 5): one fe-lsp [`LspManager`] per window, for its root.
//!
//! - **Starting.** The manager is made with the host's launcher
//!   ([`set_server_launcher`], else [`LocalLauncher`]) and the effective
//!   `[lsp]` settings ([`set_lsp_settings`], then each reload). With no
//!   root there's none, and nothing starts. Servers start lazily, on the
//!   first open file of their language (§3.2), and only once the root is
//!   trusted ([`Workbench::root_trusted`]). A new root gets a new manager;
//!   the old one's servers shut down.
//! - **Paths.** The root and each file go to the manager resolved
//!   ([`fs::canonicalize`], off the main thread: a file fully, so its own
//!   symlink is followed; only a file that doesn't exist yet is resolved
//!   by its folder, so a new file works too), so a launch through `..`,
//!   which fe-lsp refuses, or a symlink still syncs, and a file is under
//!   the root only if it really is. A path that fails to resolve is tried
//!   again at that buffer's next save, and every path at a root change
//!   (a root that fails, at the next save of any shown buffer).
//! - **Buffers.** Once a frame, after the draw, every buffer an editor
//!   shows, with a file under the root and a language, is opened on the
//!   manager once, however many panes show it, and closed when no view of
//!   it is left. Path aliases of one file (`src/main.rs`, `src/../src/main.rs`)
//!   are separate buffers but one server document: the first to open owns
//!   it, the others send nothing, and when the owner closes, a remaining
//!   buffer opens at the next frame with its own text. A buffer that emitted events since the last frame (edits
//!   or a save) is nudged ([`LspManager::changed`]), at most once a frame,
//!   so its changes go out without waiting for fe-lsp's backstop.
//! - **Events.** The manager's sink sends them with
//!   `async_channel::Sender::send_blocking`, from the servers' threads,
//!   into one `cx.spawn` loop per workbench, as `ProjectPanel` consumes
//!   `ProjectWatcher`. Status and `window/showMessage` drive a status bar
//!   item for the active editor's server.
//! - **Diagnostics** (§4.1, "Delivery to the UI"). A `Diagnostics` event goes
//!   to the [`fe_editor::diagnostics::DiagnosticSet`] of each open editor of
//!   its buffer, keyed by server, applied only if its `(session,
//!   incarnation)` is [`LspManager::active_identity`] as it's delivered and
//!   not one cleared there, and rebased from its buffer version. A
//!   `DiagnosticsCleared` empties the server's diagnostics there if they
//!   came from its identity, and has the set reject that identity from then
//!   on. A new manager's ids start over, so its editors' sets are emptied.
//! - **Commands.** `lsp.restart` and `lsp.show_log`, in the workbench's
//!   context, act on the active editor's server. Their handlers only name
//!   the op: the workbench does it.
//! - **Navigation.** Each manager comes with an [`LspNavigation`] in
//!   [`Workbench::navigation`] at [`PRECEDENCE`], over search's 0: go to
//!   definition, find references and workspace symbols come from a
//!   language's server once it's ready, and by search until then. It holds
//!   the manager weakly, sharing its lock, so a replaced or closed one's
//!   answers nothing.
//! - **Hover.** Every editor's [`fe_gpui::HoverProvider`] asks the current
//!   manager ([`LspManager::hover`]), which sends it to the server its
//!   buffer is open on. Anything but an answer, such as no server, a
//!   timeout or an error, shows nothing.
//! - **Completion.** The same provider asks for an editor's completions
//!   ([`LspManager::completion`]), resolves the selected one, and reports
//!   the server's trigger characters for the buffer.
//! - **Shutdown.** Closing the window shuts its servers down (§3.4) on
//!   their threads. Quitting shuts every workbench's down, and blocks until
//!   each closed window's and each workbench's server has stopped, for as
//!   long as a local server's launch and fe-lsp's sequence take at most:
//!   the login shell (2 s), `shutdown` (2 s), `exit` (1 s), then its forced
//!   stop, `kill`. GPUI waits only 200 ms for a quit hook's future, so the
//!   wait is in the hook itself. Stopping a manager's servers, as quitting
//!   does, cancels a launch in progress through its [`fe_lsp::LaunchCancel`], and
//!   quitting force-stops ([`KillSwitch`]) what outlasts the wait, such as
//!   a server still awaiting its `initialize` reply, and waits a little
//!   longer for that. A host's launcher must honour the cancel or bound
//!   its own launch time ([`set_server_launcher`]); one that does neither
//!   can't be preempted, and its server may outlive fastedit.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex, PoisonError, Weak, mpsc},
    time::{Duration, Instant},
};

use fe_buffer::{Buffer, BufferEvent, BufferId, Version};
use fe_command::{
    Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Invocation, Registry,
};
use fe_config::settings::LspSettings;
use fe_editor::completion::{CompletionItem, CompletionList, CompletionTrigger};
use fe_editor::{
    EditorModel,
    diagnostics::{Diagnostic, DiagnosticTag, Severity},
};
use fe_gpui::commands::RunCommand;
use fe_lsp::{
    KillSwitch, LOGIN_SHELL_TIMEOUT, LocalLauncher, LspEvent, LspManager, LspNavigation, ServerId,
    ServerLauncher, ServerStatus,
};
use gpui::{
    App, AppContext as _, Context, Global, SharedString, Subscription, Task, WeakEntity, Window,
};

use crate::{RootChanged, StatusItem, StatusSide, TrustChanged, Workbench};

/// The status bar item's id.
pub(crate) const ITEM: &str = "lsp";

/// [`LspNavigation`]'s precedence in the workbench's navigation (§5).
pub(crate) const PRECEDENCE: i32 = 100;

/// How many crash restarts fe-lsp allows within its window (§3.5).
const RESTARTS: u32 = 5;

/// The launcher a host set.
struct Launcher(Arc<dyn ServerLauncher>);

impl Global for Launcher {}

/// The effective `[lsp]` settings, as the host or a reload last gave them.
struct Settings(LspSettings);

impl Global for Settings {}

/// Has workbenches opened from now on start their language servers with
/// `launcher`, to sandbox, proxy or refuse them, rather than spawning them
/// with fe-lsp's [`LocalLauncher`]. Call it before the workbench opens.
///
/// The launcher must honour its [`fe_lsp::LaunchCancel`] or bound its own launch
/// time. Stopping a server, or quitting, fires the cancel of a launch in
/// progress, and the launch should then return promptly; quitting also
/// force-stops servers that outlast [`SHUT_DOWN_WITHIN`]. A launcher that
/// does neither can't be preempted: quitting gives up on it after
/// [`KILL_WITHIN`] more, and its server may outlive fastedit.
pub fn set_server_launcher(cx: &mut App, launcher: Arc<dyn ServerLauncher>) {
    cx.set_global(Launcher(launcher));
}

/// The effective `[lsp]` settings workbenches opened from now on start
/// with, the defaults until it's called. Call it before the workbench
/// opens; [`Workbench::reload_settings`] applies later changes.
pub fn set_lsp_settings(cx: &mut App, settings: LspSettings) {
    cx.set_global(Settings(settings));
}

/// How long quitting waits for servers to stop: the longest a
/// [`LocalLauncher`] launch blocks ([`LOGIN_SHELL_TIMEOUT`], capturing the
/// login shell's `PATH`; fe-lsp sees a stop only once the launch returns,
/// then kills what it launched), then fe-lsp's shutdown sequence (§3.4):
/// 2 s for the `shutdown` answer, 1 s for the process to go after `exit`,
/// then a `kill`, with 1 s of margin for the kill and the thread's last
/// status. So a local server that has finished initializing is always
/// stopped or killed by then. Whatever outlasts it is force-stopped.
const SHUT_DOWN_WITHIN: Duration = Duration::from_secs(LOGIN_SHELL_TIMEOUT.as_secs() + 2 + 1 + 1);

/// How much longer quitting waits for the servers it force-stopped.
const KILL_WITHIN: Duration = Duration::from_secs(1);

/// What quitting did to the servers that outlasted the graceful wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Quit {
    /// Servers still running when the graceful wait ran out, which were
    /// force-stopped.
    pub(crate) forced: usize,
    /// Servers still running after the force-stop's wait too: a launcher
    /// that ignored its cancel.
    pub(crate) left: usize,
}

/// The language servers of every window: the managers dropped before the
/// app quits (a closed window's, a replaced root's), which quitting waits
/// for, and the workbenches whose managers it shuts down then.
#[derive(Default)]
struct Shutdowns {
    retiring: Vec<Watch>,
    workbenches: Vec<WeakEntity<Workbench>>,
}

impl Global for Shutdowns {}

/// A manager's servers, as its sink hears of them on their threads: those
/// started and not stopped yet, and those stopped for good.
#[derive(Default)]
struct Running {
    running: HashSet<ServerId>,
    stopped: HashSet<ServerId>,
}

/// What waiting for a manager's servers to stop needs: its own channel, so
/// waits for several managers don't take each other's news.
struct Watch {
    running: Arc<Mutex<Running>>,
    /// Gets a message each time one of them stops.
    stopped: mpsc::Receiver<()>,
    /// Force-stops them, and outlives their manager.
    switch: KillSwitch,
}

impl Watch {
    /// Counts `server` as running, unless it has stopped already: its
    /// first status may not have been sent yet.
    fn started(&self, server: ServerId) {
        let mut running = lock(&self.running);
        if !running.stopped.contains(&server) {
            running.running.insert(server);
        }
    }

    fn is_done(&self) -> bool {
        lock(&self.running).running.is_empty()
    }

    fn count(&self) -> usize {
        lock(&self.running).running.len()
    }

    /// Blocks until every server it watches has stopped, or `deadline`.
    fn wait(&self, deadline: Instant) {
        while !self.is_done() {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.stopped.recv_timeout(left) {
                Ok(()) => {}
                // Their threads have all ended.
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => return,
            }
        }
    }
}

/// Shuts every workbench's language servers down, and blocks until they
/// and those shut down before have stopped, within [`SHUT_DOWN_WITHIN`],
/// then force-stops what's left, within [`KILL_WITHIN`]: what quitting
/// does.
pub(crate) fn shut_down_all(cx: &mut App) {
    let quit = shut_down_all_within(cx, SHUT_DOWN_WITHIN, KILL_WITHIN);
    if quit.left > 0 {
        log::warn!(
            "quitting with {} language server(s) still running: a launcher \
             ignored its cancel, and its server may outlive fastedit",
            quit.left
        );
    }
}

/// [`shut_down_all`] with `grace` for the servers to stop and `force` more
/// once they've been killed.
pub(crate) fn shut_down_all_within(cx: &mut App, grace: Duration, force: Duration) -> Quit {
    let workbenches = std::mem::take(&mut cx.default_global::<Shutdowns>().workbenches);
    for workbench in workbenches {
        workbench
            .update(cx, |workbench, cx| workbench.retire_lsp(cx))
            .ok();
    }
    let retiring = std::mem::take(&mut cx.default_global::<Shutdowns>().retiring);
    let deadline = Instant::now() + grace;
    for watch in &retiring {
        watch.wait(deadline);
    }
    let pending: Vec<&Watch> = retiring.iter().filter(|watch| !watch.is_done()).collect();
    let forced = pending.iter().map(|watch| watch.count()).sum();
    for watch in &pending {
        watch.switch.kill();
    }
    let deadline = Instant::now() + force;
    for watch in &pending {
        watch.wait(deadline);
    }
    let left = pending.iter().map(|watch| watch.count()).sum();
    Quit { forced, left }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A workbench's language servers.
pub(crate) struct Lsp {
    /// `None` with no root, and once the window has closed. Shared with
    /// its [`LspNavigation`], on navigation's background threads.
    manager: Option<Arc<Mutex<LspManager>>>,
    watch: Option<Watch>,
    /// The root the manager serves.
    root: Option<PathBuf>,
    /// Bumped with each manager, so a replaced one's events are dropped.
    generation: u64,
    sender: async_channel::Sender<(u64, LspEvent)>,
    events: async_channel::Receiver<(u64, LspEvent)>,
    documents: HashMap<BufferId, Document>,
    /// The buffer holding each resolved path's document, kept in step with
    /// `documents`: path aliases of a file are separate buffers, but the
    /// server has one document for it.
    owners: HashMap<PathBuf, BufferId>,
    /// An alias a path is kept for after its owner changed language, until
    /// it opens the path.
    reserved: HashMap<PathBuf, BufferId>,
    /// Each server's name and latest status.
    servers: HashMap<ServerId, (String, ServerStatus)>,
    /// Each server's latest `window/showMessage`, shown in place of its
    /// status until its next one.
    messages: HashMap<ServerId, String>,
    /// The workbench's effective `[lsp]` settings, as last reloaded.
    settings: LspSettings,
    /// The item's text as last set.
    shown: Option<SharedString>,
    /// The manager was made with fe-lsp's own launcher.
    #[cfg(test)]
    local: bool,
    /// How many `Diagnostics` events the sinks have sent.
    #[cfg(test)]
    diagnostics_sent: Arc<std::sync::atomic::AtomicUsize>,
    /// Every event delivered, in order, with its manager's generation and,
    /// for `Diagnostics`, whether an editor applied it.
    #[cfg(test)]
    delivered: Vec<(u64, LspEvent, Option<bool>)>,
    /// Each shown buffer's file, resolved, or `None` if it can't be.
    resolved: HashMap<BufferId, Option<PathBuf>>,
    /// Every editor's hover and completion provider, following the
    /// manager.
    hover: Rc<LspHover>,
    /// The buffers whose files are being resolved.
    resolving: HashSet<BufferId>,
    /// The saves of each buffer whose file is being resolved or failed to
    /// resolve: a save is when a new file's folder has to exist.
    retry: HashMap<BufferId, fe_buffer::Subscription>,
    /// The root, when it failed to resolve and so has no manager.
    root_failed: Option<PathBuf>,
    /// The saves of each shown buffer, while `root_failed` is set.
    root_saves: HashMap<BufferId, fe_buffer::Subscription>,
    /// The failed root is being resolved again.
    root_retrying: bool,
    /// Resolves the root for the next manager.
    _root: Option<Task<()>>,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// A buffer opened on the manager.
struct Document {
    /// Its file, resolved.
    path: PathBuf,
    /// fe-syntax's name for it.
    language: &'static str,
    /// The server it went to, kept after the server stops.
    server: Option<ServerId>,
    /// The buffer's events, only to tell it moved since the last frame.
    moved: fe_buffer::Subscription,
}

impl Lsp {
    pub(crate) fn new() -> Self {
        let (sender, events) = async_channel::unbounded();
        Self {
            manager: None,
            watch: None,
            root: None,
            generation: 0,
            sender,
            events,
            documents: HashMap::new(),
            owners: HashMap::new(),
            reserved: HashMap::new(),
            servers: HashMap::new(),
            messages: HashMap::new(),
            settings: LspSettings::default(),
            shown: None,
            #[cfg(test)]
            local: false,
            #[cfg(test)]
            diagnostics_sent: Arc::default(),
            #[cfg(test)]
            delivered: Vec::new(),
            resolved: HashMap::new(),
            hover: Rc::default(),
            resolving: HashSet::new(),
            retry: HashMap::new(),
            root_failed: None,
            root_saves: HashMap::new(),
            root_retrying: false,
            _root: None,
            _tasks: Vec::new(),
            _subscriptions: Vec::new(),
        }
    }

    /// The active editor's server's name and status, if it has one.
    fn server(&self, buffer: Option<BufferId>) -> Option<(ServerId, &str, &ServerStatus)> {
        let server = self.documents.get(&buffer?)?.server?;
        let (name, status) = self.servers.get(&server)?;
        Some((server, name, status))
    }
}

impl Workbench {
    /// Starts following the language servers; call once, from `new`.
    pub(crate) fn start_lsp(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let events = self.lsp.events.clone();
        let applied = cx.spawn(async move |workbench, cx| {
            while let Ok(event) = events.recv().await {
                let mut batch = vec![event];
                while let Ok(more) = events.try_recv() {
                    batch.push(more);
                }
                let applied = workbench.update(cx, |workbench, cx| {
                    for (generation, event) in batch {
                        workbench.lsp_event(generation, event, cx);
                    }
                    workbench.show_lsp_status(cx);
                });
                if applied.is_err() {
                    break;
                }
            }
        });
        let trust = cx
            .subscribe_self(|workbench, TrustChanged(trusted), _| workbench.apply_trust(*trusted));
        let root = cx.subscribe_self(|workbench, _: &RootChanged, cx| {
            workbench.new_lsp_manager(cx);
            cx.notify();
        });
        // The window's gone, or the workbench: its servers shut down, and
        // quitting waits for them.
        let ours = window.window_handle().window_id();
        let workbench = cx.weak_entity();
        let closed = cx.on_window_closed(move |cx, closed| {
            if closed == ours {
                workbench
                    .update(cx, |workbench, cx| workbench.retire_lsp(cx))
                    .ok();
            }
        });
        let released = cx.on_release(|workbench, cx| workbench.retire_lsp(cx));
        let this = cx.weak_entity();
        let shutdowns = cx.default_global::<Shutdowns>();
        shutdowns
            .workbenches
            .retain(|workbench| workbench.upgrade().is_some());
        shutdowns.workbenches.push(this);
        if let Some(settings) = cx.try_global::<Settings>() {
            self.lsp.settings = settings.0.clone();
        }
        self.lsp._tasks.push(applied);
        self.lsp
            ._subscriptions
            .extend([trust, root, closed, released]);
        self.new_lsp_manager(cx);
    }

    /// Replaces the manager with one for the current root, or none: the old
    /// one's servers shut down, and the open buffers go to the new one at
    /// the next frame.
    fn new_lsp_manager(&mut self, cx: &mut Context<Self>) {
        self.retire_lsp(cx);
        // The old manager's servers and identities aren't comparable with
        // the new one's.
        for editor in self.open_editors(cx) {
            editor.update(cx, |view, cx| {
                view.model_mut().reset_diagnostics();
                cx.notify();
            });
        }
        let lsp = &mut self.lsp;
        lsp.documents.clear();
        lsp.owners.clear();
        lsp.reserved.clear();
        lsp.servers.clear();
        lsp.messages.clear();
        lsp.resolved.clear();
        lsp.resolving.clear();
        lsp.retry.clear();
        lsp.root_failed = None;
        lsp.root_saves.clear();
        lsp.root_retrying = false;
        lsp.generation += 1;
        lsp.root = None;
        lsp._root = None;
        let Some(root) = self.root.clone() else {
            return;
        };
        // Resolved first, off this thread: the buffers wait for it.
        let generation = lsp.generation;
        let resolved = cx.background_spawn(async move {
            let resolved = fs::canonicalize(&root);
            (root, resolved)
        });
        lsp._root = Some(cx.spawn(async move |workbench, cx| {
            let (root, resolved) = resolved.await;
            workbench
                .update(cx, |workbench, cx| {
                    if workbench.lsp.generation != generation {
                        return;
                    }
                    match resolved {
                        Ok(root) => workbench.start_lsp_manager(root, cx),
                        Err(error) => {
                            log::warn!("no language servers for {}: {error}", root.display());
                            workbench.lsp.root_failed = Some(root);
                        }
                    }
                    cx.notify();
                })
                .ok();
        }));
    }

    /// Makes the manager for `root`, resolved.
    fn start_lsp_manager(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let lsp = &mut self.lsp;
        lsp.root = Some(root.clone());
        let host = cx.try_global::<Launcher>().map(|host| host.0.clone());
        #[cfg(test)]
        {
            lsp.local = host.is_none();
        }
        let launcher = host.unwrap_or_else(|| Arc::new(LocalLauncher));
        let (sender, generation) = (lsp.sender.clone(), lsp.generation);
        let running = Arc::new(Mutex::new(Running::default()));
        let (stop, stopped) = mpsc::channel();
        let watched = running.clone();
        #[cfg(test)]
        let sent = lsp.diagnostics_sent.clone();
        let sink = move |event: LspEvent| {
            #[cfg(test)]
            if matches!(event, LspEvent::Diagnostics { .. }) {
                sent.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            if let LspEvent::Status { server, status, .. } = &event {
                let mut running = lock(&running);
                if matches!(status, ServerStatus::Stopped(_)) {
                    running.running.remove(server);
                    running.stopped.insert(*server);
                    stop.send(()).ok();
                } else if !running.stopped.contains(server) {
                    running.running.insert(*server);
                }
            }
            sender.send_blocking((generation, event)).ok();
        };
        #[cfg(not(test))]
        let mut manager = LspManager::new(launcher, sink);
        #[cfg(test)]
        let mut manager = match cx.try_global::<TestClock>() {
            Some(clock) => LspManager::with_clock(launcher, clock.0.clone(), sink),
            None => LspManager::new(launcher, sink),
        };
        manager.update_settings(&lsp.settings);
        manager.set_trust(|_| false);
        lsp.watch = Some(Watch {
            running: watched,
            stopped,
            switch: manager.kill_switch(),
        });
        let manager = Arc::new(Mutex::new(manager));
        let navigation = LspNavigation::new(Arc::downgrade(&manager), &root);
        *lsp.hover.manager.borrow_mut() = Arc::downgrade(&manager);
        lsp.manager = Some(manager);
        // Replaces the last manager's.
        self.navigation().add(Arc::new(navigation), PRECEDENCE);
        if self.root_trusted() {
            self.apply_trust(true);
        }
    }

    /// Lets servers start for the root, or stops them, as it's trusted.
    fn apply_trust(&mut self, trusted: bool) {
        let root = self.lsp.root.clone();
        if let Some(manager) = &self.lsp.manager {
            lock(manager).set_trust(move |path| trusted && Some(path) == root.as_deref());
        }
    }

    /// Passes reloaded `[lsp]` settings on: the manager applies what
    /// changed, and a manager for a new root starts with them. Workbenches
    /// opened from now on start with them too.
    pub(crate) fn reload_lsp_settings(&mut self, settings: &LspSettings, cx: &mut App) {
        set_lsp_settings(cx, settings.clone());
        self.lsp.settings = settings.clone();
        if let Some(manager) = &self.lsp.manager {
            lock(manager).update_settings(settings);
        }
    }

    /// The frame hook, run after each draw: opens buffers newly shown,
    /// closes those no longer shown, nudges those that moved, and updates
    /// the status item.
    pub(crate) fn sync_lsp(&mut self, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        let hover: Rc<dyn fe_gpui::HoverProvider> = self.lsp.hover.clone();
        let completion: Rc<dyn fe_gpui::CompletionProvider> = self.lsp.hover.clone();
        for editor in &editors {
            let set = editor.read(cx).hover_provider();
            if !set.is_some_and(|set| Rc::ptr_eq(set, &hover)) {
                editor.update(cx, |view, _| view.set_hover_provider(Some(hover.clone())));
            }
            let set = editor.read(cx).completion_provider();
            if !set.is_some_and(|set| Rc::ptr_eq(set, &completion)) {
                editor.update(cx, |view, _| {
                    view.set_completion_provider(Some(completion.clone()))
                });
            }
        }
        if self.lsp.manager.is_none() && self.lsp.root_failed.is_some() {
            self.retry_lsp_root(&editors, cx);
        }
        let lsp = &mut self.lsp;
        let (Some(manager), Some(root)) = (lsp.manager.clone(), &lsp.root) else {
            return;
        };
        // A failed file whose buffer was saved is resolved again.
        let saved: Vec<BufferId> = lsp
            .retry
            .iter()
            .filter(|(buffer, _)| lsp.resolved.get(buffer) == Some(&None))
            .filter(|(_, saves)| {
                saves
                    .drain()
                    .iter()
                    .any(|event| matches!(event, BufferEvent::Saved { .. }))
            })
            .map(|(&buffer, _)| buffer)
            .collect();
        for buffer in saved {
            lsp.resolved.remove(&buffer);
            lsp.retry.remove(&buffer);
        }
        let mut manager = lock(&manager);
        let documents = &mut lsp.documents;
        let owners = &mut lsp.owners;
        let reserved = &mut lsp.reserved;
        let mut released = Vec::new();
        let watch = &lsp.watch;
        let mut shown = HashSet::new();
        let mut to_resolve = Vec::new();
        let mut to_follow = Vec::new();
        // Each shown buffer's language, by its first view, for finding an
        // alias that can take over a released path.
        let mut languages: HashMap<BufferId, &'static str> = HashMap::new();
        for editor in &editors {
            let view = editor.read(cx);
            if let Some(language) = view.syntax().map(|syntax| syntax.language().name()) {
                languages
                    .entry(view.model().buffer().id())
                    .or_insert(language);
            }
        }
        for editor in editors {
            let view = editor.read(cx);
            let model = view.model();
            let buffer = model.buffer().id();
            let Some(path) = model.path() else {
                continue;
            };
            // A buffer goes by its first view, should another set another
            // language.
            if !shown.insert(buffer) {
                continue;
            }
            // Resolved while its language loads, if it does.
            if !lsp.resolved.contains_key(&buffer) && lsp.resolving.insert(buffer) {
                to_resolve.push((buffer, path.to_owned()));
                to_follow.push(editor.clone());
            }
            let language = view.syntax().map(|syntax| syntax.language().name());
            match (documents.get(&buffer), language) {
                (Some(document), Some(language)) if document.language == language => continue,
                (Some(_), _) if view.is_loading_language() => continue,
                (None, None) => continue,
                _ => {}
            }
            if let Some(document) = documents.remove(&buffer) {
                owners.remove(&document.path);
                released.push(document.path);
            }
            manager.close(buffer);
            let Some(language) = language else {
                continue;
            };
            let Some(Some(path)) = lsp
                .resolved
                .get(&buffer)
                .filter(|path| path.as_ref().is_some_and(|path| path.starts_with(root)))
            else {
                continue;
            };
            let path = path.clone();
            // The server has one document per path: a buffer that is an
            // alias of an owned file waits for its owner to close.
            if owners.contains_key(&path) {
                continue;
            }
            // An owner whose language changed hands the path to a waiting
            // alias a server handles, which opens it at the next frame.
            if released.contains(&path) && !reserved.contains_key(&path) {
                let alias = lsp.resolved.iter().find(|&(&other, resolved)| {
                    other != buffer
                        && resolved.as_ref() == Some(&path)
                        && !documents.contains_key(&other)
                        && languages.get(&other).is_some_and(|language| {
                            manager.language_server(root, language).is_some()
                        })
                });
                if let Some((&alias, _)) = alias {
                    reserved.insert(path, alias);
                    continue;
                }
            }
            match reserved.get(&path) {
                Some(&alias) if alias == buffer => {
                    reserved.remove(&path);
                }
                Some(alias) if languages.contains_key(alias) && !documents.contains_key(alias) => {
                    continue;
                }
                Some(_) => {
                    reserved.remove(&path);
                }
                None => {}
            }
            let (synced, moved, text, version) = editor.update(cx, |view, _| {
                let mut buffer = view.model_mut().buffer_mut();
                let synced = buffer.subscribe();
                (
                    synced,
                    buffer.subscribe(),
                    buffer.text().clone(),
                    buffer.version(),
                )
            });
            let opened = manager.open(Some(root), buffer, text, version, synced, &path, language);
            let server = opened.unwrap_or_else(|error| {
                log::warn!("{}: {error}", path.display());
                None
            });
            if let (Some(server), Some(watch)) = (server, watch) {
                watch.started(server);
            }
            owners.insert(path.clone(), buffer);
            documents.insert(
                buffer,
                Document {
                    path,
                    language,
                    server,
                    moved,
                },
            );
        }
        documents.retain(|&buffer, document| {
            shown.contains(&buffer) || {
                manager.close(buffer);
                owners.remove(&document.path);
                released.push(document.path.clone());
                false
            }
        });
        // A path freed while another shown buffer resolves to it: that
        // buffer is opened at the next frame.
        let handed_over = released.iter().any(|path| {
            !owners.contains_key(path)
                && lsp.resolved.iter().any(|(buffer, resolved)| {
                    shown.contains(buffer) && resolved.as_ref() == Some(path)
                })
        });
        lsp.resolved.retain(|buffer, _| shown.contains(buffer));
        lsp.retry.retain(|buffer, _| shown.contains(buffer));
        // Subscribed before the resolution starts, so a save while it runs
        // retries it.
        for editor in to_follow {
            let saves = editor.update(cx, |view, _| view.model_mut().buffer_mut().subscribe());
            lsp.retry
                .insert(editor.read(cx).model().buffer().id(), saves);
        }
        let generation = lsp.generation;
        for (buffer, path) in to_resolve {
            let resolved = cx.background_spawn(async move { resolve(&path) });
            cx.spawn(async move |workbench, cx| {
                let resolved = resolved.await;
                workbench
                    .update(cx, |workbench, cx| {
                        let lsp = &mut workbench.lsp;
                        if lsp.generation != generation {
                            return;
                        }
                        lsp.resolving.remove(&buffer);
                        if resolved.is_some() {
                            lsp.retry.remove(&buffer);
                        }
                        lsp.resolved.insert(buffer, resolved);
                        cx.notify();
                    })
                    .ok();
            })
            .detach();
        }
        for (&buffer, document) in documents.iter_mut() {
            if !document.moved.drain().is_empty() {
                manager.changed(buffer);
            }
            if let Some(server) = manager.server_of(buffer) {
                document.server = Some(server);
            }
        }
        if handed_over {
            cx.notify();
        }
        self.show_lsp_status(cx);
    }

    /// With the root failed to resolve and so no manager: resolves it again
    /// at the next save of a shown buffer.
    fn retry_lsp_root(
        &mut self,
        editors: &[gpui::Entity<fe_gpui::EditorView>],
        cx: &mut Context<Self>,
    ) {
        let mut shown = HashSet::new();
        let mut saved = false;
        for editor in editors {
            let buffer = editor.read(cx).model().buffer().id();
            shown.insert(buffer);
            let lsp = &mut self.lsp;
            saved |= lsp
                .root_saves
                .entry(buffer)
                .or_insert_with(|| {
                    editor.update(cx, |view, _| view.model_mut().buffer_mut().subscribe())
                })
                .drain()
                .iter()
                .any(|event| matches!(event, BufferEvent::Saved { .. }));
        }
        let lsp = &mut self.lsp;
        lsp.root_saves.retain(|buffer, _| shown.contains(buffer));
        let Some(root) = lsp.root_failed.clone() else {
            return;
        };
        if !saved || lsp.root_retrying {
            return;
        }
        lsp.root_retrying = true;
        let generation = lsp.generation;
        let resolved = cx.background_spawn(async move { fs::canonicalize(&root) });
        lsp._root = Some(cx.spawn(async move |workbench, cx| {
            let resolved = resolved.await;
            workbench
                .update(cx, |workbench, cx| {
                    if workbench.lsp.generation != generation {
                        return;
                    }
                    workbench.lsp.root_retrying = false;
                    if let Ok(root) = resolved {
                        workbench.lsp.root_failed = None;
                        workbench.lsp.root_saves.clear();
                        workbench.start_lsp_manager(root, cx);
                    }
                    cx.notify();
                })
                .ok();
        }));
    }

    fn lsp_event(&mut self, generation: u64, event: LspEvent, cx: &mut Context<Self>) {
        let lsp = &mut self.lsp;
        #[cfg(test)]
        lsp.delivered.push((generation, event.clone(), None));
        if generation != lsp.generation {
            return;
        }
        match event {
            LspEvent::Status {
                server,
                name,
                status,
            } => {
                lsp.messages.remove(&server);
                lsp.servers.insert(server, (name, status));
            }
            LspEvent::ShowMessage {
                server,
                name,
                typ,
                message,
            } => {
                log::info!("{name} says ({typ:?}): {message}");
                lsp.messages.insert(server, message);
            }
            LspEvent::Diagnostics {
                buffer,
                session,
                incarnation,
                version,
                server,
                items,
            } => {
                // Read as it's delivered: whatever's still queued behind a
                // close and reopen, or a restart, is no longer active.
                let active = (lsp.manager.as_ref())
                    .and_then(|manager| lock(manager).active_identity(buffer, server));
                let items: Vec<_> = items.into_iter().map(diagnostic).collect();
                let mut applied = false;
                for editor in self.editors_of(buffer, cx) {
                    applied |= editor.update(cx, |view, cx| {
                        let identity = (session, incarnation);
                        let model = view.model_mut();
                        let applied = model.apply_diagnostics(
                            server,
                            identity,
                            version,
                            items.clone(),
                            active,
                        );
                        if applied {
                            cx.notify();
                        }
                        applied
                    });
                }
                #[cfg(test)]
                if let Some(delivered) = self.lsp.delivered.last_mut() {
                    delivered.2 = Some(applied);
                }
                #[cfg(not(test))]
                let _ = applied;
            }
            LspEvent::DiagnosticsCleared {
                buffer,
                session,
                incarnation,
                server,
            } => {
                for editor in self.editors_of(buffer, cx) {
                    editor.update(cx, |view, cx| {
                        if view
                            .model_mut()
                            .clear_diagnostics(server, (session, incarnation))
                        {
                            cx.notify();
                        }
                    });
                }
            }
            _ => {}
        }
    }

    /// The open editors showing `buffer`.
    fn editors_of(&self, buffer: BufferId, cx: &App) -> Vec<gpui::Entity<fe_gpui::EditorView>> {
        let mut editors = self.open_editors(cx);
        editors.retain(|editor| editor.read(cx).model().buffer().id() == buffer);
        editors
    }

    /// Sets the status item for the active editor's server, or removes it.
    fn show_lsp_status(&mut self, cx: &mut Context<Self>) {
        let buffer = self
            .active_editor(cx)
            .map(|editor| editor.read(cx).model().buffer().id());
        let lsp = &self.lsp;
        let text = lsp.server(buffer).and_then(|(server, name, status)| {
            if let Some(message) = lsp.messages.get(&server) {
                return Some(message.clone());
            }
            let status = match status {
                ServerStatus::Starting => "starting…".to_owned(),
                ServerStatus::Indexing { title } => format!("indexing {title}"),
                ServerStatus::Restarting(n) => format!("restarting ({n}/{RESTARTS})"),
                ServerStatus::Stopped(Some(error)) => format!("stopped: {error}"),
                ServerStatus::Stopped(None) => "stopped".to_owned(),
                _ => return None,
            };
            Some(format!("{name}: {status}"))
        });
        let text = text.map(SharedString::from);
        if text == self.lsp.shown {
            return;
        }
        self.lsp.shown = text.clone();
        self.status_bar().clone().update(cx, |bar, cx| match text {
            Some(text) => bar.set_item(
                StatusItem {
                    id: ITEM.into(),
                    side: StatusSide::Right,
                    priority: 0,
                    text,
                    command: Invocation::of("lsp.show_log").ok(),
                },
                cx,
            ),
            None => bar.remove_item(ITEM, cx),
        });
    }

    /// Restarts the active editor's server, or opens its log in a tab.
    pub(crate) fn run_lsp_op(&mut self, op: LspOp, window: &mut Window, cx: &mut Context<Self>) {
        let buffer = self
            .active_editor(cx)
            .map(|editor| editor.read(cx).model().buffer().id());
        let lsp = &mut self.lsp;
        let (Some((server, name, _)), Some(root)) = (lsp.server(buffer), lsp.root.clone()) else {
            return;
        };
        let name = name.to_owned();
        let Some(manager) = lsp.manager.clone() else {
            return;
        };
        let mut manager = lock(&manager);
        match op {
            LspOp::Restart => match manager.restart(&name, &root) {
                Ok(Some(restarted)) => {
                    if let Some(watch) = &lsp.watch {
                        watch.started(restarted);
                    }
                    // Until its own status comes.
                    lsp.servers
                        .insert(restarted, (name.clone(), ServerStatus::Starting));
                    for document in lsp.documents.values_mut() {
                        if document.server == Some(server) {
                            document.server = Some(restarted);
                        }
                    }
                }
                Ok(None) => log::info!("{name} isn't restarted: it doesn't apply here now"),
                Err(error) => log::error!("can't restart {name}: {error}"),
            },
            LspOp::ShowLog => {
                let log = Buffer::from_file_contents(&manager.log(&name, &root));
                self.pane()
                    .update(cx, |pane, cx| pane.add(EditorModel::new(log, None), cx));
                let focus = gpui::Focusable::focus_handle(self, cx);
                window.focus(&focus, cx);
            }
        }
        cx.notify();
    }

    /// Shuts every server down (§3.4), on their threads, and has quitting
    /// wait for them.
    fn retire_lsp(&mut self, cx: &mut App) {
        self.lsp.manager = None;
        if let Some(watch) = self.lsp.watch.take() {
            let retiring = &mut cx.default_global::<Shutdowns>().retiring;
            retiring.retain(|watch| !watch.is_done());
            retiring.push(watch);
        }
    }
}

/// Hover text and completions for an editor, from the server its buffer is
/// open on, through whichever manager the workbench has now: none (no root,
/// or a closed window) answers nothing.
#[derive(Default)]
struct LspHover {
    manager: RefCell<Weak<Mutex<LspManager>>>,
}

impl fe_gpui::HoverProvider for LspHover {
    fn hover(&self, buffer: BufferId, version: Version, offset: usize) -> fe_gpui::HoverFuture {
        let Some(manager) = self.manager.borrow().upgrade() else {
            return Box::pin(async { None });
        };
        // Sent under the lock; awaited without it.
        let reply = lock(&manager).hover(buffer, version, offset);
        Box::pin(async move {
            match reply.await {
                Ok(hover) => hover.map(|hover| fe_gpui::HoverContent {
                    text: hover.text,
                    range: hover.range,
                }),
                Err(error) => {
                    log::debug!("no hover: {error}");
                    None
                }
            }
        })
    }
}

impl fe_gpui::CompletionProvider for LspHover {
    fn trigger_characters(&self, buffer: BufferId) -> Vec<String> {
        match self.manager.borrow().upgrade() {
            Some(manager) => lock(&manager).trigger_characters(buffer),
            None => Vec::new(),
        }
    }

    fn completions(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
        trigger: CompletionTrigger,
    ) -> fe_gpui::CompletionFuture {
        let Some(manager) = self.manager.borrow().upgrade() else {
            return Box::pin(async { None });
        };
        let trigger = match trigger {
            CompletionTrigger::Invoked => fe_lsp::CompletionTrigger::Invoked,
            CompletionTrigger::Character(c) => fe_lsp::CompletionTrigger::Character(c),
            CompletionTrigger::Incomplete => fe_lsp::CompletionTrigger::Incomplete,
        };
        // Sent under the lock; awaited without it.
        let reply = lock(&manager).completion(buffer, version, offset, trigger);
        Box::pin(async move {
            match reply.await {
                Ok(completions) => completions.map(|completions| CompletionList {
                    items: completions.items.into_iter().map(completion_item).collect(),
                    is_incomplete: completions.is_incomplete,
                }),
                Err(error) => {
                    log::debug!("no completions: {error}");
                    None
                }
            }
        })
    }

    fn resolve(&self, item: CompletionItem) -> fe_gpui::ResolveFuture {
        let manager = self.manager.borrow().upgrade();
        let lsp =
            (item.data.as_ref()).and_then(|data| data.downcast_ref::<fe_lsp::CompletionItem>());
        let (Some(manager), Some(lsp)) = (manager, lsp) else {
            return Box::pin(async { item });
        };
        let reply = lock(&manager).resolve(lsp.clone());
        Box::pin(async move {
            match reply.await {
                Ok(resolved) => completion_item(resolved),
                Err(error) => {
                    log::debug!("not resolved: {error}");
                    item
                }
            }
        })
    }
}

/// fe-lsp's completion item as fe-editor's, holding it to resolve.
fn completion_item(item: fe_lsp::CompletionItem) -> CompletionItem {
    // `labelDetails`' detail right after the label, its description after
    // a space.
    let description =
        (item.label_description.as_ref()).map(|description| format!(" {description}"));
    let label_detail = match (&item.label_detail, description) {
        (None, None) => None,
        (detail, description) => {
            Some(detail.clone().unwrap_or_default() + &description.unwrap_or_default())
        }
    };
    let edit = |edit: &fe_lsp::CompletionEdit| (edit.range.clone(), edit.text.clone());
    CompletionItem {
        label: item.label.clone(),
        label_detail,
        kind: item.kind.map(|kind| fe_lsp::kind_name(kind).to_owned()),
        detail: item.detail.clone(),
        documentation: item.documentation.clone(),
        filter_text: item.filter_text.clone(),
        sort_text: item.sort_text.clone(),
        edit: edit(&item.edit),
        additional_edits: item.additional_edits.iter().map(edit).collect(),
        data: Some(Rc::new(item)),
    }
}

/// fe-lsp's diagnostic as fe-editor keeps it: one with no severity is an
/// error, as VS Code reads it.
fn diagnostic(item: fe_lsp::Diagnostic) -> (std::ops::Range<usize>, Diagnostic) {
    let severity = match item.severity {
        None | Some(fe_lsp::Severity::Error) => Severity::Error,
        Some(fe_lsp::Severity::Warning) => Severity::Warning,
        Some(fe_lsp::Severity::Information) => Severity::Information,
        Some(fe_lsp::Severity::Hint) => Severity::Hint,
    };
    let tags = (item.tags.into_iter())
        .map(|tag| match tag {
            fe_lsp::DiagnosticTag::Unnecessary => DiagnosticTag::Unnecessary,
            fe_lsp::DiagnosticTag::Deprecated => DiagnosticTag::Deprecated,
        })
        .collect();
    let diagnostic = Diagnostic {
        severity,
        message: item.message,
        source: item.source,
        code: item.code,
        tags,
    };
    (item.range, diagnostic)
}

/// `path` fully resolved, so through no `..` or symlink, its own
/// included; `None` if it can't be (a dangling symlink too). A file that
/// doesn't exist yet is resolved by its folder.
fn resolve(path: &Path) -> Option<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(_) => fs::canonicalize(path).ok(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let name = path.file_name()?;
            let folder = fs::canonicalize(path.parent()?).ok()?;
            Some(folder.join(name))
        }
        Err(_) => None,
    }
}

/// What an LSP command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LspOp {
    Restart,
    ShowLog,
}

/// The [`CommandTarget`] of the LSP commands: the op to do.
pub(crate) struct LspCommands;

impl CommandTarget for LspCommands {
    type Tx = ();
    type Target<'a> = Option<LspOp>;
}

struct Commands(Rc<Registry<LspCommands>>);

impl Global for Commands {}

/// Registers the LSP commands, and has quitting shut every server down
/// first ([`shut_down_all`]).
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
    cx.on_app_quit(|cx| {
        shut_down_all(cx);
        async {}
    })
    .detach();
}

pub(crate) fn registry() -> Registry<LspCommands> {
    let commands = [
        ("lsp.restart", "Restart Language Server", LspOp::Restart),
        ("lsp.show_log", "Language Server Log", LspOp::ShowLog),
    ];
    let mut registry = Registry::new();
    for (id, title, op) in commands {
        registry
            .register(id, title, move |target: &mut Option<LspOp>, _: &Actor| {
                *target = Some(op);
                Ok(Dispatched::new(Effect::Notify))
            })
            .expect("LSP command IDs are valid and distinct")
            .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    }
    registry
}

/// The op of `run`, if it's an LSP command that applies in its context.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<LspOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}

#[cfg(test)]
impl Workbench {
    /// Whether there's a manager, and it was made with fe-lsp's own
    /// launcher.
    pub(crate) fn lsp_manager(&self) -> Option<bool> {
        self.lsp.manager.as_ref().map(|_| self.lsp.local)
    }

    /// The server the active editor's buffer went to, if it was opened on
    /// the manager.
    pub(crate) fn lsp_active_server(&self, cx: &App) -> Option<Option<ServerId>> {
        let editor = self.active_editor(cx)?;
        let buffer = editor.read(cx).model().buffer().id();
        self.lsp
            .documents
            .get(&buffer)
            .map(|document| document.server)
    }

    /// How many `Diagnostics` events the managers' sinks have sent.
    pub(crate) fn lsp_diagnostics_sent(&self) -> usize {
        (self.lsp.diagnostics_sent).load(std::sync::atomic::Ordering::SeqCst)
    }

    /// How many events wait in the channel for the workbench to take.
    pub(crate) fn lsp_queued(&self) -> usize {
        self.lsp.events.len()
    }

    /// Every event delivered so far, with its manager's generation and,
    /// for `Diagnostics`, whether an editor applied it.
    pub(crate) fn lsp_delivered(&self) -> Vec<(u64, LspEvent, Option<bool>)> {
        self.lsp.delivered.clone()
    }

    /// Delivers `event` as if it came through the channel.
    pub(crate) fn lsp_deliver(&mut self, generation: u64, event: LspEvent, cx: &mut Context<Self>) {
        self.lsp_event(generation, event, cx);
    }

    /// What the server `name` for the root has logged.
    pub(crate) fn lsp_log(&self, name: &str) -> String {
        match (&self.lsp.manager, &self.lsp.root) {
            (Some(manager), Some(root)) => lock(manager).log(name, root),
            _ => String::new(),
        }
    }
}

/// The clock the workbench's managers run on in tests, if set.
#[cfg(test)]
pub(crate) struct TestClock(pub(crate) Arc<dyn fe_lsp::Clock>);

#[cfg(test)]
impl Global for TestClock {}

#[cfg(test)]
#[path = "lsp_trust_tests.rs"]
mod trust_tests;
