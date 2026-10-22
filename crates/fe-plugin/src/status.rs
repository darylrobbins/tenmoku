//! Status-bar items plugins contribute, kept by the host.
//!
//! A plugin with the `status` capability sets items, each its text, a tooltip and a command, and
//! the [`StatusItems`] keep them for the status bar to read; nothing calls the plugin to draw
//! one. Clicking an item dispatches its command as the plugin, `Actor::Host(<extension id>)`, so
//! the app's registry authorizes it against the plugin's grants as it does any plugin's
//! invocation: an item can't run what its plugin couldn't. Unloading a plugin clears its items.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use kiln_command::capability::{Capability, Grants};
use kiln_command::{
    Actor, CommandId, CommandTarget, DispatchError, Dispatched, Invocation, Registry, Source,
};
use kiln_plugin::bindings::{Error, unimplemented};

use crate::HostState;
pub use crate::fastedit::editor::status::Alignment;
use crate::fastedit::editor::status::{Host, Item};

/// How many items one plugin may show at once.
pub const MAX_ITEMS_PER_PLUGIN: usize = 64;

/// A plugin's status-bar item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusItem {
    pub plugin: String,
    /// Unique among the plugin's items.
    pub id: String,
    pub text: String,
    pub tooltip: Option<String>,
    /// Run when the item is clicked.
    pub command: Option<CommandId>,
    pub alignment: Alignment,
}

impl StatusItem {
    /// What clicking it dispatches: its command, by its plugin, from the mouse. None without a
    /// command.
    pub fn invocation(&self) -> Option<Invocation> {
        let command = self.command.clone()?;
        Some(
            Invocation::new(command)
                .actor(Actor::Host(self.plugin.clone()))
                .source(Source::Mouse),
        )
    }
}

/// Every plugin's status-bar items, which the app shares with every plugin's status slice and
/// with its status bar. Clones share it.
#[derive(Clone, Default)]
pub struct StatusItems {
    shared: Arc<Mutex<Shared>>,
}

#[derive(Default)]
struct Shared {
    /// Each loaded plugin's attachment: the number of its load.
    attached: BTreeMap<String, u64>,
    loads: u64,
    /// By plugin, each plugin's in the order it first set them.
    items: BTreeMap<String, Vec<StatusItem>>,
}

impl StatusItems {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every plugin's items, by plugin.
    pub fn items(&self) -> Vec<StatusItem> {
        self.lock().items.values().flatten().cloned().collect()
    }

    /// Plugin `plugin`'s item `id`.
    pub fn item(&self, plugin: &str, id: &str) -> Option<StatusItem> {
        let shared = self.lock();
        let items = shared.items.get(plugin)?;
        items.iter().find(|item| item.id == id).cloned()
    }

    /// Clicks plugin `plugin`'s item `id`: dispatches its [`StatusItem::invocation`] on
    /// `registry`, which authorizes it against the plugin's grants through its
    /// [`PluginPolicy`](kiln_command::PluginPolicy), and returns what that gave. None if there
    /// is no such item or it has no command.
    pub fn activate<T: CommandTarget>(
        &self,
        plugin: &str,
        id: &str,
        registry: &Registry<T>,
        target: &mut T::Target<'_>,
    ) -> Option<Result<Dispatched<T::Tx>, DispatchError>> {
        // Looked up before dispatching, so a command that changes the items doesn't deadlock.
        let invocation = self.item(plugin, id)?.invocation()?;
        Some(registry.dispatch(target, &invocation))
    }

    /// Forgets plugin `plugin`: its items go, and its slice sets nothing more.
    pub fn unload(&self, plugin: &str) {
        let mut shared = self.lock();
        shared.attached.remove(plugin);
        shared.items.remove(plugin);
    }

    /// Attaches a new load of `plugin`, which starts with no items.
    fn attach(&self, plugin: &str) -> u64 {
        let mut shared = self.lock();
        shared.loads += 1;
        let load = shared.loads;
        shared.attached.insert(plugin.to_owned(), load);
        shared.items.remove(plugin);
        load
    }

    /// Sets `item` of `plugin`'s, replacing the one with its id, if `load` is still the
    /// plugin's.
    fn set(&self, plugin: &str, load: u64, item: StatusItem) -> Result<(), Error> {
        let mut shared = self.lock();
        current(&shared, plugin, load)?;
        let items = shared.items.entry(plugin.to_owned()).or_default();
        if let Some(old) = items.iter_mut().find(|old| old.id == item.id) {
            *old = item;
        } else if items.len() >= MAX_ITEMS_PER_PLUGIN {
            return Err(Error::Failed(format!(
                "plugin {plugin} already shows {MAX_ITEMS_PER_PLUGIN} status items, the most it may"
            )));
        } else {
            items.push(item);
        }
        Ok(())
    }

    /// Removes `plugin`'s item `id`, if `load` is still the plugin's.
    fn remove(&self, plugin: &str, load: u64, id: &str) -> Result<(), Error> {
        let mut shared = self.lock();
        current(&shared, plugin, load)?;
        let items = shared.items.get_mut(plugin);
        let index = items
            .as_ref()
            .and_then(|items| items.iter().position(|item| item.id == id));
        let (Some(items), Some(index)) = (items, index) else {
            return Err(Error::NotFound(format!(
                "plugin {plugin} has no status item {id:?}"
            )));
        };
        items.remove(index);
        if items.is_empty() {
            shared.items.remove(plugin);
        }
        Ok(())
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Refuses `plugin` unless `load` is its current one.
fn current(shared: &Shared, plugin: &str, load: u64) -> Result<(), Error> {
    if shared.attached.get(plugin) == Some(&load) {
        Ok(())
    } else {
        Err(Error::Denied(format!("plugin {plugin} was unloaded")))
    }
}

/// The status slice of [`HostState`]. The default has no [`StatusItems`] and answers every
/// import with [`unimplemented`].
#[derive(Default)]
pub struct State {
    attached: Option<Attached>,
}

struct Attached {
    plugin: String,
    grants: Grants,
    items: StatusItems,
    /// The load of the plugin this slice is.
    load: u64,
}

impl State {
    /// Plugin `plugin`'s slice on `items`, as far as `grants` allow. It is a new load of the
    /// plugin: whatever items an earlier load left are dropped, and that load's slice sets
    /// nothing more.
    pub fn new(plugin: &str, grants: &Grants, items: &StatusItems) -> Self {
        Self {
            attached: Some(Attached {
                plugin: plugin.to_owned(),
                grants: grants.clone(),
                load: items.attach(plugin),
                items: items.clone(),
            }),
        }
    }

    /// The attachment, if its grants include `status`, which `import` needs.
    fn granted(&self, import: &str) -> Result<&Attached, Error> {
        let Some(attached) = &self.attached else {
            return unimplemented(import);
        };
        if !attached.grants.allows(&Capability::Status) {
            return Err(Error::Denied(format!(
                "{import} needs the {} capability",
                Capability::Status
            )));
        }
        Ok(attached)
    }
}

impl Host for HostState {
    fn set_item(&mut self, item: Item) -> Result<(), Error> {
        let attached = self.status.granted("fastedit:editor/status.set-item")?;
        let command = item
            .command
            .map(|command| CommandId::parse(&command))
            .transpose()
            .map_err(|error| Error::InvalidArgument(format!("item {:?}: {error}", item.id)))?;
        let plugin = &attached.plugin;
        let item = StatusItem {
            plugin: plugin.clone(),
            id: item.id,
            text: item.text,
            tooltip: item.tooltip,
            command,
            alignment: item.alignment,
        };
        attached.items.set(plugin, attached.load, item)
    }

    fn remove_item(&mut self, id: String) -> Result<(), Error> {
        let attached = self.status.granted("fastedit:editor/status.remove-item")?;
        attached.items.remove(&attached.plugin, attached.load, &id)
    }
}
