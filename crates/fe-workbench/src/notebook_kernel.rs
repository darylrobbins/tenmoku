//! A persistent Python process per notebook file, which runs the file's
//! `# %%` cells ([`crate::notebook_cells`]) one at a time in one namespace,
//! as a Jupyter kernel does. Headless: no GPUI here. [`Kernel`] starts the
//! process and threads of its own; what the kernel says comes as
//! [`KernelEvent`]s on a channel the caller polls ([`Kernel::events`]).
//!
//! # Our own kernel, not Jupyter's
//!
//! The kernel is `notebook_kernel.py`, a small script of the standard
//! library's only, embedded here ([`SCRIPT`]), written to the state
//! directory ([`write_script`]) and run as `<python> -u <script> <config>`
//! with the project's interpreter ([`interpreter`]). Not ipykernel through
//! jupyter_client: neither is in most project environments (a plain
//! `python -m venv` has neither), jupyter_client needs ZeroMQ and a
//! connection-file handshake, and running a cell would fail with "install
//! ipykernel" on a fresh project. The script works on any Python 3.8 or
//! later with nothing installed. What that gives up: Jupyter widgets,
//! completion and inspection, and rich `display_data` beyond what we add.
//! The messages mirror Jupyter's names (`execute_request`, `stream`,
//! `execute_result`, `error`, `display_data`, `execute_reply`), so an
//! ipykernel-backed transport can be added later behind the same
//! [`KernelEvent`]s without touching the UI.
//!
//! # Protocol
//!
//! JSON, one object per line, UTF-8, over the kernel's stdin and stdout.
//!
//! | Direction | Line | Jupyter's |
//! |---|---|---|
//! | to the kernel | `{"op":"exec","id":N,"code":"...","file":"/abs/x.py","line":12}` | `execute_request` |
//! | from it, once at start | `{"kind":"ready","python":"3.12.1"}` | `kernel_info_reply` |
//! | from it, as a cell starts | `{"id":N,"kind":"busy"}` | `status` (`busy`) |
//! | from it | `{"id":N,"kind":"stream","name":"stdout"\|"stderr","text":"..."}` | `stream` |
//! | from it | `{"id":N,"kind":"result","text":"repr"}` | `execute_result` |
//! | from it | `{"id":N,"kind":"error","ename":"ValueError","evalue":"...","traceback":["File \"/abs/x.py\", line 14, in <module>", ...]}` | `error` |
//! | from it | `{"id":N,"kind":"done","ok":true\|false}` | `execute_reply` |
//!
//! A `stream` written between cells, by a thread a cell started, has a null
//! `id` ([`KernelEvent::Stream`]'s `cell` is `None`). `display_data` is the
//! name images will use (a later card).
//!
//! # The kernel's side
//!
//! The code runs in one persistent namespace, a module named `__main__`
//! with `__file__` the file and `sys.path[0]` the file's folder. It's
//! compiled with the file's name and its line numbers shifted by `line`, so
//! a traceback reads like the file; the source of every cell run is kept
//! for the traceback's lines, since the file on disk may not hold what ran.
//! The kernel's own frames are dropped from tracebacks. If the last
//! statement is an expression, its value's `repr` is the `result`, unless
//! it's `None`, as a REPL shows it.
//!
//! `sys.stdout` and `sys.stderr` become writers that send `stream`s,
//! batched at most every 50 ms or 4 KiB; file descriptors 1 and 2 write
//! pipes the kernel reads, so a subprocess's output comes back the same way
//! (and, on Unix, what a cell's subprocesses wrote before it ended is sent
//! before its `done`). The protocol uses the original stdin and stdout,
//! duplicated at startup; fd 0 reads `/dev/null`, so nothing a cell starts
//! can read the protocol, and `input()` raises `EOFError` saying there's no
//! stdin (no stdin UI yet).
//!
//! # Caps
//!
//! A cell run's stream text past [`OUTPUT_CAP`] is dropped, and a last
//! `stream` says how many bytes were; a line longer than [`MAX_LINE`]
//! characters is split into lines of that length. A `result`'s text and an
//! error's value and traceback entries are cut at [`OUTPUT_CAP`] characters.
//! A line from the kernel that isn't one of the messages above, or is longer
//! than [`MAX_MESSAGE`], ends the kernel ([`KernelEvent::Died`]).
//!
//! # Cells, one at a time
//!
//! [`Kernel::execute`] sends a cell to the kernel if none is running, else
//! queues it: first in, first out, at most [`QUEUE_CAP`] waiting, past which
//! a cell is refused ([`Failure::QueueFull`]). Run Below can so queue every
//! cell. A queued cell can be cancelled ([`Kernel::cancel_queued`]).
//! [`Kernel::interrupt`] sends `SIGINT` to the kernel's process group, which
//! raises `KeyboardInterrupt` in the running cell, an `error` like any
//! other; the queue is kept unless it's flushed. The kernel sends `busy`
//! once a cell is its to interrupt (its handler raises only then, and never
//! mid-way through a protocol line), so an interrupt of a cell whose `busy`
//! hasn't come, the kernel perhaps not even up, is held until it does: it
//! reaches that cell, and never the next. On Windows our spawn gives
//! no console to send `CTRL_BREAK_EVENT` to, so interrupt isn't supported
//! there and the UI offers Restart.
//!
//! # Its process
//!
//! [`Kernel::start`] runs the interpreter in a folder only if it's trusted
//! ([`TrustedFolder`], with `kiln_shell::trust::decided`), as a run or a
//! task does, leading a process group of its own
//! ([`crate::process_group`]). Four threads serve it: one writes its stdin,
//! one reads its stdout, one keeps the last [`LAST_WORDS`] bytes of its
//! stderr (the script's own last words, should it fail), and one watches for
//! its exit or a stop and then stops the group: `SIGTERM`, then `SIGKILL`
//! [`GRACE`] later for a stop, `SIGKILL` at once for a death. The leader is
//! reaped only after the group's last signal, so its id can't pass to
//! another process meanwhile.
//!
//! A kernel that exits by itself, says something it mustn't, or closes its
//! stdout without exiting within [`CLOSED_GRACE`], delivers
//! [`KernelEvent::Died`], once, and fails its running and queued cells
//! ([`Failure::Died`]). The [`Kernel`] is then dead: the caller starts a new
//! one. Restart is [`Kernel::shutdown`] and [`Kernel::start`]: the
//! namespace, and everything the cells defined, is gone. Shutdown, or
//! dropping the [`Kernel`], stops the group, and fails the cells left with
//! [`Failure::ShutDown`]; shutdown waits for the group to be stopped, a drop
//! doesn't.

use std::{
    collections::VecDeque,
    ffi::OsString,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio},
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use kiln_l10n::verbatim;
use serde::{Deserialize, Serialize};

use crate::{
    process_group::{self, GroupId},
    run_process::Refused,
    task_run,
};

/// The kernel script, as [`write_script`] writes it.
pub const SCRIPT: &str = include_str!("notebook_kernel.py");

/// The most stream text a cell run sends, in bytes: past it the rest is
/// dropped and counted.
pub const OUTPUT_CAP: usize = 1024 * 1024;

/// The longest line of stream text, in characters: a longer one is split.
pub const MAX_LINE: usize = 64 * 1024;

/// The longest line the kernel may send, in bytes: a longer one ends it.
pub const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// How many cells may wait while one runs.
pub const QUEUE_CAP: usize = 256;

/// How long a stopped kernel's group has between `SIGTERM` and `SIGKILL`.
pub const GRACE: Duration = Duration::from_secs(2);

/// How much of the kernel's stderr is kept for [`KernelEvent::Died`].
pub const LAST_WORDS: usize = 64 * 1024;

/// How often the watcher looks for the kernel's exit.
const POLL: Duration = Duration::from_millis(20);

/// How long the watcher waits, once the group is killed, for the readers
/// to reach the end of its output: a process that left the group could
/// keep the pipes open.
const READERS_GRACE: Duration = Duration::from_secs(2);

/// How long a kernel whose stdout has closed has to exit before it's
/// counted dead for that.
pub const CLOSED_GRACE: Duration = Duration::from_secs(1);

/// The kernel script's file name before its hash.
const SCRIPT_STEM: &str = "notebook_kernel-";

/// A cell's id, the caller's.
pub type CellId = u64;

/// Which of the cell's streams text was written to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamName {
    Stdout,
    Stderr,
}

/// Why a cell didn't run, or didn't finish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// [`QUEUE_CAP`] cells were waiting already.
    QueueFull,
    /// Taken off the queue: [`Kernel::cancel_queued`], or a flushing
    /// [`Kernel::interrupt`].
    Cancelled,
    /// The kernel died while it ran or waited, or before it was asked.
    Died,
    /// The kernel was shut down, or dropped, while it ran or waited, or
    /// before it was asked.
    ShutDown,
}

/// Why a kernel died.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathCause {
    /// It exited, or was killed, by itself.
    Exited,
    /// It sent a line that isn't a message, or one out of turn.
    BadMessage,
    /// It sent a line longer than [`MAX_MESSAGE`].
    MessageTooLong,
    /// Its stdout closed, or couldn't be read, and it didn't exit within
    /// [`CLOSED_GRACE`].
    OutputClosed,
}

/// What a kernel says, and what becomes of the cells given it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KernelEvent {
    /// It has started, on this Python version.
    Ready { python: String },
    /// The cell has started to run: from here an interrupt reaches it.
    Started { cell: CellId },
    /// Text a cell wrote; `None` for text written between cells.
    Stream {
        cell: Option<CellId>,
        name: StreamName,
        text: String,
    },
    /// The `repr` of the cell's last expression's value.
    ExecuteResult { cell: CellId, text: String },
    /// The exception that ended the cell: its type's name, its message and
    /// its traceback, the kernel's frames dropped, one entry per frame with
    /// the heading and the exception's lines.
    Error {
        cell: CellId,
        ename: String,
        evalue: String,
        traceback: Vec<String>,
    },
    /// The cell ran to its end, `ok` unless an exception ended it.
    Done { cell: CellId, ok: bool },
    /// The cell won't run, or won't finish: no `Done` follows.
    Failed { cell: CellId, why: Failure },
    /// The kernel exited unexpectedly: with this status, if it could be
    /// read, and the end of what it wrote to its stderr. Its running and
    /// queued cells fail next.
    Died {
        status: Option<ExitStatus>,
        cause: DeathCause,
        last_words: String,
    },
}

/// A folder resolved and found trusted, which a kernel may run in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedFolder(PathBuf);

impl TrustedFolder {
    /// `folder`, resolved to its canonical path, if `decided` says that's
    /// trusted: `|folder| kiln_shell::trust::decided(folder, cx)`. Else
    /// what running a task there says. It reads the file system: call it
    /// off the main thread, just before starting the kernel.
    pub fn check(
        folder: &Path,
        decided: impl FnOnce(&Path) -> Option<bool>,
    ) -> Result<Self, Refused> {
        match dunce::canonicalize(folder) {
            Ok(folder) if decided(&folder) == Some(true) => Ok(Self(folder)),
            _ => Err(Refused::Untrusted(task_run::untrusted_message())),
        }
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// What to start a kernel with.
#[derive(Clone, Debug)]
pub struct KernelSpec {
    /// The interpreter ([`interpreter`]), a bare name looked up on the
    /// login shell's `PATH`.
    pub python: PathBuf,
    /// Where it runs: the project's root.
    pub cwd: TrustedFolder,
    /// The notebook's file, absolute: the namespace's `__file__`.
    pub file: PathBuf,
    /// The kernel script, as [`write_script`] wrote it.
    pub script: PathBuf,
    /// Set over the workbench's environment, with the login shell's `PATH`
    /// unless this has one.
    pub env: Vec<(OsString, OsString)>,
}

/// The project's Python, as its language server finds it: the
/// `lsp.python_environment` setting's environment, `$VIRTUAL_ENV`,
/// `<root>/.venv` or `<root>/venv` ([`fe_lsp::find_python_env`]), else
/// [`fe_dap::registry::python_interpreter`]'s answer, as the Python tasks
/// have it: `python3`. It reads the file system.
pub fn interpreter(root: &Path, python_environment: Option<&str>) -> PathBuf {
    let home = std::env::var_os(verbatim!("HOME"))
        .or_else(|| {
            cfg!(windows)
                .then(|| std::env::var_os(verbatim!("USERPROFILE")))
                .flatten()
        })
        .map(PathBuf::from);
    let virtual_env = std::env::var_os(verbatim!("VIRTUAL_ENV"));
    match fe_lsp::find_python_env(
        python_environment.map(Path::new),
        virtual_env.as_deref(),
        root,
        home.as_deref(),
    ) {
        Some(env) => env.python(),
        None => fe_dap::registry::python_interpreter(root, None, Path::is_file),
    }
}

/// Where the kernel script goes by default: `<state_dir>/notebook`.
pub fn script_dir() -> Option<PathBuf> {
    fe_config::paths::state_dir().map(|dir| dir.join(verbatim!("notebook")))
}

/// Writes [`SCRIPT`] into `dir`, under a name its content decides, unless
/// it's there already: where it is. Two writers at once each write a file
/// of their own and rename it into place. It reads and writes the file
/// system.
pub fn write_script(dir: &Path) -> io::Result<PathBuf> {
    let hash = SCRIPT
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    let path = dir.join([SCRIPT_STEM, &format!("{hash:016x}"), ".py"].concat());
    if std::fs::read(&path).is_ok_and(|text| text == SCRIPT.as_bytes()) {
        return Ok(path);
    }
    std::fs::create_dir_all(dir)?;
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let write = WRITES.fetch_add(1, Ordering::Relaxed);
    let temp = path.with_extension(format!("{}-{write}", std::process::id()));
    std::fs::write(&temp, SCRIPT)?;
    std::fs::rename(&temp, &path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })?;
    Ok(path)
}

/// What the kernel's script is told on its command line.
#[derive(Serialize)]
struct Config<'a> {
    file: &'a str,
    output_cap: usize,
    max_line: usize,
}

/// A cell, as the kernel's stdin takes it.
#[derive(Serialize)]
struct Exec<'a> {
    op: &'static str,
    id: CellId,
    code: &'a str,
    file: &'a str,
    line: u32,
}

/// A line from the kernel.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Message {
    Ready {
        python: String,
    },
    Busy {
        id: CellId,
    },
    Stream {
        id: Option<CellId>,
        name: StreamName,
        text: String,
    },
    Result {
        id: CellId,
        text: String,
    },
    Error {
        id: CellId,
        ename: String,
        evalue: String,
        traceback: Vec<String>,
    },
    Done {
        id: CellId,
        ok: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Running cells.
    Alive,
    /// Stopping, or dying: cells still queue, and fail once it's dead.
    Ending,
    /// Its group is stopped and its leader reaped: the cells given it fail
    /// so.
    Dead(Failure),
}

/// The kernel as the caller's thread and the kernel's threads see it.
struct State {
    phase: Phase,
    /// The kernel's stdin, through its writer's thread: `None` once it's
    /// stopping.
    stdin: Option<mpsc::Sender<Vec<u8>>>,
    running: Option<CellId>,
    queue: VecDeque<(CellId, Vec<u8>)>,
    /// The group, while its leader isn't reaped: it keeps its id until then.
    group: Option<GroupId>,
    /// Asked to stop: shutdown, or a drop.
    stop: bool,
    /// What the reader found that ends the kernel.
    bad: Option<DeathCause>,
    /// When the kernel's stdout closed, if it has.
    closed_at: Option<Instant>,
    /// Whether the running cell's `busy` has come: from then on an
    /// interrupt reaches it.
    started: bool,
    /// An interrupt for the running cell, held until its `busy`.
    interrupt_held: bool,
    /// The readers of stdout and stderr still reading.
    readers: usize,
    /// The end of the kernel's stderr.
    last_words: VecDeque<u8>,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    events: async_channel::Sender<KernelEvent>,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn wait_timeout<'a>(
        &self,
        state: MutexGuard<'a, State>,
        timeout: Duration,
    ) -> MutexGuard<'a, State> {
        (self.changed.wait_timeout(state, timeout))
            .map_or_else(|poison| poison.into_inner().0, |(state, _)| state)
    }

    /// Sends `event`, with the state locked so events keep the order of
    /// what they report. The channel is unbounded: this never waits.
    fn emit(&self, _state: &State, event: KernelEvent) {
        let _ = self.events.try_send(event);
    }

    /// Sends the first queued cell to the kernel, if none is running and
    /// it's alive.
    fn dispatch(&self, state: &mut State) {
        if state.running.is_some() || state.phase != Phase::Alive {
            return;
        }
        let Some((cell, line)) = state.queue.pop_front() else {
            return;
        };
        state.running = Some(cell);
        state.started = false;
        state.interrupt_held = false;
        if let Some(stdin) = &state.stdin {
            // A writer that's gone has a dead kernel: the watcher fails it.
            let _ = stdin.send(line);
        }
    }

    /// Takes in `message`, the kernel's: false if it's out of turn.
    fn receive(&self, message: Message) -> bool {
        let mut state = self.state();
        if matches!(state.phase, Phase::Dead(_)) {
            return false;
        }
        let event = match message {
            Message::Ready { python } => KernelEvent::Ready { python },
            Message::Stream { id, name, text } => KernelEvent::Stream {
                cell: id,
                name,
                text,
            },
            Message::Busy { id } if state.running == Some(id) && !state.started => {
                state.started = true;
                if std::mem::take(&mut state.interrupt_held) {
                    interrupt_group(&state);
                }
                KernelEvent::Started { cell: id }
            }
            Message::Result { id, text } if state.running == Some(id) => {
                KernelEvent::ExecuteResult { cell: id, text }
            }
            Message::Error {
                id,
                ename,
                evalue,
                traceback,
            } if state.running == Some(id) => KernelEvent::Error {
                cell: id,
                ename,
                evalue,
                traceback,
            },
            Message::Done { id, ok } if state.running == Some(id) => {
                self.emit(&state, KernelEvent::Done { cell: id, ok });
                state.running = None;
                self.dispatch(&mut state);
                return true;
            }
            Message::Busy { .. }
            | Message::Result { .. }
            | Message::Error { .. }
            | Message::Done { .. } => {
                return false;
            }
        };
        self.emit(&state, event);
        true
    }

    /// The reader found something that ends the kernel.
    fn bad(&self, cause: DeathCause) {
        let mut state = self.state();
        state.bad.get_or_insert(cause);
        drop(state);
        self.changed.notify_all();
    }

    /// The kernel's stdout has closed, or can't be read: its exit, if it
    /// comes within [`CLOSED_GRACE`], says why; else it's
    /// [`DeathCause::OutputClosed`].
    fn output_closed(&self) {
        self.state().closed_at.get_or_insert_with(Instant::now);
        self.changed.notify_all();
    }

    fn reader_done(&self) {
        self.state().readers -= 1;
        self.changed.notify_all();
    }
}

/// A Python process running one file's cells: see the module docs.
pub struct Kernel {
    shared: Arc<Shared>,
    events: async_channel::Receiver<KernelEvent>,
    pid: u32,
    watcher: Option<thread::JoinHandle<()>>,
}

impl Kernel {
    /// Starts `spec`'s interpreter running the kernel script, in its
    /// trusted folder. An error if it couldn't start, or a thread to serve
    /// it couldn't, which leaves nothing running. It doesn't wait for the
    /// kernel: [`KernelEvent::Ready`] says it's up, and cells given it
    /// before then wait for it.
    pub fn start(spec: KernelSpec) -> io::Result<Self> {
        let file = spec.file.to_string_lossy();
        let config = serde_json::to_string(&Config {
            file: &file,
            output_cap: OUTPUT_CAP,
            max_line: MAX_LINE,
        })?;
        let mut command = Command::new(&spec.python);
        command
            .arg(verbatim!("-u"))
            .arg(&spec.script)
            .arg(config)
            .current_dir(spec.cwd.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        process_group::lead_group(&mut command);
        if !spec.env.iter().any(|(name, _)| name == "PATH")
            && let Some(path) = fe_lsp::LoginPath::system().path()
        {
            command.env("PATH", path);
        }
        command.envs(spec.env.iter().map(|(name, value)| (name, value)));
        let mut child = command.spawn()?;
        let (stdin, stdout, stderr) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let (Some(stdin), Some(stdout), Some(stderr)) = (stdin, stdout, stderr) else {
            stop_now(&mut child);
            return Err(io::ErrorKind::BrokenPipe.into());
        };
        let pid = child.id();
        let (lines, to_write) = mpsc::channel::<Vec<u8>>();
        let (events, receiver) = async_channel::unbounded();
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                phase: Phase::Alive,
                stdin: Some(lines),
                running: None,
                queue: VecDeque::new(),
                group: process_group::group_of(&child),
                stop: false,
                bad: None,
                closed_at: None,
                started: false,
                interrupt_held: false,
                readers: 2,
                last_words: VecDeque::new(),
            }),
            changed: Condvar::new(),
            events,
        });
        // The watcher first: once it runs, the group is stopped whatever
        // else fails.
        let watcher = {
            let shared = shared.clone();
            thread::Builder::new()
                .name(verbatim!("notebook kernel watch").into())
                .spawn(move || watch(child, &shared))
        };
        let watcher = match watcher {
            Ok(watcher) => watcher,
            Err(error) => {
                // The child went with the closure: the group's id is all
                // that's left, and its leader isn't reaped.
                if let Some(group) = shared.state().group {
                    process_group::kill(group);
                }
                return Err(error);
            }
        };
        let kernel = Self {
            shared: shared.clone(),
            events: receiver,
            pid,
            watcher: Some(watcher),
        };
        let writer = thread::Builder::new()
            .name(verbatim!("notebook kernel stdin").into())
            .spawn(move || write_lines(stdin, &to_write));
        let reader = writer.and_then(|_| {
            let shared = shared.clone();
            thread::Builder::new()
                .name(verbatim!("notebook kernel stdout").into())
                .spawn(move || read_messages(stdout, &shared))
        });
        let errors = reader.and_then(|_| {
            let shared = shared.clone();
            thread::Builder::new()
                .name(verbatim!("notebook kernel stderr").into())
                .spawn(move || read_last_words(stderr, &shared))
        });
        if let Err(error) = errors {
            // The kernel, dropped, stops the group.
            let mut kernel = kernel;
            kernel.shutdown();
            return Err(error);
        }
        Ok(kernel)
    }

    /// What the kernel says: every clone sees each event once, so take them
    /// from one.
    pub fn events(&self) -> async_channel::Receiver<KernelEvent> {
        self.events.clone()
    }

    /// The kernel process's id, which is its group's on Unix.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Whether it still runs cells: false once it's died or been shut down.
    pub fn is_alive(&self) -> bool {
        self.shared.state().phase == Phase::Alive
    }

    /// Runs cell `cell`'s `code`, whose first row is line `first_line` of
    /// `file`, now if no cell is running, else once those queued before it
    /// have run. [`KernelEvent::Failed`] if [`QUEUE_CAP`] are waiting, or
    /// the kernel is dead.
    pub fn execute(&self, cell: CellId, code: &str, file: &Path, first_line: u32) {
        let file = file.to_string_lossy();
        let exec = Exec {
            op: verbatim!("exec"),
            id: cell,
            code,
            file: &file,
            line: first_line,
        };
        let mut line = serde_json::to_vec(&exec).unwrap_or_default();
        line.push(b'\n');
        let mut state = self.shared.state();
        let why = match state.phase {
            Phase::Dead(why) => Some(why),
            _ if state.queue.len() >= QUEUE_CAP => Some(Failure::QueueFull),
            _ => None,
        };
        if let Some(why) = why {
            self.shared.emit(&state, KernelEvent::Failed { cell, why });
            return;
        }
        state.queue.push_back((cell, line));
        self.shared.dispatch(&mut state);
    }

    /// Takes `cell` off the queue, if it's there: true if it was, and then
    /// it fails as [`Failure::Cancelled`].
    pub fn cancel_queued(&self, cell: CellId) -> bool {
        let mut state = self.shared.state();
        let Some(ix) = state.queue.iter().position(|(queued, _)| *queued == cell) else {
            return false;
        };
        state.queue.remove(ix);
        self.shared.emit(
            &state,
            KernelEvent::Failed {
                cell,
                why: Failure::Cancelled,
            },
        );
        true
    }

    /// Interrupts the running cell: `SIGINT` to the kernel's group, which
    /// ends the cell with a `KeyboardInterrupt` error unless it catches it.
    /// The queued cells run next, unless `flush`, which cancels them. True
    /// if a cell was running to interrupt. Never on Windows: see the module
    /// docs.
    pub fn interrupt(&self, flush: bool) -> bool {
        let mut state = self.shared.state();
        if flush {
            for (cell, _) in std::mem::take(&mut state.queue) {
                self.shared.emit(
                    &state,
                    KernelEvent::Failed {
                        cell,
                        why: Failure::Cancelled,
                    },
                );
            }
        }
        if state.running.is_none() || state.phase != Phase::Alive || cfg!(not(unix)) {
            return false;
        }
        if state.started {
            interrupt_group(&state);
        } else {
            state.interrupt_held = true;
        }
        true
    }

    /// Stops the kernel and everything it started, and waits for that:
    /// `SIGTERM` to its group, `SIGKILL` [`GRACE`] later. Its running and
    /// queued cells fail as [`Failure::ShutDown`].
    pub fn shutdown(&mut self) {
        self.request_stop();
        if let Some(watcher) = self.watcher.take() {
            let _ = watcher.join();
        }
    }

    fn request_stop(&self) {
        self.shared.state().stop = true;
        self.shared.changed.notify_all();
    }
}

impl Drop for Kernel {
    /// A kernel nobody holds is stopped, as [`Kernel::shutdown`] stops it,
    /// without waiting.
    fn drop(&mut self) {
        self.request_stop();
    }
}

/// `SIGINT` to the kernel's group, if it's still there: nothing on Windows.
fn interrupt_group(state: &State) {
    #[cfg(unix)]
    if let Some(group) = state.group {
        // SAFETY: kill(2) on a group whose leader isn't reaped while it's in
        // the state, which the caller holds locked, so it keeps its id.
        unsafe { libc::kill(-group, libc::SIGINT) };
    }
    #[cfg(not(unix))]
    let _ = state;
}

/// Kills `child`, which leads its group, and reaps it.
fn stop_now(child: &mut Child) {
    if let Some(group) = process_group::group_of(child) {
        process_group::kill(group);
    }
    let _ = child.wait();
}

/// The writer's thread: each line to the kernel's stdin, until it's
/// stopping or the kernel's gone.
fn write_lines(mut stdin: impl Write, lines: &mpsc::Receiver<Vec<u8>>) {
    for line in lines {
        if stdin.write_all(&line).and_then(|()| stdin.flush()).is_err() {
            break;
        }
    }
}

/// The reader's thread: the kernel's messages, until its stdout closes or
/// it says something that ends it.
fn read_messages(stdout: ChildStdout, shared: &Shared) {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    loop {
        line.clear();
        let limit = MAX_MESSAGE as u64 + 1;
        match (&mut reader).take(limit).read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => {
                shared.output_closed();
                break;
            }
            Ok(_) => {}
        }
        if line.pop() != Some(b'\n') {
            if line.len() >= MAX_MESSAGE {
                shared.bad(DeathCause::MessageTooLong);
            } else {
                // Cut off mid-line.
                shared.output_closed();
            }
            break;
        }
        let taken =
            serde_json::from_slice::<Message>(&line).is_ok_and(|message| shared.receive(message));
        if !taken {
            shared.bad(DeathCause::BadMessage);
            break;
        }
    }
    shared.reader_done();
}

/// The stderr reader's thread: the last [`LAST_WORDS`] bytes of it.
fn read_last_words(mut stderr: ChildStderr, shared: &Shared) {
    let mut buffer = [0; 8192];
    loop {
        match stderr.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let mut state = shared.state();
                let words = &mut state.last_words;
                words.extend(&buffer[..read]);
                let over = words.len().saturating_sub(LAST_WORDS);
                words.drain(..over);
            }
        }
    }
    shared.reader_done();
}

/// The watcher's thread: waits for a stop, a bad message or the kernel's
/// exit, stops the group, reaps the kernel and fails what's left.
fn watch(mut child: Child, shared: &Shared) {
    let mut state = shared.state();
    let cause = loop {
        if state.stop {
            break None;
        }
        if let Some(cause) = state.bad {
            break Some(cause);
        }
        if process_group::exited(&child) {
            break Some(DeathCause::Exited);
        }
        if state
            .closed_at
            .is_some_and(|closed| closed.elapsed() >= CLOSED_GRACE)
        {
            break Some(DeathCause::OutputClosed);
        }
        state = shared.wait_timeout(state, POLL);
    };
    state.phase = Phase::Ending;
    // The writer ends, and the kernel reads the end of its stdin.
    state.stdin = None;
    let group = state.group;
    drop(state);
    if let Some(group) = group {
        if cause.is_none() {
            process_group::terminate(group);
            let deadline = Instant::now() + GRACE;
            while !process_group::exited(&child) && Instant::now() < deadline {
                thread::sleep(POLL);
            }
        }
        process_group::kill(group);
    }
    let deadline = Instant::now() + READERS_GRACE;
    let mut state = shared.state();
    while state.readers > 0 {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        state = shared.wait_timeout(state, left);
    }
    // Not signalled again: it may be reaped.
    state.group = None;
    drop(state);
    let status = child.wait().ok();
    let mut state = shared.state();
    let why = match cause {
        Some(cause) => {
            let words = Vec::from(std::mem::take(&mut state.last_words));
            let last_words = String::from_utf8_lossy(&words).into_owned();
            let died = KernelEvent::Died {
                status,
                cause,
                last_words,
            };
            shared.emit(&state, died);
            Failure::Died
        }
        None => Failure::ShutDown,
    };
    state.phase = Phase::Dead(why);
    let cells: Vec<CellId> = (state.running.take().into_iter())
        .chain(
            std::mem::take(&mut state.queue)
                .into_iter()
                .map(|(cell, _)| cell),
        )
        .collect();
    for cell in cells {
        shared.emit(&state, KernelEvent::Failed { cell, why });
    }
    drop(state);
    shared.changed.notify_all();
}

// Unix only: they signal processes, and interrupt is Unix's.
#[cfg(all(test, unix))]
#[path = "notebook_kernel_tests.rs"]
mod tests;
