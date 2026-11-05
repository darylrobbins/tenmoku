//! Diagnostics plugins report on buffers, kept by the host.
//!
//! A plugin with the `decorations` capability publishes named sets of diagnostics on a buffer,
//! against a version of it. The [`DiagnosticStore`] keeps them, keyed by plugin, buffer and set,
//! anchored so they follow later edits; a set made against an older version is first mapped
//! through the edits since. The app reads the store and is told of each change through
//! [`DiagnosticStore::subscribe`]. Unloading a plugin or closing a buffer drops its sets.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use fe_buffer::{Anchor, Bias, BufferId, BufferSnapshot, Text, Version};
use kiln_command::capability::{Capability, Grants};
use kiln_plugin::bindings::kiln::extension::types::Diagnostic;
use kiln_plugin::bindings::{Error, unimplemented};

use crate::HostState;
use crate::decorations::{BufferVersions, MAX_ENTRIES_PER_BUFFER};
use crate::fastedit::editor::diagnostics::Host;

/// Where a range's ends go when text is inserted right at them: inside neither.
const START: Bias = Bias::Right;
const END: Bias = Bias::Left;

/// A plugin's diagnostic as it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginDiagnostic {
    pub plugin: String,
    pub set: String,
    /// Bytes of the snapshot it was read in.
    pub range: Range<usize>,
    pub diagnostic: Diagnostic,
}

type Listener = Arc<dyn Fn(u64) + Send + Sync>;

/// Every plugin's diagnostics, which the app shares with every plugin's diagnostics slice.
/// Clones share it.
#[derive(Clone, Default)]
pub struct DiagnosticStore {
    shared: Arc<Mutex<Shared>>,
}

#[derive(Default)]
struct Shared {
    /// Each loaded plugin's attachment: the number of its load.
    attached: BTreeMap<String, u64>,
    loads: u64,
    /// By buffer, then plugin, then set.
    buffers: BTreeMap<u64, BTreeMap<String, BTreeMap<String, Vec<Stored>>>>,
    listeners: Vec<Listener>,
}

struct Stored {
    /// The buffer and version the anchors were made in.
    buffer: BufferId,
    version: Version,
    range: Range<Anchor>,
    diagnostic: Diagnostic,
}

impl DiagnosticStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Calls `listener` with the id of each buffer whose diagnostics change.
    pub fn subscribe(&self, listener: impl Fn(u64) + Send + Sync + 'static) {
        self.lock().listeners.push(Arc::new(listener));
    }

    /// Every plugin's diagnostics on `buffer`, in `snapshot`, the buffer's, by plugin and set.
    pub fn diagnostics(&self, buffer: u64, snapshot: &BufferSnapshot) -> Vec<PluginDiagnostic> {
        let mut shared = self.lock();
        shared.refresh(buffer, snapshot);
        let mut found = Vec::new();
        for (plugin, sets) in shared.buffers.get(&buffer).into_iter().flatten() {
            for (set, stored) in sets {
                for stored in stored {
                    // Anchors newer than an older snapshot don't resolve in it.
                    if stored.version > snapshot.version() {
                        continue;
                    }
                    found.push(PluginDiagnostic {
                        plugin: plugin.clone(),
                        set: set.clone(),
                        range: snapshot.resolve_range(&stored.range),
                        diagnostic: stored.diagnostic.clone(),
                    });
                }
            }
        }
        found
    }

    /// Forgets plugin `plugin`: its sets go, and its slice publishes nothing more.
    pub fn unload(&self, plugin: &str) {
        let mut shared = self.lock();
        shared.attached.remove(plugin);
        let changed = shared.forget(plugin);
        notify(shared, &changed);
    }

    /// Drops every plugin's sets on `buffer`, which was closed.
    pub fn close(&self, buffer: u64) {
        let mut shared = self.lock();
        let had = shared.buffers.remove(&buffer).is_some();
        notify(
            shared,
            &[buffer].into_iter().filter(|_| had).collect::<Vec<_>>(),
        );
    }

    /// Attaches a new load of `plugin`, which starts with no sets.
    fn attach(&self, plugin: &str) -> u64 {
        let mut shared = self.lock();
        shared.loads += 1;
        let load = shared.loads;
        shared.attached.insert(plugin.to_owned(), load);
        let changed = shared.forget(plugin);
        notify(shared, &changed);
        load
    }

    /// Replaces `plugin`'s set `set` on `buffer`, if `load` is still the plugin's and its sets
    /// stay within [`MAX_ENTRIES_PER_BUFFER`].
    fn replace(
        &self,
        plugin: &str,
        load: u64,
        buffer: u64,
        set: String,
        stored: Vec<Stored>,
    ) -> Result<(), Error> {
        let mut shared = self.lock();
        shared.current(plugin, load)?;
        let sets = shared.buffers.entry(buffer).or_default();
        let others: usize = (sets.get(plugin).into_iter().flatten())
            .filter(|(name, _)| **name != set)
            .map(|(_, stored)| stored.len())
            .sum();
        let after = others + stored.len();
        if after > MAX_ENTRIES_PER_BUFFER {
            shared.prune(buffer, plugin);
            return Err(Error::InvalidArgument(format!(
                "plugin {plugin} would keep {after} diagnostics on buffer {buffer}, over the \
                 limit of {MAX_ENTRIES_PER_BUFFER}"
            )));
        }
        let sets = shared
            .buffers
            .entry(buffer)
            .or_default()
            .entry(plugin.to_owned());
        let sets = sets.or_default();
        if stored.is_empty() {
            sets.remove(&set);
        } else {
            sets.insert(set, stored);
        }
        shared.prune(buffer, plugin);
        notify(shared, &[buffer]);
        Ok(())
    }

    fn clear(&self, plugin: &str, load: u64, buffer: u64, set: &str) -> Result<(), Error> {
        let mut shared = self.lock();
        shared.current(plugin, load)?;
        let removed = (shared.buffers.get_mut(&buffer))
            .and_then(|plugins| plugins.get_mut(plugin))
            .is_some_and(|sets| sets.remove(set).is_some());
        shared.prune(buffer, plugin);
        notify(
            shared,
            &[buffer].into_iter().filter(|_| removed).collect::<Vec<_>>(),
        );
        Ok(())
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Tells the listeners that `changed` buffers changed, after releasing the store.
fn notify(shared: MutexGuard<'_, Shared>, changed: &[u64]) {
    if changed.is_empty() {
        return;
    }
    let listeners = shared.listeners.clone();
    drop(shared);
    for buffer in changed {
        for listener in &listeners {
            listener(*buffer);
        }
    }
}

impl Shared {
    /// Rebases `buffer`'s diagnostics onto `snapshot`, its newer snapshot: each anchors there,
    /// so a later read resolves them even once the edit log is compacted. Drops those of another
    /// buffer, or whose version the log can no longer map from. Leaves all alone for an older
    /// snapshot.
    fn refresh(&mut self, buffer: u64, snapshot: &BufferSnapshot) {
        let Some(plugins) = self.buffers.get_mut(&buffer) else {
            return;
        };
        for sets in plugins.values_mut() {
            for stored in sets.values_mut() {
                stored.retain_mut(|stored| {
                    if stored.buffer != snapshot.id() {
                        return false;
                    }
                    if stored.version >= snapshot.version() {
                        return true;
                    }
                    if stored.version < snapshot.oldest_version() {
                        return false;
                    }
                    let range = snapshot.resolve_range(&stored.range);
                    stored.range =
                        snapshot.anchor_at(range.start, START)..snapshot.anchor_at(range.end, END);
                    stored.version = snapshot.version();
                    true
                });
            }
            sets.retain(|_, stored| !stored.is_empty());
        }
        plugins.retain(|_, sets| !sets.is_empty());
        if plugins.is_empty() {
            self.buffers.remove(&buffer);
        }
    }

    /// Drops every set of `plugin`'s; the buffers that had any.
    fn forget(&mut self, plugin: &str) -> Vec<u64> {
        let mut changed = Vec::new();
        self.buffers.retain(|buffer, plugins| {
            if plugins.remove(plugin).is_some() {
                changed.push(*buffer);
            }
            !plugins.is_empty()
        });
        changed
    }

    /// Drops `plugin`'s entry for `buffer` if it has no sets left, and the buffer's if no plugin
    /// has.
    fn prune(&mut self, buffer: u64, plugin: &str) {
        let Some(plugins) = self.buffers.get_mut(&buffer) else {
            return;
        };
        if plugins.get(plugin).is_some_and(BTreeMap::is_empty) {
            plugins.remove(plugin);
        }
        if plugins.is_empty() {
            self.buffers.remove(&buffer);
        }
    }

    /// Refuses `plugin` unless `load` is its current one.
    fn current(&self, plugin: &str, load: u64) -> Result<(), Error> {
        if self.attached.get(plugin) == Some(&load) {
            Ok(())
        } else {
            Err(Error::Denied(format!("plugin {plugin} was unloaded")))
        }
    }
}

/// The diagnostics slice of [`HostState`]. The default has no [`DiagnosticStore`] and answers
/// every import with [`unimplemented`].
#[derive(Default)]
pub struct State {
    attached: Option<Attached>,
}

struct Attached {
    plugin: String,
    grants: Grants,
    store: DiagnosticStore,
    load: u64,
    buffers: Box<dyn BufferVersions>,
}

impl State {
    /// Plugin `plugin`'s slice on `store`, over `buffers`, as far as `grants` allow. It is a new
    /// load of the plugin: an earlier load's sets are dropped, and that load's slice publishes
    /// nothing more.
    pub fn new(
        plugin: &str,
        grants: &Grants,
        store: &DiagnosticStore,
        buffers: Box<dyn BufferVersions>,
    ) -> Self {
        Self {
            attached: Some(Attached {
                plugin: plugin.to_owned(),
                grants: grants.clone(),
                load: store.attach(plugin),
                store: store.clone(),
                buffers,
            }),
        }
    }

    /// The attachment, if its grants include `decorations`, which `import` needs.
    fn granted(&mut self, import: &str) -> Result<&mut Attached, Error> {
        let Some(attached) = self.attached.as_mut() else {
            return unimplemented(import);
        };
        if !attached.grants.allows(&Capability::Decorations) {
            return Err(Error::Denied(format!(
                "{import} needs the {} capability",
                Capability::Decorations
            )));
        }
        Ok(attached)
    }
}

impl Attached {
    /// `items` as stored on `buffer` now, their ranges in the text at `version` mapped through
    /// the edits since.
    fn stored(
        &mut self,
        buffer: u64,
        version: u64,
        items: Vec<Diagnostic>,
    ) -> Result<Vec<Stored>, Error> {
        if items.len() > MAX_ENTRIES_PER_BUFFER {
            return Err(Error::InvalidArgument(format!(
                "{} diagnostics are over the limit of {MAX_ENTRIES_PER_BUFFER} on a buffer",
                items.len()
            )));
        }
        let now = (self.buffers.now(buffer))
            .ok_or_else(|| Error::NotFound(format!("no open buffer has id {buffer}")))?;
        let then = self.buffers.at(buffer, version).filter(|then| {
            then.id() == now.id()
                && then.version() <= now.version()
                && then.version() >= now.oldest_version()
        });
        let then = then.ok_or_else(|| {
            Error::Conflict(format!(
                "buffer {buffer}'s version {version} can't be mapped to the buffer now"
            ))
        })?;
        let from = then.version();
        items
            .into_iter()
            .enumerate()
            .map(|(index, diagnostic)| {
                let range = range(then.text(), &diagnostic)
                    .map_err(|why| Error::InvalidArgument(format!("diagnostic {index}'s {why}")))?;
                let start = now.map_offset(range.start, START, from);
                let end = now.map_offset(range.end, END, from).max(start);
                Ok(Stored {
                    buffer: now.id(),
                    version: now.version(),
                    range: now.anchor_at(start, START)..now.anchor_at(end, END),
                    diagnostic,
                })
            })
            .collect()
    }
}

/// `diagnostic`'s range in `text`, or why it isn't one.
fn range(text: &Text, diagnostic: &Diagnostic) -> Result<Range<usize>, String> {
    let (start, end) = (diagnostic.range.start, diagnostic.range.end);
    let len = text.len();
    let offset = |offset: u64| usize::try_from(offset).ok().filter(|&offset| offset <= len);
    let (Some(start), Some(end)) = (offset(start), offset(end)) else {
        return Err(format!(
            "range {start}..{end} is past the end of the text, at {len}"
        ));
    };
    if start > end {
        return Err(format!("range {start}..{end} ends before it starts"));
    }
    if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return Err(format!("range {start}..{end} splits a character"));
    }
    Ok(start..end)
}

impl Host for HostState {
    fn publish(
        &mut self,
        buffer: u64,
        version: u64,
        set: String,
        items: Vec<Diagnostic>,
    ) -> Result<(), Error> {
        let attached = self
            .diagnostics
            .granted("fastedit:editor/diagnostics.publish")?;
        let stored = attached.stored(buffer, version, items)?;
        (attached.store).replace(&attached.plugin, attached.load, buffer, set, stored)
    }

    fn clear(&mut self, buffer: u64, set: String) -> Result<(), Error> {
        let attached = self
            .diagnostics
            .granted("fastedit:editor/diagnostics.clear")?;
        (attached.store).clear(&attached.plugin, attached.load, buffer, &set)
    }
}
