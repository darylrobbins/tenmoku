//! Running an external formatter over a document's text: `[formatters.<language>]`
//! in the settings. This is the run alone; nothing here chooses a formatter,
//! touches a buffer or holds the folder's trust, which are its caller's.
//!
//! [`format_text`] builds the command, runs it through a [`Runner`], and
//! answers with the one edit that turns the text into the output, or `None`
//! if there is nothing to change. It changes nothing itself, so any error
//! leaves the buffer as it was.
//!
//! # The command
//!
//! `{file}` in any word of the command, the program too, and wherever in the
//! word it falls (`--stdin-filepath={file}`), stands for:
//!
//! - a saved buffer's path, with `stdin = true`: the text goes to the
//!   program's stdin and its stdout is the result;
//! - a temporary copy of the text, with `stdin = false`, which the program
//!   rewrites and which is then read back. The copy has the buffer's file
//!   name, so its extension, in a directory of its own that only this user can
//!   read under the system's temporary directory, and all of that goes when
//!   the run ends, however it ends. A file-only tool has no copy to rewrite if
//!   its command has no `{file}`, and so reports no change;
//! - an unsaved buffer's placeholder, `untitled.<extension>` under the project
//!   root if there is one. An unsaved buffer always uses stdin, whatever
//!   `stdin` says: nothing on disk has its name, which is only a hint such as
//!   prettier's for choosing a parser.
//!
//! The program runs in the buffer's directory, or the project root for an
//! unsaved buffer, so it finds its config as it would from a terminal.
//!
//! # How a run ends
//!
//! [`FormatError::NotFound`] means the program doesn't exist, which its caller
//! may take as a cue to fall back without telling anyone. Everything else that
//! goes wrong is [`FormatError::Failed`], with what to show: a nonzero exit,
//! output that isn't UTF-8, no output at all from text that isn't blank (a
//! tool that formats in place, misconfigured, would otherwise empty the
//! buffer), a program it may not run, a copy that's no longer a regular file.
//! Past `timeout_ms`, counted over the run and reading back its copy,
//! [`format_text`] drops the run, which for [`ProcessRunner`] stops the child,
//! and answers [`FormatError::TimedOut`].
//!
//! # The real runner
//!
//! [`ProcessRunner`] looks a bare program up with
//! [`LoginPath::resolve`](fe_lsp::LoginPath::resolve), as the language servers
//! do: the login shell's `PATH`, so a Dock launch finds what a terminal does,
//! with its relative entries skipped, so a `PATH` holding `.` can't run a
//! `prettier` a cloned repository ships. The first run may be the one that
//! starts that shell, and pays for it out of its `timeout_ms`.
//!
//! Each run waits on a thread of its own, not an executor worker: the timer
//! that ends a hung run needs a worker, so hung runs mustn't hold them all.
//!
//! The child leads a process group of its own, as `npx` and the like leave
//! children of their own behind. Dropping the run stops the group: `SIGTERM`,
//! then after [`GRACE`] `SIGKILL`, the leader not reaped until then so its id
//! can't pass to another process. A process that left the group while holding
//! the pipes open, as a formatter's daemon might, looks like one still
//! running: the run ends at its timeout. Nothing here stops a run as the app
//! quits: its caller has to drop the run for that.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, DirBuilder};
use std::future::Future;
use std::io::{self, Read, Write as _};
use std::ops::Range;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::pin::{Pin, pin};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::process_group;
use fe_config::FormatterSettings;
use fe_lsp::LoginPath;
use futures::future::{Either, select};
use gpui::BackgroundExecutor;
use kiln_l10n::{tr, verbatim};

/// What a command's words hold for the file.
const FILE_PLACEHOLDER: &str = "{file}";
/// An unsaved buffer's name, and a temporary copy's if its path has none.
const UNTITLED: &str = "untitled";
/// The most stdout, or stderr, a run may write, and the longest a rewritten
/// copy may be: what a formatter of a file that fits in a buffer has no reason
/// to pass, and what bounds the memory a runaway one takes.
pub const OUTPUT_LIMIT: usize = 64 << 20;
/// How long a stopped run has between `SIGTERM` and `SIGKILL`.
const GRACE: Duration = Duration::from_secs(1);
/// How often a run looks for its end, or for being stopped.
const POLL: Duration = Duration::from_millis(5);
/// How many names a temporary directory tries before giving up.
const TEMP_ATTEMPTS: u32 = 100;

/// One run of a program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    /// Where it runs; `None` for wherever this process is.
    pub cwd: Option<PathBuf>,
    /// Its stdin; `None` for none.
    pub stdin: Option<Vec<u8>>,
}

/// How a run that started ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunOutput {
    /// Its exit code; `None` if a signal ended it.
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Runs a program. The seam [`format_text`] runs through, so nothing that
/// tests it needs a process.
pub trait Runner {
    /// Runs `spec`. An error of kind [`io::ErrorKind::NotFound`] means the
    /// program doesn't exist, and no other failure may use that kind. Dropping
    /// the future stops the run.
    fn run(&self, spec: RunSpec) -> Pin<Box<dyn Future<Output = io::Result<RunOutput>>>>;
}

/// Where the text being formatted lives.
#[derive(Clone, Copy, Debug)]
pub enum BufferPath<'a> {
    /// A saved buffer's absolute path.
    Saved(&'a Path),
    /// A buffer not yet saved.
    Unsaved {
        /// The project's root, if there is one.
        root: Option<&'a Path>,
        /// Its language's file extension, for the placeholder's name.
        extension: Option<&'a str>,
    },
}

impl BufferPath<'_> {
    /// Where the program runs.
    fn working_dir(self) -> Option<PathBuf> {
        match self {
            BufferPath::Saved(path) => path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map(Path::to_owned),
            BufferPath::Unsaved { root, .. } => root.map(Path::to_owned),
        }
    }
}

/// Why a formatter gave no text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatError {
    /// The program doesn't exist.
    NotFound(String),
    /// It ran and failed, or could not be run: `stderr_line` says why, in its
    /// own first line on stderr when it wrote one.
    Failed {
        program: String,
        stderr_line: String,
    },
    /// It ran past `timeout_ms`, and was stopped.
    TimedOut { program: String, timeout_ms: u64 },
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::NotFound(program) => {
                let said = tr!("workbench-workbench-format-not-found", program = program);
                f.write_str(&said)
            }
            FormatError::Failed {
                program,
                stderr_line,
            } => write!(f, "{}: {}", program, stderr_line),
            FormatError::TimedOut {
                program,
                timeout_ms,
            } => {
                let ms = *timeout_ms;
                let said = tr!(
                    "workbench-workbench-format-timed-out",
                    program = program,
                    ms = ms
                );
                f.write_str(&said)
            }
        }
    }
}

impl std::error::Error for FormatError {}

/// Runs `settings`'s command over `text`, the contents of `buffer`, on
/// `runner`, within `settings.timeout_ms` by `executor`'s timer. Answers the
/// edit that makes `text` the formatted text ([`diff_to_edit`]), `None` for
/// none; see the module docs.
pub async fn format_text(
    runner: &dyn Runner,
    executor: &BackgroundExecutor,
    settings: &FormatterSettings,
    buffer: BufferPath<'_>,
    text: &str,
) -> Result<Option<(Range<usize>, String)>, FormatError> {
    let Some(program) = settings.command.first() else {
        return Err(FormatError::Failed {
            program: String::new(),
            stderr_line: tr!("workbench-workbench-format-empty").into(),
        });
    };
    let failed = |stderr_line: String| FormatError::Failed {
        program: program.clone(),
        stderr_line,
    };

    let copy = match buffer {
        BufferPath::Saved(path) if !settings.stdin => {
            let name = path.file_name().unwrap_or(OsStr::new(UNTITLED)).to_owned();
            let bytes = text.as_bytes().to_vec();
            let copy = executor
                .spawn(async move { TempCopy::new(&name, &bytes) })
                .await
                .map_err(|error| {
                    let error = error.to_string();
                    failed(tr!("workbench-workbench-format-cant-copy", error = error).into())
                })?;
            Some(copy)
        }
        _ => None,
    };
    let file = match (&copy, buffer) {
        (Some(copy), _) => copy.file.clone(),
        (None, BufferPath::Saved(path)) => path.to_owned(),
        (None, BufferPath::Unsaved { root, extension }) => placeholder(root, extension),
    };
    let mut words = settings.command.iter().map(|word| expand(word, &file));
    let spec = RunSpec {
        program: words.next().unwrap_or_default(),
        args: words.collect(),
        cwd: buffer.working_dir(),
        stdin: copy.is_none().then(|| text.as_bytes().to_vec()),
    };

    // One deadline, over the run and reading back what it wrote.
    let copied = copy.as_ref().map(|copy| copy.file.clone());
    let timer = executor.timer(Duration::from_millis(settings.timeout_ms));
    let result = pin!(async {
        let output = match runner.run(spec).await {
            Ok(output) => output,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(FormatError::NotFound(program.clone()));
            }
            Err(error) => return Err(failed(error.to_string())),
        };
        if output.code != Some(0) {
            return Err(failed(failure_line(&output)));
        }
        match copied {
            Some(file) => executor
                .spawn(async move { read_limited(&file) })
                .await
                .map_err(|error| {
                    let error = error.to_string();
                    failed(tr!("workbench-workbench-format-cant-read-copy", error = error).into())
                }),
            None => Ok(output.stdout),
        }
    });
    let formatted = match select(timer, result).await {
        Either::Left(_) => {
            return Err(FormatError::TimedOut {
                program: program.clone(),
                timeout_ms: settings.timeout_ms,
            });
        }
        Either::Right((formatted, _)) => formatted?,
    };
    if formatted.len() > OUTPUT_LIMIT {
        return Err(failed(too_much_output()));
    }
    let formatted = String::from_utf8(formatted)
        .map_err(|_| failed(tr!("workbench-workbench-format-not-utf8").into()))?;
    if formatted.is_empty() && !text.trim().is_empty() {
        return Err(failed(tr!("workbench-workbench-format-no-output").into()));
    }
    Ok(diff_to_edit(text, &formatted))
}

/// The one edit that makes `old` into `new`: `(range in old, its new text)`,
/// the common prefix and suffix left out, on char boundaries, so carets and
/// folds past it survive. `None` if they're equal.
pub fn diff_to_edit(old: &str, new: &str) -> Option<(Range<usize>, String)> {
    if old == new {
        return None;
    }
    let (old_bytes, new_bytes) = (old.as_bytes(), new.as_bytes());
    let mut prefix = (old_bytes.iter().zip(new_bytes))
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
        prefix -= 1;
    }
    // Only what's left after the prefix, so the two can't overlap.
    let (old_rest, new_rest) = (&old_bytes[prefix..], &new_bytes[prefix..]);
    let mut suffix = (old_rest.iter().rev().zip(new_rest.iter().rev()))
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    Some((
        prefix..old.len() - suffix,
        new[prefix..new.len() - suffix].to_owned(),
    ))
}

/// `word` with each `{file}` as `file`, which needn't be UTF-8.
fn expand(word: &str, file: &Path) -> OsString {
    let mut pieces = word.split(FILE_PLACEHOLDER);
    let mut expanded = OsString::from(pieces.next().unwrap_or_default());
    for piece in pieces {
        expanded.push(file);
        expanded.push(piece);
    }
    expanded
}

/// What `{file}` is for an unsaved buffer.
fn placeholder(root: Option<&Path>, extension: Option<&str>) -> PathBuf {
    let name = match extension.map(|extension| extension.trim_start_matches('.')) {
        Some(extension) if !extension.is_empty() => format!("{UNTITLED}.{extension}"),
        _ => UNTITLED.to_owned(),
    };
    root.map_or_else(|| PathBuf::from(&name), |root| root.join(&name))
}

/// A failed run's first line on stderr that isn't blank, or else how it ended.
fn failure_line(output: &RunOutput) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stderr.lines().map(str::trim).find(|line| !line.is_empty());
    match (line, output.code) {
        (Some(line), _) => line.to_owned(),
        (None, Some(code)) => tr!("workbench-workbench-format-exited", code = code).into(),
        (None, None) => tr!("workbench-workbench-format-killed").into(),
    }
}

fn too_much_output() -> String {
    tr!(
        "workbench-workbench-format-too-much",
        limit = OUTPUT_LIMIT >> 20
    )
    .into()
}

/// `file`, up to one byte past [`OUTPUT_LIMIT`], if it's a regular file. A tool
/// may have left anything there: opening without blocking means a FIFO with no
/// writer can't hold a worker thread, and it's then refused for what it is.
fn read_limited(file: &Path) -> io::Result<Vec<u8>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NONBLOCK);
    let file = options.open(file)?;
    if !file.metadata()?.is_file() {
        let said = tr!("workbench-workbench-format-not-a-file");
        return Err(io::Error::other(said.to_string()));
    }
    let mut read = Vec::new();
    file.take(OUTPUT_LIMIT as u64 + 1).read_to_end(&mut read)?;
    Ok(read)
}

/// A copy of a buffer's text for a tool to rewrite, in a directory only this
/// user can read, as the system's temporary directory is anyone's. Both go when
/// this does.
struct TempCopy {
    dir: PathBuf,
    file: PathBuf,
}

impl TempCopy {
    fn new(name: &OsStr, text: &[u8]) -> io::Result<TempCopy> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temp = std::env::temp_dir();
        for _ in 0..TEMP_ATTEMPTS {
            let number = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = temp.join(format!("tenmoku-format-{}-{number}", std::process::id()));
            // Fails rather than follow anything already there.
            #[cfg_attr(not(unix), allow(unused_mut))]
            let mut builder = DirBuilder::new();
            // Windows keeps the temporary directory's own restrictions: a
            // user's is in their profile.
            #[cfg(unix)]
            builder.mode(0o700);
            match builder.create(&dir) {
                Ok(()) => {
                    let copy = TempCopy {
                        file: dir.join(name),
                        dir,
                    };
                    fs::write(&copy.file, text)?;
                    return Ok(copy);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other(
            tr!("workbench-workbench-format-names-taken").to_string(),
        ))
    }
}

impl Drop for TempCopy {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.dir) {
            log::warn!(
                "couldn't remove the formatter's copy {}: {error}",
                self.dir.display()
            );
        }
    }
}

/// The [`Runner`] that starts real processes; see the module docs.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(&self, spec: RunSpec) -> Pin<Box<dyn Future<Output = io::Result<RunOutput>>>> {
        on_own_thread(move |stopped| run_process(spec, stopped))
    }
}

/// Runs `work`, which blocks, on a thread of its own, and answers what it does.
/// Dropping the future sets the flag `work` is given, which it has to look at
/// to stop. Not on the executor's workers: there are few of them, and the timer
/// that would stop a hung run needs one, so enough hung runs at once could
/// never time out.
pub(crate) fn on_own_thread<T: Send + 'static>(
    work: impl FnOnce(&AtomicBool) -> io::Result<T> + Send + 'static,
) -> Pin<Box<dyn Future<Output = io::Result<T>>>> {
    let stop = StopOnDrop(Arc::new(AtomicBool::new(false)));
    let stopped = stop.0.clone();
    let (sender, answer) = async_channel::bounded(1);
    let thread = thread::Builder::new()
        .name(verbatim!("external formatter").into())
        .spawn(move || {
            // There's room for the one answer, and nobody to tell if it's
            // no longer wanted.
            let _ = sender.send_blocking(work(&stopped));
        });
    Box::pin(async move {
        let _stop = stop;
        thread?;
        answer.recv().await.unwrap_or_else(|_| {
            let said = tr!("workbench-workbench-format-no-answer");
            Err(io::Error::other(said.to_string()))
        })
    })
}

/// Sets its flag as it goes, which is how a blocked thread hears of a drop.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Runs `spec` to its end, blocking, until `stopped` is set. The run's group
/// is stopped on a thread of its own, once this has returned.
fn run_process(spec: RunSpec, stopped: &AtomicBool) -> io::Result<RunOutput> {
    let login = LoginPath::system();
    let cwd = spec.cwd.as_deref().unwrap_or(Path::new(""));
    let program = resolve_program(login, Path::new(&spec.program), None, cwd)?;
    let mut command = Command::new(program);
    command
        .args(&spec.args)
        .stdin(match spec.stdin {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process_group::lead_group(&mut command);
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    if let Some(path) = login.path() {
        command.env("PATH", path);
    }
    if stopped.load(Ordering::SeqCst) {
        return Err(io::ErrorKind::Interrupted.into());
    }
    let mut child = command.spawn().map_err(|error| match &spec.cwd {
        // Spawning says the same of a directory that's gone as of a missing
        // program, which is the one thing NotFound may mean.
        Some(cwd) if error.kind() == io::ErrorKind::NotFound && !cwd.is_dir() => {
            let path = cwd.display().to_string();
            io::Error::other(
                tr!("workbench-workbench-format-no-directory", path = path).to_string(),
            )
        }
        _ => error,
    })?;
    if let (Some(mut stdin), Some(input)) = (child.stdin.take(), spec.stdin) {
        // On a thread of its own, so a program that writes before it has read
        // everything can't block on this, nor this on it. One that stops
        // reading closes the pipe: that's its business.
        thread::spawn(move || drop(stdin.write_all(&input)));
    }
    let over = Arc::new(AtomicBool::new(false));
    let stdout = read_capped(child.stdout.take(), over.clone());
    let stderr = read_capped(child.stderr.take(), over.clone());
    loop {
        if stopped.load(Ordering::SeqCst) {
            stop_group(child);
            return Err(io::ErrorKind::Interrupted.into());
        }
        if over.load(Ordering::SeqCst) {
            stop_group(child);
            return Err(io::Error::other(too_much_output()));
        }
        if exited(&child) && stdout.is_finished() && stderr.is_finished() {
            let read = |reader: JoinHandle<Vec<u8>>| reader.join().unwrap_or_default();
            let (stdout, stderr) = (read(stdout), read(stderr));
            // A reader may pass the cap as it ends, after the check above.
            if stdout.len() > OUTPUT_LIMIT || stderr.len() > OUTPUT_LIMIT {
                stop_group(child);
                return Err(io::Error::other(too_much_output()));
            }
            let code = child.wait()?.code();
            return Ok(RunOutput {
                code,
                stdout,
                stderr,
            });
        }
        thread::sleep(POLL);
    }
}

/// What to run for `program`, by [`LoginPath::resolve`] on `path`, or the login
/// shell's `PATH` if that's `None`. Its answer of nothing is split in two: a
/// bare name that a directory it searches has as a file it may not run is
/// [`PermissionDenied`](io::ErrorKind::PermissionDenied), which someone should
/// hear about, and only one that's nowhere is
/// [`NotFound`](io::ErrorKind::NotFound).
pub(crate) fn resolve_program(
    login: &LoginPath,
    program: &Path,
    path: Option<&OsStr>,
    cwd: &Path,
) -> io::Result<PathBuf> {
    if let Some(resolved) = login.resolve(program, path, cwd) {
        return Ok(resolved);
    }
    let bare = !program
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .any(|&byte| std::path::is_separator(char::from(byte)));
    // The directories resolve searches: the same PATH, absolute entries only.
    let path = path
        .or_else(|| login.path())
        .map(OsStr::to_owned)
        .or_else(|| std::env::var_os("PATH"));
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute());
    let not_runnable = bare
        && path
            .iter()
            .flat_map(std::env::split_paths)
            .filter_map(|dir| match dir.strip_prefix("~") {
                Ok(rest) => Some(home.as_ref()?.join(rest)),
                Err(_) => dir.is_absolute().then_some(dir),
            })
            .any(|dir| dir.join(program).is_file());
    Err(match not_runnable {
        true => io::Error::new(
            io::ErrorKind::PermissionDenied,
            tr!("workbench-workbench-format-not-runnable").to_string(),
        ),
        false => io::ErrorKind::NotFound.into(),
    })
}

/// Whether `child` has exited, without reaping it: until it's reaped, its id,
/// which is its group's, can't go to another process.
fn exited(child: &Child) -> bool {
    process_group::exited(child)
}

/// Reads `pipe` to its end on a thread of its own, up to one byte past
/// [`OUTPUT_LIMIT`], which sets `over`.
fn read_capped(
    pipe: Option<impl Read + Send + 'static>,
    over: Arc<AtomicBool>,
) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut read = Vec::new();
        if let Some(pipe) = pipe {
            let _ = pipe.take(OUTPUT_LIMIT as u64 + 1).read_to_end(&mut read);
        }
        if read.len() > OUTPUT_LIMIT {
            over.store(true, Ordering::SeqCst);
        }
        read
    })
}

/// Stops `child`'s process group: `SIGTERM` now, and after [`GRACE`] `SIGKILL`
/// to whatever is left of it, on a thread of its own. The child, the group's
/// leader, is reaped only after that, so the group's id is still its own.
fn stop_group(mut child: Child) {
    let Some(group) = process_group::group_of(&child) else {
        let _ = child.kill();
        let _ = child.wait();
        return;
    };
    // The group's leader isn't reaped until below, so it keeps its id.
    process_group::terminate(group);
    thread::spawn(move || {
        thread::sleep(GRACE);
        process_group::kill(group);
        let _ = child.wait();
    });
}
