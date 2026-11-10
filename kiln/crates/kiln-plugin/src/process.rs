// SPDX-License-Identifier: Apache-2.0

//! The host's process-spawning imports for plugins.
//!
//! A plugin runs a program only by a `process.spawn:<program>` grant (or `process.spawn:*`
//! narrowed to it), and only the file the grant was bound to: the absolute path recorded when it
//! was granted, never the name looked up again. The file's identity is checked again just
//! before each spawn, and a program that changed or went has its grant suspended, so the user
//! is asked again. The program gets its arguments as they are, never through a shell, the stdin
//! the plugin passes, and a minimal environment; its output is capped, and a run past its
//! deadline is killed. A run has its own limit, [`PROGRAM_DEADLINE`] (10 s), not the 100 ms call
//! deadline: the time a call waits on a run isn't charged to the call, and the runs of one call
//! share that limit. Everything before a launch comes out of that share and stops at its end: the
//! check of the program, the waiting for a suspension to reach the store (the writing goes on
//! elsewhere, until it is written), and the setup, after which no child starts.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use kiln_command::capability::{Capability, Target};
use kiln_core::platform::{OutputDrain, ProcessTree, StdinFeed};
use kiln_l10n::tr;

use crate::bindings::kiln::extension::process::{Host, Output};
use crate::bindings::{Error, HostState};
use crate::grants::{
    self, GrantRecord, GrantStore, ProgramIdentity, Suspended, SuspensionWrite, SuspensionWriter,
};
use crate::limits::{Limits, PROGRAM_DEADLINE};

/// What the program runs of the call now running have used of its share, and what is left.
///
/// The runner arms it at each call's start with the call's budget and disarms it at the end;
/// [`HostState`]'s `spawn` charges each run's wall time to it, and the runner moves the call's
/// deadline back by that time. Armed, a run is killed at the earlier of its own deadline and the
/// budget left. Unarmed, as with a [`State`] used without a runner, each run has its own deadline.
/// Clones share one budget.
#[derive(Clone, Default)]
pub struct ProgramBudget(Arc<BudgetCells>);

#[derive(Default)]
struct BudgetCells {
    armed: AtomicBool,
    /// Nanoseconds the call's runs may take in all.
    budget: AtomicU64,
    /// Nanoseconds the call's runs have taken.
    spent: AtomicU64,
}

fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

impl ProgramBudget {
    /// Starts a call whose runs may take `budget` in all.
    pub fn arm(&self, budget: Duration) {
        self.0.spent.store(0, Ordering::Release);
        self.0.budget.store(nanos(budget), Ordering::Release);
        self.0.armed.store(true, Ordering::Release);
    }

    /// Ends the call: nothing is charged, and runs have their own deadlines again.
    pub fn disarm(&self) {
        self.0.armed.store(false, Ordering::Release);
        self.0.spent.store(0, Ordering::Release);
    }

    /// How long the running call's program runs have taken.
    pub fn spent(&self) -> Duration {
        Duration::from_nanos(self.0.spent.load(Ordering::Acquire))
    }

    /// `deadline` moved back by [`Self::spent`]: the call's deadline, which doesn't count the
    /// time its runs took. None when it is none, or too far off to name.
    pub fn extend(&self, deadline: Option<Instant>) -> Option<Instant> {
        deadline?.checked_add(self.spent())
    }

    /// What is left of the call's share, if a call is running.
    pub(crate) fn remaining(&self) -> Option<Duration> {
        self.0.armed.load(Ordering::Acquire).then(|| {
            let left = (self.0.budget.load(Ordering::Acquire))
                .saturating_sub(self.0.spent.load(Ordering::Acquire));
            Duration::from_nanos(left)
        })
    }

    /// Charges `taken` to the call's share, no more than is left of it, and returns what was
    /// charged: the time the call's deadline moves back by, so the time a call can exclude from
    /// its deadline is capped at its budget however many runs or refusals it makes.
    pub(crate) fn charge(&self, taken: Duration) -> Duration {
        let armed = self.0.armed.load(Ordering::Acquire);
        let budget = self.0.budget.load(Ordering::Acquire);
        let mut charged = 0;
        let _ = self
            .0
            .spent
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |spent| {
                let most = if armed {
                    budget.saturating_sub(spent)
                } else {
                    u64::MAX
                };
                charged = nanos(taken).min(most);
                Some(spent.saturating_add(charged))
            });
        Duration::from_nanos(charged)
    }
}

/// The `PATH` a spawned program gets: the system's own directories, not the app's.
pub const SYSTEM_PATH: &str = "/usr/bin:/bin";

/// How long a spawn waits, when its program neither wrote nor read, before looking again.
const IDLE: Duration = Duration::from_millis(2);

/// The process slice of [`HostState`]: the plugin's spawn grants, and how far a spawn may go.
pub struct State {
    record: GrantRecord,
    /// Where a suspension is recorded, the plugin's id there, and what writes to it.
    store: Option<(GrantStore, String, Arc<SuspensionWriter>)>,
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
    /// run killed after [`PROGRAM_DEADLINE`], in [`minimal_environment`] taken from the app's.
    pub fn new(record: GrantRecord, limits: &Limits) -> Self {
        Self {
            record,
            store: None,
            max_output: limits.max_read_bytes,
            deadline: PROGRAM_DEADLINE,
            environment: minimal_environment(std::env::vars_os()),
        }
    }

    /// Records a suspension in `store` too, under `plugin`, so the next load asks again.
    pub fn recorded_in(mut self, store: GrantStore, plugin: &str) -> Self {
        let writer = SuspensionWriter::of(&store);
        writer.start();
        self.store = Some((store, plugin.to_owned(), writer));
        self
    }

    /// Kills each run after `deadline` rather than [`PROGRAM_DEADLINE`].
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
        self.spawn_within(program, args, stdin, None)
    }

    /// [`Self::spawn`], killed at the earlier of the run's deadline and `left`, what is left of
    /// the call's share of program time. The share runs from this call, so the time checking the
    /// program takes comes out of it: a run is refused when that used it up.
    pub fn spawn_within(
        &mut self,
        program: &str,
        args: &[String],
        stdin: Option<&[u8]>,
        left: Option<Duration>,
    ) -> Result<Output, SpawnError> {
        self.spawn_from(program, args, stdin, left, Instant::now())
    }

    /// [`Self::spawn_within`] with `left` counted from `entered`, when the share was read.
    pub(crate) fn spawn_from(
        &mut self,
        program: &str,
        args: &[String],
        stdin: Option<&[u8]>,
        left: Option<Duration>,
        entered: Instant,
    ) -> Result<Output, SpawnError> {
        self.start_writer();
        let share_ends = left.map(|left| entered.checked_add(left));
        // What is left of the share now, if there is one: none is no limit but the run's own.
        let share = || {
            share_ends.map(|ends| match ends {
                Some(ends) => ends.saturating_duration_since(Instant::now()),
                None => Duration::MAX,
            })
        };
        let timed_out = |deadline| SpawnError::TimedOut {
            program: program.to_owned(),
            deadline,
        };
        let identity = self.granted(program)?;
        // A share already used up refuses before the program is checked, which reads the file.
        if share().is_some_and(|share| share.is_zero()) {
            return Err(timed_out(Duration::ZERO));
        }
        // Reading the program, and waiting for a suspension to be recorded, stop at its end too.
        let ends = share_ends.flatten();
        let checked = match ends {
            Some(ends) => grants::check_program_by(&identity, ends),
            None => Ok(grants::check_program(&identity)),
        };
        match checked {
            Err(grants::CheckTimedOut) => return Err(timed_out(share().unwrap_or_default())),
            Ok(Err(suspended)) => {
                self.suspend(program, &identity, suspended, ends);
                return Err(SpawnError::Suspended {
                    program: program.to_owned(),
                    suspended,
                });
            }
            Ok(Ok(())) => {}
        }
        let mut command = Command::new(&identity.path);
        command
            .args(args)
            .env_clear()
            .envs(self.environment.iter().map(|(name, value)| (name, value)));
        // No console window for a program run from a GUI app (`CREATE_NO_WINDOW`): set by
        // `ProcessTree::spawn` with the rest of the creation flags, in `run`.
        // The check took its time from the share too: what is left is the run's.
        let now = Instant::now();
        let own_end = now + self.deadline;
        let ends_at = ends.map_or(own_end, |ends| ends.min(own_end));
        let deadline = ends_at.saturating_duration_since(now);
        run(command, stdin, self.max_output, ends_at).map_err(|failure| match failure {
            Failure::Io(source) => SpawnError::Io {
                path: identity.path.clone(),
                source,
            },
            Failure::TimedOut => SpawnError::TimedOut {
                program: program.to_owned(),
                deadline,
            },
        })
    }

    /// Withdraws `program`'s grant here at once, and hands the store's writer the record of that.
    /// This thread waits for it only until `by`, or for the run's own deadline with no share: past
    /// it the writing goes on without it, and is retried until it's done.
    fn suspend(
        &mut self,
        program: &str,
        identity: &ProgramIdentity,
        suspended: Suspended,
        by: Option<Instant>,
    ) {
        log::warn!("process.spawn:{program}: {suspended}");
        self.record.suspend(program, suspended.reason);
        let Some((store, plugin, writer)) = &self.store else {
            return;
        };
        let written = writer.submit(SuspensionWrite {
            store: store.clone(),
            plugin: plugin.clone(),
            program: program.to_owned(),
            identity: identity.clone(),
            grant_token: self.record.grant_token(program).map(str::to_owned),
            reason: suspended.reason,
        });
        let Some(written) = written else {
            return;
        };
        let by = by.unwrap_or_else(|| Instant::now() + self.deadline);
        let left = by.saturating_duration_since(Instant::now());
        if written.recv_timeout(left).is_err() {
            log::warn!("process.spawn:{program}: its suspension isn't recorded yet");
        }
    }

    /// Gives a writer that couldn't start another chance at what is queued for it.
    fn start_writer(&self) {
        if let Some((_, _, writer)) = &self.store {
            writer.start();
        }
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
}

impl Drop for State {
    /// Unloading a plugin is one more chance for a writer that couldn't start: what is queued
    /// outlives this state either way.
    fn drop(&mut self) {
        self.start_writer();
    }
}

impl Host for HostState {
    fn spawn(
        &mut self,
        program: String,
        args: Vec<String>,
        stdin: Option<Vec<u8>>,
    ) -> Result<Output, Error> {
        // A run isn't charged to the call: its wall time moves the call's deadline back, and
        // the call's runs share the budget the runner armed.
        let started = Instant::now();
        let ran = self.process.spawn_within(
            &program,
            &args,
            stdin.as_deref(),
            self.program_runs.remaining(),
        );
        let taken = started.elapsed();
        let charged = self.program_runs.charge(taken);
        self.deadline = self
            .deadline
            .and_then(|deadline| deadline.checked_add(charged));
        ran.map_err(|error| match error {
            SpawnError::NotGranted { .. } | SpawnError::Suspended { .. } => {
                Error::Denied(error.to_string())
            }
            SpawnError::TimedOut { .. } | SpawnError::Io { .. } => Error::Failed(error.to_string()),
        })
    }
}

/// Why [`State::spawn`] failed.
#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    /// No grant names the program, or none bound it to a file.
    NotGranted { program: String },
    /// The program changed or went since it was granted; nothing ran.
    Suspended {
        program: String,
        #[source]
        suspended: Suspended,
    },
    /// It ran past its deadline and was killed, or the call's share of program time ended
    /// before it started: then nothing ran.
    TimedOut { program: String, deadline: Duration },
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// In the active locale.
impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Self::NotGranted { program } => tr!("kiln-plugin-spawn-not-granted", program = program),
            Self::Suspended { program, suspended } => tr!(
                "kiln-plugin-spawn-suspended",
                program = program,
                reason = suspended.to_string()
            ),
            Self::TimedOut { program, deadline } => tr!(
                "kiln-plugin-spawn-timed-out",
                program = program,
                deadline = format!("{deadline:?}")
            ),
            Self::Io { path, source } => tr!(
                "kiln-plugin-spawn-failed",
                path = path.display().to_string(),
                reason = source.to_string()
            ),
        })
    }
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
/// at `deadline`, which is then [`Failure::TimedOut`]; and if the setup takes it there, nothing
/// starts.
///
/// The program's stdio are sockets rather than pipes, so this thread alone can feed and drain
/// them without blocking: when it returns, by any path, the host's ends close with it and no
/// reader or writer is left behind, whatever the program, or one it started, holds open.
///
/// The program runs as a [`ProcessTree`], so killing it kills what it started too, and when this
/// returns, by any path, nothing it started is left running: a program it started that holds
/// its output open still makes the run time out, but no longer outlives it. A kill here, at the
/// cap or the deadline, has no grace period: the run is already over, and the call waits on it.
fn run(
    mut command: Command,
    stdin: Option<&[u8]>,
    max_output: usize,
    deadline: Instant,
) -> Result<Output, Failure> {
    let mut feed = match stdin {
        Some(bytes) => {
            let (feed, theirs) = StdinFeed::pair(bytes)?;
            command.stdin(theirs);
            Some(feed)
        }
        None => {
            command.stdin(Stdio::null());
            None
        }
    };
    let (stdout, theirs) = OutputDrain::pair()?;
    command.stdout(theirs);
    let (stderr, theirs) = OutputDrain::pair()?;
    command.stderr(theirs);
    #[cfg(test)]
    crate::tests::seams::during_setup();
    if Instant::now() >= deadline {
        return Err(Failure::TimedOut);
    }
    #[cfg(test)]
    crate::tests::seams::launching();
    let mut child = ProcessTree::spawn(&mut command)?;
    // The command holds the program's ends: they must close here for its output to end.
    drop(command);
    let mut stdout = OutputDrain::new(stdout)?;
    let mut stderr = OutputDrain::new(stderr)?;
    if let Some(feed) = &mut feed {
        feed.start()?;
    }
    let mut status = None;
    loop {
        let mut moved = stdout.read(max_output)? | stderr.read(max_output)?;
        if let Some(fed) = &mut feed {
            match fed.write() {
                Some(written) => moved |= written,
                // The program closed its stdin, or it is all written: either way, done with it.
                None => feed = None,
            }
        }
        if stdout.over() || stderr.over() {
            // It may have exited since it was looked at; either way what it started is killed,
            // and it is reaped.
            drop(child.kill());
            status = Some(None);
        }
        if status.is_none()
            && let Some(exited) = child.try_wait()?
        {
            status = Some(exited.code());
        }
        if let Some(status) = status
            && (stdout.ended() && stderr.ended())
        {
            return Ok(Output {
                status,
                stdout: stdout.into_kept(),
                stderr: stderr.into_kept(),
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
