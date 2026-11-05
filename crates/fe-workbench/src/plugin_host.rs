//! The app's code plugins, a preview behind `extensions.plugins_preview`
//! (extensions record `docs/decisions/vmtnmkay62-extensions-and-plugins.md`
//! §7): WebAssembly components in the user's extensions directory, run on
//! kiln-plugin's engine with fe-plugin's `fastedit:editor` world.
//!
//! - **Gate.** The host follows the setting live. Off, no plugin is
//!   compiled, loaded or asked about, and one running unloads: its
//!   `shutdown` runs and its commands are withdrawn, while its grant record
//!   is kept. On, it's the same path as a reload.
//! - **Discovery.** A plugin is a user extension with `[contributes.plugin]`
//!   ([`set_plugin_extensions`]); a workspace's is refused. What the user
//!   granted each is in [`GRANTS_FILE`] in the state directory, decided with
//!   [`GrantStore::decide`].
//! - **Prompt.** A plugin that needs the user is a [`PendingPluginGrant`],
//!   shown once in the active window ([`PluginPrompt`]); a window activated
//!   later shows it while it's still pending. Allow records the grant,
//!   Don't Allow the decline; a prompt closed without an answer leaves it
//!   pending.
//! - **Load.** Reading, compiling and instantiating happen on the
//!   background executor, the compiled component cached under
//!   `<cache dir>/plugins`; each plugin then runs on its own thread
//!   ([`HostedPlugin`]) under kiln-plugin's [`Limits`]. Its kiln slices are
//!   live: commands on the host's [`CommandHost`], files in the trusted roots
//!   of the windows open at load (a plugin with an `fs` grant reloads when
//!   they change), secrets in the platform's secret store and the network
//!   through fe-net. The editor's slices answer `unimplemented` until their
//!   cards land.
//! - **Reload.** A changed manifest or component reloads the plugin, its old
//!   version shut down before the new one's `init`; with the same grants
//!   nothing is asked. An update that asks for more keeps the old version
//!   running until the user answers.
//! - **Commands.** A plugin's commands, `<id>.<name>`, are listed in the
//!   command palette while its load runs, and a workbench runs them
//!   ([`dispatch`]) off the main thread. Unloading withdraws them.
//!
//! The extensions list reads [`plugin_states`] and calls [`unload_plugin`]
//! and [`reevaluate`].

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsString,
    fmt, fs,
    future::poll_fn,
    io,
    io::Read as _,
    path::{Path, PathBuf},
    pin::pin,
    sync::{
        Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    task::{Poll, Waker},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use fe_command::{
    CommandId, CommandMeta, ContextPredicate, DispatchError, Invocation, Value,
    capability::Capability,
};
use fe_plugin::{EditorExtensionPre, net::FeNet};
use futures::{
    channel::oneshot,
    future::{self, Either},
};
use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, Global, KeyContext, Subscription, Task, Window,
    WindowId,
};
use kiln::core::{
    extension::{ExtensionSet, ExtensionSource, Manifest},
    secrets::{KeychainStore, MemoryStore, Secret, SecretError, SecretStore},
};
use kiln_plugin::{
    bindings::{self, Level, LogSink, kiln::extension::events::Event},
    cache::ComponentCache,
    commands::{CallPlugin, CommandHost, Dispatcher, PluginCommand, Request},
    compat,
    engine::PluginEngine,
    fs::Workspace,
    grants::{Decision, GrantRecord, GrantStore, ProgramIdentity, Prompt, resolve_program},
    limits::{Limits, PROGRAM_DEADLINE, StoreLimiter},
    manifest::{ExtensionOrigin, PluginManifest},
    process::SYSTEM_PATH,
    runner::{Call, CallError, Diagnostic, LoadError, Reason, Reply},
    secrets::PluginSecrets,
};
use sha2::{Digest as _, Sha256};
use wasmtime::{
    Engine, Store, Trap, UpdateDeadline,
    component::{Component, HasSelf, Instance, InstancePre, Linker, ResourceTable, TypedFunc},
};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

pub use kiln_plugin::grants::GRANTS_FILE;
use kiln_plugin::grants::Version;
pub use kiln_plugin::manifest::plugin_kind;

use crate::{
    KEY_CONTEXT, PendingPluginGrant, PluginAnswer, PluginAsk, RootChanged, TrustChanged, Workbench,
    show_plugin_prompt,
};

/// The file in the state directory each plugin's secret names are kept in.
pub const SECRETS_INDEX_FILE: &str = "plugin-secrets.toml";

/// The directory in the cache directory compiled plugins are kept in.
pub const CACHE_DIR: &str = "plugins";

/// What a plugin that doesn't link against this host's world is told.
pub const REBUILD: &str = "the plugin API is a preview and can change in any release; rebuild \
                           the plugin against this release";

/// Where a plugin stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginState {
    /// Loaded, its commands registered.
    Running,
    /// Waiting for the user to grant what it asks for.
    WaitingForGrant,
    /// The user declined what it asks for: it isn't asked about again until
    /// the request changes.
    Declined,
    /// It failed, or was unloaded.
    Stopped(PluginDiagnostic),
    /// `extensions.plugins_preview` is off.
    PreviewOff,
}

/// Why a plugin stopped, for the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginDiagnostic {
    pub reason: StopReason,
    pub message: String,
}

impl fmt::Display for PluginDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.reason, self.message)
    }
}

/// What stopped a plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// Its code trapped.
    Trap,
    /// A call into it ran past its deadline.
    Timeout,
    /// It failed after being refused memory.
    Memory,
    /// It broke the world's contract.
    Protocol,
    /// It couldn't be loaded: its manifest, its component or its `init`.
    Load,
    /// What it was granted couldn't be read or recorded.
    Grants,
    /// [`unload_plugin`] unloaded it.
    Unloaded,
}

impl From<Reason> for StopReason {
    fn from(reason: Reason) -> Self {
        match reason {
            Reason::Trap => Self::Trap,
            Reason::Timeout => Self::Timeout,
            Reason::Memory => Self::Memory,
            Reason::Protocol => Self::Protocol,
        }
    }
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Trap => "trapped",
            Self::Timeout => "timed out",
            Self::Memory => "out of memory",
            Self::Protocol => "protocol error",
            Self::Load => "failed to load",
            Self::Grants => "grants unavailable",
            Self::Unloaded => "unloaded",
        })
    }
}

impl From<Diagnostic> for PluginDiagnostic {
    fn from(diagnostic: Diagnostic) -> Self {
        Self {
            reason: diagnostic.reason.into(),
            message: diagnostic.message,
        }
    }
}

/// A plugin as the extensions list shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: Version,
    /// The extension's directory.
    pub dir: PathBuf,
    /// What its manifest asks for.
    pub requested: fe_command::Grants,
    /// What the user granted it.
    pub granted: fe_command::Grants,
    pub state: PluginState,
}

/// Has the host keep its grants and secrets' index in `state_dir` and its
/// compiled plugins in `cache_dir`/[`CACHE_DIR`], in place of the app's
/// directories. The plugins are decided again.
pub fn set_plugin_dirs(cx: &mut App, state_dir: PathBuf, cache_dir: PathBuf) {
    let host = host_mut(cx);
    host.dirs = Some(Dirs {
        state: state_dir,
        cache: cache_dir,
    });
    host.cache = None;
    evaluate(cx, true);
}

/// The plugins among `set`'s extensions, from the extensions service: each
/// user extension with `[contributes.plugin]`. One a workspace contributes
/// is refused, with a warning. The plugins are decided again.
pub fn set_plugin_extensions(cx: &mut App, set: &ExtensionSet) {
    let found = set
        .sourced()
        .filter_map(|(source, manifest)| {
            let origin = match source {
                ExtensionSource::User => ExtensionOrigin::User,
                ExtensionSource::Workspace => ExtensionOrigin::Workspace,
            };
            match PluginManifest::from_extension(manifest, origin) {
                Ok(plugin) => plugin.map(|plugin| Found::new(manifest, Ok(plugin))),
                Err(error) if origin == ExtensionOrigin::Workspace => {
                    log::warn!("{error}");
                    None
                }
                Err(error) => Some(Found::new(manifest, Err(error.to_string()))),
            }
        })
        .collect();
    set_plugins(cx, found);
}

/// Every plugin found, in extension order, and where it stands.
pub fn plugin_states(cx: &App) -> Vec<PluginInfo> {
    let Some(host) = cx.try_global::<PluginHost>() else {
        return Vec::new();
    };
    host.found
        .iter()
        .filter_map(|found| {
            let slot = host.slots.get(&found.id)?;
            Some(PluginInfo {
                id: found.id.clone(),
                name: found.name.clone(),
                version: found.version,
                dir: found.dir.clone(),
                requested: found
                    .plugin
                    .as_ref()
                    .map(|plugin| plugin.capabilities.clone())
                    .unwrap_or_default(),
                granted: slot.granted.clone(),
                state: slot.state.clone(),
            })
        })
        .collect()
}

/// The plugins waiting for the user's grant, in extension order.
pub fn pending_plugin_grants(cx: &App) -> Vec<PendingPluginGrant> {
    let Some(host) = cx.try_global::<PluginHost>() else {
        return Vec::new();
    };
    host.found
        .iter()
        .filter_map(|found| host.slots.get(&found.id)?.pending.clone())
        .collect()
}

/// Unloads plugin `id`: its `shutdown` runs and its commands are withdrawn,
/// and a load of it under way is cancelled. It stays stopped until
/// [`reevaluate`] or a change to it.
pub fn unload_plugin(id: &str, cx: &mut App) {
    let Some(slot) = cx
        .try_global::<PluginHost>()
        .and_then(|host| host.slots.get(id))
    else {
        return;
    };
    let identity = match &slot.want {
        Want::Load { identity, .. } => Some(identity.clone()),
        _ => slot.target().map(|(identity, _)| identity),
    };
    let diagnostic = PluginDiagnostic {
        reason: StopReason::Unloaded,
        message: format!("plugin {id} was unloaded"),
    };
    let host = cx.global_mut::<PluginHost>();
    let slot = host.slots.get_mut(id).expect("found above");
    slot.stopped = identity.map(|identity| (identity, diagnostic.clone()));
    slot.set_want(Want::Unload(PluginState::Stopped(diagnostic)));
    drive(id, cx);
}

/// Decides every plugin again from its manifest, its component and its
/// grants, restarting those that stopped.
pub fn reevaluate(cx: &mut App) {
    evaluate(cx, true);
}

/// Runs `run`'s command if a running plugin registered it, off the main
/// thread, and returns true; false otherwise. One that doesn't apply in
/// `run`'s context, or whose args don't fit, is refused, as the command
/// registry refuses one. A call that unloads the plugin stops it, with its
/// diagnostic.
pub(crate) fn dispatch(run: &fe_gpui::commands::RunCommand, cx: &mut App) -> bool {
    let Some(host) = cx.try_global::<PluginHost>() else {
        return false;
    };
    let found = host.slots.iter().find_map(|(id, slot)| {
        let loaded = slot.loaded.as_ref()?;
        let command = loaded
            .commands
            .iter()
            .find(|command| *command.id() == run.0.command)?;
        Some((
            id.clone(),
            loaded.load,
            command.clone(),
            loaded.plugin.clone(),
        ))
    });
    let Some((id, load, command, plugin)) = found else {
        return false;
    };
    if let Err(error) = check_invocation(command.meta(), &run.0) {
        log::warn!("{error}");
        return true;
    }
    let args = run.0.args.clone();
    let ran = cx.background_spawn(async move {
        let ran = command.run(&args);
        let stopped = match lock(&plugin).state() {
            kiln_plugin::runner::PluginState::Unloaded(diagnostic) => Some(diagnostic.clone()),
            kiln_plugin::runner::PluginState::Running => None,
        };
        (ran, stopped)
    });
    cx.spawn(async move |cx| {
        let (ran, stopped) = ran.await;
        if let Err(error) = ran {
            log::warn!("{error}");
        }
        if let Some(diagnostic) = stopped {
            cx.update(|cx| stopped_running(&id, load, diagnostic, cx));
        }
    })
    .detach();
    true
}

/// Whether `invocation` may run `meta`'s command, as the command registry
/// checks one before running it: it applies in the invocation's context,
/// and its args fit. A command without an args schema takes none; one with
/// takes a table (or nothing, read as an empty table) holding no null and
/// every key the schema's `required` names.
fn check_invocation(meta: &CommandMeta, invocation: &Invocation) -> Result<(), DispatchError> {
    let command = meta.id.clone();
    if let Some(predicate) = &meta.context
        && !predicate.eval(&invocation.context)
    {
        let predicate = predicate.clone();
        return Err(DispatchError::NotInContext { command, predicate });
    }
    let bad = |message: String| Err(DispatchError::BadArgs { command, message });
    let empty = serde_json::Map::new();
    let table = match &invocation.args {
        Value::Null => &empty,
        Value::Object(table) => table,
        _ if meta.args_schema.is_none() => return bad("takes no args".into()),
        _ => return bad("args must be a table".into()),
    };
    let Some(schema) = &meta.args_schema else {
        return if table.is_empty() {
            Ok(())
        } else {
            bad("takes no args".into())
        };
    };
    if table.values().any(has_null) {
        return bad("args can't hold a null".into());
    }
    let required = schema.get("required").and_then(Value::as_array);
    let missing = required
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find(|key| !table.contains_key(*key));
    match missing {
        Some(key) => bad(format!("args need `{key}`")),
        None => Ok(()),
    }
}

fn has_null(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.iter().any(has_null),
        Value::Object(table) => table.values().any(has_null),
        _ => false,
    }
}

/// The workbench's key context with a flag for each load whose commands are
/// listed, so the palette lists them; none while no plugin has commands.
pub(crate) fn key_context(cx: &App) -> Option<KeyContext> {
    let host = cx.try_global::<PluginHost>()?;
    let mut flags = host
        .slots
        .values()
        .filter_map(|slot| slot.loaded.as_ref())
        .filter(|loaded| !loaded.commands.is_empty())
        .map(|loaded| loaded.flag())
        .peekable();
    flags.peek()?;
    let mut context = KeyContext::default();
    context.add(KEY_CONTEXT);
    flags.for_each(|flag| context.add(flag));
    Some(context)
}

/// The trusted roots of the open windows changed, maybe: those of plugins
/// with an `fs` grant reload if so.
pub(crate) fn workspace_changed(cx: &mut App) {
    if !cx.has_global::<PluginHost>() {
        return;
    }
    let roots = trusted_roots(cx);
    set_workspace_roots(cx, roots);
}

/// Has plugins reach `roots` as the workspace: a load under way for other
/// roots is cancelled at once, before its `init`, and the plugins with an
/// `fs` grant reload on them.
pub(crate) fn set_workspace_roots(cx: &mut App, roots: Vec<PathBuf>) {
    let host = host_mut(cx);
    if host.workspace == roots {
        return;
    }
    host.workspace = roots.clone();
    // What reaches files is wanted on the new roots now, not once decided
    // again: a load on the old ones is cancelled, and none starts.
    for slot in host.slots.values_mut() {
        let Want::Load { identity, artifact } = &slot.want else {
            continue;
        };
        if identity.workspace.is_some() && identity.workspace.as_ref() != Some(&roots) {
            let identity = Identity {
                workspace: Some(roots.clone()),
                ..identity.clone()
            };
            let artifact = artifact.clone();
            slot.set_want(Want::Load { identity, artifact });
        }
    }
    if let Some(commands) = &host.commands {
        commands.set_workspace(Workspace { roots });
    }
    drive_all(cx);
    evaluate(cx, false);
}

/// [`drive`]s every plugin.
fn drive_all(cx: &mut App) {
    let ids: Vec<String> = cx.global::<PluginHost>().slots.keys().cloned().collect();
    for id in ids {
        drive(&id, cx);
    }
}

/// Turns the preview on or off, as the settings do. Off, at once, plugins
/// unload, loads under way are cancelled and prompts showing decide
/// nothing.
pub(crate) fn set_preview(cx: &mut App, on: bool) {
    let host = host_mut(cx);
    if host.preview == on {
        return;
    }
    host.preview = on;
    if !on {
        for slot in host.slots.values_mut() {
            slot.pending = None;
            slot.set_want(Want::Unload(PluginState::PreviewOff));
        }
        cx.default_global::<Asking>().prompts.clear();
        drive_all(cx);
    }
    evaluate(cx, on);
}

/// A plugin found in the extensions: its manifest's plugin entry, or why it
/// has none.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Found {
    id: String,
    name: String,
    version: Version,
    license: String,
    dir: PathBuf,
    plugin: Result<PluginManifest, String>,
}

impl Found {
    pub(crate) fn new(manifest: &Manifest, plugin: Result<PluginManifest, String>) -> Self {
        Self {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version,
            license: manifest.license.expr.clone(),
            dir: manifest.dir.clone(),
            plugin,
        }
    }
}

/// Hands the host the plugins found, and decides them again. One no longer
/// found unloads at once, a load of it under way cancelled; one whose
/// manifest changed has no request pending until it's decided again.
pub(crate) fn set_plugins(cx: &mut App, found: Vec<Found>) {
    let host = host_mut(cx);
    for (id, slot) in &mut host.slots {
        let now = found.iter().find(|found| found.id == *id);
        let before = host.found.iter().find(|found| found.id == *id);
        match now {
            None => {
                slot.gone = true;
                slot.pending = None;
                slot.set_want(Want::Unload(PluginState::PreviewOff));
            }
            // Its request is decided again: what it asked before isn't
            // pending meanwhile.
            Some(now) if Some(now) != before => slot.pending = None,
            Some(_) => {}
        }
    }
    host.found = found;
    invalidate_prompts(cx);
    drive_all(cx);
    evaluate(cx, false);
}

/// The host's directories.
#[derive(Clone, Debug)]
struct Dirs {
    state: PathBuf,
    cache: PathBuf,
}

/// The app's plugins and what runs them.
pub(crate) struct PluginHost {
    dirs: Option<Dirs>,
    cache: Option<Arc<ComponentCache>>,
    secrets: Arc<dyn SecretStore>,
    preview: bool,
    found: Vec<Found>,
    slots: HashMap<String, Slot>,
    /// Made with the first load, once the app's commands are registered.
    commands: Option<CommandHost>,
    /// The trusted roots of the open windows.
    workspace: Vec<PathBuf>,
    evaluating: Option<Task<()>>,
    /// The palette entries listed so far, by load: each is listed once.
    listed: HashSet<u64>,
    /// The plugins whose answer is being recorded: an evaluation that
    /// started before doesn't make them pending again.
    answering: HashSet<String>,
    /// What the plugins did, for the tests.
    #[cfg(test)]
    pub(crate) record: Arc<Mutex<Vec<String>>>,
    /// Under test, each load waits for a message here before it starts.
    #[cfg(test)]
    pub(crate) load_gate: Option<async_channel::Receiver<()>>,
}

impl Global for PluginHost {}

/// One plugin.
struct Slot {
    granted: fe_command::Grants,
    state: PluginState,
    pending: Option<PendingPluginGrant>,
    loaded: Option<Loaded>,
    /// What the last evaluation decided, done once no load or unload is
    /// under way.
    want: Want,
    /// Whether a load or an unload is under way.
    busy: bool,
    /// The load under way.
    loading: Option<Loading>,
    /// What stopped, and why, which isn't loaded again until it changes or
    /// [`reevaluate`] is called: the slot shows why while it's wanted.
    stopped: Option<(Identity, PluginDiagnostic)>,
    /// Whether it's no longer found: it's dropped once unloaded.
    gone: bool,
}

impl Slot {
    fn new() -> Self {
        Self {
            granted: fe_command::Grants::new(),
            state: PluginState::PreviewOff,
            pending: None,
            loaded: None,
            want: Want::Keep,
            busy: false,
            loading: None,
            stopped: None,
            gone: false,
        }
    }

    /// Sets what the slot should have, cancelling a load under way that it
    /// no longer wants.
    fn set_want(&mut self, want: Want) {
        let wanted = match &want {
            Want::Load { identity, .. } => self
                .loading
                .as_ref()
                .is_some_and(|loading| loading.identity == *identity),
            Want::Keep => true,
            Want::Unload(_) => false,
        };
        if !wanted {
            self.cancel_load();
        }
        self.want = want;
    }

    fn cancel_load(&self) {
        if let Some(loading) = &self.loading {
            loading.cancel.store(true, Ordering::Release);
        }
    }

    /// What the slot runs, or is loading and hasn't cancelled: the load
    /// under way first, as the newer.
    fn target(&self) -> Option<(Identity, Arc<Artifact>)> {
        let loading = self
            .loading
            .as_ref()
            .filter(|loading| !loading.cancel.load(Ordering::Acquire))
            .map(|loading| (loading.identity.clone(), loading.artifact.clone()));
        loading.or_else(|| {
            let loaded = self.loaded.as_ref()?;
            Some((loaded.identity.clone(), loaded.artifact.clone()))
        })
    }
}

/// A load under way, and what cancels it: set, the load stops before
/// compiling, before instantiating and before `init`.
struct Loading {
    identity: Identity,
    artifact: Arc<Artifact>,
    cancel: Arc<AtomicBool>,
}

/// What a load ran: its component, grants, the programs its spawn grants
/// are bound to and those suspended, and, with an `fs` grant, roots.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    sha256: [u8; 32],
    version: Version,
    grants: fe_command::Grants,
    programs: BTreeMap<String, ProgramIdentity>,
    suspended: Vec<String>,
    workspace: Option<Vec<PathBuf>>,
}

/// What a load is made from: the plugin's manifest, its component and its
/// grant record, whose spawn grants it runs with.
struct Artifact {
    manifest: PluginManifest,
    bytes: Vec<u8>,
    record: GrantRecord,
}

/// What a slot should have.
enum Want {
    /// This load running.
    Load {
        identity: Identity,
        artifact: Arc<Artifact>,
    },
    /// What runs, if anything, kept as it is: while an update that asks for
    /// more waits, or was declined.
    Keep,
    /// Nothing running, in this state.
    Unload(PluginState),
}

/// A plugin running.
struct Loaded {
    /// Which load of the session it is.
    load: u64,
    identity: Identity,
    /// What it was loaded from, to load it again on other roots.
    artifact: Arc<Artifact>,
    plugin: Arc<Mutex<HostedPlugin>>,
    commands: Vec<PluginCommand>,
}

impl Loaded {
    /// The key-context flag its palette entries need.
    fn flag(&self) -> String {
        format!("plugin_load_{}", self.load)
    }
}

/// The host, made on first use, following the settings' preview.
fn host_mut(cx: &mut App) -> &mut PluginHost {
    if !cx.has_global::<PluginHost>() {
        let preview = cx
            .try_global::<kiln::GlobalSettings<fe_config::Settings>>()
            .is_some_and(|global| global.settings().extensions.plugins_preview);
        cx.set_global(PluginHost {
            dirs: default_dirs(),
            cache: None,
            secrets: secret_store(),
            preview,
            found: Vec::new(),
            slots: HashMap::new(),
            commands: None,
            workspace: Vec::new(),
            evaluating: None,
            listed: HashSet::new(),
            answering: HashSet::new(),
            #[cfg(test)]
            record: Arc::default(),
            #[cfg(test)]
            load_gate: None,
        });
        cx.observe_global::<kiln::GlobalSettings<fe_config::Settings>>(|cx| {
            let on = cx
                .global::<kiln::GlobalSettings<fe_config::Settings>>()
                .settings()
                .extensions
                .plugins_preview;
            set_preview(cx, on);
        })
        .detach();
    }
    cx.global_mut::<PluginHost>()
}

/// The app's state and cache directories; none under test, where each sets
/// its own.
fn default_dirs() -> Option<Dirs> {
    if cfg!(test) {
        return None;
    }
    let paths = fe_config::paths::app_paths();
    Some(Dirs {
        state: paths.state_dir()?.to_owned(),
        cache: paths.cache_dir()?.to_owned(),
    })
}

/// The platform keychain, opened on first use; under test, memory.
fn secret_store() -> Arc<dyn SecretStore> {
    if cfg!(test) {
        return Arc::new(MemoryStore::new());
    }
    static KEYCHAIN: OnceLock<Arc<Keychain>> = OnceLock::new();
    KEYCHAIN
        .get_or_init(|| Arc::new(Keychain::default()))
        .clone()
}

/// The keychain, opened on its first use, on the plugin's thread: opening it
/// connects to the Secret Service on Linux. One that fails to open is
/// opened again next time.
#[derive(Default)]
struct Keychain(OnceLock<KeychainStore>);

impl Keychain {
    fn store(&self) -> Result<&KeychainStore, SecretError> {
        if let Some(store) = self.0.get() {
            return Ok(store);
        }
        let store = KeychainStore::new()?;
        Ok(self.0.get_or_init(|| store))
    }
}

impl SecretStore for Keychain {
    fn get(&self, service: &str, account: &str) -> Result<Option<Secret>, SecretError> {
        self.store()?.get(service, account)
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), SecretError> {
        self.store()?.set(service, account, secret)
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError> {
        self.store()?.delete(service, account)
    }
}

/// The trusted roots of the windows open now.
fn trusted_roots(cx: &App) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = cx
        .windows()
        .into_iter()
        .filter_map(|window| window.downcast::<Workbench>())
        .filter_map(|window| window.read(cx).ok())
        .filter(|workbench| workbench.root_trusted())
        .filter_map(|workbench| workbench.root().map(Path::to_owned))
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

/// What deciding a plugin found.
enum Outcome {
    /// The preview is off.
    Off,
    /// It can't load, as `reason` says.
    Failed(StopReason, String),
    /// Load it with these grants, and the record they're from.
    Load {
        grants: fe_command::Grants,
        sha256: [u8; 32],
        bytes: Vec<u8>,
        record: Box<GrantRecord>,
    },
    Ask(PluginAsk),
    Declined,
}

/// Decides each plugin again off the main thread, then does what that
/// decided. `force` restarts those that stopped.
fn evaluate(cx: &mut App, force: bool) {
    let host = host_mut(cx);
    if force {
        host.slots.values_mut().for_each(|slot| slot.stopped = None);
    }
    let preview = host.preview;
    let found = host.found.clone();
    let store = host
        .dirs
        .as_ref()
        .map(|dirs| GrantStore::new(dirs.state.join(GRANTS_FILE)));
    let task = cx.spawn(async move |cx| {
        let outcomes = cx
            .background_spawn(async move { decide(preview, &found, store.as_ref()) })
            .await;
        cx.update(|cx| apply(outcomes, cx));
    });
    cx.global_mut::<PluginHost>().evaluating = Some(task);
}

/// What each of `found` comes to, with what it was granted: blocking.
fn decide(
    preview: bool,
    found: &[Found],
    store: Option<&GrantStore>,
) -> Vec<(String, fe_command::Grants, Outcome)> {
    let path_var = std::env::var_os("PATH").unwrap_or_else(|| OsString::from(SYSTEM_PATH));
    let record = |id: &str| {
        let record = store.map(|store| store.get(id));
        match record {
            Some(Ok(Some(record))) => record,
            Some(Err(error)) => {
                log::warn!("can't read plugin {id}'s grants: {error}");
                GrantRecord::default()
            }
            _ => GrantRecord::default(),
        }
    };
    found
        .iter()
        .map(|found| {
            let id = found.id.clone();
            let plugin = match &found.plugin {
                Ok(plugin) => plugin,
                Err(error) => {
                    let outcome = Outcome::Failed(StopReason::Load, error.clone());
                    return (id, fe_command::Grants::new(), outcome);
                }
            };
            if !preview {
                return (id.clone(), record(&id).granted, Outcome::Off);
            }
            let Some(store) = store else {
                let message = "no state directory to keep its grants in".to_owned();
                let outcome = Outcome::Failed(StopReason::Grants, message);
                return (id, fe_command::Grants::new(), outcome);
            };
            if let Err(error) = store.check_programs(&id) {
                log::warn!("can't check plugin {id}'s programs: {error}");
            }
            let decision = store.decide(&id, plugin.version, &plugin.capabilities, &path_var);
            let granted = record(&id);
            let outcome = match decision {
                Ok(Decision::Load(grants)) => match read_component(&plugin.wasm) {
                    Ok(bytes) => Outcome::Load {
                        grants,
                        sha256: Sha256::digest(&bytes).into(),
                        bytes,
                        record: Box::new(granted.clone()),
                    },
                    Err(message) => Outcome::Failed(StopReason::Load, message),
                },
                Ok(Decision::Prompt(Prompt::NewPlugin)) => Outcome::Ask(PluginAsk::Install),
                Ok(Decision::Escalation {
                    added,
                    keep_running,
                }) => Outcome::Ask(PluginAsk::Escalation {
                    added,
                    keep_running,
                }),
                Ok(Decision::Declined) => Outcome::Declined,
                Err(error) => Outcome::Failed(StopReason::Grants, error.to_string()),
            };
            (id, granted.granted, outcome)
        })
        .collect()
}

/// The component at `path`, refused over the read limit.
fn read_component(path: &Path) -> Result<Vec<u8>, String> {
    let limit = Limits::default().max_read_bytes;
    let file = fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() > limit {
        return Err(format!("{} is over the {limit}-byte limit", path.display()));
    }
    Ok(bytes)
}

/// Sets each plugin's want from what deciding it found, and does it. A
/// version kept running (or loading) while its update waits, or was
/// declined, stays wanted, reloading on new roots with its own grants if it
/// reaches the workspace's files.
fn apply(outcomes: Vec<(String, fe_command::Grants, Outcome)>, cx: &mut App) {
    let host = cx.global_mut::<PluginHost>();
    let workspace = host.workspace.clone();
    let mut ids = Vec::new();
    let found: HashMap<&str, &Found> = host
        .found
        .iter()
        .map(|found| (found.id.as_str(), found))
        .collect();
    let mut wants = Vec::new();
    for (id, granted, outcome) in outcomes {
        let Some(found) = found.get(id.as_str()) else {
            continue;
        };
        let pending = |ask: PluginAsk| {
            let plugin = found.plugin.as_ref().ok()?;
            Some(PendingPluginGrant {
                id: found.id.clone(),
                name: found.name.clone(),
                version: found.version,
                license: found.license.clone(),
                dir: found.dir.clone(),
                requested: plugin.capabilities.clone(),
                ask,
            })
        };
        let (want, pending) = match outcome {
            Outcome::Off => (Want::Unload(PluginState::PreviewOff), None),
            Outcome::Failed(reason, message) => {
                log::warn!("plugin {id} {reason}: {message}");
                let state = PluginState::Stopped(PluginDiagnostic { reason, message });
                (Want::Unload(state), None)
            }
            Outcome::Load {
                grants,
                sha256,
                bytes,
                record,
            } => {
                let identity = Identity {
                    sha256,
                    version: found.version,
                    workspace: reaches_files(&grants).then(|| workspace.clone()),
                    grants,
                    programs: record.programs.clone(),
                    suspended: record.suspended.keys().cloned().collect(),
                };
                let artifact = Arc::new(Artifact {
                    manifest: found.plugin.clone().expect("only a manifest loads"),
                    bytes,
                    record: *record,
                });
                (Want::Load { identity, artifact }, None)
            }
            Outcome::Ask(PluginAsk::Install) => (
                Want::Unload(PluginState::WaitingForGrant),
                pending(PluginAsk::Install),
            ),
            Outcome::Ask(ask) => (Want::Keep, pending(ask)),
            Outcome::Declined => (Want::Keep, None),
        };
        ids.push(id.clone());
        wants.push((id, granted, want, pending));
    }
    for (id, granted, want, pending) in wants {
        let slot = host.slots.entry(id).or_insert_with(Slot::new);
        slot.granted = granted;
        slot.gone = false;
        // Kept: the version it wanted, else the one it runs or is loading,
        // stays wanted, on the roots now if it reaches files.
        let want = match want {
            Want::Keep => {
                let kept = match &slot.want {
                    Want::Load { identity, artifact } => Some((identity.clone(), artifact.clone())),
                    _ => slot.target(),
                };
                match kept {
                    Some((identity, artifact)) => {
                        let identity = Identity {
                            workspace: identity.workspace.as_ref().map(|_| workspace.clone()),
                            ..identity
                        };
                        Want::Load { identity, artifact }
                    }
                    None => Want::Keep,
                }
            }
            want => want,
        };
        // Nothing kept: it waits or was declined, unless it was stopped.
        if matches!(want, Want::Keep) && slot.stopped.is_none() {
            slot.state = match &pending {
                Some(_) => PluginState::WaitingForGrant,
                None => PluginState::Declined,
            };
        }
        slot.set_want(want);
        slot.pending = pending.filter(|pending| !host.answering.contains(&pending.id));
    }
    for (id, slot) in &mut host.slots {
        if !ids.contains(id) {
            slot.gone = true;
            slot.pending = None;
            slot.set_want(Want::Unload(PluginState::PreviewOff));
            ids.push(id.clone());
        }
    }
    invalidate_prompts(cx);
    for id in ids {
        drive(&id, cx);
    }
}

/// Whether `grants` reach the workspace's files.
fn reaches_files(grants: &fe_command::Grants) -> bool {
    grants
        .iter()
        .any(|capability| matches!(capability, Capability::FsRead(_) | Capability::FsWrite(_)))
}

/// Starts what plugin `id`'s want needs, unless a load or an unload of it
/// is under way: that one's end drives it again.
fn drive(id: &str, cx: &mut App) {
    let host = cx.global_mut::<PluginHost>();
    let Some(slot) = host.slots.get_mut(id) else {
        return;
    };
    if slot.busy {
        return;
    }
    match &slot.want {
        Want::Keep => {}
        Want::Load { identity, .. } => {
            let stopped = (slot.stopped.as_ref())
                .filter(|(stopped, _)| stopped == identity)
                .map(|(_, diagnostic)| diagnostic.clone());
            let running = (slot.loaded.as_ref()).is_some_and(|loaded| loaded.identity == *identity);
            match stopped {
                None if running => slot.state = PluginState::Running,
                None => start_load(id, cx),
                Some(_) if slot.loaded.is_some() => start_unload(id, cx),
                Some(diagnostic) => slot.state = PluginState::Stopped(diagnostic),
            }
        }
        Want::Unload(state) => {
            if slot.loaded.is_some() {
                start_unload(id, cx);
            } else {
                slot.state = state.clone();
                if slot.gone {
                    host.slots.remove(id);
                }
            }
        }
    }
    cx.refresh_windows();
}

/// Takes plugin `id`'s load off the command host and out of the palette,
/// returning it to be shut down.
fn withdraw(id: &str, host: &mut PluginHost) -> Option<Loaded> {
    let loaded = host.slots.get_mut(id)?.loaded.take()?;
    if let Some(commands) = &host.commands {
        commands.unload(id);
    }
    Some(loaded)
}

/// Unloads plugin `id`'s load off the main thread, then puts it in the
/// state its want says.
fn start_unload(id: &str, cx: &mut App) {
    let host = cx.global_mut::<PluginHost>();
    let Some(loaded) = withdraw(id, host) else {
        return;
    };
    host.slots.get_mut(id).expect("withdrawn").busy = true;
    let shut_down = cx.background_spawn(async move { lock(&loaded.plugin).shutdown() });
    let id = id.to_owned();
    cx.spawn(async move |cx| {
        shut_down.await;
        cx.update(|cx| {
            let host = cx.global_mut::<PluginHost>();
            if let Some(slot) = host.slots.get_mut(&id) {
                slot.busy = false;
                if let Want::Unload(state) = &slot.want {
                    slot.state = state.clone();
                }
            }
            drive(&id, cx);
        });
    })
    .detach();
}

/// Loads plugin `id` as its want says, off the main thread: what ran
/// before is shut down first. Then it runs, its commands listed, or stops
/// with why, unless it was cancelled.
fn start_load(id: &str, cx: &mut App) {
    static LOADS: AtomicU64 = AtomicU64::new(0);
    let builtin = cx
        .global::<PluginHost>()
        .commands
        .is_none()
        .then(|| builtin_commands(cx));
    let host = cx.global_mut::<PluginHost>();
    if let Some(builtin) = builtin {
        let commands = CommandHost::new(builtin.iter().cloned(), Arc::new(NoInvoke));
        let namespaces: HashSet<&str> = builtin.iter().map(CommandId::namespace).collect();
        namespaces
            .into_iter()
            .for_each(|namespace| commands.reserve(namespace));
        commands.set_workspace(Workspace {
            roots: host.workspace.clone(),
        });
        host.commands = Some(commands);
    }
    let old = withdraw(id, host);
    let dirs = host.dirs.clone();
    if host.cache.is_none() {
        host.cache = dirs
            .as_ref()
            .map(|dirs| Arc::new(ComponentCache::new(dirs.cache.join(CACHE_DIR))));
    }
    let context = LoadContext {
        cache: host.cache.clone(),
        commands: host.commands.clone().expect("made above"),
        secrets: host.secrets.clone(),
        secrets_index: dirs
            .as_ref()
            .map(|dirs| dirs.state.join(SECRETS_INDEX_FILE)),
        store: dirs.map(|dirs| GrantStore::new(dirs.state.join(GRANTS_FILE))),
        workspace: Workspace {
            roots: host.workspace.clone(),
        },
        record: record(host),
        ticker,
    };
    #[cfg(test)]
    let gate = host.load_gate.clone();
    let slot = host.slots.get_mut(id).expect("driven");
    let Want::Load { identity, artifact } = &slot.want else {
        return;
    };
    let (identity, artifact) = (identity.clone(), artifact.clone());
    let cancel = Arc::new(AtomicBool::new(false));
    slot.busy = true;
    slot.loading = Some(Loading {
        identity: identity.clone(),
        artifact: artifact.clone(),
        cancel: cancel.clone(),
    });
    let load = LOADS.fetch_add(1, Ordering::Relaxed) + 1;
    let grants = identity.grants.clone();
    let loading_artifact = artifact.clone();
    let loading = cx.background_spawn(async move {
        #[cfg(test)]
        if let Some(gate) = gate {
            gate.recv().await.ok();
        }
        if let Some(old) = old {
            lock(&old.plugin).shutdown();
        }
        context.load(&loading_artifact, &grants, &cancel)
    });
    let id = id.to_owned();
    cx.spawn(async move |cx| {
        let loaded = loading.await;
        cx.update(|cx| loaded_plugin(&id, load, identity, artifact, loaded, cx));
    })
    .detach();
}

/// Why a load didn't start a plugin.
enum LoadFailure {
    /// It was cancelled: nothing of it ran.
    Cancelled,
    Failed(String),
}

/// What a load finished with: its plugin running, or why it stopped. A load
/// the slot no longer wants changes nothing but what runs, which its want
/// then replaces.
fn loaded_plugin(
    id: &str,
    load: u64,
    identity: Identity,
    artifact: Arc<Artifact>,
    loaded: Result<(Arc<Mutex<HostedPlugin>>, Vec<PluginCommand>), LoadFailure>,
    cx: &mut App,
) {
    let host = cx.global_mut::<PluginHost>();
    let Some(slot) = host.slots.get_mut(id) else {
        return;
    };
    slot.busy = false;
    slot.loading = None;
    let wanted = match &slot.want {
        Want::Load {
            identity: wanted, ..
        } => *wanted == identity,
        Want::Keep => true,
        Want::Unload(_) => false,
    };
    let mut listed = Vec::new();
    match loaded {
        Ok((plugin, commands)) => {
            let loaded = Loaded {
                load,
                identity,
                artifact,
                plugin,
                commands,
            };
            if wanted && host.listed.insert(load) {
                let flag = ContextPredicate::identifier(loaded.flag());
                listed = loaded
                    .commands
                    .iter()
                    .map(|command| {
                        let mut meta: CommandMeta = command.meta().clone();
                        meta.context = Some(match meta.context.take() {
                            Some(context) => {
                                ContextPredicate::And(Box::new(flag.clone()), Box::new(context))
                            }
                            None => flag.clone(),
                        });
                        meta
                    })
                    .collect();
            }
            let slot = host.slots.get_mut(id).expect("found above");
            slot.loaded = Some(loaded);
            if wanted {
                slot.stopped = None;
                slot.state = PluginState::Running;
            }
        }
        Err(LoadFailure::Cancelled) => {}
        Err(LoadFailure::Failed(message)) => {
            log::warn!("plugin {id} failed to load: {message}");
            if wanted {
                let diagnostic = PluginDiagnostic {
                    reason: StopReason::Load,
                    message,
                };
                slot.stopped = Some((identity, diagnostic.clone()));
                slot.state = PluginState::Stopped(diagnostic);
            }
        }
    }
    if !listed.is_empty() {
        kiln_shell::command_palette::add_commands(cx, listed);
    }
    drive(id, cx);
}

/// Load `load` of plugin `id` was unloaded by a call into it, as
/// `diagnostic` says: it stops, its commands withdrawn.
fn stopped_running(id: &str, load: u64, diagnostic: Diagnostic, cx: &mut App) {
    let Some(host) = cx.try_global::<PluginHost>() else {
        return;
    };
    let current = host
        .slots
        .get(id)
        .and_then(|slot| slot.loaded.as_ref())
        .is_some_and(|loaded| loaded.load == load);
    if !current {
        return;
    }
    let host = cx.global_mut::<PluginHost>();
    let loaded = withdraw(id, host).expect("current");
    let slot = host.slots.get_mut(id).expect("current");
    let diagnostic = PluginDiagnostic::from(diagnostic);
    slot.stopped = Some((loaded.identity, diagnostic.clone()));
    slot.state = PluginState::Stopped(diagnostic);
    cx.refresh_windows();
}

/// The app's commands, which no plugin may register or share a namespace
/// with.
fn builtin_commands(cx: &App) -> Vec<CommandId> {
    let mut ids: Vec<CommandId> = kiln::platform::commands::app_command_list(cx)
        .into_iter()
        .map(|meta| meta.id)
        .chain(
            kiln::platform::command_actions::typed_commands(cx)
                .into_iter()
                .map(|typed| typed.id),
        )
        .chain(crate::command_list().into_iter().map(|meta| meta.id))
        .collect();
    ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    ids.dedup();
    ids
}

/// Answers each command a plugin invokes as not run: invoking the editor's
/// commands is a later card's.
struct NoInvoke;

impl Dispatcher for NoInvoke {
    fn dispatch(&self, request: Request) {
        log::debug!(
            "not running {} for a plugin: plugins can't invoke commands yet",
            request.invocation().command
        );
    }
}

/// What a load needs from the host.
struct LoadContext {
    cache: Option<Arc<ComponentCache>>,
    commands: CommandHost,
    secrets: Arc<dyn SecretStore>,
    secrets_index: Option<PathBuf>,
    /// Where a spawn grant whose program changed is suspended.
    store: Option<GrantStore>,
    workspace: Workspace,
    record: Record,
    /// Starts what enforces the plugin's call deadline.
    ticker: fn(&PluginEngine) -> Result<(), String>,
}

impl LoadContext {
    /// Compiles and starts `artifact`'s plugin with `grants`, unless
    /// `cancel` is set first: blocking, for the background executor.
    fn load(
        &self,
        artifact: &Artifact,
        grants: &fe_command::Grants,
        cancel: &Arc<AtomicBool>,
    ) -> Result<(Arc<Mutex<HostedPlugin>>, Vec<PluginCommand>), LoadFailure> {
        let cancelled = || cancel.load(Ordering::Acquire);
        let failed = |message: String| {
            if cancelled() {
                LoadFailure::Cancelled
            } else {
                LoadFailure::Failed(message)
            }
        };
        if cancelled() {
            return Err(LoadFailure::Cancelled);
        }
        let id = &artifact.manifest.id;
        let engine = PluginEngine::shared().map_err(|error| failed(error.to_string()))?;
        let limits = Limits::default();
        let mut kiln = bindings::HostState::new(guest_log(id, self.record.clone()));
        kiln.commands = kiln_plugin::commands::State::new(id, grants, &self.commands)
            .map_err(|error| failed(error.to_string()))?;
        if let Some(index) = &self.secrets_index {
            let service = fe_config::paths::app_id().reverse_dns;
            let secrets = PluginSecrets::new(self.secrets.clone(), service, id.clone(), index);
            kiln.secrets = kiln_plugin::secrets::State::new(secrets);
        }
        kiln.net = kiln_plugin::net::State::new(grants, &limits, Box::new(FeNet::new(grants)));
        // A program run has its own limit, which the call's deadline doesn't count.
        let process = kiln_plugin::process::State::new(artifact.record.clone(), &limits);
        kiln.process = match &self.store {
            Some(store) => process.recorded_in(store.clone(), id),
            None => process,
        };
        let host = fe_plugin::HostState {
            kiln,
            ..fe_plugin::HostState::default()
        };
        let plugin = HostedPlugin::load(
            engine,
            self.cache.as_deref(),
            &artifact.bytes,
            id,
            grants,
            limits,
            host,
            &self.workspace,
            self.record.clone(),
            self.ticker,
            cancel.clone(),
        )
        .map_err(|error| failed(error.to_string()))?;
        let plugin = Arc::new(Mutex::new(plugin));
        let handle: kiln_plugin::commands::PluginHandle = plugin.clone();
        let commands = self.commands.commands(id, &handle);
        Ok((plugin, commands))
    }
}

/// Where the plugins' lifecycle and logs go: the `log` crate, and under
/// test the host's record too.
#[derive(Clone)]
struct Record(#[cfg(test)] Arc<Mutex<Vec<String>>>);

impl Record {
    fn push(&self, line: String) {
        log::debug!(target: "kiln_plugin::guest", "{line}");
        #[cfg(test)]
        lock(&self.0).push(line);
    }
}

fn record(_host: &PluginHost) -> Record {
    Record(
        #[cfg(test)]
        _host.record.clone(),
    )
}

/// Plugin `id`'s logs, to the `log` crate under the `kiln_plugin::guest`
/// target, and to `record`, with the thread they came from.
fn guest_log(id: &str, record: Record) -> LogSink {
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
        let thread = thread::current().name().unwrap_or_default().to_owned();
        record.push(format!("log {id} on {thread}: {message}"));
    })
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A workbench's follow of the plugins: asks about one waiting for a grant
/// when its window is active, and keeps the trusted roots up to date.
pub(crate) struct PluginPrompt {
    _subscriptions: [Subscription; 4],
}

impl PluginPrompt {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        let activation = cx.observe_window_activation(window, |_, window, cx| {
            if window.is_window_active() {
                // Activated, it asks again about what it was dismissed for.
                let me = window.window_handle().window_id();
                let asking = cx.default_global::<Asking>();
                asking.dismissed.retain(|(_, window)| *window != me);
            }
            ask_in(window, cx);
        });
        let changed = cx.observe_global_in::<PluginHost>(window, |_, window, cx| {
            ask_in(window, cx);
            cx.notify();
        });
        let workbench = cx.entity();
        let trust = cx.subscribe(&workbench, |_, _, _: &TrustChanged, cx| {
            cx.defer(workspace_changed);
        });
        let root = cx.subscribe(&workbench, |_, _, _: &RootChanged, cx| {
            cx.defer(workspace_changed);
        });
        cx.on_release(|_, cx| cx.defer(workspace_changed)).detach();
        cx.defer_in(window, |_, window, cx| {
            ask_in(window, cx);
            workspace_changed(cx);
        });
        Self {
            _subscriptions: [activation, changed, trust, root],
        }
    }
}

/// A prompt a window shows.
struct Prompting {
    window: WindowId,
    shown: PendingPluginGrant,
    /// Dropped, it stops the prompt: its answer decides nothing, and a
    /// full-access prompt after it isn't shown.
    _cancel: oneshot::Sender<()>,
}

/// The plugins windows are asking about.
#[derive(Default)]
struct Asking {
    prompts: HashMap<String, Prompting>,
    /// The plugins each window's prompt was closed for without an answer:
    /// that window doesn't ask about them again until it's next activated.
    dismissed: HashSet<(String, WindowId)>,
}

impl Global for Asking {}

/// Asks in `window`, if it's active and asking about none, about the first
/// plugin waiting for a grant that no window is asking about and that it
/// wasn't dismissed for.
fn ask_in(window: &mut Window, cx: &mut App) {
    if !window.is_window_active() {
        return;
    }
    let open: Vec<WindowId> = cx
        .windows()
        .iter()
        .map(|handle| handle.window_id())
        .collect();
    let me = window.window_handle().window_id();
    let waiting = pending_plugin_grants(cx);
    let asking = cx.default_global::<Asking>();
    // A window that closed mid-prompt asks no more.
    asking
        .prompts
        .retain(|_, prompting| open.contains(&prompting.window));
    asking.dismissed.retain(|(_, window)| open.contains(window));
    if asking
        .prompts
        .values()
        .any(|prompting| prompting.window == me)
    {
        return;
    }
    let Some(pending) = waiting.into_iter().find(|pending| {
        !asking.prompts.contains_key(&pending.id)
            && !asking.dismissed.contains(&(pending.id.clone(), me))
    }) else {
        return;
    };
    let (cancel, cancelled) = oneshot::channel();
    asking.prompts.insert(
        pending.id.clone(),
        Prompting {
            window: me,
            shown: pending.clone(),
            _cancel: cancel,
        },
    );
    let answer = show_plugin_prompt(window, &pending, cx);
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let answer = match future::select(answer, cancelled).await {
            Either::Left((answer, _)) => answer,
            // Stopped: dropping the prompt's task stops it asking.
            Either::Right(_) => return,
        };
        cx.update(|cx| prompt_finished(handle, pending, answer, cx));
    })
    .detach();
}

/// `handle`'s prompt about `pending` was answered `answer`: recorded, then
/// the window asks about the next. Closed without an answer, it stays
/// pending, and that window doesn't ask about it again until it's next
/// activated.
fn prompt_finished(
    handle: AnyWindowHandle,
    pending: PendingPluginGrant,
    answer: PluginAnswer,
    cx: &mut App,
) {
    let me = handle.window_id();
    let asking = cx.default_global::<Asking>();
    let ours = asking
        .prompts
        .get(&pending.id)
        .is_some_and(|prompting| prompting.window == me && prompting.shown == pending);
    if !ours {
        return;
    }
    asking.prompts.remove(&pending.id);
    if answer == PluginAnswer::Pending {
        asking.dismissed.insert((pending.id.clone(), me));
        return;
    }
    answered(pending, answer, cx);
    handle.update(cx, |_, window, cx| ask_in(window, cx)).ok();
}

/// Stops the prompts showing a request that's no longer the plugin's
/// pending one: the preview went off, the plugin went, or it asks for
/// something else now.
fn invalidate_prompts(cx: &mut App) {
    let pending = pending_plugin_grants(cx);
    cx.default_global::<Asking>()
        .prompts
        .retain(|id, prompting| {
            pending
                .iter()
                .any(|current| current.id == *id && *current == prompting.shown)
        });
}

/// Records `answer` to `pending`, then decides the plugins again, if
/// `pending` is still the plugin's request with the preview on: an answer to
/// one that changed decides nothing.
fn answered(pending: PendingPluginGrant, answer: PluginAnswer, cx: &mut App) {
    if answer == PluginAnswer::Pending {
        return;
    }
    let Some(host) = cx.try_global::<PluginHost>() else {
        return;
    };
    let asks = |found: &Found| {
        found.id == pending.id
            && found.version == pending.version
            && (found.plugin.as_ref()).is_ok_and(|plugin| plugin.capabilities == pending.requested)
    };
    let current = host.preview
        && host.found.iter().any(asks)
        && host
            .slots
            .get(&pending.id)
            .is_some_and(|slot| slot.pending.as_ref() == Some(&pending));
    if !current {
        log::debug!(
            "not recording an answer for plugin {}: its request changed",
            pending.id
        );
        return;
    }
    let Some(store) = host
        .dirs
        .as_ref()
        .map(|dirs| GrantStore::new(dirs.state.join(GRANTS_FILE)))
    else {
        return;
    };
    // Not asked about again while it's recorded.
    let host = cx.global_mut::<PluginHost>();
    host.answering.insert(pending.id.clone());
    if let Some(slot) = host.slots.get_mut(&pending.id) {
        slot.pending = None;
    }
    let id = pending.id.clone();
    let recorded = cx.background_spawn(async move {
        match answer {
            PluginAnswer::Allowed { accept_wildcards } => {
                let programs = programs(&pending.requested)?;
                store
                    .grant(
                        &pending.id,
                        pending.version,
                        &pending.requested,
                        &programs,
                        accept_wildcards,
                    )
                    .map(drop)
                    .map_err(|error| error.to_string())
            }
            PluginAnswer::Declined => store
                .decline(&pending.id, &pending.requested)
                .map_err(|error| error.to_string()),
            PluginAnswer::Pending => Ok(()),
        }
    });
    cx.spawn(async move |cx| {
        let recorded = recorded.await;
        cx.update(|cx| {
            cx.global_mut::<PluginHost>().answering.remove(&id);
            if let Err(message) = recorded {
                log::error!("can't record the answer for plugin {id}: {message}");
                if let Some(slot) = cx.global_mut::<PluginHost>().slots.get_mut(&id) {
                    slot.set_want(Want::Unload(PluginState::Stopped(PluginDiagnostic {
                        reason: StopReason::Grants,
                        message,
                    })));
                }
                drive(&id, cx);
                return;
            }
            evaluate(cx, false);
        });
    })
    .detach();
}

/// The programs `requested`'s `process.spawn:<program>` name, as found on
/// `PATH` now: what a grant binds them to.
fn programs(
    requested: &fe_command::Grants,
) -> Result<Vec<kiln_plugin::grants::ProgramIdentity>, String> {
    use fe_command::capability::Target;
    let path_var = std::env::var_os("PATH").unwrap_or_else(|| OsString::from(SYSTEM_PATH));
    requested
        .iter()
        .filter_map(|capability| match capability {
            Capability::ProcessSpawn(Target::Named(program)) => Some(program),
            _ => None,
        })
        .map(|program| resolve_program(program, &path_var).map_err(|error| error.to_string()))
        .collect()
}

// The runner. kiln-plugin's runs its own world's components only; this is
// it for fe-plugin's world, whose editor imports a plugin may use.

/// A plugin loaded on this host: its own store on its own thread, every
/// call into it under its [`Limits`], as kiln-plugin's runner runs one. A
/// call that traps, overruns, runs out of memory or breaks protocol unloads
/// it with a [`Diagnostic`]; [`HostedPlugin::shutdown`] calls its
/// `shutdown` and stops its thread.
pub(crate) struct HostedPlugin {
    id: String,
    state: kiln_plugin::runner::PluginState,
    channel: Option<Channel>,
    thread: Option<JoinHandle<()>>,
}

/// The caller's ends of the plugin thread's channels.
struct Channel {
    calls: SyncSender<Call>,
    replies: Receiver<Result<Reply, Diagnostic>>,
}

impl HostedPlugin {
    /// Compiles `component` (through `cache`), links it against the editor
    /// world, and starts plugin `id` on its thread, calling its `init`. It
    /// refuses to when `ticker` can't enforce the call deadline, and stops
    /// before compiling, instantiating or `init` once `cancel` is set.
    #[allow(clippy::too_many_arguments)]
    fn load(
        engine: &PluginEngine,
        cache: Option<&ComponentCache>,
        component: &[u8],
        id: &str,
        grants: &fe_command::Grants,
        limits: Limits,
        host: fe_plugin::HostState,
        workspace: &Workspace,
        record: Record,
        ticker: fn(&PluginEngine) -> Result<(), String>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, LoadError> {
        ticker(engine).map_err(|message| LoadError::Thread {
            plugin: id.to_owned(),
            error: io::Error::other(message),
        })?;
        let cancelled = || LoadError::Unloaded(cancelled(id));
        if cancel.load(Ordering::Acquire) {
            return Err(cancelled());
        }
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
        let unlinked = |error: wasmtime::Error| LoadError::Invalid {
            plugin: id.to_owned(),
            message: format!("{error:#}; {REBUILD}"),
        };
        let component = match cache {
            Some(cache) => cache.load(engine.engine(), component),
            None => Component::new(engine.engine(), component),
        }
        .map_err(invalid)?;
        let pre = linker(engine)
            .map_err(invalid)?
            .instantiate_pre(&component)
            .map_err(unlinked)?;
        EditorExtensionPre::new(pre.clone()).map_err(unlinked)?;
        if cancel.load(Ordering::Acquire) {
            return Err(cancelled());
        }
        let data = Data {
            host,
            wasi: kiln_plugin::runner::wasi_context(grants, workspace),
            table: ResourceTable::new(),
            limiter: StoreLimiter::new(&limits),
            deadline: None,
            timed_out: false,
        };
        let guest = Guest {
            id: id.to_owned(),
            limits,
            store: Store::new(engine.engine(), data),
            record,
            cancel,
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
                message: "its thread stopped while loading it".into(),
            }))
        });
        if let Err(error) = started {
            drop(thread.join());
            return Err(error);
        }
        Ok(Self {
            id: id.to_owned(),
            state: kiln_plugin::runner::PluginState::Running,
            channel: Some(Channel { calls, replies }),
            thread: Some(thread),
        })
    }

    pub(crate) fn state(&self) -> &kiln_plugin::runner::PluginState {
        &self.state
    }

    /// Runs `call` on the plugin's thread. One that fails unloads it first.
    fn call(&mut self, call: Call) -> Result<Reply, CallError> {
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
                message: "its thread stopped".into(),
            })
        });
        replied.map_err(|diagnostic| {
            log::warn!("{diagnostic}");
            self.channel = None;
            if let Some(thread) = self.thread.take() {
                drop(thread.join());
            }
            self.state = kiln_plugin::runner::PluginState::Unloaded(diagnostic.clone());
            CallError::Failed(diagnostic)
        })
    }

    /// Calls the plugin's `shutdown` and stops its thread; later calls run
    /// nothing.
    fn shutdown(&mut self) {
        // Closing the channel tells the thread to shut the plugin down.
        self.channel = None;
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

impl CallPlugin for HostedPlugin {
    fn call(&mut self, call: Call) -> Result<Reply, CallError> {
        HostedPlugin::call(self, call)
    }
}

impl Drop for HostedPlugin {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The editor world's imports and WASI's, against [`Data`].
fn linker(engine: &PluginEngine) -> wasmtime::Result<Linker<Data>> {
    use fe_plugin::fastedit::editor::{buffer, decorations, status};
    let mut linker = Linker::<Data>::new(engine.engine());
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    bindings::add_to_linker(&mut linker, |data| &mut data.host.kiln)?;
    buffer::add_to_linker::<_, HasSelf<fe_plugin::HostState>>(&mut linker, |data| &mut data.host)?;
    decorations::add_to_linker::<_, HasSelf<fe_plugin::HostState>>(&mut linker, |data| {
        &mut data.host
    })?;
    status::add_to_linker::<_, HasSelf<fe_plugin::HostState>>(&mut linker, |data| &mut data.host)?;
    compat::link(&mut linker)?;
    Ok(linker)
}

/// A plugin's store data.
struct Data {
    host: fe_plugin::HostState,
    wasi: WasiCtx,
    table: ResourceTable,
    limiter: StoreLimiter,
    /// When the running call must end.
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

/// The world's exports, called async so a WASI import that waits is
/// cancelled at the deadline.
struct Exports {
    init: TypedFunc<(), (Result<(), bindings::Error>,)>,
    shutdown: TypedFunc<(), ()>,
    run_command: TypedFunc<(String, String), (Result<(), bindings::Error>,)>,
    handle_events: TypedFunc<(Vec<Event>,), ()>,
}

impl Exports {
    fn new(store: &mut Store<Data>, instance: &Instance) -> wasmtime::Result<Self> {
        let mut export = |interface: &str, name: &str| {
            instance
                .get_export_index(&mut *store, None, interface)
                .and_then(|interface| {
                    instance.get_export_index(&mut *store, Some(&interface), name)
                })
                .ok_or_else(|| wasmtime::format_err!("no export {interface}#{name}"))
        };
        let run_command = export("kiln:extension/command-handler@1.0.0", "run-command")?;
        let handle_events = export("kiln:extension/event-handler@1.0.0", "handle-events")?;
        Ok(Self {
            init: instance.get_typed_func(&mut *store, "init")?,
            shutdown: instance.get_typed_func(&mut *store, "shutdown")?,
            run_command: instance.get_typed_func(&mut *store, run_command)?,
            handle_events: instance.get_typed_func(&mut *store, handle_events)?,
        })
    }
}

/// The plugin thread's side.
struct Guest {
    id: String,
    limits: Limits,
    store: Store<Data>,
    record: Record,
    /// Set, the load stops before instantiating or `init`.
    cancel: Arc<AtomicBool>,
}

impl Guest {
    /// Instantiates the component and calls `init`, telling `started`, then
    /// runs each call until one fails or the channel closes, when it calls
    /// `shutdown`.
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
        self.record.push(format!("init {}", self.id));
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
        self.record.push(format!("shutdown {}", self.id));
    }

    fn start(&mut self, pre: &InstancePre<Data>) -> Result<Exports, LoadError> {
        self.store.limiter(|data| &mut data.limiter);
        self.store.epoch_deadline_callback(|mut store| {
            let data = store.data_mut();
            // The call's deadline doesn't count the time its program runs took.
            let deadline = data.host.kiln.program_runs.extend(data.deadline);
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                data.timed_out = true;
                return Ok(UpdateDeadline::Interrupt);
            }
            Ok(UpdateDeadline::Continue(1))
        });
        let cancelled = |guest: &Self| {
            let cancelled = guest.cancel.load(Ordering::Acquire);
            cancelled.then(|| LoadError::Unloaded(self::cancelled(&guest.id)))
        };
        if let Some(cancelled) = cancelled(self) {
            return Err(cancelled);
        }
        let instance = self
            .guarded(async |store| pre.instantiate_async(store).await)
            .map_err(LoadError::Unloaded)?;
        let exports = Exports::new(&mut self.store, &instance).map_err(|error| {
            LoadError::Unloaded(self.diagnosis(Reason::Protocol, format!("{error:?}")))
        })?;
        if let Some(cancelled) = cancelled(self) {
            return Err(cancelled);
        }
        self.store.data_mut().host.kiln.commands.begin_init();
        let (initialized,) = self
            .guarded(async |store| exports.init.call_async(store, ()).await)
            .map_err(LoadError::Unloaded)?;
        initialized.map_err(|error| LoadError::Init {
            plugin: self.id.clone(),
            error,
        })?;
        self.store.data_mut().host.kiln.commands.end_init();
        Ok(exports)
    }

    /// Runs `call` with the epoch advancing until it returns or its deadline
    /// passes.
    fn guarded<R>(
        &mut self,
        call: impl AsyncFnOnce(&mut Store<Data>) -> wasmtime::Result<R>,
    ) -> Result<R, Diagnostic> {
        let deadline = Instant::now().checked_add(self.limits.call_deadline);
        let program_runs = self.store.data().host.kiln.program_runs.clone();
        program_runs.arm(PROGRAM_DEADLINE);
        let passed = || {
            let deadline = program_runs.extend(deadline);
            deadline.is_some_and(|deadline| Instant::now() >= deadline)
        };
        let data = self.store.data_mut();
        data.deadline = deadline;
        data.host.kiln.deadline = deadline;
        data.timed_out = false;
        data.limiter.clear_refused();
        self.store.set_epoch_deadline(1);
        let ticking = ticking();
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
                wake_on_tick(cx.waker());
                Poll::Pending
            })
            .await
        });
        drop(ticking);
        let overran = passed();
        program_runs.disarm();
        let data = self.store.data_mut();
        data.deadline = None;
        data.host.kiln.deadline = None;
        let timed_out = data.timed_out || overran;
        match returned {
            Some(Ok(returned)) if !timed_out => Ok(returned),
            Some(Err(error)) if !timed_out => Err(self.diagnose(&error)),
            _ => {
                let deadline = self.limits.call_deadline;
                Err(self.diagnosis(
                    Reason::Timeout,
                    format!("a call ran past its {deadline:?} deadline"),
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

// The epoch ticker for this host's calls: kiln-plugin's advances the
// shared engine's epoch only for its own runner's calls.

/// How often the epoch advances while a call runs.
const EPOCH: Duration = Duration::from_millis(2);

/// How many calls are running, which the ticker waits for.
static RUNNING: Mutex<usize> = Mutex::new(0);
static STARTED: Condvar = Condvar::new();

/// The calls to wake on the next tick.
static WAITING: Mutex<Vec<Waker>> = Mutex::new(Vec::new());

/// A call running: the epoch advances until it drops.
struct Ticking;

impl Drop for Ticking {
    fn drop(&mut self) {
        *lock(&RUNNING) -= 1;
    }
}

/// Starts the ticker for `engine` on first use: an error, then and from then
/// on, if it couldn't be started, as nothing would enforce a call's
/// deadline.
fn ticker(engine: &PluginEngine) -> Result<(), String> {
    static TICKER: OnceLock<Result<(), String>> = OnceLock::new();
    TICKER
        .get_or_init(|| {
            start_ticker(engine.engine().clone(), |tick| {
                thread::Builder::new()
                    .name("plugin-host-epoch".into())
                    .spawn(tick)
                    .map(drop)
            })
        })
        .clone()
}

/// Starts, through `spawn`, the thread that advances `engine`'s epoch and
/// wakes the waiting calls every [`EPOCH`] while a call runs.
fn start_ticker(
    engine: Engine,
    spawn: impl FnOnce(Box<dyn FnOnce() + Send>) -> io::Result<()>,
) -> Result<(), String> {
    spawn(Box::new(move || {
        loop {
            drop(
                STARTED
                    .wait_while(lock(&RUNNING), |running| *running == 0)
                    .unwrap_or_else(PoisonError::into_inner),
            );
            thread::sleep(EPOCH);
            engine.increment_epoch();
            std::mem::take(&mut *lock(&WAITING))
                .into_iter()
                .for_each(Waker::wake);
        }
    }))
    .map_err(|error| format!("no thread to enforce plugins' deadlines: {error}"))
}

/// Keeps the epoch advancing until the guard drops: the ticker started
/// before the plugin loaded.
fn ticking() -> Ticking {
    *lock(&RUNNING) += 1;
    STARTED.notify_all();
    Ticking
}

/// What a cancelled load of plugin `id` says.
fn cancelled(id: &str) -> Diagnostic {
    Diagnostic {
        plugin: id.to_owned(),
        reason: Reason::Protocol,
        message: "its load was cancelled".into(),
    }
}

/// Wakes `waker` on the next tick.
fn wake_on_tick(waker: &Waker) {
    let mut waiting = lock(&WAITING);
    if !waiting.iter().any(|waiting| waiting.will_wake(waker)) {
        waiting.push(waker.clone());
    }
}

#[cfg(test)]
#[path = "plugin_host_tests.rs"]
mod plugin_host_tests;
