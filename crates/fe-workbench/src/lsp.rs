//! Language servers for the workbench (docs/decisions/lsp-client.md §8,
//! card 5): one fe-lsp [`LspManager`] per window, for its root or for none.
//!
//! - **Starting.** The manager is made with the host's launcher
//!   ([`set_server_launcher`], else [`LocalLauncher`]) and the effective
//!   `[lsp]` settings ([`set_lsp_settings`], then each reload), and the
//!   servers extensions contribute that the user consented to
//!   ([`crate::lsp_consent`]), its launcher wrapped in a [`ConsentGate`]
//!   that checks each launch of one against its grant. Servers
//!   start lazily, on the first open file of their language (§3.2): for a
//!   file under the root, at the root, only once it's trusted
//!   ([`Workbench::root_trusted`]); for one outside it, or in a window with
//!   no root, at its marker root, only if that's inside a folder the user
//!   trusted, in any window ([`kiln_shell::trust::decided`]), followed as
//!   decisions change. An untrusted one gets no prompt: its file is
//!   highlighted alone, and the status item says why. A new root gets a
//!   new manager; the old one's servers shut down.
//! - **Trusted settings.** While the root is trusted, its manager gets the
//!   project file's `[lsp]` table unfiltered: the root's
//!   `.tenmoku/settings.toml` read as an [`LspLayer`], which keeps its last
//!   good table while the file has errors, laid over the user's file read
//!   the same way, over the defaults. The user's layer is the app's, kept
//!   for every window whatever the host's reads of the file give them, and
//!   the filtered settings the workbench was given, which have the project's
//!   keys narrowed, aren't a part of it. It's read on the
//!   background executor when the root becomes trusted, when a manager
//!   starts for a trusted root, at each reload, and when the file changes:
//!   the workbench watches it while the root is trusted, as the app's store
//!   narrows it and so reloads for no change to the keys it drops. Servers
//!   are let start only once the first read is applied, so none launches
//!   with the filtered settings first, and the documents opened before
//!   then are opened again, under the trusted size limit. A read that lands
//!   after the root, its trust or a newer read came is dropped. Untrusting
//!   goes back to the filtered settings, which are also the ones
//!   workbenches opened later start with.
//! - **Paths.** The root and each file go to the manager resolved
//!   ([`dunce::canonicalize`], off the main thread: a file fully, so its own
//!   symlink is followed; only a file that doesn't exist yet is resolved
//!   by its folder, so a new file works too), so a launch through `..`,
//!   which fe-lsp refuses, or a symlink still syncs, and a file is under
//!   the root only if it really is. A path that fails to resolve is tried
//!   again at that buffer's next save, and every path at a root change
//!   (a root that fails, at the next save of any shown buffer).
//! - **Buffers.** Once a frame, after the draw, every buffer an editor
//!   shows, with a file and a language, is opened on the manager once,
//!   its language fe-syntax's name for it or, for a file fe-syntax has no
//!   language for, its extension ([`routed_language`]),
//!   however many panes show it, and closed when no view of it is left. Path aliases of one file (`src/main.rs`, `src/../src/main.rs`)
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
//!   its buffer, keyed by server (a [`DiagnosticOwner`], beside the
//!   plugins' in [`crate::plugin_diagnostics`]), applied only if its `(session,
//!   incarnation)` is [`LspManager::active_identity`] as it's delivered and
//!   not one cleared there, and rebased from its buffer version. A
//!   `DiagnosticsCleared` empties the server's diagnostics there if they
//!   came from its identity, and has the set reject that identity from then
//!   on. A new manager's ids start over, so its editors' sets are emptied,
//!   and the plugins' shown again.
//! - **Commands.** `lsp.restart` and `lsp.show_log`, in the workbench's
//!   context, act on the active editor's server. `lsp.rename`,
//!   `lsp.code_actions`, `lsp.format_document` and `lsp.format_selection`,
//!   in the editor's, are each done in a module of their own
//!   (`lsp_rename`, `lsp_code_actions`, `lsp_format`). They're typed:
//!   `#[kiln::command]` methods of the workbench (`workbench_commands.rs`);
//!   the editor's run only where its context holds.
//! - **Workspace edits.** A server's `workspace/applyEdit`
//!   ([`LspEvent::ApplyEdit`]) is applied on the main thread, all or
//!   nothing (`workspace_edit`), and answered applied, or failed with why.
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
//!   the server's trigger characters for the buffer. It adds the user's
//!   snippets ([`crate::user_snippets`]) and extensions' after them, server
//!   or none, unless a punctuation trigger character asked
//!   ([`crate::extension_snippets`]).
//! - **Signature help.** So does it for an editor's signature help
//!   ([`LspManager::signature_help`]) and its trigger characters
//!   ([`LspManager::signature_help_triggers`]). fe-lsp keeps no parameter's
//!   documentation, so the popover shows the signature's alone.
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
//!   can't be preempted, and its server may outlive Tenmoku.

use std::{
    borrow::Cow,
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    pin::Pin,
    rc::Rc,
    sync::{Arc, Mutex, PoisonError, Weak, mpsc},
    time::{Duration, Instant},
};

use fe_buffer::{Buffer, BufferEvent, BufferId, Version};
use fe_command::Invocation;
use fe_config::settings::{LspLayer, LspSettings};
use fe_editor::completion::{CompletionItem, CompletionList, CompletionTrigger};
use fe_editor::{
    EditorModel,
    diagnostics::{Diagnostic, DiagnosticTag, RelatedInfo, Severity},
};
use fe_lsp::{
    FileDiagnostic, KillSwitch, LOGIN_SHELL_TIMEOUT, LocalLauncher, LspEvent, LspManager,
    LspNavigation, PositionEncoding, ServerId, ServerLauncher, ServerSpec, ServerStatus,
};
use gpui::{App, AppContext as _, Context, Global, Subscription, Task, WeakEntity, Window};

use crate::diagnostic_owner::{DiagnosticIdentity, DiagnosticOwner};
use crate::lsp_consent::ConsentGate;
use crate::open::settings_path;
use crate::workbench::toast::{self, Toast};
use crate::{RootChanged, TrustChanged, Workbench};
use kiln_shell::{Icon, StatusSide, StatusTone};

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

/// The servers extensions contribute that the user consented to.
#[derive(Default)]
struct ExtensionServers(Vec<ServerSpec>);

impl Global for ExtensionServers {}

/// The user's settings file's `lsp` table, for every window of the app:
/// kept at its last good value while the file has errors, whatever the
/// host's own reads of the file give a window, and whether the window has
/// a root or a manager. It follows the file itself, not the effective
/// settings reloads, which skip an edit a project's setting masks.
struct UserLayer {
    /// The file, kept apart from the layer so comparing it never waits on a
    /// read holding the layer's lock.
    path: PathBuf,
    layer: Arc<Mutex<LspLayer>>,
    /// Reads the layer again for each change of the file.
    _changes: Task<()>,
    /// What the watcher would send a change to.
    #[cfg(test)]
    changed: async_channel::Sender<()>,
}

impl Global for UserLayer {}

/// The app's layer for the user's file `path`, read off the main thread
/// now and whenever the file changes.
fn user_layer(cx: &mut App, path: PathBuf) -> Arc<Mutex<LspLayer>> {
    if let Some(user) = cx.try_global::<UserLayer>()
        && user.path == path
    {
        return user.layer.clone();
    }
    let layer = Arc::new(Mutex::new(LspLayer::user(path.clone())));
    let (sender, changes) = async_channel::unbounded::<()>();
    // Started off the main thread, and dropped there when the task is
    // cancelled, as the project's watcher is ([`Workbench::watch_project`]).
    let (release, released) = async_channel::bounded::<()>(1);
    let watched = path.clone();
    let wake = sender.clone();
    cx.background_spawn(async move {
        if cfg!(test) {
            return;
        }
        let watcher = fe_config::ConfigWatcher::new(vec![watched.clone()], move |_| {
            wake.send_blocking(()).ok();
        })
        .inspect_err(|error| log::warn!("not watching {}: {error}", watched.display()))
        .ok();
        released.recv().await.ok();
        drop(watcher);
    })
    .detach();
    // The first read comes with the layer.
    sender.send_blocking(()).ok();
    let held = layer.clone();
    let task = cx.spawn(async move |cx| {
        let _release = release;
        while changes.recv().await.is_ok() {
            while changes.try_recv().is_ok() {}
            let layer = held.clone();
            cx.background_spawn(async move { lock(&layer).read() })
                .await;
        }
    });
    cx.set_global(UserLayer {
        path,
        layer: layer.clone(),
        _changes: task,
        #[cfg(test)]
        changed: sender,
    });
    layer
}

/// The layers a read of the trusted settings gives.
pub(crate) struct Layers {
    pub(crate) user: Option<LspLayer>,
    pub(crate) project: LspLayer,
}

impl Layers {
    /// The project's table over the user's, over the defaults.
    fn apply(&self) -> LspSettings {
        let user = match &self.user {
            Some(user) => user.apply(&LspSettings::default()),
            None => LspSettings::default(),
        };
        self.project.apply(&user)
    }
}

/// Has workbenches opened from now on start their language servers with
/// `launcher`, to sandbox, proxy or refuse them, rather than spawning them
/// with fe-lsp's [`LocalLauncher`]. Call it before the workbench opens.
///
/// The launcher must honour its [`fe_lsp::LaunchCancel`] or bound its own launch
/// time. Stopping a server, or quitting, fires the cancel of a launch in
/// progress, and the launch should then return promptly; quitting also
/// force-stops servers that outlast [`SHUT_DOWN_WITHIN`]. A launcher that
/// does neither can't be preempted: quitting gives up on it after
/// [`KILL_WITHIN`] more, and its server may outlive Tenmoku.
pub fn set_server_launcher(cx: &mut App, launcher: Arc<dyn ServerLauncher>) {
    cx.set_global(Launcher(launcher));
}

/// The effective `[lsp]` settings workbenches opened from now on start
/// with, the defaults until it's called. Call it before the workbench
/// opens; [`Workbench::reload_settings`] applies later changes.
pub fn set_lsp_settings(cx: &mut App, settings: LspSettings) {
    cx.set_global(Settings(settings));
}

/// The language servers extensions contribute that the user consented to
/// ([`crate::lsp_consent`]), in extension order: every workbench's manager
/// starts with them, and those running now get them at once, restarting
/// only the servers whose spec changed ([`LspManager::set_extension_specs`])
/// or which run an invocation no longer granted ([`ConsentGate::stale`]).
/// The `[lsp]` settings apply over them. Not to be called while a workbench
/// is being updated.
pub(crate) fn hand_extension_servers(cx: &mut App, specs: Vec<ServerSpec>) {
    cx.set_global(ExtensionServers(specs.clone()));
    let workbenches = match cx.try_global::<Shutdowns>() {
        Some(shutdowns) => shutdowns.workbenches.clone(),
        None => return,
    };
    for workbench in workbenches {
        workbench
            .update(cx, |workbench, _| {
                let lsp = &workbench.lsp;
                if let Some(manager) = &lsp.manager {
                    let mut manager = lock(manager);
                    let restarted = manager.set_extension_specs(specs.clone());
                    // Those running what the user no longer grants, after
                    // they granted a change, run it as granted now. One
                    // just restarted for its new spec already does, and
                    // restarting it again would launch it twice.
                    for (name, root) in lsp.gate.iter().flat_map(|gate| gate.stale()) {
                        if restarted.contains(&(name.clone(), root.clone())) {
                            continue;
                        }
                        if let Err(error) = manager.restart(&name, &root) {
                            log::warn!("can't restart {name}: {error}");
                        }
                    }
                }
            })
            .ok();
    }
}

/// The language servers extensions contribute that the user consented to,
/// as the managers have them: none until [`crate::set_extension_servers`]
/// has worked out which.
pub fn extension_servers(cx: &App) -> Vec<ServerSpec> {
    cx.try_global::<ExtensionServers>()
        .map(|servers| servers.0.clone())
        .unwrap_or_default()
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

/// Held by a manager's sink, so it drops with the last of the manager and
/// its server threads.
struct Cleanup {
    /// Gets a message each time a server stops, and when the sink drops.
    stop: mpsc::Sender<()>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        self.stop.send(()).ok();
    }
}

/// What waiting for a manager's servers to stop needs: its own channel, so
/// waits for several managers don't take each other's news.
struct Watch {
    /// Gets a message each time one of them stops.
    stopped: mpsc::Receiver<()>,
    /// Force-stops them, tells which threads haven't ended, and outlives
    /// their manager.
    switch: KillSwitch,
}

impl Watch {
    /// Everything it watches has ended, processes reaped, not merely
    /// reported `Stopped`.
    fn is_done(&self) -> bool {
        self.switch.unfinished() == 0
    }

    /// The servers not done: those whose threads haven't ended, whether or
    /// not they've reported `Stopped`.
    fn count(&self) -> usize {
        self.switch.unfinished()
    }

    /// Blocks until every server it watches has ended, or `deadline`.
    fn wait(&self, deadline: Instant) {
        while !self.is_done() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            match self.stopped.recv_timeout(left) {
                Ok(()) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => return,
                // The sink has dropped; a thread may be a moment from its
                // end.
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(Duration::from_millis(1).min(left))
                }
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
             ignored its cancel, and its server may outlive Tenmoku",
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

/// A prepared rename's answer: its target, if there's one.
pub(crate) type RenameAnswer = Result<Option<fe_lsp::rename::RenameTarget>, fe_lsp::LspError>;
/// A rename's answer: the edits to make.
pub(crate) type RenameEdits = Result<Vec<fe_lsp::FileEdits>, fe_lsp::LspError>;

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A workbench's language servers.
pub(crate) struct Lsp {
    /// `None` while the root resolves, if it failed to, and once the
    /// window has closed. Shared with its [`LspNavigation`], on
    /// navigation's background threads.
    manager: Option<Arc<Mutex<LspManager>>>,
    /// The manager's launcher, which lets an extension's server run only
    /// what the user granted it.
    gate: Option<Arc<ConsentGate>>,
    watch: Option<Watch>,
    /// The window root the manager serves, if the window has one.
    pub(crate) root: Option<PathBuf>,
    /// The first marker root the manager's trust predicate was asked about
    /// since it was last taken: taken around each `open`, it's the root the
    /// document routes to. Only written under the manager's lock.
    asked: Arc<Mutex<Option<PathBuf>>>,
    /// When the manager started or the project watcher's last batch
    /// arrived, whichever is later: a file created since is `Created`.
    watched_at: Option<std::time::SystemTime>,
    /// Bumped with each manager, so a replaced one's events are dropped.
    generation: u64,
    sender: async_channel::Sender<(u64, LspEvent)>,
    events: async_channel::Receiver<(u64, LspEvent)>,
    documents: HashMap<BufferId, Document>,
    /// The buffer holding each resolved path's document on each server
    /// (`None`: no server), kept in step with `documents`: path aliases of a
    /// file are separate buffers, but a server has one document for it.
    owners: HashMap<Route, BufferId>,
    /// An alias a path on a server is kept for after its owner changed
    /// language, until it opens the path.
    reserved: HashMap<Route, BufferId>,
    /// Each server's name and latest status.
    servers: HashMap<ServerId, (String, ServerStatus)>,
    /// Each server's latest `window/showMessage`, shown in place of its
    /// status until its next one.
    messages: HashMap<ServerId, String>,
    /// Each server's diagnostics for files it doesn't have open, by the
    /// file's resolved path, as it sent them, until it sends none for the
    /// file, restarts or stops: the Problems panel lists them while no
    /// editor shows the file.
    pub(crate) file_diagnostics:
        HashMap<(ServerId, PathBuf), (PositionEncoding, Vec<FileDiagnostic>)>,
    /// Those files' texts on disk and rows, as the panel lists them.
    pub(crate) unopened: crate::dock::problems_panel::Unopened,
    /// The workbench's effective `[lsp]` settings, as last reloaded: the
    /// project layer's narrowed, for an untrusted root.
    settings: LspSettings,
    /// The root's project settings file's `lsp` table, as last read for a
    /// trusted root, kept at its last good value while the file has errors.
    /// Shared with the reads, on the background executor, so the main
    /// thread never locks it.
    project: Option<Arc<Mutex<LspLayer>>>,
    /// The project settings file, for its watcher.
    project_path: Option<PathBuf>,
    /// The user's settings file's `lsp` table, as for `project`: the app's,
    /// shared by every window's reads.
    user: Option<Arc<Mutex<LspLayer>>>,
    /// Bumped with each read of the trusted settings and each trust
    /// change, so a superseded read is dropped.
    trusted_read: u64,
    /// The manager has the trusted settings and lets servers start.
    gate_open: bool,
    /// Reads the trusted settings.
    _trusted: Option<Task<()>>,
    /// Holds the project settings file's watcher while the root is
    /// trusted, and reads the trusted settings again when it reports a
    /// change.
    _project_changes: Option<Task<()>>,
    /// What the watcher would send a change to.
    #[cfg(test)]
    project_changed: Option<async_channel::Sender<()>>,
    /// The item's text as last set, and its tooltip if it isn't the log's.
    shown: Option<(String, StatusTone, Option<String>)>,
    /// The manager was made with fe-lsp's own launcher.
    #[cfg(test)]
    local: bool,
    /// The settings last handed to a manager.
    #[cfg(test)]
    handed: Option<LspSettings>,
    /// How many `Diagnostics` events the sinks have sent.
    #[cfg(test)]
    diagnostics_sent: Arc<std::sync::atomic::AtomicUsize>,
    /// How many `Restarting` statuses the sinks have sent.
    #[cfg(test)]
    restarting_sent: Arc<std::sync::atomic::AtomicUsize>,
    /// Every event delivered, in order, with its manager's generation and,
    /// for `Diagnostics`, whether an editor applied it.
    #[cfg(test)]
    delivered: Vec<(u64, LspEvent, Option<bool>)>,
    /// Each shown buffer's file, resolved, or `None` if it can't be.
    pub(crate) resolved: HashMap<BufferId, Option<PathBuf>>,
    /// The path each buffer's file was last resolved from, or is being:
    /// a buffer saved under another name is resolved, and opened, anew.
    resolved_from: HashMap<BufferId, PathBuf>,
    /// Every editor's hover and completion provider, following the
    /// manager.
    hover: Rc<LspHover>,
    /// Every editor's caret highlights.
    highlights: highlights::Highlights,
    /// Every editor's document symbols request.
    symbols: symbols::Symbols,
    /// Every editor's documentation renderer.
    markdown: markdown::Renderers,
    /// Every editor's save hook, and whether `editor.format_on_save` is on
    /// (set from the settings each reload).
    pub(crate) format_on_save: Rc<std::cell::Cell<bool>>,
    /// Whether `editor.inlay_hints` is on (set from the settings each reload).
    pub(crate) inlay_hints_on: std::cell::Cell<bool>,
    /// Every editor's inlay hints.
    inlay_hints: inlay_hints::Slots,
    /// Whether `editor.code_lens` is on (set from the settings each reload).
    pub(crate) code_lens_on: std::cell::Cell<bool>,
    /// Whether `editor.linked_editing` is on (set from the settings each reload).
    pub(crate) linked_editing_on: std::cell::Cell<bool>,
    /// Whether `editor.format_on_type` and `editor.format_on_paste` are on
    /// (set from the settings each reload).
    pub(crate) format_on_type_on: std::cell::Cell<bool>,
    pub(crate) format_on_paste_on: std::cell::Cell<bool>,
    /// Every open editor's typing and pasting, heard for those.
    on_type: format_on_type::OnType,
    /// The active editor's linked editing.
    linked: linked_editing::Linked,
    /// Whether `editor.lightbulb` is on (set from the settings each reload).
    pub(crate) lightbulb_on: std::cell::Cell<bool>,
    /// The active editor's lightbulb.
    lightbulb: lightbulb::Lightbulb,
    /// The folding requests of each open buffer.
    folds: folds::Folds,
    /// The code lenses of each open buffer.
    lenses: lenses::Lenses,
    /// Whether `editor.links` is on (set from the settings each reload).
    pub(crate) links_on: std::cell::Cell<bool>,
    /// The document links of each open buffer.
    links: links::Links,
    /// Whether `editor.color_decorators` is on (set from the settings each reload).
    pub(crate) color_decorators_on: std::cell::Cell<bool>,
    /// The document colours of each open buffer.
    colors: colors::Colors,
    /// Whether `editor.semantic_highlighting` is on (set from the settings each reload).
    pub(crate) semantic_highlighting_on: std::cell::Cell<bool>,
    /// Every shown buffer's semantic tokens.
    semantic_tokens: semantic_tokens::Slots,
    /// `editor.code_actions_on_save`, read live by the save hook.
    pub(crate) code_actions_on_save: Rc<RefCell<Vec<String>>>,
    save_hook: Option<Rc<dyn fe_gpui::SaveHook>>,
    /// A rename's guard while one is running ([`Workbench::rename_symbol`]).
    pub(crate) rename: std::rc::Weak<()>,
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
    /// A save arrived while the root was being resolved again: if that
    /// fails, it's resolved once more.
    root_save_pending: bool,
    /// Resolves the root for the next manager.
    _root: Option<Task<()>>,
    /// The code actions being asked for.
    pub(crate) code_actioning: Option<Task<()>>,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// A server (by name, if the language has one) and a resolved path: what
/// one document is of.
type Route = (Option<String>, PathBuf);

/// The name of the server `language` is routed to, if any, trusted root or
/// not: documents wait for it while the root isn't trusted, so it holds the
/// path's document from then on.
fn server_name(manager: &LspManager, language: Option<&str>) -> Option<String> {
    manager.route_for(language?)
}

/// The language `view`'s buffer is routed to a server by: fe-syntax's name
/// for its grammar, or, for a file fe-syntax has no language for, its
/// extension in lowercase, which a server's `languages` may list (a
/// `[lsp.servers.zls]` with `languages = ["zig"]` takes `.zig` files).
/// `None` while its language loads, and for plain text otherwise: a file
/// fe-syntax has a language for but shown as plain text isn't routed, nor
/// is one set to plain text by hand.
pub(crate) fn routed_language(view: &fe_gpui::EditorView) -> Option<Cow<'static, str>> {
    if let Some(syntax) = view.syntax() {
        return Some(Cow::Borrowed(syntax.language().name()));
    }
    if view.is_loading_language() || view.is_language_chosen() {
        return None;
    }
    let path = view.model().path()?;
    if fe_syntax::language_name_for_path(path).is_some() {
        return None;
    }
    let extension = path.extension()?.to_str()?;
    Some(Cow::Owned(extension.to_ascii_lowercase()))
}

/// A buffer opened on the manager.
struct Document {
    /// Its file, resolved.
    path: PathBuf,
    /// The name of the server its language is handled by, if any.
    server_name: Option<String>,
    /// fe-syntax's name for it, or its extension: see [`routed_language`].
    language: Cow<'static, str>,
    /// The server it went to, kept after the server stops.
    server: Option<ServerId>,
    /// The folder its server runs in, or would: the window root for a file
    /// under it, else the file's marker root.
    root: Option<PathBuf>,
    /// The buffer's events, only to tell it moved since the last frame.
    moved: fe_buffer::Subscription,
}

impl Lsp {
    /// Whether owners are the documents the manager serves: each served
    /// document owns its path on its server, and one parked owns it only
    /// where none is served.
    #[cfg(test)]
    pub(crate) fn owners_match_manager(&self) -> bool {
        let Some(manager) = &self.manager else {
            return true;
        };
        let manager = lock(manager);
        let served = self.documents.iter().all(|(&buffer, document)| {
            manager.server_of(buffer).is_none()
                || self
                    .owners
                    .get(&(manager.route_of(buffer), document.path.clone()))
                    == Some(&buffer)
        });
        let parked = self.owners.iter().all(|(route, &buffer)| {
            manager.server_of(buffer).is_some()
                || !self.documents.iter().any(|(&other, document)| {
                    manager.server_of(other).is_some()
                        && (manager.route_of(other), document.path.clone()) == *route
                })
        });
        served && parked
    }

    /// The server each owned path is held on, by name, sorted.
    #[cfg(test)]
    pub(crate) fn owner_servers(&self) -> Vec<Option<String>> {
        let mut servers: Vec<_> = self
            .owners
            .keys()
            .map(|(server, _)| server.clone())
            .collect();
        servers.sort();
        servers
    }

    /// Each owner's server by the manager's own account, sorted, to compare
    /// with [`owner_servers`](Self::owner_servers).
    #[cfg(test)]
    pub(crate) fn actual_owner_servers(&self) -> Vec<Option<String>> {
        let Some(manager) = &self.manager else {
            return Vec::new();
        };
        let manager = lock(manager);
        let mut servers: Vec<_> = self
            .owners
            .values()
            .map(|&buffer| manager.route_of(buffer))
            .collect();
        servers.sort();
        servers
    }

    /// The aliases paths are kept for after their owners changed language.
    #[cfg(test)]
    pub(crate) fn reserved_buffers(&self) -> Vec<BufferId> {
        self.reserved.values().copied().collect()
    }

    pub(crate) fn new() -> Self {
        let (sender, events) = async_channel::unbounded();
        Self {
            manager: None,
            gate: None,
            rename: std::rc::Weak::new(),
            watch: None,
            root: None,
            asked: Arc::default(),
            watched_at: None,
            generation: 0,
            sender,
            events,
            documents: HashMap::new(),
            owners: HashMap::new(),
            reserved: HashMap::new(),
            servers: HashMap::new(),
            messages: HashMap::new(),
            file_diagnostics: HashMap::new(),
            unopened: Default::default(),
            settings: LspSettings::default(),
            project: None,
            project_path: None,
            user: None,
            trusted_read: 0,
            gate_open: false,
            _trusted: None,
            _project_changes: None,
            #[cfg(test)]
            project_changed: None,
            shown: None,
            #[cfg(test)]
            local: false,
            #[cfg(test)]
            handed: None,
            #[cfg(test)]
            diagnostics_sent: Arc::default(),
            #[cfg(test)]
            restarting_sent: Arc::default(),
            #[cfg(test)]
            delivered: Vec::new(),
            resolved: HashMap::new(),
            resolved_from: HashMap::new(),
            hover: Rc::default(),
            highlights: Default::default(),
            symbols: Default::default(),
            markdown: Default::default(),
            format_on_save: Rc::default(),
            inlay_hints_on: std::cell::Cell::new(true),
            inlay_hints: Default::default(),
            code_lens_on: std::cell::Cell::new(true),
            lightbulb_on: std::cell::Cell::new(true),
            linked_editing_on: std::cell::Cell::new(false),
            format_on_type_on: std::cell::Cell::new(false),
            format_on_paste_on: std::cell::Cell::new(false),
            on_type: Default::default(),
            linked: Default::default(),
            lightbulb: Default::default(),
            folds: Default::default(),
            lenses: Default::default(),
            links_on: std::cell::Cell::new(true),
            links: Default::default(),
            color_decorators_on: std::cell::Cell::new(true),
            colors: Default::default(),
            semantic_highlighting_on: std::cell::Cell::new(true),
            semantic_tokens: Default::default(),
            code_actions_on_save: Rc::default(),
            save_hook: None,
            resolving: HashSet::new(),
            retry: HashMap::new(),
            root_failed: None,
            root_saves: HashMap::new(),
            root_retrying: false,
            root_save_pending: false,
            _root: None,
            code_actioning: None,
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

    /// The manager, if there is one.
    pub(crate) fn manager(&self) -> Option<Arc<Mutex<LspManager>>> {
        self.manager.clone()
    }

    /// Drops `server`'s diagnostics for files it doesn't have open, or
    /// every server's: whether there were any.
    fn drop_file_diagnostics(&mut self, server: Option<ServerId>) -> bool {
        let before = self.file_diagnostics.len();
        let unopened = &mut self.unopened;
        self.file_diagnostics.retain(|(of, path), _| {
            let keep = server.is_some_and(|server| server != *of);
            if !keep {
                unopened.forget(path);
            }
            keep
        });
        self.file_diagnostics.len() != before
    }
}

/// The status item's text and tone for server `name` in `status`, or its
/// `message` if it has one: none for a server running with nothing to say.
pub(crate) fn status_text(
    name: &str,
    status: &ServerStatus,
    message: Option<&str>,
) -> Option<(String, StatusTone)> {
    if let Some(message) = message {
        return Some((message.to_owned(), StatusTone::Error));
    }
    let (status, tone) = match status {
        ServerStatus::Starting => ("starting…".to_owned(), StatusTone::Default),
        ServerStatus::Indexing { title } => (format!("indexing {title}"), StatusTone::Default),
        ServerStatus::Restarting(n) => {
            (format!("restarting ({n}/{RESTARTS})"), StatusTone::Warning)
        }
        ServerStatus::Stopped(Some(error)) => (format!("stopped: {error}"), StatusTone::Error),
        ServerStatus::Stopped(None) => ("stopped".to_owned(), StatusTone::Default),
        _ => return None,
    };
    Some((format!("{name}: {status}"), tone))
}

/// The language server's status item showing `text`.
pub(crate) fn lsp_status_item(text: String, tone: StatusTone) -> kiln_shell::StatusItem {
    let mut item = kiln_shell::StatusItem::new(ITEM, StatusSide::Right, 0, text)
        .icon(Icon::Server)
        .tone(tone)
        .tooltip("Show Language Server Log");
    if let Ok(command) = Invocation::of("lsp.show_log") {
        item = item.command(command);
    }
    item
}

/// The status item for a file whose server `name` doesn't start because
/// its marker root, `root`, isn't inside a trusted folder.
fn untrusted_status(name: &str, root: &Path) -> (String, StatusTone, Option<String>) {
    (
        format!("{name}: folder not trusted"),
        StatusTone::Warning,
        Some(format!(
            "{} isn't inside a trusted folder, so its language servers don't start",
            root.display()
        )),
    )
}

impl Workbench {
    /// Starts following the language servers; call once, from `new`.
    pub(crate) fn start_lsp(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        *self.lsp.hover.workbench.borrow_mut() = Some((cx.weak_entity(), cx.to_async()));
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
        let trust = cx.subscribe_self(|workbench, TrustChanged(trusted), cx| {
            workbench.apply_trust(*trusted, cx)
        });
        // Any folder's decision: a file's marker root outside the root may
        // be in it.
        let decisions = kiln_shell::trust::observe(cx, |workbench, cx| {
            workbench.set_lsp_trust(cx);
            cx.notify();
        });
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
            .extend([trust, decisions, root, closed, released]);
        self.new_lsp_manager(cx);
    }

    /// Closes `buffer`'s document on its server, if it has one, and forgets
    /// its file: the next frame opens it afresh under the path it has then.
    /// For a buffer about to be saved under another name, so the document
    /// under the old one hears of no save.
    pub(crate) fn lsp_release(&mut self, buffer: BufferId, cx: &mut Context<Self>) {
        let lsp = &mut self.lsp;
        lsp.resolved.remove(&buffer);
        lsp.resolved_from.remove(&buffer);
        lsp.resolving.remove(&buffer);
        if let Some(document) = lsp.documents.remove(&buffer) {
            let route = (document.server_name, document.path);
            if lsp.owners.get(&route) == Some(&buffer) {
                lsp.owners.remove(&route);
            }
        }
        if let Some(manager) = lsp.manager.clone() {
            lock(&manager).close(buffer);
        }
        cx.notify();
    }

    /// Replaces the manager with one for the current root, or for no root:
    /// the old one's servers shut down, and the open buffers go to the new
    /// one at the next frame.
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
        // The plugins' went with them: they're shown again.
        crate::plugin_diagnostics::reshow(cx);
        let lsp = &mut self.lsp;
        lsp.documents.clear();
        lsp.owners.clear();
        lsp.reserved.clear();
        lsp.servers.clear();
        lsp.messages.clear();
        let dropped = lsp.drop_file_diagnostics(None);
        lsp.resolved.clear();
        lsp.resolved_from.clear();
        lsp.resolving.clear();
        lsp.retry.clear();
        lsp.root_failed = None;
        lsp.root_saves.clear();
        lsp.root_retrying = false;
        lsp.root_save_pending = false;
        lsp.generation += 1;
        lsp.root = None;
        lsp._root = None;
        lsp.project = None;
        lsp.project_path = None;
        lsp.gate_open = false;
        lsp._project_changes = None;
        lsp.user = settings_path(cx).map(|path| user_layer(cx, path));
        if dropped {
            self.sync_problems(cx);
        }
        let lsp = &mut self.lsp;
        let Some(root) = self.root.clone() else {
            return self.start_lsp_manager(None, cx);
        };
        // Resolved first, off this thread: the buffers wait for it.
        let generation = lsp.generation;
        let resolved = cx.background_spawn(async move {
            let resolved = dunce::canonicalize(&root);
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
                        Ok(root) => workbench.start_lsp_manager(Some(root), cx),
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

    /// Makes the manager for `root`, resolved, or for a window with none.
    fn start_lsp_manager(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        let lsp = &mut self.lsp;
        lsp.root.clone_from(&root);
        lsp.watched_at = Some(std::time::SystemTime::now());
        if let Some(root) = &root {
            let path = fe_config::project_settings_path(root);
            lsp.project = Some(Arc::new(Mutex::new(LspLayer::project(path.clone()))));
            lsp.project_path = Some(path);
        }
        lsp.user = settings_path(cx).map(|path| user_layer(cx, path));
        let host = cx.try_global::<Launcher>().map(|host| host.0.clone());
        #[cfg(test)]
        {
            lsp.local = host.is_none();
        }
        let host = host.unwrap_or_else(|| Arc::new(LocalLauncher));
        let gate = Arc::new(ConsentGate::new(cx, host));
        lsp.gate = Some(gate.clone());
        let launcher: Arc<dyn ServerLauncher> = gate;
        let (sender, generation) = (lsp.sender.clone(), lsp.generation);
        let (stop, stopped) = mpsc::channel();
        let cleanup = Cleanup { stop };
        #[cfg(test)]
        let sent = lsp.diagnostics_sent.clone();
        #[cfg(test)]
        let restarting = lsp.restarting_sent.clone();
        let sink = move |mut event: LspEvent| {
            #[cfg(test)]
            if matches!(event, LspEvent::Diagnostics { .. }) {
                sent.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            #[cfg(test)]
            if matches!(
                event,
                LspEvent::Status {
                    status: ServerStatus::Restarting(_),
                    ..
                }
            ) {
                restarting.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            if let LspEvent::Status {
                status: ServerStatus::Stopped(_),
                ..
            } = &event
            {
                cleanup.stop.send(()).ok();
            }
            // Here, on the server's thread, in the order sent: the main
            // thread doesn't touch the disk for it.
            if let LspEvent::FileDiagnostics {
                path: Some(path), ..
            } = &mut event
                && let Some(resolved) = resolve(path)
            {
                *path = resolved;
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
        manager.set_extension_specs(extension_servers(cx));
        manager.update_settings(&lsp.settings);
        #[cfg(test)]
        {
            lsp.handed = Some(lsp.settings.clone());
        }
        manager.set_trust(|_| false);
        lsp.watch = Some(Watch {
            stopped,
            switch: manager.kill_switch(),
        });
        let manager = Arc::new(Mutex::new(manager));
        *lsp.hover.manager.borrow_mut() = Arc::downgrade(&manager);
        if let Some(root) = &root {
            let navigation = LspNavigation::new(Arc::downgrade(&manager), root);
            // Replaces the last manager's.
            self.navigation().add(Arc::new(navigation), PRECEDENCE);
        }
        *self.lsp.hover.executor.borrow_mut() = Some(cx.background_executor().clone());
        self.lsp.manager = Some(manager);
        self.set_lsp_trust(cx);
        if self.root_trusted() {
            self.apply_trust(true, cx);
        }
    }

    /// Gives the manager the trust predicate for where it stands now:
    /// servers start for the window root once its trusted settings are
    /// applied, and for another marker root if it's inside a folder the
    /// user trusted, in any window ([`kiln_shell::trust::decided`]), but
    /// for none while a trusted root's settings are read, so none launches
    /// with the filtered settings first. Again at each decision.
    fn set_lsp_trust(&mut self, cx: &mut Context<Self>) {
        let lsp = &self.lsp;
        let Some(manager) = lsp.manager.clone() else {
            return;
        };
        let (root, open) = (lsp.root.clone(), lsp.gate_open);
        let hold = !open && self.root_trusted();
        let decided = kiln_shell::trust::decisions(cx);
        let asked = lsp.asked.clone();
        lock(&manager).set_trust(move |path| {
            lock(&asked).get_or_insert_with(|| path.to_owned());
            match &root {
                _ if hold => false,
                Some(root) if path == root => open,
                _ => decided(path) == Some(true),
            }
        });
    }

    /// Lets servers start for the root once its trusted settings are read,
    /// or stops them and goes back to the filtered settings, as it's
    /// trusted.
    fn apply_trust(&mut self, trusted: bool, cx: &mut Context<Self>) {
        self.lsp.trusted_read += 1;
        if trusted {
            // Nothing starts until they're read.
            self.set_lsp_trust(cx);
            self.watch_project(cx);
            return self.read_trusted_settings(cx);
        }
        let lsp = &mut self.lsp;
        lsp.gate_open = false;
        lsp._project_changes = None;
        if lsp.drop_file_diagnostics(None) {
            self.sync_problems(cx);
        }
        self.set_lsp_trust(cx);
        let lsp = &mut self.lsp;
        if let Some(manager) = lsp.manager.clone() {
            let mut manager = lock(&manager);
            manager.update_settings(&lsp.settings);
            #[cfg(test)]
            {
                lsp.handed = Some(lsp.settings.clone());
            }
        }
    }

    /// Watches the root's project settings file while the root is trusted,
    /// reading the trusted settings again when it changes: the app's store
    /// narrows it, so a change to keys it drops reaches no reload. The
    /// watcher starts off the main thread. Not in tests, which hand changes
    /// to [`Workbench::lsp_project_changed`]: GPUI's test scheduler rejects
    /// wakeups from the watcher's thread.
    fn watch_project(&mut self, cx: &mut Context<Self>) {
        let lsp = &mut self.lsp;
        let Some(path) = lsp.project_path.clone() else {
            return;
        };
        if lsp._project_changes.is_some() {
            return;
        }
        let (sender, changes) = async_channel::unbounded::<()>();
        #[cfg(test)]
        {
            lsp.project_changed = Some(sender.clone());
        }
        // The watcher is made, held and dropped on the background executor:
        // dropping it joins its thread, which may be in the middle of
        // blocking file system work. The task below holds `release` and
        // drops it without waiting, whenever it's cancelled, which ends the
        // background task.
        let (release, released) = async_channel::bounded::<()>(1);
        cx.background_spawn(async move {
            if cfg!(test) {
                return;
            }
            let watcher = fe_config::ConfigWatcher::new(vec![path.clone()], move |_| {
                sender.send_blocking(()).ok();
            })
            .inspect_err(|error| log::warn!("not watching {}: {error}", path.display()))
            .ok();
            released.recv().await.ok();
            drop(watcher);
        })
        .detach();
        let generation = lsp.generation;
        lsp._project_changes = Some(cx.spawn(async move |workbench, cx| {
            let _release = release;
            while changes.recv().await.is_ok() {
                while changes.try_recv().is_ok() {}
                let read = workbench.update(cx, |workbench, cx| {
                    if workbench.lsp.generation == generation && workbench.root_trusted() {
                        workbench.read_trusted_settings(cx);
                    }
                });
                if read.is_err() {
                    break;
                }
            }
        }));
    }

    /// Reads the user's and the root's project settings files off the main
    /// thread, then lays the project's `lsp` table over the user's, hands
    /// them to the manager and lets servers start, unless the root, its
    /// trust or a newer read came first.
    fn read_trusted_settings(&mut self, cx: &mut Context<Self>) {
        let lsp = &mut self.lsp;
        let (Some(_), Some(project)) = (&lsp.manager, lsp.project.clone()) else {
            return;
        };
        lsp.trusted_read += 1;
        let (read, generation) = (lsp.trusted_read, lsp.generation);
        let user = lsp.user.clone();
        let layers = cx.background_spawn(async move {
            let user = user.map(|user| {
                let mut user = lock(&user);
                user.read();
                user.clone()
            });
            let mut project = lock(&project);
            project.read();
            Layers {
                user,
                project: project.clone(),
            }
        });
        lsp._trusted = Some(cx.spawn(async move |workbench, cx| {
            let layers = layers.await;
            workbench
                .update(cx, |workbench, cx| {
                    workbench.apply_trusted_settings(read, generation, &layers, cx)
                })
                .ok();
        }));
    }

    /// Applies the trusted settings, `layers`' project laid over its user,
    /// from read `read` for manager `generation`, if they're still
    /// current, and lets servers start for the root.
    ///
    /// Opening the gate, the documents opened while it was shut are opened
    /// again at the next frame: a document keeps the size limit it was
    /// opened under, and the trusted settings may set another.
    fn apply_trusted_settings(
        &mut self,
        read: u64,
        generation: u64,
        layers: &Layers,
        cx: &mut Context<Self>,
    ) {
        let lsp = &mut self.lsp;
        if lsp.trusted_read != read || lsp.generation != generation || !self.trust.trusted() {
            return;
        }
        let Some(manager) = lsp.manager.clone() else {
            return;
        };
        let settings = layers.apply();
        let mut manager = lock(&manager);
        if !lsp.gate_open {
            for &buffer in lsp.documents.keys() {
                manager.close(buffer);
            }
            lsp.documents.clear();
            lsp.owners.clear();
            lsp.reserved.clear();
            cx.notify();
        }
        manager.update_settings(&settings);
        #[cfg(test)]
        {
            lsp.handed = Some(settings.clone());
        }
        lsp.gate_open = true;
        drop(manager);
        self.set_lsp_trust(cx);
    }

    /// Passes reloaded `[lsp]` settings on: the manager applies what
    /// changed, the trusted settings read again for a trusted root, and a
    /// manager for a new root starts with them. Workbenches opened from now
    /// on start with them too, filtered, whatever this root's trust.
    pub(crate) fn reload_lsp_settings(&mut self, settings: &LspSettings, cx: &mut Context<Self>) {
        set_lsp_settings(cx, settings.clone());
        self.lsp.settings = settings.clone();
        if self.root_trusted() {
            return self.read_trusted_settings(cx);
        }
        if let Some(manager) = self.lsp.manager.clone() {
            lock(&manager).update_settings(settings);
            #[cfg(test)]
            {
                self.lsp.handed = Some(settings.clone());
            }
        }
    }

    /// Gives every editor the save hook while `editor.format_on_save` is on or
    /// `editor.code_actions_on_save` names a kind,
    /// and none otherwise, as a save with a hook starts later than one
    /// without.
    pub(crate) fn sync_save_hooks(&mut self, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        let save_hook = match &self.lsp.save_hook {
            Some(hook) => hook.clone(),
            None => {
                let hook: Rc<dyn fe_gpui::SaveHook> =
                    Rc::new(crate::lsp_format::FormatOnSave::new(
                        cx.weak_entity(),
                        self.lsp.format_on_save.clone(),
                        self.lsp.code_actions_on_save.clone(),
                    ));
                self.lsp.save_hook = Some(hook.clone());
                hook
            }
        };
        let wanted = (self.lsp.format_on_save.get()
            || !self.lsp.code_actions_on_save.borrow().is_empty())
        .then(|| save_hook.clone());
        for editor in &editors {
            let set = editor.read(cx).save_hook();
            let same = match (set, &wanted) {
                (Some(set), Some(wanted)) => Rc::ptr_eq(set, wanted),
                (None, None) => true,
                _ => false,
            };
            if !same {
                editor.update(cx, |view, _| view.set_save_hook(wanted.clone()));
            }
        }
    }

    /// The file tree changed: what ignore-file patterns matched may have.
    pub(crate) fn forget_ignore_matches(&self) {
        self.lsp.hover.ignore.forget_matches();
    }

    /// The frame hook, run after each draw: opens buffers newly shown,
    /// closes those no longer shown, nudges those that moved, and updates
    /// the status item.
    pub(crate) fn sync_lsp(&mut self, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        self.sync_save_hooks(cx);
        self.sync_highlights(cx);
        self.sync_linked_editing(cx);
        self.sync_format_on_type(&editors, cx);
        self.sync_lightbulb(cx);
        self.sync_inlay_hints(cx);
        self.sync_lenses(cx);
        self.sync_links(cx);
        self.sync_colors(cx);
        self.sync_symbols(cx);
        self.sync_folds(cx);
        self.sync_semantic_tokens(cx);
        self.sync_markdown_renderers(cx);
        self.lsp.hover.ignore.sync(&editors, cx);
        let hover: Rc<dyn fe_gpui::HoverProvider> = self.lsp.hover.clone();
        let completion: Rc<dyn fe_gpui::CompletionProvider> = self.lsp.hover.clone();
        let signature_help: Rc<dyn fe_gpui::SignatureHelpProvider> = self.lsp.hover.clone();
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
            let set = editor.read(cx).signature_help_provider();
            if !set.is_some_and(|set| Rc::ptr_eq(set, &signature_help)) {
                editor.update(cx, |view, _| {
                    view.set_signature_help_provider(Some(signature_help.clone()))
                });
            }
        }
        if self.lsp.manager.is_none() && self.lsp.root_failed.is_some() {
            self.retry_lsp_root(&editors, cx);
        }
        let lsp = &mut self.lsp;
        let (Some(manager), root) = (lsp.manager.clone(), lsp.root.as_deref()) else {
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
        let mut shown = HashSet::new();
        let mut to_resolve = Vec::new();
        let mut to_follow = Vec::new();
        // Each shown buffer's language, by its first view (the one that
        // opens it), for finding an alias that can take over a released
        // path.
        let mut languages: HashMap<BufferId, Option<Cow<'static, str>>> = HashMap::new();
        for editor in &editors {
            let view = editor.read(cx);
            languages
                .entry(view.model().buffer().id())
                .or_insert_with(|| routed_language(view));
        }
        // Whether `alias` can open `route`'s path: shown, resolved to it,
        // not open, and in a language that server handles.
        let eligible = |alias: BufferId,
                        route: &Route,
                        resolved: &HashMap<BufferId, Option<PathBuf>>,
                        documents: &HashMap<BufferId, Document>,
                        manager: &LspManager| {
            route.0.is_some()
                && !documents.contains_key(&alias)
                && resolved.get(&alias).and_then(Option::as_ref) == Some(&route.1)
                && languages
                    .get(&alias)
                    .is_some_and(|language| server_name(manager, language.as_deref()) == route.0)
        };
        // Trust grants, settings reloads and a server's closing documents
        // move them on the manager by themselves: owners are taken anew
        // from where the manager has each document, those it serves first,
        // then those parked (by a document of the URI there, or a root not
        // trusted) where none is served.
        for (&buffer, document) in documents.iter_mut() {
            document.server_name = manager.route_of(buffer);
        }
        owners.clear();
        for served in [true, false] {
            for (&buffer, document) in documents.iter() {
                if manager.server_of(buffer).is_some() == served {
                    owners
                        .entry((document.server_name.clone(), document.path.clone()))
                        .or_insert(buffer);
                }
            }
        }
        reserved
            .retain(|route, &mut alias| eligible(alias, route, &lsp.resolved, documents, &manager));
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
            // Saved under another name: closed, then opened under the new
            // path once that's resolved, as a language change is.
            let moved = lsp
                .resolved_from
                .get(&buffer)
                .is_some_and(|from| from != path);
            if moved {
                lsp.resolved.remove(&buffer);
                lsp.resolving.remove(&buffer);
            }
            // Resolved while its language loads, if it does.
            if !lsp.resolved.contains_key(&buffer) && lsp.resolving.insert(buffer) {
                lsp.resolved_from.insert(buffer, path.to_owned());
                to_resolve.push((buffer, path.to_owned()));
                to_follow.push(editor.clone());
            }
            let language = routed_language(view);
            match (documents.get(&buffer), &language) {
                _ if moved => {}
                (Some(document), Some(language)) if &document.language == language => continue,
                (Some(_), _) if view.is_loading_language() => continue,
                (None, None) => continue,
                _ => {}
            }
            if let Some(document) = documents.remove(&buffer) {
                let route = (document.server_name, document.path);
                if owners.get(&route) == Some(&buffer) {
                    owners.remove(&route);
                }
                // Handed to a waiting alias the server handles, which opens
                // it at the next frame. The owner itself opens under its new
                // server regardless.
                if !reserved.contains_key(&route) {
                    let alias = lsp.resolved.keys().find(|&&other| {
                        other != buffer
                            && eligible(other, &route, &lsp.resolved, documents, &manager)
                    });
                    if let Some(&alias) = alias {
                        reserved.insert(route.clone(), alias);
                    }
                }
                released.push(route);
            }
            manager.close(buffer);
            let Some(language) = language else {
                continue;
            };
            // Under the root or not: one outside it goes to a server at its
            // marker root, if that's trusted.
            let Some(Some(path)) = lsp.resolved.get(&buffer) else {
                continue;
            };
            let path = path.clone();
            let server_name = server_name(&manager, Some(&language));
            let route: Route = (server_name.clone(), path.clone());
            // A server has one document per path: a buffer that is an alias
            // of a file owned there waits for its owner to close.
            if owners.contains_key(&route) {
                continue;
            }
            // A reservation holds only while its alias can still open the
            // path; otherwise it's dropped, and this buffer takes it.
            match reserved.get(&route) {
                Some(&alias) if alias == buffer => {
                    reserved.remove(&route);
                }
                Some(&alias) if eligible(alias, &route, &lsp.resolved, documents, &manager) => {
                    continue;
                }
                Some(_) => {
                    reserved.remove(&route);
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
            // The first root the trust predicate is asked about is the
            // document's: `manager.close` above left nothing for `open` to
            // close first.
            lock(&lsp.asked).take();
            let opened = manager.open(root, buffer, text, version, synced, &path, &language);
            let server = opened.unwrap_or_else(|error| {
                log::warn!("{}: {error}", path.display());
                None
            });
            let server_root = lock(&lsp.asked).take();
            owners.insert(route, buffer);
            documents.insert(
                buffer,
                Document {
                    path,
                    server_name,
                    language,
                    server,
                    root: server_root,
                    moved,
                },
            );
        }
        documents.retain(|&buffer, document| {
            shown.contains(&buffer) || {
                manager.close(buffer);
                let route = (document.server_name.clone(), document.path.clone());
                if owners.get(&route) == Some(&buffer) {
                    owners.remove(&route);
                }
                released.push(route);
                false
            }
        });
        // A path freed while another shown buffer resolves to it: that
        // buffer is opened at the next frame.
        let handed_over = released.iter().any(|route| {
            !owners.contains_key(route)
                && lsp.resolved.iter().any(|(buffer, resolved)| {
                    shown.contains(buffer)
                        && !documents.contains_key(buffer)
                        && resolved.as_ref() == Some(&route.1)
                })
        });
        lsp.resolved.retain(|buffer, _| shown.contains(buffer));
        lsp.resolved_from.retain(|buffer, _| shown.contains(buffer));
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
            let resolved = cx.background_spawn({
                let path = path.clone();
                async move { resolve(&path) }
            });
            cx.spawn(async move |workbench, cx| {
                let resolved = resolved.await;
                workbench
                    .update(cx, |workbench, cx| {
                        let lsp = &mut workbench.lsp;
                        // A buffer saved under another name since is being
                        // resolved anew.
                        if lsp.generation != generation
                            || lsp.resolved_from.get(&buffer) != Some(&path)
                        {
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
        if !saved {
            return;
        }
        if lsp.root_retrying {
            lsp.root_save_pending = true;
            return;
        }
        self.resolve_lsp_root(root, cx);
    }

    /// Resolves the failed root again; if that fails and a save arrived
    /// meanwhile, once more.
    fn resolve_lsp_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let lsp = &mut self.lsp;
        lsp.root_retrying = true;
        lsp.root_save_pending = false;
        let generation = lsp.generation;
        let path = root.clone();
        let resolved = cx.background_spawn(async move { dunce::canonicalize(&path) });
        lsp._root = Some(cx.spawn(async move |workbench, cx| {
            let resolved = resolved.await;
            workbench
                .update(cx, |workbench, cx| {
                    if workbench.lsp.generation != generation {
                        return;
                    }
                    workbench.lsp.root_retrying = false;
                    match resolved {
                        Ok(root) => {
                            workbench.lsp.root_failed = None;
                            workbench.lsp.root_saves.clear();
                            workbench.lsp.root_save_pending = false;
                            workbench.start_lsp_manager(Some(root), cx);
                        }
                        Err(_) if workbench.lsp.root_save_pending => {
                            workbench.resolve_lsp_root(root, cx);
                        }
                        Err(_) => {}
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
                // The session that sent them has ended.
                let ended = matches!(
                    status,
                    ServerStatus::Restarting(_) | ServerStatus::Stopped(_)
                );
                lsp.messages.remove(&server);
                lsp.servers.insert(server, (name, status));
                if ended && lsp.drop_file_diagnostics(Some(server)) {
                    self.sync_problems(cx);
                }
            }
            LspEvent::ShowMessage {
                server,
                name,
                typ,
                message,
            } => {
                log::info!("{name} says ({typ:?}): {message}");
                // fe-workbench has no lsp_types of its own; its Debug is the
                // level's name.
                let toast = match format!("{typ:?}").as_str() {
                    "Error" => Some(Toast::error(name.clone())),
                    "Warning" => Some(Toast::warning(name.clone())),
                    _ => None,
                };
                if let Some(toast) = toast {
                    toast::show(cx, toast.detail(message.clone()));
                }
                lsp.messages.insert(server, message);
            }
            LspEvent::ShowMessageRequest {
                name,
                typ,
                message,
                actions,
                reply,
                ..
            } => {
                log::info!("{name} asks ({typ:?}): {message}");
                let toast = match format!("{typ:?}").as_str() {
                    "Error" => Toast::error(name),
                    "Warning" => Toast::warning(name),
                    _ => Toast::info(name),
                };
                // Dropped unanswered (no window, closed, dismissed), the
                // reply answers none.
                let toast = toast
                    .detail(message)
                    .choices(actions, move |choice| match choice {
                        Some(index) => reply.choose(index),
                        None => reply.dismiss(),
                    });
                toast::show(cx, toast);
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
                    .and_then(|manager| lock(manager).active_identity(buffer, server))
                    .map(|(session, incarnation)| DiagnosticIdentity::Lsp(session, incarnation));
                let items: Vec<_> = items.into_iter().map(diagnostic).collect();
                let mut applied = false;
                for editor in self.editors_of(buffer, cx) {
                    applied |= editor.update(cx, |view, cx| {
                        let identity = DiagnosticIdentity::Lsp(session, incarnation);
                        let model = view.model_mut();
                        let applied = model.apply_diagnostics(
                            DiagnosticOwner::Server(server),
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
                        let identity = DiagnosticIdentity::Lsp(session, incarnation);
                        if view
                            .model_mut()
                            .clear_diagnostics(DiagnosticOwner::Server(server), identity)
                        {
                            cx.notify();
                        }
                    });
                }
            }
            LspEvent::FileDiagnostics {
                server,
                path: Some(path),
                encoding,
                items,
                ..
            } => {
                let key = (server, path);
                lsp.unopened.forget(&key.1);
                let changed = if items.is_empty() {
                    lsp.file_diagnostics.remove(&key).is_some()
                } else {
                    lsp.file_diagnostics.insert(key, (encoding, items));
                    true
                };
                if changed {
                    self.sync_problems(cx);
                }
            }
            LspEvent::ApplyEdit {
                server,
                label,
                edits,
                reply,
            } => {
                let name = (lsp.servers.get(&server))
                    .map_or_else(|| format!("{server:?}"), |(name, _)| name.clone());
                let label = label.unwrap_or_else(|| "an edit".to_owned());
                // Claimed before anything changes, so the server never
                // hears it wasn't applied when it was. Not claimable, it
                // was answered not applied already: it timed out, or the
                // session ended.
                let Some(reply) = reply.claim() else {
                    log::info!("dropped {name}'s {label}: it's no longer awaited");
                    return;
                };
                match self.apply_file_edits(edits, cx) {
                    Ok(applied) => {
                        log::debug!(
                            "applied {name}'s {label}: {} edits in {} files",
                            applied.edits,
                            applied.files
                        );
                        reply.applied();
                    }
                    Err(error) => {
                        log::info!("didn't apply {name}'s {label}: {error}");
                        reply.failed(error);
                    }
                }
            }
            LspEvent::Refresh {
                server,
                what: fe_lsp::Refresh::InlayHints,
            } => self.refresh_inlay_hints(server, cx),
            LspEvent::Refresh {
                server,
                what: fe_lsp::Refresh::CodeLens,
            } => self.refresh_lenses(server, cx),
            LspEvent::Refresh {
                server,
                what: fe_lsp::Refresh::SemanticTokens,
            } => self.refresh_semantic_tokens(server, cx),
            _ => {}
        }
    }

    /// Asks what renaming at `offset` of `buffer` at `version` would
    /// rename: `None` if there's no manager or no server for `buffer`.
    pub(crate) fn lsp_prepare_rename(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
    ) -> Option<std::pin::Pin<Box<dyn std::future::Future<Output = RenameAnswer> + Send>>> {
        let manager = lock(self.lsp.manager.as_ref()?);
        manager.server_of(buffer)?;
        Some(Box::pin(manager.prepare_rename(buffer, version, offset)))
    }

    /// Asks for the edits renaming at `offset` of `buffer` at `version` to
    /// `new_name`, as [`Self::lsp_prepare_rename`] does.
    pub(crate) fn lsp_rename(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
        new_name: String,
    ) -> Option<std::pin::Pin<Box<dyn std::future::Future<Output = RenameEdits> + Send>>> {
        let manager = lock(self.lsp.manager.as_ref()?);
        manager.server_of(buffer)?;
        Some(Box::pin(manager.rename(buffer, version, offset, new_name)))
    }

    /// The open editors showing `buffer`.
    pub(crate) fn editors_of(
        &self,
        buffer: BufferId,
        cx: &App,
    ) -> Vec<gpui::Entity<fe_gpui::EditorView>> {
        let mut editors = self.open_editors(cx);
        editors.retain(|editor| editor.read(cx).model().buffer().id() == buffer);
        editors
    }

    /// The code actions client of the server `buffer` is open on, if it
    /// has one.
    pub(crate) fn lsp_code_actioner(&self, buffer: BufferId) -> Option<CodeActioner> {
        let manager = self.lsp.manager.as_ref()?;
        lock(manager).server_of(buffer)?;
        Some(CodeActioner(manager.clone()))
    }

    /// Gives the active editor's server `status` and `message`, as its
    /// events would, and updates the item.
    #[cfg(test)]
    pub(crate) fn force_lsp_status(
        &mut self,
        status: ServerStatus,
        message: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let buffer = self
            .active_editor(cx)
            .map(|editor| editor.read(cx).model().buffer().id());
        let (server, ..) = self.lsp.server(buffer).expect("the buffer has a server");
        self.lsp.servers.get_mut(&server).unwrap().1 = status;
        match message {
            Some(message) => self.lsp.messages.insert(server, message),
            None => self.lsp.messages.remove(&server),
        };
        self.show_lsp_status(cx);
    }

    /// Sets the status item for the active editor's server, or removes it.
    fn show_lsp_status(&mut self, cx: &mut Context<Self>) {
        let buffer = self
            .active_editor(cx)
            .map(|editor| editor.read(cx).model().buffer().id());
        let lsp = &self.lsp;
        // An untrusted folder says so, over a server it stopped.
        let shown = match self.untrusted_root(buffer, cx) {
            Some((name, root)) => Some(untrusted_status(&name, &root)),
            None => lsp.server(buffer).and_then(|(server, name, status)| {
                let message = lsp.messages.get(&server).map(String::as_str);
                status_text(name, status, message).map(|(text, tone)| (text, tone, None))
            }),
        };
        if shown == self.lsp.shown {
            return;
        }
        self.lsp.shown.clone_from(&shown);
        self.status_bar().clone().update(cx, |bar, cx| match shown {
            Some((text, tone, None)) => bar.set_item(lsp_status_item(text, tone), cx),
            Some((text, tone, Some(tooltip))) => {
                let item = kiln_shell::StatusItem::new(ITEM, StatusSide::Right, 0, text)
                    .icon(Icon::Server)
                    .tone(tone)
                    .tooltip(tooltip);
                bar.set_item(item, cx)
            }
            None => bar.remove_item(ITEM, cx),
        });
    }

    /// The server `buffer` is routed to and the marker root it runs in, if
    /// that root, outside the window root, isn't inside a trusted folder,
    /// so the server doesn't start there or was stopped.
    fn untrusted_root(&self, buffer: Option<BufferId>, cx: &App) -> Option<(String, PathBuf)> {
        let lsp = &self.lsp;
        let buffer = buffer?;
        let document = lsp.documents.get(&buffer)?;
        let (name, root) = (document.server_name.clone()?, document.root.clone()?);
        if lsp.root.as_ref() == Some(&root) || kiln_shell::trust::decided(&root, cx) == Some(true) {
            return None;
        }
        Some((name, root))
    }

    /// Restarts the active editor's server, or opens its log in a tab if
    /// `log`.
    pub(crate) fn run_server_op(&mut self, log: bool, window: &mut Window, cx: &mut Context<Self>) {
        let buffer = self
            .active_editor(cx)
            .map(|editor| editor.read(cx).model().buffer().id());
        let lsp = &mut self.lsp;
        // The folder the server runs in: the window root, or a marker root
        // outside it.
        let root = buffer
            .and_then(|buffer| lsp.documents.get(&buffer)?.root.clone())
            .or_else(|| lsp.root.clone());
        let (Some((server, name, _)), Some(root)) = (lsp.server(buffer), root) else {
            return;
        };
        let name = name.to_owned();
        let Some(manager) = lsp.manager.clone() else {
            return;
        };
        let mut manager = lock(&manager);
        match log {
            false => match manager.restart(&name, &root) {
                Ok(Some(restarted)) => {
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
                Err(error) => {
                    log::error!("can't restart {name}: {error}");
                    toast::show(
                        cx,
                        Toast::error(format!("Can't restart {name}")).detail(error.to_string()),
                    );
                }
            },
            true => {
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

/// A buffer's server's code actions, through the workbench's manager.
#[derive(Clone)]
pub(crate) struct CodeActioner(Arc<Mutex<LspManager>>);

pub(crate) type Answer<T> = Pin<Box<dyn Future<Output = Result<T, fe_lsp::LspError>> + Send>>;

impl CodeActioner {
    /// The actions for `range` of `buffer` at `version`, with the
    /// diagnostics shown over it. Sent under the lock; awaited without it.
    pub(crate) fn ask(
        &self,
        buffer: BufferId,
        version: Version,
        range: std::ops::Range<usize>,
        diagnostics: Vec<fe_lsp::Diagnostic>,
        query: fe_lsp::code_action::CodeActionQuery,
    ) -> Answer<Vec<fe_lsp::CodeAction>> {
        Box::pin(lock(&self.0).code_actions_for(buffer, version, range, diagnostics, query))
    }

    /// The actions of kind `kind` alone for `range` of `buffer` at
    /// `version`, asked for without the user having invoked them.
    pub(crate) fn ask_kind(
        &self,
        buffer: BufferId,
        version: Version,
        range: std::ops::Range<usize>,
        kind: &str,
    ) -> Answer<Vec<fe_lsp::CodeAction>> {
        let query = fe_lsp::code_action::CodeActionQuery {
            only: Some(vec![kind.to_owned()]),
            automatic: true,
        };
        Box::pin(lock(&self.0).code_actions_for(buffer, version, range, Vec::new(), query))
    }

    /// `action` with the edit its server resolves.
    pub(crate) fn resolve(&self, action: &fe_lsp::CodeAction) -> Answer<fe_lsp::CodeAction> {
        Box::pin(lock(&self.0).resolve_code_action(action))
    }

    /// `action`'s edit, decoded.
    pub(crate) fn edits(
        &self,
        action: &fe_lsp::CodeAction,
    ) -> Result<Vec<fe_lsp::FileEdits>, fe_lsp::LspError> {
        lock(&self.0).code_action_edits(action)
    }

    /// Has the server run `action`'s command, if it has one.
    pub(crate) fn run(&self, action: &fe_lsp::CodeAction) -> Answer<()> {
        Box::pin(lock(&self.0).run_code_action_command(action))
    }
}

/// Hover text, completions and signature help for an editor, from the
/// server its buffer is open on, through whichever manager the workbench has
/// now: none (no root, or a closed window) answers nothing. Completions
/// add the user's and extensions' snippets ([`crate::extension_snippets`]),
/// server or none, unless a punctuation trigger character asked. In an ignore file,
/// completions add paths and hovers add a pattern's matches, server or none
/// ([`crate::ignore_assist`]).
#[derive(Default)]
struct LspHover {
    manager: RefCell<Weak<Mutex<LspManager>>>,
    /// The workbench, to read the editor asking for completions in.
    workbench: RefCell<Option<(WeakEntity<Workbench>, gpui::AsyncApp)>>,
    /// Runs the commands accepted completions have.
    executor: RefCell<Option<gpui::BackgroundExecutor>>,
    ignore: Rc<crate::ignore_assist::IgnoreAssist>,
}

impl LspHover {
    /// The workbench, if `buffer` is an ignore file to help with.
    fn ignore_host(&self, buffer: BufferId) -> Option<(WeakEntity<Workbench>, gpui::AsyncApp)> {
        (self.ignore.covers(buffer))
            .then(|| self.workbench.borrow().clone())
            .flatten()
    }
}

impl fe_gpui::HoverProvider for LspHover {
    fn hover(&self, buffer: BufferId, version: Version, offset: usize) -> fe_gpui::HoverFuture {
        let ignore = self.ignore_host(buffer);
        let assist = self.ignore.clone();
        // Sent under the lock; awaited without it.
        let reply = (self.manager.borrow().upgrade())
            .map(|manager| lock(&manager).hover(buffer, version, offset));
        Box::pin(async move {
            let matches = match ignore {
                Some((workbench, cx)) => assist.hover(workbench, cx, buffer, version, offset).await,
                None => None,
            };
            let server = match reply {
                Some(reply) => match reply.await {
                    Ok(hover) => hover.map(|hover| fe_gpui::HoverContent {
                        text: hover.text,
                        range: hover.range,
                    }),
                    Err(error) => {
                        log::debug!("no hover: {error}");
                        None
                    }
                },
                None => None,
            };
            crate::ignore_assist::merged_hover(server, matches)
        })
    }
}

impl fe_gpui::CompletionProvider for LspHover {
    fn trigger_characters(&self, buffer: BufferId) -> Vec<String> {
        let mut triggers = match self.manager.borrow().upgrade() {
            Some(manager) => lock(&manager).trigger_characters(buffer),
            None => Vec::new(),
        };
        let path = crate::ignore_assist::TRIGGER;
        if self.ignore.covers(buffer) && !triggers.iter().any(|trigger| trigger == path) {
            triggers.push(path.to_owned());
        }
        triggers
    }

    fn completions(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
        trigger: CompletionTrigger,
    ) -> fe_gpui::CompletionFuture {
        let host = crate::extension_snippets::offers_snippets(&trigger)
            .then(|| self.workbench.borrow().clone())
            .flatten();
        let ignore = self.ignore_host(buffer);
        let trigger = match trigger {
            CompletionTrigger::Invoked => fe_lsp::CompletionTrigger::Invoked,
            CompletionTrigger::Character(c) => fe_lsp::CompletionTrigger::Character(c),
            CompletionTrigger::Incomplete => fe_lsp::CompletionTrigger::Incomplete,
        };
        // Sent under the lock; awaited without it.
        let reply = (self.manager.borrow().upgrade())
            .map(|manager| lock(&manager).completion(buffer, version, offset, trigger));
        Box::pin(async move {
            // Read on the first poll, before the server answers.
            let snippets = host.map_or_else(CompletionList::default, |(workbench, cx)| {
                cx.update(|cx| {
                    crate::extension_snippets::snippet_completions(
                        cx, &workbench, buffer, version, offset,
                    )
                })
            });
            let paths = match ignore {
                Some((workbench, cx)) => {
                    crate::ignore_assist::path_completions(workbench, cx, buffer, version, offset)
                        .await
                }
                None => None,
            };
            let list = match reply {
                Some(reply) => match reply.await {
                    Ok(completions) => completions.map(|completions| CompletionList {
                        items: completions.items.into_iter().map(completion_item).collect(),
                        is_incomplete: completions.is_incomplete,
                    }),
                    Err(error) => {
                        log::debug!("no completions: {error}");
                        None
                    }
                },
                None => None,
            };
            if snippets.items.is_empty() && !snippets.is_incomplete && paths.is_none() {
                return list;
            }
            let mut list = list.unwrap_or_default();
            list.items.extend(snippets.items);
            list.is_incomplete |= snippets.is_incomplete;
            if let Some(paths) = paths {
                list.items.extend(paths.items);
                list.is_incomplete |= paths.is_incomplete;
            }
            Some(list)
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

    fn run_command(&self, item: CompletionItem) {
        let manager = self.manager.borrow().upgrade();
        let lsp =
            (item.data.as_ref()).and_then(|data| data.downcast_ref::<fe_lsp::CompletionItem>());
        let (Some(manager), Some(lsp)) = (manager, lsp) else {
            return;
        };
        // Sent under the lock; awaited without it.
        let reply = lock(&manager).run_completion_command(lsp);
        let Some(executor) = self.executor.borrow().clone() else {
            return;
        };
        executor
            .spawn(async move {
                if let Err(error) = reply.await {
                    log::debug!("completion command not run: {error}");
                }
            })
            .detach();
    }
}

impl fe_gpui::SignatureHelpProvider for LspHover {
    fn triggers(&self, buffer: BufferId) -> Option<(Vec<char>, Vec<char>)> {
        let manager = self.manager.borrow().upgrade()?;
        lock(&manager).signature_help_triggers(buffer)
    }

    fn signature_help(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
        context: fe_gpui::SignatureContext,
    ) -> fe_gpui::SignatureHelpFuture {
        let Some(manager) = self.manager.borrow().upgrade() else {
            return Box::pin(async { None });
        };
        let mut lsp = fe_lsp::SignatureContext::new(match context.trigger {
            fe_gpui::SignatureTrigger::Invoked => fe_lsp::SignatureTrigger::Invoked,
            fe_gpui::SignatureTrigger::Character(c) => fe_lsp::SignatureTrigger::Character(c),
            fe_gpui::SignatureTrigger::ContentChange => fe_lsp::SignatureTrigger::ContentChange,
        });
        lsp.is_retrigger = context.is_retrigger;
        lsp.active = context.active.map(|help| {
            let signatures = (help.signatures.into_iter())
                .map(|signature| {
                    let parameters = signature.parameters.into_iter().map(|p| p.range);
                    fe_lsp::Signature::new(
                        signature.label,
                        signature.documentation,
                        parameters.collect(),
                        signature.active_parameter,
                    )
                })
                .collect();
            fe_lsp::SignatureHelp::new(signatures, help.active_signature, help.active_parameter)
        });
        // Sent under the lock; awaited without it.
        let reply = lock(&manager).signature_help(buffer, version, offset, lsp);
        Box::pin(async move {
            match reply.await {
                Ok(help) => help.map(signature_help),
                Err(error) => {
                    log::debug!("no signature help: {error}");
                    None
                }
            }
        })
    }
}

/// fe-lsp's signature help as fe-gpui's.
fn signature_help(help: fe_lsp::SignatureHelp) -> fe_gpui::SignatureHelp {
    let signatures = (help.signatures.into_iter())
        .map(|signature| fe_gpui::Signature {
            label: signature.label,
            documentation: signature.documentation,
            parameters: (signature.parameters.into_iter())
                .map(|range| fe_gpui::Parameter {
                    range,
                    documentation: None,
                })
                .collect(),
            active_parameter: signature.active_parameter,
        })
        .collect();
    fe_gpui::SignatureHelp {
        signatures,
        active_signature: help.active_signature,
        active_parameter: help.active_parameter,
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
        kind: item.kind.map(|kind| fe_lsp::kind_label(kind).to_string()),
        detail: item.detail.clone(),
        documentation: item.documentation.clone(),
        filter_text: item.filter_text.clone(),
        sort_text: item.sort_text.clone(),
        group: 0,
        edit: edit(&item.edit),
        additional_edits: item.additional_edits.iter().map(edit).collect(),
        snippet: item.snippet,
        commit_characters: item.commit_characters.clone(),
        command: item.command().map(|c| c.command.clone()),
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
    let related = (item.related.into_iter())
        .map(|related| RelatedInfo {
            path: related.path,
            row: related.row,
            range: related.range,
            message: related.message,
        })
        .collect();
    let diagnostic = Diagnostic {
        severity,
        message: item.message,
        source: item.source,
        code: item.code,
        tags,
        related,
        code_href: item.code_description.map(String::from),
    };
    (item.range, diagnostic)
}

/// `path` fully resolved, so through no `..` or symlink, its own
/// included; `None` if it can't be (a dangling symlink too). A file that
/// doesn't exist yet is resolved by its folder.
pub(crate) fn resolve(path: &Path) -> Option<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(_) => dunce::canonicalize(path).ok(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let name = path.file_name()?;
            let folder = dunce::canonicalize(path.parent()?).ok()?;
            Some(folder.join(name))
        }
        Err(_) => None,
    }
}

/// Has quitting shut every server down first ([`shut_down_all`]).
pub(crate) fn init(cx: &mut App) {
    cx.on_app_quit(|cx| {
        shut_down_all(cx);
        async {}
    })
    .detach();
}

#[cfg(test)]
impl Workbench {
    /// Tells the workbench its root's project settings file changed, as
    /// its watcher would outside tests.
    pub(crate) fn lsp_project_changed(&self) {
        if let Some(sender) = &self.lsp.project_changed {
            sender.send_blocking(()).ok();
        }
    }

    /// Tells the app its user settings file changed, as its watcher would
    /// outside tests.
    pub(crate) fn lsp_user_changed(&self, cx: &App) {
        if let Some(user) = cx.try_global::<UserLayer>() {
            user.changed.send_blocking(()).ok();
        }
    }

    /// The app's user layer.
    pub(crate) fn lsp_user_layer(&self) -> Option<Arc<Mutex<LspLayer>>> {
        self.lsp.user.clone()
    }

    /// Starts a manager for `root` as a root change does.
    pub(crate) fn lsp_start_manager(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.start_lsp_manager(Some(root), cx);
    }

    /// The `[lsp]` settings last handed to a manager.
    pub(crate) fn lsp_handed(&self) -> Option<LspSettings> {
        self.lsp.handed.clone()
    }

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

    /// How many `Restarting` statuses the managers' sinks have sent.
    #[cfg(test)]
    pub(crate) fn lsp_restarting_sent(&self) -> usize {
        (self.lsp.restarting_sent).load(std::sync::atomic::Ordering::SeqCst)
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

    /// `edit` decoded by the manager against the documents `server` has
    /// open, as a rename's answer is. Blocks until the server's thread has.
    pub(crate) fn lsp_decode_edit(
        &self,
        server: ServerId,
        edit: lsp_types::WorkspaceEdit,
    ) -> Result<Vec<fe_lsp::FileEdits>, fe_lsp::LspError> {
        let manager = self.lsp.manager.as_ref().expect("a manager");
        let decoded = lock(manager).decode_workspace_edit(server, edit);
        futures::executor::block_on(decoded)
    }

    /// Stops `server`, as a restart or a settings change would.
    pub(crate) fn lsp_stop(&self, server: ServerId) {
        lock(self.lsp.manager.as_ref().expect("a manager")).stop(server);
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

#[path = "lsp_format_on_type.rs"]
mod format_on_type;
#[path = "lsp_highlights.rs"]
mod highlights;
#[path = "lsp_lightbulb.rs"]
mod lightbulb;
#[path = "lsp_linked_editing.rs"]
mod linked_editing;

#[path = "lsp_colors.rs"]
mod colors;
#[path = "lsp_folds.rs"]
mod folds;
#[path = "lsp_inlay_hints.rs"]
mod inlay_hints;
#[path = "lsp_lenses.rs"]
mod lenses;
#[path = "lsp_links.rs"]
mod links;

#[path = "lsp_symbols.rs"]
mod symbols;

#[path = "lsp_semantic_tokens.rs"]
mod semantic_tokens;

#[path = "lsp_file_operations.rs"]
mod file_operations;

#[path = "lsp_watched_files.rs"]
mod watched_files;

#[path = "lsp_markdown.rs"]
mod markdown;

#[cfg(test)]
#[path = "lsp_trust_tests.rs"]
mod trust_tests;

#[cfg(test)]
#[path = "lsp_trusted_settings_tests.rs"]
mod trusted_settings_tests;
