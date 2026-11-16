// SPDX-License-Identifier: Apache-2.0

//! The session's actor: one per session, on its thread. See the parent
//! module for what it does; this is how.
//!
//! One adapter at a time is a *generation*: launched, driven until its
//! process has gone, then either the session ends or (a restart emulated)
//! a new generation starts on the same handle. Within a generation a
//! single loop waits on everything at once: the adapter's messages, the
//! handle's commands, the timers, terminal replies, the adapter's stderr
//! and stdout, the writer and the process's exit. Every handler runs to
//! completion between two waits, so nothing is shared but [`Shared`].

use std::collections::HashMap;
use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures::channel::{mpsc, oneshot};
use futures::stream::FuturesUnordered;
use futures::{AsyncRead, AsyncReadExt as _, AsyncWrite, FutureExt as _, StreamExt as _};
use kiln_l10n::{tr, verbatim};
use serde_json::{Value, json};

use super::{
    Command, DapEvent, EndReason, Ended, Epoch, Outgoing, RequestError, SessionError,
    SessionOptions, SessionState, Shared, StartRequest, TerminalReply, TerminalStarted, lock,
    to_value,
};
use crate::clock::Clock;
use crate::launcher::{AdapterIo, AdapterLauncher, ExitInfo, LaunchCancel};
use crate::protocol::{
    BreakpointsBody, CancelArguments, Capabilities, CapabilitiesBody, ContinuedBody,
    DisconnectArguments, Empty, EventMessage, ExitedBody, InitializeArguments, Message, Object,
    OutputBody, Request, RequestMessage, ResponseMessage, RestartArguments, RunInTerminalArguments,
    RunInTerminalBody, SetBreakpointsArguments, SetExceptionBreakpointsArguments, Source,
    StoppedBody, TerminateArguments, Thread, ThreadBody, ThreadsBody, event, request,
};
use crate::transport::{FrameReader, FrameWriter};

mod stops;

use stops::{Fetch, Probe, Stops};

/// How a request's timeout is chosen from `timeouts`: `launch`'s for
/// `launch` and `attach`, none for a `repl` evaluation (the user cancels
/// it), `request`'s for everything else, `configurationDone` included.
fn timeout_for(command: &str, arguments: &Value, timeouts: &super::Timeouts) -> Option<Duration> {
    match command {
        "launch" | "attach" => Some(timeouts.launch),
        "evaluate" if arguments.get("context").and_then(Value::as_str) == Some("repl") => None,
        _ => Some(timeouts.request),
    }
}

/// What a request in flight is for.
enum Purpose {
    /// The host's: answered through `reply`, or `Stale` if its ids are of
    /// `epoch` and the reply comes under another.
    External {
        token: u64,
        reply: oneshot::Sender<Result<Value, RequestError>>,
        epoch: Option<Epoch>,
    },
    /// A resume (§2 item 4): the user's, or (with a `reply`) the host's
    /// raw request. Its completion goes through the resume guard.
    Resume {
        token: Option<u64>,
        reply: Option<oneshot::Sender<Result<Value, RequestError>>>,
    },
    /// Stop data the session fetches for the stop at `epoch`.
    Fetch {
        epoch: Epoch,
        what: Fetch,
    },
    /// Uncertain's probe for a stop (§13 F2), sent at `epoch`.
    Probe {
        epoch: Epoch,
        what: Probe,
    },
    Pause,
    /// `threads` sent first for a Pause with no thread known, at `epoch`.
    PauseThreads {
        epoch: Epoch,
    },
    Initialize,
    /// `launch` or `attach`.
    Start,
    /// A `setBreakpoints` (for `path`) or `setExceptionBreakpoints` sent on
    /// the `round`th `initialized`.
    Configuration {
        round: u32,
        path: Option<String>,
    },
    ConfigurationDone {
        round: u32,
    },
    /// The thread baseline, after startup or a restart, sent at `epoch`.
    Baseline {
        epoch: Epoch,
    },
    Restart,
    Terminate,
    Disconnect,
    Cancel,
}

struct Pending {
    command: String,
    purpose: Purpose,
    sent_at: Instant,
    timeout: Option<Duration>,
}

/// Every timer, at most one of each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Timer {
    /// Request `seq`'s timeout.
    Request(i64),
    /// [`super::INITIALIZED_GRACE`] after a successful `launch` response.
    Initialized,
    /// [`super::TERMINATED_GRACE`] after the user's `terminate`.
    Terminated,
    /// [`super::EXIT_GRACE`] after `disconnect`'s answer, or after the
    /// adapter's output closed.
    Exit,
}

type Sleep = Pin<Box<dyn Future<Output = ()> + Send>>;

/// The sources the loop takes turns over, timers apart.
const SOURCES: usize = 7;

/// The armed timers. Disarming one drops its sleep, so a clock sees only
/// the timers that are live.
#[derive(Default)]
struct Timers(Vec<(Timer, Sleep)>);

impl Timers {
    fn arm(&mut self, timer: Timer, sleep: Sleep) {
        self.cancel(timer);
        self.0.push((timer, sleep));
    }

    fn cancel(&mut self, timer: Timer) {
        self.0.retain(|(armed, _)| *armed != timer);
    }

    fn is_armed(&self, timer: Timer) -> bool {
        self.0.iter().any(|(armed, _)| *armed == timer)
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Timer> {
        let fired = self
            .0
            .iter_mut()
            .position(|(_, sleep)| sleep.as_mut().poll(cx).is_ready());
        match fired {
            Some(at) => Poll::Ready(self.0.remove(at).0),
            None => Poll::Pending,
        }
    }
}

/// Where startup has got to, for one generation.
#[derive(Default)]
struct Startup {
    /// `initialize` answered: requests may go.
    initialize_answered: bool,
    /// The `launch` (or `attach`) response succeeded.
    started: bool,
    initialized_seen: bool,
    /// Configuration counts as complete.
    configured: bool,
    baseline_sent: bool,
    /// Which `initialized` the latest configuration is for, from 1.
    round: u32,
    /// That round's configuration requests not yet answered.
    outstanding: usize,
}

/// How far shutting down has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// `terminate` sent: waiting for `terminated`.
    Terminate,
    /// `disconnect` sent: waiting for its answer.
    Disconnect,
    /// Waiting for the adapter to exit.
    Exit,
    /// Killed, or being killed: waiting for the exit.
    Kill,
}

struct Shutdown {
    reason: EndReason,
    /// A restart being emulated: once the adapter is gone, start another.
    relaunch: bool,
    phase: Phase,
}

/// How a generation ended.
enum Outcome {
    Relaunch,
    End(EndReason),
}

type TerminalAnswer = (
    i64,
    String,
    Result<Result<TerminalStarted, String>, oneshot::Canceled>,
);

/// One adapter's worth of state.
#[derive(Default)]
struct Generation {
    /// The last `seq` the client used.
    seq: i64,
    /// Framed bodies to the writer.
    out: Option<mpsc::UnboundedSender<Vec<u8>>>,
    pending: HashMap<i64, Pending>,
    /// Host request tokens to their `seq`.
    tokens: HashMap<u64, i64>,
    timers: Timers,
    capabilities: Capabilities,
    startup: Startup,
    /// The pending restart's `seq`.
    restart: Option<i64>,
    /// The latest thread baseline's `seq`: only its reply counts.
    baseline: Option<i64>,
    shutdown: Option<Shutdown>,
    /// Kill the adapter before the next wait.
    kill: bool,
    /// `runInTerminal` replies to come.
    terminals: FuturesUnordered<Pin<Box<dyn Future<Output = TerminalAnswer> + Send>>>,
}

/// What woke the loop.
enum Input {
    Message(Result<Option<Message>, crate::TransportError>),
    Command(Option<Command>),
    Timer(Timer),
    Terminal(TerminalAnswer),
    Stderr(Option<Vec<u8>>),
    Stdout(Option<Vec<u8>>),
    /// The writer stopped: the stream failed.
    WriterDone(Result<(), String>),
    Exited(ExitInfo),
}

pub(super) struct Actor {
    options: SessionOptions,
    launcher: Arc<dyn AdapterLauncher>,
    clock: Arc<dyn Clock>,
    sink: Box<dyn FnMut(DapEvent) + Send>,
    shared: Arc<Shared>,
    commands: mpsc::UnboundedReceiver<Command>,
    commands_closed: bool,
    started_at: Instant,
    state: SessionState,
    /// From the latest `exited`.
    exit_code: Option<i64>,
    /// Host requests waiting for `initialize`'s answer.
    queued: Vec<Outgoing>,
    threads: Vec<Thread>,
    /// The stop epoch and what hangs on it (card D2b).
    s: Stops,
    g: Generation,
}

impl Actor {
    pub(super) fn new(
        options: SessionOptions,
        launcher: Arc<dyn AdapterLauncher>,
        clock: Arc<dyn Clock>,
        sink: Box<dyn FnMut(DapEvent) + Send>,
        shared: Arc<Shared>,
        commands: mpsc::UnboundedReceiver<Command>,
    ) -> Self {
        let started_at = clock.now();
        Self {
            options,
            launcher,
            clock,
            sink,
            shared,
            commands,
            commands_closed: false,
            started_at,
            state: SessionState::Launching,
            exit_code: None,
            queued: Vec::new(),
            threads: Vec::new(),
            s: Stops::default(),
            g: Generation::default(),
        }
    }

    pub(super) async fn run(mut self) {
        let reason = loop {
            match self.generation().await {
                Outcome::Relaunch => {
                    log::info!(
                        "debug adapter {}: restarting by a new launch",
                        self.options.adapter.name
                    );
                }
                Outcome::End(reason) => break reason,
            }
        };
        self.end_generation();
        for queued in std::mem::take(&mut self.queued) {
            let _ = queued.reply.send(Err(RequestError::Ended {
                command: queued.command,
            }));
        }
        let ended = Ended {
            exit_code: self.exit_code,
            reason,
        };
        // A resume asked for that the actor never took ends with it.
        lock(&self.shared.view).resume_requested = false;
        self.set_state(SessionState::Ended(ended));
    }

    // Plumbing.

    fn emit(&mut self, event: DapEvent) {
        (self.sink)(event);
    }

    fn set_state(&mut self, state: SessionState) {
        if self.state == state {
            return;
        }
        self.state = state.clone();
        lock(&self.shared.view).state = state.clone();
        self.emit(DapEvent::State(state));
    }

    fn set_threads(&mut self, threads: Vec<Thread>) {
        self.threads = threads.clone();
        lock(&self.shared.view).threads = threads.clone();
        self.emit(DapEvent::Threads(threads));
    }

    fn elapsed(&self) -> Duration {
        self.clock
            .now()
            .checked_duration_since(self.started_at)
            .unwrap_or_default()
    }

    fn note(&self, text: &str) {
        let at = self.elapsed();
        lock(&self.shared.log).note(at, text);
    }

    fn log_message(&self, message: &Message, received: bool, took: Option<Duration>) {
        let at = self.elapsed();
        lock(&self.shared.log).message(at, message, received, took);
    }

    fn write(&mut self, message: &Message) {
        self.log_message(message, false, None);
        let Some(out) = &self.g.out else {
            return;
        };
        match serde_json::to_vec(message) {
            Ok(body) => {
                let _ = out.unbounded_send(body);
            }
            Err(_) => log::warn!("couldn't serialise a debug adapter message"),
        }
    }

    /// Sends a request; its timeout is armed before it goes.
    fn send(&mut self, command: &str, arguments: Value, purpose: Purpose) -> i64 {
        self.g.seq += 1;
        let seq = self.g.seq;
        let timeout = timeout_for(command, &arguments, &self.options.timeouts);
        if let Some(timeout) = timeout {
            let sleep = self.clock.sleep(timeout);
            self.g.timers.arm(Timer::Request(seq), sleep);
        }
        if let Purpose::External { token, .. }
        | Purpose::Resume {
            token: Some(token), ..
        } = &purpose
        {
            self.g.tokens.insert(*token, seq);
        }
        self.g.pending.insert(
            seq,
            Pending {
                command: command.to_owned(),
                purpose,
                sent_at: self.clock.now(),
                timeout,
            },
        );
        let message = Message::Request(RequestMessage {
            seq,
            command: command.to_owned(),
            arguments: Some(arguments),
            extra: Object::new(),
        });
        self.write(&message);
        seq
    }

    /// Answers the adapter's request `request`.
    fn respond(&mut self, request: &RequestMessage, result: Result<Value, String>) {
        self.g.seq += 1;
        let response = match result {
            Ok(body) => ResponseMessage {
                seq: self.g.seq,
                request_seq: request.seq,
                success: true,
                command: request.command.clone(),
                message: None,
                body: Some(body),
                extra: Object::new(),
            },
            Err(message) => request.fail(self.g.seq, message, None),
        };
        self.write(&Message::Response(response));
    }

    // Generations.

    /// Launches an adapter and drives it until it has gone.
    async fn generation(&mut self) -> Outcome {
        self.g = Generation::default();
        // A new adapter: nothing of the old one's stops is valid.
        self.s.new_adapter();
        self.advance_epoch();
        lock(&self.shared.view).resume_requested = false;
        self.exit_code = None;
        lock(&self.shared.view).capabilities = Capabilities::default();
        self.set_state(SessionState::Launching);
        let cancel = LaunchCancel::new();
        *lock(&self.shared.launch_cancel) = cancel.clone();
        if self.shared.stopping.load(Ordering::SeqCst) {
            return Outcome::End(EndReason::Stopped);
        }
        let launched = self
            .launcher
            .launch(&self.options.adapter, &self.options.cwd, &cancel);
        let io = match launched {
            Ok(io) => io,
            Err(_) if self.shared.stopping.load(Ordering::SeqCst) => {
                return Outcome::End(EndReason::Stopped);
            }
            Err(err) => {
                return Outcome::End(EndReason::Failed(SessionError::Launch(err.to_string())));
            }
        };
        let AdapterIo {
            input,
            output,
            stderr,
            stdout,
            mut process,
            ..
        } = io;
        let (out, bodies) = mpsc::unbounded();
        self.g.out = Some(out);
        let mut writer = pin!(write_all(FrameWriter::new(input), bodies).fuse());
        let mut writer_done = false;
        let mut turn = 0;
        let mut reader = Some(FrameReader::new(output));
        let mut stderr = stderr.map(Drain::new);
        let mut stdout = stdout.map(Drain::new);

        self.set_state(SessionState::Initializing);
        self.send_initialize();

        loop {
            if std::mem::take(&mut self.g.kill) {
                process.kill();
            }
            // Timers first (each fires once, so they can't starve the
            // rest, and a timeout that has come due is handled before any
            // command sent after it); then every other source in turn,
            // starting one further on each time, so none starves another.
            turn = (turn + 1) % SOURCES;
            let input = {
                let mut read = pin!(async {
                    match reader.as_mut() {
                        Some(reader) => reader.read().await,
                        None => futures::future::pending().await,
                    }
                });
                let mut err_chunk = pin!(chunk(stderr.as_mut()));
                let mut out_chunk = pin!(chunk(stdout.as_mut()));
                let mut exited = process.exited();
                let commands = &mut self.commands;
                let commands_closed = self.commands_closed;
                let timers = &mut self.g.timers;
                let terminals = &mut self.g.terminals;
                poll_fn(|cx| {
                    if let Poll::Ready(timer) = timers.poll(cx) {
                        return Poll::Ready(Input::Timer(timer));
                    }
                    for source in (0..SOURCES).map(|at| (turn + at) % SOURCES) {
                        let ready = match source {
                            0 => read.as_mut().poll(cx).map(Input::Message),
                            1 if !commands_closed => {
                                commands.poll_next_unpin(cx).map(Input::Command)
                            }
                            2 => match terminals.poll_next_unpin(cx) {
                                Poll::Ready(Some(answer)) => Poll::Ready(Input::Terminal(answer)),
                                _ => Poll::Pending,
                            },
                            3 => err_chunk.as_mut().poll(cx).map(Input::Stderr),
                            4 => out_chunk.as_mut().poll(cx).map(Input::Stdout),
                            5 if !writer_done => writer.as_mut().poll(cx).map(Input::WriterDone),
                            6 => exited.as_mut().poll(cx).map(Input::Exited),
                            _ => Poll::Pending,
                        };
                        if ready.is_ready() {
                            return ready;
                        }
                    }
                    Poll::Pending
                })
                .await
            };
            match input {
                Input::Message(Ok(Some(message))) => self.receive(message),
                Input::Message(Ok(None)) => {
                    reader = None;
                    self.output_closed();
                }
                Input::Message(Err(err)) => {
                    reader = None;
                    self.abort(SessionError::Transport(transport_error(&err)));
                }
                Input::Command(Some(command)) => self.command(command),
                Input::Command(None) => {
                    self.commands_closed = true;
                    self.stop();
                }
                Input::Timer(timer) => self.timer(timer),
                Input::Terminal((seq, command, answer)) => {
                    self.terminal_answered(seq, command, answer)
                }
                Input::Stderr(Some(bytes)) => {
                    let at = self.elapsed();
                    lock(&self.shared.log).stderr(at, &bytes);
                }
                Input::Stderr(None) => {
                    stderr = None;
                    let at = self.elapsed();
                    lock(&self.shared.log).stderr_end(at);
                }
                // Over TCP the adapter's stdout isn't the transport: read
                // so it never blocks on a full pipe, and drop it.
                Input::Stdout(Some(_)) => {}
                Input::Stdout(None) => stdout = None,
                Input::WriterDone(result) => {
                    writer_done = true;
                    self.writer_failed(result);
                }
                Input::Exited(exit) => {
                    self.drain_after_exit(&mut reader, &mut stderr).await;
                    let outcome = self.outcome(exit);
                    self.end_generation();
                    return outcome;
                }
            }
        }
    }

    /// The adapter has exited: what it sent before it went (its `exited`
    /// and `terminated`, a last line of stderr) is still read, to the end of
    /// each stream or until [`super::EXIT_DRAIN`] has passed for streams
    /// that stay open.
    async fn drain_after_exit(
        &mut self,
        reader: &mut Option<FrameReader<Pin<Box<dyn AsyncRead + Send>>>>,
        stderr: &mut Option<Drain>,
    ) {
        let mut deadline = self.clock.sleep(super::EXIT_DRAIN);
        let mut turn = false;
        while reader.is_some() || stderr.is_some() {
            turn = !turn;
            let input = {
                let mut read = pin!(async {
                    match reader.as_mut() {
                        Some(reader) => reader.read().await,
                        None => futures::future::pending().await,
                    }
                });
                let mut err_chunk = pin!(chunk(stderr.as_mut()));
                poll_fn(|cx| {
                    if deadline.as_mut().poll(cx).is_ready() {
                        return Poll::Ready(None);
                    }
                    // Alternate which goes first, so neither stream holds
                    // up the other; poll the second only if the first has
                    // nothing, so no read that completed is dropped.
                    for stream in [turn, !turn] {
                        let ready = if stream {
                            read.as_mut().poll(cx).map(|read| Some(Ok(read)))
                        } else {
                            err_chunk.as_mut().poll(cx).map(|bytes| Some(Err(bytes)))
                        };
                        if ready.is_ready() {
                            return ready;
                        }
                    }
                    Poll::Pending
                })
                .await
            };
            match input {
                None => break,
                Some(Ok(Ok(Some(message)))) => self.receive(message),
                Some(Ok(_)) => *reader = None,
                Some(Err(Some(bytes))) => {
                    let at = self.elapsed();
                    lock(&self.shared.log).stderr(at, &bytes);
                }
                Some(Err(None)) => *stderr = None,
            }
        }
        let at = self.elapsed();
        lock(&self.shared.log).stderr_end(at);
    }

    /// The adapter has gone: what happens next.
    fn outcome(&mut self, exit: ExitInfo) -> Outcome {
        let stopping = self.shared.stopping.load(Ordering::SeqCst);
        match self.g.shutdown.take() {
            Some(shutdown) if shutdown.relaunch && !stopping => Outcome::Relaunch,
            Some(shutdown) => Outcome::End(shutdown.reason),
            None => {
                log::warn!(
                    "debug adapter {} exited by itself: {exit:?}",
                    self.options.adapter.name
                );
                Outcome::End(EndReason::AdapterExited(exit))
            }
        }
    }

    /// Fails what this generation still owes the host and forgets the rest.
    fn end_generation(&mut self) {
        let pending = std::mem::take(&mut self.g.pending);
        for (_, pending) in pending {
            fail_ended(pending);
        }
        self.g.out = None;
        self.g.timers = Timers::default();
        self.g.terminals = FuturesUnordered::new();
        if !self.threads.is_empty() {
            self.set_threads(Vec::new());
        }
        lock(&self.shared.view).threads_answered = false;
        // However the adapter went, its stop is over.
        self.s.new_run();
        self.advance_epoch();
    }

    // Startup.

    fn send_initialize(&mut self) {
        let adapter = &self.options.adapter;
        let arguments = InitializeArguments {
            adapter_id: adapter
                .types
                .first()
                .cloned()
                .unwrap_or_else(|| adapter.name.clone()),
            client_id: Some(verbatim!("tenmoku").to_owned()),
            client_name: Some(verbatim!("Tenmoku").to_owned()),
            lines_start_at1: Some(true),
            columns_start_at1: Some(true),
            path_format: Some(verbatim!("path").to_owned()),
            supports_variable_type: Some(true),
            supports_variable_paging: Some(true),
            supports_run_in_terminal_request: Some(true),
            supports_progress_reporting: Some(true),
            supports_invalidated_event: Some(true),
            supports_start_debugging_request: Some(false),
            ..Default::default()
        };
        self.send(
            request::Initialize::COMMAND,
            to_value(&arguments),
            Purpose::Initialize,
        );
    }

    fn initialize_answered(&mut self, result: Result<Value, RequestError>) {
        match result {
            Ok(body) => {
                let capabilities: Capabilities = parse(body).unwrap_or_default();
                self.g.capabilities = capabilities.clone();
                lock(&self.shared.view).capabilities = capabilities.clone();
                self.emit(DapEvent::Capabilities(capabilities));
                self.g.startup.initialize_answered = true;
                // `launch` goes at once and isn't awaited (§2 item 2): an
                // adapter may answer it only after `configurationDone`.
                let command = match self.options.request {
                    StartRequest::Launch => request::Launch::COMMAND,
                    StartRequest::Attach => request::Attach::COMMAND,
                };
                let arguments = Value::Object(self.options.arguments.clone());
                self.send(command, arguments, Purpose::Start);
                for queued in std::mem::take(&mut self.queued) {
                    self.send_external(queued);
                }
            }
            Err(RequestError::Timeout { after, .. }) => {
                self.abort(SessionError::InitializeTimeout(after))
            }
            Err(err) => self.shut_down(
                EndReason::Failed(SessionError::InitializeFailed(
                    err.message().map(str::to_owned),
                )),
                false,
                false,
            ),
        }
    }

    fn start_answered(&mut self, result: Result<Value, RequestError>) {
        match result {
            Ok(_) => {
                self.g.startup.started = true;
                if !self.g.startup.initialized_seen && !self.g.startup.configured {
                    let sleep = self.clock.sleep(super::INITIALIZED_GRACE);
                    self.g.timers.arm(Timer::Initialized, sleep);
                }
                self.maybe_baseline();
            }
            Err(RequestError::Timeout { after, .. }) => {
                self.abort(SessionError::StartTimeout(after))
            }
            Err(err) => self.shut_down(
                EndReason::Failed(SessionError::StartFailed(err.message().map(str::to_owned))),
                false,
                false,
            ),
        }
    }

    /// `initialized`: configure (§2 item 3), on each one.
    fn initialized(&mut self) {
        self.g.startup.initialized_seen = true;
        self.g.timers.cancel(Timer::Initialized);
        if self.state == SessionState::Initializing {
            self.set_state(SessionState::Configuring);
        }
        // A new round: configuration is complete again only when this
        // round is.
        self.g.startup.round += 1;
        let round = self.g.startup.round;
        self.g.startup.outstanding = 0;
        self.g.startup.configured = false;
        for (path, breakpoints) in self.options.breakpoints.clone() {
            let arguments = SetBreakpointsArguments {
                source: Source {
                    path: Some(path.clone()),
                    ..Default::default()
                },
                breakpoints: Some(breakpoints),
                ..Default::default()
            };
            self.send(
                request::SetBreakpoints::COMMAND,
                to_value(&arguments),
                Purpose::Configuration {
                    round,
                    path: Some(path),
                },
            );
            self.g.startup.outstanding += 1;
        }
        // The specification's condition (the `initialized` event): only if
        // the adapter offers filters, or lacks `configurationDone`.
        let offered = self
            .g
            .capabilities
            .exception_breakpoint_filters
            .clone()
            .unwrap_or_default();
        if !offered.is_empty() || !self.supports_configuration_done() {
            let chosen = lock(&self.shared.exception_filters).clone();
            let filters = chosen
                .or_else(|| self.options.exception_filters.clone())
                .unwrap_or_else(|| {
                    offered
                        .iter()
                        .filter(|filter| filter.default == Some(true))
                        .map(|filter| filter.filter.clone())
                        .collect()
                });
            let arguments = SetExceptionBreakpointsArguments {
                filters,
                ..Default::default()
            };
            self.send(
                request::SetExceptionBreakpoints::COMMAND,
                to_value(&arguments),
                Purpose::Configuration { round, path: None },
            );
            self.g.startup.outstanding += 1;
        }
        if self.g.startup.outstanding == 0 {
            self.configuration_requests_done(round);
        }
    }

    fn supports_configuration_done(&self) -> bool {
        self.g.capabilities.supports_configuration_done_request == Some(true)
    }

    fn configuration_answered(
        &mut self,
        round: u32,
        path: Option<String>,
        result: Result<Value, RequestError>,
    ) {
        // An earlier round's reply is out of date: a later `initialized`
        // configured again since.
        if round != self.g.startup.round {
            log::debug!("ignored a configuration reply from an earlier round");
            return;
        }
        match result {
            Ok(body) => {
                if let Some(path) = path {
                    let breakpoints = parse::<BreakpointsBody>(body)
                        .map(|body| body.breakpoints)
                        .unwrap_or_default();
                    self.emit(DapEvent::Breakpoints { path, breakpoints });
                }
            }
            Err(err) => self.emit(DapEvent::Failed(err)),
        }
        self.g.startup.outstanding = self.g.startup.outstanding.saturating_sub(1);
        if self.g.startup.outstanding == 0 {
            self.configuration_requests_done(round);
        }
    }

    /// The round's configuration requests are all answered (or failed, or
    /// timed out): `configurationDone` if supported, else configuration is
    /// complete.
    fn configuration_requests_done(&mut self, round: u32) {
        if self.supports_configuration_done() {
            self.send(
                request::ConfigurationDone::COMMAND,
                to_value(&Empty::default()),
                Purpose::ConfigurationDone { round },
            );
        } else {
            self.configuration_complete();
        }
    }

    fn configuration_done_answered(&mut self, round: u32, result: Result<Value, RequestError>) {
        match result {
            Ok(_) => {}
            // §13 F10: an adapter that never answers ends the session.
            Err(RequestError::Timeout { after, .. }) => {
                return self.abort(SessionError::ConfigurationDoneTimeout(after));
            }
            // Shown, if still current; the session carries on.
            Err(err) if round == self.g.startup.round => self.emit(DapEvent::Failed(err)),
            Err(_) => {}
        }
        if round == self.g.startup.round {
            self.configuration_complete();
        }
    }

    fn configuration_complete(&mut self) {
        self.g.startup.configured = true;
        self.g.timers.cancel(Timer::Initialized);
        self.maybe_baseline();
    }

    /// Once both the start response and configuration completion are in,
    /// in either order: `threads`, once, whatever the state.
    fn maybe_baseline(&mut self) {
        let startup = &self.g.startup;
        if !startup.started || !startup.configured || startup.baseline_sent {
            return;
        }
        self.g.startup.baseline_sent = true;
        self.send_baseline();
        if matches!(
            self.state,
            SessionState::Initializing | SessionState::Configuring
        ) {
            self.set_state(SessionState::Running);
        }
    }

    /// Sends `threads` as the baseline. An earlier baseline still in flight
    /// is given up, so its reply can't replace this one's.
    fn send_baseline(&mut self) {
        if let Some(old) = self.g.baseline.take()
            && self.g.pending.remove(&old).is_some()
        {
            self.g.timers.cancel(Timer::Request(old));
        }
        let epoch = self.s.epoch;
        let seq = self.send(
            request::Threads::COMMAND,
            to_value(&Empty::default()),
            Purpose::Baseline { epoch },
        );
        self.g.baseline = Some(seq);
    }

    fn initialized_grace_over(&mut self) {
        let startup = &self.g.startup;
        if startup.initialized_seen || startup.configured || !startup.started {
            return;
        }
        log::warn!(
            "debug adapter {} sent no initialized event within {} s of its launch response",
            self.options.adapter.name,
            super::INITIALIZED_GRACE.as_secs()
        );
        self.note(&tr!(
            "dap-log-no-initialized",
            seconds = super::INITIALIZED_GRACE.as_secs()
        ));
        self.configuration_complete();
    }

    // Messages from the adapter.

    fn receive(&mut self, message: Message) {
        match message {
            Message::Response(response) => self.response(response),
            Message::Event(event) => {
                self.log_message(&Message::Event(event.clone()), true, None);
                self.event(event);
            }
            Message::Request(request) => {
                self.log_message(&Message::Request(request.clone()), true, None);
                self.reverse_request(request);
            }
        }
    }

    fn response(&mut self, response: ResponseMessage) {
        let seq = response.request_seq;
        // A resume that timed out stays the pending resume: its late reply
        // still goes through the guard.
        let pending = self
            .g
            .pending
            .remove(&seq)
            .or_else(|| self.late_resume(seq, &response.command));
        let took = pending.as_ref().map(|pending| {
            self.clock
                .now()
                .checked_duration_since(pending.sent_at)
                .unwrap_or_default()
        });
        self.log_message(&Message::Response(response.clone()), true, took);
        let Some(pending) = pending else {
            log::debug!("dropped a late or unknown response to request {seq}");
            return;
        };
        self.g.timers.cancel(Timer::Request(seq));
        let result = if response.success {
            Ok(response.body.clone().unwrap_or(Value::Null))
        } else {
            Err(RequestError::Failed {
                command: pending.command.clone(),
                message: response.message.clone(),
                error: response.error(),
            })
        };
        self.complete(seq, pending, result);
    }

    /// A request's reply, failure or timeout, by what it was for.
    fn complete(&mut self, seq: i64, pending: Pending, result: Result<Value, RequestError>) {
        match pending.purpose {
            Purpose::External {
                token,
                reply,
                epoch,
            } => {
                self.g.tokens.remove(&token);
                // A reply about a stop that has ended since is dropped (a
                // new epoch answers such requests at once, so this is the
                // guard's second line).
                let result = match epoch {
                    Some(epoch) if epoch != self.s.epoch => Err(RequestError::Stale {
                        command: pending.command,
                    }),
                    _ => result,
                };
                let _ = reply.send(result);
            }
            Purpose::Resume { token, reply } => {
                if let Some(token) = token {
                    self.g.tokens.remove(&token);
                }
                if let Some(reply) = reply {
                    let _ = reply.send(result.clone());
                }
                self.resume_answered(seq, result);
            }
            Purpose::Fetch { epoch, what } => self.fetch_answered(seq, epoch, what, result),
            Purpose::Probe { epoch, what } => self.probe_answered(seq, epoch, what, result),
            Purpose::Pause => {
                // Shown; the state stays as it was.
                if let Err(err) = result {
                    self.emit(DapEvent::Failed(err));
                }
            }
            Purpose::PauseThreads { epoch } => self.pause_threads_answered(seq, epoch, result),
            Purpose::Initialize => self.initialize_answered(result),
            Purpose::Start => self.start_answered(result),
            Purpose::Configuration { round, path } => {
                self.configuration_answered(round, path, result)
            }
            Purpose::ConfigurationDone { round } => self.configuration_done_answered(round, result),
            Purpose::Baseline { epoch } => match result {
                Ok(body) => {
                    let threads = parse::<ThreadsBody>(body)
                        .map(|body| body.threads)
                        .unwrap_or_default();
                    self.threads_answered(seq, epoch, threads);
                }
                // The payload log has the reply itself, redacted.
                Err(_) => log::warn!("the thread baseline failed"),
            },
            Purpose::Restart => self.restart_answered(seq, result),
            Purpose::Terminate => {
                // A terminate that failed or went unanswered gets no
                // `terminated`: disconnect now.
                if result.is_err() && self.phase() == Some(Phase::Terminate) {
                    self.disconnect();
                }
            }
            Purpose::Disconnect => {
                if self.phase() == Some(Phase::Disconnect) {
                    self.expect_exit();
                }
            }
            Purpose::Cancel => {}
        }
    }

    fn event(&mut self, message: EventMessage) {
        if self.g.shutdown.is_some() && message.event != "terminated" && message.event != "exited" {
            // Terminating ignores the rest; output still reaches the host.
            if message.is::<event::Output>()
                && let Ok(body) = message.body::<event::Output>()
            {
                self.output(body);
            }
            return;
        }
        match message.event.as_str() {
            "initialized" => self.initialized(),
            "capabilities" => {
                if let Ok(body) = message.body::<event::CapabilitiesEvent>() {
                    let CapabilitiesBody { capabilities, .. } = body;
                    self.g.capabilities.merge(&capabilities);
                    let merged = self.g.capabilities.clone();
                    lock(&self.shared.view).capabilities = merged.clone();
                    self.emit(DapEvent::Capabilities(merged));
                }
            }
            "stopped" => {
                let body: StoppedBody = message.body::<event::Stopped>().unwrap_or_default();
                self.stopped(body);
            }
            "continued" => {
                let body: ContinuedBody = message.body::<event::Continued>().unwrap_or_default();
                self.continued(body);
            }
            "exited" => {
                if let Ok(ExitedBody { exit_code, .. }) = message.body::<event::Exited>() {
                    self.exit_code = Some(exit_code);
                    self.emit(DapEvent::Exited(exit_code));
                }
            }
            "terminated" => self.terminated(),
            "thread" => {
                if let Ok(ThreadBody {
                    reason, thread_id, ..
                }) = message.body::<event::ThreadEvent>()
                {
                    let mut threads = self.threads.clone();
                    match reason.as_str() {
                        "started" => {
                            if !threads.iter().any(|thread| thread.id == thread_id) {
                                threads.push(Thread {
                                    id: thread_id,
                                    ..Default::default()
                                });
                            }
                        }
                        "exited" => {
                            threads.retain(|thread| thread.id != thread_id);
                            self.s.thread_exited(thread_id);
                        }
                        _ => return,
                    }
                    // Newer than any `threads` reply still to come.
                    self.s.threads_changed(self.g.seq);
                    self.set_threads(threads);
                    if reason == "exited" {
                        self.thread_gone(thread_id);
                    }
                }
            }
            "output" => {
                if let Ok(body) = message.body::<event::Output>() {
                    self.output(body);
                }
            }
            "breakpoint" => {
                if let Ok(body) = message.body::<event::BreakpointEvent>() {
                    self.emit(DapEvent::Breakpoint(body));
                }
            }
            // Logged and ignored (§2).
            "module" | "loadedSource" | "memory" => {}
            _ => self.emit(DapEvent::Other(message)),
        }
    }

    fn output(&mut self, body: OutputBody) {
        if body.category.as_deref() != Some("telemetry") {
            self.emit(DapEvent::Output(body));
        }
    }

    fn reverse_request(&mut self, request: RequestMessage) {
        match request.command.as_str() {
            "runInTerminal" => {
                if self.g.shutdown.is_some() {
                    return self
                        .respond(&request, Err(tr!("dap-run-in-terminal-ending").to_string()));
                }
                let arguments: RunInTerminalArguments =
                    match request.arguments::<crate::protocol::reverse::RunInTerminal>() {
                        Ok(arguments) => arguments,
                        Err(err) => return self.respond(&request, Err(err.to_string())),
                    };
                let (reply, answer) = oneshot::channel();
                let seq = request.seq;
                let command = request.command.clone();
                self.g
                    .terminals
                    .push(Box::pin(async move { (seq, command, answer.await) }));
                self.emit(DapEvent::RunInTerminal {
                    args: arguments.args,
                    cwd: arguments.cwd,
                    env: arguments.env.unwrap_or_default(),
                    kind: arguments.kind,
                    title: arguments.title,
                    reply: TerminalReply(reply),
                });
            }
            // Child sessions (js-debug's) aren't supported in v1 (§2).
            "startDebugging" => {
                self.respond(
                    &request,
                    Err(tr!("dap-start-debugging-refused").to_string()),
                );
            }
            command => {
                let message = tr!("dap-reverse-unsupported", command = command).to_string();
                self.respond(&request, Err(message));
            }
        }
    }

    fn terminal_answered(
        &mut self,
        seq: i64,
        command: String,
        answer: Result<Result<TerminalStarted, String>, oneshot::Canceled>,
    ) {
        let request = RequestMessage {
            seq,
            command,
            ..Default::default()
        };
        let result = match answer {
            Ok(Ok(started)) => Ok(to_value(&RunInTerminalBody {
                process_id: started.process_id,
                shell_process_id: started.shell_process_id,
                ..Default::default()
            })),
            Ok(Err(message)) => Err(message),
            Err(oneshot::Canceled) => Err(tr!("dap-run-in-terminal-dropped").to_string()),
        };
        self.respond(&request, result);
    }

    // The host's commands.

    fn command(&mut self, command: Command) {
        match command {
            Command::Request(outgoing) => {
                if self.g.shutdown.is_some() {
                    let _ = outgoing.reply.send(Err(RequestError::Ended {
                        command: outgoing.command,
                    }));
                } else if !self.g.startup.initialize_answered {
                    self.queued.push(outgoing);
                } else {
                    self.send_external(outgoing);
                }
            }
            Command::Abandon(token) => self.abandon(token),
            Command::Stop => self.stop(),
            Command::Restart => self.restart(),
            Command::Resume {
                resume,
                thread_id,
                epoch,
            } => self.resume_command(resume, thread_id, epoch),
            Command::Pause { thread_id } => self.pause_command(thread_id),
        }
    }

    fn send_external(&mut self, outgoing: Outgoing) {
        let Outgoing {
            token,
            command,
            arguments,
            epoch,
            reply,
        } = outgoing;
        // Ids of a stop that's over: answered without a round trip; and
        // one asked of no stop, while the program may run, isn't sent.
        if let Some(epoch) = epoch {
            if epoch != self.s.epoch {
                let _ = reply.send(Err(RequestError::Stale { command }));
                return;
            }
            if self.state != SessionState::Stopped {
                let _ = reply.send(Err(RequestError::NotStopped { command }));
                return;
            }
        }
        if super::is_session_owned(&command) {
            let _ = reply.send(Err(RequestError::SessionOwned { command }));
            return;
        }
        if stops::is_resume(&command) {
            return self.raw_resume(token, command, arguments, reply);
        }
        // Stop data is asked for only of a stop, and is of it (§13 F2).
        let epoch = match self.stop_data_epoch(&command, &arguments, epoch) {
            Ok(epoch) => epoch,
            Err(()) => {
                let _ = reply.send(Err(RequestError::NotStopped { command }));
                return;
            }
        };
        self.send(
            &command,
            arguments,
            Purpose::External {
                token,
                reply,
                epoch,
            },
        );
    }

    /// A host request's future was dropped: forget it, and `cancel` it if
    /// the adapter supports that.
    fn abandon(&mut self, token: u64) {
        if let Some(at) = self.queued.iter().position(|queued| queued.token == token) {
            self.queued.remove(at);
            return;
        }
        let Some(seq) = self.g.tokens.remove(&token) else {
            return;
        };
        // A resume stays the pending resume, its late reply still going
        // through the guard: only its reply is given up, and the adapter
        // asked to drop it.
        if let Some(Pending {
            purpose: Purpose::Resume { token, reply },
            ..
        }) = self.g.pending.get_mut(&seq)
        {
            *token = None;
            *reply = None;
            return self.cancel_request(seq);
        }
        if self.g.pending.remove(&seq).is_none() {
            return;
        }
        self.g.timers.cancel(Timer::Request(seq));
        self.cancel_request(seq);
    }

    fn cancel_request(&mut self, seq: i64) {
        if self.g.capabilities.supports_cancel_request == Some(true) && self.g.shutdown.is_none() {
            let arguments = CancelArguments {
                request_id: Some(seq),
                ..Default::default()
            };
            self.send(
                request::Cancel::COMMAND,
                to_value(&arguments),
                Purpose::Cancel,
            );
        }
    }

    fn timer(&mut self, timer: Timer) {
        match timer {
            Timer::Request(seq) => {
                let Some(pending) = self.g.pending.remove(&seq) else {
                    return;
                };
                let command = pending.command.clone();
                log::warn!("debug adapter request {command} ({seq}) timed out");
                let after = pending.timeout.unwrap_or_default();
                if matches!(pending.purpose, Purpose::External { .. }) {
                    self.cancel_request(seq);
                }
                self.complete(seq, pending, Err(RequestError::Timeout { command, after }));
            }
            Timer::Initialized => self.initialized_grace_over(),
            Timer::Terminated => {
                if self.phase() == Some(Phase::Terminate) {
                    self.disconnect();
                }
            }
            Timer::Exit => {
                if let Some(shutdown) = &mut self.g.shutdown {
                    shutdown.phase = Phase::Kill;
                }
                log::warn!(
                    "debug adapter {} didn't exit {} s after disconnecting: killing it",
                    self.options.adapter.name,
                    super::EXIT_GRACE.as_secs()
                );
                self.g.kill = true;
            }
        }
    }

    // Restart (§2 item 7).

    fn restart(&mut self) {
        if self.g.shutdown.is_some()
            || !matches!(self.state, SessionState::Running | SessionState::Stopped)
        {
            log::info!("restart ignored: the session isn't running or stopped");
            return;
        }
        if self.g.capabilities.supports_restart_request == Some(true) {
            let arguments = RestartArguments {
                arguments: Some(self.options.arguments.clone()),
                ..Default::default()
            };
            // The stop's ids are retired, and a pending resume given up.
            self.s.new_run();
            self.advance_epoch();
            let seq = self.send(
                request::Restart::COMMAND,
                to_value(&arguments),
                Purpose::Restart,
            );
            self.g.restart = Some(seq);
            // The old run is over from here, whatever comes of the restart:
            // an exit code from now on is the new run's.
            self.exit_code = None;
            self.set_state(SessionState::Restarting);
        } else {
            self.emulate_restart();
        }
    }

    /// The restart guard: only the pending restart, while still Restarting.
    fn restart_answered(&mut self, seq: i64, result: Result<Value, RequestError>) {
        if self.g.restart != Some(seq) || self.state != SessionState::Restarting {
            log::debug!("ignored a stale restart completion ({seq})");
            return;
        }
        self.g.restart = None;
        match result {
            Ok(_) => {
                // A new run: the old one's threads go.
                self.set_threads(Vec::new());
                lock(&self.shared.view).threads_answered = false;
                self.s.new_run();
                self.s.threads_changed(self.g.seq);
                self.send_baseline();
                self.set_state(SessionState::Running);
            }
            Err(err) => {
                self.emit(DapEvent::Failed(err));
                self.emulate_restart();
            }
        }
    }

    /// Stop the adapter, then start a new one from the same options.
    fn emulate_restart(&mut self) {
        self.shut_down(EndReason::Stopped, true, true);
    }

    // Termination (§2 item 6).

    fn phase(&self) -> Option<Phase> {
        self.g.shutdown.as_ref().map(|shutdown| shutdown.phase)
    }

    /// The user's Stop. While a restart is being emulated, it stops the
    /// relaunch instead.
    fn stop(&mut self) {
        if let Some(shutdown) = &mut self.g.shutdown {
            if shutdown.relaunch {
                shutdown.relaunch = false;
                shutdown.reason = EndReason::Stopped;
            }
            return;
        }
        self.shut_down(EndReason::Stopped, true, false);
    }

    /// Starts shutting down: `terminate` first for the user's Stop (when
    /// supported), else `disconnect` at once. From here every completion
    /// but `terminate`'s and `disconnect`'s is ignored, and the host's
    /// requests fail.
    fn shut_down(&mut self, reason: EndReason, by_user: bool, relaunch: bool) {
        if self.g.shutdown.is_some() {
            return;
        }
        self.forget_requests();
        self.g.shutdown = Some(Shutdown {
            reason,
            relaunch,
            phase: Phase::Disconnect,
        });
        self.set_state(SessionState::Terminating);
        if by_user && self.g.capabilities.supports_terminate_request == Some(true) {
            if let Some(shutdown) = &mut self.g.shutdown {
                shutdown.phase = Phase::Terminate;
            }
            let sleep = self.clock.sleep(super::TERMINATED_GRACE);
            self.g.timers.arm(Timer::Terminated, sleep);
            self.send(
                request::Terminate::COMMAND,
                to_value(&TerminateArguments::default()),
                Purpose::Terminate,
            );
        } else {
            self.disconnect();
        }
    }

    /// Fails the host's requests and drops the session's own, so no late
    /// completion changes anything.
    fn forget_requests(&mut self) {
        let pending = std::mem::take(&mut self.g.pending);
        for (seq, pending) in pending {
            self.g.timers.cancel(Timer::Request(seq));
            fail_ended(pending);
        }
        // No stop is current, and no resume pending, from here.
        self.s.new_run();
        self.advance_epoch();
        self.g.tokens.clear();
        for queued in std::mem::take(&mut self.queued) {
            let _ = queued.reply.send(Err(RequestError::Ended {
                command: queued.command,
            }));
        }
        self.g.restart = None;
        self.g.timers.cancel(Timer::Initialized);
    }

    fn disconnect(&mut self) {
        self.g.timers.cancel(Timer::Terminated);
        if let Some(shutdown) = &mut self.g.shutdown {
            shutdown.phase = Phase::Disconnect;
        }
        let arguments = DisconnectArguments {
            terminate_debuggee: Some(true),
            ..Default::default()
        };
        self.send(
            request::Disconnect::COMMAND,
            to_value(&arguments),
            Purpose::Disconnect,
        );
    }

    /// `terminated`: the user's `terminate` took, or the debuggee ended by
    /// itself.
    fn terminated(&mut self) {
        match self.phase() {
            Some(Phase::Terminate) => self.disconnect(),
            Some(_) => {}
            // The debuggee ended: `disconnect` at once, never `terminate`.
            None => self.shut_down(EndReason::Finished, false, false),
        }
    }

    /// The adapter's output ended: nothing more can be read, so wait for
    /// its exit, killing it if it lingers.
    fn output_closed(&mut self) {
        if self.phase() == Some(Phase::Kill) {
            return;
        }
        self.expect_exit();
        // Nothing will answer: give up the requests now.
        for (seq, pending) in std::mem::take(&mut self.g.pending) {
            self.g.timers.cancel(Timer::Request(seq));
            fail_ended(pending);
        }
        self.g.tokens.clear();
    }

    /// Nothing more can be exchanged with the adapter: wait for it to exit,
    /// killing it after [`super::EXIT_GRACE`]. A kill deadline already set
    /// stays as it is.
    fn expect_exit(&mut self) {
        if !self.g.timers.is_armed(Timer::Exit) {
            let sleep = self.clock.sleep(super::EXIT_GRACE);
            self.g.timers.arm(Timer::Exit, sleep);
        }
        if let Some(shutdown) = &mut self.g.shutdown {
            shutdown.phase = Phase::Exit;
        }
    }

    /// The writer stopped with `result`. A failure means nothing more
    /// reaches the adapter: the session ends with it, or, already shutting
    /// down, stops waiting for answers.
    fn writer_failed(&mut self, result: Result<(), String>) {
        let Err(why) = result else {
            return;
        };
        log::warn!(
            "couldn't write to debug adapter {}",
            self.options.adapter.name
        );
        match self.phase() {
            Some(Phase::Kill) => {}
            Some(_) => {
                self.forget_requests();
                self.expect_exit();
            }
            None => self.abort(SessionError::Transport(why)),
        }
    }

    /// Ends the session at once: the adapter is unresponsive or broke the
    /// protocol, so it is killed.
    fn abort(&mut self, error: SessionError) {
        // Only the error's kind goes to the application log: its text can
        // hold what the adapter sent.
        log::warn!(
            "debug adapter {}: {}; killing it",
            self.options.adapter.name,
            error_kind(&error)
        );
        match &mut self.g.shutdown {
            Some(shutdown) => {
                shutdown.relaunch = false;
                shutdown.phase = Phase::Kill;
            }
            None => {
                self.forget_requests();
                self.g.shutdown = Some(Shutdown {
                    reason: EndReason::Failed(error),
                    relaunch: false,
                    phase: Phase::Kill,
                });
                self.set_state(SessionState::Terminating);
            }
        }
        self.g.timers.cancel(Timer::Terminated);
        self.g.timers.cancel(Timer::Exit);
        self.g.kill = true;
    }
}

/// A transport error as the session reports it. A body that isn't a
/// protocol message is reported without serde's account of it, which quotes
/// (escaped, so past redaction) whatever the adapter sent.
fn transport_error(err: &crate::TransportError) -> String {
    match err {
        crate::TransportError::Protocol(crate::ProtocolError::BadMessage(_)) => {
            tr!("dap-session-bad-message").to_string()
        }
        other => other.to_string(),
    }
}

/// A session error's kind, for the application log: no text from the
/// adapter.
fn error_kind(error: &SessionError) -> &'static str {
    match error {
        SessionError::Launch(_) => "launch_failed",
        SessionError::InitializeFailed(_) => "initialize_failed",
        SessionError::InitializeTimeout(_) => "initialize_timeout",
        SessionError::StartFailed(_) => "start_failed",
        SessionError::StartTimeout(_) => "start_timeout",
        SessionError::ConfigurationDoneTimeout(_) => "configuration_done_timeout",
        SessionError::Transport(_) => "transport",
    }
}

/// Answers a request's host, if it has one, that the session ended.
fn fail_ended(pending: Pending) {
    let reply = match pending.purpose {
        Purpose::External { reply, .. } => reply,
        Purpose::Resume {
            reply: Some(reply), ..
        } => reply,
        _ => return,
    };
    let _ = reply.send(Err(RequestError::Ended {
        command: pending.command,
    }));
}

/// Parses a reply body as `T`; a missing body reads as `{}`.
fn parse<T: serde::de::DeserializeOwned>(body: Value) -> Option<T> {
    let body = match body {
        Value::Null => json!({}),
        body => body,
    };
    serde_json::from_value(body).ok()
}

/// The writer: each body framed, in order, until the session drops its
/// sender (`Ok`, the stream closed) or the stream fails (its error).
async fn write_all(
    mut writer: FrameWriter<Pin<Box<dyn AsyncWrite + Send>>>,
    mut bodies: mpsc::UnboundedReceiver<Vec<u8>>,
) -> Result<(), String> {
    while let Some(body) = bodies.next().await {
        writer
            .write_body(&body)
            .await
            .map_err(|err| crate::TransportError::Io(err).to_string())?;
    }
    let _ = writer.close().await;
    Ok(())
}

/// One of the adapter's other streams, read so it never blocks on a full
/// pipe.
struct Drain {
    stream: Pin<Box<dyn AsyncRead + Send>>,
}

impl Drain {
    fn new(stream: Pin<Box<dyn AsyncRead + Send>>) -> Self {
        Self { stream }
    }
}

/// The next chunk of `drain`'s stream; `None` once it has ended. Pending
/// forever with no stream.
async fn chunk(drain: Option<&mut Drain>) -> Option<Vec<u8>> {
    let Some(drain) = drain else {
        return futures::future::pending().await;
    };
    let mut buffer = [0; 4096];
    match drain.stream.read(&mut buffer).await {
        Ok(0) | Err(_) => None,
        Ok(read) => Some(buffer[..read].to_vec()),
    }
}
