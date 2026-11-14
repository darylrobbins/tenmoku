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
//! **Requests** ([`Session::request`]) time out on the injected clock:
//! [`REQUEST_TIMEOUT`] by default, [`LAUNCH_TIMEOUT`] for `launch` and
//! `attach`, none for `evaluate` in the `repl` context. A request future
//! dropped before its reply sends `cancel` when the adapter supports it,
//! and nothing waits for its reply after.
//!
//! **Reverse requests:** `runInTerminal` reaches the sink as
//! [`DapEvent::RunInTerminal`], whose [`TerminalReply`] answers it;
//! `startDebugging` is refused, since child sessions aren't supported.
//!
//! The stop/resume half of §2 (stop epochs, the resume guard, Uncertain,
//! pause) is card D2b, which adds its states to [`SessionState`]. Here a
//! `stopped` event moves to Stopped and `continued` to Running.

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
    BreakpointBody, Capabilities, ContinuedBody, ErrorMessage, EventMessage, Object, OutputBody,
    Request, SourceBreakpoint, StoppedBody, Thread,
};

mod actor;

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
}

/// Where a session is (§2's state machine, without D2b's resume states).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// Starting the adapter.
    Launching,
    /// `initialize` sent; no `initialized` yet.
    Initializing,
    /// `initialized` seen; configuration or the `launch` response still to
    /// come.
    Configuring,
    Running,
    Stopped,
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
    /// `initialize` went unanswered.
    InitializeTimeout,
    /// `launch` or `attach` failed, with the adapter's message.
    StartFailed(Option<String>),
    /// `launch` or `attach` went unanswered.
    StartTimeout,
    /// `configurationDone` went unanswered (§13 F10).
    ConfigurationDoneTimeout,
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
            SessionError::InitializeTimeout => tr!(
                "dap-session-initialize-timeout",
                seconds = REQUEST_TIMEOUT.as_secs()
            ),
            SessionError::StartFailed(message) => match message {
                Some(message) => tr!("dap-session-start-failed", why = message.as_str()),
                None => tr!("dap-session-start-failed-bare"),
            },
            SessionError::StartTimeout => tr!(
                "dap-session-start-timeout",
                seconds = LAUNCH_TIMEOUT.as_secs()
            ),
            SessionError::ConfigurationDoneTimeout => tr!(
                "dap-session-configuration-done-timeout",
                seconds = CONFIGURATION_DONE_TIMEOUT.as_secs()
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

/// What the handle asks the actor.
enum Command {
    Request(Outgoing),
    /// A request future was dropped before its reply.
    Abandon(u64),
    Stop,
    Restart,
}

/// A host request on its way to the actor.
struct Outgoing {
    token: u64,
    command: String,
    arguments: Value,
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

struct View {
    state: SessionState,
    capabilities: Capabilities,
    threads: Vec<Thread>,
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
            view: Mutex::new(View {
                state: SessionState::Launching,
                capabilities: Capabilities::default(),
                threads: Vec::new(),
            }),
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

    /// Sends request `R`. The future resolves with its body, or why there
    /// is none; dropping it first sends `cancel` if the adapter supports
    /// it. Requests made before `initialize` is answered wait for it.
    pub fn request<R: Request>(&self, arguments: &R::Arguments) -> PendingRequest<R::Body> {
        self.raw_request(R::COMMAND, to_value(arguments))
    }

    /// Sends a request by command name, with free-form arguments.
    pub fn raw_request<T>(&self, command: &str, arguments: Value) -> PendingRequest<T> {
        let token = self.tokens.fetch_add(1, Ordering::Relaxed);
        let (reply, receiver) = oneshot::channel();
        let _ = self.commands.unbounded_send(Command::Request(Outgoing {
            token,
            command: command.to_owned(),
            arguments,
            reply,
        }));
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
