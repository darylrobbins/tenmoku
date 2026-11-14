//! Claude Agent preflight: find `claude` and the ACP adapter, with their
//! versions, before any session (decision record
//! `docs/decisions/50t8wt9we2-ai-agents.md`, § 4 "Locating the programs";
//! this card's calls are in `docs/decisions/g30bkbs8zh-claude-agent-preflight.md`).
//!
//! The answer per program is [`Report::Found`], [`Report::Missing`] or
//! [`Report::TooOld`]; [`Preflight::ready`] is true only when both are found
//! and neither is too old.
//!
//! What this module does not do (§ 4 "What fe-acp never does"): it reads no
//! Claude Code credential file and decides nothing about login. Whether the
//! user is signed in only ever comes from the adapter's `authRequired` and
//! `_auth/status_update`. It never runs the adapter (an ACP server would wait
//! on stdin) and never uses `npx` or anything under a workspace root.
//!
//! Nothing runs at startup. Callers run [`check`] off the UI thread when the
//! chat needs the answer. fe-acp stays headless: the caller passes in
//! everything, in a [`PreflightEnv`].

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::claude::MIN_ADAPTER_VERSION;

/// A `major.minor.patch` version.
pub type Version = (u64, u64, u64);

/// The adapter's package name, as its `package.json` has it.
const ADAPTER_PACKAGE: &str = "@agentclientprotocol/claude-agent-acp";

/// How long `claude --version` may run.
pub const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// What the preflight is told; it reads nothing from the process itself.
#[derive(Debug, Clone)]
pub struct PreflightEnv {
    /// `agents.claude_code.claude`; empty means not set.
    pub configured_claude: String,
    /// `agents.claude_code.adapter`; empty means not set.
    pub configured_adapter: String,
    /// The login shell's `PATH` (`fe_lsp::LoginPath::path()`).
    pub login_path: Option<OsString>,
    /// The home directory: `~`, the fallbacks, and `claude --version`'s cwd.
    pub home: PathBuf,
    /// `MISE_DATA_DIR`.
    pub mise_data_dir: Option<PathBuf>,
    /// `HOMEBREW_PREFIX`.
    pub homebrew_prefix: Option<PathBuf>,
    /// The `claude --version` timeout.
    pub version_timeout: Duration,
}

impl PreflightEnv {
    /// An environment with nothing configured and the default timeout.
    #[must_use]
    pub fn new(home: PathBuf, login_path: Option<OsString>) -> Self {
        Self {
            configured_claude: String::new(),
            configured_adapter: String::new(),
            login_path,
            home,
            mise_data_dir: None,
            homebrew_prefix: None,
            version_timeout: VERSION_TIMEOUT,
        }
    }
}

/// The filesystem and process seam, so tests need no real programs.
pub trait Probe {
    /// Whether `path` is a file this process may run.
    fn is_runnable(&self, path: &Path) -> bool;
    /// The path with symlinks resolved.
    fn canonicalize(&self, path: &Path) -> Option<PathBuf>;
    /// A small text file's contents.
    fn read_small(&self, path: &Path) -> Option<String>;
    /// Runs `<program> --version` in `cwd` with `PATH` set to `path_env`, and
    /// returns its stdout; `None` when it fails or outlives `timeout` (and is
    /// killed).
    fn run_version(
        &self,
        program: &Path,
        cwd: &Path,
        path_env: Option<&OsStr>,
        timeout: Duration,
    ) -> Option<String>;
}

/// Where a program was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The configured path.
    Configured,
    /// The login shell's `PATH`.
    LoginPath,
    /// `~/.claude/local` (claude only).
    ClaudeLocal,
    /// `~/.local/bin`, the native installer's directory.
    NativeInstaller,
    /// The mise shims directory.
    MiseShims,
    /// A Homebrew `bin`.
    Homebrew,
}

/// One program's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    /// Found; the version is `None` when it couldn't be told.
    Found {
        /// The program's path.
        path: PathBuf,
        /// Where it was found.
        source: Source,
        /// Its version, if known.
        version: Option<Version>,
    },
    /// Not found; `configured` when a configured path was set but isn't runnable.
    Missing {
        /// A configured path was set.
        configured: bool,
    },
    /// Found, but below the floor.
    TooOld {
        /// The program's path.
        path: PathBuf,
        /// Its version.
        version: Version,
        /// The oldest version accepted.
        floor: Version,
    },
}

/// Both programs' answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preflight {
    /// The `claude` CLI.
    pub claude: Report,
    /// The ACP adapter.
    pub adapter: Report,
}

impl Preflight {
    /// True when both are found and neither is too old.
    #[must_use]
    pub fn ready(&self) -> bool {
        matches!(self.claude, Report::Found { .. }) && matches!(self.adapter, Report::Found { .. })
    }
}

/// Checks both programs.
#[must_use]
pub fn check(env: &PreflightEnv, probe: &dyn Probe) -> Preflight {
    Preflight {
        claude: check_claude(env, probe),
        adapter: check_adapter(env, probe),
    }
}

/// Finds `claude` and reads its version. There is no floor.
#[must_use]
pub fn check_claude(env: &PreflightEnv, probe: &dyn Probe) -> Report {
    match locate("claude", &env.configured_claude, true, env, probe) {
        Err(configured) => Report::Missing { configured },
        Ok((path, source)) => {
            let version = probe
                .run_version(
                    &path,
                    &env.home,
                    env.login_path.as_deref(),
                    env.version_timeout,
                )
                .and_then(|out| parse_leading_version(&out));
            Report::Found {
                path,
                source,
                version,
            }
        }
    }
}

/// Finds the adapter and reads its version from its `package.json`; it is
/// never run.
#[must_use]
pub fn check_adapter(env: &PreflightEnv, probe: &dyn Probe) -> Report {
    match locate(
        "claude-agent-acp",
        &env.configured_adapter,
        false,
        env,
        probe,
    ) {
        Err(configured) => Report::Missing { configured },
        Ok((path, source)) => match adapter_version(&path, probe) {
            Some(version) if version < MIN_ADAPTER_VERSION => Report::TooOld {
                path,
                version,
                floor: MIN_ADAPTER_VERSION,
            },
            version => Report::Found {
                path,
                source,
                version,
            },
        },
    }
}

/// `Err(configured)` when nothing was found.
fn locate(
    name: &str,
    configured: &str,
    claude_local: bool,
    env: &PreflightEnv,
    probe: &dyn Probe,
) -> Result<(PathBuf, Source), bool> {
    if !configured.trim().is_empty() {
        let path = expand_home(configured.trim(), &env.home);
        return if path.is_absolute() && probe.is_runnable(&path) {
            Ok((path, Source::Configured))
        } else {
            Err(true)
        };
    }
    let mut dirs: Vec<(PathBuf, Source)> = Vec::new();
    if let Some(path) = &env.login_path {
        dirs.extend(
            std::env::split_paths(path)
                .filter(|dir| dir.is_absolute())
                .map(|dir| (dir, Source::LoginPath)),
        );
    }
    if claude_local {
        dirs.push((env.home.join(".claude/local"), Source::ClaudeLocal));
    }
    dirs.push((env.home.join(".local/bin"), Source::NativeInstaller));
    let mise = env
        .mise_data_dir
        .clone()
        .filter(|dir| dir.is_absolute())
        .unwrap_or_else(|| env.home.join(".local/share/mise"));
    dirs.push((mise.join("shims"), Source::MiseShims));
    match env.homebrew_prefix.as_ref().filter(|dir| dir.is_absolute()) {
        Some(prefix) => dirs.push((prefix.join("bin"), Source::Homebrew)),
        None => {
            for prefix in ["/opt/homebrew", "/usr/local", "/home/linuxbrew/.linuxbrew"] {
                dirs.push((Path::new(prefix).join("bin"), Source::Homebrew));
            }
        }
    }
    dirs.into_iter()
        .map(|(dir, source)| (dir.join(name), source))
        .find(|(path, _)| probe.is_runnable(path))
        .ok_or(false)
}

fn expand_home(path: &str, home: &Path) -> PathBuf {
    if path == "~" {
        home.to_path_buf()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(path)
    }
}

/// The version of the package that holds the adapter's program: the nearest
/// `package.json` above the canonical path that names the adapter.
fn adapter_version(path: &Path, probe: &dyn Probe) -> Option<Version> {
    let real = probe.canonicalize(path)?;
    real.ancestors().skip(1).find_map(|dir| {
        let text = probe.read_small(&dir.join("package.json"))?;
        let json: serde_json::Value = serde_json::from_str(&text).ok()?;
        (json.get("name")?.as_str()? == ADAPTER_PACKAGE)
            .then(|| parse_leading_version(json.get("version")?.as_str()?))
            .flatten()
    })
}

/// The leading `MAJOR.MINOR.PATCH` of `2.0.1 (Claude Code)` or `v0.107.1-beta`.
#[must_use]
pub fn parse_leading_version(text: &str) -> Option<Version> {
    let text = text.trim_start().trim_start_matches('v');
    let end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let mut parts = text[..end].split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    Some((next()?, next()?, next()?))
}

/// The real filesystem and processes.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemProbe;

/// The most a small file or `--version` answer is read.
const SMALL: u64 = 256 * 1024;

impl Probe for SystemProbe {
    fn is_runnable(&self, path: &Path) -> bool {
        if !std::fs::metadata(path).is_ok_and(|meta| meta.is_file()) {
            return false;
        }
        // Executable for this process's user and groups, as `LoginPath` asks.
        #[cfg(unix)]
        {
            use rustix::fs::{Access, AtFlags, CWD};
            rustix::fs::accessat(CWD, path, Access::EXEC_OK, AtFlags::EACCESS).is_ok()
        }
        #[cfg(not(unix))]
        {
            true
        }
    }

    fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        std::fs::canonicalize(path).ok()
    }

    fn read_small(&self, path: &Path) -> Option<String> {
        let mut text = String::new();
        std::fs::File::open(path)
            .ok()?
            .take(SMALL)
            .read_to_string(&mut text)
            .ok()?;
        Some(text)
    }

    fn run_version(
        &self,
        program: &Path,
        cwd: &Path,
        path_env: Option<&OsStr>,
        timeout: Duration,
    ) -> Option<String> {
        use std::process::{Command, Stdio};
        let mut command = Command::new(program);
        command
            .arg("--version")
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(path) = path_env {
            command.env("PATH", path);
        }
        // Its own process group, so a timeout or exit can take down what a
        // wrapper script started, which may hold the stdout pipe.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command.spawn().ok()?;
        let mut stdout = child.stdout.take()?;
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut text = String::new();
            let read = (&mut stdout).take(SMALL).read_to_string(&mut text);
            let _ = sender.send(read.ok().map(|_| text));
        });
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => break None,
            }
        };
        // The answer must arrive by the deadline too: a descendant that kept
        // stdout open can't stall the caller.
        let text = status.as_ref().and_then(|_| {
            receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .ok()
                .flatten()
        });
        #[cfg(unix)]
        if let Some(group) = i32::try_from(child.id())
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
        if status.is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
        status.filter(std::process::ExitStatus::success)?;
        text
    }
}

#[cfg(test)]
#[path = "preflight_tests.rs"]
mod tests;
