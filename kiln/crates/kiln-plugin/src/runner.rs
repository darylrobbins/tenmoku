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

use std::{
    fmt,
    future::poll_fn,
    io,
    pin::pin,
    sync::mpsc::{self, Receiver, SyncSender},
    task::Poll,
    thread::{self, JoinHandle},
    time::Instant,
};

use kiln_command::Grants;
use kiln_core::ApiVersion;
use kiln_l10n::tr;
use wasmtime::{
    Store, Trap, UpdateDeadline,
    component::{Component, Instance, InstancePre, Linker, ResourceTable, TypedFunc},
};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::bindings::kiln::extension::events::Event;
use crate::bindings::{self, Error, HostState, Level, LogSink};
use crate::cache::ComponentCache;
use crate::compat::{self, Compat, HOST_API, Incompatible, World};
use crate::engine::{self, PluginEngine};
use crate::fs::{self, Workspace};
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
}

/// The caller's ends of the plugin thread's channels.
struct Channel {
    calls: SyncSender<Call>,
    replies: Receiver<Result<Reply, Diagnostic>>,
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
        let component = match cache {
            Some(cache) => cache.load(engine.engine(), component),
            None => Component::new(engine.engine(), component),
        }
        .map_err(invalid)?;
        // Links and type-checks the world here, so only a component that fits it gets a thread.
        let pre = linker(engine)
            .and_then(|linker| linker.instantiate_pre(&component))
            .map_err(invalid)?;
        world.check(&pre).map_err(invalid)?;
        let data = Data {
            host,
            wasi: wasi_context(grants, &Workspace::default()),
            table: ResourceTable::new(),
            limiter: StoreLimiter::new(&limits),
            deadline: None,
            timed_out: false,
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
            .map_err(|error| LoadError::Thread {
                plugin: id.to_owned(),
                error,
            })?;
        let started = start.recv().unwrap_or_else(|_| {
            Err(LoadError::Unloaded(Diagnostic {
                plugin: id.to_owned(),
                reason: Reason::Trap,
                message: tr!("kiln-plugin-thread-stopped-loading").to_string(),
            }))
        });
        if let Err(error) = started {
            // The thread has dropped the store and is ending.
            drop(thread.join());
            return Err(error);
        }
        Ok(Self {
            id: id.to_owned(),
            state: PluginState::Running,
            channel: Some(Channel { calls, replies }),
            thread: Some(thread),
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

    /// Runs `call` on the plugin's thread and returns what it returned. A call that traps,
    /// overruns, runs out of memory or breaks protocol unloads the plugin first.
    pub fn call(&mut self, call: Call) -> Result<Reply, CallError> {
        let Some(channel) = &self.channel else {
            return Err(CallError::Unloaded);
        };
        let replied = match channel.calls.send(call) {
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

fn linker(engine: &PluginEngine) -> wasmtime::Result<Linker<Data>> {
    let mut linker = Linker::<Data>::new(engine.engine());
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    bindings::add_to_linker(&mut linker, |data| &mut data.host)?;
    compat::link(&mut linker)?;
    Ok(linker)
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
}

impl WasiView for Data {
    fn ctx(&mut self) -> WasiCtxView<'_> {
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
}

impl Exports {
    /// `instance`'s exports, named as `world` names them.
    fn new(store: &mut Store<Data>, instance: &Instance, world: World) -> wasmtime::Result<Self> {
        let mut export = |interface: &str, name: &str| {
            instance
                .get_export_index(&mut *store, None, interface)
                .and_then(|interface| {
                    instance.get_export_index(&mut *store, Some(&interface), name)
                })
                .ok_or_else(|| {
                    let export = format!("{interface}#{name}");
                    wasmtime::format_err!("{}", tr!("kiln-plugin-no-export", export = export))
                })
        };
        let run_command = export(&world.interface("command-handler"), "run-command")?;
        let handle_events = export(&world.interface("event-handler"), "handle-events")?;
        Ok(Self {
            init: instance.get_typed_func(&mut *store, "init")?,
            shutdown: instance.get_typed_func(&mut *store, "shutdown")?,
            run_command: instance.get_typed_func(&mut *store, run_command)?,
            handle_events: instance.get_typed_func(&mut *store, handle_events)?,
        })
    }
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
        calls: &Receiver<Call>,
        replies: &SyncSender<Result<Reply, Diagnostic>>,
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
        for call in calls {
            let reply = self.guarded(async |store| match call {
                Call::RunCommand { id, args } => exports
                    .run_command
                    .call_async(store, (id, args))
                    .await
                    .map(|(result,)| Reply::RunCommand(result)),
                Call::HandleEvents(events) => exports
                    .handle_events
                    .call_async(store, (events,))
                    .await
                    .map(|()| Reply::HandleEvents),
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
        let exports = Exports::new(&mut self.store, &instance, self.world).map_err(|error| {
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
