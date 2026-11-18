// SPDX-License-Identifier: Apache-2.0

//! Background jobs: the `jobs` and `job-context` imports, and the queue they run from (§3.2 and
//! §3.4 of `docs/decisions/dqsjfernkt-plugins-stage-3.md`).
//!
//! A plugin with the `jobs` capability starts a job from its interactive instance. The job waits
//! in its [`JobHost`]'s queue until the concurrency limits ([`RUNNING_PER_PLUGIN`],
//! [`RUNNING_IN_ALL`]) and the memory all running jobs may hold together
//! ([`Ceilings::all_jobs_memory_bytes`]) let it start, queued jobs in order. Then the runner runs
//! it in a worker store, a second instance of the plugin without `init`, on a thread of its own:
//! [`Launch`] is what it is handed. The worker's `run-job` returns the job's result, which the
//! host keeps only until the interactive instance's `job-finished` has it.
//!
//! Every queue and buffer here is capped before it grows: [`JOBS_PER_PLUGIN`] and
//! [`JOBS_IN_ALL`] queued and running jobs, [`QUEUED_SPEC_BYTES`] of queued specs per plugin,
//! [`UNDELIVERED_JOBS`] ended jobs and [`UNDELIVERED_BYTES`] of results waiting for delivery.
//! A result that would pass the bytes is dropped, and the job delivered as failed.
//!
//! The messages a refusal carries are the API's own, the same in every locale, so a plugin can
//! tell them apart: they aren't translated.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::mem;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use kiln_command::Grants;
use kiln_command::capability::{Capability, FsScope};
use kiln_l10n::verbatim;

use crate::bindings::jobs_world::kiln::extension::{job_context, jobs as wit};
use crate::bindings::{Error, HostState, unimplemented};
use crate::limits::{BackgroundLimits, Ceilings};
use crate::paths::{self, Rules};

pub use wit::{JobId, JobKind, JobSpec, JobState, JobStatus, Progress, Waiting};

/// The most wasm a job runs without entering a host import before it is stopped: the slice the
/// call hook renews on each return from the host. Catches an infinite loop without fuel.
pub const SLICE: Duration = Duration::from_secs(5);

/// How long a job being cancelled, or past its wall time, may run on once `cancelled` turns
/// true.
pub const CANCEL_GRACE: Duration = Duration::from_secs(2);

/// How many jobs of one plugin run at once.
pub const RUNNING_PER_PLUGIN: usize = 2;

/// How many jobs of all plugins run at once.
pub const RUNNING_IN_ALL: usize = 4;

/// How many jobs of one plugin are queued and running.
pub const JOBS_PER_PLUGIN: usize = 8;

/// How many jobs of all plugins are queued and running.
pub const JOBS_IN_ALL: usize = 32;

/// The longest job name, in bytes.
pub const MAX_NAME_BYTES: usize = 64;

/// The longest job title, in characters.
pub const MAX_TITLE_CHARS: usize = 200;

/// The most bytes of a job's args.
pub const MAX_ARGS_BYTES: usize = 1 << 20;

/// The most bytes all of one plugin's queued specs hold together.
pub const QUEUED_SPEC_BYTES: usize = 8 << 20;

/// The longest result a job returns: the same as one read.
pub const MAX_RESULT_BYTES: usize = 16 << 20;

/// The most bytes of results one plugin's ended jobs hold until they are delivered.
pub const UNDELIVERED_BYTES: usize = 32 << 20;

/// How many ended jobs of one plugin wait for delivery before it may start no more.
pub const UNDELIVERED_JOBS: usize = 64;

/// The longest progress message that reaches the UI, in characters.
pub const MAX_PROGRESS_CHARS: usize = 200;

/// `failed`: past [`JOBS_PER_PLUGIN`], [`JOBS_IN_ALL`] or [`UNDELIVERED_JOBS`].
pub const TOO_MANY_JOBS: &str = verbatim!("too many jobs");
/// `failed`: past [`QUEUED_SPEC_BYTES`].
pub const QUEUED_SPECS_FULL: &str = verbatim!("too many jobs: queued specs over 8 MiB");
/// `invalid-argument`: past [`MAX_NAME_BYTES`].
pub const NAME_TOO_LONG: &str = verbatim!("job name over 64 bytes");
/// `invalid-argument`: past [`MAX_TITLE_CHARS`].
pub const TITLE_TOO_LONG: &str = verbatim!("job title over 200 characters");
/// `invalid-argument`: past [`MAX_ARGS_BYTES`].
pub const ARGS_TOO_LARGE: &str = verbatim!("job args over 1 MiB");
/// `invalid-argument`: a `root` that isn't an open root the plugin's fs grant reaches.
pub const ROOT_NOT_OPEN: &str = verbatim!("root is not an open root the fs grant reaches");
/// `not-found`: an id this load of the plugin didn't start, or already delivered.
pub const NO_SUCH_JOB: &str = verbatim!("no such job");
/// `denied`: `jobs.start` or `jobs.cancel` in a worker store.
pub const NOT_IN_A_JOB: &str = verbatim!("not in a background job");
/// `failed`: the plugin was unloaded.
pub const UNLOADED: &str = verbatim!("the plugin is unloaded");
/// A job's end when its result would pass [`UNDELIVERED_BYTES`].
pub const RESULT_DROPPED: &str = verbatim!("result dropped: too many undelivered");
/// A job's end when its result passes [`MAX_RESULT_BYTES`].
pub const RESULT_TOO_LARGE: &str = verbatim!("result over 16 MiB");
/// A job's end when it ran a [`SLICE`] of wasm without entering the host.
pub const SLICE_OVERRUN: &str = verbatim!("ran 5 s of wasm without entering the host");
/// A job's end when its worker store was refused memory.
pub const OUT_OF_MEMORY: &str = verbatim!("out of memory");
/// A load refused: a plugin that runs jobs is a binary component lifting its strings as UTF-8.
pub const NOT_UTF8: &str =
    verbatim!("a plugin that runs jobs is a binary component lifting its strings as UTF-8");
/// `failed`: an import a job was waiting in, or would wait in, once the job is being stopped.
pub const CANCELLED: &str = verbatim!("cancelled");
/// How often an import a job waits in looks at whether the job is being stopped.
pub const STOP_POLL: Duration = Duration::from_millis(10);
/// The most bytes of a failure's diagnostic a job's end keeps.
pub const MAX_DIAGNOSTIC_BYTES: usize = 4 << 10;
/// A worker store's string past its memory: a broken lift.
pub const OUT_OF_BOUNDS: &str = verbatim!("a string past the end of memory");
/// A job's end when its thread stopped without ending it.
pub const THREAD_STOPPED: &str = verbatim!("its thread stopped");

/// Why a running job is being stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// `jobs.cancel`, the Runs list, or the plugin unloading.
    Cancelled,
    /// It ran past its wall time.
    TimedOut,
}

/// What a running job and whoever stops it share: whether it is being stopped, since when, and
/// the progress it last reported.
#[derive(Clone)]
pub struct JobControl(Arc<Control>);

struct Control {
    cancelled: AtomicBool,
    /// The first stop asked for, and when.
    stop: Mutex<Option<(Stop, Instant)>>,
    progress: Mutex<Progress>,
}

impl Default for JobControl {
    fn default() -> Self {
        Self(Arc::new(Control {
            cancelled: AtomicBool::new(false),
            stop: Mutex::new(None),
            progress: Mutex::new(Progress {
                fraction: None,
                message: None,
            }),
        }))
    }
}

impl JobControl {
    /// Asks the job to stop: `job-context.cancelled` turns true now, and [`Self::past_grace`]
    /// [`CANCEL_GRACE`] later. The first stop asked for is the one the job ends with.
    pub fn stop(&self, stop: Stop) {
        lock(&self.0.stop).get_or_insert((stop, Instant::now()));
        self.0.cancelled.store(true, Ordering::Release);
    }

    /// Whether the job is being stopped.
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    /// Why the job is being stopped, if it is.
    pub fn stopping(&self) -> Option<Stop> {
        lock(&self.0.stop).map(|(stop, _)| stop)
    }

    /// Whether the job was asked to stop at least [`CANCEL_GRACE`] before `now`, so it ends
    /// whatever it is doing.
    pub fn past_grace(&self, now: Instant) -> bool {
        lock(&self.0.stop)
            .is_some_and(|(_, at)| at.checked_add(CANCEL_GRACE).is_some_and(|ends| now >= ends))
    }

    /// When the job must end, given its wall time ends at `wall_ends`: then, or once it is being
    /// stopped, [`CANCEL_GRACE`] after that. What a worker's waiting imports wait until.
    pub fn ends(&self, wall_ends: Option<Instant>) -> Option<Instant> {
        let grace_ends = (*lock(&self.0.stop)).and_then(|(_, at)| at.checked_add(CANCEL_GRACE));
        match (wall_ends, grace_ends) {
            (Some(wall), Some(grace)) => Some(wall.min(grace)),
            (wall, grace) => wall.or(grace),
        }
    }

    /// What the job last reported.
    pub fn progress(&self) -> Progress {
        lock(&self.0.progress).clone()
    }

    fn report(&self, mut progress: Progress) {
        // A bounded copy: truncating would keep the whole of a long message's allocation.
        if let Some(message) = &mut progress.message
            && let Some((cut, _)) = message.char_indices().nth(MAX_PROGRESS_CHARS)
        {
            *message = message[..cut].to_owned();
        }
        progress.fraction = progress.fraction.filter(|fraction| fraction.is_finite());
        *lock(&self.0.progress) = progress;
    }
}

/// How a job ended, kept until it is delivered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ended {
    /// `run-job` returned this, or the job failed with it.
    Finished(Result<String, Error>),
    Cancelled,
    TimedOut,
}

impl Ended {
    /// Failed with `message`.
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Finished(Err(Error::Failed(message.into())))
    }

    /// As `job-finished` is told it.
    pub fn into_state(self) -> JobState {
        match self {
            Self::Finished(result) => JobState::Finished(result),
            Self::Cancelled => JobState::Cancelled,
            Self::TimedOut => JobState::TimedOut,
        }
    }

    fn status(&self) -> JobStatus {
        match self {
            Self::Finished(Ok(_)) => JobStatus::Succeeded,
            Self::Finished(Err(_)) => JobStatus::Failed,
            Self::Cancelled => JobStatus::Cancelled,
            Self::TimedOut => JobStatus::TimedOut,
        }
    }

    /// The bytes it holds: its result's, or its error's message.
    fn bytes(&self) -> usize {
        match self {
            Self::Finished(Ok(result)) => result.len(),
            Self::Finished(Err(error)) => match error {
                Error::Unimplemented(message)
                | Error::Denied(message)
                | Error::NotFound(message)
                | Error::InvalidArgument(message)
                | Error::Conflict(message)
                | Error::Failed(message) => message.len(),
            },
            Self::Cancelled | Self::TimedOut => 0,
        }
    }
}

/// A job to run now, as the runner is handed it: what `run-job` is told, the limits it runs
/// under, and the [`JobControl`] it watches. [`Launch::finish`] ends it; dropped unfinished, it
/// ends failed.
pub struct Launch {
    pub job: JobId,
    pub plugin: String,
    pub name: String,
    pub args: String,
    pub root: Option<String>,
    /// The worker store's memory cap.
    pub memory_bytes: u64,
    /// How long it runs before it is stopped as timed out.
    pub wall: Duration,
    /// The roots open as it starts, to preopen as the plugin's grants allow.
    pub roots: Vec<PathBuf>,
    pub control: JobControl,
    ending: Option<Ending>,
}

/// Where a [`Launch`]'s end goes.
struct Ending {
    host: JobHost,
    key: u64,
    memory_bytes: u64,
    /// What its [`Reservation`]s charged against [`UNDELIVERED_BYTES`].
    reserved: Arc<AtomicUsize>,
}

/// Charges a job's result against its plugin's undelivered results before the result is copied
/// out of the worker; [`Launch::finish`] keeps the result under the charge.
#[derive(Clone)]
pub struct Reservation {
    host: JobHost,
    key: u64,
    reserved: Arc<AtomicUsize>,
}

impl Reservation {
    /// Charges `bytes`: false, charging nothing, when they won't fit, or past
    /// [`MAX_RESULT_BYTES`].
    pub fn reserve(&self, bytes: usize) -> bool {
        if bytes > MAX_RESULT_BYTES {
            return false;
        }
        let mut table = self.host.table();
        let Some(plugin) = table.plugins.get_mut(&self.key) else {
            return false;
        };
        if plugin.undelivered_bytes + bytes > UNDELIVERED_BYTES {
            return false;
        }
        plugin.undelivered_bytes += bytes;
        self.reserved.fetch_add(bytes, Ordering::AcqRel);
        true
    }
}

impl Launch {
    /// What charges its result: none once it has ended.
    pub fn reservation(&self) -> Option<Reservation> {
        let ending = self.ending.as_ref()?;
        Some(Reservation {
            host: ending.host.clone(),
            key: ending.key,
            reserved: Arc::clone(&ending.reserved),
        })
    }

    /// Ends the job as `ended`: its slot and memory go back to the queue, and its end waits for
    /// delivery, unless the plugin was unloaded meanwhile.
    pub fn finish(mut self, ended: Ended) {
        if let Some(ending) = self.ending.take() {
            ending.host.finish(&ending, self.job, ended);
        }
    }
}

impl Drop for Launch {
    fn drop(&mut self) {
        if let Some(ending) = self.ending.take() {
            ending
                .host
                .finish(&ending, self.job, Ended::failed(THREAD_STOPPED));
        }
    }
}

/// Starts a worker for each [`Launch`]: the runner's, for one plugin. It must not block.
pub type Launcher = Arc<dyn Fn(Launch) + Send + Sync>;

/// Makes the [`HostState`] a worker store's imports run against, before the runner sets its
/// jobs slice.
pub type WorkerHost = Arc<dyn Fn() -> HostState + Send + Sync>;

/// Hears the id of a plugin whose job has ended, so its end waits for [`Jobs::take_ended`].
pub type OnEnded = Arc<dyn Fn(&str) + Send + Sync>;

/// The queue every plugin's jobs run from: one per app, holding the limits across plugins.
#[derive(Clone)]
pub struct JobHost(Arc<Shared>);

struct Shared {
    ceilings: Ceilings,
    table: Mutex<Table>,
    on_ended: Mutex<Option<OnEnded>>,
}

/// How many jobs a [`JobHost`] holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub queued: usize,
    pub running: usize,
}

#[derive(Default)]
struct Table {
    next_job: JobId,
    next_key: u64,
    next_seq: u64,
    plugins: HashMap<u64, PluginJobs>,
    roots: Vec<PathBuf>,
    /// Queued and running jobs of every plugin, those of unloaded plugins still running among
    /// them.
    jobs: usize,
    running: usize,
    running_memory: u64,
}

/// One load of a plugin's jobs.
struct PluginJobs {
    plugin: String,
    limits: BackgroundLimits,
    /// Whether its grants reach the open roots, so a job may name one.
    reaches_roots: bool,
    launcher: Launcher,
    jobs: BTreeMap<JobId, Job>,
    /// Queued and running.
    active: usize,
    running: usize,
    queued_bytes: usize,
    /// Ended jobs, oldest first.
    undelivered: VecDeque<JobId>,
    undelivered_bytes: usize,
}

struct Job {
    /// Its place in the queue of every plugin's jobs.
    seq: u64,
    spec: JobSpec,
    phase: Phase,
}

enum Phase {
    Queued(Waiting),
    Running(JobControl),
    /// With the bytes charged for it against [`UNDELIVERED_BYTES`].
    Ended(Ended, usize),
}

impl JobHost {
    /// A queue under `ceilings`, with no open roots.
    pub fn new(ceilings: Ceilings) -> Self {
        Self(Arc::new(Shared {
            ceilings,
            table: Mutex::new(Table {
                next_job: 1,
                ..Table::default()
            }),
            on_ended: Mutex::new(None),
        }))
    }

    /// The process-wide queue, under this computer's [`Ceilings::host`].
    pub fn shared() -> &'static Self {
        static SHARED: OnceLock<JobHost> = OnceLock::new();
        SHARED.get_or_init(|| Self::new(Ceilings::host()))
    }

    pub fn ceilings(&self) -> Ceilings {
        self.0.ceilings
    }

    /// The roots of the open windows, as host paths: a job's `root` must be one of them.
    pub fn set_roots(&self, roots: Vec<PathBuf>) {
        self.table().roots = roots;
    }

    /// Calls `on_ended` with a plugin's id each time one of its jobs ends, on the thread that
    /// ended it.
    pub fn on_ended(&self, on_ended: OnEnded) {
        *lock(&self.0.on_ended) = Some(on_ended);
    }

    /// The jobs queued and running now, of every plugin.
    pub fn counts(&self) -> Counts {
        let table = self.table();
        Counts {
            queued: table.jobs - table.running,
            running: table.running,
        }
    }

    /// Takes plugin `plugin`'s jobs, under `limits` held to the ceilings, each run by
    /// `launcher`. Its jobs may name an open root only if `grants` reach the workspace.
    pub fn attach(
        &self,
        plugin: &str,
        grants: &Grants,
        limits: BackgroundLimits,
        launcher: Launcher,
    ) -> Jobs {
        let (limits, _) = self.0.ceilings.clamp(limits);
        let reaches_roots = grants.allows(&Capability::FsRead(FsScope::Workspace))
            || grants.allows(&Capability::FsWrite(FsScope::Workspace));
        let mut table = self.table();
        let key = table.next_key;
        table.next_key += 1;
        table.plugins.insert(
            key,
            PluginJobs {
                plugin: plugin.to_owned(),
                limits,
                reaches_roots,
                launcher,
                jobs: BTreeMap::new(),
                active: 0,
                running: 0,
                queued_bytes: 0,
                undelivered: VecDeque::new(),
                undelivered_bytes: 0,
            },
        );
        Jobs {
            host: self.clone(),
            key,
        }
    }

    fn table(&self) -> MutexGuard<'_, Table> {
        lock(&self.0.table)
    }

    /// Ends `job` of `ending`'s plugin as `ended`, charging its result before keeping it.
    fn finish(&self, ending: &Ending, job: JobId, ended: Ended) {
        let mut table = self.table();
        table.jobs -= 1;
        table.running -= 1;
        table.running_memory -= ending.memory_bytes;
        let mut notify = None;
        if let Some(plugin) = table.plugins.get_mut(&ending.key) {
            plugin.active -= 1;
            plugin.running -= 1;
            // What was reserved is charged again below, as the result's own size. The table's
            // lock is held, so no reservation comes between.
            plugin.undelivered_bytes -= ending.reserved.load(Ordering::Acquire);
            if let Some(entry) = plugin.jobs.get_mut(&job) {
                // Quota before growth: a result that won't fit is never kept, nor one past the
                // limit of a result, whatever ended the job.
                let ended = if ended.bytes() > MAX_RESULT_BYTES {
                    Ended::failed(RESULT_TOO_LARGE)
                } else {
                    ended
                };
                let bytes = ended.bytes();
                let (ended, charged) = if plugin.undelivered_bytes + bytes > UNDELIVERED_BYTES {
                    (Ended::failed(RESULT_DROPPED), 0)
                } else {
                    (ended, bytes)
                };
                plugin.undelivered_bytes += charged;
                entry.phase = Phase::Ended(ended, charged);
                plugin.undelivered.push_back(job);
                notify = Some(plugin.plugin.clone());
            }
        }
        let scheduled = table.schedule(self);
        drop(table);
        scheduled.run(self);
        if let Some(plugin) = notify {
            self.notify(&plugin);
        }
    }

    fn notify(&self, plugin: &str) {
        let on_ended = lock(&self.0.on_ended).clone();
        if let Some(on_ended) = on_ended {
            on_ended(plugin);
        }
    }
}

impl Table {
    /// Starts every queued job the limits let start, in queue order, and marks what each of
    /// the rest waits for. A job held back by the limits across plugins holds back every job
    /// after it, so a large one isn't passed over forever.
    fn schedule(&mut self, host: &JobHost) -> Scheduled {
        let mut queued: Vec<(u64, u64, JobId)> = (self.plugins.iter())
            .flat_map(|(&key, plugin)| {
                (plugin.jobs.iter())
                    .filter(|(_, job)| matches!(job.phase, Phase::Queued(_)))
                    .map(move |(&id, job)| (job.seq, key, id))
            })
            .collect();
        queued.sort_unstable();
        let all_jobs_memory = host.0.ceilings.all_jobs_memory_bytes;
        let mut held = None;
        let mut scheduled = Scheduled::default();
        for (_, key, id) in queued {
            let Some(plugin) = self.plugins.get_mut(&key) else {
                continue;
            };
            let memory_bytes = plugin.limits.memory_bytes;
            let waiting = if held.is_some() {
                held
            } else if plugin.running >= RUNNING_PER_PLUGIN {
                Some(Waiting::Concurrency)
            } else if self.running >= RUNNING_IN_ALL {
                held = Some(Waiting::Concurrency);
                held
            } else if self.running > 0
                && self.running_memory.saturating_add(memory_bytes) > all_jobs_memory
            {
                held = Some(Waiting::Memory);
                held
            } else {
                None
            };
            let Some(job) = plugin.jobs.get_mut(&id) else {
                continue;
            };
            if let Some(waiting) = waiting {
                job.phase = Phase::Queued(waiting);
                continue;
            }
            plugin.queued_bytes -= spec_bytes(&job.spec);
            // Its root may have closed while it waited: then it ends without starting.
            if let Some(root) = &job.spec.root
                && !(plugin.reaches_roots && is_open_root(root, &self.roots))
            {
                job.spec.drop_payload();
                plugin.active -= 1;
                self.jobs -= 1;
                let ended = Ended::failed(ROOT_NOT_OPEN);
                let charged = ended.bytes();
                let (ended, charged) = if plugin.undelivered_bytes + charged > UNDELIVERED_BYTES {
                    (Ended::failed(RESULT_DROPPED), 0)
                } else {
                    (ended, charged)
                };
                plugin.undelivered_bytes += charged;
                job.phase = Phase::Ended(ended, charged);
                plugin.undelivered.push_back(id);
                scheduled.ended.push(plugin.plugin.clone());
                continue;
            }
            let control = JobControl::default();
            job.phase = Phase::Running(control.clone());
            plugin.running += 1;
            self.running += 1;
            self.running_memory += memory_bytes;
            let launch = Launch {
                job: id,
                plugin: plugin.plugin.clone(),
                name: mem::take(&mut job.spec.name),
                args: mem::take(&mut job.spec.args),
                root: job.spec.root.take(),
                memory_bytes,
                wall: plugin.limits.wall,
                roots: self.roots.clone(),
                control,
                ending: Some(Ending {
                    host: host.clone(),
                    key,
                    memory_bytes,
                    reserved: Arc::default(),
                }),
            };
            scheduled
                .launches
                .push((Arc::clone(&plugin.launcher), launch));
        }
        scheduled
    }
}

/// What [`Table::schedule`] did: the jobs to launch, and the plugins whose jobs it ended.
#[derive(Default)]
struct Scheduled {
    launches: Vec<(Launcher, Launch)>,
    ended: Vec<String>,
}

impl Scheduled {
    /// Hands each launch to its launcher and tells of each ended job: outside the table's lock.
    fn run(self, host: &JobHost) {
        for (launcher, launch) in self.launches {
            launcher(launch);
        }
        for plugin in self.ended {
            host.notify(&plugin);
        }
    }
}

impl JobSpec {
    /// Drops what only `run-job` needed, keeping what the Runs list shows.
    fn drop_payload(&mut self) {
        self.name = String::new();
        self.args = String::new();
        self.root = None;
    }
}

/// The bytes a queued spec holds.
fn spec_bytes(spec: &JobSpec) -> usize {
    spec.name.len() + spec.args.len() + spec.title.len() + spec.root.as_ref().map_or(0, String::len)
}

/// One load of a plugin's jobs, on its [`JobHost`]: what its imports and the app reach them by.
/// [`Self::detach`] ends them.
#[derive(Clone)]
pub struct Jobs {
    host: JobHost,
    key: u64,
}

impl Jobs {
    /// Queues a job, checked against the spec and queue limits first; nothing is queued when it
    /// is refused.
    pub fn start(&self, spec: JobSpec) -> Result<JobId, Error> {
        let invalid = |message: &str| Err(Error::InvalidArgument(message.to_owned()));
        if spec.name.len() > MAX_NAME_BYTES {
            return invalid(NAME_TOO_LONG);
        }
        if spec.title.chars().count() > MAX_TITLE_CHARS {
            return invalid(TITLE_TOO_LONG);
        }
        if spec.args.len() > MAX_ARGS_BYTES {
            return invalid(ARGS_TOO_LARGE);
        }
        let bytes = spec_bytes(&spec);
        let mut table = self.host.table();
        let Table {
            plugins,
            roots,
            jobs,
            next_job,
            next_seq,
            ..
        } = &mut *table;
        let Some(plugin) = plugins.get_mut(&self.key) else {
            return Err(Error::Failed(UNLOADED.to_owned()));
        };
        if let Some(root) = &spec.root
            && !(plugin.reaches_roots && is_open_root(root, roots))
        {
            return invalid(ROOT_NOT_OPEN);
        }
        if plugin.active >= JOBS_PER_PLUGIN
            || *jobs >= JOBS_IN_ALL
            || plugin.undelivered.len() >= UNDELIVERED_JOBS
        {
            return Err(Error::Failed(TOO_MANY_JOBS.to_owned()));
        }
        if plugin.queued_bytes + bytes > QUEUED_SPEC_BYTES {
            return Err(Error::Failed(QUEUED_SPECS_FULL.to_owned()));
        }
        let id = *next_job;
        *next_job += 1;
        let seq = *next_seq;
        *next_seq += 1;
        plugin.jobs.insert(
            id,
            Job {
                seq,
                spec,
                phase: Phase::Queued(Waiting::Concurrency),
            },
        );
        plugin.active += 1;
        plugin.queued_bytes += bytes;
        *jobs += 1;
        let scheduled = table.schedule(&self.host);
        drop(table);
        scheduled.run(&self.host);
        Ok(id)
    }

    /// Cancels `job`: a queued one ends without starting, a running one is stopped (its
    /// `cancelled` turns true, and [`CANCEL_GRACE`] later it ends whatever it is doing). One
    /// that has ended already is left as it ended.
    pub fn cancel(&self, job: JobId) -> Result<(), Error> {
        let mut table = self.host.table();
        let not_found = || Err(Error::NotFound(NO_SUCH_JOB.to_owned()));
        let Table { plugins, jobs, .. } = &mut *table;
        let Some(plugin) = plugins.get_mut(&self.key) else {
            return not_found();
        };
        let Some(entry) = plugin.jobs.get_mut(&job) else {
            return not_found();
        };
        match &entry.phase {
            Phase::Queued(_) => {}
            Phase::Running(control) => {
                control.stop(Stop::Cancelled);
                return Ok(());
            }
            Phase::Ended(..) => return Ok(()),
        }
        plugin.queued_bytes -= spec_bytes(&entry.spec);
        entry.spec.drop_payload();
        plugin.active -= 1;
        *jobs -= 1;
        entry.phase = Phase::Ended(Ended::Cancelled, 0);
        plugin.undelivered.push_back(job);
        let plugin = plugin.plugin.clone();
        let scheduled = table.schedule(&self.host);
        drop(table);
        scheduled.run(&self.host);
        self.host.notify(&plugin);
        Ok(())
    }

    /// Where `job` is: `unknown` once its end has been delivered, or for an id this load of the
    /// plugin didn't start.
    pub fn state(&self, job: JobId) -> JobStatus {
        let table = self.host.table();
        let phase = (table.plugins.get(&self.key))
            .and_then(|plugin| plugin.jobs.get(&job))
            .map(|job| &job.phase);
        match phase {
            Some(Phase::Queued(waiting)) => JobStatus::Queued(*waiting),
            Some(Phase::Running(control)) => JobStatus::Running(control.progress()),
            Some(Phase::Ended(ended, _)) => ended.status(),
            None => JobStatus::Unknown,
        }
    }

    /// The job that ended first and hasn't been delivered, taken out: from here it is the
    /// caller's to deliver, and the host holds nothing of it.
    pub fn take_ended(&self) -> Option<(JobId, Ended)> {
        let mut table = self.host.table();
        let plugin = table.plugins.get_mut(&self.key)?;
        while let Some(job) = plugin.undelivered.pop_front() {
            if let Some(Job {
                phase: Phase::Ended(ended, charged),
                ..
            }) = plugin.jobs.remove(&job)
            {
                plugin.undelivered_bytes -= charged;
                return Some((job, ended));
            }
        }
        None
    }

    /// How many ended jobs wait for delivery.
    pub fn undelivered(&self) -> usize {
        let table = self.host.table();
        (table.plugins.get(&self.key)).map_or(0, |plugin| plugin.undelivered.len())
    }

    /// The bytes of results waiting for delivery.
    pub fn undelivered_bytes(&self) -> usize {
        let table = self.host.table();
        (table.plugins.get(&self.key)).map_or(0, |plugin| plugin.undelivered_bytes)
    }

    /// Every job this load holds: queued, running and ended but not delivered.
    pub fn ids(&self) -> Vec<JobId> {
        let table = self.host.table();
        (table.plugins.get(&self.key))
            .map(|plugin| plugin.jobs.keys().copied().collect())
            .unwrap_or_default()
    }

    /// Ends this load's jobs, as unloading the plugin does: running ones are stopped as
    /// cancelled, queued ones removed, and undelivered ends dropped. Every id is `unknown`
    /// afterwards.
    pub fn detach(&self) {
        let mut table = self.host.table();
        let Some(plugin) = table.plugins.remove(&self.key) else {
            return;
        };
        for job in plugin.jobs.into_values() {
            match job.phase {
                Phase::Queued(_) => table.jobs -= 1,
                // Its slot goes back as its worker ends.
                Phase::Running(control) => control.stop(Stop::Cancelled),
                Phase::Ended(..) => {}
            }
        }
        let scheduled = table.schedule(&self.host);
        drop(table);
        scheduled.run(&self.host);
    }
}

/// Whether guest path `root` is the guest path of one of the open `roots`.
fn is_open_root(root: &str, roots: &[PathBuf]) -> bool {
    let root = root.trim_end_matches('/');
    roots.iter().any(|open| {
        (open.to_str())
            .and_then(|open| paths::root_to_guest(open, Rules::HOST).ok())
            .is_some_and(|open| open.trim_end_matches('/') == root)
    })
}

/// The jobs slice of [`HostState`]: the queue the plugin's jobs go to, its background limits,
/// and, once the runner has loaded it with the `jobs` capability, its [`Jobs`]. In a worker
/// store, the job it runs instead.
#[derive(Default)]
pub struct State {
    host: Option<JobHost>,
    limits: BackgroundLimits,
    workers: Option<WorkerHost>,
    attached: Option<Jobs>,
    worker: Option<JobControl>,
}

impl State {
    /// Jobs on `host` under `limits`. Without this, jobs go to [`JobHost::shared`] under the
    /// default limits.
    pub fn new(host: JobHost, limits: BackgroundLimits) -> Self {
        Self {
            host: Some(host),
            limits,
            ..Self::default()
        }
    }

    /// Runs each worker store's imports against what `workers` makes. Without this, a worker's
    /// imports run against [`HostState::new`] logging to the `log` crate.
    pub fn with_workers(mut self, workers: WorkerHost) -> Self {
        self.workers = Some(workers);
        self
    }

    /// A worker store's slice, running the job `control` watches.
    pub fn worker(control: JobControl) -> Self {
        Self {
            worker: Some(control),
            ..Self::default()
        }
    }

    pub fn host(&self) -> JobHost {
        self.host
            .clone()
            .unwrap_or_else(|| JobHost::shared().clone())
    }

    pub fn limits(&self) -> BackgroundLimits {
        self.limits
    }

    pub fn workers(&self) -> Option<WorkerHost> {
        self.workers.clone()
    }

    /// In a worker store, the job it runs: what its waiting imports watch.
    pub fn control(&self) -> Option<&JobControl> {
        self.worker.as_ref()
    }

    /// The plugin's jobs, once it is loaded with the `jobs` capability.
    pub fn jobs(&self) -> Option<&Jobs> {
        self.attached.as_ref()
    }

    pub(crate) fn attach(&mut self, jobs: Jobs) {
        self.attached = Some(jobs);
    }
}

impl wit::Host for HostState {
    fn start(&mut self, spec: JobSpec) -> Result<JobId, Error> {
        if self.jobs.worker.is_some() {
            return Err(Error::Denied(NOT_IN_A_JOB.to_owned()));
        }
        match &self.jobs.attached {
            Some(jobs) => jobs.start(spec),
            None => unimplemented("kiln:extension/jobs.start"),
        }
    }

    fn cancel(&mut self, job: JobId) -> Result<(), Error> {
        if self.jobs.worker.is_some() {
            return Err(Error::Denied(NOT_IN_A_JOB.to_owned()));
        }
        match &self.jobs.attached {
            Some(jobs) => jobs.cancel(job),
            None => unimplemented("kiln:extension/jobs.cancel"),
        }
    }

    fn state(&mut self, job: JobId) -> JobStatus {
        (self.jobs.attached.as_ref()).map_or(JobStatus::Unknown, |jobs| jobs.state(job))
    }
}

impl job_context::Host for HostState {
    fn report(&mut self, progress: Progress) {
        if let Some(control) = &self.jobs.worker {
            control.report(progress);
        }
    }

    fn cancelled(&mut self) -> bool {
        self.jobs
            .worker
            .as_ref()
            .is_some_and(JobControl::is_cancelled)
    }
}

/// Whether `component` is a binary component every function of which lifts its strings as
/// UTF-8, as a plugin that runs jobs must: a job's result is then read in place, checked and
/// charged before the host copies it, where UTF-16 would be transcoded into a host string first.
pub fn lifts_utf8(component: &[u8]) -> bool {
    use wasmparser::{CanonicalFunction, CanonicalOption, Payload};
    if !wasmparser::Parser::is_component(component) {
        return false;
    }
    for payload in wasmparser::Parser::new(0).parse_all(component) {
        let Ok(payload) = payload else {
            return false;
        };
        let Payload::ComponentCanonicalSection(section) = payload else {
            continue;
        };
        for function in section {
            let Ok(function) = function else {
                return false;
            };
            if let CanonicalFunction::Lift { options, .. } = function
                && options.iter().any(|option| {
                    matches!(
                        option,
                        CanonicalOption::UTF16 | CanonicalOption::CompactUTF16
                    )
                })
            {
                return false;
            }
        }
    }
    true
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A launcher that keeps each launch, running nothing, so a test ends each when it likes.
    fn holding() -> (Launcher, Arc<Mutex<Vec<Launch>>>) {
        let held = Arc::new(Mutex::new(Vec::new()));
        let keep = Arc::clone(&held);
        let launcher: Launcher = Arc::new(move |launch| lock(&keep).push(launch));
        (launcher, held)
    }

    fn spec(args: usize, title: usize) -> JobSpec {
        JobSpec {
            name: "n".to_owned(),
            args: "a".repeat(args),
            title: "t".repeat(title),
            kind: JobKind::Other,
            cancellable: true,
            root: None,
        }
    }

    /// A plugin on `host` whose jobs each hold `memory_bytes`, and what it launched.
    fn plugin(host: &JobHost, memory_bytes: u64) -> (Jobs, Arc<Mutex<Vec<Launch>>>) {
        let (launcher, held) = holding();
        let limits = BackgroundLimits {
            memory_bytes,
            ..BackgroundLimits::default()
        };
        (host.attach("p", &Grants::new(), limits, launcher), held)
    }

    /// Ends the launch of `job` as finished, outside the lock its launcher takes.
    fn end(held: &Mutex<Vec<Launch>>, job: JobId) {
        let launch = {
            let mut held = lock(held);
            let at = held.iter().position(|launch| launch.job == job).unwrap();
            held.remove(at)
        };
        launch.finish(Ended::Finished(Ok(String::new())));
    }

    #[test]
    fn a_job_past_the_memory_of_all_running_jobs_waits_and_holds_back_those_after_it() {
        // 2 GiB of RAM: 512 MiB per job, 1 GiB for all running jobs together.
        let host = JobHost::new(Ceilings::for_ram(2 << 30));
        let (a, a_held) = plugin(&host, 512 << 20);
        let (b, b_held) = plugin(&host, 512 << 20);
        let (c, _) = plugin(&host, 1 << 20);
        let a1 = a.start(spec(0, 1)).unwrap();
        let b1 = b.start(spec(0, 1)).unwrap();
        let a2 = a.start(spec(0, 1)).unwrap();
        // Small enough to fit, but queued after a job that waits.
        let c1 = c.start(spec(0, 1)).unwrap();
        assert!(matches!(a.state(a1), JobStatus::Running(_)));
        assert!(matches!(b.state(b1), JobStatus::Running(_)));
        assert_eq!(a.state(a2), JobStatus::Queued(Waiting::Memory));
        assert_eq!(c.state(c1), JobStatus::Queued(Waiting::Memory));

        end(&a_held, a1);
        assert!(matches!(a.state(a2), JobStatus::Running(_)));
        // `b1` and `a2` hold the 1 GiB.
        assert_eq!(c.state(c1), JobStatus::Queued(Waiting::Memory));
        end(&b_held, b1);
        assert!(matches!(c.state(c1), JobStatus::Running(_)));
        assert_eq!(
            host.counts(),
            Counts {
                queued: 0,
                running: 2
            }
        );
    }

    #[test]
    fn four_jobs_run_in_all_and_two_per_plugin() {
        let host = JobHost::new(Ceilings::for_ram(64 << 30));
        let plugins: Vec<_> = (0..3).map(|_| plugin(&host, 1 << 20)).collect();
        let ids: Vec<Vec<JobId>> = (plugins.iter())
            .map(|(jobs, _)| (0..3).map(|_| jobs.start(spec(0, 1)).unwrap()).collect())
            .collect();
        let status = |plugin: usize, job: usize| plugins[plugin].0.state(ids[plugin][job]);
        // In order: the first plugin's two, then the second's two, then all slots are taken.
        assert!(matches!(status(0, 0), JobStatus::Running(_)));
        assert!(matches!(status(0, 1), JobStatus::Running(_)));
        assert_eq!(status(0, 2), JobStatus::Queued(Waiting::Concurrency));
        assert!(matches!(status(1, 0), JobStatus::Running(_)));
        assert!(matches!(status(1, 1), JobStatus::Running(_)));
        assert_eq!(status(2, 0), JobStatus::Queued(Waiting::Concurrency));
        assert_eq!(
            host.counts(),
            Counts {
                queued: 5,
                running: 4
            }
        );
        // A slot freed goes to the oldest queued job of a plugin with room.
        end(&plugins[1].1, ids[1][0]);
        assert_eq!(status(0, 2), JobStatus::Queued(Waiting::Concurrency));
        assert!(matches!(status(1, 2), JobStatus::Running(_)));
        assert_eq!(status(2, 0), JobStatus::Queued(Waiting::Concurrency));
    }

    #[test]
    fn thirty_two_jobs_in_all() {
        let host = JobHost::new(Ceilings::for_ram(64 << 30));
        let plugins: Vec<_> = (0..5).map(|_| plugin(&host, 1 << 20)).collect();
        let mut started = 0;
        for (jobs, _) in &plugins {
            while started < JOBS_IN_ALL && jobs.ids().len() < JOBS_PER_PLUGIN {
                jobs.start(spec(0, 1)).unwrap();
                started += 1;
            }
        }
        let last = &plugins[4].0;
        assert_eq!(last.ids().len(), 0);
        assert_eq!(
            last.start(spec(0, 1)),
            Err(Error::Failed(TOO_MANY_JOBS.to_owned()))
        );
        assert_eq!(host.counts().queued + host.counts().running, JOBS_IN_ALL);
    }

    #[test]
    fn queued_specs_over_8_mib_are_refused() {
        let host = JobHost::new(Ceilings::for_ram(64 << 30));
        // Two plugins take every running slot, so the third's jobs all queue.
        let busy: Vec<_> = (0..2).map(|_| plugin(&host, 1 << 20)).collect();
        for (jobs, _) in &busy {
            jobs.start(spec(0, 1)).unwrap();
            jobs.start(spec(0, 1)).unwrap();
        }
        let (jobs, _) = plugin(&host, 1 << 20);
        let large = || spec(MAX_ARGS_BYTES, MAX_TITLE_CHARS);
        for _ in 0..7 {
            jobs.start(large()).unwrap();
        }
        assert_eq!(
            jobs.start(large()),
            Err(Error::Failed(QUEUED_SPECS_FULL.to_owned()))
        );
        assert_eq!(jobs.ids().len(), 7);
        // A small one still fits.
        jobs.start(spec(0, 1)).unwrap();
    }

    #[test]
    fn progress_is_held_to_its_limits() {
        let control = JobControl::default();
        control.report(Progress {
            fraction: Some(f32::NAN),
            message: Some("é".repeat(MAX_PROGRESS_CHARS + 50)),
        });
        let progress = control.progress();
        assert_eq!(progress.fraction, None);
        let message = lock(&control.0.progress).message.clone().unwrap();
        assert_eq!(message.chars().count(), MAX_PROGRESS_CHARS);
        // Nothing of the long message's allocation is kept.
        let kept = lock(&control.0.progress)
            .message
            .as_ref()
            .unwrap()
            .capacity();
        assert!(kept <= MAX_PROGRESS_CHARS * 4, "{kept}");
    }

    /// A plugin on `host` that reaches the open roots, whose jobs each hold 1 MiB.
    fn rooted(host: &JobHost) -> (Jobs, Arc<Mutex<Vec<Launch>>>) {
        let (launcher, held) = holding();
        let grants: Grants = [Capability::FsRead(FsScope::Workspace)]
            .into_iter()
            .collect();
        let limits = BackgroundLimits {
            memory_bytes: 1 << 20,
            ..BackgroundLimits::default()
        };
        (host.attach("p", &grants, limits, launcher), held)
    }

    #[test]
    fn a_queued_job_whose_root_closed_ends_without_starting() {
        let host = JobHost::new(Ceilings::for_ram(64 << 30));
        let root = std::env::temp_dir();
        let guest = paths::root_to_guest(root.to_str().unwrap(), Rules::HOST).unwrap();
        host.set_roots(vec![root.clone()]);
        let (jobs, held) = rooted(&host);
        let running = [
            jobs.start(spec(0, 1)).unwrap(),
            jobs.start(spec(0, 1)).unwrap(),
        ];
        let queued = jobs
            .start(JobSpec {
                root: Some(guest),
                ..spec(0, 1)
            })
            .unwrap();
        assert_eq!(jobs.state(queued), JobStatus::Queued(Waiting::Concurrency));

        host.set_roots(Vec::new());
        end(&held, running[0]);
        assert_eq!(jobs.state(queued), JobStatus::Failed);
        assert!(lock(&held).iter().all(|launch| launch.job != queued));
        assert_eq!(jobs.take_ended().map(|(job, _)| job), Some(running[0]));
        assert_eq!(
            jobs.take_ended(),
            Some((queued, Ended::failed(ROOT_NOT_OPEN)))
        );
    }

    #[test]
    fn a_cancelled_queued_job_keeps_none_of_its_payload() {
        let host = JobHost::new(Ceilings::for_ram(64 << 30));
        let (jobs, _held) = plugin(&host, 1 << 20);
        jobs.start(spec(0, 1)).unwrap();
        jobs.start(spec(0, 1)).unwrap();
        for _ in 0..UNDELIVERED_JOBS - 2 {
            let job = jobs.start(spec(MAX_ARGS_BYTES, 1)).unwrap();
            jobs.cancel(job).unwrap();
        }
        let table = host.table();
        let held: usize = (table.plugins.values())
            .flat_map(|plugin| plugin.jobs.values())
            .map(|job| spec_bytes(&job.spec))
            .sum();
        // The titles alone.
        assert!(held < 1024, "{held}");
    }

    #[test]
    fn a_reserved_result_is_charged_before_it_is_kept() {
        let host = JobHost::new(Ceilings::for_ram(64 << 30));
        let (jobs, held) = plugin(&host, 1 << 20);
        let first = jobs.start(spec(0, 1)).unwrap();
        let second = jobs.start(spec(0, 1)).unwrap();
        let mut launches = mem::take(&mut *lock(&held));
        let reservations: Vec<_> = (launches.iter())
            .map(|launch| launch.reservation().unwrap())
            .collect();
        assert!(reservations[0].reserve(MAX_RESULT_BYTES));
        assert_eq!(jobs.undelivered_bytes(), MAX_RESULT_BYTES);
        assert!(!reservations[1].reserve(MAX_RESULT_BYTES + 1));
        assert!(reservations[1].reserve(MAX_RESULT_BYTES));
        // Full: a third reservation fails, charging nothing.
        assert!(!reservations[1].reserve(1));
        assert_eq!(jobs.undelivered_bytes(), UNDELIVERED_BYTES);
        let second_launch = launches.pop().unwrap();
        let first_launch = launches.pop().unwrap();
        first_launch.finish(Ended::Finished(Ok("x".repeat(10))));
        // Charged as what it kept.
        assert_eq!(jobs.undelivered_bytes(), MAX_RESULT_BYTES + 10);
        second_launch.finish(Ended::Finished(Ok(String::new())));
        assert_eq!(jobs.undelivered_bytes(), 10);
        assert_eq!(jobs.state(first), JobStatus::Succeeded);
        assert_eq!(jobs.state(second), JobStatus::Succeeded);
    }

    #[test]
    fn a_launch_dropped_unfinished_ends_failed() {
        let host = JobHost::new(Ceilings::for_ram(64 << 30));
        let (jobs, held) = plugin(&host, 1 << 20);
        let job = jobs.start(spec(0, 1)).unwrap();
        drop(mem::take(&mut *lock(&held)));
        assert_eq!(jobs.state(job), JobStatus::Failed);
        assert_eq!(
            jobs.take_ended(),
            Some((job, Ended::failed(THREAD_STOPPED)))
        );
        assert_eq!(host.counts(), Counts::default());
    }
}
