// SPDX-License-Identifier: Apache-2.0

//! A scripted fake debug adapter for tests (docs/decisions/r1b9dw3g0g-debugger.md
//! §2), behind the `test-support` feature.
//!
//! [`serve`] runs it on a thread of its own over any byte stream: the
//! in-memory [`pipe`]s that [`connect`] and [`FakeLauncher`] hand the client
//! as its [`AdapterIo`], or an accepted TCP connection (the `fe-dap-fake`
//! binary). It records every message it receives and sends.
//!
//! **The program.** A [`Script`]'s [`Program`] is a list of source lines,
//! each in a function, and each function's locals. A program counter moves
//! over it deterministically: `next` and `stepIn` go to the next line,
//! `stepOut` to the next line in another function, and `continue` runs to
//! the next line with a breakpoint, or to a [`Line::spin`] line (a busy
//! loop that runs until paused), or to the end, where it sends `exited` and
//! `terminated`. There is one thread, id [`THREAD_ID`]. Frame ids and
//! variable references are valid for one stop only, as a real adapter's.
//!
//! **The knobs** for the hard orderings, one per item of §2's list:
//!
//! | §2 knob | [`Script`] field |
//! |---|---|
//! | the startup orderings of item 3 | [`startup`](Script::startup) |
//! | … with and without `supportsConfigurationDoneRequest` | [`capabilities`](Script::capabilities) |
//! | resumes answered with no `continued` event | [`continued_events`](Script::continued_events) |
//! | a `stopped` event sent before a step's response | [`stopped_before_step_response`](Script::stopped_before_step_response) |
//! | a resume that fails | [`faults`](Script::faults): [`FaultKind::Fail`] |
//! | a resume that is never answered | [`faults`](Script::faults): [`FaultKind::Unanswered`] |
//! | a resume's failure delayed until after another resume, or a terminate | [`faults`](Script::faults): [`FaultKind::FailAfter`] |
//! | a `variables` reply delayed past a step | [`faults`](Script::faults): [`FaultKind::ReplyAfter`] |
//! | a program that runs with no `thread` events | [`thread_events`](Script::thread_events) |
//! | breakpoint ids kept across a replacement | [`keep_breakpoint_ids`](Script::keep_breakpoint_ids) |
//! | unverified, then verified, breakpoints | [`verify_breakpoints_later`](Script::verify_breakpoints_later) |
//! | the adapter exiting mid-request | [`faults`](Script::faults): [`FaultKind::Exit`] |
//! | a request answered with a body of the test's (no threads) | [`faults`](Script::faults): [`FaultKind::Answer`] |
//!
//! A [`Fault`] applies to one request, by command and occurrence, so any
//! request can fail, go unanswered, be held, be answered as the test says
//! or end the adapter, and a held
//! reply is let go by a [`Trigger`]: the next resume, a terminate, a
//! `cancel` naming it, or the test ([`FakeAdapter::release`]).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::poll_fn;
use std::io;
use std::path::Path;
use std::pin::{Pin, pin};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, ready};
use std::thread;
use std::time::{Duration, Instant};

use futures::channel::{mpsc, oneshot};
use futures::future::{Either, FusedFuture as _, FutureExt as _, Shared as SharedFuture};
use futures::{AsyncRead, AsyncWrite, StreamExt as _};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::launcher::{
    AdapterIo, AdapterLauncher, AdapterProcess, AdapterSpec, ExitInfo, LaunchCancel,
};
use crate::protocol::{
    Breakpoint, BreakpointBody, BreakpointsBody, Capabilities, CompletionItem, CompletionsBody,
    ContinueBody, ContinuedBody, Empty, ErrorMessage, EvaluateBody, EventMessage,
    ExceptionBreakpointsFilter, ExitedBody, GotoTarget, GotoTargetsBody, Message, Object,
    OutputBody, RequestMessage, ResponseMessage, RunInTerminalArguments, Scope, ScopesBody,
    SetExceptionBreakpointsBody, SetVariableBody, Source, SourceBody, StackFrame, StackTraceBody,
    StartDebuggingArguments, StoppedBody, TerminatedBody, Thread, ThreadBody, ThreadsBody,
    Variable, VariablesBody, event, request, reverse,
};
use crate::transport::{FrameReader, FrameWriter};

/// The fake program's one thread.
pub const THREAD_ID: i64 = 1;

/// How the fake behaves. Every field has a default, so a script names only
/// what a test changes (also in JSON, for `fe-dap-fake`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Script {
    /// `initialize`'s answer. By default it supports `configurationDone`,
    /// `terminate`, `restart` and `cancel`, and offers two exception
    /// filters, `uncaught` on by default.
    pub capabilities: Capabilities,
    /// When `initialized` and the `launch` (or `attach`) response go.
    pub startup: Startup,
    pub program: Program,
    /// Stops on the first line, reason `entry`, when the program starts.
    pub stop_on_entry: bool,
    /// Sends `continued` after each resume's response (the default). False:
    /// resumes are answered with no `continued` event, as the specification
    /// has it for resumes the client asked for.
    pub continued_events: bool,
    /// A step (`next`, `stepIn`, `stepOut`) sends its `stopped` event before
    /// its response.
    pub stopped_before_step_response: bool,
    /// Sends `thread` events: `started` when the program starts and
    /// `exited` when it ends (the default). False: a program that runs with
    /// no `thread` events.
    pub thread_events: bool,
    /// `setBreakpoints` answers each breakpoint unverified (reason
    /// `pending`), and a `breakpoint` event verifies it when the program
    /// starts.
    pub verify_breakpoints_later: bool,
    /// `setBreakpoints` hands a line's earlier id back when the line is
    /// sent again (the protocol allows it), instead of a fresh one.
    pub keep_breakpoint_ids: bool,
    /// What goes wrong, request by request.
    pub faults: Vec<Fault>,
    /// The program's exit code, in `exited`.
    pub exit_code: i64,
    /// `output` events sent when the program starts.
    pub output: Vec<OutputBody>,
    /// `launch` first asks the client to `runInTerminal` the program, and
    /// is answered after the client's reply.
    pub run_in_terminal: bool,
    /// Sends a `startDebugging` request after answering `launch`.
    pub start_debugging: bool,
    /// Keeps running after `disconnect`, and after its input closes, until
    /// killed: a mute adapter.
    pub lingers: bool,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            capabilities: Capabilities {
                supports_configuration_done_request: Some(true),
                supports_terminate_request: Some(true),
                supports_restart_request: Some(true),
                supports_cancel_request: Some(true),
                supports_set_variable: Some(true),
                supports_completions_request: Some(true),
                supports_goto_targets_request: Some(true),
                supports_function_breakpoints: Some(true),
                exception_breakpoint_filters: Some(vec![
                    ExceptionBreakpointsFilter {
                        filter: "raised".into(),
                        label: "Raised Exceptions".into(),
                        default: Some(false),
                        ..Default::default()
                    },
                    ExceptionBreakpointsFilter {
                        filter: "uncaught".into(),
                        label: "Uncaught Exceptions".into(),
                        default: Some(true),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            },
            startup: Startup::default(),
            program: Program::default(),
            stop_on_entry: false,
            continued_events: true,
            stopped_before_step_response: false,
            thread_events: true,
            verify_breakpoints_later: false,
            keep_breakpoint_ids: false,
            faults: Vec::new(),
            exit_code: 0,
            output: Vec::new(),
            run_in_terminal: false,
            start_debugging: false,
            lingers: false,
        }
    }
}

impl Script {
    /// This script without `supportsConfigurationDoneRequest`: its
    /// configuration ends with `setExceptionBreakpoints`, which the client
    /// then always sends, and the program starts on it.
    pub fn without_configuration_done(mut self) -> Self {
        self.capabilities.supports_configuration_done_request = None;
        self
    }

    fn configuration_done(&self) -> bool {
        self.capabilities.supports_configuration_done_request == Some(true)
    }
}

/// The startup orderings of §2 item 3.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Startup {
    /// `initialized` right after `initialize`'s response; `launch` is
    /// answered once configuration is done: after `configurationDone`, or,
    /// without it, after `setExceptionBreakpoints`.
    #[default]
    InitializedFirst,
    /// `launch` is answered at once, and `initialized` follows it.
    LaunchResponseFirst,
    /// `launch` is answered at once, and `initialized` waits for
    /// [`FakeAdapter::release`]: an adapter slow to send it, or (never
    /// released) one that never does.
    InitializedOnRelease,
}

/// What goes wrong with one request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Fault {
    pub command: String,
    /// Which of the requests with this command, from 1.
    #[serde(default = "first")]
    pub occurrence: usize,
    pub kind: FaultKind,
}

fn first() -> usize {
    1
}

impl Fault {
    /// `kind`, for the first `command` request.
    pub fn new(command: impl Into<String>, kind: FaultKind) -> Self {
        Self {
            command: command.into(),
            occurrence: 1,
            kind,
        }
    }

    /// For the `occurrence`th (from 1).
    pub fn nth(mut self, occurrence: usize) -> Self {
        self.occurrence = occurrence;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultKind {
    /// Answered at once with `success: false` and this `message`; nothing
    /// runs.
    Fail(String),
    /// Never answered; nothing runs.
    Unanswered,
    /// Answered with `success: false` and `message`, but only after
    /// `trigger`; nothing runs.
    FailAfter { trigger: Trigger, message: String },
    /// Answered normally, but only after `trigger`. A resume runs when it
    /// is answered; any other request's reply is the one it had when it
    /// came (a `variables` reply from before a step).
    ReplyAfter(Trigger),
    /// The adapter exits with this code on receiving it, unanswered.
    Exit(i32),
    /// Answered at once with `success: true` and this body; nothing runs.
    /// For instance `threads` answering `{"threads": []}`: a program that
    /// reports no threads.
    Answer(Value),
}

/// What lets a held reply go.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// Once the next resume (`continue`, `next`, `stepIn`, `stepOut`,
    /// `goto`) has been answered, and its events sent.
    NextResume,
    /// Once a `terminate` or `disconnect` has been answered.
    Terminate,
    /// Once a `cancel` naming the held request has been answered.
    Cancel,
    /// When the test calls [`FakeAdapter::release`].
    Manual,
}

/// The fake's program.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Program {
    /// The source file's path, in every frame and breakpoint.
    pub path: String,
    /// Line 1 first. The first line's function is the entry point, at the
    /// bottom of every stack.
    pub lines: Vec<Line>,
    pub functions: Vec<Function>,
    /// The `Globals` scope's variables, marked `expensive`.
    pub globals: Vec<Local>,
}

impl Default for Program {
    fn default() -> Self {
        let line = |function: &str, text: &str| Line {
            text: text.into(),
            function: function.into(),
            spin: false,
        };
        Self {
            path: "/fake/src/main.rs".into(),
            lines: vec![
                line("main", "let x = 1;"),
                line("main", "let point = Point { x, y: 2 };"),
                line("main", "let y = add(x, 2);"),
                line("add", "a + b"),
                line("main", "println!(\"{y}\");"),
            ],
            functions: vec![
                Function {
                    name: "main".into(),
                    locals: vec![
                        Local::new("x", "1", "i32"),
                        Local {
                            children: vec![
                                Local::new("x", "1", "i32"),
                                Local::new("y", "2", "i32"),
                            ],
                            ..Local::new("point", "Point { x: 1, y: 2 }", "Point")
                        },
                    ],
                },
                Function {
                    name: "add".into(),
                    locals: vec![Local::new("a", "1", "i32"), Local::new("b", "2", "i32")],
                },
            ],
            globals: vec![Local::new("COUNT", "3", "usize")],
        }
    }
}

impl Program {
    /// The 1-based line holding `text`. Panics if none does.
    pub fn line_of(&self, text: &str) -> i64 {
        let index = self
            .lines
            .iter()
            .position(|line| line.text.contains(text))
            .unwrap_or_else(|| panic!("no line holds {text:?}"));
        index as i64 + 1
    }

    fn function(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|function| function.name == name)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Line {
    pub text: String,
    pub function: String,
    /// A busy loop: `continue` reaching it keeps running there until a
    /// `pause` (or a breakpoint on it, which stops first).
    pub spin: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Function {
    pub name: String,
    pub locals: Vec<Local>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Local {
    pub name: String,
    pub value: String,
    pub r#type: String,
    /// Fetched with `variables` on its reference.
    pub children: Vec<Local>,
}

impl Local {
    pub fn new(name: &str, value: &str, r#type: &str) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            r#type: r#type.into(),
            children: Vec::new(),
        }
    }
}

/// One direction of an in-memory byte stream.
pub fn pipe() -> (PipeReader, PipeWriter) {
    let (sender, receiver) = mpsc::unbounded();
    (
        PipeReader {
            chunks: receiver,
            chunk: Vec::new(),
            read: 0,
        },
        PipeWriter(sender),
    )
}

/// The writing end of a [`pipe`]. Closing it (or dropping it) ends the
/// reader's stream.
pub struct PipeWriter(mpsc::UnboundedSender<Vec<u8>>);

impl AsyncWrite for PipeWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(match self.0.unbounded_send(buf.to_vec()) {
            Ok(()) => Ok(buf.len()),
            Err(_) => Err(io::ErrorKind::BrokenPipe.into()),
        })
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.0.close_channel();
        Poll::Ready(Ok(()))
    }
}

/// The reading end of a [`pipe`]: each write arrives as written, and may
/// be read in pieces.
pub struct PipeReader {
    chunks: mpsc::UnboundedReceiver<Vec<u8>>,
    chunk: Vec<u8>,
    read: usize,
}

impl AsyncRead for PipeReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        while this.read == this.chunk.len() {
            match ready!(this.chunks.poll_next_unpin(cx)) {
                Some(chunk) => {
                    this.chunk = chunk;
                    this.read = 0;
                }
                None => return Poll::Ready(Ok(0)),
            }
        }
        let n = out.len().min(this.chunk.len() - this.read);
        out[..n].copy_from_slice(&this.chunk[this.read..this.read + n]);
        this.read += n;
        Poll::Ready(Ok(n))
    }
}

/// A fake serving a script over in-memory pipes, and the client's ends of
/// them as an [`AdapterIo`] whose process is the fake.
pub fn connect(script: Script) -> (AdapterIo, FakeAdapter) {
    let (client_reads, fake_writes) = pipe();
    let (fake_reads, client_writes) = pipe();
    let fake = serve(script, fake_reads, fake_writes);
    let io = AdapterIo {
        input: Box::pin(client_writes),
        output: Box::pin(client_reads),
        stderr: None,
        stdout: None,
        port: None,
        process: Box::new(FakeProcess(fake.clone())),
    };
    (io, fake)
}

/// An [`AdapterLauncher`] starting a fake per launch, all on one script.
pub struct FakeLauncher {
    script: Mutex<Script>,
    launched: Mutex<Vec<(AdapterSpec, FakeAdapter)>>,
    changed: Condvar,
}

impl FakeLauncher {
    pub fn new(script: Script) -> Self {
        Self {
            script: Mutex::new(script),
            launched: Mutex::default(),
            changed: Condvar::new(),
        }
    }

    /// Changes the script for later launches.
    pub fn set_script(&self, script: Script) {
        *lock(&self.script) = script;
    }

    /// Every fake launched, oldest first, with the spec it was launched
    /// for.
    pub fn launched(&self) -> Vec<(AdapterSpec, FakeAdapter)> {
        lock(&self.launched).clone()
    }

    /// The `n`th fake launched (from 0), waiting up to 10 s for it.
    pub fn wait_for_launch(&self, n: usize) -> FakeAdapter {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut launched = lock(&self.launched);
        loop {
            if let Some((_, fake)) = launched.get(n) {
                return fake.clone();
            }
            let now = Instant::now();
            assert!(now < deadline, "launch {n} never came");
            launched = self
                .changed
                .wait_timeout(launched, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl AdapterLauncher for FakeLauncher {
    fn launch(
        &self,
        spec: &AdapterSpec,
        _cwd: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<AdapterIo> {
        if cancel.is_cancelled() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let (io, fake) = connect(lock(&self.script).clone());
        lock(&self.launched).push((spec.clone(), fake));
        self.changed.notify_all();
        Ok(io)
    }
}

/// The fake as an [`AdapterProcess`]: it exits when the fake stops, and
/// [`kill`](AdapterProcess::kill) stops it.
struct FakeProcess(FakeAdapter);

impl AdapterProcess for FakeProcess {
    fn id(&self) -> Option<u32> {
        None
    }

    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>> {
        Box::pin(self.0.exited())
    }

    fn kill(&mut self) {
        self.0.kill();
    }
}

/// The test's handle on a running fake. Clones share it.
#[derive(Clone)]
pub struct FakeAdapter {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<Record>,
    changed: Condvar,
    controls: mpsc::UnboundedSender<Control>,
    exited: SharedFuture<oneshot::Receiver<ExitInfo>>,
}

#[derive(Default)]
struct Record {
    received: Vec<Message>,
    sent: Vec<Message>,
    exit: Option<ExitInfo>,
    killed: bool,
}

enum Control {
    Release,
    ReleaseOldest(usize),
    Kill,
    Send(Box<Message>),
    CloseOutput,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl FakeAdapter {
    /// Every message it has received, in order.
    pub fn received(&self) -> Vec<Message> {
        lock(&self.shared.state).received.clone()
    }

    /// Every message it has sent, in order.
    pub fn sent(&self) -> Vec<Message> {
        lock(&self.shared.state).sent.clone()
    }

    /// The commands of the requests it has received, in order.
    pub fn commands(&self) -> Vec<String> {
        self.received()
            .into_iter()
            .filter_map(|message| match message {
                Message::Request(request) => Some(request.command),
                _ => None,
            })
            .collect()
    }

    /// Blocks until `done` holds of what it has received and sent. Panics
    /// after 10 s.
    pub fn wait_until(&self, what: &str, done: impl Fn(&[Message], &[Message]) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut state = lock(&self.shared.state);
        loop {
            if done(&state.received, &state.sent) {
                return;
            }
            let now = Instant::now();
            assert!(now < deadline, "the fake never got to: {what}");
            state = self
                .shared
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Lets go the `n` oldest replies held for [`Trigger::Manual`], and no
    /// others.
    pub fn release_oldest(&self, n: usize) {
        let _ = self
            .shared
            .controls
            .unbounded_send(Control::ReleaseOldest(n));
    }

    /// Lets go every reply held for [`Trigger::Manual`].
    pub fn release(&self) {
        let _ = self.shared.controls.unbounded_send(Control::Release);
    }

    /// Sends `message` as its own, numbered in turn: an event or response
    /// the script wouldn't send (a second `initialized`, a late reply).
    pub fn send(&self, message: Message) {
        let _ = self
            .shared
            .controls
            .unbounded_send(Control::Send(Box::new(message)));
    }

    /// Closes its output once what it sent is written, and keeps running:
    /// an adapter whose stdout closed.
    pub fn close_output(&self) {
        let _ = self.shared.controls.unbounded_send(Control::CloseOutput);
    }

    /// Stops it, as a kill would: it sends nothing more.
    pub fn kill(&self) {
        lock(&self.shared.state).killed = true;
        let _ = self.shared.controls.unbounded_send(Control::Kill);
    }

    /// Whether it was killed.
    pub fn killed(&self) -> bool {
        lock(&self.shared.state).killed
    }

    /// How it ended, if it has.
    pub fn exit(&self) -> Option<ExitInfo> {
        lock(&self.shared.state).exit
    }

    /// Resolves when it has ended.
    pub fn exited(&self) -> impl Future<Output = ExitInfo> + Send + 'static {
        let exited = self.shared.exited.clone();
        async move { exited.await.unwrap_or_default() }
    }

    /// Blocks until it has ended. Panics after 10 s.
    pub fn wait_exit(&self) -> ExitInfo {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut state = lock(&self.shared.state);
        loop {
            if let Some(exit) = state.exit {
                return exit;
            }
            let now = Instant::now();
            assert!(now < deadline, "the fake never exited");
            state = self
                .shared
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

/// Runs the fake on a thread of its own, reading the client's messages from
/// `input` and writing its own to `output`.
pub fn serve<R, W>(script: Script, input: R, output: W) -> FakeAdapter
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let (controls, control_receiver) = mpsc::unbounded();
    let (exit_sender, exited) = oneshot::channel();
    let shared = Arc::new(Shared {
        state: Mutex::default(),
        changed: Condvar::new(),
        controls,
        exited: exited.shared(),
    });
    let fake = FakeAdapter {
        shared: shared.clone(),
    };
    let adapter = Adapter::new(script, FrameWriter::new(output), shared.clone());
    let spawned = thread::Builder::new()
        .name("fe-dap fake".into())
        .spawn(move || {
            let exit = async_io::block_on(adapter.run(FrameReader::new(input), control_receiver));
            lock(&shared.state).exit = Some(exit);
            shared.changed.notify_all();
            let _ = exit_sender.send(exit);
        });
    if let Err(err) = spawned {
        panic!("couldn't start the fake adapter's thread: {err}");
    }
    fake
}

/// Where the program is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Run {
    NotStarted,
    Stopped(usize),
    /// Running, spinning at this line (or about to move from it).
    Running(usize),
    Ended,
}

/// A reply held until its trigger.
struct Held {
    trigger: Trigger,
    /// The held request's `seq`, for [`Trigger::Cancel`].
    request_seq: i64,
    action: HeldAction,
}

enum HeldAction {
    /// What handling the request did when it came, in order: its messages,
    /// and the releases and exit it led to, held back with them.
    Steps(Vec<Step>),
    /// A resume, run when let go.
    Resume(RequestMessage),
    Fail(RequestMessage, String),
    /// The adapter exits, code 0 (after `disconnect`).
    Exit,
}

/// One thing a captured reply does when it is let go.
enum Step {
    Send(Message),
    /// Lets go what `trigger` (for [`Trigger::Cancel`], naming this request
    /// seq) holds.
    Release(Trigger, Option<i64>),
    /// Exits, once the replies let go before it have gone.
    Exit,
}

/// Where a list of variables lives in the program, so a reference reads
/// and `setVariable` changes the program itself.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Container {
    /// A function's locals.
    Locals(String),
    Globals,
    /// The children of the variable of this name in that container.
    Children(Box<Container>, String),
}

/// Where a resume goes.
enum Target {
    /// Stops at this line (or ends past the last), reason `reason`.
    Step(usize, &'static str),
    /// Runs from this line.
    Run(usize),
}

struct Adapter<W> {
    script: Script,
    /// Taken by [`run`](Self::run), which drives it beside everything else,
    /// so a stalled write never keeps a kill from being seen.
    writer: Option<FrameWriter<W>>,
    /// Framed bodies to the writer; `None` once the fake is ending.
    out: Option<mpsc::UnboundedSender<Vec<u8>>>,
    bodies: Option<mpsc::UnboundedReceiver<Vec<u8>>>,
    shared: Arc<Shared>,
    seq: i64,
    run: Run,
    /// Counts stops: frame ids and variable references are of one.
    epoch: i64,
    /// What each of this stop's variable references names.
    references: HashMap<i64, Container>,
    next_reference: i64,
    /// By line (0-based): breakpoint ids.
    breakpoints: BTreeMap<usize, Vec<i64>>,
    /// Function breakpoints, at their function's first line.
    function_breakpoints: BTreeMap<usize, Vec<i64>>,
    unverified: Vec<Breakpoint>,
    next_breakpoint: i64,
    /// By line: the id last handed out, for `keep_breakpoint_ids`.
    kept_ids: BTreeMap<i64, i64>,
    counts: HashMap<String, usize>,
    held: Vec<Held>,
    ready: VecDeque<HeldAction>,
    launch: Option<RequestMessage>,
    terminal_request: Option<i64>,
    launched: bool,
    configured: bool,
    started: bool,
    /// `terminate` or `disconnect` came: nothing starts the program after.
    terminated: bool,
    done: Option<ExitInfo>,
    /// While [`capture`](Self::capture) runs, what would be sent.
    capturing: Option<Vec<Step>>,
}

impl<W: AsyncWrite + Unpin> Adapter<W> {
    fn new(script: Script, writer: FrameWriter<W>, shared: Arc<Shared>) -> Self {
        let (out, bodies) = mpsc::unbounded();
        Self {
            script,
            writer: Some(writer),
            out: Some(out),
            bodies: Some(bodies),
            shared,
            seq: 0,
            run: Run::NotStarted,
            epoch: 0,
            references: HashMap::new(),
            next_reference: 1,
            breakpoints: BTreeMap::new(),
            function_breakpoints: BTreeMap::new(),
            unverified: Vec::new(),
            next_breakpoint: 1,
            kept_ids: BTreeMap::new(),
            counts: HashMap::new(),
            held: Vec::new(),
            ready: VecDeque::new(),
            launch: None,
            terminal_request: None,
            launched: false,
            configured: false,
            started: false,
            terminated: false,
            done: None,
            capturing: None,
        }
    }

    async fn run<R: AsyncRead + Unpin>(
        mut self,
        reader: FrameReader<R>,
        mut controls: mpsc::UnboundedReceiver<Control>,
    ) -> ExitInfo {
        let (Some(writer), Some(bodies)) = (self.writer.take(), self.bodies.take()) else {
            unreachable!("run takes the writer once");
        };
        let mut writing = pin!(write_all(writer, bodies).fuse());
        let mut reader = Some(reader);
        loop {
            while self.done.is_none()
                && let Some(action) = self.ready.pop_front()
            {
                self.act(action).await;
            }
            if let Some(exit) = self.done {
                return self.finish(exit, writing, &mut controls).await;
            }
            let input = {
                let mut read = pin!(async {
                    match reader.as_mut() {
                        Some(reader) => reader.read().await,
                        None => futures::future::pending().await,
                    }
                });
                poll_fn(|cx| {
                    if let Poll::Ready(control) = controls.poll_next_unpin(cx) {
                        return Poll::Ready(Either::Right(control));
                    }
                    // Writing goes on beside reading; when it stops (the
                    // stream failed), later sends go nowhere.
                    if !writing.is_terminated() {
                        let _ = writing.as_mut().poll(cx);
                    }
                    read.as_mut().poll(cx).map(Either::Left)
                })
                .await
            };
            match input {
                Either::Left(Ok(Some(message))) => {
                    self.record(|record| record.received.push(message.clone()));
                    self.receive(message).await;
                }
                Either::Left(Ok(None)) => {
                    reader = None;
                    if !self.script.lingers {
                        self.done = Some(ExitInfo {
                            code: Some(0),
                            signal: None,
                        });
                    }
                }
                Either::Left(Err(err)) => {
                    log::warn!("the fake adapter's input failed: {err}");
                    self.done = Some(ExitInfo {
                        code: Some(1),
                        signal: None,
                    });
                }
                Either::Right(Some(Control::Release)) => {
                    self.release(|held| held.trigger == Trigger::Manual)
                }
                Either::Right(Some(Control::ReleaseOldest(n))) => {
                    let mut left = n;
                    self.release(|held| {
                        let go = held.trigger == Trigger::Manual && left > 0;
                        if go {
                            left -= 1;
                        }
                        go
                    })
                }
                Either::Right(Some(Control::Kill)) => self.done = Some(KILLED),
                Either::Right(Some(Control::Send(message))) => self.send(*message).await,
                // The writer writes the rest, then closes the stream.
                Either::Right(Some(Control::CloseOutput)) => self.out = None,
                // The shared state holds a sender, so this can't happen.
                Either::Right(None) => {}
            }
        }
    }

    /// Ending: a killed fake drops its writer at once, without a close
    /// that could stall; otherwise what was sent is written and the stream
    /// closed, unless a kill comes first.
    async fn finish(
        &mut self,
        exit: ExitInfo,
        mut writing: Pin<&mut futures::future::Fuse<impl Future<Output = ()>>>,
        controls: &mut mpsc::UnboundedReceiver<Control>,
    ) -> ExitInfo {
        if exit == KILLED {
            return exit;
        }
        // Closing the queue lets the writer finish: the rest, then close.
        self.out = None;
        if writing.is_terminated() {
            return exit;
        }
        loop {
            match futures::future::select(writing.as_mut(), controls.next()).await {
                Either::Left(_) => return exit,
                Either::Right((Some(Control::Kill), _)) => return KILLED,
                Either::Right((
                    Some(
                        Control::Release
                        | Control::ReleaseOldest(_)
                        | Control::Send(_)
                        | Control::CloseOutput,
                    ),
                    _,
                )) => {}
                // Can't happen (see run); the writer then decides.
                Either::Right((None, _)) => {
                    writing.as_mut().await;
                    return exit;
                }
            }
        }
    }

    fn record(&self, change: impl FnOnce(&mut Record)) {
        change(&mut lock(&self.shared.state));
        self.shared.changed.notify_all();
    }

    async fn send(&mut self, mut message: Message) {
        if let Some(captured) = &mut self.capturing {
            captured.push(Step::Send(message));
            return;
        }
        if self.done.is_some() {
            return;
        }
        self.seq += 1;
        match &mut message {
            Message::Request(request) => request.seq = self.seq,
            Message::Response(response) => response.seq = self.seq,
            Message::Event(event) => event.seq = self.seq,
        }
        self.record(|record| record.sent.push(message.clone()));
        match serde_json::to_vec(&message) {
            Ok(body) => {
                if let Some(out) = &self.out {
                    let _ = out.unbounded_send(body);
                }
            }
            Err(err) => log::warn!("the fake adapter couldn't serialise a message: {err}"),
        }
    }

    async fn event<E: crate::protocol::Event>(&mut self, body: &E::Body) {
        self.send(Message::Event(EventMessage::new::<E>(0, body)))
            .await;
    }

    async fn respond<R: crate::protocol::Request>(
        &mut self,
        request: &RequestMessage,
        body: &R::Body,
    ) {
        self.send(Message::Response(request.respond::<R>(0, body)))
            .await;
    }

    async fn fail(&mut self, request: &RequestMessage, message: &str) {
        self.send(Message::Response(failure(request, message)))
            .await;
    }

    async fn receive(&mut self, message: Message) {
        match message {
            Message::Request(request) => self.request(request).await,
            Message::Response(response) => self.response(response).await,
            Message::Event(_) => {}
        }
    }

    /// Moves the held replies `which` picks to the ready queue, in order.
    fn release(&mut self, mut which: impl FnMut(&Held) -> bool) {
        let (going, staying) = std::mem::take(&mut self.held)
            .into_iter()
            .partition(|held| which(held));
        self.held = staying;
        self.ready
            .extend(going.into_iter().map(|held: Held| held.action));
    }

    /// Lets go what `trigger` holds (only the request `request_seq` for a
    /// [`Trigger::Cancel`]), or, while capturing, holds that back too.
    fn release_trigger(&mut self, trigger: Trigger, request_seq: Option<i64>) {
        if let Some(captured) = &mut self.capturing {
            captured.push(Step::Release(trigger, request_seq));
            return;
        }
        self.release(|held| {
            held.trigger == trigger && request_seq.is_none_or(|seq| held.request_seq == seq)
        });
    }

    /// Exits once the replies already let go have gone, or, while
    /// capturing, once the captured reply is let go.
    fn schedule_exit(&mut self) {
        match &mut self.capturing {
            Some(captured) => captured.push(Step::Exit),
            None => self.ready.push_back(HeldAction::Exit),
        }
    }

    /// Sends the failures already let go, ahead of anything else queued.
    async fn send_ready_failures(&mut self) {
        let ready = std::mem::take(&mut self.ready);
        for action in ready {
            if matches!(action, HeldAction::Fail(..)) {
                self.act(action).await;
            } else {
                self.ready.push_back(action);
            }
        }
    }

    async fn act(&mut self, action: HeldAction) {
        match action {
            HeldAction::Steps(steps) => {
                for step in steps {
                    match step {
                        Step::Send(message) => self.send(message).await,
                        Step::Release(trigger, seq) => self.release_trigger(trigger, seq),
                        Step::Exit => self.schedule_exit(),
                    }
                }
            }
            HeldAction::Resume(request) => self.resume(&request).await,
            HeldAction::Fail(request, message) => self.fail(&request, &message).await,
            HeldAction::Exit => {
                self.done = Some(ExitInfo {
                    code: Some(0),
                    signal: None,
                });
            }
        }
    }

    async fn request(&mut self, request: RequestMessage) {
        let count = self.counts.entry(request.command.clone()).or_default();
        *count += 1;
        let count = *count;
        let fault = self
            .script
            .faults
            .iter()
            .position(|fault| fault.command == request.command && fault.occurrence == count)
            .map(|at| self.script.faults.remove(at));
        let Some(fault) = fault else {
            return self.handle(request).await;
        };
        match fault.kind {
            FaultKind::Fail(message) => self.fail(&request, &message).await,
            FaultKind::Unanswered => {}
            FaultKind::FailAfter { trigger, message } => self.held.push(Held {
                trigger,
                request_seq: request.seq,
                action: HeldAction::Fail(request, message),
            }),
            FaultKind::ReplyAfter(trigger) => {
                let request_seq = request.seq;
                let action = if is_resume(&request.command) {
                    HeldAction::Resume(request)
                } else {
                    HeldAction::Steps(self.capture(request).await)
                };
                self.held.push(Held {
                    trigger,
                    request_seq,
                    action,
                });
            }
            FaultKind::Exit(code) => {
                self.done = Some(ExitInfo {
                    code: Some(code),
                    signal: None,
                });
            }
            FaultKind::Answer(body) => {
                let response = ResponseMessage {
                    request_seq: request.seq,
                    success: true,
                    command: request.command.clone(),
                    body: Some(body),
                    ..Default::default()
                };
                self.send(Message::Response(response)).await;
            }
        }
    }

    /// What handling `request` would send now, and the releases and exit
    /// it would lead to, without doing them. Its other effects (the
    /// program's state) happen now: a held reply is the one it had when it
    /// came.
    async fn capture(&mut self, request: RequestMessage) -> Vec<Step> {
        self.capturing = Some(Vec::new());
        self.handle(request).await;
        self.capturing.take().unwrap_or_default()
    }
}

/// How a killed fake ends.
const KILLED: ExitInfo = ExitInfo {
    code: None,
    signal: Some(9),
};

/// The fake's writer: each body framed, in order, then (once the queue
/// closes) the stream closed.
async fn write_all<W: AsyncWrite + Unpin>(
    mut writer: FrameWriter<W>,
    mut bodies: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    while let Some(body) = bodies.next().await {
        if let Err(err) = writer.write_body(&body).await {
            log::warn!("the fake adapter couldn't write: {err}");
            return;
        }
    }
    let _ = writer.close().await;
}

fn is_resume(command: &str) -> bool {
    matches!(command, "continue" | "next" | "stepIn" | "stepOut" | "goto")
}

fn failure(request: &RequestMessage, message: &str) -> ResponseMessage {
    request.fail(
        0,
        message,
        Some(ErrorMessage {
            id: 1,
            format: message.to_owned(),
            ..Default::default()
        }),
    )
}

/// Requests, one by one.
impl<W: AsyncWrite + Unpin> Adapter<W> {
    async fn handle(&mut self, request: RequestMessage) {
        match request.command.as_str() {
            "initialize" => {
                let capabilities = self.script.capabilities.clone();
                self.respond::<request::Initialize>(&request, &capabilities)
                    .await;
                if self.script.startup == Startup::InitializedFirst {
                    self.event::<event::Initialized>(&Empty::default()).await;
                }
            }
            "launch" | "attach" => {
                if self.script.run_in_terminal {
                    let arguments: Object =
                        request.arguments::<request::Launch>().unwrap_or_default();
                    let mut args = vec![self.script.program.path.clone()];
                    if let Some(Value::Array(more)) = arguments.get("args") {
                        args.extend(
                            more.iter()
                                .filter_map(|arg| arg.as_str().map(str::to_owned)),
                        );
                    }
                    let terminal = RunInTerminalArguments {
                        cwd: arguments
                            .get("cwd")
                            .and_then(Value::as_str)
                            .unwrap_or("/fake")
                            .to_owned(),
                        args,
                        kind: Some("integrated".into()),
                        title: Some("fake program".into()),
                        ..Default::default()
                    };
                    self.launch = Some(request);
                    self.send(Message::Request(RequestMessage::new::<
                        reverse::RunInTerminal,
                    >(0, &terminal)))
                        .await;
                    self.terminal_request = Some(self.seq);
                } else {
                    self.launch = Some(request);
                    self.proceed_with_launch().await;
                }
            }
            "configurationDone" => {
                self.respond::<request::ConfigurationDone>(&request, &Empty::default())
                    .await;
                self.configured().await;
            }
            "setBreakpoints" => self.set_breakpoints(&request).await,
            "setFunctionBreakpoints" => self.set_function_breakpoints(&request).await,
            "setExceptionBreakpoints" => {
                self.respond::<request::SetExceptionBreakpoints>(
                    &request,
                    &SetExceptionBreakpointsBody::default(),
                )
                .await;
                if !self.script.configuration_done() {
                    self.configured().await;
                }
            }
            "threads" => {
                let threads = if self.started && self.run != Run::Ended {
                    vec![Thread {
                        id: THREAD_ID,
                        name: "main".into(),
                        ..Default::default()
                    }]
                } else {
                    Vec::new()
                };
                self.respond::<request::Threads>(
                    &request,
                    &ThreadsBody {
                        threads,
                        ..Default::default()
                    },
                )
                .await;
            }
            "stackTrace" => self.stack_trace(&request).await,
            "scopes" => self.scopes(&request).await,
            "variables" => self.variables(&request).await,
            "evaluate" => self.evaluate(&request).await,
            "setVariable" => self.set_variable(&request).await,
            "completions" => self.completions(&request).await,
            "source" => {
                let content = self
                    .script
                    .program
                    .lines
                    .iter()
                    .map(|line| format!("{}\n", line.text))
                    .collect();
                self.respond::<request::SourceRequest>(
                    &request,
                    &SourceBody {
                        content,
                        mime_type: Some("text/x-rust".into()),
                        ..Default::default()
                    },
                )
                .await;
            }
            "gotoTargets" => {
                let line = request
                    .arguments::<request::GotoTargets>()
                    .map(|args| args.line)
                    .unwrap_or_default();
                let targets = if (1..=self.script.program.lines.len() as i64).contains(&line) {
                    vec![GotoTarget {
                        id: line,
                        label: format!("line {line}"),
                        line,
                        ..Default::default()
                    }]
                } else {
                    Vec::new()
                };
                self.respond::<request::GotoTargets>(
                    &request,
                    &GotoTargetsBody {
                        targets,
                        ..Default::default()
                    },
                )
                .await;
            }
            command if is_resume(command) => self.resume(&request).await,
            "pause" => {
                self.respond::<request::Pause>(&request, &Empty::default())
                    .await;
                if let Run::Running(line) = self.run {
                    self.stop(line, "pause", Vec::new()).await;
                }
            }
            "cancel" => {
                let cancelled = request
                    .arguments::<request::Cancel>()
                    .ok()
                    .and_then(|args| args.request_id);
                self.respond::<request::Cancel>(&request, &Empty::default())
                    .await;
                if let Some(cancelled) = cancelled {
                    self.release_trigger(Trigger::Cancel, Some(cancelled));
                }
            }
            "terminate" => {
                self.respond::<request::Terminate>(&request, &Empty::default())
                    .await;
                self.end_startup().await;
                // A failure held for the terminate goes out before the end:
                // the session closes the transport on `terminated`, which
                // would otherwise race the failure's send.
                if self.started && self.run != Run::Ended {
                    self.release_trigger(Trigger::Terminate, None);
                    self.send_ready_failures().await;
                    self.end().await;
                } else {
                    self.event::<event::Terminated>(&TerminatedBody::default())
                        .await;
                    self.release_trigger(Trigger::Terminate, None);
                }
            }
            "disconnect" => {
                self.respond::<request::Disconnect>(&request, &Empty::default())
                    .await;
                self.end_startup().await;
                self.release_trigger(Trigger::Terminate, None);
                if !self.script.lingers {
                    self.schedule_exit();
                }
            }
            "restart" => {
                self.respond::<request::Restart>(&request, &Empty::default())
                    .await;
                self.run = Run::NotStarted;
                self.started = false;
                self.configured = false;
                self.retire();
                self.event::<event::Initialized>(&Empty::default()).await;
            }
            _ => self.fail(&request, "unrecognised request").await,
        }
    }

    /// A reply to one of the fake's own requests.
    async fn response(&mut self, response: ResponseMessage) {
        if self.terminal_request == Some(response.request_seq) {
            self.terminal_request = None;
            self.proceed_with_launch().await;
        }
    }

    /// Answers `launch` (or holds its answer) as [`Startup`] says.
    async fn proceed_with_launch(&mut self) {
        let Some(launch) = self.launch.clone() else {
            return;
        };
        match self.script.startup {
            Startup::InitializedFirst => {
                if self.configured {
                    self.answer_launch(&launch).await;
                }
            }
            Startup::LaunchResponseFirst => {
                self.answer_launch(&launch).await;
                self.event::<event::Initialized>(&Empty::default()).await;
            }
            Startup::InitializedOnRelease => {
                self.answer_launch(&launch).await;
                self.held.push(Held {
                    trigger: Trigger::Manual,
                    request_seq: launch.seq,
                    action: HeldAction::Steps(vec![Step::Send(Message::Event(
                        EventMessage::new::<event::Initialized>(0, &Empty::default()),
                    ))]),
                });
            }
        }
        self.maybe_start().await;
    }

    async fn answer_launch(&mut self, launch: &RequestMessage) {
        if self.launched {
            return;
        }
        self.launched = true;
        self.respond::<request::Launch>(launch, &Empty::default())
            .await;
        if self.script.start_debugging {
            let child = StartDebuggingArguments {
                configuration: Object::from_iter([("name".to_owned(), json!("child"))]),
                request: "launch".into(),
                ..Default::default()
            };
            self.send(Message::Request(RequestMessage::new::<
                reverse::StartDebugging,
            >(0, &child)))
                .await;
        }
    }

    /// Configuration is done: `configurationDone`, or (without it)
    /// `setExceptionBreakpoints`.
    async fn configured(&mut self) {
        self.configured = true;
        if self.script.startup == Startup::InitializedFirst
            && self.terminal_request.is_none()
            && let Some(launch) = self.launch.clone()
        {
            self.answer_launch(&launch).await;
        }
        self.maybe_start().await;
    }

    /// The session is ending: a launch not yet answered fails, and no
    /// later configuration or `runInTerminal` reply starts the program.
    async fn end_startup(&mut self) {
        self.terminated = true;
        self.terminal_request = None;
        if !self.launched
            && let Some(launch) = self.launch.clone()
        {
            self.launched = true;
            self.fail(&launch, "the session was terminated").await;
        }
    }

    async fn maybe_start(&mut self) {
        if self.terminated || !self.launched || !self.configured || self.started {
            return;
        }
        self.started = true;
        if self.script.thread_events {
            self.event::<event::ThreadEvent>(&ThreadBody {
                reason: "started".into(),
                thread_id: THREAD_ID,
                ..Default::default()
            })
            .await;
        }
        for output in self.script.output.clone() {
            self.event::<event::Output>(&output).await;
        }
        for breakpoint in std::mem::take(&mut self.unverified) {
            self.event::<event::BreakpointEvent>(&BreakpointBody {
                reason: "changed".into(),
                breakpoint: Breakpoint {
                    verified: true,
                    reason: None,
                    ..breakpoint
                },
                ..Default::default()
            })
            .await;
        }
        if self.script.stop_on_entry {
            self.stop(0, "entry", Vec::new()).await;
        } else {
            self.run = Run::Running(0);
            self.run_from(0).await;
        }
    }

    async fn set_breakpoints(&mut self, request: &RequestMessage) {
        let Ok(args) = request.arguments::<request::SetBreakpoints>() else {
            return self.fail(request, "bad arguments").await;
        };
        let lines: Vec<i64> = match (&args.breakpoints, &args.lines) {
            (Some(breakpoints), _) => breakpoints.iter().map(|b| b.line).collect(),
            (None, Some(lines)) => lines.clone(),
            (None, None) => Vec::new(),
        };
        let ours = args.source.path.as_deref() == Some(self.script.program.path.as_str());
        if ours {
            // The source's breakpoints are replaced: those still waiting to
            // be verified with them (only this source's ever wait).
            self.breakpoints.clear();
            self.unverified.clear();
        }
        let mut answered = Vec::new();
        for line in lines {
            let kept = (self.script.keep_breakpoint_ids && ours)
                .then(|| self.kept_ids.get(&line).copied())
                .flatten();
            let id = kept.unwrap_or_else(|| {
                let id = self.next_breakpoint;
                self.next_breakpoint += 1;
                id
            });
            if self.script.keep_breakpoint_ids && ours {
                self.kept_ids.insert(line, id);
            }
            let exists = ours && (1..=self.script.program.lines.len() as i64).contains(&line);
            let mut breakpoint = Breakpoint {
                verified: exists,
                id: Some(id),
                source: Some(args.source.clone()),
                line: Some(line),
                ..Default::default()
            };
            if exists {
                self.breakpoints
                    .entry(line as usize - 1)
                    .or_default()
                    .push(id);
                if self.script.verify_breakpoints_later && !self.started {
                    breakpoint.verified = false;
                    breakpoint.reason = Some("pending".into());
                    self.unverified.push(breakpoint.clone());
                }
            } else {
                breakpoint.message = Some("no code at this line".into());
                breakpoint.reason = Some("failed".into());
            }
            answered.push(breakpoint);
        }
        self.respond::<request::SetBreakpoints>(
            request,
            &BreakpointsBody {
                breakpoints: answered,
                ..Default::default()
            },
        )
        .await;
    }

    async fn set_function_breakpoints(&mut self, request: &RequestMessage) {
        let Ok(args) = request.arguments::<request::SetFunctionBreakpoints>() else {
            return self.fail(request, "bad arguments").await;
        };
        self.function_breakpoints.clear();
        let mut answered = Vec::new();
        for wanted in args.breakpoints {
            let id = self.next_breakpoint;
            self.next_breakpoint += 1;
            let at = self
                .script
                .program
                .lines
                .iter()
                .position(|line| line.function == wanted.name);
            if let Some(at) = at {
                self.function_breakpoints.entry(at).or_default().push(id);
            }
            answered.push(Breakpoint {
                verified: at.is_some(),
                id: Some(id),
                line: at.map(|at| at as i64 + 1),
                ..Default::default()
            });
        }
        self.respond::<request::SetFunctionBreakpoints>(
            request,
            &BreakpointsBody {
                breakpoints: answered,
                ..Default::default()
            },
        )
        .await;
    }

    /// The frames of a stop at `line`: its function's, then the entry
    /// point's at the line it called from, if that differs.
    fn frames(&self, line: usize) -> Vec<(String, usize)> {
        let lines = &self.script.program.lines;
        let mut frames = vec![(lines[line].function.clone(), line)];
        let entry = &lines[0].function;
        if &lines[line].function != entry
            && let Some(caller) = (0..line).rev().find(|&at| &lines[at].function == entry)
        {
            frames.push((entry.clone(), caller));
        }
        frames
    }

    fn frame_id(&self, index: usize) -> i64 {
        self.epoch * 1000 + index as i64
    }

    /// The frame (function and line) `frame_id` names in this stop.
    fn frame(&self, frame_id: i64) -> Option<(String, usize)> {
        let Run::Stopped(line) = self.run else {
            return None;
        };
        if frame_id / 1000 != self.epoch {
            return None;
        }
        self.frames(line)
            .get(usize::try_from(frame_id % 1000).ok()?)
            .cloned()
    }

    fn source(&self) -> Source {
        Source {
            name: Path::new(&self.script.program.path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
            path: Some(self.script.program.path.clone()),
            ..Default::default()
        }
    }

    async fn stack_trace(&mut self, request: &RequestMessage) {
        let Run::Stopped(line) = self.run else {
            return self.fail(request, "notStopped").await;
        };
        let args = request
            .arguments::<request::StackTrace>()
            .unwrap_or_default();
        let frames = self.frames(line);
        let total = frames.len() as i64;
        let start = args.start_frame.unwrap_or(0).max(0) as usize;
        let levels = args.levels.filter(|&levels| levels > 0).unwrap_or(total) as usize;
        let stack_frames = frames
            .into_iter()
            .enumerate()
            .skip(start)
            .take(levels)
            .map(|(index, (function, line))| StackFrame {
                id: self.frame_id(index),
                name: function,
                line: line as i64 + 1,
                column: 1,
                source: Some(self.source()),
                ..Default::default()
            })
            .collect();
        self.respond::<request::StackTrace>(
            request,
            &StackTraceBody {
                stack_frames,
                total_frames: Some(total),
                ..Default::default()
            },
        )
        .await;
    }

    fn reference(&mut self, container: Container) -> i64 {
        let reference = self.next_reference;
        self.next_reference += 1;
        self.references.insert(reference, container);
        reference
    }

    /// The variables `container` holds; none for a function the program
    /// doesn't list.
    fn resolve(&self, container: &Container) -> Option<&[Local]> {
        let program = &self.script.program;
        match container {
            Container::Locals(function) => Some(
                program
                    .function(function)
                    .map_or(&[][..], |function| &function.locals[..]),
            ),
            Container::Globals => Some(&program.globals),
            Container::Children(parent, name) => self
                .resolve(parent)?
                .iter()
                .find(|local| &local.name == name)
                .map(|local| &local.children[..]),
        }
    }

    fn resolve_mut(&mut self, container: &Container) -> Option<&mut Vec<Local>> {
        fn walk<'a>(program: &'a mut Program, container: &Container) -> Option<&'a mut Vec<Local>> {
            match container {
                Container::Locals(function) => program
                    .functions
                    .iter_mut()
                    .find(|f| &f.name == function)
                    .map(|function| &mut function.locals),
                Container::Globals => Some(&mut program.globals),
                Container::Children(parent, name) => walk(program, parent)?
                    .iter_mut()
                    .find(|local| &local.name == name)
                    .map(|local| &mut local.children),
            }
        }
        walk(&mut self.script.program, container)
    }

    fn locals(&self, function: &str) -> Vec<Local> {
        self.resolve(&Container::Locals(function.to_owned()))
            .unwrap_or_default()
            .to_vec()
    }

    async fn scopes(&mut self, request: &RequestMessage) {
        let frame = request
            .arguments::<request::Scopes>()
            .ok()
            .and_then(|args| self.frame(args.frame_id));
        let Some((function, _)) = frame else {
            return self.fail(request, "invalid frameId").await;
        };
        let scopes = vec![
            Scope {
                name: "Locals".into(),
                variables_reference: self.reference(Container::Locals(function)),
                expensive: false,
                presentation_hint: Some("locals".into()),
                ..Default::default()
            },
            Scope {
                name: "Globals".into(),
                variables_reference: self.reference(Container::Globals),
                expensive: true,
                ..Default::default()
            },
        ];
        self.respond::<request::Scopes>(
            request,
            &ScopesBody {
                scopes,
                ..Default::default()
            },
        )
        .await;
    }

    /// `local`, of `container`, as a `Variable`, with a reference to its
    /// children if it has any.
    fn variable(&mut self, container: &Container, local: Local) -> Variable {
        let variables_reference = if local.children.is_empty() {
            0
        } else {
            self.reference(Container::Children(
                Box::new(container.clone()),
                local.name.clone(),
            ))
        };
        Variable {
            name: local.name.clone(),
            value: local.value,
            variables_reference,
            r#type: Some(local.r#type),
            evaluate_name: Some(local.name),
            ..Default::default()
        }
    }

    async fn variables(&mut self, request: &RequestMessage) {
        let found = request
            .arguments::<request::Variables>()
            .ok()
            .and_then(|args| self.references.get(&args.variables_reference).cloned());
        let Some(container) = found else {
            return self.fail(request, "invalid variablesReference").await;
        };
        let locals = self.resolve(&container).unwrap_or_default().to_vec();
        let variables = locals
            .into_iter()
            .map(|local| self.variable(&container, local))
            .collect();
        self.respond::<request::Variables>(
            request,
            &VariablesBody {
                variables,
                ..Default::default()
            },
        )
        .await;
    }

    /// The function a request's optional `frameId` names, by default the
    /// top frame's.
    fn function_of(&self, frame_id: Option<i64>) -> Option<String> {
        match frame_id {
            Some(frame_id) => self.frame(frame_id).map(|(function, _)| function),
            None => match self.run {
                Run::Stopped(line) | Run::Running(line) => {
                    Some(self.script.program.lines[line].function.clone())
                }
                _ => None,
            },
        }
    }

    async fn evaluate(&mut self, request: &RequestMessage) {
        let Ok(args) = request.arguments::<request::Evaluate>() else {
            return self.fail(request, "bad arguments").await;
        };
        let containers = self
            .function_of(args.frame_id)
            .map(Container::Locals)
            .into_iter()
            .chain([Container::Globals]);
        let found = containers.into_iter().find_map(|container| {
            let local = self
                .resolve(&container)?
                .iter()
                .find(|local| local.name == args.expression)?
                .clone();
            Some((container, local))
        });
        let Some((container, local)) = found else {
            let message = format!("{} isn't defined", args.expression);
            return self.fail(request, &message).await;
        };
        let variable = self.variable(&container, local);
        self.respond::<request::Evaluate>(
            request,
            &EvaluateBody {
                result: variable.value,
                variables_reference: variable.variables_reference,
                r#type: variable.r#type,
                ..Default::default()
            },
        )
        .await;
    }

    async fn set_variable(&mut self, request: &RequestMessage) {
        let Ok(args) = request.arguments::<request::SetVariable>() else {
            return self.fail(request, "bad arguments").await;
        };
        let Some(container) = self.references.get(&args.variables_reference).cloned() else {
            return self.fail(request, "invalid variablesReference").await;
        };
        // Only the member of the container the reference names.
        let changed = self
            .resolve_mut(&container)
            .and_then(|locals| locals.iter_mut().find(|local| local.name == args.name))
            .map(|local| {
                local.value = args.value.clone();
                local.r#type.clone()
            });
        let Some(r#type) = changed else {
            return self.fail(request, "no such variable").await;
        };
        self.respond::<request::SetVariable>(
            request,
            &SetVariableBody {
                value: args.value,
                r#type: Some(r#type),
                ..Default::default()
            },
        )
        .await;
    }

    async fn completions(&mut self, request: &RequestMessage) {
        let Ok(args) = request.arguments::<request::Completions>() else {
            return self.fail(request, "bad arguments").await;
        };
        let targets = self
            .function_of(args.frame_id)
            .map(|function| self.locals(&function))
            .unwrap_or_default()
            .into_iter()
            .filter(|local| local.name.starts_with(&args.text))
            .map(|local| CompletionItem {
                label: local.name,
                r#type: Some("variable".into()),
                ..Default::default()
            })
            .collect();
        self.respond::<request::Completions>(
            request,
            &CompletionsBody {
                targets,
                ..Default::default()
            },
        )
        .await;
    }
}

/// The program counter.
impl<W: AsyncWrite + Unpin> Adapter<W> {
    /// Retires this stop's frame ids and variable references.
    fn retire(&mut self) {
        self.references.clear();
    }

    async fn stop(&mut self, line: usize, reason: &str, hit: Vec<i64>) {
        self.run = Run::Stopped(line);
        self.epoch += 1;
        self.retire();
        self.event::<event::Stopped>(&StoppedBody {
            reason: reason.into(),
            thread_id: Some(THREAD_ID),
            all_threads_stopped: Some(true),
            hit_breakpoint_ids: (!hit.is_empty()).then_some(hit),
            ..Default::default()
        })
        .await;
    }

    /// Runs from `from` (inclusive) to a breakpoint, a spinning line, or
    /// the end.
    async fn run_from(&mut self, from: usize) {
        for line in from..self.script.program.lines.len() {
            let hit: Vec<i64> = self
                .breakpoints
                .get(&line)
                .into_iter()
                .chain(self.function_breakpoints.get(&line))
                .flatten()
                .copied()
                .collect();
            if !hit.is_empty() {
                return self.stop(line, "breakpoint", hit).await;
            }
            if self.script.program.lines[line].spin {
                self.run = Run::Running(line);
                return;
            }
        }
        self.end().await;
    }

    /// The program ends: `thread` exited, `exited` and `terminated`.
    async fn end(&mut self) {
        self.run = Run::Ended;
        self.retire();
        if self.script.thread_events {
            self.event::<event::ThreadEvent>(&ThreadBody {
                reason: "exited".into(),
                thread_id: THREAD_ID,
                ..Default::default()
            })
            .await;
        }
        let exit_code = self.script.exit_code;
        self.event::<event::Exited>(&ExitedBody {
            exit_code,
            ..Default::default()
        })
        .await;
        self.event::<event::Terminated>(&TerminatedBody::default())
            .await;
    }

    fn target(&self, request: &RequestMessage, line: usize) -> Target {
        let lines = &self.script.program.lines;
        match request.command.as_str() {
            "continue" => Target::Run(line + 1),
            "stepOut" => {
                let function = &lines[line].function;
                let out = (line + 1..lines.len())
                    .find(|&at| &lines[at].function != function)
                    .unwrap_or(lines.len());
                Target::Step(out, "step")
            }
            "goto" => {
                let target = request
                    .arguments::<request::Goto>()
                    .map(|args| args.target_id)
                    .unwrap_or_default();
                Target::Step(usize::try_from(target - 1).unwrap_or(line), "goto")
            }
            // next, stepIn
            _ => Target::Step(line + 1, "step"),
        }
    }

    async fn go(&mut self, target: Target) {
        match target {
            Target::Step(line, _) if line >= self.script.program.lines.len() => self.end().await,
            Target::Step(line, reason) => self.stop(line, reason, Vec::new()).await,
            Target::Run(from) => self.run_from(from).await,
        }
    }

    async fn resume(&mut self, request: &RequestMessage) {
        let Run::Stopped(line) = self.run else {
            return self.fail(request, "notStopped").await;
        };
        let target = self.target(request, line);
        let step = matches!(request.command.as_str(), "next" | "stepIn" | "stepOut");
        self.retire();
        if step && self.script.stopped_before_step_response {
            self.go(target).await;
            self.respond::<request::Next>(request, &Empty::default())
                .await;
        } else {
            if request.command == "continue" {
                let body = ContinueBody {
                    all_threads_continued: Some(true),
                    ..Default::default()
                };
                self.respond::<request::Continue>(request, &body).await;
            } else {
                self.respond::<request::Next>(request, &Empty::default())
                    .await;
            }
            self.run = Run::Running(line);
            if self.script.continued_events {
                self.event::<event::Continued>(&ContinuedBody {
                    thread_id: THREAD_ID,
                    all_threads_continued: Some(true),
                    ..Default::default()
                })
                .await;
            }
            self.go(target).await;
        }
        self.release_trigger(Trigger::NextResume, None);
    }
}
