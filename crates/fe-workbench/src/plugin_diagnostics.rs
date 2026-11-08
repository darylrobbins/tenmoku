//! The diagnostics plugins publish (`fastedit:editor/diagnostics`), shown as
//! a language server's are: squiggles, inline diagnostics, next and previous
//! diagnostic, and Problems panel rows, beside the servers' own.
//!
//! - **Store.** The app has one fe-plugin [`DiagnosticStore`] ([`Diagnostics`]),
//!   which each plugin's slice publishes to from the plugin's thread, mapping
//!   the version it names as its decorations do
//!   ([`crate::plugin_paint::Paint::diagnostics`]). Each change to a buffer's
//!   sets wakes the main thread, which shows them.
//! - **Shown.** Each editor of the buffer gets each plugin's sets on it,
//!   merged, as that plugin's ([`DiagnosticOwner::Plugin`]), from its load
//!   ([`DiagnosticIdentity::Plugin`]); the servers' and the other plugins'
//!   stay as they are. A diagnostic without a source shows the plugin's name.
//!   An editor opened on the buffer later is made to hold them as the store
//!   has them at the next sync ([`sync`]), whatever it was made with: one made
//!   from a copy of another's model (a split, a diff's retained source) is
//!   rid of what was cleared or ended since. So are editors whose diagnostics
//!   were all reset for a new language server manager ([`reshow`]). The
//!   Problems panel lists an open buffer's
//!   diagnostics from its editors, so they're listed there too.
//! - **Gone.** Unloading a plugin, reloading it or its load stopping
//!   ([`unload`]) ends its load: its sets leave the store, and the editors'
//!   diagnostics from that load are cleared. A buffer no editor shows any more
//!   is closed in the store ([`sync`]): plugin diagnostics are for open
//!   buffers only.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ops::Range,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use fe_editor::diagnostics::{Diagnostic, Severity};
use fe_gpui::EditorView;
use fe_plugin::{
    decorations::BufferVersions,
    diagnostics::{self, DiagnosticStore, PluginDiagnostic},
};
use gpui::{App, Entity, EntityId, Global, Task};
use kiln_plugin::bindings::kiln::extension::types::Severity as PluginSeverity;

use crate::{
    diagnostic_owner::{DiagnosticIdentity, DiagnosticOwner},
    plugin_bridge, plugin_host,
};

/// The app's plugin diagnostics, and the way to wake the main thread to show
/// them; what each plugin's diagnostics slice is made on. Clones share them.
#[derive(Clone)]
pub(crate) struct Diagnostics {
    store: DiagnosticStore,
}

impl Diagnostics {
    /// Plugin `plugin`'s `diagnostics` slice, as far as `grants` allow,
    /// mapping the versions it names through `buffers`. It's a new load: the
    /// last one's sets are dropped.
    pub(crate) fn state(
        &self,
        plugin: &str,
        grants: &fe_command::Grants,
        buffers: Box<dyn BufferVersions>,
    ) -> diagnostics::State {
        diagnostics::State::new(plugin, grants, &self.store, buffers)
    }

    /// Drops plugin `plugin`'s sets, and its slice publishes nothing more,
    /// from any thread: a load that failed after its slice was made.
    pub(crate) fn forget(&self, plugin: &str) {
        self.store.unload(plugin);
    }

    /// Every plugin's diagnostics the store keeps on `buffer`.
    #[cfg(test)]
    pub(crate) fn kept(
        &self,
        buffer: u64,
        snapshot: &fe_buffer::BufferSnapshot,
    ) -> Vec<PluginDiagnostic> {
        self.store.diagnostics(buffer, snapshot)
    }
}

/// The main thread's side.
struct PluginDiagnostics {
    diagnostics: Diagnostics,
    /// The buffers whose sets changed since they were last shown, by the
    /// bridge's ids; the store's listener adds to them from any thread.
    changed: Arc<Mutex<BTreeSet<u64>>>,
    /// Each plugin's owner key, the same over its loads.
    keys: BTreeMap<String, u64>,
    /// Each plugin's load as its diagnostics are shown, while it's loaded;
    /// the next one's is another.
    loads: HashMap<String, u64>,
    /// The last key or load given.
    last: u64,
    /// The buffers that had plugin diagnostics when last shown.
    known: HashSet<u64>,
    /// Each open editor shown its buffer's, and the buffer.
    seen: HashMap<EntityId, u64>,
    _wake: Task<()>,
}

impl Global for PluginDiagnostics {}

/// The app's [`Diagnostics`], made on first use.
pub(crate) fn diagnostics(cx: &mut App) -> Diagnostics {
    if let Some(state) = cx.try_global::<PluginDiagnostics>() {
        return state.diagnostics.clone();
    }
    let store = DiagnosticStore::new();
    let changed: Arc<Mutex<BTreeSet<u64>>> = Arc::default();
    let (wake, woken) = async_channel::bounded(1);
    let listening = changed.clone();
    store.subscribe(move |buffer| {
        lock(&listening).insert(buffer);
        wake.try_send(()).ok();
    });
    let task = cx.spawn(async move |cx| {
        while woken.recv().await.is_ok() {
            cx.update(show_changed);
        }
    });
    let diagnostics = Diagnostics { store };
    cx.set_global(PluginDiagnostics {
        diagnostics: diagnostics.clone(),
        changed,
        keys: BTreeMap::new(),
        loads: HashMap::new(),
        last: 0,
        known: HashSet::new(),
        seen: HashMap::new(),
        _wake: task,
    });
    diagnostics
}

/// Plugin `plugin`'s load ended (unloaded, reloading or stopped): its sets
/// leave the store, and its diagnostics the editors, at the next effect.
pub(crate) fn unload(plugin: &str, cx: &mut App) {
    if !cx.has_global::<PluginDiagnostics>() {
        return;
    }
    let state = cx.global_mut::<PluginDiagnostics>();
    state.diagnostics.store.unload(plugin);
    state.loads.remove(plugin);
    lock(&state.changed).extend(state.known.iter().copied());
    cx.defer(show_changed);
}

/// The open editors may have changed: a buffer no editor shows any more is
/// closed in the store, and an editor not seen yet is shown its buffer's
/// diagnostics, whatever it was made with: one made from a copy of another's
/// model (a split, a diff's retained source) holds what that one held then.
pub(crate) fn sync(cx: &mut App) {
    if !cx.has_global::<PluginDiagnostics>() {
        return;
    }
    let open = open_editors(cx);
    let state = cx.global_mut::<PluginDiagnostics>();
    let editors: HashMap<EntityId, u64> = (open.iter())
        .flat_map(|(buffer, editors)| editors.iter().map(|editor| (editor.entity_id(), *buffer)))
        .collect();
    state.seen.retain(|editor, _| editors.contains_key(editor));
    let gone: Vec<u64> = (state.known.iter())
        .filter(|buffer| !open.contains_key(buffer))
        .copied()
        .collect();
    for buffer in gone {
        state.known.remove(&buffer);
        state.diagnostics.store.close(buffer);
    }
    let unseen: Vec<u64> = (editors.iter())
        .filter(|(editor, buffer)| state.seen.get(editor) != Some(buffer))
        .map(|(_, buffer)| *buffer)
        .collect();
    lock(&state.changed).extend(unseen);
    show_changed(cx);
}

/// The editors' diagnostics were all reset: each is shown the plugins' again,
/// after the update under way.
pub(crate) fn reshow(cx: &mut App) {
    if !cx.has_global::<PluginDiagnostics>() {
        return;
    }
    cx.global_mut::<PluginDiagnostics>().seen.clear();
    cx.defer(sync);
}

/// The open editors, by their buffer's bridge id.
fn open_editors(cx: &mut App) -> HashMap<u64, Vec<Entity<EditorView>>> {
    let mut open: HashMap<u64, Vec<Entity<EditorView>>> = HashMap::new();
    for (buffer, _, editor) in plugin_bridge::open_editors(cx) {
        open.entry(buffer).or_default().push(editor);
    }
    open
}

/// Shows each changed buffer's plugin diagnostics in its editors.
fn show_changed(cx: &mut App) {
    let Some(state) = cx.try_global::<PluginDiagnostics>() else {
        return;
    };
    let changed = std::mem::take(&mut *lock(&state.changed));
    if changed.is_empty() {
        return;
    }
    let mut open = open_editors(cx);
    for buffer in changed {
        let editors = open.remove(&buffer).unwrap_or_default();
        show(buffer, editors, cx);
    }
}

/// What an editor is to hold of one plugin's diagnostics.
struct Wanted {
    owner: DiagnosticOwner,
    /// The plugin's load, while it's loaded.
    load: Option<DiagnosticIdentity>,
    /// The load's diagnostics on the buffer, if it has any.
    items: Option<Vec<(Range<usize>, Diagnostic)>>,
}

/// Makes each of `editors`, those of buffer `buffer`, hold every plugin's
/// diagnostics on it as the store has them now: those of a load that ended
/// are cleared and those of a set since cleared emptied, whatever the editor
/// held before. A buffer with none open is closed in the store.
fn show(buffer: u64, editors: Vec<Entity<EditorView>>, cx: &mut App) {
    let Some(first) = editors.first() else {
        let state = cx.global_mut::<PluginDiagnostics>();
        state.known.remove(&buffer);
        state.diagnostics.store.close(buffer);
        return;
    };
    let snapshot = first.read(cx).model().buffer().snapshot();
    let mut found: BTreeMap<String, Vec<PluginDiagnostic>> = BTreeMap::new();
    let kept = (cx.global::<PluginDiagnostics>().diagnostics.store).diagnostics(buffer, &snapshot);
    for item in kept {
        found.entry(item.plugin.clone()).or_default().push(item);
    }
    let names: HashMap<String, String> = (found.keys())
        .map(|plugin| {
            let name = plugin_host::plugin_name(plugin, cx);
            (plugin.clone(), name.unwrap_or_else(|| plugin.clone()))
        })
        .collect();
    let state = cx.global_mut::<PluginDiagnostics>();
    if found.is_empty() {
        state.known.remove(&buffer);
    } else {
        state.known.insert(buffer);
    }
    for plugin in found.keys() {
        key(state, plugin);
    }
    // Every plugin seen so far: an editor may hold any one's.
    let keys: Vec<(String, u64)> = (state.keys.iter())
        .map(|(plugin, key)| (plugin.clone(), *key))
        .collect();
    let mut wanted = Vec::new();
    for (plugin, key) in keys {
        let items = found.remove(&plugin);
        let load = match items {
            Some(_) => Some(load(state, &plugin)),
            None => state.loads.get(&plugin).copied(),
        };
        let items = items.map(|items| {
            (items.into_iter())
                .map(|item| diagnostic(item, &names[&plugin]))
                .collect()
        });
        wanted.push(Wanted {
            owner: DiagnosticOwner::Plugin(key),
            load: load.map(DiagnosticIdentity::Plugin),
            items,
        });
    }
    for editor in &editors {
        state.seen.insert(editor.entity_id(), buffer);
    }
    let version = snapshot.version();
    for editor in &editors {
        editor.update(cx, |view, cx| {
            let model = view.model_mut();
            let mut changed = false;
            for wanted in &wanted {
                let set = model.diagnostic_set::<DiagnosticOwner, DiagnosticIdentity>();
                let held = set.and_then(|set| set.identity(wanted.owner));
                let holds_any =
                    set.is_some_and(|set| !set.server_entries(&snapshot, wanted.owner).is_empty());
                if let Some(held) = held
                    && Some(held) != wanted.load
                {
                    changed |= model.clear_diagnostics(wanted.owner, held);
                }
                match (&wanted.items, wanted.load) {
                    (Some(items), Some(load)) => {
                        changed |= model.apply_diagnostics(
                            wanted.owner,
                            load,
                            version,
                            items.clone(),
                            Some(load),
                        );
                    }
                    // Emptied, not cleared: the load may publish again.
                    (None, Some(load)) if held == Some(load) && holds_any => {
                        changed |= model.apply_diagnostics(
                            wanted.owner,
                            load,
                            version,
                            Vec::new(),
                            Some(load),
                        );
                    }
                    _ => {}
                }
            }
            if changed {
                cx.notify();
            }
        });
    }
}

/// Plugin `plugin`'s current load, given on first use.
fn load(state: &mut PluginDiagnostics, plugin: &str) -> u64 {
    if let Some(load) = state.loads.get(plugin) {
        return *load;
    }
    state.last += 1;
    state.loads.insert(plugin.to_owned(), state.last);
    state.last
}

/// Plugin `plugin`'s owner key, given on first use.
fn key(state: &mut PluginDiagnostics, plugin: &str) -> u64 {
    if let Some(key) = state.keys.get(plugin) {
        return *key;
    }
    state.last += 1;
    state.keys.insert(plugin.to_owned(), state.last);
    state.last
}

/// A plugin's diagnostic as fe-editor keeps it, its range in the snapshot it
/// was read in; one without a source shows plugin `name`.
fn diagnostic(item: PluginDiagnostic, name: &str) -> (Range<usize>, Diagnostic) {
    let found = item.diagnostic;
    let severity = match found.severity {
        PluginSeverity::Error => Severity::Error,
        PluginSeverity::Warning => Severity::Warning,
        PluginSeverity::Info => Severity::Information,
        PluginSeverity::Hint => Severity::Hint,
    };
    let mut diagnostic = Diagnostic::new(severity, found.message);
    diagnostic.source = Some(found.source.unwrap_or_else(|| name.to_owned()));
    diagnostic.code = found.code;
    (item.range, diagnostic)
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
#[path = "plugin_diagnostics_tests.rs"]
mod plugin_diagnostics_tests;
