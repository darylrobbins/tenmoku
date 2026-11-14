// SPDX-License-Identifier: Apache-2.0

//! What the session tests share: a [`Session`] on a [`FakeLauncher`] and a
//! [`ManualClock`], its events collected for the test to wait on.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::clock::ManualClock;
use crate::fake::{FakeAdapter, FakeLauncher, Program, Script};
use crate::launcher::AdapterSpec;
use crate::protocol::{Message, Object, RequestMessage, ResponseMessage, SourceBreakpoint};
use crate::session::{DapEvent, Ended, Session, SessionOptions, SessionState};

/// The fake program's source path.
pub(super) fn program_path() -> String {
    Program::default().path
}

/// The line of the default program the breakpoint tests stop at.
pub(super) fn breakpoint_line() -> i64 {
    Program::default().line_of("let y = add")
}

/// Options for the fake: `launch` with a program, and no breakpoints.
pub(super) fn options() -> SessionOptions {
    SessionOptions {
        adapter: AdapterSpec::new("fake", "fake"),
        cwd: "/fake".into(),
        arguments: object(json!({ "program": "/fake/main" })),
        ..Default::default()
    }
}

/// [`options`] with a breakpoint on [`breakpoint_line`].
pub(super) fn options_with_breakpoint() -> SessionOptions {
    let mut options = options();
    options.breakpoints.insert(
        program_path(),
        vec![SourceBreakpoint {
            line: breakpoint_line(),
            ..Default::default()
        }],
    );
    options
}

pub(super) fn object(value: Value) -> Object {
    match value {
        Value::Object(map) => map,
        other => panic!("not an object: {other}"),
    }
}

pub(super) struct Harness {
    pub(super) session: Session,
    pub(super) launcher: Arc<FakeLauncher>,
    pub(super) clock: Arc<ManualClock>,
    receiver: Receiver<DapEvent>,
    /// Every event so far, oldest first.
    pub(super) events: Vec<DapEvent>,
}

pub(super) fn start(script: Script, options: SessionOptions) -> Harness {
    let launcher = Arc::new(FakeLauncher::new(script));
    let clock = Arc::new(ManualClock::new());
    let (sender, receiver) = channel();
    let session = Session::start(options, launcher.clone(), clock.clone(), move |event| {
        let _ = sender.send(event);
    })
    .expect("the session thread starts");
    Harness {
        session,
        launcher,
        clock,
        receiver,
        events: Vec::new(),
    }
}

impl Harness {
    /// The `n`th fake launched, from 0.
    pub(super) fn fake(&self, n: usize) -> FakeAdapter {
        self.launcher.wait_for_launch(n)
    }

    /// Takes in events until `done` holds of them all. Panics after 10 s.
    pub(super) fn wait(&mut self, what: &str, done: impl Fn(&[DapEvent]) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(&self.events) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.receiver.recv_timeout(left) {
                Ok(event) => self.events.push(event),
                Err(RecvTimeoutError::Timeout) => {
                    panic!("never got to {what}: {:?}", self.states())
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("the session ended before {what}: {:?}", self.states())
                }
            }
        }
    }

    /// Takes in every event already sent.
    pub(super) fn drain(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            self.events.push(event);
        }
    }

    /// The states the session has been in, in order.
    pub(super) fn states(&self) -> Vec<SessionState> {
        states(&self.events)
    }

    /// Waits until the session has been in a state `wanted` holds of,
    /// `nth` times (from 1).
    pub(super) fn wait_state_times(
        &mut self,
        what: &str,
        nth: usize,
        wanted: impl Fn(&SessionState) -> bool,
    ) {
        self.wait(what, |events| {
            states(events).iter().filter(|state| wanted(state)).count() >= nth
        });
    }

    pub(super) fn wait_state(&mut self, wanted: SessionState) {
        let what = format!("{wanted:?}");
        self.wait_state_times(&what, 1, |state| *state == wanted);
    }

    /// Waits for the end, and how it went.
    pub(super) fn wait_ended(&mut self) -> Ended {
        self.wait_state_times("the end", 1, SessionState::is_ended);
        match self.states().last() {
            Some(SessionState::Ended(ended)) => ended.clone(),
            other => panic!("the last state isn't the end: {other:?}"),
        }
    }

    /// The protocol log, one line per entry.
    pub(super) fn log_lines(&self) -> Vec<String> {
        self.session
            .log()
            .iter()
            .map(|entry| entry.text.clone())
            .collect()
    }

    /// The index of the first log line holding every one of `parts`.
    pub(super) fn log_index(&self, parts: &[&str]) -> usize {
        let lines = self.log_lines();
        lines
            .iter()
            .position(|line| parts.iter().all(|part| line.contains(part)))
            .unwrap_or_else(|| panic!("no log line holds {parts:?}: {lines:#?}"))
    }
}

pub(super) fn states(events: &[DapEvent]) -> Vec<SessionState> {
    events
        .iter()
        .filter_map(|event| match event {
            DapEvent::State(state) => Some(state.clone()),
            _ => None,
        })
        .collect()
}

/// The requests `fake` has received with `command`.
pub(super) fn requests(fake: &FakeAdapter, command: &str) -> Vec<RequestMessage> {
    fake.received()
        .into_iter()
        .filter_map(|message| match message {
            Message::Request(request) if request.command == command => Some(request),
            _ => None,
        })
        .collect()
}

/// Waits until `fake` has received `count` requests with `command`.
pub(super) fn wait_requests(
    fake: &FakeAdapter,
    command: &str,
    count: usize,
) -> Vec<RequestMessage> {
    fake.wait_until(command, |received, _| {
        received
            .iter()
            .filter(|message| matches!(message, Message::Request(r) if r.command == command))
            .count()
            >= count
    });
    requests(fake, command)
}

/// Waits until `fake` has received a response to its request `command`.
pub(super) fn wait_client_response(fake: &FakeAdapter, command: &str) -> ResponseMessage {
    fake.wait_until(command, |received, _| {
        received
            .iter()
            .any(|message| matches!(message, Message::Response(r) if r.command == command))
    });
    fake.received()
        .into_iter()
        .find_map(|message| match message {
            Message::Response(response) if response.command == command => Some(response),
            _ => None,
        })
        .unwrap()
}

/// The commands `fake` received after the first `after`.
pub(super) fn commands_after(fake: &FakeAdapter, after: &str) -> Vec<String> {
    let commands = fake.commands();
    let at = commands
        .iter()
        .position(|command| command == after)
        .unwrap_or_else(|| panic!("no {after} in {commands:?}"));
    commands[at + 1..].to_vec()
}

/// Moves `clock` to just short of the timer that has `left` to go, checks
/// it hasn't fired, then moves it the rest of the way.
pub(super) fn advance_past(clock: &ManualClock, left: Duration) {
    let short = Duration::from_millis(1);
    clock.wait_for_sleep(left);
    clock.advance(left - short);
    assert!(
        clock.sleeping().contains(&short),
        "the {left:?} timer went early: {:?}",
        clock.sleeping()
    );
    clock.advance(short);
}

/// The default script, with a breakpoint's program.
pub(super) fn script() -> Script {
    Script::default()
}

/// A launcher handing out one prepared [`crate::launcher::AdapterIo`]: an
/// adapter made by hand, for what the fake can't do.
pub(super) struct IoLauncher(std::sync::Mutex<Option<crate::launcher::AdapterIo>>);

impl IoLauncher {
    pub(super) fn new(io: crate::launcher::AdapterIo) -> Arc<Self> {
        Arc::new(Self(std::sync::Mutex::new(Some(io))))
    }
}

impl crate::launcher::AdapterLauncher for IoLauncher {
    fn launch(
        &self,
        _: &AdapterSpec,
        _: &std::path::Path,
        _: &crate::launcher::LaunchCancel,
    ) -> std::io::Result<crate::launcher::AdapterIo> {
        self.0
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| std::io::Error::other("launched once"))
    }
}

/// A process that runs until killed.
pub(super) struct TestProcess {
    kill: Option<futures::channel::oneshot::Sender<()>>,
    gone: futures::future::Shared<futures::channel::oneshot::Receiver<()>>,
    killed: Arc<std::sync::atomic::AtomicBool>,
}

impl TestProcess {
    /// The process, and whether it has been killed.
    pub(super) fn new() -> (Box<Self>, Arc<std::sync::atomic::AtomicBool>) {
        use futures::FutureExt as _;
        let (kill, gone) = futures::channel::oneshot::channel();
        let killed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let process = Box::new(Self {
            kill: Some(kill),
            gone: gone.shared(),
            killed: killed.clone(),
        });
        (process, killed)
    }
}

impl crate::launcher::AdapterProcess for TestProcess {
    fn id(&self) -> Option<u32> {
        None
    }

    fn exited(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::launcher::ExitInfo> + Send + '_>>
    {
        let gone = self.gone.clone();
        Box::pin(async move {
            let _ = gone.await;
            crate::launcher::ExitInfo {
                code: None,
                signal: Some(9),
            }
        })
    }

    fn kill(&mut self) {
        self.killed.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(kill) = self.kill.take() {
            let _ = kill.send(());
        }
    }
}

/// `message`, framed.
pub(super) fn frame(message: &Message) -> Vec<u8> {
    let body = serde_json::to_vec(message).unwrap();
    let mut framed = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    framed.extend_from_slice(&body);
    framed
}

/// A stream that always has more: `first`, then `again` over and over.
pub(super) struct Endless {
    pending: Vec<u8>,
    again: Vec<u8>,
    at: usize,
}

impl Endless {
    pub(super) fn new(first: Vec<u8>, again: Vec<u8>) -> Self {
        Self {
            pending: first,
            again,
            at: 0,
        }
    }
}

impl futures::AsyncRead for Endless {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        out: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let this = &mut *self;
        let mut written = 0;
        while written < out.len() {
            if this.at == this.pending.len() {
                this.pending = this.again.clone();
                this.at = 0;
            }
            let n = (out.len() - written).min(this.pending.len() - this.at);
            out[written..written + n].copy_from_slice(&this.pending[this.at..this.at + n]);
            written += n;
            this.at += n;
        }
        std::task::Poll::Ready(Ok(written))
    }
}

/// A successful `initialize` response to the session's first request,
/// with `capabilities`.
pub(super) fn initialize_response(capabilities: Value) -> Message {
    Message::Response(ResponseMessage {
        seq: 1,
        request_seq: 1,
        success: true,
        command: "initialize".into(),
        body: Some(capabilities),
        ..Default::default()
    })
}

/// A session on hand-made `io`, its events forwarded but its output only
/// counted (an endless adapter sends endless output).
pub(super) fn start_on(io: crate::launcher::AdapterIo, options: SessionOptions) -> Harness {
    let launcher = Arc::new(FakeLauncher::new(Script::default()));
    let clock = Arc::new(ManualClock::new());
    let (sender, receiver) = channel();
    let session = Session::start(options, IoLauncher::new(io), clock.clone(), move |event| {
        if !matches!(event, DapEvent::Output(_)) {
            let _ = sender.send(event);
        }
    })
    .expect("the session thread starts");
    Harness {
        session,
        launcher,
        clock,
        receiver,
        events: Vec::new(),
    }
}

/// Reads the next request the session wrote to `sent`, by command.
/// Panics after 10 s.
pub(super) fn next_command(
    sent: &mut crate::transport::FrameReader<crate::fake::PipeReader>,
) -> String {
    use futures::future::Either;
    async_io::block_on(async {
        let timeout = async_io::Timer::after(Duration::from_secs(10));
        match futures::future::select(std::pin::pin!(sent.read()), timeout).await {
            Either::Left((Ok(Some(Message::Request(request))), _)) => request.command,
            Either::Left((other, _)) => panic!("not a request: {other:?}"),
            Either::Right(_) => panic!("the session wrote nothing in 10 s"),
        }
    })
}

/// A stream of separate reads: each chunk alone, a `Pending` (woken at
/// once) between two, and then, when `open`, `Pending` for ever instead of
/// the end.
pub(super) struct Reads {
    chunks: std::collections::VecDeque<Vec<u8>>,
    paused: bool,
    open: bool,
}

impl Reads {
    pub(super) fn new(chunks: Vec<Vec<u8>>, open: bool) -> Self {
        Self {
            chunks: chunks.into(),
            paused: false,
            open,
        }
    }
}

impl futures::AsyncRead for Reads {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        out: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let this = &mut *self;
        if !this.paused {
            this.paused = true;
            cx.waker().wake_by_ref();
            return std::task::Poll::Pending;
        }
        let Some(mut chunk) = this.chunks.pop_front() else {
            return if this.open {
                std::task::Poll::Pending
            } else {
                std::task::Poll::Ready(Ok(0))
            };
        };
        this.paused = false;
        let n = chunk.len().min(out.len());
        out[..n].copy_from_slice(&chunk[..n]);
        if n < chunk.len() {
            chunk.drain(..n);
            this.chunks.push_front(chunk);
            this.paused = true;
        }
        std::task::Poll::Ready(Ok(n))
    }
}

/// A process that has already exited.
pub(super) fn exited_process() -> Box<TestProcess> {
    use crate::launcher::AdapterProcess as _;
    let (mut process, _) = TestProcess::new();
    process.kill();
    process
}
