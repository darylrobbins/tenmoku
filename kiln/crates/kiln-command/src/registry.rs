// SPDX-License-Identifier: Apache-2.0

use std::{borrow::Cow, cell::RefCell, collections::HashMap, sync::Arc};

use kiln_l10n::SharedString;
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    Actor, CommandId, ContextPredicate, InvalidCommandId, Invocation, PluginAccess, PluginDenied,
    PluginPolicy, authorize::authorize, command_detail, command_label,
};

/// A kind of thing commands run against. Implemented by a marker type, so
/// the target handlers receive can borrow: the editor's is a model plus the
/// host it runs in, borrowed for one dispatch.
pub trait CommandTarget: 'static {
    /// What a command reports it produced, such as a transaction ID.
    type Tx;
    /// What handlers receive.
    type Target<'a>;
}

/// What a dispatched command did, so the UI knows what to refresh.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Effect {
    Nothing,
    /// Something outside the text and selections changed, such as the
    /// dirty flag: repaint.
    Notify,
    /// The text or the selections moved: repaint and scroll to the cursor.
    Changed,
}

/// A handler's result: its effect and the transactions it produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dispatched<Tx> {
    pub effect: Effect,
    pub transactions: Vec<Tx>,
}

impl<Tx> Dispatched<Tx> {
    pub fn new(effect: Effect) -> Self {
        Self {
            effect,
            transactions: Vec::new(),
        }
    }

    pub fn with_transactions(mut self, transactions: Vec<Tx>) -> Self {
        self.transactions = transactions;
        self
    }
}

/// A handler's failure, such as a save that couldn't write.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct CommandError(pub String);

impl CommandError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// What the registry knows about a command, for keymaps, the palette and
/// agents' tool lists.
///
/// `title` and `description` are the English text it was registered with;
/// show [`CommandMeta::label`] and [`CommandMeta::detail`], which look them
/// up in the active locale first.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CommandMeta {
    pub id: CommandId,
    pub title: String,
    pub description: String,
    /// The JSON schema of its args, or `None` if it takes none.
    pub args_schema: Option<Value>,
    /// Where it applies; `None` is everywhere.
    pub context: Option<ContextPredicate>,
    /// Whether it can change the buffer's text, so it can't run read-only
    /// and is what a dry run would propose.
    pub mutates_buffer: bool,
}

impl CommandMeta {
    /// Its title in the active locale: `command-<id>`, else `title`
    /// ([`crate::command_label`]).
    pub fn label(&self) -> SharedString {
        command_label(self.id.as_str(), &self.title)
    }

    /// Its description in the active locale: `command-<id>`'s
    /// `.description`, else `description` ([`crate::command_detail`]).
    pub fn detail(&self) -> SharedString {
        command_detail(self.id.as_str(), &self.description)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RegisterError {
    #[error(transparent)]
    InvalidId(#[from] InvalidCommandId),
    #[error("command {0} is already registered")]
    Duplicate(CommandId),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DispatchError {
    #[error("unknown command {0}")]
    UnknownCommand(CommandId),
    #[error("bad args for {command}: {message}")]
    BadArgs { command: CommandId, message: String },
    #[error("{command} doesn't apply here: needs {predicate}")]
    NotInContext {
        command: CommandId,
        predicate: ContextPredicate,
    },
    #[error("{command} failed: {error}")]
    Failed {
        command: CommandId,
        error: CommandError,
    },
    /// A plugin's invocation its grants don't cover.
    #[error(transparent)]
    PluginDenied(#[from] PluginDenied),
}

/// [`Registry::allow_plugins`] named a command that isn't registered.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("unknown command {0}")]
pub struct UnknownCommand(pub String);

enum Failure {
    BadArgs(String),
    Command(CommandError),
}

type Handler<T> = Box<
    dyn for<'a, 'b> Fn(
        &'b mut <T as CommandTarget>::Target<'a>,
        &Actor,
        &Value,
    ) -> Result<Dispatched<<T as CommandTarget>::Tx>, Failure>,
>;

type Observer = Box<dyn Fn(&Invocation)>;

struct Entry<T: CommandTarget> {
    meta: CommandMeta,
    check_args: fn(&Value) -> Result<(), String>,
    handler: Handler<T>,
    /// What a plugin needs to run it; `None` refuses every plugin.
    plugin_access: Option<PluginAccess>,
}

/// The commands for one kind of target, by ID.
pub struct Registry<T: CommandTarget> {
    entries: Vec<Entry<T>>,
    index: HashMap<CommandId, usize>,
    observers: Vec<Observer>,
    plugin_policy: Option<Arc<dyn PluginPolicy>>,
    /// The plugin whose dispatch is running, so a dispatch nested in it
    /// runs as that plugin, within its grants, whatever actor it names.
    acting_plugin: RefCell<Option<String>>,
}

impl<T: CommandTarget> Default for Registry<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
            observers: Vec::new(),
            plugin_policy: None,
            acting_plugin: RefCell::new(None),
        }
    }
}

impl<T: CommandTarget> Registry<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a command without args. Its handler receives the target
    /// and the invoking actor.
    pub fn register<F>(
        &mut self,
        id: &str,
        title: &str,
        handler: F,
    ) -> Result<CommandBuilder<'_>, RegisterError>
    where
        F: for<'a, 'b> Fn(&'b mut T::Target<'a>, &Actor) -> Result<Dispatched<T::Tx>, CommandError>
            + 'static,
    {
        self.insert(
            id,
            title,
            None,
            no_args,
            Box::new(move |target, actor, args| {
                no_args(args).map_err(Failure::BadArgs)?;
                handler(target, actor).map_err(Failure::Command)
            }),
        )
    }

    /// Registers a command whose args deserialize into `A`. Args that don't,
    /// or that aren't a table or hold a null, give a
    /// [`DispatchError::BadArgs`] and run nothing; `Null` args are read as an
    /// empty table, so all-default args can be left out.
    pub fn register_with_args<A, F>(
        &mut self,
        id: &str,
        title: &str,
        handler: F,
    ) -> Result<CommandBuilder<'_>, RegisterError>
    where
        A: DeserializeOwned + JsonSchema + 'static,
        F: for<'a, 'b> Fn(
                &'b mut T::Target<'a>,
                &Actor,
                A,
            ) -> Result<Dispatched<T::Tx>, CommandError>
            + 'static,
    {
        let schema = schemars::schema_for!(A).to_value();
        self.insert(
            id,
            title,
            Some(schema),
            |args| typed_args::<A>(args).map(drop),
            Box::new(move |target, actor, args| {
                let args = typed_args::<A>(args).map_err(Failure::BadArgs)?;
                handler(target, actor, args).map_err(Failure::Command)
            }),
        )
    }

    fn insert(
        &mut self,
        id: &str,
        title: &str,
        args_schema: Option<Value>,
        check_args: fn(&Value) -> Result<(), String>,
        handler: Handler<T>,
    ) -> Result<CommandBuilder<'_>, RegisterError> {
        let id = CommandId::parse(id)?;
        if self.index.contains_key(&id) {
            return Err(RegisterError::Duplicate(id));
        }
        self.index.insert(id.clone(), self.entries.len());
        self.entries.push(Entry {
            meta: CommandMeta {
                id,
                title: title.to_string(),
                description: String::new(),
                args_schema,
                context: None,
                mutates_buffer: false,
            },
            check_args,
            handler,
            plugin_access: None,
        });
        let meta = &mut self.entries.last_mut().expect("just pushed").meta;
        Ok(CommandBuilder { meta })
    }

    /// Opens the registered command `id` to plugins that hold `commands`
    /// and every capability in `access`. Commands not opened refuse every
    /// plugin.
    pub fn allow_plugins(&mut self, id: &str, access: PluginAccess) -> Result<(), UnknownCommand> {
        let ix = CommandId::parse(id)
            .ok()
            .and_then(|id| self.index.get(&id).copied())
            .ok_or_else(|| UnknownCommand(id.to_string()))?;
        self.entries[ix].plugin_access = Some(access);
        Ok(())
    }

    /// What a plugin needs to run `id`, if it's open to plugins.
    pub fn plugin_access(&self, id: &CommandId) -> Option<&PluginAccess> {
        self.entry(id)
            .and_then(|entry| entry.plugin_access.as_ref())
    }

    /// Sets what plugin invocations are checked against. Without one, every
    /// plugin is refused.
    pub fn set_plugin_policy(&mut self, policy: Arc<dyn PluginPolicy>) {
        self.plugin_policy = Some(policy);
    }

    /// Every command's metadata, in registration order.
    pub fn commands(&self) -> impl ExactSizeIterator<Item = &CommandMeta> {
        self.entries.iter().map(|entry| &entry.meta)
    }

    pub fn get(&self, id: &CommandId) -> Option<&CommandMeta> {
        self.entry(id).map(|entry| &entry.meta)
    }

    fn entry(&self, id: &CommandId) -> Option<&Entry<T>> {
        self.index.get(id).map(|&ix| &self.entries[ix])
    }

    /// Checks that the command exists, that a plugin invoking it may run
    /// it, that it applies in the invocation's context and that it accepts
    /// its args, without running anything.
    pub fn validate(&self, invocation: &Invocation) -> Result<&CommandMeta, DispatchError> {
        let invocation = self.as_acting_plugin(invocation);
        let entry = self.applicable(&invocation)?;
        (entry.check_args)(&invocation.args).map_err(|message| DispatchError::BadArgs {
            command: invocation.command.clone(),
            message,
        })?;
        Ok(&entry.meta)
    }

    /// Checks that `command` exists and accepts `args`, whatever context it
    /// applies in: for checking a keymap binding before it's ever pressed.
    pub fn validate_args(
        &self,
        command: &CommandId,
        args: &Value,
    ) -> Result<&CommandMeta, DispatchError> {
        let entry = self
            .entry(command)
            .ok_or_else(|| DispatchError::UnknownCommand(command.clone()))?;
        (entry.check_args)(args).map_err(|message| DispatchError::BadArgs {
            command: command.clone(),
            message,
        })?;
        Ok(&entry.meta)
    }

    /// The invocation as it runs: inside a plugin's dispatch, as that
    /// plugin.
    fn as_acting_plugin<'i>(&self, invocation: &'i Invocation) -> Cow<'i, Invocation> {
        match &*self.acting_plugin.borrow() {
            Some(plugin) if !matches!(&invocation.actor, Actor::Host(name) if name == plugin) => {
                Cow::Owned(invocation.clone().actor(Actor::Host(plugin.clone())))
            }
            _ => Cow::Borrowed(invocation),
        }
    }

    /// The command's entry, if it exists, a plugin invoking it may run it,
    /// and it applies in the invocation's context.
    fn applicable(&self, invocation: &Invocation) -> Result<&Entry<T>, DispatchError> {
        let command = &invocation.command;
        let entry = self
            .entry(command)
            .ok_or_else(|| DispatchError::UnknownCommand(command.clone()))?;
        if let Actor::Host(plugin) = &invocation.actor {
            authorize(
                self.plugin_policy.as_deref(),
                plugin,
                command,
                entry.plugin_access.as_ref(),
                &invocation.args,
            )?;
        }
        if let Some(predicate) = &entry.meta.context
            && !predicate.eval(&invocation.context)
        {
            return Err(DispatchError::NotInContext {
                command: command.clone(),
                predicate: predicate.clone(),
            });
        }
        Ok(entry)
    }

    /// Runs the invocation's command on `target`, if it applies in the
    /// invocation's context and its args are good, then tells the
    /// observers.
    ///
    /// An invocation by a plugin (`Actor::Host`) runs only if the plugin's
    /// grants cover the command's [`PluginAccess`]; see
    /// [`Registry::allow_plugins`]. A dispatch nested in a plugin's, such as
    /// a command running another through this registry, runs as that
    /// plugin and is checked the same way.
    pub fn dispatch(
        &self,
        target: &mut T::Target<'_>,
        invocation: &Invocation,
    ) -> Result<Dispatched<T::Tx>, DispatchError> {
        let invocation = &*self.as_acting_plugin(invocation);
        let entry = self.applicable(invocation)?;
        let _acting = match &invocation.actor {
            Actor::Host(plugin) => Some(Acting::new(&self.acting_plugin, plugin)),
            _ => None,
        };
        let command = || invocation.command.clone();
        let dispatched =
            (entry.handler)(target, &invocation.actor, &invocation.args).map_err(|failure| {
                match failure {
                    Failure::BadArgs(message) => DispatchError::BadArgs {
                        command: command(),
                        message,
                    },
                    Failure::Command(error) => DispatchError::Failed {
                        command: command(),
                        error,
                    },
                }
            })?;
        for observer in &self.observers {
            observer(invocation);
        }
        Ok(dispatched)
    }

    /// Calls `observer` with every invocation that dispatches successfully:
    /// the seam for recording, logging and replication.
    pub fn observe(&mut self, observer: impl Fn(&Invocation) + 'static) {
        self.observers.push(Box::new(observer));
    }
}

/// Marks a plugin's dispatch as running until dropped, even by a panic,
/// then puts back what was running before.
struct Acting<'r> {
    slot: &'r RefCell<Option<String>>,
    outer: Option<String>,
}

impl<'r> Acting<'r> {
    fn new(slot: &'r RefCell<Option<String>>, plugin: &str) -> Self {
        let outer = slot.replace(Some(plugin.to_string()));
        Self { slot, outer }
    }
}

impl Drop for Acting<'_> {
    fn drop(&mut self) {
        *self.slot.borrow_mut() = self.outer.take();
    }
}

/// Sets a newly registered command's optional metadata.
pub struct CommandBuilder<'r> {
    meta: &'r mut CommandMeta,
}

impl CommandBuilder<'_> {
    pub fn description(self, description: &str) -> Self {
        self.meta.description = description.to_string();
        self
    }

    pub fn context(self, predicate: ContextPredicate) -> Self {
        self.meta.context = Some(predicate);
        self
    }

    pub fn mutates_buffer(self, mutates_buffer: bool) -> Self {
        self.meta.mutates_buffer = mutates_buffer;
        self
    }
}

fn no_args(args: &Value) -> Result<(), String> {
    match args {
        Value::Null => Ok(()),
        Value::Object(table) if table.is_empty() => Ok(()),
        _ => Err("takes no args".to_string()),
    }
}

fn typed_args<A: DeserializeOwned>(args: &Value) -> Result<A, String> {
    representable(args)?;
    let result = if args.is_null() {
        A::deserialize(&Value::Object(Default::default()))
    } else {
        A::deserialize(args)
    };
    result.map_err(|error| error.to_string())
}

/// Checks that args are a table (or nothing) holding no nulls, so they're
/// TOML as well as JSON. Serde alone would read a positional array into a
/// struct, and pass over nulls in fields it ignores or reads as `None`.
fn representable(args: &Value) -> Result<(), String> {
    fn has_null(value: &Value) -> bool {
        match value {
            Value::Null => true,
            Value::Array(items) => items.iter().any(has_null),
            Value::Object(table) => table.values().any(has_null),
            _ => false,
        }
    }
    match args {
        Value::Null => Ok(()),
        Value::Object(table) if table.values().any(has_null) => {
            Err("args can't hold a null".to_string())
        }
        Value::Object(_) => Ok(()),
        _ => Err("args must be a table".to_string()),
    }
}
