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
//!   ([`fs::canonicalize`], off the main thread: a file by its folder, so a
//!   new file works too), so a launch through `..`, which fe-lsp refuses,
//!   or a symlink still syncs, and a file is under the root only if it
//!   really is.
//! - **Buffers.** Once a frame, after the draw, every buffer an editor
//!   shows, with a file under the root and a language, is opened on the
//!   manager once, however many panes show it, and closed when no view of
//!   it is left. A buffer that emitted events since the last frame (edits
//!   or a save) is nudged ([`LspManager::changed`]), at most once a frame,
//!   so its changes go out without waiting for fe-lsp's backstop.
//! - **Events.** The manager's sink sends them with
//!   `async_channel::Sender::send_blocking`, from the servers' threads,
//!   into one `cx.spawn` loop per workbench, as `ProjectPanel` consumes
//!   `ProjectWatcher`. Status and `window/showMessage` drive a status bar
//!   item for the active editor's server; diagnostics aren't this card's.
//! - **Commands.** `lsp.restart` and `lsp.show_log`, in the workbench's
//!   context, act on the active editor's server. Their handlers only name
//!   the op: the workbench does it.
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
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex, PoisonError, mpsc},
    time::{Duration, Instant},
};

use fe_buffer::{Buffer, BufferId};
use fe_command::{
    Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Invocation, Registry,
};
use fe_config::settings::LspSettings;
use fe_editor::EditorModel;
use fe_gpui::commands::RunCommand;
use fe_lsp::{
    KillSwitch, LOGIN_SHELL_TIMEOUT, LocalLauncher, LspEvent, LspManager, ServerId, ServerLauncher,
    ServerStatus,
};
use gpui::{
    App, AppContext as _, Context, Global, SharedString, Subscription, Task, WeakEntity, Window,
};

use crate::{RootChanged, StatusItem, StatusSide, TrustChanged, Workbench};

/// The status bar item's id.
pub(crate) const ITEM: &str = "lsp";

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
    /// `None` with no root, and once the window has closed.
    manager: Option<LspManager>,
    watch: Option<Watch>,
    /// The root the manager serves.
    root: Option<PathBuf>,
    /// Bumped with each manager, so a replaced one's events are dropped.
    generation: u64,
    sender: async_channel::Sender<(u64, LspEvent)>,
    events: async_channel::Receiver<(u64, LspEvent)>,
    documents: HashMap<BufferId, Document>,
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
    /// Each shown buffer's file, resolved, or `None` if it can't be.
    resolved: HashMap<BufferId, Option<PathBuf>>,
    /// The buffers whose files are being resolved.
    resolving: HashSet<BufferId>,
    /// Resolves the root for the next manager.
    _root: Option<Task<()>>,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// A buffer opened on the manager.
struct Document {
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
            servers: HashMap::new(),
            messages: HashMap::new(),
            settings: LspSettings::default(),
            shown: None,
            #[cfg(test)]
            local: false,
            resolved: HashMap::new(),
            resolving: HashSet::new(),
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
                        workbench.lsp_event(generation, event);
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
        let lsp = &mut self.lsp;
        lsp.documents.clear();
        lsp.servers.clear();
        lsp.messages.clear();
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
                            log::warn!("no language servers for {}: {error}", root.display())
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
        lsp.root = Some(root);
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
        let sink = move |event: LspEvent| {
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
        lsp.manager = Some(manager);
        if self.root_trusted() {
            self.apply_trust(true);
        }
    }

    /// Lets servers start for the root, or stops them, as it's trusted.
    fn apply_trust(&mut self, trusted: bool) {
        let root = self.lsp.root.clone();
        if let Some(manager) = &mut self.lsp.manager {
            manager.set_trust(move |path| trusted && Some(path) == root.as_deref());
        }
    }

    /// Passes reloaded `[lsp]` settings on: the manager applies what
    /// changed, and a manager for a new root starts with them. Workbenches
    /// opened from now on start with them too.
    pub(crate) fn reload_lsp_settings(&mut self, settings: &LspSettings, cx: &mut App) {
        set_lsp_settings(cx, settings.clone());
        self.lsp.settings = settings.clone();
        if let Some(manager) = &mut self.lsp.manager {
            manager.update_settings(settings);
        }
    }

    /// The frame hook, run after each draw: opens buffers newly shown,
    /// closes those no longer shown, nudges those that moved, and updates
    /// the status item.
    pub(crate) fn sync_lsp(&mut self, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        let lsp = &mut self.lsp;
        let (Some(manager), Some(root)) = (&mut lsp.manager, &lsp.root) else {
            return;
        };
        let documents = &mut lsp.documents;
        let watch = &lsp.watch;
        let mut shown = HashSet::new();
        let mut to_resolve = Vec::new();
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
            }
            let language = view.syntax().map(|syntax| syntax.language().name());
            match (documents.get(&buffer), language) {
                (Some(document), Some(language)) if document.language == language => continue,
                (Some(_), _) if view.is_loading_language() => continue,
                (None, None) => continue,
                _ => {}
            }
            documents.remove(&buffer);
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
            documents.insert(
                buffer,
                Document {
                    language,
                    server,
                    moved,
                },
            );
        }
        documents.retain(|&buffer, _| {
            shown.contains(&buffer) || {
                manager.close(buffer);
                false
            }
        });
        lsp.resolved.retain(|buffer, _| shown.contains(buffer));
        for (buffer, path) in to_resolve {
            let resolved = cx.background_spawn(async move { resolve(&path) });
            cx.spawn(async move |workbench, cx| {
                let resolved = resolved.await;
                workbench
                    .update(cx, |workbench, cx| {
                        workbench.lsp.resolving.remove(&buffer);
                        workbench.lsp.resolved.insert(buffer, resolved);
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
        self.show_lsp_status(cx);
    }

    fn lsp_event(&mut self, generation: u64, event: LspEvent) {
        let lsp = &mut self.lsp;
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
            // Diagnostics are card 7's.
            _ => {}
        }
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
        let Some(manager) = &mut lsp.manager else {
            return;
        };
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

/// `path` with its folder resolved, so through no `..` or symlinked
/// folder; `None` if its folder can't be, or it names no file.
fn resolve(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?;
    let folder = fs::canonicalize(path.parent()?).ok()?;
    Some(folder.join(name))
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

    /// What the server `name` for the root has logged.
    pub(crate) fn lsp_log(&self, name: &str) -> String {
        match (&self.lsp.manager, &self.lsp.root) {
            (Some(manager), Some(root)) => manager.log(name, root),
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
