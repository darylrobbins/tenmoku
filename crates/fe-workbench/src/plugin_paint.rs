//! What plugins draw: their decorations in the editors and their items on
//! the status bar, from fe-plugin's [`DecorationStore`] and
//! [`StatusItems`], one of each for the app ([`Paint`]). Nothing here
//! calls a plugin: the editors and the bars read the host's copy.
//!
//! - **Decorations.** Each editor a workbench shows ([`sync`]) gets the
//!   store's provider for its buffer, by the bridge's buffer ids
//!   ([`crate::plugin_bridge`]); a split made from an editor carries its
//!   provider over, so the copy is swapped for the split's own. An editor
//!   closed loses it, and a buffer no editor shows any more is closed in
//!   the store. A set is made against a version: the snapshots plugins
//!   were shown are kept, the last [`SHOWN`] of each buffer, as an editor
//!   catches up with each change and as a plugin reads the buffer, and a
//!   set against one gone is refused for the plugin to set again. Gutter
//!   entries are kept by the store but not drawn: the gutter has no seam
//!   for them.
//! - **Status items.** Each window's bar shows every plugin's items, on
//!   the side their alignment names, after the built-in ones. A click runs
//!   the item's command as its plugin, authorized as `commands.invoke` is
//!   ([`clicked`]): an editor command through the editor's registry, which
//!   checks it against the plugin's grants, on the window's active editor;
//!   one of the plugin's own commands as the palette runs it. Any other is
//!   refused.
//! - **Repaint.** A plugin's call that reached the `decorations` or
//!   `status` imports asks for a repaint ([`Paint::repaint`]) without
//!   waiting: the main thread then notifies the editors and syncs the bars.
//!   Unloading a plugin ([`unload`]), or a load of it failing after its
//!   slices were made ([`Paint::forget`]), drops its sets and items and
//!   repaints: whatever its `init` set before it failed goes too.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use fe_buffer::BufferSnapshot;
use fe_command::{CommandId, Invocation, Registry, Source, Value};
use fe_editor::{commands::Editor, decorations::ProviderId};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_plugin::{
    decorations::{self, BufferVersions, DecorationStore},
    editor::BufferAccess,
    status::{self, Alignment, StatusItem as PluginItem, StatusItems},
};
use gpui::{
    App, Entity, EntityId, Focusable as _, Global, Subscription, WeakEntity, Window, WindowId,
};
use kiln_l10n::tr;
use kiln_shell::StatusSide;

use crate::{Workbench, plugin_bridge, plugin_diagnostics, plugin_events, plugin_host};

/// How many of a buffer's versions plugins were shown are kept to map a
/// set made against one to the buffer now.
pub(crate) const SHOWN: usize = 16;

/// The bar's priority for plugins' items: after the built-in ones on
/// either side, in the order first set.
const PRIORITY: i32 = 1000;

/// The command a plugin's item on the bar runs when clicked, naming the
/// item: it runs the item's own command as the plugin ([`clicked`]).
const CLICK: &str = "workbench.click_plugin_status_item";

/// The app's plugin decorations and status items, and the way to ask the
/// main thread to repaint them; what each plugin's slices are made on.
/// Clones share them.
#[derive(Clone)]
pub(crate) struct Paint {
    decorations: DecorationStore,
    items: StatusItems,
    shown: Shown,
    repaint: async_channel::Sender<()>,
    diagnostics: plugin_diagnostics::Diagnostics,
}

impl Paint {
    /// Plugin `plugin`'s `decorations` slice, as far as `grants` allow,
    /// reading the buffers through `buffers`.
    pub(crate) fn decorations(
        &self,
        plugin: &str,
        grants: &fe_command::Grants,
        buffers: Box<dyn BufferAccess>,
    ) -> decorations::State {
        decorations::State::new(plugin, grants, &self.decorations, self.versions(buffers))
    }

    /// Plugin `plugin`'s `diagnostics` slice ([`crate::plugin_diagnostics`]),
    /// as far as `grants` allow, mapping versions as its decorations do.
    pub(crate) fn diagnostics(
        &self,
        plugin: &str,
        grants: &fe_command::Grants,
        buffers: Box<dyn BufferAccess>,
    ) -> fe_plugin::diagnostics::State {
        (self.diagnostics).state(plugin, grants, self.versions(buffers))
    }

    /// The buffers as a plugin's slice maps a version it names: now,
    /// through `buffers`, and as plugins were shown them.
    pub(crate) fn versions(&self, buffers: Box<dyn BufferAccess>) -> Box<dyn BufferVersions> {
        Box::new(Buffers {
            buffers,
            shown: self.shown.clone(),
        })
    }

    /// Plugin `plugin`'s `status` slice, as far as `grants` allow.
    pub(crate) fn status(&self, plugin: &str, grants: &fe_command::Grants) -> status::State {
        status::State::new(plugin, grants, &self.items)
    }

    /// Drops plugin `plugin`'s sets and items, and its slices set nothing
    /// more, from any thread; then asks for a repaint.
    pub(crate) fn forget(&self, plugin: &str) {
        self.decorations.unload(plugin);
        self.items.unload(plugin);
        self.diagnostics.forget(plugin);
        self.repaint();
    }

    /// Asks the main thread to repaint the decorations and the bars, from
    /// any thread, without waiting; asks made before it does are one.
    pub(crate) fn repaint(&self) {
        self.repaint.try_send(()).ok();
    }
}

/// The snapshots plugins were shown, by buffer id. Clones share them.
#[derive(Clone, Default)]
struct Shown(Arc<Mutex<HashMap<u64, Versions>>>);

/// A buffer's snapshots plugins were shown, each with its version as
/// plugins know it, oldest first.
type Versions = VecDeque<(u64, BufferSnapshot)>;

impl Shown {
    /// Keeps `snapshot`, buffer `buffer` at plugin version `version`,
    /// unless it has one as new.
    fn record(&self, buffer: u64, version: u64, snapshot: &BufferSnapshot) {
        let mut shown = lock(&self.0);
        let versions = shown.entry(buffer).or_default();
        if versions
            .back()
            .is_some_and(|(newest, _)| *newest >= version)
        {
            return;
        }
        if versions.len() >= SHOWN {
            versions.pop_front();
        }
        versions.push_back((version, snapshot.clone()));
    }

    fn at(&self, buffer: u64, version: u64) -> Option<BufferSnapshot> {
        let shown = lock(&self.0);
        let versions = shown.get(&buffer)?;
        let (_, snapshot) = versions.iter().find(|(kept, _)| *kept == version)?;
        Some(snapshot.clone())
    }

    fn forget(&self, buffer: u64) {
        lock(&self.0).remove(&buffer);
    }
}

/// A plugin's buffers, as its decorations name them: now, through the
/// bridge, and as it was shown them.
struct Buffers {
    buffers: Box<dyn BufferAccess>,
    shown: Shown,
}

impl BufferVersions for Buffers {
    fn now(&mut self, buffer: u64) -> Option<BufferSnapshot> {
        let text = self.buffers.snapshot(buffer)?;
        // A set made against it next maps from it.
        self.shown.record(buffer, text.version, &text.snapshot);
        Some(text.snapshot)
    }

    fn at(&mut self, buffer: u64, version: u64) -> Option<BufferSnapshot> {
        self.shown.at(buffer, version)
    }
}

/// The main thread's side.
struct PluginPaint {
    paint: Paint,
    /// Each editor given the store's provider, by its entity.
    editors: HashMap<EntityId, Painted>,
    /// Each window's plugin items, as last set on its bar.
    bars: HashMap<WindowId, Vec<kiln_shell::StatusItem>>,
    /// The editor's commands, checking a click's invocation as the
    /// plugin's: made on the first click.
    registry: Option<Registry<Editor>>,
    _repaint: gpui::Task<()>,
}

impl Global for PluginPaint {}

/// An editor painting plugins' decorations.
struct Painted {
    editor: WeakEntity<EditorView>,
    buffer: u64,
    provider: ProviderId,
    /// Keeps the snapshots it catches up with.
    _observe: Subscription,
}

/// The app's [`Paint`], made on first use.
pub(crate) fn paint(cx: &mut App) -> Paint {
    if let Some(paint) = cx.try_global::<PluginPaint>() {
        return paint.paint.clone();
    }
    let (repaint, asked) = async_channel::bounded(1);
    let paint = Paint {
        decorations: DecorationStore::new(),
        items: StatusItems::new(),
        shown: Shown::default(),
        repaint,
        diagnostics: plugin_diagnostics::diagnostics(cx),
    };
    let task = cx.spawn(async move |cx| {
        while asked.recv().await.is_ok() {
            cx.update(repaint_now);
        }
    });
    cx.set_global(PluginPaint {
        paint: paint.clone(),
        editors: HashMap::new(),
        bars: HashMap::new(),
        registry: None,
        _repaint: task,
    });
    // The editors open already paint from now on.
    cx.defer(sync);
    paint
}

/// Drops plugin `plugin`'s sets and items, which unloaded, and repaints.
pub(crate) fn unload(plugin: &str, cx: &mut App) {
    let Some(paint) = cx.try_global::<PluginPaint>() else {
        return;
    };
    paint.paint.decorations.unload(plugin);
    paint.paint.items.unload(plugin);
    repaint_now(cx);
}

/// The open editors or windows may have changed: each editor shown gets
/// the provider for its buffer, those closed lose it, buffers no longer
/// open are closed in the store, and each window's bar shows the items.
pub(crate) fn sync(cx: &mut App) {
    if !cx.has_global::<PluginPaint>() {
        return;
    }
    let open = plugin_bridge::open_editors(cx);
    let shown = cx.global::<PluginPaint>().paint.shown.clone();
    let mut buffers = HashSet::new();
    let mut editors = HashSet::new();
    for (buffer, version, editor) in &open {
        buffers.insert(*buffer);
        editors.insert(editor.entity_id());
        shown.record(
            *buffer,
            *version,
            &editor.read(cx).model().buffer().snapshot(),
        );
        let given = cx.global::<PluginPaint>().editors.get(&editor.entity_id());
        if given.is_some_and(|painted| painted.buffer == *buffer) {
            continue;
        }
        give(editor, *buffer, cx);
    }
    let state = cx.global_mut::<PluginPaint>();
    let closed: Vec<Painted> = (state.editors)
        .extract_if(|editor, _| !editors.contains(editor))
        .map(|(_, painted)| painted)
        .collect();
    let store = state.paint.decorations.clone();
    for painted in closed {
        if let Some(editor) = painted.editor.upgrade() {
            editor.update(cx, |view, cx| {
                view.model_mut()
                    .remove_decoration_provider(painted.provider);
                cx.notify();
            });
        }
    }
    let gone: Vec<u64> = {
        let shown = lock(&shown.0);
        (shown.keys().copied())
            .filter(|buffer| !buffers.contains(buffer))
            .collect()
    };
    for buffer in gone {
        store.close(buffer);
        shown.forget(buffer);
    }
    sync_bars(cx);
    plugin_diagnostics::sync(cx);
}

/// Gives `editor`, of buffer `buffer`, the store's provider for it, in
/// place of one it carried over from the editor it was split from.
fn give(editor: &Entity<EditorView>, buffer: u64, cx: &mut App) {
    let state = cx.global_mut::<PluginPaint>();
    let carried: Vec<ProviderId> = (state.editors.values())
        .filter(|painted| painted.buffer == buffer)
        .map(|painted| painted.provider)
        .collect();
    let replaced = state.editors.remove(&editor.entity_id());
    let provider = Rc::new(RefCell::new(state.paint.decorations.provider(buffer)));
    let provider = editor.update(cx, |view, cx| {
        let model = view.model_mut();
        for id in carried
            .into_iter()
            .chain(replaced.map(|painted| painted.provider))
        {
            model.remove_decoration_provider(id);
        }
        let id = model.add_decoration_provider(provider);
        cx.notify();
        id
    });
    let observe = cx.observe(editor, |editor, cx| caught_up(&editor, cx));
    let painted = Painted {
        editor: editor.downgrade(),
        buffer,
        provider,
        _observe: observe,
    };
    (cx.global_mut::<PluginPaint>().editors).insert(editor.entity_id(), painted);
}

/// `editor` notified: its snapshot is kept at its buffer's plugin version.
/// The events of its changes go first, so the version is the one they
/// name.
fn caught_up(editor: &Entity<EditorView>, cx: &mut App) {
    let Some(painted) = cx.global::<PluginPaint>().editors.get(&editor.entity_id()) else {
        return;
    };
    let buffer = painted.buffer;
    let snapshot = editor.read(cx).model().buffer().snapshot();
    plugin_events::catch_up(cx);
    let Some((id, version)) =
        plugin_bridge::advance(snapshot.id(), snapshot.version(), Some(&snapshot), cx)
    else {
        return;
    };
    if id == buffer {
        (cx.global::<PluginPaint>().paint.shown).record(id, version, &snapshot);
    }
}

/// Repaints what plugins draw: the editors painting their decorations
/// notified, and each window's bar showing their items.
fn repaint_now(cx: &mut App) {
    let Some(state) = cx.try_global::<PluginPaint>() else {
        return;
    };
    let editors: Vec<_> = (state.editors.values())
        .filter_map(|painted| painted.editor.upgrade())
        .collect();
    for editor in editors {
        editor.update(cx, |_, cx| cx.notify());
    }
    sync_bars(cx);
}

/// Sets each window's bar to show the plugins' items: those gone removed,
/// those new or changed set.
fn sync_bars(cx: &mut App) {
    let items: Vec<kiln_shell::StatusItem> = (cx.global::<PluginPaint>().paint.items.items())
        .iter()
        .map(bar_item)
        .collect();
    let windows: Vec<_> = (cx.windows().into_iter())
        .filter_map(|window| window.downcast::<Workbench>())
        .collect();
    let mut open = HashSet::new();
    for window in windows {
        let Ok(bar) = window
            .read(cx)
            .map(|workbench| workbench.status_bar().clone())
        else {
            continue;
        };
        let id = window.window_id();
        open.insert(id);
        let last = (cx.global_mut::<PluginPaint>().bars)
            .insert(id, items.clone())
            .unwrap_or_default();
        if last == items {
            continue;
        }
        bar.update(cx, |bar, cx| {
            for item in &last {
                if !items.iter().any(|now| now.id == item.id) {
                    bar.remove_item(&item.id, cx);
                }
            }
            for item in &items {
                if !last.contains(item) {
                    bar.set_item(item.clone(), cx);
                }
            }
        });
    }
    (cx.global_mut::<PluginPaint>().bars).retain(|window, _| open.contains(window));
}

/// Plugin `item` as the bar draws it: named by plugin and id, and, with a
/// command, clicked through [`CLICK`].
fn bar_item(item: &PluginItem) -> kiln_shell::StatusItem {
    let side = match item.alignment {
        Alignment::Left => StatusSide::Left,
        Alignment::Right => StatusSide::Right,
    };
    let id = format!("plugin:{}:{}", item.plugin, item.id);
    let mut shown = kiln_shell::StatusItem::new(id, side, PRIORITY, item.text.clone());
    shown.tooltip = item.tooltip.clone().map(Into::into);
    if item.command.is_some() {
        let click = Invocation::new(CommandId::parse(CLICK).expect("a valid id"))
            .args(serde_json::json!({ "plugin": item.plugin, "item": item.id }))
            .source(Source::Mouse);
        shown = shown.command(click);
    }
    shown
}

/// Runs `run`, if it's a click on a plugin's item, as [`CLICK`] says, and
/// returns true; false for any other command. `editor` is the window's
/// active editor, which an editor command runs on.
pub(crate) fn clicked(
    run: &RunCommand,
    editor: Option<Entity<EditorView>>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    if run.0.command.as_str() != CLICK {
        return false;
    }
    let arg = |name: &str| run.0.args.get(name).and_then(Value::as_str);
    let (Some(plugin), Some(id)) = (arg("plugin"), arg("item")) else {
        log::warn!("{CLICK} needs a plugin and an item");
        return true;
    };
    let Some(state) = cx.try_global::<PluginPaint>() else {
        return true;
    };
    let Some(invocation) = (state.paint.items.item(plugin, id)).and_then(|item| item.invocation())
    else {
        return true;
    };
    // One of the plugin's own, as the palette runs it.
    let own = plugin_host::command_host(cx)
        .is_some_and(|commands| commands.registered(plugin).contains(&invocation.command));
    if own {
        let invocation = invocation.context(run.0.context.clone());
        plugin_host::dispatch(&RunCommand(invocation), cx);
        return true;
    }
    // An editor command, checked against the plugin's grants as its
    // invocation through `commands.invoke` is.
    let invocation = invocation.context(fe_editor::commands::context());
    let checked = registry(cx).and_then(|registry| registry.validate(&invocation).map(drop));
    if let Err(error) = checked {
        log::warn!("{error}");
        return true;
    }
    let Some(editor) = editor else {
        log::warn!("{} has no editor to run on", invocation.command);
        return true;
    };
    // Not inside the handler that took the click.
    window.defer(cx, move |window, cx| {
        let handle = editor.focus_handle(cx);
        handle.dispatch_action(&RunCommand(invocation), window, cx);
    });
    true
}

/// The editor's commands under the plugin host's policy, or why there are
/// none: no plugin was ever loaded.
fn registry(cx: &mut App) -> Result<&Registry<Editor>, fe_command::DispatchError> {
    if cx.global::<PluginPaint>().registry.is_none() {
        let Some(commands) = plugin_host::command_host(cx) else {
            return Err(fe_command::DispatchError::Failed {
                command: CommandId::parse(CLICK).expect("a valid id"),
                error: fe_command::CommandError(tr!("workbench-extensions-paint-no-plugin").into()),
            });
        };
        let mut registry = fe_editor::commands::registry();
        registry.set_plugin_policy(Arc::new(commands));
        cx.global_mut::<PluginPaint>().registry = Some(registry);
    }
    Ok(cx
        .global::<PluginPaint>()
        .registry
        .as_ref()
        .expect("made above"))
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
#[path = "plugin_paint_tests.rs"]
mod plugin_paint_tests;
