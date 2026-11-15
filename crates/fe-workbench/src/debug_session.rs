//! Debug sessions in the workbench (docs/decisions/r1b9dw3g0g-debugger.md
//! §§5, 7, 8 and 10; card D7): starting a launch configuration, its
//! pre-launch build, the program's own terminal tab, breakpoints sent as
//! they change, stops opened at their row, and the commands.
//!
//! - **One [`Debugger`] per window**, made with its workbench ([`init`]) and
//!   found through [`Workbench::debugger`] or [`debugger_of`]. It holds the
//!   window's session, one at a time: starting another stops it.
//! - **Commands** (`workbench_commands.rs`): `debug.start` shows the
//!   configuration picker ([`picker`]), or continues a stopped session;
//!   `debug.stop` ends the session (or a start under way); `debug.rerun_last`
//!   resolves the last configuration again and starts it afresh;
//!   `debug.show_log` opens the adapter protocol log in a read-only tab.
//! - **Starts** are numbered: each one, from reading the configurations to
//!   the session's launch, is the debugger's start under way until it
//!   launches or is given up, and any step that finds a newer start, or
//!   none, does nothing. `debug.stop`, untrusting the folder and a new root
//!   give it up.
//! - **Trust** (§8). A configuration comes from the project, so starting
//!   one needs the folder's live trust, checked on every path just before
//!   the launch (after a pre-launch build too). An untrusted folder's
//!   configurations are listed but never started: the toast reads "Trust
//!   this folder to debug its programs", with Trust Folder.
//!   Untrusting the folder, or changing the root, stops the session.
//! - **Pre-launch** (§7). A `pre_launch_task` runs through
//!   [`Workbench::run_task_reading`] (its tab, its matcher into Problems),
//!   and the session starts only if it exits 0; a detected Cargo
//!   configuration's build runs the same way, and its program is the
//!   executable cargo's `compiler-artifact` message names
//!   ([`launch::cargo_artifact`]). A task whose tab closes first gives the
//!   start up.
//! - **Java** (D21b, [`crate::debug_java`]): a configuration of the `java`
//!   adapter asks the root's jdtls for its main class and classpath, if it
//!   doesn't give them, and for java-debug's port, which its session
//!   connects to; the debugger finds java-debug's jar for jdtls.
//! - **The program's terminal** (§5). An adapter's `runInTerminal` runs its
//!   argv directly in the configuration's tab,
//!   [`TerminalPanel::add_debuggee`](crate::TerminalPanel): a task tab's
//!   look with no task bookkeeping. Only the session's own request, while it
//!   isn't stopping, in its root, trusted, is run; a tab whose program
//!   hasn't started when its session stops, or within
//!   [`TERMINAL_START`], is closed. Its rerun asks for a fresh session of
//!   the configuration, and so a fresh `runInTerminal`.
//! - **Breakpoints** (§5 "Sending"). The session starts with each file's
//!   enabled breakpoints by line on disk, as the store has them just before
//!   it launches; after that a file's set is sent again when the store says
//!   it changed or a save moved it ([`BreakpointEvent`]), never per
//!   keystroke. Each send is numbered per file, and only the last one's
//!   reply counts. What the adapter says of each breakpoint, verified,
//!   message and line, is kept for the session's life ([`Verification`]).
//! - **Stops** open the top frame's file with the caret on its row, and say
//!   "Stopped at line N"; "Running" and "Session ended" are said too.
//! - **Failures** a launch's adapter reports in its own terse words get
//!   Tenmoku's: a ptrace refusal on Linux names `kernel.yama.ptrace_scope`
//!   and its value, read through [`DebugHost::ptrace_scope`]; a signing
//!   refusal on macOS says what the program needs (§10). A request that
//!   fails while the session carries on (a Continue, a breakpoint update) is
//!   said in a toast too.
//! - **Settings**: `[debug]` (`DebugSettings`), the app's as the window
//!   opens and then from `reload_settings`, applies to the sessions started
//!   after it changes: the user's adapters, `debug.log_payloads` and
//!   `debug.timeouts`.
//!
//! **For the cards after this one** (D6's gutter and band, D7b's Runs, D9's
//! panels): a [`Debugger`] emits [`DebuggerEvent`]s, a start under way and
//! given up, the session's state, each stop with its file, row and frame,
//! output and breakpoint verification, and answers [`Debugger::controls`]
//! for the toolbar.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use fe_buffer::Buffer;
use fe_config::debug_settings::DebugSettings;
use fe_dap::{
    AdapterLauncher, AdapterSpec, Clock, Control, Controls, DapEvent, Disabled, EndReason, Epoch,
    HostOs, LocalLauncher, LogEntry, RequestError, Resume, Session, SessionError, SessionState,
    SystemClock, TerminalReply, TerminalStarted, Timeouts,
    protocol::{
        Breakpoint as AdapterBreakpoint, BreakpointBody, BreakpointsBody, OutputBody,
        SetBreakpointsArguments, Source, SourceBreakpoint, StackFrame, request,
    },
};
use fe_editor::EditorModel;
use futures::{StreamExt as _, channel::mpsc};
use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, EntityId, EventEmitter, Focusable as _,
    Global, Subscription, Task, WeakEntity, Window,
};
use kiln_l10n::tr;

use crate::{
    RootChanged, TerminalPanel, TrustChanged, Workbench,
    debug_breakpoints::{BreakpointEvent, BreakpointStore},
    launch_defs::{
        self, LaunchConfig, PreLaunch, Program,
        variables::{ActiveFile, Values},
    },
    task_commands::project_files,
    task_defs::{self, Group, Task as ProjectTask, TaskCommand},
    task_run::TaskExit,
    terminal_panel::{Debuggee, DebuggeeRerun},
    workbench::toast::{self, Toast},
};

pub(crate) mod launch;
pub(crate) mod picker;

/// How long a debuggee's tab may take to start its program before the
/// adapter is told it couldn't, and the tab closes.
pub const TERMINAL_START: Duration = Duration::from_secs(10);

/// What sessions start on: the launcher (by default fe-dap's
/// [`LocalLauncher`], given `debug.timeouts.connect`), its clock, the
/// host's own adapter specs (which win over every other of their name), and
/// where the ptrace scope is read. Tests set a fake ([`set_debug_host`]).
#[derive(Clone)]
pub struct DebugHost {
    /// `None` for a [`LocalLauncher`] per session.
    pub launcher: Option<Arc<dyn AdapterLauncher>>,
    pub clock: Arc<dyn Clock>,
    pub specs: Vec<AdapterSpec>,
    /// Where bare adapter commands are looked up: `None` for the login
    /// shell's `PATH`, as language servers'.
    pub login: Option<Arc<fe_lsp::LoginPath>>,
    /// Yama's `kernel.yama.ptrace_scope`, as its file holds it, if it can be
    /// read.
    pub ptrace_scope: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    /// Whose refusals are explained.
    pub os: HostOs,
}

impl Default for DebugHost {
    fn default() -> Self {
        Self {
            launcher: None,
            clock: Arc::new(SystemClock),
            specs: Vec::new(),
            login: None,
            ptrace_scope: Arc::new(launch::read_ptrace_scope),
            os: HostOs::current(),
        }
    }
}

impl DebugHost {
    /// The launcher a session starts on, with `settings`' connect timeout.
    fn launcher(&self, settings: &DebugSettings) -> Arc<dyn AdapterLauncher> {
        self.launcher.clone().unwrap_or_else(|| {
            let connect = Duration::from_millis(settings.timeouts.connect.max(1));
            Arc::new(
                LocalLauncher::new()
                    .with_clock(self.clock.clone())
                    .with_connect_timeout(connect),
            )
        })
    }
}

impl Global for DebugHost {}

/// Starts sessions on `host` from now on.
pub fn set_debug_host(cx: &mut App, host: DebugHost) {
    cx.set_global(host);
}

fn debug_host(cx: &App) -> DebugHost {
    cx.try_global::<DebugHost>().cloned().unwrap_or_default()
}

/// A session's timeouts, from `settings`.
fn timeouts(settings: &DebugSettings) -> Timeouts {
    Timeouts {
        request: Duration::from_millis(settings.timeouts.request.max(1)),
        launch: Duration::from_millis(settings.timeouts.launch.max(1)),
    }
}

/// Each window's debugger, by its workbench.
#[derive(Default)]
struct Debuggers(HashMap<EntityId, Entity<Debugger>>);

impl Global for Debuggers {}

/// The debugger of `workbench`'s window.
pub fn debugger_of(workbench: &Entity<Workbench>, cx: &App) -> Option<Entity<Debugger>> {
    let debuggers = cx.try_global::<Debuggers>()?;
    debuggers.0.get(&workbench.entity_id()).cloned()
}

/// A root's launch configurations, what was wrong with its launch files,
/// and its tasks, which a pre-launch task names.
#[derive(Default)]
pub(crate) struct Listing {
    pub configs: Vec<LaunchConfig>,
    pub diagnostics: Vec<launch_defs::LaunchDiagnostic>,
    pub tasks: Vec<ProjectTask>,
}

/// What the adapter says of one breakpoint, kept for the session's life:
/// a later `setBreakpoints` reply for its file, or a `breakpoint` event
/// naming its id, replaces it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verification {
    /// The line it was sent on, from 1: its line on disk.
    pub line: u32,
    pub verified: bool,
    /// Why it isn't verified, or the adapter's remark.
    pub message: Option<String>,
    /// Where the adapter put it, from 1, if it said: drawn there for the
    /// session's life, never persisted.
    pub adapter_line: Option<u32>,
    /// The adapter's id for it, which its `breakpoint` events name.
    pub id: Option<i64>,
}

/// Where the program stopped: the top frame of the stopped thread.
#[derive(Clone, Debug, PartialEq)]
pub struct StopLocation {
    pub epoch: Epoch,
    pub thread_id: i64,
    pub frame: StackFrame,
    /// The frame's file, if it has one.
    pub path: Option<PathBuf>,
    /// The frame's row, from 0.
    pub row: u32,
}

/// What a [`Debugger`] tells as its session goes.
#[derive(Clone, Debug, PartialEq)]
pub enum DebuggerEvent {
    /// A start is under way: its configuration is read, built or resolved.
    /// Its session's [`State`](Self::State) follows, or
    /// [`StartGivenUp`](Self::StartGivenUp).
    Starting(String),
    /// The start under way was stopped, failed or was replaced, before its
    /// session launched.
    StartGivenUp,
    /// The session's state changed: it started, ran, stopped, ended; or it
    /// stopped again, at a new epoch, while stopped: the last stop's
    /// [`Debugger::stop_location`] is over, and [`Stopped`](Self::Stopped)
    /// follows once the new one's frames come.
    State(SessionState),
    /// The program stopped there, and the file opened at its row.
    Stopped(StopLocation),
    /// Program output.
    Output(OutputBody),
    /// What the adapter says of a file's breakpoints changed
    /// ([`Debugger::verification`]).
    Breakpoints(PathBuf),
}

/// A start under way: its configuration read, its pre-launch build, or its
/// configuration resolved.
struct Starting {
    id: u64,
    name: String,
    _work: Option<Task<()>>,
}

/// One file's last `setBreakpoints`.
struct FileSend {
    /// Which send it is: 0 for the session's own, as it configured.
    generation: u64,
    lines: Vec<u32>,
}

/// The window's session.
struct Running {
    id: u64,
    /// Its configuration's name.
    name: String,
    /// The canonical root it debugs.
    root: PathBuf,
    session: Session,
    state: SessionState,
    /// Stopped by the user, the folder's trust or the root: nothing it asks
    /// for runs any more.
    stopping: bool,
    stop: Option<StopLocation>,
    /// By canonical path.
    verification: BTreeMap<PathBuf, Vec<Verification>>,
    /// Each file's last send.
    sent: BTreeMap<PathBuf, FileSend>,
    /// The number the next send gets.
    next_send: u64,
    /// Files whose breakpoints changed before the session was configured:
    /// sent once it is.
    pending: BTreeSet<PathBuf>,
    /// The debuggee tabs whose program hasn't started.
    unstarted: Vec<EntityId>,
    /// The stop epoch last seen: one that moves on retires the stop's place.
    seen_epoch: Epoch,
    /// Opening the stopped file, while it's read: dropped with its stop.
    navigation: Option<Task<()>>,
    /// Hands the session's events to the workbench.
    _pump: Task<()>,
    /// To the root's breakpoint store.
    _store: Option<Subscription>,
    store: Option<Entity<BreakpointStore>>,
}

/// A window's debugger: see the [module](self).
pub struct Debugger {
    window: AnyWindowHandle,
    terminal: WeakEntity<TerminalPanel>,
    settings: DebugSettings,
    next_id: u64,
    starting: Option<Starting>,
    session: Option<Running>,
    /// The configuration last started, by name, for `debug.rerun_last`.
    last: Option<String>,
    /// java-debug's jar, for jdtls (D21b).
    pub(crate) java: crate::debug_java::JavaJar,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DebuggerEvent> for Debugger {}

impl Debugger {
    /// The session's state: the last one's once it has ended, `None` if
    /// none has started.
    pub fn state(&self) -> Option<SessionState> {
        self.session.as_ref().map(|running| running.state.clone())
    }

    /// The configuration of the start under way, if one is.
    pub fn starting(&self) -> Option<&str> {
        self.starting.as_ref().map(|start| start.name.as_str())
    }

    /// Whether a session is starting or hasn't ended.
    pub fn is_active(&self) -> bool {
        self.starting.is_some() || (self.session.as_ref()).is_some_and(|running| !running.ended())
    }

    /// Which controls are enabled now and why the others aren't: from the
    /// session (§2, D2b); while a start is under way only Stop, which gives
    /// it up; all off with neither.
    pub fn controls(&self) -> Controls {
        if self.starting.is_some() {
            return Controls {
                stop: Control::Enabled,
                ..Controls::none(Disabled::Starting)
            };
        }
        match &self.session {
            Some(running) if !running.ended() => running.session.controls(),
            _ => Controls::none(Disabled::Ended),
        }
    }

    /// Where the program is stopped, while it is.
    pub fn stop_location(&self) -> Option<&StopLocation> {
        let running = self.session.as_ref()?;
        let stop = running.stop.as_ref()?;
        let current = running.state == SessionState::Stopped && running.live();
        (current && stop.epoch == running.session.epoch()).then_some(stop)
    }

    /// What the adapter said of the breakpoints of the file at canonical
    /// `path`, by line, this session.
    pub fn verification(&self, path: &Path) -> &[Verification] {
        (self.session.as_ref())
            .and_then(|running| running.verification.get(path))
            .map_or(&[], Vec::as_slice)
    }

    /// The configuration of the session, or of the last one.
    pub fn configuration(&self) -> Option<&str> {
        self.session.as_ref().map(|running| running.name.as_str())
    }

    /// The session, or the last one, for requests of the cards after this
    /// one (D8's steps, D9's panels).
    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref().map(|running| &running.session)
    }

    /// The session's protocol log, or the last one's.
    pub fn log(&self) -> Vec<LogEntry> {
        (self.session.as_ref()).map_or_else(Vec::new, |running| running.session.log())
    }

    /// The `[debug]` settings the next session starts with.
    pub fn settings(&self) -> &DebugSettings {
        &self.settings
    }

    fn current(&mut self, id: u64) -> Option<&mut Running> {
        self.session.as_mut().filter(|running| running.id == id)
    }

    /// Whether start `id` is the one under way.
    fn is_starting(&self, id: u64) -> bool {
        (self.starting.as_ref()).is_some_and(|start| start.id == id)
    }

    /// Begins a start of the configuration `name`, giving up any other and
    /// stopping the session: its number.
    fn begin_start(&mut self, name: &str, cx: &mut Context<Self>) -> u64 {
        self.give_up_start(cx);
        self.stop_session(cx);
        self.next_id += 1;
        self.last = Some(name.to_owned());
        self.starting = Some(Starting {
            id: self.next_id,
            name: name.to_owned(),
            _work: None,
        });
        cx.emit(DebuggerEvent::Starting(name.to_owned()));
        self.next_id
    }

    /// Keeps `work` for start `id`, if it's still the one under way:
    /// dropping it with the start cancels it.
    fn keep_work(&mut self, id: u64, work: Task<()>) {
        if let Some(start) = self.starting.as_mut().filter(|start| start.id == id) {
            start._work = Some(work);
        }
    }

    /// Gives up the start under way, if there is one.
    fn give_up_start(&mut self, cx: &mut Context<Self>) -> bool {
        let given_up = self.starting.take().is_some();
        if given_up {
            cx.emit(DebuggerEvent::StartGivenUp);
        }
        given_up
    }

    /// Gives up start `id`, if it's the one under way.
    fn give_up(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.is_starting(id) {
            self.give_up_start(cx);
        }
    }

    /// Stops the session, if it hasn't ended: from now on nothing it asks
    /// for runs, and its debuggee tabs whose process hasn't started close.
    /// Whether there was one.
    fn stop_session(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(running) = self.session.as_mut().filter(|running| !running.ended()) else {
            return false;
        };
        running.stopping = true;
        running.session.stop();
        let unstarted = std::mem::take(&mut running.unstarted);
        self.close_tabs(unstarted, cx);
        true
    }

    /// Closes the terminal tabs showing `views` whose process hasn't started
    /// by the time the window can be updated: one that has keeps its tab.
    fn close_tabs(&self, views: Vec<EntityId>, cx: &mut Context<Self>) {
        if views.is_empty() {
            return;
        }
        let (window, terminal) = (self.window, self.terminal.clone());
        cx.defer(move |cx| {
            let Some(terminal) = terminal.upgrade() else {
                return;
            };
            let closed = window.update(cx, |_, window, cx| {
                terminal.update(cx, |terminal, cx| {
                    for view in views {
                        terminal.close_unstarted(view, window, cx);
                    }
                });
            });
            closed.ok();
        });
    }
}

impl Running {
    fn ended(&self) -> bool {
        self.state.is_ended()
    }

    /// Whether the session is configured, so a file's breakpoints go now.
    fn configured(&self) -> bool {
        matches!(
            self.state,
            SessionState::Running
                | SessionState::Stopped
                | SessionState::Resuming
                | SessionState::Uncertain
        )
    }

    /// Whether what it asks for may run: not stopping or over.
    fn live(&self) -> bool {
        !self.stopping && !self.ended()
    }

    /// The verification of a `setBreakpoints` reply for `path`, sent on
    /// `lines`.
    fn verify(&mut self, path: PathBuf, lines: &[u32], answered: Vec<AdapterBreakpoint>) {
        let verification = (lines.iter().zip(answered))
            .map(|(&line, answer)| Verification {
                line,
                verified: answer.verified,
                message: answer.message,
                adapter_line: answer.line.and_then(|line| u32::try_from(line).ok()),
                id: answer.id,
            })
            .collect();
        self.verification.insert(path, verification);
    }

    /// A `breakpoint` event's change, by the id it names. The file it's
    /// in, if known.
    fn breakpoint_event(&mut self, body: BreakpointBody) -> Option<PathBuf> {
        let changed = body.breakpoint;
        let id = changed.id?;
        let (path, verification) = (self.verification.iter_mut()).find_map(|(path, all)| {
            let found = all.iter_mut().find(|entry| entry.id == Some(id))?;
            Some((path.clone(), found))
        })?;
        if body.reason == "removed" {
            let all = self.verification.get_mut(&path)?;
            all.retain(|entry| entry.id != Some(id));
            return Some(path);
        }
        verification.verified = changed.verified;
        verification.message = changed.message;
        if let Some(line) = changed.line.and_then(|line| u32::try_from(line).ok()) {
            verification.adapter_line = Some(line);
        }
        Some(path)
    }
}

/// The source path a file's breakpoints go under.
fn source_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `breakpoints` as `setBreakpoints` sends them: by line, the rest being
/// D17's.
fn source_breakpoints(breakpoints: &[fe_dap::Breakpoint]) -> Vec<SourceBreakpoint> {
    (breakpoints.iter())
        .map(|breakpoint| SourceBreakpoint {
            line: i64::from(breakpoint.line),
            ..SourceBreakpoint::default()
        })
        .collect()
}

/// Each file's enabled breakpoints in `store`, by source path.
fn store_breakpoints(store: &BreakpointStore) -> BTreeMap<String, Vec<SourceBreakpoint>> {
    (store.files().into_iter())
        .map(|path| {
            let enabled = source_breakpoints(&store.enabled(&path));
            (source_path(&path), enabled)
        })
        .filter(|(_, breakpoints)| !breakpoints.is_empty())
        .collect()
}

/// Whether a request's failure is one to tell the user of: not one the
/// session gave up itself, or one that changed nothing.
fn worth_saying(error: &RequestError) -> bool {
    match error {
        RequestError::Failed { message, .. } => {
            !matches!(message.as_deref(), Some("cancelled" | "notStopped"))
        }
        RequestError::Timeout { .. } | RequestError::BadBody { .. } => true,
        RequestError::Ended { .. }
        | RequestError::Stale { .. }
        | RequestError::NotStopped { .. }
        | RequestError::SessionOwned { .. } => false,
    }
}

/// The app's settings now, which a window's first `reload_settings` may
/// not repeat.
fn app_debug_settings(cx: &App) -> DebugSettings {
    (cx.try_global::<kiln::GlobalSettings<fe_config::Settings>>())
        .map(|global| global.settings().debug.clone())
        .unwrap_or_default()
}

/// Makes each window's [`Debugger`] with its workbench, following its
/// trust, its root and its debuggee tabs' reruns.
pub(crate) fn init(cx: &mut App) {
    cx.observe_new(|workbench: &mut Workbench, window, cx| {
        let Some(window) = window else {
            return;
        };
        let trust = cx.subscribe_self(|workbench, TrustChanged(trusted), cx| {
            if !*trusted {
                workbench.end_debugging(cx);
            }
        });
        let root = cx.subscribe_self(|workbench, _: &RootChanged, cx| {
            workbench.end_debugging(cx);
            // The last configuration was the old root's.
            if let Some(debugger) = workbench.debugger(cx) {
                debugger.update(cx, |debugger, _| debugger.last = None);
            }
        });
        let terminal = workbench.terminal().clone();
        let rerun = cx.subscribe_in(
            &terminal,
            window,
            |workbench, _, rerun: &DebuggeeRerun, window, cx| {
                // The panel keeps the root as the window opened it, a
                // symlink perhaps: both resolved.
                let root = workbench.root().map(canonical);
                if root.is_some_and(|root| root == canonical(&rerun.root)) {
                    workbench.rerun_debug_named(rerun.name.clone(), window, cx);
                }
            },
        );
        let settings = app_debug_settings(cx);
        let debugger = cx.new(|_| Debugger {
            window: window.window_handle(),
            terminal: terminal.downgrade(),
            settings,
            next_id: 0,
            starting: None,
            session: None,
            last: None,
            java: crate::debug_java::JavaJar::default(),
            _subscriptions: vec![trust, root, rerun],
        });
        let id = cx.entity_id();
        cx.default_global::<Debuggers>().0.insert(id, debugger);
        workbench.find_java_jar(cx);
        cx.on_release(move |_, cx| {
            if cx.has_global::<Debuggers>() {
                cx.global_mut::<Debuggers>().0.remove(&id);
            }
        })
        .detach();
    })
    .detach();
}

/// `path` with its symlinks resolved, or as it is if that fails.
fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// The toast an untrusted folder's start gets.
fn untrusted_toast() -> Toast {
    let trust =
        fe_command::CommandId::parse("workbench.trust_folder").expect("the command's id is valid");
    Toast::warning(tr!("workbench-debug-untrusted"))
        .action(tr!("workbench-debug-trust-folder"), trust)
}

/// Says `text` to a screen reader.
fn announce(cx: &mut App, text: impl Into<gpui::SharedString>) {
    fe_ui::announce(cx, text, fe_ui::Politeness::Polite);
}

/// The task a detected Cargo configuration's build runs as: `argv` in the
/// root, a build, its rendered diagnostics matched as cargo's are.
fn cargo_build_task(argv: &[String], root: &Path) -> ProjectTask {
    ProjectTask {
        label: argv.join(" "),
        command: TaskCommand::Argv(argv.to_vec()),
        cwd: root.to_owned(),
        env: BTreeMap::new(),
        group: Some(Group::Build),
        default: false,
        source: task_defs::Source::Cargo,
        problem_matcher: None,
    }
}

impl Workbench {
    /// This window's debugger.
    pub fn debugger(&self, cx: &Context<Self>) -> Option<Entity<Debugger>> {
        let debuggers = cx.try_global::<Debuggers>()?;
        debuggers.0.get(&cx.entity_id()).cloned()
    }

    /// Applies the `[debug]` settings to the sessions started from now on.
    pub(crate) fn reload_debug_settings(
        &mut self,
        settings: &DebugSettings,
        cx: &mut Context<Self>,
    ) {
        if let Some(debugger) = self.debugger(cx) {
            debugger.update(cx, |debugger, _| debugger.settings = settings.clone());
            // `[debug.adapters.java] path` may name another jar.
            self.find_java_jar(cx);
        }
    }

    /// Reads the root's launch configurations and tasks, off the main
    /// thread: none with no root.
    pub(crate) fn read_launch_configs(&self, cx: &Context<Self>) -> Task<Listing> {
        let Some(root) = self.root().map(canonical) else {
            return Task::ready(Listing::default());
        };
        let files = project_files(cx);
        let DebugHost { specs, login, .. } = debug_host(cx);
        let settings = (self.debugger(cx))
            .map(|debugger| debugger.read(cx).settings.clone())
            .unwrap_or_default();
        let language = (self.active_editor(cx))
            .and_then(|editor| editor.read(cx).syntax().map(|s| s.language().name()));
        // Asked now, of the jdtls running for the root (D21b).
        let java_mains = self.java_main_classes(&root, cx);
        cx.background_spawn(async move {
            let java_mains = match java_mains {
                Some(mains) => mains.await,
                None => Vec::new(),
            };
            let (tasks, _) = task_defs::load(&*files, &root);
            let registry = launch::registry(&specs, &settings, login);
            let project = launch_defs::Project::new(&*files, &root, &registry)
                .with_tasks(&tasks)
                .with_active_language(language)
                .with_java_mains(&java_mains);
            let (configs, diagnostics) = launch_defs::load(&project);
            Listing {
                configs,
                diagnostics,
                tasks,
            }
        })
    }

    /// `debug.start`: continues a stopped session, else shows the
    /// configuration picker.
    pub(crate) fn debug_start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let resumed = debugger.update(cx, |debugger, _| {
            let running = debugger.session.as_ref().filter(|running| running.live())?;
            let paused = matches!(
                running.state,
                SessionState::Stopped | SessionState::Uncertain
            );
            Some(paused && running.session.resume(Resume::Continue, None))
        });
        if resumed == Some(true) {
            return;
        }
        let reading = self.read_launch_configs(cx);
        self.show_debug_picker(reading, window, cx);
    }

    /// `debug.stop`: ends the session, or gives up a start under way.
    pub(crate) fn debug_stop(&mut self, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let stopped = debugger.update(cx, |debugger, cx| {
            let gave_up = debugger.give_up_start(cx);
            debugger.stop_session(cx) || gave_up
        });
        if !stopped {
            toast::show(cx, Toast::info(tr!("workbench-debug-not-running")));
        }
    }

    /// `debug.rerun_last`: resolves the configuration last started again,
    /// and starts it afresh.
    pub(crate) fn debug_rerun_last(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let last = (self.debugger(cx)).and_then(|debugger| debugger.read(cx).last.clone());
        match last {
            Some(name) => self.rerun_debug_named(name, window, cx),
            None => toast::show(cx, Toast::info(tr!("workbench-debug-none-run"))),
        }
    }

    /// Starts the configuration `name` afresh, once the list is read again:
    /// a start of its own from the reading on.
    pub(crate) fn rerun_debug_named(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let id = debugger.update(cx, |debugger, cx| debugger.begin_start(&name, cx));
        let reading = self.read_launch_configs(cx);
        let work = cx.spawn_in(window, async move |workbench, cx| {
            let listing = reading.await;
            let started = workbench.update_in(cx, |workbench, window, cx| {
                let Some(debugger) = workbench.debugger(cx) else {
                    return;
                };
                if !debugger.read(cx).is_starting(id) {
                    return;
                }
                let Listing { configs, tasks, .. } = listing;
                match configs.into_iter().find(|config| config.name == name) {
                    Some(config) => workbench.start_config(id, config, tasks, window, cx),
                    None => {
                        debugger.update(cx, |debugger, cx| debugger.give_up(id, cx));
                        let message = tr!("workbench-debug-config-gone", name = name.as_str());
                        toast::show(cx, Toast::error(message));
                    }
                }
            });
            started.ok();
        });
        debugger.update(cx, |debugger, _| debugger.keep_work(id, work));
    }

    /// `debug.show_log`: the session's protocol log, or the last one's, in
    /// a read-only tab.
    pub(crate) fn debug_show_log(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let debugger = debugger.read(cx);
        if debugger.session.is_none() {
            return toast::show(cx, Toast::info(tr!("workbench-debug-none-run")));
        }
        let mut text = String::new();
        for entry in debugger.log() {
            text.push_str(&entry.to_string());
            text.push('\n');
        }
        let model = EditorModel::new(Buffer::from_file_contents(&text), None);
        let editor = self.pane().update(cx, |pane, cx| {
            let editor = pane.new_editor(model, cx);
            editor.update(cx, |editor, cx| {
                editor.set_read_only(true, cx);
                editor.set_tab_title(tr!("workbench-debug-log-title"), "debug.log", cx);
            });
            pane.add_item(Box::new(editor.clone()), cx);
            editor
        });
        window.focus(&editor.focus_handle(cx), cx);
    }

    /// Starts `config`, one of the root's, whose pre-launch task is one of
    /// `tasks`: only in a trusted folder, and only if it can start. Stops
    /// the session there is first.
    pub(crate) fn start_debug_configuration(
        &mut self,
        config: LaunchConfig,
        tasks: Vec<ProjectTask>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        if !self.root_trusted() {
            return toast::show(cx, untrusted_toast());
        }
        let id = debugger.update(cx, |debugger, cx| debugger.begin_start(&config.name, cx));
        self.start_config(id, config, tasks, window, cx);
    }

    /// Start `id` of `config`: its pre-launch step, then its launch.
    fn start_config(
        &mut self,
        id: u64,
        config: LaunchConfig,
        tasks: Vec<ProjectTask>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let give_up = |toast: Toast, cx: &mut Context<Self>| {
            debugger.update(cx, |debugger, cx| debugger.give_up(id, cx));
            toast::show(cx, toast);
        };
        let Some(root) = self.root().map(canonical) else {
            return debugger.update(cx, |debugger, cx| debugger.give_up(id, cx));
        };
        if !self.root_trusted() {
            return give_up(untrusted_toast(), cx);
        }
        let cannot = |why: String| {
            let name = config.name.as_str();
            Toast::error(tr!("workbench-debug-cannot-start", name = name, why = why))
        };
        if !config.can_start() {
            return give_up(cannot(config.blockers.join("; ")), cx);
        }
        let (task, bin) = match &config.pre_launch {
            None => return self.launch_debug(id, config, None, cx),
            Some(PreLaunch::Task(label)) => {
                match tasks.into_iter().find(|task| task.label == *label) {
                    Some(task) => (task, None),
                    None => {
                        let why = tr!(
                            "workbench-debug-launch-task-not-found",
                            task = label.as_str()
                        );
                        return give_up(cannot(why.into()), cx);
                    }
                }
            }
            Some(PreLaunch::CargoBuild(argv)) => {
                let bin = match &config.program {
                    Some(Program::CargoArtifact { bin }) => Some(bin.clone()),
                    _ => None,
                };
                (cargo_build_task(argv, &root), bin)
            }
        };
        let label = task.label.clone();
        // A cargo build's artifact messages; nothing of another task's.
        let keep: fn(&str) -> bool = match bin {
            Some(_) => launch::is_artifact_message,
            None => |_| false,
        };
        let started = self.run_task_reading(
            task,
            keep,
            Box::new(move |workbench, exit, cx| {
                workbench.pre_launch_done(id, config, label, bin, exit, cx);
            }),
            window,
            cx,
        );
        if !started {
            debugger.update(cx, |debugger, cx| debugger.give_up(id, cx));
        }
    }

    /// The pre-launch task `label` of start `id` exited, or (`None`) its tab
    /// closed or reran first: the session starts only if it exited 0 (and,
    /// for a cargo build of `bin`, named the program).
    fn pre_launch_done(
        &mut self,
        id: u64,
        config: LaunchConfig,
        label: String,
        bin: Option<String>,
        exit: Option<TaskExit>,
        cx: &mut Context<Self>,
    ) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        if !debugger.read(cx).is_starting(id) {
            return;
        }
        let fail = |message: Option<gpui::SharedString>, cx: &mut Context<Self>| {
            debugger.update(cx, |debugger, cx| debugger.give_up(id, cx));
            if let Some(message) = message {
                toast::show(cx, Toast::error(message));
            }
        };
        let Some(exit) = exit else {
            return fail(None, cx);
        };
        if exit.code != Some(0) {
            let message = tr!("workbench-debug-pre-launch-failed", label = label.as_str());
            return fail(Some(message), cx);
        }
        let program = match &bin {
            Some(bin) => match launch::cargo_artifact(&exit.lines.join("\n"), bin) {
                Some(program) => Some(program),
                None => {
                    let message = tr!("workbench-debug-cargo-no-artifact", bin = bin.as_str());
                    return fail(Some(message), cx);
                }
            },
            None => None,
        };
        self.launch_debug(id, config, program, cx);
    }

    /// Resolves start `id`'s `config` off the main thread, then starts its
    /// session, if the start is still current and the folder still trusted.
    fn launch_debug(
        &mut self,
        id: u64,
        config: LaunchConfig,
        program: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let Some(root) = self.root().map(canonical) else {
            return debugger.update(cx, |debugger, cx| debugger.give_up(id, cx));
        };
        if !self.root_trusted() {
            debugger.update(cx, |debugger, cx| debugger.give_up(id, cx));
            return toast::show(cx, untrusted_toast());
        }
        let host = debug_host(cx);
        let active = (self.active_editor(cx)).and_then(|editor| {
            let editor = editor.read(cx);
            let path = editor.model().path()?.to_owned();
            let head = editor.model().primary_selection().head();
            let line = editor.model().text().offset_to_point(head).row as usize + 1;
            Some(ActiveFile { path, line })
        });
        let values = Values {
            workspace_folder: root.clone(),
            active,
            user_home: std::env::var_os("HOME").map(PathBuf::from),
            env: std::env::vars().collect(),
        };
        // A java configuration asks jdtls for its main class, classpath and
        // java-debug's port first (D21b).
        let java = match crate::debug_java::is_java(&config) {
            true => match self.java_start(&root, &config, &values, cx) {
                Ok(java) => Some(java),
                Err(why) => {
                    debugger.update(cx, |debugger, cx| debugger.give_up(id, cx));
                    let message = tr!("workbench-debug-cant-start", name = config.name.as_str());
                    return toast::show(cx, Toast::error(message).detail(why));
                }
            },
            false => None,
        };
        let settings = debugger.read(cx).settings.clone();
        let resolve = launch::Resolve {
            config: config.clone(),
            root: root.clone(),
            values,
            program,
            specs: host.specs.clone(),
            login: host.login.clone(),
            settings: settings.clone(),
            // Taken just before the launch.
            breakpoints: BTreeMap::new(),
        };
        let resolving = cx.background_spawn(async move {
            let java = match java {
                Some(java) => Some(java.run().await?),
                None => None,
            };
            launch::session_options_with(resolve, java)
        });
        let work = cx.spawn(async move |workbench, cx| {
            let options = resolving.await;
            let resolved = Resolved {
                id,
                name: config.name,
                root,
                options,
                host,
                settings,
            };
            workbench
                .update(cx, |workbench, cx| workbench.session_resolved(resolved, cx))
                .ok();
        });
        debugger.update(cx, |debugger, _| debugger.keep_work(id, work));
    }

    /// A start's configuration resolved: starts its session, if the start
    /// is still the one under way and the folder still trusted.
    fn session_resolved(&mut self, resolved: Resolved, cx: &mut Context<Self>) {
        let Resolved {
            id,
            name,
            root,
            options,
            host,
            settings,
        } = resolved;
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        if !debugger.read(cx).is_starting(id) {
            return;
        }
        let give_up = |toast: Toast, cx: &mut Context<Self>| {
            debugger.update(cx, |debugger, cx| debugger.give_up(id, cx));
            toast::show(cx, toast);
        };
        let mut options = match options {
            Ok(options) => options,
            Err(why) => {
                let message = tr!("workbench-debug-cant-start", name = name.as_str());
                return give_up(Toast::error(message).detail(why), cx);
            }
        };
        // The live check, just before the launch.
        if !self.root_trusted() || self.root().map(canonical).as_ref() != Some(&root) {
            return give_up(untrusted_toast(), cx);
        }
        // The store as it is now, and followed from now on.
        let store = self.breakpoint_store(cx);
        if let Some(store) = &store {
            options.breakpoints = store_breakpoints(store.read(cx));
        }
        let sent = (options.breakpoints.iter())
            .map(|(path, breakpoints)| {
                let lines = breakpoints.iter().map(|b| b.line as u32).collect();
                let send = FileSend {
                    generation: 0,
                    lines,
                };
                (PathBuf::from(path), send)
            })
            .collect();
        let adapter = options.adapter.clone();
        let (events, mut received) = mpsc::unbounded();
        let launcher = host.launcher(&settings);
        let session = Session::start(options, launcher, host.clock.clone(), {
            move |event| {
                let _ = events.unbounded_send(event);
            }
        });
        let session = match session {
            Ok(session) => session,
            Err(error) => {
                let message = tr!("workbench-debug-cant-start", name = name.as_str());
                return give_up(Toast::error(message).detail(error.to_string()), cx);
            }
        };
        let window = debugger.read(cx).window;
        let pump = cx.spawn(async move |workbench, cx| {
            while let Some(event) = received.next().await {
                let handled = cx.update_window(window, |_, window, cx| {
                    workbench.update(cx, |workbench, cx| {
                        workbench.debug_event(id, event, &adapter, &host, window, cx)
                    })
                });
                if !matches!(handled, Ok(Ok(()))) {
                    return;
                }
            }
        });
        let subscription = store.as_ref().map(|store| {
            cx.subscribe(store, move |workbench, _, event: &BreakpointEvent, cx| {
                workbench.debug_breakpoints_changed(id, event, cx);
            })
        });
        debugger.update(cx, |debugger, cx| {
            // The start becomes the session: no StartGivenUp.
            debugger.starting = None;
            debugger.session = Some(Running {
                id,
                name,
                root,
                session,
                state: SessionState::Launching,
                stopping: false,
                stop: None,
                verification: BTreeMap::new(),
                sent,
                next_send: 1,
                pending: BTreeSet::new(),
                unstarted: Vec::new(),
                seen_epoch: Epoch::default(),
                navigation: None,
                _pump: pump,
                _store: subscription,
                store,
            });
            cx.emit(DebuggerEvent::State(SessionState::Launching));
        });
    }

    /// Stops the session, and gives up any start under way: the folder was
    /// untrusted, or the root changed.
    fn end_debugging(&mut self, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        debugger.update(cx, |debugger, cx| {
            debugger.give_up_start(cx);
            debugger.stop_session(cx);
        });
    }

    /// An event of session `id`, on the main thread.
    fn debug_event(
        &mut self,
        id: u64,
        event: DapEvent,
        adapter: &AdapterSpec,
        host: &DebugHost,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let current = (debugger.read(cx).session.as_ref()).is_some_and(|running| running.id == id);
        if !current {
            if let DapEvent::RunInTerminal { reply, .. } = event {
                reply.failed(tr!("workbench-debug-session-over"));
            }
            return;
        }
        // Retired before the event, which may be the new stop's frames, and
        // after it; told once, with the state as it now is, unless the event
        // told the state itself.
        let told = matches!(event, DapEvent::State(_));
        let retired = self.note_epoch(id, cx);
        self.handle_dap_event(id, event, adapter, host, window, cx);
        let retired = self.note_epoch(id, cx) || retired;
        if retired && !told {
            debugger.update(cx, |debugger, cx| {
                if let Some(running) = debugger.current(id) {
                    cx.emit(DebuggerEvent::State(running.state.clone()));
                }
            });
        }
    }

    /// Whether session `id`'s stop epoch moved on since it was last seen: a
    /// new stop, a resume, a partial continue or the shown thread's exit
    /// (fe-dap's retained stops, which stay Stopped). Then the last stop's
    /// place, and any opening of its file, is over: whether it was.
    fn note_epoch(&mut self, id: u64, cx: &mut Context<Self>) -> bool {
        let Some(debugger) = self.debugger(cx) else {
            return false;
        };
        debugger.update(cx, |debugger, _| {
            let Some(running) = debugger.current(id) else {
                return false;
            };
            let epoch = running.session.epoch();
            if epoch == running.seen_epoch {
                return false;
            }
            running.seen_epoch = epoch;
            running.navigation = None;
            if running
                .stop
                .as_ref()
                .is_some_and(|stop| stop.epoch != epoch)
            {
                running.stop = None;
            }
            true
        })
    }

    fn handle_dap_event(
        &mut self,
        id: u64,
        event: DapEvent,
        adapter: &AdapterSpec,
        host: &DebugHost,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        match event {
            DapEvent::State(state) => self.debug_state(id, state, adapter, host, cx),
            DapEvent::Breakpoints { path, breakpoints } => {
                self.session_sent_breakpoints(id, PathBuf::from(path), breakpoints, cx);
            }
            DapEvent::Breakpoint(body) => debugger.update(cx, |debugger, cx| {
                if let Some(path) = debugger.current(id).and_then(|r| r.breakpoint_event(body)) {
                    cx.emit(DebuggerEvent::Breakpoints(path));
                }
            }),
            DapEvent::Frames {
                epoch,
                thread_id,
                frames,
                ..
            } => self.debug_frames(id, epoch, thread_id, frames, window, cx),
            DapEvent::Output(body) => {
                debugger.update(cx, |_, cx| cx.emit(DebuggerEvent::Output(body)));
            }
            DapEvent::Failed(error) => self.request_failed(id, &error, cx),
            DapEvent::RunInTerminal {
                args,
                cwd,
                env,
                reply,
                ..
            } => {
                let debuggee = Debuggee {
                    name: String::new(),
                    argv: args,
                    cwd: PathBuf::from(cwd),
                    env,
                };
                self.run_debuggee(id, debuggee, reply, window, cx);
            }
            _ => {}
        }
    }

    /// Says a request of session `id` failed while the session carries on,
    /// unless the failure changed nothing.
    fn request_failed(&mut self, id: u64, error: &RequestError, cx: &mut Context<Self>) {
        log::warn!("debug session: {error}");
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let name = (debugger.read(cx).session.as_ref())
            .filter(|running| running.id == id && running.live())
            .map(|running| running.name.clone());
        let Some(name) = name else {
            return;
        };
        if worth_saying(error) {
            let message = tr!("workbench-debug-request-failed", name = name.as_str());
            toast::show(cx, Toast::warning(message).detail(error.to_string()));
        }
    }

    /// Session `id` moved to `state`.
    fn debug_state(
        &mut self,
        id: u64,
        state: SessionState,
        adapter: &AdapterSpec,
        host: &DebugHost,
        cx: &mut Context<Self>,
    ) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let (was, flush) = debugger.update(cx, |debugger, cx| {
            let Some(running) = debugger.current(id) else {
                return (None, Vec::new());
            };
            let was = std::mem::replace(&mut running.state, state.clone());
            if state != SessionState::Stopped {
                running.stop = None;
                running.navigation = None;
            }
            let flush = if running.configured() {
                std::mem::take(&mut running.pending).into_iter().collect()
            } else {
                Vec::new()
            };
            let mut unstarted = Vec::new();
            if state.is_ended() {
                running._store = None;
                running.pending.clear();
                unstarted = std::mem::take(&mut running.unstarted);
            }
            debugger.close_tabs(unstarted, cx);
            cx.emit(DebuggerEvent::State(state.clone()));
            (Some(was), flush)
        });
        let Some(was) = was else {
            return;
        };
        for path in flush {
            self.send_breakpoints(id, &path, cx);
        }
        match &state {
            SessionState::Running if was != SessionState::Running => {
                announce(cx, tr!("workbench-debug-running"));
            }
            SessionState::Ended(ended) if !was.is_ended() => {
                announce(cx, tr!("workbench-debug-session-ended"));
                match &ended.reason {
                    EndReason::Failed(error) => self.debug_failed(id, error, adapter, host, cx),
                    EndReason::AdapterExited(_) => {
                        let message = tr!(
                            "workbench-debug-adapter-exited",
                            adapter = adapter.name.as_str()
                        );
                        toast::show(cx, Toast::error(message));
                    }
                    EndReason::Finished | EndReason::Stopped => {}
                }
            }
            _ => {}
        }
    }

    /// Says why session `id` failed: in Tenmoku's words for a ptrace or
    /// signing refusal.
    fn debug_failed(
        &mut self,
        id: u64,
        error: &SessionError,
        adapter: &AdapterSpec,
        host: &DebugHost,
        cx: &mut Context<Self>,
    ) {
        let name = (self.debugger(cx))
            .and_then(|debugger| {
                let debugger = debugger.read(cx);
                let running = debugger.session.as_ref().filter(|r| r.id == id)?;
                Some(running.name.clone())
            })
            .unwrap_or_default();
        let said = match error {
            SessionError::Launch(why) => Some(why.as_str()),
            SessionError::StartFailed(Some(why)) | SessionError::InitializeFailed(Some(why)) => {
                Some(why.as_str())
            }
            _ => None,
        };
        let scope = host.ptrace_scope.clone();
        let refusal = said.and_then(|said| launch::refusal(said, adapter, host.os, || scope()));
        let message = tr!("workbench-debug-session-failed", name = name.as_str());
        let toast = match refusal {
            Some(refusal) => Toast::error(refusal.message()).detail(error.to_string()),
            None => Toast::error(message).detail(error.to_string()),
        };
        toast::show(cx, toast);
    }

    /// A stop's first frames: opens the top frame's file at its row and
    /// says so, if the stop is still the session's.
    fn debug_frames(
        &mut self,
        id: u64,
        epoch: Epoch,
        thread_id: i64,
        frames: Vec<StackFrame>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let Some(frame) = frames.into_iter().next() else {
            return;
        };
        let line = u32::try_from(frame.line.max(1)).unwrap_or(1);
        let path = (frame.source.as_ref())
            .and_then(|source| source.path.as_deref())
            .filter(|path| !path.is_empty())
            .map(PathBuf::from);
        let location = StopLocation {
            epoch,
            thread_id,
            frame,
            path: path.clone(),
            row: line - 1,
        };
        let shown = debugger.update(cx, |debugger, cx| {
            let running = debugger.current(id).filter(|running| running.live())?;
            if running.session.epoch() != epoch || running.state != SessionState::Stopped {
                return None;
            }
            running.stop = Some(location.clone());
            cx.emit(DebuggerEvent::Stopped(location));
            Some(())
        });
        if shown.is_none() {
            return;
        }
        if let Some(path) = path {
            self.show_stop(id, epoch, path, line, window, cx);
        }
        announce(cx, tr!("workbench-debug-stopped-at-line", line = line));
    }

    /// Opens `path` with the caret on 1-based `line` for session `id`'s stop
    /// at `epoch`: at once if it's in a tab, else once it's read off the
    /// main thread, and then only if that stop is still the session's. The
    /// read goes with the stop ([`Running::navigation`]).
    fn show_stop(
        &mut self,
        id: u64,
        epoch: Epoch,
        path: PathBuf,
        line: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A Save As holding the file: the stop opens it once that's let go,
        // if it's still current, so the saving buffer is the one shown.
        let saving = crate::open::any_reserved()
            .then(|| {
                let identity = crate::open::file_identity(&path);
                crate::open::reserved(&path, identity.as_deref())
            })
            .flatten()
            .map(|(_, over)| over);
        if saving.is_none() && self.pane().read(cx).position(&path).is_some() {
            return self
                .open_document_at(path, Some(line), None, window, cx)
                .detach();
        }
        #[cfg(test)]
        let gate = cx.try_global::<StopOpenGate>().map(|gate| gate.0.clone());
        let read = saving.is_none().then(|| {
            cx.background_spawn({
                let path = path.clone();
                async move {
                    #[cfg(test)]
                    if let Some(gate) = gate {
                        gate.recv().await.ok();
                    }
                    crate::loader::open(path)
                }
            })
        });
        let navigation = cx.spawn_in(window, async move |workbench, cx| {
            if let Some(over) = saving {
                over.recv().await.ok();
            }
            let result = match read {
                Some(read) => Some(read.await),
                None => None,
            };
            let landed = workbench.update_in(cx, |workbench, window, cx| {
                if !workbench.stop_is_current(id, epoch, cx) {
                    return;
                }
                let pane = workbench.pane().clone();
                if pane.read(cx).position(&path).is_none() {
                    let Some(result) = result else {
                        // The Save As is over and left no tab of the file:
                        // read it now, as the stop's own navigation.
                        return workbench.show_stop(id, epoch, path, line, window, cx);
                    };
                    // The read lands now, in the tab it opens.
                    pane.update(cx, |pane, cx| {
                        pane.open_at(path.clone(), None, window, cx);
                        pane.file_read(path.clone(), result, window, cx);
                    });
                    // A failed read opens no tab, and the stop's caret goes
                    // nowhere: no read again outside the stop's guard.
                    if pane.read(cx).position(&path).is_none() {
                        return;
                    }
                }
                (workbench.open_document_at(path, Some(line), None, window, cx)).detach();
            });
            landed.ok();
        });
        if let Some(debugger) = self.debugger(cx) {
            debugger.update(cx, |debugger, _| {
                if let Some(running) = debugger.current(id) {
                    running.navigation = Some(navigation);
                }
            });
        }
    }

    /// Whether session `id` is live and stopped at `epoch`, its place shown.
    fn stop_is_current(&self, id: u64, epoch: Epoch, cx: &Context<Self>) -> bool {
        let Some(debugger) = self.debugger(cx) else {
            return false;
        };
        let debugger = debugger.read(cx);
        (debugger.session.as_ref()).is_some_and(|running| {
            running.id == id
                && running.live()
                && running.state == SessionState::Stopped
                && running.session.epoch() == epoch
                && running
                    .stop
                    .as_ref()
                    .is_some_and(|stop| stop.epoch == epoch)
        })
    }

    /// Runs what session `id`'s adapter asks for in the configuration's
    /// tab, and answers with its process id once it has started. Refused
    /// unless the session is live, in this root, and the root trusted; and
    /// given up, its tab closed, if the session stops or the program
    /// doesn't start within [`TERMINAL_START`].
    fn run_debuggee(
        &mut self,
        id: u64,
        mut debuggee: Debuggee,
        reply: TerminalReply,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(debugger) = self.debugger(cx) else {
            return reply.failed(tr!("workbench-debug-session-over"));
        };
        let live = (debugger.read(cx).session.as_ref())
            .filter(|running| running.id == id && running.live())
            .map(|running| (running.name.clone(), running.root.clone()));
        let Some((name, root)) = live else {
            return reply.failed(tr!("workbench-debug-session-over"));
        };
        // Only a trusted folder's session can ask, but trust is live.
        if !self.root_trusted() || self.root().map(canonical).as_ref() != Some(&root) {
            return reply.failed(tr!("workbench-debug-untrusted"));
        }
        debuggee.name = name;
        if !debuggee.cwd.is_absolute() {
            debuggee.cwd = root.join(&debuggee.cwd);
        }
        let terminal = self.terminal().clone();
        let view = terminal.update(cx, |terminal, cx| {
            terminal.add_debuggee(&debuggee, window, cx)
        });
        let view = match view {
            Ok(view) => view,
            Err(error) => return reply.failed(error.to_string()),
        };
        let view_id = view.entity_id();
        debugger.update(cx, |debugger, _| {
            if let Some(running) = debugger.current(id) {
                running.unstarted.push(view_id);
            }
        });
        self.show_terminal_dock(window, cx);
        let timer = cx.background_executor().clone();
        let (terminal, view) = (terminal.downgrade(), view.downgrade());
        cx.spawn(async move |workbench, cx| {
            let tick = Duration::from_millis(20);
            let mut waited = Duration::ZERO;
            loop {
                let checked = workbench.update(cx, |workbench, cx| {
                    let debugger = workbench.debugger(cx)?;
                    let live = (debugger.read(cx).session.as_ref())
                        .is_some_and(|running| running.id == id && running.live());
                    let tab = terminal.upgrade().zip(view.upgrade());
                    // Whether its process has started, live session or not:
                    // a started one keeps its tab.
                    let pid = (tab.as_ref())
                        .and_then(|(terminal, view)| terminal.read(cx).started_pid(view, cx));
                    match pid {
                        Some(pid) => {
                            debugger.update(cx, |debugger, _| {
                                if let Some(running) = debugger.current(id) {
                                    running.unstarted.retain(|unstarted| *unstarted != view_id);
                                }
                            });
                            // The adapter is told only while it's the live
                            // session's.
                            Some(if live { Ok(pid) } else { Err(()) })
                        }
                        None if live && tab.is_some() && waited < TERMINAL_START => None,
                        None => {
                            // Given up: the tab mustn't start it later, if
                            // it still hasn't once the close runs.
                            debugger.update(cx, |debugger, cx| {
                                if let Some(running) = debugger.current(id) {
                                    running.unstarted.retain(|unstarted| *unstarted != view_id);
                                }
                                debugger.close_tabs(vec![view_id], cx);
                            });
                            Some(Err(()))
                        }
                    }
                });
                match checked {
                    Ok(Some(Ok(pid))) => {
                        return reply.started(TerminalStarted {
                            process_id: pid.map(i64::from),
                            shell_process_id: None,
                        });
                    }
                    Ok(None) => {}
                    _ => return reply.failed(tr!("workbench-debug-terminal-didnt-start")),
                }
                timer.timer(tick).await;
                waited += tick;
            }
        })
        .detach();
    }

    /// The store said a file's breakpoints changed: session `id` sends the
    /// file's set again, once configured, unless only unsaved rows moved.
    fn debug_breakpoints_changed(
        &mut self,
        id: u64,
        event: &BreakpointEvent,
        cx: &mut Context<Self>,
    ) {
        let path = match event {
            BreakpointEvent::Changed(path) | BreakpointEvent::Saved(path) => path.clone(),
            BreakpointEvent::Moved(_) => return,
        };
        self.breakpoints_due(id, path, cx);
    }

    /// Sends the file at `path`'s breakpoints to session `id` now if it's
    /// configured, else once it is.
    fn breakpoints_due(&mut self, id: u64, path: PathBuf, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let now = debugger.update(cx, |debugger, _| {
            let running = debugger.current(id).filter(|running| running.live())?;
            if running.configured() {
                return Some(true);
            }
            running.pending.insert(path.clone());
            Some(false)
        });
        if now == Some(true) {
            self.send_breakpoints(id, &path, cx);
        }
    }

    /// The session's own `setBreakpoints` for `path` was answered: as it
    /// configured, the set it started with; after a send of the host's (a
    /// restart sending its first set again), a stale one, which the store's
    /// set replaces.
    fn session_sent_breakpoints(
        &mut self,
        id: u64,
        path: PathBuf,
        breakpoints: Vec<AdapterBreakpoint>,
        cx: &mut Context<Self>,
    ) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let stale = debugger.update(cx, |debugger, cx| {
            let running = debugger.current(id)?;
            let lines = match running.sent.get(&path) {
                Some(send) if send.generation == 0 => send.lines.clone(),
                None => Vec::new(),
                Some(_) => return Some(true),
            };
            running.verify(path.clone(), &lines, breakpoints);
            cx.emit(DebuggerEvent::Breakpoints(path.clone()));
            Some(false)
        });
        if stale == Some(true) {
            self.breakpoints_due(id, path, cx);
        }
    }

    /// Sends the file at `path`'s enabled breakpoints to session `id`, and
    /// keeps what the adapter says of them, if no later send has gone.
    fn send_breakpoints(&mut self, id: u64, path: &Path, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let pending = debugger.update(cx, |debugger, cx| {
            let running = debugger.current(id).filter(|running| running.live())?;
            let store = running.store.clone()?;
            let breakpoints = source_breakpoints(&store.read(cx).enabled(path));
            let lines: Vec<u32> = breakpoints.iter().map(|b| b.line as u32).collect();
            let generation = running.next_send;
            running.next_send += 1;
            let send = FileSend {
                generation,
                lines: lines.clone(),
            };
            running.sent.insert(path.to_owned(), send);
            let arguments = SetBreakpointsArguments {
                source: Source {
                    name: path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned()),
                    path: Some(source_path(path)),
                    ..Source::default()
                },
                breakpoints: Some(breakpoints),
                ..SetBreakpointsArguments::default()
            };
            let reply = running
                .session
                .request::<request::SetBreakpoints>(&arguments);
            Some((reply, lines, generation))
        });
        let Some((reply, lines, generation)) = pending else {
            return;
        };
        let path = path.to_owned();
        cx.spawn(async move |workbench, cx| {
            let reply: Result<BreakpointsBody, _> = reply.await;
            workbench
                .update(cx, |workbench, cx| {
                    let body = match reply {
                        Ok(body) => body,
                        Err(error) => return workbench.request_failed(id, &error, cx),
                    };
                    let Some(debugger) = workbench.debugger(cx) else {
                        return;
                    };
                    debugger.update(cx, |debugger, cx| {
                        let Some(running) = debugger.current(id) else {
                            return;
                        };
                        // A later send's reply has the last word.
                        let last = running.sent.get(&path).map(|send| send.generation);
                        if last != Some(generation) {
                            return;
                        }
                        running.verify(path.clone(), &lines, body.breakpoints);
                        cx.emit(DebuggerEvent::Breakpoints(path));
                    });
                })
                .ok();
        })
        .detach();
    }
}

/// Holds each stopped file's read until the test sends: a file still
/// loading when its stop ends.
#[cfg(test)]
pub(crate) struct StopOpenGate(pub(crate) async_channel::Receiver<()>);

#[cfg(test)]
impl Global for StopOpenGate {}

/// A start's configuration, resolved off the main thread.
struct Resolved {
    id: u64,
    name: String,
    root: PathBuf,
    options: Result<fe_dap::SessionOptions, String>,
    host: DebugHost,
    settings: DebugSettings,
}
