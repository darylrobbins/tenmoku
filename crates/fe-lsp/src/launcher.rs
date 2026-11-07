//! The launch seam (docs/decisions/lsp-client.md §3.1): fe-lsp never spawns
//! a process itself. It asks a [`ServerLauncher`] for a server's byte
//! streams, on the server's own thread.

use std::ffi::{OsStr, OsString};
use std::future::Future;
#[cfg(unix)]
use std::hash::{BuildHasher, RandomState};
use std::io;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::process::{CommandExt, ExitStatusExt};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::pin::{Pin, pin};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError};
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use futures::channel::oneshot;
use futures::future::{FutureExt, Shared};
use futures::{AsyncRead, AsyncWrite};

/// What to start: from settings or supplied by the host.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerSpec {
    /// The server's name, e.g. "rust-analyzer". Names its thread and its
    /// status.
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    /// Set on top of the launcher's own environment.
    pub env: Vec<(String, String)>,
    /// fe-syntax language names the server handles.
    pub languages: Vec<String>,
    /// Sent as `initializationOptions`.
    pub initialization_options: Option<serde_json::Value>,
    /// Answers `workspace/configuration`: each item gets the value at its
    /// dotted `section` within this, or `null`.
    pub settings: serde_json::Value,
    pub readiness: ReadinessMode,
    /// Work-done progress token prefixes that don't hold back readiness.
    pub ready_ignores_progress: Vec<String>,
    pub unversioned_diagnostics: UnversionedDiagnostics,
    pub pull_diagnostics: PullDiagnostics,
    /// File or folder names that mark a project root, tried in priority
    /// order: a file outside the window's folder gets the nearest ancestor
    /// holding the first marker found anywhere up its tree.
    pub root_markers: Vec<String>,
}

impl ServerSpec {
    /// A spec running `command` with no arguments, environment, languages
    /// or settings, and the default modes.
    pub fn new(name: impl Into<String>, command: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            args: Vec::new(),
            env: Vec::new(),
            languages: Vec::new(),
            initialization_options: None,
            settings: serde_json::Value::Null,
            readiness: ReadinessMode::default(),
            ready_ignores_progress: Vec::new(),
            unversioned_diagnostics: UnversionedDiagnostics::default(),
            pull_diagnostics: PullDiagnostics::default(),
            root_markers: Vec::new(),
        }
    }
}

/// How a server's readiness is decided (§3.4). Chosen before startup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReadinessMode {
    /// rust-analyzer's `experimental/serverStatus` with `quiescent: true`.
    ServerStatus,
    /// `initialize` has answered and no work-done progress has been open
    /// for 500 ms.
    #[default]
    Progress,
}

/// What happens to a `publishDiagnostics` without a version (§4.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnversionedDiagnostics {
    /// Dropped: nothing fixes the text they were computed on.
    Drop,
    /// Decoded against the newest version in the incarnation's ring, the
    /// text the server was last sent, as VS Code shows them.
    Latest,
    /// [`Latest`](Self::Latest) while the session doesn't pull diagnostics,
    /// [`Drop`](Self::Drop) while it does: a pulling server's pushes stay
    /// dropped (typescript-language-server's, beside its tsserver pulls).
    #[default]
    LatestUnlessPulled,
}

/// How a server's diagnostics are pulled (§4.1, §7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PullDiagnostics {
    /// `textDocument/diagnostic`, when the server advertises
    /// `diagnosticProvider`.
    #[default]
    Advertised,
    /// tsserver's `syntacticDiagnosticsSync`, `semanticDiagnosticsSync` and
    /// `suggestionDiagnosticsSync`, sent back to back through
    /// `workspace/executeCommand` `typescript.tsserverRequest`, when the
    /// server registers that command (typescript-language-server). Its
    /// pushes merge tsserver's three kinds as they arrive, each of whatever
    /// text it was computed on, so no version fixes them. But tsserver runs
    /// no request queued after a `change` or `open` (its fence commands)
    /// before that one, so the three are computed on the text of the
    /// version sent before them. Checked by
    /// `tests::tsserver_diagnostics::pulls_through_tsserver_request` and
    /// recorded in docs/decisions/rq81bj4nvm-tsserver-diagnostics.md.
    TsserverRequest,
}

/// A running server's transport. Client → server bytes go to `input`.
pub struct ServerIo {
    pub input: Pin<Box<dyn AsyncWrite + Send>>,
    pub output: Pin<Box<dyn AsyncRead + Send>>,
    pub stderr: Option<Pin<Box<dyn AsyncRead + Send>>>,
    pub process: Box<dyn ServerProcess>,
}

/// How a server went away. Both fields are `None` when only its transport
/// is known to have closed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

/// The launcher's handle on what it started, for shutdown and crash
/// detection.
pub trait ServerProcess: Send {
    /// Resolves when the server has gone: exited, or its transport closed.
    /// Also after [`kill`](Self::kill), once it has taken effect: a
    /// replacement for the server waits for that.
    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>>;
    /// Forcibly stops it (kill the process group, close the proxy …).
    fn kill(&mut self);
}

/// Hands fe-lsp a server's byte streams. A host implements it to sandbox,
/// containerise or proxy servers, or to refuse one: an `Err` marks the
/// server `Stopped` with the error's message.
pub trait ServerLauncher: Send + Sync {
    /// Called on the server's own thread; may block (e.g. start a
    /// container).
    ///
    /// `cancel` fires when the server is stopped, or its manager's
    /// [`KillSwitch`](crate::KillSwitch) is thrown, while it launches. A
    /// launch should then return promptly: with
    /// [`ErrorKind::Interrupted`](io::ErrorKind::Interrupted), or with what
    /// it has started, which fe-lsp kills before sending it anything.
    /// Either way the server is `Stopped(None)`, not failed. fe-lsp can't
    /// preempt a launch that ignores `cancel`, so a launcher that doesn't
    /// watch it must bound its own launch time.
    fn launch(&self, spec: &ServerSpec, root: &Path, cancel: &LaunchCancel)
    -> io::Result<ServerIo>;
}

/// Asks a [`ServerLauncher::launch`] in progress to give up. fe-lsp makes
/// one per server instance and fires it on a stop or a kill; a host's tests
/// can make and fire their own. Clones share it, and once fired it stays
/// fired.
#[derive(Clone)]
pub struct LaunchCancel {
    own: Arc<Flag>,
    /// The manager's kill switch, which cancels it too.
    switch: Option<Arc<Flag>>,
}

impl LaunchCancel {
    pub fn new() -> Self {
        Self {
            own: Arc::default(),
            switch: None,
        }
    }

    /// One that `switch` fires too.
    pub(crate) fn linked(switch: Arc<Flag>) -> Self {
        Self {
            own: Arc::default(),
            switch: Some(switch),
        }
    }

    pub fn cancel(&self) {
        self.own.fire();
    }

    pub fn is_cancelled(&self) -> bool {
        self.own.fired() || self.switch.as_ref().is_some_and(|switch| switch.fired())
    }

    /// Resolves once it's cancelled, at once if it is already. A blocking
    /// launcher can wait on it beside its own work, with
    /// `async_io::block_on(futures::future::select(..))`.
    pub fn cancelled(&self) -> impl Future<Output = ()> + Send + 'static {
        let own = Flag::wait(&self.own);
        let switch = self.switch.as_ref().map(Flag::wait);
        async move {
            match switch {
                None => own.await,
                Some(switch) => {
                    futures::future::select(pin!(own), pin!(switch)).await;
                }
            }
        }
    }
}

impl Default for LaunchCancel {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for LaunchCancel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchCancel")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// A one-way flag that can be waited on: [`LaunchCancel`]'s, and the
/// manager's kill switch.
pub(crate) struct Flag {
    fired: AtomicBool,
    /// Taken (dropped) when it fires, which resolves `receiver`.
    sender: Mutex<Option<oneshot::Sender<()>>>,
    receiver: Shared<oneshot::Receiver<()>>,
}

impl Default for Flag {
    fn default() -> Self {
        let (sender, receiver) = oneshot::channel();
        Self {
            fired: AtomicBool::new(false),
            sender: Mutex::new(Some(sender)),
            receiver: receiver.shared(),
        }
    }
}

impl Flag {
    pub(crate) fn fire(&self) {
        self.fired.store(true, Ordering::SeqCst);
        drop(
            self.sender
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
    }

    pub(crate) fn fired(&self) -> bool {
        self.fired.load(Ordering::SeqCst)
    }

    /// Resolves once it has fired. It keeps the signal, so its sender can
    /// only go by firing.
    pub(crate) fn wait(this: &Arc<Self>) -> impl Future<Output = ()> + Send + 'static {
        let this = this.clone();
        async move {
            let _ = this.receiver.clone().await;
        }
    }
}

/// The default launcher: a local process in its own process group, with
/// piped stdio, cwd = root, the spec's environment and `kill_on_drop`.
/// [`ServerProcess::kill`] signals the whole group, so the server's own
/// children go too.
///
/// A bare `command` (a name with no `/`) is looked up on the login shell's
/// `PATH` ([`LoginPath::system`]), which the process also gets, unless the
/// spec's `env` sets one: an app started from Finder has only a minimal
/// `PATH`, while servers are usually installed where a terminal finds
/// them. An absolute command is used as it is, and a relative one is under
/// the root (see [`LoginPath::resolve`]). A bare one not found on its
/// `PATH` fails to launch, as not found.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalLauncher;

impl LocalLauncher {
    /// A [`LocalLauncher`] looking bare commands up on `path` rather than
    /// on [`LoginPath::system`].
    pub fn with_login_path(path: Arc<LoginPath>) -> impl ServerLauncher {
        WithPath(path)
    }
}

impl ServerLauncher for LocalLauncher {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        launch_local(spec, root, LoginPath::system(), cancel)
    }
}

struct WithPath(Arc<LoginPath>);

impl ServerLauncher for WithPath {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        launch_local(spec, root, &self.0, cancel)
    }
}

/// Resolves the command (which may run the login shell, within
/// [`LOGIN_SHELL_TIMEOUT`]), then spawns it, unless `cancel` fired
/// meanwhile.
fn launch_local(
    spec: &ServerSpec,
    root: &Path,
    login: &LoginPath,
    cancel: &LaunchCancel,
) -> io::Result<ServerIo> {
    let spec_path = spec
        .env
        .iter()
        .rev()
        .find(|(name, _)| name == "PATH")
        .map(|(_, path)| OsString::from(path));
    let program = login
        .resolve(&spec.command, spec_path.as_deref(), root)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} not found", spec.command.display()),
            )
        })?;
    let mut command = std::process::Command::new(program);
    // Also for absolute commands: a server may run other programs off
    // its PATH (`#!/usr/bin/env node`).
    if spec_path.is_none()
        && let Some(path) = login.path()
    {
        command.env("PATH", path);
    }
    command
        .args(&spec.args)
        .current_dir(root)
        .envs(spec.env.iter().map(|(k, v)| (k, v)));
    #[cfg(unix)]
    command.process_group(0);
    // No console window for the server; its children go in a job (below).
    #[cfg(windows)]
    command.creation_flags(job::CREATE_NO_WINDOW);
    let mut command = async_process::Command::from(command);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if cancel.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            format!("{}'s launch was cancelled", spec.name),
        ));
    }
    let mut child = command.spawn()?;
    // What `process_group` does on Unix: the server and what it starts are
    // killed together.
    #[cfg(windows)]
    let job = job::Job::for_process(child.id())
        .inspect_err(|err| {
            log::warn!(
                "language server {} runs outside a job, so its children may outlive it: {err}",
                spec.name
            );
        })
        .ok();
    let (Some(input), Some(output), stderr) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return Err(io::Error::other("the child's stdio wasn't piped"));
    };
    Ok(ServerIo {
        input: Box::pin(input),
        output: Box::pin(output),
        stderr: stderr.map(|s| Box::pin(s) as Pin<Box<dyn AsyncRead + Send>>),
        process: Box::new(LocalProcess {
            child,
            exit: None,
            #[cfg(windows)]
            job,
        }),
    })
}

/// How long the login shell may take to print its `PATH`.
pub const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_secs(2);

/// What the login shell runs: `printf %s "$PATH"`, with a line break after
/// it, appended to the file [`PATH_FILE_VAR`] names.
#[cfg(unix)]
const PRINT_PATH: &str = r#"printf "%s\n" "$PATH" >> "$FASTEDIT_LOGIN_PATH_FILE""#;

/// Names the file the login shell writes its `PATH` to.
#[cfg(unix)]
const PATH_FILE_VAR: &str = "FASTEDIT_LOGIN_PATH_FILE";

/// The `PATH` a login shell has, which bare commands are looked up on
/// (docs/decisions/lsp-client.md §3.3). The shell runs once, the first
/// time its `PATH` is needed, as `<shell> -l -c 'printf "%s\n" "$PATH" >>
/// "$FASTEDIT_LOGIN_PATH_FILE"'`, and what it wrote is kept.
///
/// The `PATH` goes to a file of Tenmoku's, not the shell's output, where
/// a profile's or logout script's chatter could pass for it. Only that
/// command writes there, and its final line break shows it wrote the
/// whole `PATH`, so any `PATH` is kept as it was written: relative
/// entries, or an empty one, which a shell takes as its working
/// directory. If there's nowhere to put that file, or the shell fails,
/// takes more than [`LOGIN_SHELL_TIMEOUT`], or ends without having written
/// its `PATH`, this process's `PATH` is used instead, and why is logged.
pub struct LoginPath {
    shell: PathBuf,
    /// What a leading `~` stands for.
    home: Option<PathBuf>,
    path: OnceLock<Option<OsString>>,
}

impl LoginPath {
    /// The `PATH` the login shell `shell` prints, with `$HOME` as `~`
    /// (`%USERPROFILE%` on Windows, where `HOME` is seldom set).
    pub fn new(shell: impl Into<PathBuf>) -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| {
                if cfg!(windows) {
                    std::env::var_os("USERPROFILE")
                } else {
                    None
                }
            })
            .map(PathBuf::from)
            .filter(|home| home.is_absolute());
        Self::with_home(shell, home)
    }

    /// As [`new`](Self::new), with `home` as `~`, or no `~` at all.
    pub(crate) fn with_home(shell: impl Into<PathBuf>, home: Option<PathBuf>) -> Self {
        Self {
            shell: shell.into(),
            home,
            path: OnceLock::new(),
        }
    }

    /// `$SHELL`'s, or, if that isn't an absolute path, `/bin/sh`'s, which
    /// is logged; shared by every [`LocalLauncher`].
    pub fn system() -> &'static LoginPath {
        static SYSTEM: LazyLock<LoginPath> = LazyLock::new(|| {
            // Windows has no login shell: commands are looked up on
            // Tenmoku's own PATH, which a user's environment sets.
            if cfg!(windows) {
                return LoginPath::new(PathBuf::new());
            }
            let shell = std::env::var_os("SHELL").filter(|shell| Path::new(shell).is_absolute());
            LoginPath::new(shell.unwrap_or_else(|| {
                log::warn!(
                    "SHELL isn't an absolute path, so language servers are looked up on the \
                     PATH of /bin/sh as a login shell"
                );
                "/bin/sh".into()
            }))
        });
        &SYSTEM
    }

    /// The login shell's `PATH`: on Windows, which has none, `None`.
    #[cfg(windows)]
    pub fn path(&self) -> Option<&OsStr> {
        let _ = (&self.shell, &self.path);
        None
    }

    /// The login shell's `PATH`, running it the first time; `None` if it
    /// failed.
    #[cfg(unix)]
    pub fn path(&self) -> Option<&OsStr> {
        self.path
            .get_or_init(|| match run_login_shell(&self.shell) {
                Ok(path) => Some(path),
                Err(why) => {
                    log::warn!(
                        "bare language server commands are looked up on Tenmoku's own PATH: \
                         the login shell {} {why}",
                        self.shell.display()
                    );
                    None
                }
            })
            .as_deref()
    }

    /// What to run for `command`, started in `cwd`, as a shell started
    /// there would run it. A bare name (no `/`) is the first file this
    /// process may run on one `PATH`, in order: `spec_path` if given, else
    /// the login shell's, or this process's if the shell failed. An
    /// absolute command is used as it is, and a relative one is under
    /// `cwd`.
    ///
    /// In a command and in a `PATH` entry alike, a leading `~` (alone or
    /// before a `/`) is the home directory, as bash has it, and never a
    /// directory named `~` under `cwd`. Other relative or empty entries are
    /// skipped, because `cwd` is the workspace root, which may be untrusted:
    /// a `PATH` holding `.`, a trailing `:` or `node_modules/.bin` must not
    /// run a server a cloned repository ships there as soon as a file opens.
    /// A relative `command` (with a `/`) is still under `cwd`, as whoever
    /// wrote it meant. `None` for a bare name not found, or a `~` with no
    /// home.
    pub fn resolve(
        &self,
        command: &Path,
        spec_path: Option<&OsStr>,
        cwd: &Path,
    ) -> Option<PathBuf> {
        if !is_bare(command) {
            let joined = cwd.join(expand_home(command, self.home.as_deref())?);
            // On Windows `\tools\x.exe` is on the current drive, as a launch
            // would find it.
            if cfg!(windows) && joined.has_root() && !joined.is_absolute() {
                return std::path::absolute(&joined).ok();
            }
            return Some(joined);
        }
        let process_path;
        let path = match spec_path.or_else(|| self.path()) {
            Some(path) => path,
            None => {
                process_path = std::env::var_os("PATH")?;
                &process_path
            }
        };
        find_executable(command, path, self.home.as_deref())
    }
}

/// A name with no `/`, such as `rust-analyzer`: one looked up on a `PATH`,
/// as `execvp` would.
fn is_bare(command: &Path) -> bool {
    !command
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .any(|&byte| std::path::is_separator(char::from(byte)))
        && !command.has_root()
}

/// `path` with a leading `~`, alone or before a `/`, made `home`; `None`
/// with no home. Anything else (`~user` too) as it is.
fn expand_home(path: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(first)) if first == "~" => Some(home?.join(components.as_path())),
        _ => Some(path.to_owned()),
    }
}

/// The first `dir/name` on `path` that is a file this process may run,
/// a `dir` with a leading `~` being under `home`. Only absolute
/// directories are searched: empty, `.` and other relative entries are
/// skipped, because they'd resolve under `cwd`, which is the workspace and
/// may be untrusted (a cloned repository could ship a `rust-analyzer` there
/// for a `PATH` holding `.`, an empty entry or `node_modules/.bin`). As in
/// a shell's lookup, a file it may not run is passed over for the next.
fn find_executable(name: &Path, path: &OsStr, home: Option<&Path>) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter_map(|dir| {
            // A `~` entry with no home is nowhere.
            let dir = expand_home(&dir, home)?;
            dir.is_absolute().then_some(dir)
        })
        .flat_map(|dir| candidates(&dir, name))
        .find(|candidate| is_runnable(candidate))
}

/// What `name` may be in `dir`: `dir/name`.
#[cfg(unix)]
fn candidates(dir: &Path, name: &Path) -> Vec<PathBuf> {
    vec![dir.join(name)]
}

/// What `name` may be in `dir`: `dir\name`, then with each of `PATHEXT`'s
/// extensions (`.exe`, `.cmd`, …), as `cmd` looks a command up.
#[cfg(windows)]
fn candidates(dir: &Path, name: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![dir.join(name)];
    for extension in program_extensions() {
        let mut with = dir.join(name).into_os_string();
        with.push(&extension);
        candidates.push(PathBuf::from(with));
    }
    candidates
}

/// `PATHEXT`'s extensions, each with its dot and in lower case as files
/// are usually named (`.exe`, not `.EXE`), or Windows' default.
#[cfg(windows)]
fn program_extensions() -> Vec<String> {
    std::env::var("PATHEXT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_owned())
        .split(';')
        .map(str::trim)
        .filter(|ext| ext.starts_with('.') && ext.len() > 1)
        .map(str::to_ascii_lowercase)
        .collect()
}

/// A file the kernel lets this process execute, judged for its effective
/// user and groups as `exec` judges it: an execute bit alone may be
/// another user's, and an ACL may deny it. (rustix's `fs` is on through
/// async-io and async-process.)
#[cfg(unix)]
fn is_runnable(file: &Path) -> bool {
    use rustix::fs::{Access, AtFlags, CWD};
    std::fs::metadata(file).is_ok_and(|meta| meta.is_file())
        && rustix::fs::accessat(CWD, file, Access::EXEC_OK, AtFlags::EACCESS).is_ok()
}

/// A file Windows runs as a program: one whose extension is `PATHEXT`'s.
#[cfg(windows)]
fn is_runnable(file: &Path) -> bool {
    let Some(extension) = file.extension().and_then(OsStr::to_str) else {
        return false;
    };
    std::fs::metadata(file).is_ok_and(|meta| meta.is_file())
        && program_extensions()
            .iter()
            .any(|known| known[1..].eq_ignore_ascii_case(extension))
}

/// Runs `shell -l -c` [`PRINT_PATH`] in a process group of its own and
/// returns the `PATH` it wrote, or why not, within [`LOGIN_SHELL_TIMEOUT`].
/// It runs on a thread of its own, so nothing the shell does (an exec or a
/// reap stuck on a hung file system, say) holds this up past the deadline;
/// that thread kills the shell's group then, and is left to finish alone.
#[cfg(unix)]
fn run_login_shell(shell: &Path) -> Result<OsString, String> {
    let deadline = Instant::now() + LOGIN_SHELL_TIMEOUT;
    let (sender, written) = std::sync::mpsc::channel();
    let shell = shell.to_owned();
    std::thread::Builder::new()
        .name("fe-lsp login shell".into())
        .spawn(move || {
            let _ = sender.send(capture_login_path(&shell, deadline));
        })
        .map_err(|err| format!("couldn't be run: {err}"))?;
    match written.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(written) => written,
        Err(RecvTimeoutError::Timeout) => Err(too_slow()),
        Err(RecvTimeoutError::Disconnected) => Err("stopped without an answer".into()),
    }
}

#[cfg(unix)]
fn too_slow() -> String {
    format!(
        "took over {} s to print its PATH",
        LOGIN_SHELL_TIMEOUT.as_secs()
    )
}

/// [`run_login_shell`]'s work, given until `deadline`: the `PATH` the
/// shell wrote, what's in its file before the final line break. The group
/// is killed if the shell takes longer. What it prints itself is
/// discarded.
#[cfg(unix)]
fn capture_login_path(shell: &Path, deadline: Instant) -> Result<OsString, String> {
    let file = PathFile::create().map_err(|err| format!("had nowhere to write its PATH: {err}"))?;
    let mut child = std::process::Command::new(shell)
        .args(["-l", "-c", PRINT_PATH])
        .env(PATH_FILE_VAR, &file.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|err| format!("didn't start: {err}"))?;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                kill_group(&mut child);
                return Err(too_slow());
            }
            Err(err) => {
                kill_group(&mut child);
                return Err(format!("couldn't be waited for: {err}"));
            }
        }
    };
    if !status.success() {
        return Err(format!("failed: {status}"));
    }
    let written = std::fs::read(&file.0)
        .map_err(|err| format!("wrote a PATH that couldn't be read back: {err}"))?;
    // Nothing, or no line break: the command didn't run, or didn't finish.
    match written.strip_suffix(b"\n") {
        Some(path) => Ok(OsStr::from_bytes(path).to_owned()),
        None => Err("ended without writing its PATH".into()),
    }
}

/// An empty file, new and only this user's, for the login shell to append
/// its `PATH` to. Removed when dropped.
#[cfg(unix)]
struct PathFile(PathBuf);

#[cfg(unix)]
impl PathFile {
    /// In the temporary directory, or in `/tmp` if that one can't be used.
    fn create() -> io::Result<Self> {
        let mut failed = None;
        for dir in [std::env::temp_dir(), PathBuf::from("/tmp")] {
            // Absolute, in case the shell's profile changes directory. An
            // empty `TMPDIR` can't be.
            let created = std::path::absolute(dir).and_then(|dir| Self::create_in(&dir));
            match created {
                Ok(file) => return Ok(file),
                Err(err) => {
                    failed.get_or_insert(err);
                }
            }
        }
        Err(failed.expect("tried at least once"))
    }

    fn create_in(dir: &Path) -> io::Result<Self> {
        let mut taken = None;
        for attempt in 0..16_u32 {
            // Random, so that no one can take every name first.
            let random = RandomState::new().hash_one(attempt);
            let path = dir.join(format!(
                "fastedit-login-path-{}-{random:016x}",
                std::process::id()
            ));
            // Never an existing file, or what a link there points to.
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(_) => return Ok(Self(path)),
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => taken = Some(err),
                Err(err) => return Err(err),
            }
        }
        Err(taken.expect("tried at least once"))
    }
}

#[cfg(unix)]
impl Drop for PathFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Kills `child`'s process group, which it leads, and reaps it.
#[cfg(unix)]
fn kill_group(child: &mut std::process::Child) {
    if let Some(group) = i32::try_from(child.id())
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    {
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

struct LocalProcess {
    child: async_process::Child,
    exit: Option<ExitInfo>,
    /// The job the server and its children run in, killed with it, and
    /// when this drops.
    #[cfg(windows)]
    job: Option<job::Job>,
}

/// A process dropped without having been seen to exit (its thread
/// panicked, say) takes its group with it: `kill_on_drop` alone would
/// leave the server's own children running.
impl Drop for LocalProcess {
    fn drop(&mut self) {
        if self.exit.is_none() {
            self.kill();
            // Reaped before the thread ends, which a replacement waits for.
            let _ = async_io::block_on(self.child.status());
        }
    }
}

impl ServerProcess for LocalProcess {
    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>> {
        Box::pin(async move {
            if let Some(exit) = self.exit {
                return exit;
            }
            let exit = match self.child.status().await {
                Ok(status) => ExitInfo {
                    code: status.code(),
                    #[cfg(unix)]
                    signal: status.signal(),
                    #[cfg(windows)]
                    signal: None,
                },
                Err(err) => {
                    log::warn!("waiting for language server {}: {err}", self.child.id());
                    ExitInfo::default()
                }
            };
            self.exit = Some(exit);
            exit
        })
    }

    fn kill(&mut self) {
        // The child leads its group (`process_group(0)`), so the group id is
        // its pid. ESRCH means the whole group has already gone. Signalled
        // even after the leader was reaped, so children it left behind go
        // too: a pid isn't reused while its group still has members.
        #[cfg(unix)]
        {
            let group = i32::try_from(self.child.id())
                .ok()
                .and_then(rustix::process::Pid::from_raw);
            if let Some(group) = group
                && let Err(err) =
                    rustix::process::kill_process_group(group, rustix::process::Signal::KILL)
                && err != rustix::io::Errno::SRCH
            {
                log::warn!("killing language server group {}: {err}", self.child.id());
            }
        }
        // The job holds the server and everything it started.
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.kill();
        }
        let _ = self.child.kill();
    }
}

/// A Windows job object, which kills the processes in it together: what a
/// process group is to a language server on Unix.
#[cfg(windows)]
mod job {
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    pub(super) use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    pub(super) struct Job(OwnedHandle);

    impl Job {
        /// A job holding process `pid`, which kills everything in it once
        /// its last handle (this) closes. A process the server starts
        /// before it is assigned stays outside it.
        pub(super) fn for_process(pid: u32) -> io::Result<Job> {
            // SAFETY: no name or security attributes.
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `job` is a new handle that nothing else owns.
            let job = Job(unsafe { OwnedHandle::from_raw_handle(job) });
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: `limits` is valid for reads of its size.
            let set = unsafe {
                SetInformationJobObject(
                    job.handle(),
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if set == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: always safe to call.
            let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
            if process.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: both handles are open.
            let assigned = unsafe { AssignProcessToJobObject(job.handle(), process) };
            let err = io::Error::last_os_error();
            // SAFETY: `process` is ours and not used after this.
            unsafe { CloseHandle(process) };
            if assigned == 0 {
                return Err(err);
            }
            Ok(job)
        }

        /// Kills every process in the job.
        pub(super) fn kill(&self) {
            // SAFETY: the job handle is open.
            unsafe { TerminateJobObject(self.handle(), 1) };
        }

        fn handle(&self) -> HANDLE {
            self.0.as_raw_handle() as HANDLE
        }
    }
}
