// SPDX-License-Identifier: Apache-2.0

//! Plugin-contributed commands on the kiln command registry, and the commands plugins invoke.
//!
//! A plugin with the `commands` capability registers commands while its `init` runs, each
//! `<extension id>.<name>`: never a built-in's id, another plugin's or one in a reserved
//! namespace. The host collects them as [`PluginCommand`]s for the app to register on its
//! [`Registry`](kiln_command::Registry); running one calls the plugin's `run-command` on the
//! plugin's own thread, under its call deadline. Unloading a plugin from the [`CommandHost`]
//! returns the ids to take off the registry again.
//!
//! A plugin invokes a command as `Invocation { actor: Host(<extension id>), source: Script }`,
//! so what it does is recorded, observed and replayed like what a user does. The invocation
//! goes to the app's [`Dispatcher`] as a [`Request`], which the app runs on its registry; the
//! registry authorizes it against the plugin's grants, with the [`CommandHost`] as its
//! [`PluginPolicy`], and a refusal reaches the plugin as `denied`, naming what it lacks. The
//! plugin waits for the answer only until its call's deadline.
//!
//! Each load of a plugin is one attachment to the host. Unloading revokes it: whatever the
//! plugin's store still holds can then register, invoke and run nothing, even once the same id
//! is loaded again, and the same id loads again only once no request of the old one is left.
//!
//! A store with no [`CommandHost`] answers both imports with [`unimplemented`].

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use kiln_command::{
    Actor, Capability, CommandError, CommandId, CommandMeta, ContextPredicate, DispatchError,
    Effect, Grants, Invocation, PluginPolicy, Source,
};
use serde_json::Value;

use crate::bindings::kiln::extension::commands::{Command, Host};
use crate::bindings::{Error, HostState, unimplemented};
use crate::events::Recipient;
use crate::fs::Workspace;
use crate::runner::{Call, CallError, Plugin, Reply};

/// Takes the invocations plugins make to the app, which runs each on its registry and
/// answers it. It is called on the invoking plugin's thread and must not wait on the app: the
/// plugin waits for the answer itself, up to its call's deadline.
pub trait Dispatcher: Send + Sync {
    fn dispatch(&self, request: Request);
}

/// A plugin's invocation, waiting for the app to run it. Dropped unanswered, it answers the
/// plugin with a failure.
pub struct Request {
    invocation: Invocation,
    waiting: Arc<Waiting>,
    attachment: Arc<Attachment>,
}

/// Where a [`Request`]'s answer goes.
#[derive(Default)]
struct Waiting {
    answer: Mutex<Option<Result<(), Error>>>,
    answered: Condvar,
    /// Whether the plugin stopped waiting.
    abandoned: AtomicBool,
}

impl Request {
    pub fn invocation(&self) -> &Invocation {
        &self.invocation
    }

    /// Whether nobody waits for it any more, so the app should not run it: the plugin's call
    /// ran past its deadline, or the plugin was unloaded.
    pub fn is_cancelled(&self) -> bool {
        self.waiting.abandoned.load(Ordering::Acquire)
            || self.attachment.revoked.load(Ordering::Acquire)
    }

    /// Answers the plugin with what dispatching it returned. A refusal reaches the plugin as
    /// `denied`, naming what it lacks.
    pub fn answer(self, result: Result<(), DispatchError>) {
        self.reply(result.map_err(|error| match error {
            DispatchError::UnknownCommand(_) => Error::NotFound(error.to_string()),
            DispatchError::BadArgs { .. } => Error::InvalidArgument(error.to_string()),
            DispatchError::PluginDenied(_) => Error::Denied(error.to_string()),
            DispatchError::NotInContext { .. } | DispatchError::Failed { .. } => {
                Error::Failed(error.to_string())
            }
        }));
    }

    /// Runs it with `dispatch` and answers with what that returned, unless it is cancelled.
    pub fn run(self, dispatch: impl FnOnce(&Invocation) -> Result<(), DispatchError>) {
        if self.is_cancelled() {
            return;
        }
        let result = dispatch(&self.invocation);
        self.answer(result);
    }

    fn reply(&self, result: Result<(), Error>) {
        let mut answer = lock(&self.waiting.answer);
        if answer.is_none() {
            *answer = Some(result);
            self.waiting.answered.notify_all();
        }
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        self.reply(Err(Error::Failed(format!(
            "{} was not run",
            self.invocation.command
        ))));
        self.attachment.requests.fetch_sub(1, Ordering::AcqRel);
    }
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("invocation", &self.invocation)
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// Runs calls into one plugin: its [`Plugin`], or the [`Recipient`] that owns it.
pub trait CallPlugin: Send {
    fn call(&mut self, call: Call) -> Result<Reply, CallError>;
}

impl CallPlugin for Plugin {
    fn call(&mut self, call: Call) -> Result<Reply, CallError> {
        Plugin::call(self, call)
    }
}

impl CallPlugin for Recipient {
    fn call(&mut self, call: Call) -> Result<Reply, CallError> {
        Recipient::call(self, call)
    }
}

/// A loaded plugin, shared by its [`PluginCommand`]s.
pub type PluginHandle = Arc<Mutex<dyn CallPlugin>>;

/// What the app shares with every plugin's commands slice: the ids taken, the namespaces
/// reserved, the workspace, each loaded plugin's grants and commands, and the [`Dispatcher`].
/// Clones share it. It is the [`PluginPolicy`] for the app's registry.
#[derive(Clone)]
pub struct CommandHost {
    shared: Arc<Mutex<Shared>>,
    dispatcher: Arc<dyn Dispatcher>,
}

struct Shared {
    builtin: HashSet<CommandId>,
    reserved: HashSet<String>,
    workspace: Workspace,
    plugins: HashMap<String, Loaded>,
    /// Unloaded plugins' attachments that still have requests out.
    retired: Vec<(String, Arc<Attachment>)>,
}

/// A plugin attached to the host.
struct Loaded {
    grants: Grants,
    commands: Vec<CommandMeta>,
    attachment: Arc<Attachment>,
}

/// One load of a plugin, which its store, its [`PluginCommand`]s and its [`Request`]s hold.
#[derive(Default)]
struct Attachment {
    /// Whether the plugin was unloaded.
    revoked: AtomicBool,
    /// How many of its requests are out.
    requests: AtomicUsize,
    /// Whether its thread waits in an `invoke`, so it can take no call.
    invoking: AtomicBool,
}

/// Why [`State::new`] could not attach a plugin to the host.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AttachError {
    #[error("plugin {0} is already loaded")]
    AlreadyLoaded(String),
    /// An unloaded load of it has a request the app has not let go of yet.
    #[error("plugin {0} was unloaded with an invocation still out")]
    RequestsOut(String),
}

impl CommandHost {
    /// A host whose app has the `builtin` commands, no namespace reserved, and dispatches
    /// plugins' invocations through `dispatcher`.
    pub fn new(
        builtin: impl IntoIterator<Item = CommandId>,
        dispatcher: Arc<dyn Dispatcher>,
    ) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                builtin: builtin.into_iter().collect(),
                reserved: HashSet::new(),
                workspace: Workspace::default(),
                plugins: HashMap::new(),
                retired: Vec::new(),
            })),
            dispatcher,
        }
    }

    /// Keeps plugins out of `namespace`, a command id's first segment, such as the app's own:
    /// no plugin registers a command in it.
    pub fn reserve(&self, namespace: &str) {
        self.lock().reserved.insert(namespace.to_owned());
    }

    /// Sets the folders open in the app, which a plugin's path args must be inside.
    pub fn set_workspace(&self, workspace: Workspace) {
        self.lock().workspace = workspace;
    }

    /// The ids of plugin `plugin`'s commands, in the order registered.
    pub fn registered(&self, plugin: &str) -> Vec<CommandId> {
        self.lock()
            .plugins
            .get(plugin)
            .map(|loaded| loaded.commands.iter().map(|meta| meta.id.clone()).collect())
            .unwrap_or_default()
    }

    /// Plugin `plugin`'s registered commands, each run through `handle`, the loaded plugin.
    pub fn commands(&self, plugin: &str, handle: &PluginHandle) -> Vec<PluginCommand> {
        let shared = self.lock();
        let Some(loaded) = shared.plugins.get(plugin) else {
            return Vec::new();
        };
        loaded
            .commands
            .iter()
            .map(|meta| PluginCommand {
                plugin: plugin.to_owned(),
                meta: meta.clone(),
                handle: Arc::clone(handle),
                attachment: Arc::clone(&loaded.attachment),
            })
            .collect()
    }

    /// Forgets plugin `plugin`: its grants go, so the registry refuses it, its commands' ids
    /// are free again, and what its store, its commands and its requests still hold is
    /// revoked. Returns those ids, in the order registered, for the app to remove from its
    /// registry.
    pub fn unload(&self, plugin: &str) -> Vec<CommandId> {
        self.lock().unload(plugin)
    }

    fn attach(&self, plugin: &str, grants: &Grants) -> Result<Arc<Attachment>, AttachError> {
        let mut shared = self.lock();
        if shared.plugins.contains_key(plugin) {
            return Err(AttachError::AlreadyLoaded(plugin.to_owned()));
        }
        shared
            .retired
            .retain(|(_, attachment)| attachment.requests.load(Ordering::Acquire) > 0);
        if shared.retired.iter().any(|(retired, _)| retired == plugin) {
            return Err(AttachError::RequestsOut(plugin.to_owned()));
        }
        let attachment = Arc::new(Attachment::default());
        shared.plugins.insert(
            plugin.to_owned(),
            Loaded {
                grants: grants.clone(),
                commands: Vec::new(),
                attachment: Arc::clone(&attachment),
            },
        );
        Ok(attachment)
    }

    /// Unloads `plugin` if `attachment` is still its load, checked under the same lock, so a
    /// load that replaced it stays.
    fn detach(&self, plugin: &str, attachment: &Arc<Attachment>) {
        let mut shared = self.lock();
        if current(&shared, plugin, attachment).is_ok() {
            shared.unload(plugin);
        }
    }

    /// Refuses `plugin` unless `attachment` is still its load.
    fn current(&self, plugin: &str, attachment: &Arc<Attachment>) -> Result<(), Error> {
        let shared = self.lock();
        current(&shared, plugin, attachment).map(drop)
    }

    /// Adds `command` to `plugin`'s, if `attachment` is still its load and the id is free.
    fn register(
        &self,
        plugin: &str,
        attachment: &Arc<Attachment>,
        command: CommandMeta,
    ) -> Result<(), Error> {
        let mut shared = self.lock();
        current(&shared, plugin, attachment)?;
        let id = &command.id;
        if shared.builtin.contains(id) {
            return Err(Error::Denied(format!("{id} is a built-in command")));
        }
        if let Some((owner, _)) = shared.plugins.iter().find(|(_, loaded)| {
            loaded
                .commands
                .iter()
                .any(|registered| registered.id == *id)
        }) {
            return Err(Error::Denied(format!(
                "{id} is already registered by plugin {owner}"
            )));
        }
        let namespace = id.namespace();
        if shared.reserved.contains(namespace) {
            return Err(Error::Denied(format!(
                "the {namespace} namespace is reserved"
            )));
        }
        if let Some(loaded) = shared.plugins.get_mut(plugin) {
            loaded.commands.push(command);
        }
        Ok(())
    }

    /// `invocation` as a request of `plugin`'s, if `attachment` is still its load.
    fn request(
        &self,
        plugin: &str,
        attachment: &Arc<Attachment>,
        invocation: Invocation,
    ) -> Result<(Request, Arc<Waiting>), Error> {
        let shared = self.lock();
        current(&shared, plugin, attachment)?;
        // Counted while the host is locked, so an unload that follows sees it.
        attachment.requests.fetch_add(1, Ordering::AcqRel);
        let waiting = Arc::new(Waiting::default());
        let request = Request {
            invocation,
            waiting: Arc::clone(&waiting),
            attachment: Arc::clone(attachment),
        };
        Ok((request, waiting))
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }
}

impl Shared {
    /// [`CommandHost::unload`], under its lock.
    fn unload(&mut self, plugin: &str) -> Vec<CommandId> {
        let Some(loaded) = self.plugins.remove(plugin) else {
            return Vec::new();
        };
        loaded.attachment.revoked.store(true, Ordering::Release);
        self.retired
            .push((plugin.to_owned(), Arc::clone(&loaded.attachment)));
        loaded.commands.into_iter().map(|meta| meta.id).collect()
    }
}

/// Refuses `plugin` unless `attachment` is its current load.
fn current(shared: &Shared, plugin: &str, attachment: &Arc<Attachment>) -> Result<(), Error> {
    match shared.plugins.get(plugin) {
        Some(loaded) if Arc::ptr_eq(&loaded.attachment, attachment) => Ok(()),
        _ => Err(Error::Denied(format!("plugin {plugin} was unloaded"))),
    }
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl PluginPolicy for CommandHost {
    fn grants(&self, plugin: &str) -> Option<Grants> {
        self.lock()
            .plugins
            .get(plugin)
            .map(|loaded| loaded.grants.clone())
    }

    fn workspace_roots(&self) -> Vec<PathBuf> {
        self.lock().workspace.roots.clone()
    }
}

/// A command a plugin registered, for the app's registry.
#[derive(Clone)]
pub struct PluginCommand {
    plugin: String,
    meta: CommandMeta,
    handle: PluginHandle,
    attachment: Arc<Attachment>,
}

impl PluginCommand {
    /// The plugin's id.
    pub fn plugin(&self) -> &str {
        &self.plugin
    }

    pub fn id(&self) -> &CommandId {
        &self.meta.id
    }

    /// Its id, title, args schema, context and whether it mutates the buffer.
    pub fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    /// Calls the plugin's `run-command` with `args` as JSON (`{}` for `Null`), on the plugin's
    /// thread under its call deadline. A command that mutates the buffer reports
    /// [`Effect::Changed`], any other [`Effect::Notify`]. It fails without calling the plugin
    /// once the plugin is unloaded, or while the plugin waits in an `invoke` and so can take no
    /// call.
    pub fn run(&self, args: &Value) -> Result<Effect, CommandError> {
        let id = &self.meta.id;
        let plugin = &self.plugin;
        if self.attachment.revoked.load(Ordering::Acquire) {
            return Err(CommandError::new(format!(
                "{id}: plugin {plugin} was unloaded"
            )));
        }
        if self.attachment.invoking.load(Ordering::Acquire) {
            return Err(CommandError::new(format!(
                "{id}: plugin {plugin} is waiting on a command it invoked"
            )));
        }
        let args = if args.is_null() {
            "{}".to_owned()
        } else {
            args.to_string()
        };
        let call = Call::RunCommand {
            id: id.to_string(),
            args,
        };
        let replied = lock(&self.handle).call(call);
        match replied {
            Ok(Reply::RunCommand(Ok(()))) if self.meta.mutates_buffer => Ok(Effect::Changed),
            Ok(Reply::RunCommand(Ok(()))) => Ok(Effect::Notify),
            Ok(Reply::RunCommand(Err(error))) => Err(CommandError::new(format!(
                "{id}: plugin {plugin} returned {error:?}"
            ))),
            Ok(reply) => Err(CommandError::new(format!(
                "{id}: plugin {plugin} answered {reply:?}"
            ))),
            Err(error) => Err(CommandError::new(format!("{id}: {error}"))),
        }
    }
}

impl std::fmt::Debug for PluginCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginCommand")
            .field("plugin", &self.plugin)
            .field("meta", &self.meta)
            .finish_non_exhaustive()
    }
}

/// The commands slice of [`HostState`]. The default has no [`CommandHost`].
#[derive(Default)]
pub struct State {
    attached: Option<Attached>,
}

/// A plugin's slice on a [`CommandHost`].
struct Attached {
    plugin: String,
    grants: Grants,
    host: CommandHost,
    attachment: Arc<Attachment>,
    phase: Phase,
}

/// Where a plugin's load has got to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Being instantiated: it registers nothing yet.
    Instantiating,
    /// In its `init`, so it may register commands.
    Initializing,
    /// Loaded.
    Running,
}

impl State {
    /// Plugin `plugin`'s slice on `host`, holding `grants`. It registers commands only between
    /// [`begin_init`](Self::begin_init) and [`end_init`](Self::end_init), which the runner calls
    /// around the plugin's `init`; dropped before `end_init`, as when instantiating it or its
    /// `init` fails, it takes the plugin off `host` again.
    pub fn new(plugin: &str, grants: &Grants, host: &CommandHost) -> Result<Self, AttachError> {
        let attachment = host.attach(plugin, grants)?;
        Ok(Self {
            attached: Some(Attached {
                plugin: plugin.to_owned(),
                grants: grants.clone(),
                host: host.clone(),
                attachment,
                phase: Phase::Instantiating,
            }),
        })
    }

    /// The plugin's `init` is about to run: it may register commands.
    pub fn begin_init(&mut self) {
        if let Some(attached) = &mut self.attached {
            attached.phase = Phase::Initializing;
        }
    }

    /// The plugin's `init` has returned: it registers no more commands.
    pub fn end_init(&mut self) {
        if let Some(attached) = &mut self.attached {
            attached.phase = Phase::Running;
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(attached) = &self.attached
            && attached.phase != Phase::Running
        {
            attached.host.detach(&attached.plugin, &attached.attachment);
        }
    }
}

impl Attached {
    /// Refuses `what` unless the plugin holds `commands` and is still loaded.
    fn granted(&self, what: &str) -> Result<(), Error> {
        if !self.grants.allows(&Capability::Commands) {
            return Err(Error::Denied(format!(
                "plugin {} needs the {} capability to {what}",
                self.plugin,
                Capability::Commands
            )));
        }
        self.host.current(&self.plugin, &self.attachment)
    }

    fn register(&self, command: Command) -> Result<(), Error> {
        self.granted("register commands")?;
        let plugin = &self.plugin;
        if self.phase != Phase::Initializing {
            return Err(Error::Denied(format!(
                "plugin {plugin} registers commands only during init"
            )));
        }
        let id = CommandId::parse(&command.id)
            .map_err(|error| Error::InvalidArgument(error.to_string()))?;
        let name = id
            .as_str()
            .strip_prefix(plugin.as_str())
            .and_then(|rest| rest.strip_prefix('.'));
        if name.is_none() {
            return Err(Error::Denied(format!(
                "{id} is outside plugin {plugin}'s namespace: its commands are {plugin}.<name>"
            )));
        }
        let args_schema = command
            .args_schema
            .map(|schema| serde_json::from_str::<Value>(&schema))
            .transpose()
            .map_err(|error| Error::InvalidArgument(format!("{id}: args-schema: {error}")))?;
        let context = command
            .when
            .map(|when| ContextPredicate::parse(&when))
            .transpose()
            .map_err(|error| Error::InvalidArgument(format!("{id}: when: {error}")))?;
        self.host.register(
            plugin,
            &self.attachment,
            CommandMeta {
                id,
                title: command.title,
                description: String::new(),
                args_schema,
                context,
                mutates_buffer: command.mutates_buffer,
            },
        )
    }

    /// Sends the invocation to the app and waits for its answer, until `deadline` if any.
    fn invoke(&self, id: &str, args: &str, deadline: Option<Instant>) -> Result<(), Error> {
        self.granted("invoke commands")?;
        let id = CommandId::parse(id).map_err(|error| Error::InvalidArgument(error.to_string()))?;
        let args = if args.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(args)
                .map_err(|error| Error::InvalidArgument(format!("{id}: args: {error}")))?
        };
        let invocation = Invocation::new(id.clone())
            .args(args)
            .actor(Actor::Host(self.plugin.clone()))
            .source(Source::Script);
        let (request, waiting) = self
            .host
            .request(&self.plugin, &self.attachment, invocation)?;
        let _invoking = Invoking::new(&self.attachment.invoking);
        self.host.dispatcher.dispatch(request);
        let mut answer = lock(&waiting.answer);
        loop {
            if let Some(answer) = answer.take() {
                return answer;
            }
            answer = match deadline {
                None => waiting
                    .answered
                    .wait(answer)
                    .unwrap_or_else(PoisonError::into_inner),
                Some(deadline) => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        waiting.abandoned.store(true, Ordering::Release);
                        return Err(Error::Failed(format!(
                            "{id} was not answered before the call's deadline"
                        )));
                    }
                    waiting
                        .answered
                        .wait_timeout(answer, left)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
            };
        }
    }
}

/// Marks a plugin as waiting in an `invoke` until dropped.
struct Invoking<'a>(&'a AtomicBool);

impl<'a> Invoking<'a> {
    fn new(flag: &'a AtomicBool) -> Self {
        flag.store(true, Ordering::Release);
        Self(flag)
    }
}

impl Drop for Invoking<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Host for HostState {
    fn register(&mut self, command: Command) -> Result<(), Error> {
        match &self.commands.attached {
            Some(attached) => attached.register(command),
            None => unimplemented("kiln:extension/commands.register"),
        }
    }

    fn invoke(&mut self, id: String, args: String) -> Result<(), Error> {
        match &self.commands.attached {
            Some(attached) => attached.invoke(&id, &args, self.deadline),
            None => unimplemented("kiln:extension/commands.invoke"),
        }
    }
}
