//! The API chat's `run_command` (the agents record's § 6 and § 7, card
//! AG8): what a call runs, whether it may, and running it.
//!
//! - **Resolving** ([`resolve_program`]). A bare program name (no `/`) is
//!   looked up on the login `PATH` ([`login_path`]), skipping two kinds of
//!   entry: relative ones, empty ones included, and those inside the root
//!   or any of the window's folders, as written or where they really are.
//!   The match is a regular file this process may run, its symlinks
//!   followed to its real path, which must not be inside a folder either.
//!   That real path is what runs ([`Program::Resolved`]), never the name
//!   again, so a repository can't put its own `git` in front of an
//!   approval. A program given with a path ([`Program::Given`]) runs from
//!   the command's folder as written, and matches no allow pattern and no
//!   remembered answer: it asks every time.
//! - **Deciding** ([`decide`]). The chat's grants are the ceiling:
//!   `process.spawn:<program>`, the program's file name, must be granted,
//!   else the call is denied by a hard rule. Then the [`Policy`] decides on
//!   the argv with the resolved path ([`Prepared::request`]), so a
//!   remembered answer holds only while the name resolves where it did.
//! - **Running** ([`RunningCommand::start`]). The resolved path runs, with
//!   the name asked for as its `argv[0]`, so a program that dispatches on
//!   its name (rustup's `cargo`) runs as asked. It runs on a thread of its
//!   own, leading a process group of its own ([`crate::process_group`]), in
//!   the folder, with stdin closed and stdout and stderr on one pipe, so
//!   their output interleaves as it was written. Past [`Limits::timeout`],
//!   or once cancelled ([`RunningCommand::cancel`], or dropping it), the
//!   group gets `SIGTERM`, and once none of it is left, or
//!   [`Limits::grace`] later, `SIGKILL`: a member cleaning up has the grace
//!   even if the leader ended at once. When the program ends, whatever it
//!   left running in its group is killed with it. The leader isn't reaped
//!   before its group is last signalled, so the group's id can't pass to
//!   another process meanwhile. The pipe is read on the runner's thread
//!   without blocking, at most [`PUMP_BYTES`] between looks at the
//!   deadlines, so a program writing without pause can't hold them off; it
//!   is closed [`DRAIN`] after the group is gone, so a process that left the
//!   group holding it keeps nothing of Tenmoku's.
//! - **Output** ([`Capture`]): at most [`HEAD`] bytes from the start and
//!   [`TAIL`] from the end are kept, so a chatty program never fills
//!   memory; past [`HEAD`] + [`TAIL`] the result has `[N bytes omitted]`
//!   between them. Then `exit N`, or `killed after T s`
//!   ([`Ran::result_text`]).
//!
//! The text here is the model's: the results and errors are what the tool
//! description in `fe_chat::tools` promises, in English.

use std::{
    collections::VecDeque,
    ffi::{OsStr, OsString},
    fmt,
    io::Read as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use fe_chat::{
    ChatId, RunCommandInput,
    permissions::{Level, Policy, Reason, Request},
};
use kiln::command::{Capability, Grants};

use crate::{chat_panel::chat_apply::locate, process_group};

#[cfg(test)]
#[path = "agent_command_tests.rs"]
mod tests;

/// The most output kept from a command's start.
pub(crate) const HEAD: usize = 8 << 10;
/// The most output kept from a command's end.
pub(crate) const TAIL: usize = 24 << 10;

/// How long a stopped command's group has between `SIGTERM` and `SIGKILL`.
pub(crate) const GRACE: Duration = Duration::from_secs(2);

/// How often the runner's thread looks for the program's exit, a cancel and
/// the deadline.
const POLL: Duration = Duration::from_millis(5);

/// How long the output is read for once the group is gone: a process that
/// left it can still hold the pipe, which is closed on it then.
const DRAIN: Duration = Duration::from_millis(250);

/// How often a stopped group is looked at during its grace.
const GRACE_POLL: Duration = Duration::from_millis(20);

/// The `PATH` bare programs are looked up on: the login shell's
/// ([`fe_lsp::LoginPath::system`]), else this process's. Runs the login
/// shell the first time, so call it off the main thread.
pub(crate) fn login_path() -> Option<OsString> {
    fe_lsp::LoginPath::system()
        .path()
        .map(OsStr::to_owned)
        .or_else(|| std::env::var_os("PATH"))
}

/// Whether `program` is a bare name, looked up on a `PATH`: no separator
/// and no root.
pub(crate) fn is_bare(program: &str) -> bool {
    !program.is_empty()
        && !program.chars().any(std::path::is_separator)
        && !Path::new(program).has_root()
}

/// `path` with its symlinks resolved, if it exists.
fn real(path: &Path) -> Option<PathBuf> {
    dunce::canonicalize(path).ok()
}

/// Whether `path` is inside any of `folders`, as written or where either
/// really is.
fn inside(path: &Path, folders: &[PathBuf]) -> bool {
    let real_path = real(path);
    folders.iter().any(|folder| {
        let real_folder = real(folder);
        path.starts_with(folder)
            || real_folder
                .as_deref()
                .is_some_and(|real_folder| path.starts_with(real_folder))
            || real_path.as_deref().is_some_and(|real_path| {
                real_path.starts_with(folder)
                    || (real_folder.as_deref()).is_some_and(|folder| real_path.starts_with(folder))
            })
    })
}

/// What `name` may be in `dir`.
#[cfg(unix)]
fn candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    vec![dir.join(name)]
}

/// What `name` may be in `dir`: Windows runs a name with one of the
/// program extensions added, or as given if it has one.
#[cfg(windows)]
fn candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    const EXTENSIONS: [&str; 4] = ["exe", "com", "bat", "cmd"];
    let given = Path::new(name).extension().and_then(OsStr::to_str);
    if given.is_some_and(|given| EXTENSIONS.iter().any(|ext| ext.eq_ignore_ascii_case(given))) {
        return vec![dir.join(name)];
    }
    (EXTENSIONS.iter())
        .map(|ext| dir.join(format!("{name}.{ext}")))
        .collect()
}

/// Whether `file` is a regular file, its links followed, this process may
/// run.
#[cfg(unix)]
fn is_runnable(file: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt as _;
    if !std::fs::metadata(file).is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    let Ok(path) = std::ffi::CString::new(file.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `path` is a NUL-terminated string that outlives the call.
    unsafe { libc::access(path.as_ptr(), libc::X_OK) == 0 }
}

#[cfg(windows)]
fn is_runnable(file: &Path) -> bool {
    std::fs::metadata(file).is_ok_and(|meta| meta.is_file())
}

/// The real path of the program a bare `name` names on `path`: the first
/// runnable regular file `<dir>/<name>` for an absolute `dir` of `path`
/// outside every one of `folders`, whose real path is outside them too.
/// Relative entries, empty ones included, are skipped: they'd resolve
/// under the command's folder, which is the project. `None` if `name`
/// isn't bare, or nothing matches.
pub(crate) fn resolve_program(name: &str, path: &OsStr, folders: &[PathBuf]) -> Option<PathBuf> {
    if !is_bare(name) || name == "." || name == ".." {
        return None;
    }
    std::env::split_paths(path)
        .filter(|dir| dir.is_absolute() && !inside(dir, folders))
        .flat_map(|dir| candidates(&dir, name))
        .filter(|candidate| is_runnable(candidate))
        .filter_map(|candidate| real(&candidate))
        .find(|resolved| is_runnable(resolved) && !inside(resolved, folders))
}

/// The program a call runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Program {
    /// A bare name, resolved on the login `PATH` to this real path.
    Resolved(PathBuf),
    /// A program given with a path: this one, under the command's folder
    /// if it was relative. It matches no allow pattern or remembered answer.
    Given(PathBuf),
}

impl Program {
    /// What runs.
    pub(crate) fn path(&self) -> &Path {
        match self {
            Program::Resolved(path) | Program::Given(path) => path,
        }
    }

    /// The resolved real path, which a remembered answer is bound to; none
    /// for a program given with a path.
    pub(crate) fn resolved(&self) -> Option<&Path> {
        match self {
            Program::Resolved(path) => Some(path),
            Program::Given(_) => None,
        }
    }
}

/// A `run_command` call made ready to decide and run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prepared {
    /// The argv as the model gave it: the program as written, then its
    /// arguments.
    pub(crate) argv: Vec<String>,
    pub(crate) program: Program,
    /// The folder it runs in, really: the root, or `cwd` inside it.
    pub(crate) cwd: PathBuf,
    /// The `PATH` it was resolved on, which it runs with.
    pub(crate) path: Option<OsString>,
}

impl Prepared {
    /// The [`Policy`]'s request for chat `chat`: the argv, with the
    /// resolved real path for a bare name.
    pub(crate) fn request(&self, chat: ChatId) -> Request {
        Request::argv(
            chat,
            self.argv.clone(),
            self.program.resolved().map(Path::to_path_buf),
        )
    }

    /// The program's file name, which grants name.
    pub(crate) fn program_name(&self) -> Option<&str> {
        let first = self.argv.first()?;
        Path::new(first).file_name()?.to_str()
    }
}

/// Makes `input` ready under `root`, one of `folders` (the window's), on
/// `path`, the login `PATH`: its folder found inside the root, its program
/// resolved. Reads the disk. An error is the call's `is_error` result, and
/// nothing is decided or run.
pub(crate) fn prepare(
    input: &RunCommandInput,
    root: &Path,
    folders: &[PathBuf],
    path: Option<&OsStr>,
) -> Result<Prepared, String> {
    let cwd = match input.cwd.as_deref() {
        None | Some("") | Some(".") => {
            real(root).ok_or_else(|| "The project folder can't be read".to_owned())?
        }
        Some(cwd) => locate(root, cwd, &[])?.path,
    };
    if !cwd.is_dir() {
        return Err(format!(
            "`{}` is not a folder",
            input.cwd.as_deref().unwrap_or(".")
        ));
    }
    let name = input.program.as_str();
    if name.is_empty() || name.trim() != name {
        return Err("`program` must be a program's name or path".to_owned());
    }
    let program = if is_bare(name) {
        let mut all = folders.to_vec();
        all.push(root.to_owned());
        let resolved = path
            .and_then(|path| resolve_program(name, path, &all))
            .ok_or_else(|| {
                format!(
                    "`{name}` was not found on the login PATH (folders inside the project \
                     and relative entries are not searched)"
                )
            })?;
        Program::Resolved(resolved)
    } else {
        Program::Given(cwd.join(name))
    };
    let mut argv = vec![input.program.clone()];
    argv.extend(input.args.iter().cloned());
    Ok(Prepared {
        argv,
        program,
        cwd,
        path: path.map(OsStr::to_owned),
    })
}

/// Who decided a request, as the audit trail records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Decider {
    /// The class's level.
    Policy,
    /// A `commands_*` pattern.
    Rule(String),
    /// The user, at the prompt.
    User,
    /// A remembered "Allow for this chat".
    Session,
    /// A rule no setting changes: a grant missing, a hard deny.
    Hard,
}

impl fmt::Display for Decider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Decider::Policy => f.write_str("policy"),
            Decider::Rule(pattern) => write!(f, "rule:{pattern}"),
            Decider::User => f.write_str("user"),
            Decider::Session => f.write_str("session"),
            Decider::Hard => f.write_str("hard"),
        }
    }
}

/// What [`decide`] came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Decision {
    pub(crate) level: Level,
    pub(crate) by: Decider,
}

/// Whether `grants` let the chat run `prepared`'s program:
/// `process.spawn:<file name>`, or `process.spawn:*`.
pub(crate) fn granted(grants: &Grants, prepared: &Prepared) -> bool {
    prepared
        .program_name()
        .and_then(|name| Capability::parse(&format!("process.spawn:{name}")).ok())
        .is_some_and(|capability| grants.allows(&capability))
}

/// Decides `prepared` for chat `chat`: denied by a hard rule unless
/// `grants` allow its program, else as `policy` decides its request.
pub(crate) fn decide(
    grants: &Grants,
    policy: &Policy,
    chat: ChatId,
    prepared: &Prepared,
) -> Decision {
    if !granted(grants, prepared) {
        return Decision {
            level: Level::Deny,
            by: Decider::Hard,
        };
    }
    let (level, reason) = policy.decide_with_reason(&prepared.request(chat));
    let by = match reason {
        Reason::Class => Decider::Policy,
        Reason::Pattern(pattern) => Decider::Rule(pattern),
        Reason::Remembered => Decider::Session,
        Reason::Hard => Decider::Hard,
    };
    Decision { level, by }
}

/// What a command may take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limits {
    /// How long it may run (`agents.command_timeout_secs`).
    pub(crate) timeout: Duration,
    /// How long its group has between `SIGTERM` and `SIGKILL`.
    pub(crate) grace: Duration,
}

impl Limits {
    /// A timeout of `secs` seconds, with the usual [`GRACE`].
    pub(crate) fn secs(secs: u32) -> Limits {
        Limits {
            timeout: Duration::from_secs(u64::from(secs)),
            grace: GRACE,
        }
    }
}

/// A command's output, capped: the first [`HEAD`] bytes and the last
/// [`TAIL`], and how many there were in all.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Capture {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    total: u64,
}

impl Capture {
    /// Takes `bytes` in, dropping the oldest of the tail past [`TAIL`].
    pub(crate) fn push(&mut self, mut bytes: &[u8]) {
        self.total += bytes.len() as u64;
        let room = HEAD - self.head.len();
        if room > 0 {
            let taken = room.min(bytes.len());
            self.head.extend_from_slice(&bytes[..taken]);
            bytes = &bytes[taken..];
        }
        if bytes.len() >= TAIL {
            self.tail.clear();
            self.tail.extend(&bytes[bytes.len() - TAIL..]);
            return;
        }
        let over = (self.tail.len() + bytes.len()).saturating_sub(TAIL);
        self.tail.drain(..over);
        self.tail.extend(bytes);
    }

    /// How many bytes were written in all.
    pub(crate) fn total(&self) -> u64 {
        self.total
    }

    /// The output: whole if it fits [`HEAD`] + [`TAIL`], else its head and
    /// tail with `[N bytes omitted]` between, cut at character boundaries
    /// (what a cut leaves of a character counts as omitted).
    pub(crate) fn text(&self) -> String {
        let tail: Vec<u8> = self.tail.iter().copied().collect();
        let kept = (self.head.len() + tail.len()) as u64;
        if kept == self.total {
            let mut all = self.head.clone();
            all.extend_from_slice(&tail);
            return String::from_utf8_lossy(&all).into_owned();
        }
        let head = &self.head[..char_end(&self.head)];
        let tail = &tail[char_start(&tail)..];
        let omitted = self.total - (head.len() + tail.len()) as u64;
        format!(
            "{}\n[{omitted} bytes omitted]\n{}",
            String::from_utf8_lossy(head),
            String::from_utf8_lossy(tail)
        )
    }
}

/// Where `bytes` ends at a character boundary: before a trailing
/// character cut short.
fn char_end(bytes: &[u8]) -> usize {
    let len = bytes.len();
    for back in 1..=len.min(4) {
        let byte = bytes[len - back];
        if byte & 0xc0 != 0x80 {
            // The lead byte of the last character: how long it should be.
            let want = match byte {
                0x00..=0x7f => 1,
                0xc0..=0xdf => 2,
                0xe0..=0xef => 3,
                _ => 4,
            };
            return if want > back { len - back } else { len };
        }
    }
    len
}

/// Where `bytes` starts at a character boundary: after a leading
/// character's continuation bytes, at most three.
fn char_start(bytes: &[u8]) -> usize {
    (bytes.iter().take(3))
        .take_while(|byte| *byte & 0xc0 == 0x80)
        .count()
}

/// Why a command was killed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KillReason {
    /// It ran past [`Limits::timeout`].
    Timeout,
    /// It was cancelled: the user stopped the turn, or trust was withdrawn
    /// (the loop knows which).
    Cancelled,
}

/// How a command ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// It exited with this code.
    Exited(i32),
    /// A signal of its own ended it.
    Signalled(i32),
    /// Its group was killed.
    Killed(KillReason),
    /// It never ran, for this reason.
    NotStarted(String),
}

/// A command run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ran {
    pub(crate) output: Capture,
    pub(crate) end: End,
    /// The timeout it ran under.
    pub(crate) timeout: Duration,
    /// How long it ran.
    pub(crate) elapsed: Duration,
}

impl Ran {
    /// The exit code, if it exited.
    pub(crate) fn exit_code(&self) -> Option<i32> {
        match self.end {
            End::Exited(code) => Some(code),
            _ => None,
        }
    }

    /// Whether the call's result is an error: it never ran, or was killed.
    pub(crate) fn is_error(&self) -> bool {
        matches!(self.end, End::NotStarted(_) | End::Killed(_))
    }

    /// The call's result: the output, then `exit N`, `killed after T s` or
    /// why it didn't run.
    pub(crate) fn result_text(&self) -> String {
        let status = match &self.end {
            End::Exited(code) => format!("exit {code}"),
            End::Signalled(signal) => format!("killed by signal {signal}"),
            End::Killed(KillReason::Timeout) => {
                format!("killed after {} s", self.timeout.as_secs())
            }
            End::Killed(KillReason::Cancelled) => {
                format!("killed after {:.1} s", self.elapsed.as_secs_f64())
            }
            End::NotStarted(why) => return why.clone(),
        };
        let output = self.output.text();
        if output.is_empty() {
            status
        } else if output.ends_with('\n') {
            format!("{output}{status}")
        } else {
            format!("{output}\n{status}")
        }
    }
}

/// A command running on a thread of its own. Dropping it cancels it.
pub(crate) struct RunningCommand {
    cancel: Arc<AtomicBool>,
    done: async_channel::Receiver<Ran>,
    group: Arc<Mutex<Option<process_group::GroupId>>>,
}

impl RunningCommand {
    /// Starts `prepared` under `limits`, on a thread of its own. Fails only
    /// if no thread starts: a program that can't run ends
    /// [`End::NotStarted`].
    pub(crate) fn start(prepared: Prepared, limits: Limits) -> std::io::Result<RunningCommand> {
        let cancel = Arc::new(AtomicBool::new(false));
        let group = Arc::new(Mutex::new(None));
        let (sender, done) = async_channel::bounded(1);
        let (cancelled, leader) = (cancel.clone(), group.clone());
        thread::Builder::new()
            .name("agent command".into())
            .spawn(move || {
                let ran = run(&prepared, limits, &cancelled, &leader);
                let _ = sender.try_send(ran);
            })?;
        Ok(RunningCommand {
            cancel,
            done,
            group,
        })
    }

    /// Stops the command and what it started: `SIGTERM` to its group, then
    /// `SIGKILL` after the grace. It ends [`KillReason::Cancelled`].
    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// A handle that cancels it from elsewhere.
    pub(crate) fn canceller(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    /// The group it leads, once it has started, while it runs.
    pub(crate) fn group(&self) -> Option<process_group::GroupId> {
        *self
            .group
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// How it ended. Dropping the future cancels it.
    pub(crate) async fn finished(self) -> Ran {
        match self.done.recv().await {
            Ok(ran) => ran,
            Err(_) => Ran {
                output: Capture::default(),
                end: End::NotStarted("The command's runner stopped".to_owned()),
                timeout: Duration::ZERO,
                elapsed: Duration::ZERO,
            },
        }
    }

    /// How it ended, waiting on this thread: for tests.
    #[cfg(test)]
    pub(crate) fn wait(self) -> Ran {
        let ran = self.done.recv_blocking();
        ran.expect("the runner sends how it ended")
    }
}

impl Drop for RunningCommand {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Runs `prepared` to its end, on the runner's thread.
fn run(
    prepared: &Prepared,
    limits: Limits,
    cancel: &AtomicBool,
    leader: &Mutex<Option<process_group::GroupId>>,
) -> Ran {
    let start = Instant::now();
    let not_started = |why: String| Ran {
        output: Capture::default(),
        end: End::NotStarted(why),
        timeout: limits.timeout,
        elapsed: start.elapsed(),
    };
    if cancel.load(Ordering::SeqCst) {
        return Ran {
            end: End::Killed(KillReason::Cancelled),
            ..not_started(String::new())
        };
    }
    let (reader, writer) = match std::io::pipe() {
        Ok(pipe) => pipe,
        Err(error) => return not_started(format!("The command couldn't start: {error}")),
    };
    let mut command = Command::new(prepared.program.path());
    command
        .args(&prepared.argv[1..])
        .current_dir(&prepared.cwd)
        .stdin(Stdio::null());
    if let Some(path) = &prepared.path {
        command.env("PATH", path);
    }
    match writer.try_clone() {
        Ok(copy) => command.stdout(copy).stderr(writer),
        Err(error) => return not_started(format!("The command couldn't start: {error}")),
    };
    // The name the program was asked for, not its real path, so a program
    // that dispatches on its name (rustup's `cargo`, busybox) runs as asked.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::arg0(&mut command, &prepared.argv[0]);
    process_group::lead_group(&mut command);
    let spawned = command.spawn();
    // The pipe's writing ends are the program's alone now, so the reader
    // sees its end once the program and what it started are gone.
    drop(command);
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            return not_started(format!(
                "`{}` couldn't start: {error}",
                prepared.program.path().display()
            ));
        }
    };
    let group = process_group::group_of(&child);
    *leader
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = group;

    let mut output = match Output::new(reader) {
        Ok(output) => output,
        Err(error) => {
            // With nothing reading it the pipe would fill and the program
            // wait for ever: stop it now.
            if let Some(group) = group {
                process_group::kill(group);
            }
            let _ = child.wait();
            return not_started(format!("The command's output couldn't be read: {error}"));
        }
    };

    let deadline = start + limits.timeout;
    let killed = loop {
        output.pump(POLL);
        if process_group::exited(&child) {
            break None;
        }
        if cancel.load(Ordering::SeqCst) {
            break Some(KillReason::Cancelled);
        }
        if Instant::now() >= deadline {
            break Some(KillReason::Timeout);
        }
    };
    if let Some(group) = group {
        if killed.is_some() {
            // The whole group has the grace, not just the leader: what is
            // cleaning up after SIGTERM may outlive it.
            process_group::terminate(group);
            let grace_end = Instant::now() + limits.grace;
            while group_alive(group, &child) && Instant::now() < grace_end {
                output.pump(GRACE_POLL);
            }
        }
        // Whatever is left of the group: what ignored SIGTERM, and what the
        // program left running when it ended.
        process_group::kill(group);
    }
    let status = child.wait();
    *leader
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    // A process that left the group can still hold the pipe: what it writes
    // within the drain is read, then the pipe is closed on it.
    output.drain(DRAIN);
    let output = output.finish();
    let end = match (killed, status) {
        (Some(reason), _) => End::Killed(reason),
        (None, Ok(status)) => match (status.code(), process_group::signal_of(&status)) {
            (Some(code), _) => End::Exited(code),
            (None, Some(signal)) => End::Signalled(signal),
            (None, None) => End::Exited(-1),
        },
        (None, Err(error)) => End::NotStarted(format!("The command's end wasn't seen: {error}")),
    };
    Ran {
        output,
        end,
        timeout: limits.timeout,
        elapsed: start.elapsed(),
    }
}

/// Whether any of group `group` is left alive but its leader `child`,
/// which isn't reaped: the leader runs, or (on Linux) another member that
/// isn't a zombie does. Elsewhere on Unix, any member, so the leader's
/// zombie keeps the grace going to its end.
fn group_alive(group: process_group::GroupId, child: &std::process::Child) -> bool {
    if !process_group::exited(child) {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return true;
        };
        entries.flatten().any(|entry| {
            let Some(pid) = (entry.file_name().to_str()).and_then(|name| name.parse::<i32>().ok())
            else {
                return false;
            };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
                return false;
            };
            // `pid (comm) state ppid pgrp …`; comm may hold spaces and `)`.
            let Some((_, rest)) = stat.rsplit_once(") ") else {
                return false;
            };
            let mut fields = rest.split(' ');
            let state = fields.next();
            let pgrp = fields.nth(1).and_then(|pgrp| pgrp.parse::<i32>().ok());
            pgrp == Some(group) && !matches!(state, Some("Z" | "X" | "x"))
        })
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        // SAFETY: signal 0 only asks whether the group has a member; its
        // leader isn't reaped, so it keeps its id.
        unsafe { libc::kill(-group, 0) == 0 }
    }
    #[cfg(windows)]
    {
        let _ = group;
        false
    }
}

/// The most a single [`Output::pump`] reads before it hands back, so a
/// program writing without pause can't keep the runner from its deadlines.
const PUMP_BYTES: usize = 256 << 10;

/// The command's output pipe, read into a [`Capture`] as it fills, on the
/// runner's thread and never blocking (`poll` on Unix, `PeekNamedPipe` on
/// Windows), so no thread is left behind: once [`Output::finish`] drops it,
/// the pipe is closed on any process still holding it.
struct Output {
    reader: std::io::PipeReader,
    capture: Capture,
    ended: bool,
}

impl Output {
    #[cfg(unix)]
    fn new(reader: std::io::PipeReader) -> std::io::Result<Output> {
        use std::os::fd::AsRawFd as _;
        let fd = reader.as_raw_fd();
        // SAFETY: fcntl(2) on a descriptor `reader` owns.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        // SAFETY: as above.
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Output {
            reader,
            capture: Capture::default(),
            ended: false,
        })
    }

    #[cfg(windows)]
    fn new(reader: std::io::PipeReader) -> std::io::Result<Output> {
        Ok(Output {
            reader,
            capture: Capture::default(),
            ended: false,
        })
    }

    /// Reads what's written, waiting up to `wait` for some, and hands back
    /// within about `wait` or [`PUMP_BYTES`] read, whichever comes first.
    fn pump(&mut self, wait: Duration) {
        let wait = wait.max(Duration::from_millis(1));
        let deadline = Instant::now() + wait;
        if self.ended {
            thread::sleep(wait);
            return;
        }
        let mut chunk = [0u8; 8 << 10];
        let mut taken = 0;
        while taken < PUMP_BYTES {
            match self.read_some(&mut chunk, deadline) {
                Read::Some(read) => {
                    self.capture.push(&chunk[..read]);
                    taken += read;
                }
                Read::Nothing => return,
                Read::Ended => {
                    self.ended = true;
                    return;
                }
            }
            if Instant::now() >= deadline {
                return;
            }
        }
    }

    /// Reads into `chunk` without blocking past `deadline`.
    #[cfg(unix)]
    fn read_some(&mut self, chunk: &mut [u8], deadline: Instant) -> Read {
        use std::os::fd::AsRawFd as _;
        loop {
            match self.reader.read(chunk) {
                Ok(0) => return Read::Ended,
                Ok(read) => return Read::Some(read),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Read::Nothing;
                    }
                    let mut poll = libc::pollfd {
                        fd: self.reader.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    let millis =
                        libc::c_int::try_from(left.as_millis().max(1)).unwrap_or(libc::c_int::MAX);
                    // SAFETY: one pollfd, valid for the call.
                    if unsafe { libc::poll(&mut poll, 1, millis) } == 0 {
                        return Read::Nothing;
                    }
                }
                Err(_) => return Read::Ended,
            }
        }
    }

    /// Reads into `chunk` without blocking past `deadline`: only what
    /// `PeekNamedPipe` says is there, so the read returns at once.
    #[cfg(windows)]
    fn read_some(&mut self, chunk: &mut [u8], deadline: Instant) -> Read {
        use std::os::windows::io::AsRawHandle as _;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn PeekNamedPipe(
                pipe: *mut std::ffi::c_void,
                buffer: *mut std::ffi::c_void,
                size: u32,
                read: *mut u32,
                available: *mut u32,
                left_of_message: *mut u32,
            ) -> i32;
        }
        loop {
            let mut available = 0u32;
            // SAFETY: the handle is the pipe `reader` owns; only `available`
            // is written.
            let peeked = unsafe {
                PeekNamedPipe(
                    self.reader.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            };
            if peeked == 0 {
                // Broken: every writer has closed it.
                return Read::Ended;
            }
            if available > 0 {
                let want = chunk.len().min(available as usize);
                return match self.reader.read(&mut chunk[..want]) {
                    Ok(0) | Err(_) => Read::Ended,
                    Ok(read) => Read::Some(read),
                };
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Read::Nothing;
            }
            thread::sleep(left.min(Duration::from_millis(2)));
        }
    }

    /// Reads on until every writer has closed the pipe, or for at most
    /// `within`.
    fn drain(&mut self, within: Duration) {
        let end = Instant::now() + within;
        while !self.ended {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            self.pump(left.min(POLL));
        }
    }

    /// What was read; the pipe is closed.
    fn finish(self) -> Capture {
        self.capture
    }
}

/// What one read of the pipe came to.
enum Read {
    Some(usize),
    /// Nothing by the deadline.
    Nothing,
    /// Every writer has closed it.
    Ended,
}
