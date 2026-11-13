//! Running a program for a run in the workbench's [`Runs`]: a test or build
//! whose output a parser reads as it arrives. A task in a terminal tab
//! ([`crate::task_run`]) is a pty, which gives colour and prompts; this gives
//! plain lines, through pipes.
//!
//! [`RunProcess::start`] starts the program (its argv, folder and
//! environment) only in a trusted folder, on a thread of its own, leading a
//! process group of its own ([`crate::process_group`]) with stdin closed and
//! stdout and stderr piped. Its lines go to a [`LineSink`] there, off the
//! main thread, in batches of at most one per [`BATCH_INTERVAL`]; what the
//! sink makes of them ([`RunUpdate`]s) is applied to the run once a batch.
//! Both streams' lines are kept, interleaved as they were read, for the
//! Output view: up to [`OUTPUT_CAP`], past which the oldest go and
//! [`RunOutput::truncated`] is set.
//!
//! Nothing waits unbounded: the readers hand over a few lines at a time, a
//! batch takes in at most [`BATCH_CAP`], and a few batches wait for the main
//! thread. Past that the pipes fill and the program waits, so a slow sink or
//! main thread slows the program rather than piling up its output.
//!
//! The program's exit finishes its run: zero passed, anything else failed
//! with its code (none if a signal ended it). [`RunProcess::cancel`], or
//! dropping the [`RunProcess`], cancels the run at once and stops the group:
//! `SIGTERM`, then `SIGKILL` [`GRACE`] later. A thread that does only that,
//! started before the program is, sends both, so neither waits for the sink
//! or the main thread; with no such thread, nothing starts. A sink that
//! panics stops the program the same way, its run failed. Quitting the app
//! sends every group still running `SIGTERM`.
//!
//! The leader isn't reaped before its group is last signalled, so the
//! group's id can't pass to another process meanwhile.

use std::{
    collections::VecDeque,
    ffi::OsString,
    io::{BufRead, BufReader, Read},
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        mpsc::{self, RecvTimeoutError, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use gpui::{App, AppContext as _, Context, Entity, Subscription, Task, WeakEntity};
use kiln_l10n::{tr, verbatim};

use crate::{
    process_group::{self, GroupId},
    runs::{RunEnd, RunId, RunKind, RunSummary, Runs},
    task_run,
};

/// The least time between two batches of lines handed to a [`LineSink`].
pub const BATCH_INTERVAL: Duration = Duration::from_millis(16);

/// How much output a run keeps, counting each line's bytes and its newline.
pub const OUTPUT_CAP: usize = 8 * 1024 * 1024;

/// How long a cancelled run's group has between `SIGTERM` and `SIGKILL`.
pub const GRACE: Duration = Duration::from_secs(2);

/// The longest line read as one: a longer one comes as several.
pub const MAX_LINE: usize = 64 * 1024;

/// How much a batch takes in, each line counted as for [`OUTPUT_CAP`], before
/// the runner's thread stops reading until it's handed over: the pipes then
/// fill, and the program waits for the sink and the main thread. A batch may
/// pass it by one line.
pub const BATCH_CAP: usize = 512 * 1024;

/// How many lines the pipe readers may have read that the runner's thread
/// hasn't taken: past that they wait, and so does the program.
const LINES_IN_FLIGHT: usize = 64;

/// How often the runner's thread looks for a cancel and the program's exit.
const POLL: Duration = Duration::from_millis(5);

/// How many batches may wait for the main thread before the runner's thread
/// waits for it, and so the program for the runner. It looks for a cancel
/// as it waits.
const BATCHES_IN_FLIGHT: usize = 8;

/// Which pipe a line came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// What a [`LineSink`] says of its run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RunUpdate {
    /// How far along it is, from 0 to 1, if that's known.
    Progress(Option<f32>),
    /// What it's done so far.
    Summary(RunSummary),
}

/// What reads a run's lines: a test or build parser. It's called on the
/// runner's thread, never the main one.
pub trait LineSink: Send + 'static {
    /// One line from `stream`, with no line ending, read `at` this long
    /// after the program started. Bytes that aren't UTF-8 are replaced.
    fn line(&mut self, stream: Stream, text: &str, at: Duration) -> Vec<RunUpdate>;

    /// The end of a batch: every line read so far has been handed over.
    fn batch_end(&mut self) -> Vec<RunUpdate> {
        Vec::new()
    }
}

/// What to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSpec {
    pub kind: RunKind,
    /// The run's title, as [`Runs`] lists it.
    pub title: String,
    /// The program, looked up on the `PATH` it's given if it's a bare name.
    pub program: OsString,
    pub args: Vec<OsString>,
    /// Where it runs, which has to be trusted.
    pub cwd: PathBuf,
    /// Set over the workbench's own environment, with the login shell's
    /// `PATH` unless this has one.
    pub env: Vec<(OsString, OsString)>,
}

/// Why [`RunProcess::start`] started nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The folder isn't trusted, or couldn't be resolved to say: with what
    /// running a task there says ([`task_run::untrusted_message`]).
    Untrusted(String),
}

impl Refused {
    /// What to tell the user.
    pub fn message(&self) -> &str {
        match self {
            Self::Untrusted(message) => message,
        }
    }
}

/// One line of a run's output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputLine {
    pub stream: Stream,
    pub text: String,
    /// How long after the program started it was read.
    pub at: Duration,
}

impl OutputLine {
    /// What it counts for against [`OUTPUT_CAP`]: its bytes and a newline.
    fn size(&self) -> usize {
        self.text.len() + 1
    }
}

/// The output a run keeps: both streams' lines in the order they were read,
/// the newest [`OUTPUT_CAP`] of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunOutput {
    lines: VecDeque<OutputLine>,
    size: usize,
    truncated: bool,
}

impl RunOutput {
    /// The lines kept, oldest first.
    pub fn lines(&self) -> impl ExactSizeIterator<Item = &OutputLine> {
        self.lines.iter()
    }

    /// Whether older lines were dropped to keep under [`OUTPUT_CAP`].
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// The size of the lines kept, each with a newline.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Adds `batch`'s lines, which follow these: if it dropped some of its
    /// own to fit the cap, these are older still, so they all go too.
    pub(crate) fn append(&mut self, batch: RunOutput) {
        if batch.truncated {
            *self = batch;
        } else {
            self.extend(batch.lines);
        }
    }

    /// Adds `lines`, dropping the oldest past [`OUTPUT_CAP`].
    pub(crate) fn extend(&mut self, lines: impl IntoIterator<Item = OutputLine>) {
        for line in lines {
            self.size += line.size();
            self.lines.push_back(line);
        }
        while self.size > OUTPUT_CAP {
            let Some(oldest) = self.lines.pop_front() else {
                break;
            };
            self.size -= oldest.size();
            self.truncated = true;
        }
    }
}

/// What the runner's thread tells the main thread.
enum Message {
    /// A batch's lines, those past the cap already dropped, and what the
    /// sink said of them.
    Batch {
        lines: RunOutput,
        updates: Vec<RunUpdate>,
    },
    /// The program couldn't start: why, as a line of output.
    CantRun(String),
    /// The run is over.
    Ended { end: RunEnd, code: Option<i32> },
}

/// Where a run's program is, as the main thread, the runner's thread and
/// the stopper's thread see it.
#[derive(Default)]
struct State {
    /// The group, while its leader isn't reaped: it keeps its id until then.
    group: Option<GroupId>,
    /// Nothing starts once this is set, and what's started is stopped.
    cancelled: bool,
    /// The program ended by itself, and was reaped, or never started: there's
    /// nothing to stop.
    finished: bool,
    /// The stopper is done: the group has had its `SIGKILL`, and its leader
    /// may be reaped.
    killed: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn wait<'a>(&self, state: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        (self.changed.wait(state)).unwrap_or_else(|poison| poison.into_inner())
    }

    fn cancelled(&self) -> bool {
        self.state().cancelled
    }

    /// Stops the program, or keeps it from starting.
    fn cancel(&self) {
        self.state().cancelled = true;
        self.changed.notify_all();
    }

    /// There's nothing to stop.
    fn finish(&self) {
        self.state().finished = true;
        self.changed.notify_all();
    }

    /// As the app quits: `SIGTERM` now, as the stopper's thread may not get
    /// the chance, and nothing starts after this.
    fn quit(&self) {
        let mut state = self.state();
        state.cancelled = true;
        if let Some(group) = state.group {
            // Not reaped while it's here and the lock is held: it keeps its id.
            process_group::terminate(group);
        }
        drop(state);
        self.changed.notify_all();
    }
}

/// The stopper's thread: once the run is cancelled, `SIGTERM` to the group,
/// and `SIGKILL` [`GRACE`] later, whatever the other threads are doing.
fn stop_when_cancelled(shared: &Shared) {
    let mut state = shared.state();
    while !state.cancelled && !state.finished {
        state = shared.wait(state);
    }
    if !state.finished
        && let Some(group) = state.group
    {
        // The leader isn't reaped until `killed` is set, below.
        process_group::terminate(group);
        let deadline = Instant::now() + GRACE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            state = (shared.changed.wait_timeout(state, left))
                .map_or_else(|poison| poison.into_inner().0, |(state, _)| state);
        }
        if let Some(group) = state.group {
            process_group::kill(group);
        }
    }
    state.killed = true;
    drop(state);
    shared.changed.notify_all();
}

/// Starts a thread named `name` doing `work`. A test can make it fail.
fn spawn_thread(name: &str, work: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    #[cfg(test)]
    if THREADS_LEFT.with(|left| left.replace(left.get().map(|left| left.saturating_sub(1))))
        == Some(0)
    {
        return Err(std::io::ErrorKind::OutOfMemory.into());
    }
    thread::Builder::new().name(name.into()).spawn(work)?;
    Ok(())
}

#[cfg(test)]
thread_local! {
    /// How many more threads [`spawn_thread`] may start on this thread
    /// before it fails: any number if `None`.
    pub(crate) static THREADS_LEFT: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

/// A program running for a run, and the output it's kept.
pub struct RunProcess {
    run: RunId,
    runs: WeakEntity<Runs>,
    output: RunOutput,
    progress: Option<f32>,
    summary: RunSummary,
    /// Whether the sink gave a summary, which the exit then keeps.
    summarised: bool,
    end: Option<RunEnd>,
    shared: Arc<Shared>,
    _messages: Task<()>,
    /// Its stop as the app quits, and its run's end once it's released.
    _hooks: [Subscription; 2],
}

impl RunProcess {
    /// Starts `spec` for a new run in `runs`, its lines to `sink`, if its
    /// folder resolves to one that's trusted when it's checked, just before
    /// it starts; else nothing starts and no run begins.
    pub fn start(
        spec: RunSpec,
        sink: impl LineSink,
        runs: Entity<Runs>,
        cx: &mut App,
    ) -> Task<Result<Entity<RunProcess>, Refused>> {
        let cwd = spec.cwd.clone();
        cx.spawn(async move |cx| {
            let resolved = cx
                .background_spawn(async move { dunce::canonicalize(&cwd).ok() })
                .await;
            cx.update(|cx| {
                let trusted = resolved
                    .as_deref()
                    .is_some_and(|folder| kiln_shell::trust::decided(folder, cx) == Some(true));
                let Some(cwd) = resolved.filter(|_| trusted) else {
                    return Err(Refused::Untrusted(task_run::untrusted_message()));
                };
                let spec = RunSpec { cwd, ..spec };
                Ok(cx.new(|cx| Self::new(spec, Box::new(sink), runs, cx)))
            })
        })
    }

    fn new(
        spec: RunSpec,
        sink: Box<dyn LineSink>,
        runs: Entity<Runs>,
        cx: &mut Context<Self>,
    ) -> Self {
        let run = runs.update(cx, |runs, cx| runs.begin(spec.kind, spec.title.clone(), cx));
        let shared = Arc::new(Shared::default());
        let (sender, messages) = async_channel::bounded(BATCHES_IN_FLIGHT);
        let program = spec.program.to_string_lossy().into_owned();
        let stopper = {
            let shared = shared.clone();
            spawn_thread(verbatim!("run process stop"), move || {
                stop_when_cancelled(&shared)
            })
        };
        let runner = stopper.and_then(|()| {
            let (runner_shared, sender) = (shared.clone(), sender.clone());
            let started = spawn_thread(verbatim!("run process"), move || {
                run_to_end(spec, sink, runner_shared, &sender)
            });
            if started.is_err() {
                shared.finish();
            }
            started
        });
        if let Err(error) = runner {
            let error = error.to_string();
            let said = tr!(
                "workbench-workbench-filter-cant-run",
                program = program,
                error = error
            );
            // Room for both: nothing else is sent.
            let _ = sender.try_send(Message::CantRun(said.into()));
            let ended = Message::Ended {
                end: RunEnd::Failed { code: None },
                code: None,
            };
            let _ = sender.try_send(ended);
        }
        drop(sender);
        let messages = cx.spawn(async move |this, cx| {
            while let Ok(message) = messages.recv().await {
                if this.update(cx, |this, cx| this.apply(message, cx)).is_err() {
                    break;
                }
            }
        });
        let quit = cx.on_app_quit(|this, _| {
            this.shared.quit();
            async {}
        });
        // Its run is over once nobody holds it: the program's stopped below.
        let released = cx.on_release(|this, cx| {
            if this.end.is_none() {
                let run = this.run;
                let finish = |runs: &mut Runs, cx: &mut Context<Runs>| {
                    runs.finish(run, RunEnd::Cancelled, None, cx)
                };
                this.runs.update(cx, finish).ok();
            }
        });
        Self {
            run,
            runs: runs.downgrade(),
            output: RunOutput::default(),
            progress: None,
            summary: RunSummary::None,
            summarised: false,
            end: None,
            shared,
            _messages: messages,
            _hooks: [quit, released],
        }
    }

    /// Its run in the workbench's [`Runs`].
    pub fn run(&self) -> RunId {
        self.run
    }

    /// The output kept so far.
    pub fn output(&self) -> &RunOutput {
        &self.output
    }

    /// How the run ended, once it has.
    pub fn end(&self) -> Option<RunEnd> {
        self.end
    }

    /// Stops the program and everything it started: `SIGTERM` to its group,
    /// `SIGKILL` [`GRACE`] later. The run is cancelled now, unless it had
    /// ended already, whatever the program's end turns out to be.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        self.shared.cancel();
        if self.end.is_none() {
            self.end = Some(RunEnd::Cancelled);
            let run = self.run;
            (self.runs)
                .update(cx, |runs, cx| runs.finish(run, RunEnd::Cancelled, None, cx))
                .ok();
            cx.notify();
        }
    }

    fn apply(&mut self, message: Message, cx: &mut Context<Self>) {
        match message {
            // Cancelled already.
            Message::Ended { .. } if self.end.is_some() => {}
            Message::Batch { lines, updates } => {
                self.output.append(lines);
                if !updates.is_empty() {
                    for update in updates {
                        match update {
                            RunUpdate::Progress(progress) => self.progress = progress,
                            RunUpdate::Summary(summary) => {
                                self.summary = summary;
                                self.summarised = true;
                            }
                        }
                    }
                    let (run, progress, summary) = (self.run, self.progress, self.summary);
                    (self.runs)
                        .update(cx, |runs, cx| runs.update(run, progress, summary, cx))
                        .ok();
                }
            }
            Message::CantRun(said) => self.output.extend([OutputLine {
                stream: Stream::Stderr,
                text: said,
                at: Duration::ZERO,
            }]),
            Message::Ended { end, code } => {
                self.end = Some(end);
                let summary = match (self.summarised, code) {
                    (false, Some(code)) => Some(RunSummary::Exit(code)),
                    _ => None,
                };
                let run = self.run;
                (self.runs)
                    .update(cx, |runs, cx| runs.finish(run, end, summary, cx))
                    .ok();
            }
        }
        cx.notify();
    }
}

impl Drop for RunProcess {
    /// A program nobody holds any more is stopped, as a cancel stops it.
    fn drop(&mut self) {
        self.shared.cancel();
    }
}

/// Runs `spec` to its end, or until it's cancelled, on the runner's thread:
/// see the module docs.
fn run_to_end(
    spec: RunSpec,
    mut sink: Box<dyn LineSink>,
    shared: Arc<Shared>,
    sender: &async_channel::Sender<Message>,
) {
    let send = |message| sender.send_blocking(message).is_ok();
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process_group::lead_group(&mut command);
    if !spec.env.iter().any(|(name, _)| name == "PATH")
        && let Some(path) = fe_lsp::LoginPath::system().path()
    {
        command.env("PATH", path);
    }
    command.envs(spec.env.iter().map(|(name, value)| (name, value)));
    let started = Instant::now();
    let spawned = {
        let mut state = shared.state();
        if state.cancelled {
            None
        } else {
            let spawned = command.spawn();
            if let Ok(child) = &spawned {
                state.group = process_group::group_of(child);
            }
            Some(spawned)
        }
    };
    let mut child = match spawned {
        Some(Ok(child)) => child,
        Some(Err(error)) => {
            let program = spec.program.to_string_lossy().into_owned();
            let error = error.to_string();
            let said = tr!(
                "workbench-workbench-filter-cant-run",
                program = program,
                error = error
            );
            shared.finish();
            send(Message::CantRun(said.into()));
            send(Message::Ended {
                end: RunEnd::Failed { code: None },
                code: None,
            });
            return;
        }
        None => {
            shared.finish();
            send(Message::Ended {
                end: RunEnd::Cancelled,
                code: None,
            });
            return;
        }
    };
    let (lines, read) = mpsc::sync_channel(LINES_IN_FLIGHT);
    read_lines(child.stdout.take(), Stream::Stdout, started, lines.clone());
    read_lines(child.stderr.take(), Stream::Stderr, started, lines);
    let mut pending: Vec<OutputLine> = Vec::new();
    let mut pending_size = 0;
    let mut delivered: Option<Instant> = None;
    let mut read_all = false;
    let stopped = loop {
        if shared.cancelled() {
            break RunEnd::Cancelled;
        }
        if read_all || pending_size >= BATCH_CAP {
            // Nothing to read, or no room: the readers wait meanwhile.
            thread::sleep(POLL);
        } else {
            let mut next = read.recv_timeout(POLL);
            loop {
                match next {
                    Ok(line) => {
                        pending_size += line.size();
                        pending.push(line);
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => {
                        read_all = true;
                        break;
                    }
                }
                if pending_size >= BATCH_CAP {
                    break;
                }
                next = read.try_recv().map_err(|error| match error {
                    TryRecvError::Empty => RecvTimeoutError::Timeout,
                    TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
                });
            }
        }
        let over = read_all && process_group::exited(&child);
        let since = delivered.map(|at| at.elapsed());
        let due = since.is_none_or(|since| since >= BATCH_INTERVAL);
        if !pending.is_empty() && (due || over) {
            if let Some(since) = since.filter(|_| !due) {
                // The last batch keeps its distance from the one before.
                thread::sleep(BATCH_INTERVAL - since);
            }
            let lines = std::mem::take(&mut pending);
            pending_size = 0;
            let batch = panic::catch_unwind(AssertUnwindSafe(|| deliver(lines, sink.as_mut())));
            let Ok(batch) = batch else {
                log::error!("a run's line sink panicked: its program is stopped");
                shared.cancel();
                break RunEnd::Failed { code: None };
            };
            delivered = Some(Instant::now());
            if !publish(sender, batch, &shared) {
                // Cancelled while the main thread was behind, or nobody's
                // listening: the run's gone.
                shared.cancel();
                break RunEnd::Cancelled;
            }
        }
        if over && pending.is_empty() {
            let mut state = shared.state();
            if state.cancelled {
                // The stopper has the group: it may have members left.
                break RunEnd::Cancelled;
            }
            state.group = None;
            state.finished = true;
            drop(state);
            shared.changed.notify_all();
            let (end, code) = match child.wait() {
                Ok(status) => match status.code() {
                    Some(0) => (RunEnd::Passed, Some(0)),
                    code => (RunEnd::Failed { code }, code),
                },
                Err(_) => (RunEnd::Failed { code: None }, None),
            };
            send(Message::Ended { end, code });
            return;
        }
    };
    // What's still to read is nobody's now: the readers stop at their next
    // line.
    drop((read, pending));
    // The leader is reaped once the stopper has killed its group.
    let mut state = shared.state();
    while !state.killed {
        state = shared.wait(state);
    }
    state.group = None;
    drop(state);
    if process_group::group_of(&child).is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    send(Message::Ended {
        end: stopped,
        code: None,
    });
}

/// Sends `message` to the main thread, waiting while it's behind, unless the
/// run is cancelled meanwhile: false then, or if nobody's listening.
fn publish(sender: &async_channel::Sender<Message>, message: Message, shared: &Shared) -> bool {
    let mut message = message;
    loop {
        match sender.try_send(message) {
            Ok(()) => return true,
            Err(async_channel::TrySendError::Closed(_)) => return false,
            Err(async_channel::TrySendError::Full(back)) => {
                if shared.cancelled() {
                    return false;
                }
                message = back;
                thread::sleep(POLL);
            }
        }
    }
}

/// Hands `lines` to `sink` and keeps the newest of them that fit the cap.
fn deliver(lines: Vec<OutputLine>, sink: &mut dyn LineSink) -> Message {
    let mut updates = Vec::new();
    for line in &lines {
        updates.extend(sink.line(line.stream, &line.text, line.at));
    }
    updates.extend(sink.batch_end());
    let mut kept = RunOutput::default();
    kept.extend(lines);
    Message::Batch {
        lines: kept,
        updates,
    }
}

/// Reads `pipe`'s lines on a thread of its own, each to `lines` with when it
/// was read, until the pipe closes or nobody's listening.
fn read_lines(
    pipe: Option<impl Read + Send + 'static>,
    stream: Stream,
    started: Instant,
    lines: mpsc::SyncSender<OutputLine>,
) {
    let Some(pipe) = pipe else {
        return;
    };
    let read = move || {
        let mut reader = BufReader::new(pipe);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match (&mut reader)
                .take(MAX_LINE as u64)
                .read_until(b'\n', &mut bytes)
            {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let at = started.elapsed();
            if bytes.last() == Some(&b'\n') {
                bytes.pop();
                if bytes.last() == Some(&b'\r') {
                    bytes.pop();
                }
            }
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if lines.send(OutputLine { stream, text, at }).is_err() {
                return;
            }
        }
    };
    let spawned = thread::Builder::new()
        .name(verbatim!("run output").into())
        .spawn(read);
    if let Err(error) = spawned {
        log::warn!("couldn't read a run's {stream:?}: {error}");
    }
}
