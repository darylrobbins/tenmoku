// SPDX-License-Identifier: LicenseRef-Proprietary

//! The host's process-spawning imports for plugins.
//!
//! A plugin runs a program only by a `process.spawn:<program>` grant (or `process.spawn:*`
//! narrowed to it), and only the file the grant was bound to: the absolute path recorded when it
//! was granted, never the name looked up again. The file's identity is checked again just
//! before each spawn, and a program that changed or went has its grant suspended, so the user
//! is asked again. The program gets its arguments as they are, never through a shell, the stdin
//! the plugin passes, and a minimal environment; its output is capped, and a run past its
//! deadline is killed.

use std::ffi::OsString;
use std::io::{self, Read as _, Write as _};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use kiln_command::capability::{Capability, Target};

use crate::bindings::kiln::extension::process::{Host, Output};
use crate::bindings::{Error, HostState};
use crate::grants::{self, GrantRecord, GrantStore, ProgramIdentity, Suspended};
use crate::limits::Limits;

/// How long a spawned program may run before it is killed.
pub const SPAWN_DEADLINE: Duration = Duration::from_secs(30);

/// The `PATH` a spawned program gets: the system's own directories, not the app's.
pub const SYSTEM_PATH: &str = "/usr/bin:/bin";

/// How long a spawn waits, when its program neither wrote nor read, before looking again.
const IDLE: Duration = Duration::from_millis(2);

/// The process slice of [`HostState`]: the plugin's spawn grants, and how far a spawn may go.
pub struct State {
    record: GrantRecord,
    /// Where a suspension is recorded, and the plugin's id there.
    store: Option<(GrantStore, String)>,
    max_output: usize,
    deadline: Duration,
    environment: Vec<(OsString, OsString)>,
}

impl Default for State {
    /// Grants nothing: every spawn is refused.
    fn default() -> Self {
        Self::new(GrantRecord::default(), &Limits::default())
    }
}

impl State {
    /// Spawns as `record` grants, each stream of output capped at `limits.max_read_bytes`, each
    /// run killed after [`SPAWN_DEADLINE`], in [`minimal_environment`] taken from the app's.
    pub fn new(record: GrantRecord, limits: &Limits) -> Self {
        Self {
            record,
            store: None,
            max_output: limits.max_read_bytes,
            deadline: SPAWN_DEADLINE,
            environment: minimal_environment(std::env::vars_os()),
        }
    }

    /// Records a suspension in `store` too, under `plugin`, so the next load asks again.
    pub fn recorded_in(mut self, store: GrantStore, plugin: &str) -> Self {
        self.store = Some((store, plugin.to_owned()));
        self
    }

    /// Kills each run after `deadline` rather than [`SPAWN_DEADLINE`].
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// The plugin's grants, less any suspended since.
    pub fn record(&self) -> &GrantRecord {
        &self.record
    }

    /// Runs the program granted as `program` with `args`, feeding it `stdin`, to completion.
    ///
    /// Its stdout and stderr are each cut at the cap, and a program still running when it writes
    /// past it is killed, so its status is none. One still running at the deadline is killed and
    /// the call fails: so does one whose output stays open past it, as when a program it started
    /// holds it. Either way nothing of the call's is left running in the host when it returns.
    pub fn spawn(
        &mut self,
        program: &str,
        args: &[String],
        stdin: Option<&[u8]>,
    ) -> Result<Output, SpawnError> {
        let identity = self.granted(program)?;
        if let Err(suspended) = grants::check_program(&identity) {
            self.suspend(program, suspended);
            return Err(SpawnError::Suspended {
                program: program.to_owned(),
                suspended,
            });
        }
        let mut command = Command::new(&identity.path);
        command
            .args(args)
            .env_clear()
            .envs(self.environment.iter().map(|(name, value)| (name, value)));
        run(command, stdin, self.max_output, self.deadline).map_err(|failure| match failure {
            Failure::Io(source) => SpawnError::Io {
                path: identity.path.clone(),
                source,
            },
            Failure::TimedOut => SpawnError::TimedOut {
                program: program.to_owned(),
                deadline: self.deadline,
            },
        })
    }

    /// What `program`'s grant is bound to, if it is granted and bound.
    fn granted(&self, program: &str) -> Result<ProgramIdentity, SpawnError> {
        if let Some(&reason) = self.record.suspended.get(program) {
            return Err(SpawnError::Suspended {
                program: program.to_owned(),
                suspended: Suspended { reason },
            });
        }
        let capability = Capability::ProcessSpawn(Target::Named(program.to_owned()));
        let not_granted = || SpawnError::NotGranted {
            program: program.to_owned(),
        };
        if !self.record.granted.allows(&capability) {
            return Err(not_granted());
        }
        // `process.spawn:*` allows any name, but only a name bound to a program runs.
        self.record
            .programs
            .get(program)
            .cloned()
            .ok_or_else(not_granted)
    }

    fn suspend(&mut self, program: &str, suspended: Suspended) {
        log::warn!("process.spawn:{program}: {suspended}");
        self.record.suspend(program, suspended.reason);
        if let Some((store, plugin)) = &self.store
            && let Err(error) = store.check_programs(plugin)
        {
            log::warn!("can't record that process.spawn:{program} is suspended: {error}");
        }
    }
}

impl Host for HostState {
    fn spawn(
        &mut self,
        program: String,
        args: Vec<String>,
        stdin: Option<Vec<u8>>,
    ) -> Result<Output, Error> {
        self.process
            .spawn(&program, &args, stdin.as_deref())
            .map_err(|error| match error {
                SpawnError::NotGranted { .. } | SpawnError::Suspended { .. } => {
                    Error::Denied(error.to_string())
                }
                SpawnError::TimedOut { .. } | SpawnError::Io { .. } => {
                    Error::Failed(error.to_string())
                }
            })
    }
}

/// Why [`State::spawn`] failed.
#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    /// No grant names the program, or none bound it to a file.
    #[error("process.spawn:{program} is not granted")]
    NotGranted { program: String },
    /// The program changed or went since it was granted; nothing ran.
    #[error("process.spawn:{program}: {suspended}")]
    Suspended {
        program: String,
        #[source]
        suspended: Suspended,
    },
    /// It ran past its deadline and was killed.
    #[error("{program} ran past its {deadline:?} deadline and was killed")]
    TimedOut { program: String, deadline: Duration },
    #[error("can't run {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// The environment a spawned program gets out of the app's `environment`: `PATH` set to
/// [`SYSTEM_PATH`], and `HOME`, `LANG`, `LC_*`, `TMPDIR` and `TERM` as the app has them.
pub fn minimal_environment(
    environment: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    let passed = |name: &OsString| {
        name.to_str().is_some_and(|name| {
            matches!(name, "HOME" | "LANG" | "TMPDIR" | "TERM") || name.starts_with("LC_")
        })
    };
    let mut minimal = vec![("PATH".into(), SYSTEM_PATH.into())];
    minimal.extend(environment.into_iter().filter(|(name, _)| passed(name)));
    minimal
}

enum Failure {
    Io(io::Error),
    TimedOut,
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Runs `command` to completion, feeding it `stdin`, its stdout and stderr each kept to
/// `max_output` bytes: one still running when it writes past that is killed. It is killed too
/// at `deadline`, which is then [`Failure::TimedOut`].
///
/// The program's stdio are sockets rather than pipes, so this thread alone can feed and drain
/// them without blocking: when it returns, by any path, the host's ends close with it and no
/// reader or writer is left behind, whatever the program, or one it started, holds open.
fn run(
    mut command: Command,
    stdin: Option<&[u8]>,
    max_output: usize,
    deadline: Duration,
) -> Result<Output, Failure> {
    let deadline = Instant::now() + deadline;
    let mut feed = match stdin {
        Some(bytes) => {
            let (ours, theirs) = UnixStream::pair()?;
            command.stdin(OwnedFd::from(theirs));
            Some(Feed {
                socket: ours,
                bytes,
            })
        }
        None => {
            command.stdin(Stdio::null());
            None
        }
    };
    let (stdout, theirs) = UnixStream::pair()?;
    command.stdout(OwnedFd::from(theirs));
    let (stderr, theirs) = UnixStream::pair()?;
    command.stderr(OwnedFd::from(theirs));
    let mut child = command.spawn()?;
    // The command holds the program's ends: they must close here for its output to end.
    drop(command);
    let mut child = Running(&mut child);
    let mut stdout = Drain::new(stdout)?;
    let mut stderr = Drain::new(stderr)?;
    if let Some(feed) = &feed {
        feed.socket.set_nonblocking(true)?;
    }
    let mut status = None;
    loop {
        let mut moved = stdout.read(max_output)? | stderr.read(max_output)?;
        if let Some(fed) = &mut feed {
            match fed.write() {
                Ok(written) => moved |= written,
                // The program closed its stdin, or it is all written: either way, done with it.
                Err(()) => feed = None,
            }
        }
        if stdout.over || stderr.over {
            child.kill();
            status = Some(None);
        }
        if status.is_none()
            && let Some(exited) = child.0.try_wait()?
        {
            status = Some(exited.code());
        }
        if let Some(status) = status
            && (stdout.ended() && stderr.ended())
        {
            return Ok(Output {
                status,
                stdout: stdout.kept,
                stderr: stderr.kept,
            });
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(Failure::TimedOut);
        }
        if !moved {
            thread::sleep(IDLE.min(deadline - now));
        }
    }
}

/// A spawned program, killed and reaped if it is still running when this drops.
struct Running<'a>(&'a mut Child);

impl Running<'_> {
    fn kill(&mut self) {
        // It may have exited since it was looked at; either way it is reaped.
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            self.kill();
        }
    }
}

/// The plugin's stdin bytes, on their way into the program.
struct Feed<'a> {
    socket: UnixStream,
    bytes: &'a [u8],
}

impl Feed<'_> {
    /// Writes what the socket takes: whether any went, or `Err` once there's nothing more to
    /// write or the program won't read it.
    fn write(&mut self) -> Result<bool, ()> {
        if self.bytes.is_empty() {
            return Err(());
        }
        match self.socket.write(self.bytes) {
            Ok(written) => {
                self.bytes = &self.bytes[written..];
                Ok(written > 0)
            }
            Err(error) if retry(&error) => Ok(false),
            Err(_) => Err(()),
        }
    }
}

/// One of the program's output streams, read as far as it goes without blocking.
struct Drain {
    /// None once it ended, or went past the cap.
    socket: Option<UnixStream>,
    kept: Vec<u8>,
    over: bool,
}

impl Drain {
    fn new(socket: UnixStream) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket: Some(socket),
            kept: Vec::new(),
            over: false,
        })
    }

    fn ended(&self) -> bool {
        self.socket.is_none()
    }

    /// Reads what is there, keeping `cap` bytes; at the first byte past them it stops for
    /// good. Whether anything was read.
    fn read(&mut self, cap: usize) -> io::Result<bool> {
        let Some(socket) = &mut self.socket else {
            return Ok(false);
        };
        let mut chunk = [0; 16 << 10];
        let mut moved = false;
        loop {
            match socket.read(&mut chunk) {
                Ok(0) => {
                    self.socket = None;
                    return Ok(true);
                }
                Ok(read) => {
                    moved = true;
                    let room = cap - self.kept.len();
                    self.kept.extend_from_slice(&chunk[..read.min(room)]);
                    if read > room {
                        self.over = true;
                        self.socket = None;
                        return Ok(true);
                    }
                }
                Err(error) if retry(&error) => return Ok(moved),
                Err(error) => return Err(error),
            }
        }
    }
}

/// Whether `error` only says to try again later.
fn retry(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}
