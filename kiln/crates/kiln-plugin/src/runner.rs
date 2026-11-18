// SPDX-License-Identifier: Apache-2.0

//! Running a plugin: each in its own `Store`, owned by its own OS thread, so guest code never
//! runs on the caller's thread.
//!
//! [`Plugin::load`] compiles the component, starts the thread, instantiates the world and calls
//! `init`; [`Plugin::call`] sends the thread a typed request and waits for its reply. Every call
//! into the guest runs under the plugin's [`Limits`]: a deadline and a memory cap. The engine's
//! epoch interrupts guest code past its deadline, and the call itself is raced against it, so a
//! host import the guest blocks in (a WASI pollable, say) is cancelled too. A call that traps,
//! overruns, runs out of memory or answers out of protocol unloads the plugin: its store and
//! thread are dropped, it moves to [`PluginState::Unloaded`] with a [`Diagnostic`], and every
//! later call is refused without running anything.
//!
//! A plugin with the `jobs` capability also links the jobs interfaces and is checked against the
//! `jobs-extension` world. Its jobs run in worker stores: each a second instance of its
//! component, never `init`ed, on a thread of its own, under the job's wall time and memory cap.
//! There the epoch stops a job that runs [`jobs::SLICE`] of wasm without entering the host (the
//! call hook renews the slice on each return from an import), one past its wall time, and one
//! still running [`jobs::CANCEL_GRACE`] after it was cancelled. Any of those ends the job and
//! drops its store, never the plugin. [`Plugin::deliver_jobs`] hands each ended job to the
//! plugin's `job-finished`, and unloading the plugin ends its jobs.

use std::{
    cell::RefCell,
    fmt,
    future::poll_fn,
    io, mem,
    pin::pin,
    sync::{
        Arc,
        mpsc::{self, Receiver, SyncSender},
    },
    task::Poll,
    thread::{self, JoinHandle},
    time::Instant,
};

use kiln_command::{Capability, Grants};
use kiln_core::ApiVersion;
use kiln_l10n::tr;
use wasmtime::{
    CallHook, Store, Trap, UpdateDeadline,
    component::{
        Component, ComponentType, Instance, InstancePre, Lift, Linker, ResourceTable, TypedFunc,
        WasmStr,
    },
};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::bindings::kiln::extension::events::Event;
use crate::bindings::{self, Error, HostState, Level, LogSink};
use crate::cache::ComponentCache;
use crate::commands;
use crate::compat::{self, Compat, HOST_API, Incompatible, World};
use crate::engine::{self, PluginEngine};
use crate::fs::{self, Workspace};
use crate::jobs::{
    self, Ended, JobId, JobState, Jobs, Launch, Launcher, Reservation, Stop, WorkerHost,
};
use crate::limits::{Limits, PROGRAM_DEADLINE, StoreLimiter};

/// Why a plugin was unloaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Its code trapped.
    Trap,
    /// A call ran past its deadline.
    Timeout,
    /// It failed after being refused memory or a table.
    Memory,
    /// It broke the world's contract: an import it needs is missing, or it answered with a value
    /// that does not lift.
    Protocol,
}

impl Reason {
    /// Its name, as the catalogue's messages select on it.
    fn selector(self) -> &'static str {
        match self {
            Self::Trap => "trap",
            Self::Timeout => "timeout",
            Self::Memory => "memory",
            Self::Protocol => "protocol",
        }
    }
}

/// In the active locale.
impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&tr!("kiln-plugin-unload-reason", reason = self.selector()))
    }
}

/// What unloaded a plugin, for the user: its message is in the active
/// locale when it was unloaded.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub struct Diagnostic {
    /// The plugin's id.
    pub plugin: String,
    pub reason: Reason,
    pub message: String,
}

/// In the active locale.
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&tr!(
            "kiln-plugin-unloaded",
            plugin = &self.plugin,
            reason = self.reason.selector(),
            message = &self.message
        ))
    }
}

/// Whether a plugin still runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginState {
    Running,
    Unloaded(Diagnostic),
}

/// Why [`Plugin::load`] failed.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    TooLarge {
        plugin: String,
        bytes: usize,
        limit: usize,
    },
    Invalid {
        plugin: String,
        message: String,
    },
    Thread {
        plugin: String,
        error: io::Error,
    },
    /// The API version it needs is one this host doesn't serve; nothing of it was compiled.
    Incompatible {
        plugin: String,
        error: Incompatible,
    },
    /// Its `init` returned an error.
    Init {
        plugin: String,
        error: Error,
    },
    /// Instantiating it or its `init` trapped, overran or broke protocol.
    Unloaded(Diagnostic),
}

/// In the active locale.
impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&match self {
            Self::TooLarge {
                plugin,
                bytes,
                limit,
            } => tr!(
                "kiln-plugin-too-large",
                plugin = plugin,
                bytes = *bytes,
                limit = *limit
            ),
            Self::Invalid { plugin, message } => {
                tr!("kiln-plugin-invalid", plugin = plugin, reason = message)
            }
            Self::Thread { plugin, error } => tr!(
                "kiln-plugin-no-thread",
                plugin = plugin,
                reason = error.to_string()
            ),
            Self::Incompatible { plugin, error } => {
                tr!(
                    "kiln-plugin-invalid",
                    plugin = plugin,
                    reason = error.to_string()
                )
            }
            Self::Init { plugin, error } => tr!(
                "kiln-plugin-init-failed",
                plugin = plugin,
                reason = format!("{error:?}")
            ),
            Self::Unloaded(diagnostic) => return diagnostic.fmt(f),
        })
    }
}

/// Why [`Plugin::call`] failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CallError {
    /// This call unloaded the plugin.
    Failed(Diagnostic),
    /// The plugin was unloaded before; nothing ran.
    Unloaded,
}

/// In the active locale.
impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed(diagnostic) => diagnostic.fmt(f),
            Self::Unloaded => f.write_str(&tr!("kiln-plugin-call-unloaded")),
        }
    }
}

/// A call into a plugin's exports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    /// `command-handler.run-command`.
    RunCommand { id: String, args: String },
    /// `event-handler.handle-events`.
    HandleEvents(Vec<Event>),
}

/// What a [`Call`] returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    RunCommand(Result<(), Error>),
    HandleEvents,
}

/// A loaded plugin. Dropping it calls the plugin's `shutdown` and stops its thread.
pub struct Plugin {
    id: String,
    state: PluginState,
    /// Some while it runs.
    channel: Option<Channel>,
    thread: Option<JoinHandle<()>>,
    /// Its jobs, with the `jobs` capability, until it is unloaded.
    jobs: Option<Jobs>,
}

/// The caller's ends of the plugin thread's channels.
struct Channel {
    calls: SyncSender<Request>,
    replies: Receiver<Result<Replied, Diagnostic>>,
}

/// What the plugin thread is asked to run.
enum Request {
    Call(Call),
    /// `job-handler.job-finished`.
    JobFinished(JobId, Ended),
}

/// What a [`Request`] returned.
enum Replied {
    Call(Reply),
    JobFinished,
}

impl Plugin {
    /// Loads `component` as plugin `id`, its logs going to the `log` crate under its id.
    pub fn load(
        engine: &PluginEngine,
        component: &[u8],
        id: &str,
        grants: &Grants,
        limits: Limits,
    ) -> Result<Self, LoadError> {
        Self::load_logging_to(engine, component, id, grants, limits, guest_log(id))
    }

    /// Loads `component` as [`load`](Self::load) does, taking its compiled form from `cache`.
    /// The caller has checked the grants before this: the cache only skips compilation.
    pub fn load_cached(
        engine: &PluginEngine,
        cache: Option<&ComponentCache>,
        component: &[u8],
        id: &str,
        grants: &Grants,
        limits: Limits,
    ) -> Result<Self, LoadError> {
        Self::load_with(engine, cache, component, id, grants, limits, guest_log(id))
    }

    /// Loads `component` as plugin `id`, its logs going to `log`, which runs on the plugin's
    /// thread.
    pub fn load_logging_to(
        engine: &PluginEngine,
        component: &[u8],
        id: &str,
        grants: &Grants,
        limits: Limits,
        log: LogSink,
    ) -> Result<Self, LoadError> {
        Self::load_with(engine, None, component, id, grants, limits, log)
    }

    fn load_with(
        engine: &PluginEngine,
        cache: Option<&ComponentCache>,
        component: &[u8],
        id: &str,
        grants: &Grants,
        limits: Limits,
        log: LogSink,
    ) -> Result<Self, LoadError> {
        Self::load_hosted(
            engine,
            cache,
            component,
            id,
            grants,
            limits,
            HostState::new(log),
        )
    }

    /// Loads `component` as [`load_cached`](Self::load_cached) does, its imports running
    /// against `host`, whose slices the caller has set up for plugin `id`: its commands slice
    /// on the app's [`CommandHost`](crate::commands::CommandHost), say. `host` runs on the
    /// plugin's thread; its commands slice takes registrations only while `init` runs, and it
    /// learns each call's deadline as [`HostState::deadline`]. It runs against the current
    /// [`World`], as a plugin needing [`HOST_API`].
    pub fn load_hosted(
        engine: &PluginEngine,
        cache: Option<&ComponentCache>,
        component: &[u8],
        id: &str,
        grants: &Grants,
        limits: Limits,
        host: HostState,
    ) -> Result<Self, LoadError> {
        Self::load_for(
            engine,
            cache,
            component,
            id,
            grants,
            limits,
            host,
            &Compat::CURRENT,
            HOST_API,
        )
    }

    /// Loads `component` as [`load_hosted`](Self::load_hosted) does, as a plugin needing API
    /// `needed` (its manifest's `engine`) on `compat`'s host. It is refused, before anything is
    /// compiled, when that host doesn't serve `needed`; otherwise it runs against `needed`'s
    /// generated world ([`Compat::world_for`]), so it is asked only for that world's exports.
    #[allow(clippy::too_many_arguments)]
    pub fn load_for(
        engine: &PluginEngine,
        cache: Option<&ComponentCache>,
        component: &[u8],
        id: &str,
        grants: &Grants,
        limits: Limits,
        host: HostState,
        compat: &Compat,
        needed: ApiVersion,
    ) -> Result<Self, LoadError> {
        let world = compat
            .world_for(needed)
            .map_err(|error| LoadError::Incompatible {
                plugin: id.to_owned(),
                error,
            })?;
        if component.len() > limits.max_read_bytes {
            return Err(LoadError::TooLarge {
                plugin: id.to_owned(),
                bytes: component.len(),
                limit: limits.max_read_bytes,
            });
        }
        let invalid = |error: wasmtime::Error| LoadError::Invalid {
            plugin: id.to_owned(),
            message: format!("{error:#}"),
        };
        let runs_jobs = world >= World::V1_1 && grants.allows(&Capability::Jobs);
        if runs_jobs && !jobs::lifts_utf8(component) {
            return Err(LoadError::Invalid {
                plugin: id.to_owned(),
                message: jobs::NOT_UTF8.to_owned(),
            });
        }
        let component = match cache {
            Some(cache) => cache.load(engine.engine(), component),
            None => Component::new(engine.engine(), component),
        }
        .map_err(invalid)?;
        // Links and type-checks the world here, so only a component that fits it gets a thread.
        // The jobs interfaces link only with the `jobs` capability, so a component importing
        // them without it fails here.
        let pre = linker(engine, runs_jobs)
            .and_then(|linker| linker.instantiate_pre(&component))
            .map_err(invalid)?;
        world.check(&pre).map_err(invalid)?;
        let mut host = host;
        let jobs = if runs_jobs {
            bindings::jobs_world::JobsExtensionPre::new(pre.clone()).map_err(invalid)?;
            let workers = Arc::new(Workers {
                id: id.to_owned(),
                engine: engine.clone(),
                pre: pre.clone(),
                grants: grants.clone(),
                world,
                commands: host.commands.for_jobs(),
                host: host.jobs.workers().unwrap_or_else(|| {
                    let id = id.to_owned();
                    Arc::new(move || HostState::new(guest_log(&id)))
                }),
            });
            let launcher: Launcher = Arc::new(move |launch| workers.launch(launch));
            let jobs = (host.jobs.host()).attach(id, grants, host.jobs.limits(), launcher);
            host.jobs.attach(jobs.clone());
            Some(jobs)
        } else {
            None
        };
        let detach = |jobs: &Option<Jobs>| jobs.iter().for_each(Jobs::detach);
        let data = Data {
            host,
            wasi: wasi_context(grants, &Workspace::default()),
            table: ResourceTable::new(),
            limiter: StoreLimiter::new(&limits),
            deadline: None,
            timed_out: false,
            slice_ends: None,
            sliced: false,
            imported: false,
        };
        let guest = Guest {
            id: id.to_owned(),
            world,
            engine: engine.clone(),
            limits,
            store: Store::new(engine.engine(), data),
        };
        let (started, start) = mpsc::sync_channel(1);
        let (calls, received) = mpsc::sync_channel(1);
        let (reply, replies) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name(format!("plugin {id}"))
            .spawn(move || guest.run(&pre, &started, &received, &reply))
            .map_err(|error| {
                detach(&jobs);
                LoadError::Thread {
                    plugin: id.to_owned(),
                    error,
                }
            })?;
        let started = start.recv().unwrap_or_else(|_| {
            Err(LoadError::Unloaded(Diagnostic {
                plugin: id.to_owned(),
                reason: Reason::Trap,
                message: tr!("kiln-plugin-thread-stopped-loading").to_string(),
            }))
        });
        if let Err(error) = started {
            detach(&jobs);
            // The thread has dropped the store and is ending.
            drop(thread.join());
            return Err(error);
        }
        Ok(Self {
            id: id.to_owned(),
            state: PluginState::Running,
            channel: Some(Channel { calls, replies }),
            thread: Some(thread),
            jobs,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn state(&self) -> &PluginState {
        &self.state
    }

    /// Whether the plugin's thread is still running: false once it is unloaded.
    pub fn thread_running(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }

    /// Its jobs: some while it runs, if it was loaded with the `jobs` capability.
    pub fn jobs(&self) -> Option<&Jobs> {
        self.jobs.as_ref()
    }

    /// Hands each of its ended jobs to its `job-finished`, oldest first, and returns how many it
    /// handed over. Each is the host's no longer once taken, so it is delivered once; a call that
    /// fails unloads the plugin, as [`call`](Self::call) does.
    pub fn deliver_jobs(&mut self) -> Result<usize, CallError> {
        // Only those ended now: a `job-finished` that starts a job and waits for it doesn't
        // keep this delivering.
        let ended = self.jobs.as_ref().map_or(0, Jobs::undelivered);
        let mut delivered = 0;
        while delivered < ended
            && let Some((job, ended)) = self.jobs.as_ref().and_then(Jobs::take_ended)
        {
            self.request(Request::JobFinished(job, ended))?;
            delivered += 1;
        }
        Ok(delivered)
    }

    /// Runs `call` on the plugin's thread and returns what it returned. A call that traps,
    /// overruns, runs out of memory or breaks protocol unloads the plugin first.
    pub fn call(&mut self, call: Call) -> Result<Reply, CallError> {
        match self.request(Request::Call(call))? {
            Replied::Call(reply) => Ok(reply),
            Replied::JobFinished => unreachable!("a call answered as a job's delivery"),
        }
    }

    fn request(&mut self, request: Request) -> Result<Replied, CallError> {
        let Some(channel) = &self.channel else {
            return Err(CallError::Unloaded);
        };
        let replied = match channel.calls.send(request) {
            Ok(()) => channel.replies.recv().ok(),
            Err(_) => None,
        };
        let replied = replied.unwrap_or_else(|| {
            Err(Diagnostic {
                plugin: self.id.clone(),
                reason: Reason::Trap,
                message: tr!("kiln-plugin-thread-stopped").to_string(),
            })
        });
        replied.map_err(|diagnostic| {
            self.unload(diagnostic.clone());
            CallError::Failed(diagnostic)
        })
    }

    fn unload(&mut self, diagnostic: Diagnostic) {
        log::warn!("{diagnostic}");
        self.jobs.take().iter().for_each(Jobs::detach);
        self.channel = None;
        if let Some(thread) = self.thread.take() {
            // It has replied, dropped its store and is ending.
            drop(thread.join());
        }
        self.state = PluginState::Unloaded(diagnostic);
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        self.jobs.take().iter().for_each(Jobs::detach);
        // Closing the channel tells the thread to shut the plugin down and end.
        self.channel = None;
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

/// A plugin's WASI context: no preopens beyond what [`fs::configure`] grants, no environment,
/// no arguments, and stdio that reaches nothing of the host's.
pub fn wasi_context(grants: &Grants, workspace: &Workspace) -> WasiCtx {
    let mut wasi = WasiCtxBuilder::new();
    fs::configure(&mut wasi, grants, workspace);
    wasi.build()
}

/// A linker of every import, the jobs interfaces only when the plugin `runs_jobs`.
fn linker(engine: &PluginEngine, runs_jobs: bool) -> wasmtime::Result<Linker<Data>> {
    let mut linker = Linker::<Data>::new(engine.engine());
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    bindings::add_to_linker(&mut linker, imported)?;
    if runs_jobs {
        bindings::add_jobs_to_linker(&mut linker, imported)?;
    }
    compat::link(&mut linker)?;
    Ok(linker)
}

/// The [`HostState`] an import runs against, noted as entered for an import.
fn imported(data: &mut Data) -> &mut HostState {
    data.imported = true;
    &mut data.host
}

/// Plugin `id`'s logs, to the `log` crate under the `kiln_plugin::guest` target.
fn guest_log(id: &str) -> LogSink {
    let id = id.to_owned();
    Box::new(move |level, message| {
        let level = match level {
            Level::Trace => log::Level::Trace,
            Level::Debug => log::Level::Debug,
            Level::Info => log::Level::Info,
            Level::Warn => log::Level::Warn,
            Level::Error => log::Level::Error,
        };
        log::log!(target: "kiln_plugin::guest", level, "{id}: {message}");
    })
}

/// A plugin's store data.
struct Data {
    host: HostState,
    wasi: WasiCtx,
    table: ResourceTable,
    limiter: StoreLimiter,
    /// When the running call must end; none while none runs, or when the deadline is too far
    /// off to name.
    deadline: Option<Instant>,
    /// Whether the epoch stopped the running call at its deadline.
    timed_out: bool,
    /// In a worker store: when the job's slice of wasm ends, unless it enters the host first.
    slice_ends: Option<Instant>,
    /// Whether the epoch stopped the job at the end of its slice.
    sliced: bool,
    /// Whether the host was entered for an import since the store last entered the host:
    /// wasmtime calls the call hook around its own calls into the host as well, which enter
    /// none.
    imported: bool,
}

impl WasiView for Data {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        self.imported = true;
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

/// A world's exports, typed. The generated `Extension` calls them synchronously, which would
/// block in an async WASI import past any deadline; these are called async.
struct Exports {
    init: TypedFunc<(), (Result<(), Error>,)>,
    shutdown: TypedFunc<(), ()>,
    run_command: TypedFunc<(String, String), (Result<(), Error>,)>,
    handle_events: TypedFunc<(Vec<Event>,), ()>,
    /// With the `jobs` capability.
    job_finished: Option<TypedFunc<(JobId, JobState), ()>>,
}

impl Exports {
    /// `instance`'s exports, named as `world` names them.
    fn new(
        store: &mut Store<Data>,
        instance: &Instance,
        world: World,
        runs_jobs: bool,
    ) -> wasmtime::Result<Self> {
        let run_command = export(store, instance, world, "command-handler", "run-command")?;
        let handle_events = export(store, instance, world, "event-handler", "handle-events")?;
        let job_finished = if runs_jobs {
            let job_finished = export(store, instance, world, "job-handler", "job-finished")?;
            Some(instance.get_typed_func(&mut *store, job_finished)?)
        } else {
            None
        };
        Ok(Self {
            init: instance.get_typed_func(&mut *store, "init")?,
            shutdown: instance.get_typed_func(&mut *store, "shutdown")?,
            run_command: instance.get_typed_func(&mut *store, run_command)?,
            handle_events: instance.get_typed_func(&mut *store, handle_events)?,
            job_finished,
        })
    }
}

/// `instance`'s export `name` of interface `interface`, named as `world` names it.
fn export(
    store: &mut Store<Data>,
    instance: &Instance,
    world: World,
    interface: &str,
    name: &str,
) -> wasmtime::Result<wasmtime::component::ComponentExportIndex> {
    let interface = world.interface(interface);
    instance
        .get_export_index(&mut *store, None, &interface)
        .and_then(|index| instance.get_export_index(&mut *store, Some(&index), name))
        .ok_or_else(|| {
            let export = format!("{interface}#{name}");
            wasmtime::format_err!("{}", tr!("kiln-plugin-no-export", export = export))
        })
}

/// The plugin thread's side: the store and what it runs.
struct Guest {
    id: String,
    /// The world it runs against.
    world: World,
    engine: PluginEngine,
    limits: Limits,
    store: Store<Data>,
}

impl Guest {
    /// The plugin thread: instantiates the component and calls `init`, telling `started` how
    /// that went, then runs each call `calls` brings until one fails or the channel closes, when
    /// it calls `shutdown`. The store drops as it returns.
    fn run(
        mut self,
        pre: &InstancePre<Data>,
        started: &SyncSender<Result<(), LoadError>>,
        calls: &Receiver<Request>,
        replies: &SyncSender<Result<Replied, Diagnostic>>,
    ) {
        let exports = match self.start(pre) {
            Ok(exports) => exports,
            Err(error) => {
                drop(started.send(Err(error)));
                return;
            }
        };
        if started.send(Ok(())).is_err() {
            return;
        }
        for request in calls {
            let reply = self.guarded(async |store| match request {
                Request::Call(Call::RunCommand { id, args }) => exports
                    .run_command
                    .call_async(store, (id, args))
                    .await
                    .map(|(result,)| Replied::Call(Reply::RunCommand(result))),
                Request::Call(Call::HandleEvents(events)) => exports
                    .handle_events
                    .call_async(store, (events,))
                    .await
                    .map(|()| Replied::Call(Reply::HandleEvents)),
                Request::JobFinished(job, ended) => match &exports.job_finished {
                    Some(job_finished) => job_finished
                        .call_async(store, (job, ended.into_state()))
                        .await
                        .map(|()| Replied::JobFinished),
                    None => Ok(Replied::JobFinished),
                },
            });
            let failed = reply.is_err();
            if replies.send(reply).is_err() || failed {
                return;
            }
        }
        if let Err(diagnostic) =
            self.guarded(async |store| exports.shutdown.call_async(store, ()).await)
        {
            log::warn!("{diagnostic}");
        }
    }

    fn start(&mut self, pre: &InstancePre<Data>) -> Result<Exports, LoadError> {
        self.store.limiter(|data| &mut data.limiter);
        self.store.epoch_deadline_callback(|mut store| {
            let data = store.data_mut();
            // The call's deadline doesn't count the time its program runs took.
            let deadline = data.host.program_runs.extend(data.deadline);
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                data.timed_out = true;
                return Ok(UpdateDeadline::Interrupt);
            }
            Ok(UpdateDeadline::Continue(1))
        });
        let instance = self
            .guarded(async |store| pre.instantiate_async(store).await)
            .map_err(LoadError::Unloaded)?;
        let runs_jobs = self.store.data().host.jobs.jobs().is_some();
        let exports =
            Exports::new(&mut self.store, &instance, self.world, runs_jobs).map_err(|error| {
                LoadError::Unloaded(self.diagnosis(Reason::Protocol, format!("{error:?}")))
            })?;
        self.store.data_mut().host.commands.begin_init();
        let (initialized,) = self
            .guarded(async |store| exports.init.call_async(store, ()).await)
            .map_err(LoadError::Unloaded)?;
        initialized.map_err(|error| LoadError::Init {
            plugin: self.id.clone(),
            error,
        })?;
        self.store.data_mut().host.commands.end_init();
        Ok(exports)
    }

    /// Runs `call` with the epoch advancing, until it returns or its deadline passes: the epoch
    /// stops guest code at the deadline, and a call still waiting on a host import then is
    /// dropped. A call that returns past its deadline has overrun too.
    fn guarded<R>(
        &mut self,
        call: impl AsyncFnOnce(&mut Store<Data>) -> wasmtime::Result<R>,
    ) -> Result<R, Diagnostic> {
        let deadline = Instant::now().checked_add(self.limits.call_deadline);
        let program_runs = self.store.data().host.program_runs.clone();
        program_runs.arm(PROGRAM_DEADLINE);
        let passed = || {
            let deadline = program_runs.extend(deadline);
            deadline.is_some_and(|deadline| Instant::now() >= deadline)
        };
        let data = self.store.data_mut();
        data.deadline = deadline;
        data.host.deadline = deadline;
        data.timed_out = false;
        data.limiter.clear_refused();
        self.store.set_epoch_deadline(1);
        let ticking = self.engine.ticking();
        let store = &mut self.store;
        let returned = wasmtime_wasi::runtime::in_tokio(async {
            let mut call = pin!(call(store));
            poll_fn(|cx| {
                if let Poll::Ready(returned) = call.as_mut().poll(cx) {
                    return Poll::Ready(Some(returned));
                }
                if passed() {
                    return Poll::Ready(None);
                }
                engine::wake_on_tick(cx.waker());
                Poll::Pending
            })
            .await
        });
        drop(ticking);
        let overran = passed();
        program_runs.disarm();
        let data = self.store.data_mut();
        data.deadline = None;
        data.host.deadline = None;
        let timed_out = data.timed_out || overran;
        match returned {
            Some(Ok(returned)) if !timed_out => Ok(returned),
            Some(Err(error)) if !timed_out => Err(self.diagnose(&error)),
            _ => {
                let deadline = self.limits.call_deadline;
                Err(self.diagnosis(
                    Reason::Timeout,
                    tr!(
                        "kiln-plugin-call-deadline",
                        deadline = format!("{deadline:?}")
                    )
                    .to_string(),
                ))
            }
        }
    }

    fn diagnose(&self, error: &wasmtime::Error) -> Diagnostic {
        let reason = if self.store.data().limiter.refused() {
            Reason::Memory
        } else if error.is::<Trap>() {
            Reason::Trap
        } else {
            Reason::Protocol
        };
        self.diagnosis(reason, format!("{error:?}"))
    }

    fn diagnosis(&self, reason: Reason, message: String) -> Diagnostic {
        Diagnostic {
            plugin: self.id.clone(),
            reason,
            message,
        }
    }
}

/// What a plugin's worker stores are made of: its component, linked as its interactive store
/// is, and its grants. The launcher [`Plugin::load_for`] attaches its jobs with.
struct Workers {
    id: String,
    engine: PluginEngine,
    pre: InstancePre<Data>,
    grants: Grants,
    world: World,
    /// Its workers' commands slices: on the plugin's own attachment, whatever `host` makes.
    commands: commands::JobCommands,
    host: WorkerHost,
}

impl Workers {
    /// Runs `launch` on a thread of its own, at lowered priority. If there is none, the launch
    /// drops unfinished, so the job ends failed.
    fn launch(self: &Arc<Self>, mut launch: Launch) {
        let workers = Arc::clone(self);
        let id = &self.id;
        let spawned = thread::Builder::new()
            .name(format!("plugin {id}"))
            .spawn(move || {
                kiln_core::platform::lower_thread_priority();
                let ended = workers.run(&mut launch);
                launch.finish(ended);
            });
        if let Err(error) = spawned {
            log::warn!("no thread for a job of plugin {id}: {error}");
        }
    }

    /// Runs `launch`'s job in a new worker store, without `init`, and returns how it ended. The
    /// epoch stops it at the end of a slice, at its wall time and past its cancel grace; one
    /// waiting on an async host import then is dropped, and the imports that wait on their own
    /// wait only until then. The store drops as it returns.
    fn run(&self, launch: &mut Launch) -> Ended {
        let mut host = (self.host)();
        host.jobs = jobs::State::worker(launch.control.clone());
        host.commands = self.commands.state();
        // A job's programs run as long as its wall time allows, not an interactive call's 10 s.
        host.process = mem::take(&mut host.process).with_deadline(launch.wall);
        let limits = Limits {
            memory_bytes: usize::try_from(launch.memory_bytes).unwrap_or(usize::MAX),
            ..Limits::default()
        };
        let workspace = Workspace {
            roots: launch.roots.clone(),
        };
        let mut store = Store::new(
            self.engine.engine(),
            Data {
                host,
                wasi: wasi_context(&self.grants, &workspace),
                table: ResourceTable::new(),
                limiter: StoreLimiter::new(&limits),
                deadline: None,
                timed_out: false,
                slice_ends: None,
                sliced: false,
                imported: false,
            },
        );
        store.limiter(|data| &mut data.limiter);
        let wall_ends = Instant::now().checked_add(launch.wall);
        let control = launch.control.clone();
        // Every time the epoch ticks over running wasm.
        let ticked = move |data: &mut Data| {
            let now = Instant::now();
            if wall_ends.is_some_and(|ends| now >= ends) {
                control.stop(Stop::TimedOut);
            }
            if data.slice_ends.is_some_and(|ends| now >= ends) {
                data.sliced = true;
                return UpdateDeadline::Interrupt;
            }
            if control.past_grace(now) {
                return UpdateDeadline::Interrupt;
            }
            UpdateDeadline::Continue(1)
        };
        store.epoch_deadline_callback(move |mut store| Ok(ticked(store.data_mut())));
        // On entering the host, an import that waits is told when the job must end: its wall
        // time, or its grace once it is being stopped. A slice starts each time wasm runs again:
        // as the job starts, and on each return from an import. wasmtime's own calls into the
        // host (the epoch's check, `memory.grow`, the bulk memory instructions) enter no import,
        // so they don't renew it.
        let control = launch.control.clone();
        store.call_hook(move |mut store, hook| {
            let data = store.data_mut();
            match hook {
                CallHook::CallingHost => {
                    let ends = control.ends(wall_ends);
                    data.host.deadline = ends;
                    match ends {
                        Some(ends) => (data.host.program_runs)
                            .arm(ends.saturating_duration_since(Instant::now())),
                        None => data.host.program_runs.disarm(),
                    }
                }
                CallHook::ReturningFromHost if !mem::take(&mut data.imported) => {}
                CallHook::CallingWasm | CallHook::ReturningFromHost => {
                    data.slice_ends = Instant::now().checked_add(jobs::SLICE);
                }
                CallHook::ReturningFromWasm => {}
            }
            Ok(())
        });
        store.set_epoch_deadline(1);
        let control = launch.control.clone();
        let ticking = self.engine.ticking();
        // The call runs on this thread, its result lifted here too.
        let reserving = Reserving::set(launch.reservation());
        let returned = wasmtime_wasi::runtime::in_tokio(async {
            let store = &mut store;
            let mut run = pin!(async {
                let instance = self.pre.instantiate_async(&mut *store).await?;
                let run_job = export(store, &instance, self.world, "job-runner", "run-job")?;
                // The result is checked and charged as it is lifted, before it is copied out of
                // the worker's memory, and before the guest's post-return may reuse that memory.
                let run_job = instance.get_typed_func::<
                    (JobId, String, String, Option<String>),
                    (Result<KeptText, GuestError>,),
                >(&mut *store, run_job)?;
                let args = (
                    launch.job,
                    mem::take(&mut launch.name),
                    mem::take(&mut launch.args),
                    launch.root.clone(),
                );
                let (returned,) = run_job.call_async(&mut *store, args).await?;
                Ok::<_, wasmtime::Error>(returned)
            });
            poll_fn(|cx| {
                if let Poll::Ready(returned) = run.as_mut().poll(cx) {
                    return Poll::Ready(Some(returned));
                }
                let now = Instant::now();
                if wall_ends.is_some_and(|ends| now >= ends) {
                    control.stop(Stop::TimedOut);
                }
                if control.past_grace(now) {
                    return Poll::Ready(None);
                }
                engine::wake_on_tick(cx.waker());
                Poll::Pending
            })
            .await
        });
        drop(reserving);
        drop(ticking);
        // An import that returned late doesn't make the job's wall time longer.
        if wall_ends.is_some_and(|ends| Instant::now() >= ends) {
            control.stop(Stop::TimedOut);
        }
        let data = store.data();
        match (control.stopping(), returned) {
            (Some(Stop::TimedOut), _) => Ended::TimedOut,
            (Some(Stop::Cancelled), _) => Ended::Cancelled,
            _ if data.sliced => Ended::failed(jobs::SLICE_OVERRUN),
            (None, Some(Ok(result))) => kept(result),
            _ if data.limiter.refused() => Ended::failed(jobs::OUT_OF_MEMORY),
            (None, Some(Err(error))) => {
                // Its room is charged before it is written: with no room, it is dropped unwritten.
                let reserved = (launch.reservation())
                    .is_some_and(|reservation| reservation.reserve(jobs::MAX_DIAGNOSTIC_BYTES));
                if reserved {
                    Ended::failed(diagnostic(&error))
                } else {
                    Ended::failed(jobs::RESULT_DROPPED)
                }
            }
            (None, None) => Ended::failed(jobs::THREAD_STOPPED),
        }
    }
}

/// `kiln:extension/types.error` as `run-job` returns it, its message lifted as [`KeptText`].
#[derive(ComponentType, Lift)]
#[component(variant)]
enum GuestError {
    #[component(name = "unimplemented")]
    Unimplemented(KeptText),
    #[component(name = "denied")]
    Denied(KeptText),
    #[component(name = "not-found")]
    NotFound(KeptText),
    #[component(name = "invalid-argument")]
    InvalidArgument(KeptText),
    #[component(name = "conflict")]
    Conflict(KeptText),
    #[component(name = "failed")]
    Failed(KeptText),
}

/// A string `run-job` returned, as the host keeps it: checked against
/// [`jobs::MAX_RESULT_BYTES`] and charged to the plugin's undelivered results (through the
/// worker thread's [`RESERVATION`]) while it is lifted, and only then copied out of the worker's
/// memory. A plugin that runs jobs lifts its strings as UTF-8 ([`jobs::lifts_utf8`]).
enum KeptText {
    Kept(String),
    /// Past [`jobs::MAX_RESULT_BYTES`]: nothing copied.
    TooLarge,
    /// It wouldn't fit the plugin's undelivered results: nothing copied.
    Dropped,
}

// What charges the result `run-job` returns, while it runs on this worker's thread.
thread_local! {
    static RESERVATION: RefCell<Option<Reservation>> = const { RefCell::new(None) };
}

/// Holds this thread's [`RESERVATION`] until dropped.
struct Reserving;

impl Reserving {
    fn set(reservation: Option<Reservation>) -> Self {
        RESERVATION.set(reservation);
        Self
    }
}

impl Drop for Reserving {
    fn drop(&mut self) {
        RESERVATION.set(None);
    }
}

impl KeptText {
    /// The string of `len` bytes at `ptr` in `memory`, kept as [`KeptText`] says.
    fn keep(memory: &[u8], ptr: u32, len: u32) -> wasmtime::Result<Self> {
        let out_of_bounds = || wasmtime::Error::msg(jobs::OUT_OF_BOUNDS);
        let (ptr, len) = (usize::try_from(ptr)?, usize::try_from(len)?);
        let bytes = (memory.get(ptr..))
            .and_then(|rest| rest.get(..len))
            .ok_or_else(out_of_bounds)?;
        if len > jobs::MAX_RESULT_BYTES {
            return Ok(Self::TooLarge);
        }
        let reserved = RESERVATION.with_borrow(|reservation| {
            (reservation.as_ref()).is_some_and(|reservation| reservation.reserve(len))
        });
        if !reserved {
            return Ok(Self::Dropped);
        }
        Ok(Self::Kept(std::str::from_utf8(bytes)?.to_owned()))
    }
}

// SAFETY: a `string`, lowered as a pointer and a length, as `WasmStr` is.
unsafe impl ComponentType for KeptText {
    type Lower = <WasmStr as ComponentType>::Lower;

    const ABI: wasmtime::component::__internal::CanonicalAbiInfo = <WasmStr as ComponentType>::ABI;

    fn typecheck(
        ty: &wasmtime::component::__internal::InterfaceType,
        types: &wasmtime::component::__internal::InstanceType<'_>,
    ) -> wasmtime::Result<()> {
        <WasmStr as ComponentType>::typecheck(ty, types)
    }
}

// SAFETY: reads the pointer and length as the canonical ABI lays a `string` out, flat or in
// memory, and the bytes only within the memory's bounds.
unsafe impl Lift for KeptText {
    fn linear_lift_from_flat(
        cx: &mut wasmtime::component::__internal::LiftContext<'_>,
        _ty: wasmtime::component::__internal::InterfaceType,
        src: &Self::Lower,
    ) -> wasmtime::Result<Self> {
        Self::keep(cx.memory(), src[0].get_u32(), src[1].get_u32())
    }

    fn linear_lift_from_memory(
        cx: &mut wasmtime::component::__internal::LiftContext<'_>,
        _ty: wasmtime::component::__internal::InterfaceType,
        bytes: &[u8],
    ) -> wasmtime::Result<Self> {
        let word = |at: usize| {
            (bytes.get(at..at + 4))
                .and_then(|word| word.try_into().ok())
                .map(u32::from_le_bytes)
                .ok_or_else(|| wasmtime::Error::msg(jobs::OUT_OF_BOUNDS))
        };
        Self::keep(cx.memory(), word(0)?, word(4)?)
    }
}

/// One of [`Error`]'s cases, made from its message.
type MakeError = fn(String) -> Error;

/// The job's end as `run-job` returned `result`.
fn kept(result: Result<KeptText, GuestError>) -> Ended {
    let (text, error): (_, Option<MakeError>) = match result {
        Ok(text) => (text, None),
        Err(GuestError::Unimplemented(text)) => (text, Some(Error::Unimplemented)),
        Err(GuestError::Denied(text)) => (text, Some(Error::Denied)),
        Err(GuestError::NotFound(text)) => (text, Some(Error::NotFound)),
        Err(GuestError::InvalidArgument(text)) => (text, Some(Error::InvalidArgument)),
        Err(GuestError::Conflict(text)) => (text, Some(Error::Conflict)),
        Err(GuestError::Failed(text)) => (text, Some(Error::Failed)),
    };
    match text {
        KeptText::Kept(text) => Ended::Finished(match error {
            None => Ok(text),
            Some(error) => Err(error(text)),
        }),
        KeptText::TooLarge => Ended::failed(jobs::RESULT_TOO_LARGE),
        KeptText::Dropped => Ended::failed(jobs::RESULT_DROPPED),
    }
}

/// `error` for a job's end: at most [`jobs::MAX_DIAGNOSTIC_BYTES`] of it, written no further,
/// as a trap's backtrace names functions the guest chose. Its room is allocated once, at the
/// bound the caller has reserved.
fn diagnostic(error: &wasmtime::Error) -> String {
    struct Bounded(String);
    impl fmt::Write for Bounded {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            let room = jobs::MAX_DIAGNOSTIC_BYTES - self.0.len();
            if text.len() <= room {
                self.0.push_str(text);
                return Ok(());
            }
            let cut = (0..=room)
                .rev()
                .find(|&at| text.is_char_boundary(at))
                .unwrap_or(0);
            self.0.push_str(&text[..cut]);
            Err(fmt::Error)
        }
    }
    let mut bounded = Bounded(String::with_capacity(jobs::MAX_DIAGNOSTIC_BYTES));
    // A full writer stops the formatting: what fit is the diagnostic.
    fmt::Write::write_fmt(&mut bounded, format_args!("{error:#}")).ok();
    bounded.0
}
