// SPDX-License-Identifier: Apache-2.0

//! A debug session (docs/decisions/r1b9dw3g0g-debugger.md §2, card D2):
//! [`Session`] is the handle, and its actor runs on a thread of its own
//! named `fe-dap <adapter>`, under `async_io::block_on`.
//!
//! **Startup** (§2 items 1-3). The actor launches the adapter, sends
//! `initialize` and keeps its [`Capabilities`] (a `capabilities` event
//! merges into them), then sends `launch` (or `attach`) at once, without
//! awaiting its response. On each `initialized` it configures: a
//! `setBreakpoints` per file, `setExceptionBreakpoints` only if the adapter
//! offers exception filters or lacks `configurationDone`, and, once those
//! are answered, `configurationDone` if supported. Configuration is
//! complete on `configurationDone`'s response, or without it on the last
//! configuration reply; if the `launch` response succeeded and no
//! `initialized` comes within [`INITIALIZED_GRACE`], it counts as complete
//! with nothing sent (a later `initialized` still configures). Once both
//! the `launch` response and configuration completion are in, the actor
//! sends `threads` once: the thread baseline.
//!
//! **`configurationDone` has a timeout** (§13 F10): unanswered within
//! [`CONFIGURATION_DONE_TIMEOUT`], the adapter is killed and the session
//! ends with [`SessionError::ConfigurationDoneTimeout`].
//!
//! **Restart** (item 7): with `supportsRestartRequest`, `restart` moves to
//! [`SessionState::Restarting`]; its success, while it is still the pending
//! restart and the state is still Restarting, moves to Running with a fresh
//! `threads` baseline, and the adapter's next `initialized` configures
//! again. Its failure or timeout, or an adapter without the request, falls
//! back to emulation: the session stops the adapter (item 6) and starts a
//! new one from the same options, on the same handle.
//!
//! **Termination** (item 6): the user's [`Session::stop`] sends `terminate`
//! when supported, then `disconnect { terminateDebuggee }` on `terminated`
//! or after [`TERMINATED_GRACE`]; without it, `disconnect` at once. The
//! adapter's own `terminated` gets `disconnect` at once and never
//! `terminate`. Either way an adapter still running [`EXIT_GRACE`] after
//! `disconnect`'s response (or its timeout) is killed. An adapter exiting
//! on its own ends the session as [`EndReason::AdapterExited`], with no
//! automatic restart.
//!
//! **Requests** ([`Session::request`]) time out on the injected clock,
//! after the session's [`Timeouts`]: [`REQUEST_TIMEOUT`] by default,
//! [`LAUNCH_TIMEOUT`] for `launch` and `attach`, none for `evaluate` in the
//! `repl` context. A request future
//! dropped before its reply sends `cancel` when the adapter supports it,
//! and nothing waits for its reply after.
//!
//! **Reverse requests:** `runInTerminal` reaches the sink as
//! [`DapEvent::RunInTerminal`], whose [`TerminalReply`] answers it;
//! `startDebugging` is refused, since child sessions aren't supported.
//!
//! **Stops and resumes** (§2 items 4-5, §13 F2 and F11; card D2b). The
//! [`Epoch`] names the current suspension: a `stopped` event, a
//! `continued` event, sending a resume ([`Session::resume`]: `continue`,
//! `next`, `stepIn`, `stepOut`) and sending `restart` each move it on, so
//! every [`StopRef`] the host holds from an earlier one answers
//! [`RequestError::Stale`] locally, and a fetch's reply that comes under
//! another epoch is dropped. On each stop the session fetches lazily:
//! `threads`, `stackTrace` for the stopped thread ([`FRAME_PAGE`] frames,
//! more through [`Session::frames`]), `scopes` for the top frame, and
//! `variables` for each scope not marked `expensive`, into
//! [`Session::stop_data`]; deeper children only on request.
//!
//! A resume moves to [`SessionState::Resuming`] and is the *pending
//! resume*; its completion applies only through the guard (still the
//! pending resume, the same epoch, still Resuming, or Uncertain by its own
//! timeout). Its success moves to Running with no `continued` needed, or,
//! for a `continue` answered `allThreadsContinued: false`, to a stop on the
//! threads left suspended, fetched again; its timeout `cancel`s it and
//! moves to [`SessionState::Uncertain`]. Its failure, `"cancelled"`
//! included, also leaves the session Uncertain (§13 F2, over §2's "returns
//! to Stopped"), unless the adapter said `notStopped`, which is Running.
//! The session then probes, `threads` and then `stackTrace` on one thread:
//! an answer `notStopped` moves to Running, and a stack with frames proves
//! a stop (at a new epoch, data fetched again) only with
//! [`SessionOptions::stack_proves_stop`]; otherwise only a `stopped` event
//! does. Stop data (`stackTrace`, `scopes`, `variables`, `evaluate`, …) is
//! asked for only while Stopped, of a suspended thread, and every such
//! request, typed or raw, is of the stop it went in; a frameless `evaluate`
//! may go while running ([`RequestError::NotStopped`] otherwise).
//!
//! [`Session::pause`] targets the given thread, else the one that last
//! stopped, else the first known (the startup baseline's), and with none
//! known sends `threads` first. [`Session::controls`] says which of
//! Continue, Pause, the steps, Restart and Stop are enabled and why not,
//! for the host's toolbar and commands; once the user has stopped the
//! session (§13 F11) none ever is again, whatever late event comes.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};
use std::thread;
use std::time::Duration;

use futures::channel::{mpsc, oneshot};
use kiln_l10n::{tr, verbatim};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::clock::Clock;
use crate::launcher::{AdapterLauncher, AdapterSpec, ExitInfo, LaunchCancel};
use crate::log::{EnvExpansion, LogEntry, LogMode, ProtocolLog};
use crate::protocol::{
    BreakpointBody, Capabilities, ContinuedBody, ErrorMessage, EvaluateArguments, EvaluateBody,
    EventMessage, Object, OutputBody, Request, Scope, ScopesArguments, ScopesBody,
    SourceBreakpoint, StackFrame, StackTraceArguments, StackTraceBody, StoppedBody, Thread,
    Variable, VariablesArguments, VariablesBody, request,
};

mod actor;
mod controls;

pub use controls::{Control, Controls, Disabled};

/// A request's timeout unless its command has its own.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// `launch`'s and `attach`'s timeout.
pub const LAUNCH_TIMEOUT: Duration = Duration::from_secs(30);
/// `configurationDone`'s timeout (§13 F10): on expiry the session ends.
pub const CONFIGURATION_DONE_TIMEOUT: Duration = REQUEST_TIMEOUT;
/// How long after a successful `launch` response the session waits for
/// `initialized` before counting configuration complete.
pub const INITIALIZED_GRACE: Duration = Duration::from_secs(5);
/// How long after the user's `terminate` the session waits for
/// `terminated` before sending `disconnect`.
pub const TERMINATED_GRACE: Duration = Duration::from_secs(2);
/// How long after `disconnect`'s response (or timeout) the adapter has to
/// exit before it is killed.
pub const EXIT_GRACE: Duration = Duration::from_secs(2);
/// How long, once the adapter has exited, the session goes on reading what
/// it sent before, from streams that stay open.
pub const EXIT_DRAIN: Duration = Duration::from_secs(1);
/// How many frames a `stackTrace` asks for: the stop's first page, and
/// each further page [`Session::frames`] fetches.
pub const FRAME_PAGE: i64 = 20;

/// How long the adapter may take to answer (the user's `debug.timeouts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeouts {
    /// Any request's, `configurationDone`'s too, but `launch`'s and
    /// `attach`'s.
    pub request: Duration,
    /// `launch`'s and `attach`'s.
    pub launch: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            request: REQUEST_TIMEOUT,
            launch: LAUNCH_TIMEOUT,
        }
    }
}

/// What starts the debuggee.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StartRequest {
    #[default]
    Launch,
    Attach,
}

/// Everything a session is started from: the resolved configuration.
#[derive(Clone, Debug, Default)]
pub struct SessionOptions {
    pub adapter: AdapterSpec,
    /// The debuggee's working directory, given to the launcher.
    pub cwd: PathBuf,
    pub request: StartRequest,
    /// `launch` or `attach` arguments (the configuration's, expanded), and
    /// `restart`'s.
    pub arguments: Object,
    /// By source path: the breakpoints each `setBreakpoints` sends.
    pub breakpoints: BTreeMap<String, Vec<SourceBreakpoint>>,
    /// The exception filters to turn on; `None` for the ones the adapter
    /// marks `default`.
    pub exception_filters: Option<Vec<String>>,
    /// The protocol log's mode (D7 sets it from `debug.log_payloads`).
    pub log: LogMode,
    /// What each `${env:NAME}` in the configuration expanded to, for the
    /// log's redaction.
    pub expansions: Vec<EnvExpansion>,
    /// The adapter answers `stackTrace` only for a suspended thread, so a
    /// stack Uncertain's probe gets proves a stop (§13 F2). Off by default:
    /// debugpy, for one, answers with a running thread's stack after a
    /// short wait, and then only a `stopped` event ends Uncertain.
    pub stack_proves_stop: bool,
    /// How long its requests may go unanswered.
    pub timeouts: Timeouts,
}

/// Where a session is (§2's state machine).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SessionState {
    /// Starting the adapter.
    #[default]
    Launching,
    /// `initialize` sent; no `initialized` yet.
    Initializing,
    /// `initialized` seen; configuration or the `launch` response still to
    /// come.
    Configuring,
    Running,
    Stopped,
    /// A resume (`continue`, `next`, `stepIn`, `stepOut`) is in flight: the
    /// stop's data is retired, and stepping and Continue are disabled.
    Resuming,
    /// The program may be running or still stopped: a resume went
    /// unanswered, or failed (§13 F2). Continue, Pause and Stop work;
    /// stepping and fetching stop data don't, until a stop is proven.
    Uncertain,
    /// A `restart` request is in flight.
    Restarting,
    /// Shutting down: late completions are ignored.
    Terminating,
    Ended(Ended),
}

impl SessionState {
    pub fn is_ended(&self) -> bool {
        matches!(self, SessionState::Ended(_))
    }
}

/// How a session ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ended {
    /// The debuggee's exit code, from an `exited` event.
    pub exit_code: Option<i64>,
    pub reason: EndReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// The adapter said the debuggee ended (`terminated`).
    Finished,
    /// The user stopped it.
    Stopped,
    /// The adapter went away by itself. Not restarted.
    AdapterExited(ExitInfo),
    Failed(SessionError),
}

/// Why a session failed: each a named error the host shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionError {
    /// The launcher couldn't start the adapter: its message.
    Launch(String),
    /// `initialize` failed, with the adapter's message.
    InitializeFailed(Option<String>),
    /// `initialize` went unanswered for this long.
    InitializeTimeout(Duration),
    /// `launch` or `attach` failed, with the adapter's message.
    StartFailed(Option<String>),
    /// `launch` or `attach` went unanswered for this long.
    StartTimeout(Duration),
    /// `configurationDone` went unanswered for this long (§13 F10).
    ConfigurationDoneTimeout(Duration),
    /// The adapter broke the protocol.
    Transport(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            SessionError::Launch(why) => tr!("dap-session-launch-failed", why = why.as_str()),
            SessionError::InitializeFailed(message) => match message {
                Some(message) => tr!("dap-session-initialize-failed", why = message.as_str()),
                None => tr!("dap-session-initialize-failed-bare"),
            },
            SessionError::InitializeTimeout(after) => {
                tr!("dap-session-initialize-timeout", seconds = after.as_secs())
            }
            SessionError::StartFailed(message) => match message {
                Some(message) => tr!("dap-session-start-failed", why = message.as_str()),
                None => tr!("dap-session-start-failed-bare"),
            },
            SessionError::StartTimeout(after) => {
                tr!("dap-session-start-timeout", seconds = after.as_secs())
            }
            SessionError::ConfigurationDoneTimeout(after) => tr!(
                "dap-session-configuration-done-timeout",
                seconds = after.as_secs()
            ),
            SessionError::Transport(why) => tr!("dap-session-transport", why = why.as_str()),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for SessionError {}

/// Why a request got no body.
#[derive(Clone, Debug, PartialEq)]
pub enum RequestError {
    /// The adapter answered `success: false`: its short `message`
    /// (`"cancelled"`, `"notStopped"` or free text) and detailed error.
    Failed {
        command: String,
        message: Option<String>,
        error: Option<ErrorMessage>,
    },
    /// No answer within the command's timeout.
    Timeout { command: String, after: Duration },
    /// The session is shutting down or over.
    Ended { command: String },
    /// The reply's body isn't the command's.
    BadBody { command: String, why: String },
    /// Its [`StopRef`] is of a stop that's over (or its reply came after
    /// that stop ended): answered locally, without a round trip.
    Stale { command: String },
    /// Not sent: the program may be running, so there is no stop data to
    /// ask about (Resuming or Uncertain), or it isn't stopped (a resume
    /// while running).
    NotStopped { command: String },
    /// Not sent: the session sends this command itself (startup,
    /// [`Session::restart`], [`Session::stop`]), so its epochs and guards
    /// hold.
    SessionOwned { command: String },
}

impl RequestError {
    /// The adapter's short `message`, for a failure.
    pub fn message(&self) -> Option<&str> {
        match self {
            RequestError::Failed { message, .. } => message.as_deref(),
            _ => None,
        }
    }
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            RequestError::Failed {
                command, message, ..
            } => match message {
                Some(message) => tr!(
                    "dap-request-failed",
                    command = command.as_str(),
                    why = message.as_str()
                ),
                None => tr!("dap-request-failed-bare", command = command.as_str()),
            },
            RequestError::Timeout { command, after } => tr!(
                "dap-request-timeout",
                command = command.as_str(),
                seconds = after.as_secs()
            ),
            RequestError::Ended { command } => {
                tr!("dap-request-ended", command = command.as_str())
            }
            RequestError::BadBody { command, why } => tr!(
                "dap-request-bad-body",
                command = command.as_str(),
                why = why.as_str()
            ),
            RequestError::Stale { command } => {
                tr!("dap-request-stale", command = command.as_str())
            }
            RequestError::NotStopped { command } => {
                tr!("dap-request-not-stopped", command = command.as_str())
            }
            RequestError::SessionOwned { command } => {
                tr!("dap-request-session-owned", command = command.as_str())
            }
        };
        f.write_str(&text)
    }
}

impl std::error::Error for RequestError {}

/// What a session tells its host, on the session thread.
#[derive(Debug)]
pub enum DapEvent {
    State(SessionState),
    /// The adapter's capabilities: `initialize`'s, or after a
    /// `capabilities` event.
    Capabilities(Capabilities),
    /// The known threads: the baseline's reply, or a `thread` event's
    /// change.
    Threads(Vec<Thread>),
    /// A `setBreakpoints` reply for one source path.
    Breakpoints {
        path: String,
        breakpoints: Vec<crate::protocol::Breakpoint>,
    },
    /// A `breakpoint` event.
    Breakpoint(BreakpointBody),
    Stopped(StoppedBody),
    Continued(ContinuedBody),
    /// The stop's frames for `thread_id`, fetched at `epoch`: the first
    /// [`FRAME_PAGE`].
    Frames {
        epoch: Epoch,
        thread_id: i64,
        frames: Vec<StackFrame>,
        total_frames: Option<i64>,
    },
    /// The top frame's scopes.
    Scopes {
        frame: StopRef,
        scopes: Vec<Scope>,
    },
    /// A scope's variables (a scope not marked `expensive`).
    Variables {
        reference: StopRef,
        variables: Vec<Variable>,
    },
    /// The debuggee's exit code.
    Exited(i64),
    /// Program output (telemetry is dropped).
    Output(OutputBody),
    /// A request of the session's own failed and the session carries on
    /// (a failed `configurationDone`, `setBreakpoints` or `restart`): shown,
    /// not fatal.
    Failed(RequestError),
    /// The adapter asks for a command to be run in a terminal. Answer with
    /// `reply`; dropping it answers a failure.
    RunInTerminal {
        args: Vec<String>,
        cwd: String,
        /// A `None` value removes the variable.
        env: BTreeMap<String, Option<String>>,
        /// `"integrated"` or `"external"`, if given.
        kind: Option<String>,
        title: Option<String>,
        reply: TerminalReply,
    },
    /// Any other event: `invalidated`, `process`, progress, or the
    /// adapter's own.
    Other(EventMessage),
}

/// What a terminal started for `runInTerminal`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalStarted {
    pub process_id: Option<i64>,
    pub shell_process_id: Option<i64>,
}

/// Answers one `runInTerminal`. Dropped unanswered, it answers a failure.
#[derive(Debug)]
pub struct TerminalReply(oneshot::Sender<Result<TerminalStarted, String>>);

impl TerminalReply {
    /// The terminal started: the adapter gets its process ids.
    pub fn started(self, started: TerminalStarted) {
        let _ = self.0.send(Ok(started));
    }

    /// The terminal couldn't start: the adapter gets `message`.
    pub fn failed(self, message: impl Into<String>) {
        let _ = self.0.send(Err(message.into()));
    }
}

/// Names one suspension of the debuggee (§2 item 4): `variablesReference`
/// and `frameId` values are valid only within the epoch they came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Epoch(u64);

impl Epoch {
    pub fn get(self) -> u64 {
        self.0
    }

    fn next(self) -> Epoch {
        Epoch(self.0 + 1)
    }
}

/// An id from one stop, a frame's `id` or a `variablesReference`, with the
/// epoch it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StopRef {
    pub epoch: Epoch,
    pub id: i64,
}

impl StopRef {
    pub fn new(epoch: Epoch, id: i64) -> Self {
        Self { epoch, id }
    }
}

/// What the session fetched for the current stop (§2 item 5).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StopData {
    pub epoch: Epoch,
    /// The `stopped` event's body; `None` for a stop the session proved
    /// with a probe (§13 F2).
    pub stopped: Option<StoppedBody>,
    /// The thread whose frames these are, once known.
    pub thread_id: Option<i64>,
    /// The first [`FRAME_PAGE`] frames, top first.
    pub frames: Vec<StackFrame>,
    pub total_frames: Option<i64>,
    /// The top frame's scopes.
    pub scopes: Vec<Scope>,
    /// By `variablesReference`: each scope's variables that was fetched
    /// (those not marked `expensive`).
    pub variables: BTreeMap<i64, Vec<Variable>>,
}

impl StopData {
    /// Frame `id` (or any id) of this stop, as a [`StopRef`].
    pub fn stop_ref(&self, id: i64) -> StopRef {
        StopRef::new(self.epoch, id)
    }

    /// The top frame, as a [`StopRef`].
    pub fn top_frame(&self) -> Option<StopRef> {
        self.frames.first().map(|frame| self.stop_ref(frame.id))
    }

    /// The scope named `name` of the top frame, its reference as a
    /// [`StopRef`].
    pub fn scope(&self, name: &str) -> Option<StopRef> {
        self.scopes
            .iter()
            .find(|scope| scope.name == name)
            .map(|scope| self.stop_ref(scope.variables_reference))
    }
}

/// A resume the user asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resume {
    Continue,
    Next,
    StepIn,
    StepOut,
}

impl Resume {
    pub fn command(self) -> &'static str {
        match self {
            Resume::Continue => request::Continue::COMMAND,
            Resume::Next => request::Next::COMMAND,
            Resume::StepIn => request::StepIn::COMMAND,
            Resume::StepOut => request::StepOut::COMMAND,
        }
    }
}

/// What the handle asks the actor.
enum Command {
    Request(Outgoing),
    /// A request future was dropped before its reply.
    Abandon(u64),
    Stop,
    Restart,
    /// The user's resume, asked while the view was at `epoch`.
    Resume {
        resume: Resume,
        thread_id: Option<i64>,
        epoch: Epoch,
    },
    Pause {
        thread_id: Option<i64>,
    },
}

/// A host request on its way to the actor.
struct Outgoing {
    token: u64,
    command: String,
    arguments: Value,
    /// The stop its ids are of, if any: another epoch's answers `Stale`.
    epoch: Option<Epoch>,
    reply: oneshot::Sender<Result<Value, RequestError>>,
}

/// What the handle reads without asking the actor.
struct Shared {
    view: Mutex<View>,
    log: Mutex<ProtocolLog>,
    /// Set by [`Session::stop`]: no further adapter is launched.
    stopping: AtomicBool,
    /// The cancel for the launch in progress.
    launch_cancel: Mutex<LaunchCancel>,
}

/// What the handle reads: the actor's state, as it publishes it.
#[derive(Default)]
pub(crate) struct View {
    pub(crate) state: SessionState,
    pub(crate) capabilities: Capabilities,
    pub(crate) threads: Vec<Thread>,
    /// A `threads` reply has come in this run: an empty list is then the
    /// program's own answer.
    pub(crate) threads_answered: bool,
    pub(crate) epoch: Epoch,
    pub(crate) stop: Option<StopData>,
    /// [`Session::resume`] asked for a resume the actor hasn't taken yet:
    /// the controls are off from the call on, while the state it was asked
    /// in (Stopped or Uncertain) lasts.
    pub(crate) resume_requested: bool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A running debug session. Dropping it stops the session.
pub struct Session {
    commands: mpsc::UnboundedSender<Command>,
    shared: Arc<Shared>,
    tokens: AtomicU64,
}

impl Session {
    /// Starts a session on a thread of its own. `sink` gets its events, on
    /// that thread.
    pub fn start(
        options: SessionOptions,
        launcher: Arc<dyn AdapterLauncher>,
        clock: Arc<dyn Clock>,
        sink: impl FnMut(DapEvent) + Send + 'static,
    ) -> io::Result<Session> {
        let (commands, receiver) = mpsc::unbounded();
        let shared = Arc::new(Shared {
            view: Mutex::new(View::default()),
            log: Mutex::new(ProtocolLog::new(options.log, &options.expansions)),
            stopping: AtomicBool::new(false),
            launch_cancel: Mutex::new(LaunchCancel::new()),
        });
        let mut name = String::from(verbatim!("fe-dap "));
        name.push_str(&options.adapter.name);
        let actor = actor::Actor::new(
            options,
            launcher,
            clock,
            Box::new(sink),
            shared.clone(),
            receiver,
        );
        thread::Builder::new()
            .name(name)
            .spawn(move || async_io::block_on(actor.run()))?;
        Ok(Session {
            commands,
            shared,
            tokens: AtomicU64::new(1),
        })
    }

    pub fn state(&self) -> SessionState {
        lock(&self.shared.view).state.clone()
    }

    /// The adapter's capabilities, as far as they are known.
    pub fn capabilities(&self) -> Capabilities {
        lock(&self.shared.view).capabilities.clone()
    }

    /// The threads known: the baseline, changed by `thread` events.
    pub fn threads(&self) -> Vec<Thread> {
        lock(&self.shared.view).threads.clone()
    }

    /// The protocol log, oldest entry first.
    pub fn log(&self) -> Vec<LogEntry> {
        lock(&self.shared.log).entries()
    }

    /// The current stop epoch.
    pub fn epoch(&self) -> Epoch {
        lock(&self.shared.view).epoch
    }

    /// The current stop's data, as far as it has been fetched; `None` when
    /// the program isn't stopped.
    pub fn stop_data(&self) -> Option<StopData> {
        lock(&self.shared.view).stop.clone()
    }

    /// Which controls are enabled now, and why the others aren't.
    pub fn controls(&self) -> Controls {
        let view = lock(&self.shared.view);
        Controls::of(&view, self.shared.stopping.load(Ordering::SeqCst))
    }

    /// The user's Continue or step, on `thread_id` (by default the thread
    /// that last stopped, else the first known). Refused, `false`, unless
    /// [`controls`](Self::controls) has it enabled; from an accepted call
    /// on, Continue and stepping are disabled.
    pub fn resume(&self, resume: Resume, thread_id: Option<i64>) -> bool {
        self.resume_if(resume, thread_id, None)
    }

    /// [`resume`](Self::resume), but only from the stop at `epoch`: refused,
    /// `false`, if the session's stop epoch has moved on, as a new stop or
    /// a resume moves it. Checked under the lock the resume is sent under,
    /// so no stop can come between the check and the resume: for a host's
    /// resume that waited on a request since the user asked (run to
    /// cursor's breakpoint).
    pub fn resume_at(&self, resume: Resume, thread_id: Option<i64>, epoch: Epoch) -> bool {
        self.resume_if(resume, thread_id, Some(epoch))
    }

    fn resume_if(&self, resume: Resume, thread_id: Option<i64>, at: Option<Epoch>) -> bool {
        let mut view = lock(&self.shared.view);
        if at.is_some_and(|at| at != view.epoch) {
            return false;
        }
        let controls = Controls::of(&view, self.shared.stopping.load(Ordering::SeqCst));
        let control = match resume {
            Resume::Continue => controls.continue_,
            Resume::Next | Resume::StepIn | Resume::StepOut => controls.step,
        };
        if !control.is_enabled() {
            return false;
        }
        view.resume_requested = true;
        let epoch = view.epoch;
        // Sent under the lock, so commands go in the order they were
        // allowed.
        let _ = self.commands.unbounded_send(Command::Resume {
            resume,
            thread_id,
            epoch,
        });
        true
    }

    /// The user's Pause, on `thread_id` (by default the thread that last
    /// stopped, else the first known, else the first `threads` lists).
    /// Refused, `false`, unless [`controls`](Self::controls) has it
    /// enabled. It changes no state: the `stopped` event that follows does.
    pub fn pause(&self, thread_id: Option<i64>) -> bool {
        let view = lock(&self.shared.view);
        let controls = Controls::of(&view, self.shared.stopping.load(Ordering::SeqCst));
        if !controls.pause.is_enabled() {
            return false;
        }
        let _ = self.commands.unbounded_send(Command::Pause { thread_id });
        true
    }

    /// More of a stop's frames: [`FRAME_PAGE`] of `thread_id`'s from
    /// `start_frame`, if the stop at `epoch` is still current.
    pub fn frames(
        &self,
        epoch: Epoch,
        thread_id: i64,
        start_frame: i64,
    ) -> PendingRequest<StackTraceBody> {
        let arguments = StackTraceArguments {
            thread_id,
            start_frame: Some(start_frame),
            levels: Some(FRAME_PAGE),
            ..Default::default()
        };
        self.send_request(
            request::StackTrace::COMMAND,
            to_value(&arguments),
            Some(epoch),
        )
    }

    /// A frame's scopes, if its stop is still current.
    pub fn scopes(&self, frame: StopRef) -> PendingRequest<ScopesBody> {
        let arguments = ScopesArguments {
            frame_id: frame.id,
            ..Default::default()
        };
        self.send_request(
            request::Scopes::COMMAND,
            to_value(&arguments),
            Some(frame.epoch),
        )
    }

    /// A scope's or a variable's children, if its stop is still current.
    pub fn variables(&self, reference: StopRef) -> PendingRequest<VariablesBody> {
        let arguments = VariablesArguments {
            variables_reference: reference.id,
            ..Default::default()
        };
        self.send_request(
            request::Variables::COMMAND,
            to_value(&arguments),
            Some(reference.epoch),
        )
    }

    /// Evaluates in `frame` (its id replaces `arguments.frameId`), if its
    /// stop is still current; with no frame, in the adapter's global
    /// context, refused while Resuming or Uncertain.
    pub fn evaluate(
        &self,
        arguments: &EvaluateArguments,
        frame: Option<StopRef>,
    ) -> PendingRequest<EvaluateBody> {
        let mut arguments = arguments.clone();
        arguments.frame_id = frame.map(|frame| frame.id);
        self.send_request(
            request::Evaluate::COMMAND,
            to_value(&arguments),
            frame.map(|frame| frame.epoch),
        )
    }

    /// Sends request `R`. The future resolves with its body, or why there
    /// is none; dropping it first sends `cancel` if the adapter supports
    /// it. Requests made before `initialize` is answered wait for it.
    pub fn request<R: Request>(&self, arguments: &R::Arguments) -> PendingRequest<R::Body> {
        self.raw_request(R::COMMAND, to_value(arguments))
    }

    /// Sends a request by command name, with free-form arguments. A
    /// resume sent this way (`continue`, `next`, `stepIn`, `stepOut`,
    /// `goto`, `restartFrame`) is the session's pending resume, as
    /// [`resume`](Self::resume)'s is.
    pub fn raw_request<T>(&self, command: &str, arguments: Value) -> PendingRequest<T> {
        self.send_request(command, arguments, None)
    }

    fn send_request<T>(
        &self,
        command: &str,
        arguments: Value,
        epoch: Option<Epoch>,
    ) -> PendingRequest<T> {
        let token = self.tokens.fetch_add(1, Ordering::Relaxed);
        let (reply, receiver) = oneshot::channel();
        let view = lock(&self.shared.view);
        // Stop data is of the stop it is asked in, bound here, before the
        // actor takes it: a stop that ends meanwhile answers it `Stale`. A
        // frameless evaluation asked while not stopped is about no stop.
        let unbound =
            is_frameless_evaluate(command, &arguments) && view.state != SessionState::Stopped;
        let epoch = epoch.or_else(|| (is_stop_data(command) && !unbound).then_some(view.epoch));
        // Sent under the lock, so the epoch read is the one it goes after.
        let _ = self.commands.unbounded_send(Command::Request(Outgoing {
            token,
            command: command.to_owned(),
            arguments,
            epoch,
            reply,
        }));
        drop(view);
        PendingRequest {
            receiver,
            token,
            command: command.to_owned(),
            commands: self.commands.clone(),
            done: false,
            body: PhantomData,
        }
    }

    /// The user's Stop: ends the session (§2 item 6), and gives up a launch
    /// in progress.
    pub fn stop(&self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        lock(&self.shared.launch_cancel).cancel();
        let _ = self.commands.unbounded_send(Command::Stop);
    }

    /// Restarts the debuggee (§2 item 7), from Running or Stopped.
    pub fn restart(&self) {
        let _ = self.commands.unbounded_send(Command::Restart);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Whether `command` asks about the stopped program: stop data, sent only
/// while stopped, and of one stop.
pub(crate) fn is_stop_data(command: &str) -> bool {
    matches!(
        command,
        "stackTrace"
            | "scopes"
            | "variables"
            | "evaluate"
            | "setVariable"
            | "setExpression"
            | "completions"
            | "exceptionInfo"
    )
}

/// Whether the session sends `command` itself: startup, restart and
/// termination, never the host's to send.
pub(crate) fn is_session_owned(command: &str) -> bool {
    matches!(
        command,
        "initialize"
            | "launch"
            | "attach"
            | "configurationDone"
            | "restart"
            | "terminate"
            | "disconnect"
    )
}

/// An `evaluate` naming no frame: some adapters answer it while running.
pub(crate) fn is_frameless_evaluate(command: &str, arguments: &Value) -> bool {
    command == request::Evaluate::COMMAND && arguments.get("frameId").is_none_or(Value::is_null)
}

fn to_value<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// A request in flight: resolves to its body.
#[must_use = "dropping it cancels the request"]
pub struct PendingRequest<T> {
    receiver: oneshot::Receiver<Result<Value, RequestError>>,
    token: u64,
    command: String,
    commands: mpsc::UnboundedSender<Command>,
    done: bool,
    body: PhantomData<fn() -> T>,
}

impl<T> Unpin for PendingRequest<T> {}

impl<T: DeserializeOwned> Future for PendingRequest<T> {
    type Output = Result<T, RequestError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        let result = match Pin::new(&mut this.receiver).poll(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(result) => result,
        };
        this.done = true;
        let command = this.command.clone();
        Poll::Ready(match result {
            Ok(Ok(body)) => {
                let body = match body {
                    Value::Null => Value::Object(Object::new()),
                    body => body,
                };
                serde_json::from_value(body).map_err(|err| RequestError::BadBody {
                    command,
                    why: err.to_string(),
                })
            }
            Ok(Err(err)) => Err(err),
            // The actor is gone.
            Err(_) => Err(RequestError::Ended { command }),
        })
    }
}

impl<T> Drop for PendingRequest<T> {
    fn drop(&mut self) {
        if !self.done {
            let _ = self.commands.unbounded_send(Command::Abandon(self.token));
        }
    }
}
