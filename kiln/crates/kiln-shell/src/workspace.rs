// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`Workspace`]: a window's [`Shell`] whose centre is a [`PaneGroup`] of
//! panes, and the glue every app with panes would otherwise write itself.
//! It keeps the group's panes in step: each sees the others ([`Peers`]),
//! a pane its last tab left closes unless it's the last, a tab dropped on
//! a pane or its edge moves there, and a click makes a pane the active
//! one. The window's title and the shell's follow the active item; focus
//! goes back to it, or to the active pane while it's empty; and closing
//! the window or quitting asks every pane's items ([`close`]).
//!
//! Docks go at the window's edges ([`Workspace::add_panel`]), and focus
//! never stays in one that's hidden. A built-in status bar shows the
//! workspace's own items over the active item's ([`Workspace::set_status_item`]),
//! redrawing as the active item changes, with no notify from the host.
//!
//! Given a [`WindowLayout`] ([`Workspace::set_layout`]), the workspace
//! saves its own layout: its panes and docks, and the app's keys
//! ([`Workspace::on_save`]), as their shape changes, in full as the app
//! quits and as the window closes; [`Workspace::restore`] puts one back.
//!
//! A host draws the workspace and adds its own around it, hearing
//! [`WorkspaceEvent`]s; [`Workspace::shell`] and [`Workspace::group_mut`]
//! reach the parts.

use std::{collections::HashMap, rc::Rc};

use gpui::{
    App, AppContext as _, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable,
    IntoElement, Pixels, Render, SharedString, Subscription, Task, WeakEntity, Window, WindowId,
    div,
};
use kiln_command::Value;
use kiln_platform::Capabilities;
use kiln_ui::ui_theme;

use crate::{
    Direction, Dock, DockItem, DockPosition, DockState, DockView as _, ItemHandle, ItemId,
    ItemPane, MenuBarView, PaneEvent, PaneGroup, PaneView, Shell, StatusItem, StatusItems,
    close::{self, WindowItems},
    dock::DockEvent,
    layout::{self, WindowLayout},
    pane::{Peers, TabDropped},
    status,
};

/// Makes an empty pane, not yet in a group.
type NewPane<P> = Box<dyn Fn(&mut Window, &mut App) -> Entity<P>>;

/// Gives the app's own keys for the saved entry.
type OnSave = Box<dyn Fn(&App) -> Vec<(&'static str, Value)>>;

/// The docks' positions, as their entries are named.
const DOCKS: [(DockPosition, &str); 3] = [
    (DockPosition::Left, "left"),
    (DockPosition::Right, "right"),
    (DockPosition::Bottom, "bottom"),
];

/// A restore whose items are still being read.
struct Restoring {
    /// Puts the items in once they're read; dropped, it never does.
    filling: Task<()>,
    /// The entry restored, saved in its place at quit or close meanwhile.
    saved: Value,
}

/// What a workspace tells its host.
pub enum WorkspaceEvent<P = ItemPane> {
    /// The active pane, or its active item, is another one.
    ActiveChanged,
    /// A pane joined the group, split off by a command or a dropped tab, or
    /// restored.
    PaneAdded(Entity<P>),
    /// The pane with this ID left the group.
    PaneRemoved(EntityId),
}

/// A window's shell around its panes, one per window:
///
/// ```ignore
/// cx.open_window(options, |window, cx| {
///     cx.new(|cx| Workspace::item_panes("notes", window, cx).title("Notes"))
/// })
/// ```
///
/// A host with a pane view of its own gives the first pane and a factory
/// for the empty panes a restored layout needs ([`Workspace::new`]); a
/// split makes its pane with [`PaneView::new_peer`].
pub struct Workspace<P: PaneView = ItemPane> {
    shell: Entity<Shell>,
    group: PaneGroup<P>,
    /// The pane items open in, whose active item the titles follow.
    active: Entity<P>,
    /// The group's panes, which each of them sees.
    peers: Peers<P>,
    /// The title shown while there's no active item.
    title: SharedString,
    new_pane: NewPane<P>,
    /// The active pane and its active item when last followed.
    shown: (EntityId, Option<ItemId>),
    /// Per pane: its notifications, its activation and tabs dropped on it.
    panes: HashMap<EntityId, [Subscription; 3]>,
    /// The app's root key context, which the docks' rail tooltips look the
    /// show keys up in.
    context: SharedString,
    docks: HashMap<DockPosition, Entity<Dock>>,
    /// The workspace's own status items, shown over the active item's.
    status: StatusItems,
    /// Called each time the bar draws.
    status_sources: Vec<fn(&App) -> Option<StatusItem>>,
    /// Hear the docks' rail clicks and changes.
    dock_events: Vec<Subscription>,
    /// Where the layout is saved, once it's given, and the window it's
    /// the layout of.
    layout: Option<(WindowLayout, WindowId)>,
    /// Each pane's items when the layout was last saved: a reorder the
    /// shape doesn't show, its documents numbered in order, still saves.
    saved_items: Vec<Vec<ItemId>>,
    /// Save the layout as it changes, at quit and as the window closes.
    layout_hooks: Vec<Subscription>,
    on_save: Vec<OnSave>,
    restoring: Option<Restoring>,
}

impl<P: PaneView> Workspace<P> {
    /// A workspace of one pane, `first`, its shell under the app's root key
    /// `context` with this platform's chrome. `new_pane` makes the empty
    /// panes a restored layout needs ([`Workspace::new_pane`]).
    pub fn new(
        context: impl Into<SharedString>,
        first: Entity<P>,
        new_pane: impl Fn(&mut Window, &mut App) -> Entity<P> + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let context = context.into();
        let workspace = cx.weak_entity();
        let centre = cx.new(|cx| Panes {
            workspace: workspace.clone(),
            focus: cx.focus_handle(),
        });
        let shell = cx.new(|cx| {
            let mut shell = Shell::new(context.clone(), ui_theme(cx), window, cx)
                .platform_chrome(Capabilities::current(), cx);
            shell.set_center(centre, cx);
            shell
        });
        let this = cx.entity();
        let bar = cx.new(|cx| {
            // The bar draws what the workspace holds, so it redraws as the
            // workspace changes.
            cx.observe(&this, |_, _, cx| cx.notify()).detach();
            StatusBar(workspace.clone())
        });
        shell.update(cx, |shell, cx| shell.set_status_bar(Some(bar.into()), cx));
        // Closing the window and quitting ask every pane's items first.
        let items: WindowItems = Rc::new(move |cx| {
            (workspace.upgrade())
                .map(|workspace| workspace.read(cx).group.items(cx))
                .unwrap_or_default()
        });
        close::set_window_items(window, items, cx);
        // The titles once the builder has given the fallback.
        cx.defer_in(window, Self::follow_active);
        let shown = (first.entity_id(), active_item_id(&first, cx));
        let mut workspace = Self {
            shell,
            group: PaneGroup::new(first.clone()),
            active: first.clone(),
            peers: Peers::default(),
            title: SharedString::default(),
            new_pane: Box::new(new_pane),
            shown,
            panes: HashMap::new(),
            context,
            docks: HashMap::new(),
            status: StatusItems::default(),
            status_sources: Vec::new(),
            dock_events: Vec::new(),
            layout: None,
            saved_items: Vec::new(),
            layout_hooks: Vec::new(),
            on_save: Vec::new(),
            restoring: None,
        };
        workspace.watch(&first, window, cx);
        workspace
    }

    /// Draws the chrome as on a platform with `capabilities`, rather than
    /// this one's: for tests, and for showing one platform's chrome on
    /// another.
    pub fn capabilities(self, capabilities: Capabilities, cx: &mut Context<Self>) -> Self {
        self.shell.update(cx, |shell, cx| {
            shell.set_title_bar(capabilities.unified_title_bar, cx);
            let bar = (!capabilities.native_menus).then(|| cx.new(MenuBarView::new));
            shell.set_menu_bar(bar, cx);
            shell.set_window_frame(!capabilities.unified_title_bar, cx);
        });
        self
    }

    /// The title the window and the shell show while there's no active
    /// item, such as the app's name.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = title.into();
        self
    }

    /// Changes the fallback title: see [`Workspace::title`].
    pub fn set_title(
        &mut self,
        title: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.title = title.into();
        self.follow_active(window, cx);
    }

    pub fn shell(&self) -> &Entity<Shell> {
        &self.shell
    }

    pub fn group(&self) -> &PaneGroup<P> {
        &self.group
    }

    /// The group, to change as the workspace's methods don't: a pane added
    /// or removed through it isn't watched or forgotten.
    pub fn group_mut(&mut self) -> &mut PaneGroup<P> {
        &mut self.group
    }

    /// The pane items open in.
    pub fn active_pane(&self) -> &Entity<P> {
        &self.active
    }

    /// The active pane's active item, if it has one.
    pub fn active_item<'a>(&self, cx: &'a App) -> Option<&'a dyn ItemHandle> {
        self.active.read(cx).pane().active_item()
    }

    /// Adds `panel` to the dock at `position`, making the dock on first use,
    /// with its rail's tooltips finding the panel's show key in the
    /// workspace's context.
    pub fn add_panel(
        &mut self,
        position: DockPosition,
        panel: Entity<impl DockItem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dock = match self.docks.get(&position) {
            Some(dock) => dock.clone(),
            None => {
                let state = DockState::new(position, ui_theme(cx))
                    .shortcut_contexts([self.context.clone()]);
                let dock = cx.new(|_| Dock::with_state(state));
                // A rail click hid the panel that had focus.
                let hid =
                    cx.subscribe_in(&dock, window, |workspace, _, _: &DockEvent, window, cx| {
                        workspace.focus_active(window, cx)
                    });
                // A toggle, a resize or another panel shown.
                let changed = cx.observe(&dock, |workspace, _, cx| workspace.save_layout(cx));
                self.dock_events.extend([hid, changed]);
                self.docks.insert(position, dock.clone());
                self.shell.update(cx, |shell, cx| {
                    shell.set_dock(position, Some(dock.clone().into()), cx)
                });
                dock
            }
        };
        dock.update(cx, |dock, cx| dock.add_panel(panel, cx));
    }

    /// The dock at `position`, if a panel was added there.
    pub fn dock(&self, position: DockPosition) -> Option<&Entity<Dock>> {
        self.docks.get(&position)
    }

    /// Sets the width of the dock at `position`, or its height at the
    /// bottom; nothing if it has none.
    pub fn set_dock_size(&mut self, position: DockPosition, size: Pixels, cx: &mut Context<Self>) {
        if let Some(dock) = self.docks.get(&position) {
            dock.update(cx, |dock, cx| {
                dock.dock_mut().set_size(size);
                cx.notify();
            });
        }
    }

    /// Shows the dock at `position` if it's hidden, else hides it. Focus
    /// doesn't stay in a panel that's hidden: it goes to the active item.
    pub fn toggle_dock(
        &mut self,
        position: DockPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dock) = self.docks.get(&position).cloned() else {
            return;
        };
        dock.update(cx, |dock, cx| dock.toggle(cx));
        if dock.read(cx).dock().hidden_panel_focused(window, cx) {
            self.focus_active(window, cx);
        }
    }

    /// Shows `item` on the status bar over the active item's, replacing the
    /// workspace's item with its id.
    pub fn set_status_item(&mut self, item: StatusItem, cx: &mut Context<Self>) {
        self.status.set(item);
        cx.notify();
    }

    /// Removes the workspace's status item `id`.
    pub fn remove_status_item(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.status.remove(id) {
            cx.notify();
        }
    }

    /// Calls `source` each time the status bar draws, showing the item it
    /// gives. It runs on every draw, so keep it O(1): read a value, don't
    /// compute one.
    pub fn add_status_source(
        &mut self,
        source: fn(&App) -> Option<StatusItem>,
        cx: &mut Context<Self>,
    ) {
        self.status_sources.push(source);
        cx.notify();
    }

    /// Every status item to draw: the workspace's and the sources' over the
    /// active item's.
    fn status_items(&self, cx: &App) -> Vec<StatusItem> {
        let mut items = self.status.clone();
        for source in &self.status_sources {
            if let Some(item) = source(cx) {
                items.set(item);
            }
        }
        items.items_for(self.active_item(cx), cx)
    }

    /// An empty pane from the factory given to [`Workspace::new`], not yet
    /// in the group.
    pub fn new_pane(&self, window: &mut Window, cx: &mut App) -> Entity<P> {
        (self.new_pane)(window, cx)
    }

    /// Adds `item` to the active pane, shown and focused.
    pub fn add_item(&mut self, item: Box<dyn ItemHandle>, window: &mut Window, cx: &mut App) {
        (self.active).update(cx, |pane, cx| pane.insert_item(item, None, window, cx));
    }

    /// Shows and focuses the first item of the active pane that `pick`s;
    /// false if none does.
    pub fn activate_item_where(
        &mut self,
        pick: impl Fn(&dyn ItemHandle, &App) -> bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let pane = self.active.clone();
        let items = pane.read(cx).pane().items();
        let Some(ix) = items.iter().position(|item| pick(&**item, cx)) else {
            return false;
        };
        pane.update(cx, |pane, cx| {
            pane.activate_item(ix, window, cx);
            cx.notify();
        });
        self.follow_active(window, cx);
        true
    }

    /// Splits a new pane off the active one, `direction` of it, showing a
    /// second view of the active item ([`ItemHandle::split`]), and makes it
    /// the active pane, focused.
    pub fn split(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<P> {
        let old = self.active.clone();
        let item = (old.read(cx).pane().active_item()).map(ItemHandle::boxed_clone);
        let copy = item.and_then(|item| item.split(window, cx));
        let new = old.update(cx, |pane, cx| cx.new(|cx| pane.new_peer(cx)));
        self.group.split(&old, new.clone(), direction);
        self.watch(&new, window, cx);
        cx.emit(WorkspaceEvent::PaneAdded(new.clone()));
        self.active = new.clone();
        match copy {
            Some(copy) => new.update(cx, |pane, cx| pane.insert_item(copy, None, window, cx)),
            None => window.focus(&new.focus_handle(cx), cx),
        }
        self.follow_active(window, cx);
        new
    }

    /// Closes `pane`, unless it's the last, making the nearest pane active
    /// if it was. Focus in it goes to the active pane's item: a tab whose
    /// close waited may have left focus on the pane it emptied after another
    /// became active. Its items close with it, unasked: a host asks them
    /// first ([`close::confirm`]).
    pub fn close_pane(&mut self, pane: &Entity<P>, window: &mut Window, cx: &mut Context<Self>) {
        if self.group.panes().len() < 2 || !self.group.panes().contains(&pane) {
            return;
        }
        let focused = pane.focus_handle(cx);
        let had_focus = focused.is_focused(window) || focused.contains_focused(window, cx);
        let nearest = self.group.nearest(pane, cx);
        self.group.remove(pane);
        self.panes.remove(&pane.entity_id());
        self.sync_peers();
        cx.emit(WorkspaceEvent::PaneRemoved(pane.entity_id()));
        let was_active = self.active == *pane;
        if was_active {
            self.active = nearest.unwrap_or_else(|| self.group.panes()[0].clone());
        }
        if was_active || had_focus {
            self.focus_active(window, cx);
        }
        self.follow_active(window, cx);
    }

    /// What has focus when the active pane does: its active item, or the
    /// pane itself while it's empty.
    fn active_focus(&self, cx: &App) -> FocusHandle {
        match self.active_item(cx) {
            Some(item) => item.item_focus_handle(cx),
            None => self.active.focus_handle(cx),
        }
    }

    /// Focuses the active item, or the active pane while it's empty.
    pub fn focus_active(&mut self, window: &mut Window, cx: &mut App) {
        window.focus(&self.active_focus(cx), cx);
    }

    /// Joins `pane`, already in the group, to the workspace's panes: they
    /// all see it, and the workspace follows its changes, its clicks and
    /// the tabs dropped on it.
    fn watch(&mut self, pane: &Entity<P>, window: &mut Window, cx: &mut Context<Self>) {
        let peers = self.peers.clone();
        pane.update(cx, |pane, _| pane.pane_mut().set_peers(peers));
        let changed = cx.observe_in(pane, window, Self::pane_changed);
        let activated = cx.subscribe_in(
            pane,
            window,
            |workspace, pane, _: &PaneEvent, window, cx| {
                if workspace.active != *pane {
                    workspace.active = pane.clone();
                    workspace.follow_active(window, cx);
                }
            },
        );
        let dropped = cx.subscribe_in(pane, window, Self::tab_dropped);
        self.panes
            .insert(pane.entity_id(), [changed, activated, dropped]);
        self.sync_peers();
    }

    /// Tells every pane what the group's panes are now.
    fn sync_peers(&self) {
        *self.peers.borrow_mut() = (self.group.panes().into_iter())
            .map(Entity::downgrade)
            .collect();
    }

    /// A pane, or an item in it, changed: a pane its last tab left closes,
    /// unless it's the last one.
    fn pane_changed(&mut self, pane: Entity<P>, window: &mut Window, cx: &mut Context<Self>) {
        let emptied = pane.update(cx, |pane, _| pane.pane_mut().take_emptied());
        if emptied && self.group.panes().len() > 1 {
            self.close_pane(&pane, window, cx);
        }
        self.follow_active(window, cx);
    }

    /// A tab was dropped on a pane, or on an edge of one to split it.
    fn tab_dropped(
        &mut self,
        _: &Entity<P>,
        dropped: &TabDropped<P>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = dropped.dragged.pane.upgrade() else {
            return;
        };
        let (item, destination) = (dropped.dragged.item, dropped.destination.clone());
        let moved = (self.group).move_tab(&source, item, destination, window, cx);
        if let Some((pane, split_off)) = moved {
            if split_off {
                self.watch(&pane, window, cx);
                cx.emit(WorkspaceEvent::PaneAdded(pane.clone()));
            }
            self.active = pane;
            self.follow_active(window, cx);
        }
    }

    /// The window's title and the shell's follow the active item, and the
    /// host hears when it's another.
    fn follow_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = match self.active_item(cx) {
            Some(item) => item.window_title(cx),
            None => self.title.clone(),
        };
        if window.window_title() != *title {
            window.set_window_title(&title);
        }
        self.shell.update(cx, |shell, cx| {
            if *shell.title() != title {
                shell.set_title(title, cx);
            }
        });
        let shown = (self.active.entity_id(), active_item_id(&self.active, cx));
        if shown != self.shown {
            self.shown = shown;
            cx.emit(WorkspaceEvent::ActiveChanged);
        }
        cx.notify();
    }
}

// Saved layouts.
impl<P: PaneView> Workspace<P> {
    /// Saves the workspace's layout in `layout` from now on: its
    /// [`entry`](Workspace::entry) whenever its shape changes (a split, a
    /// close, a tab moved, opened or closed, another active pane or item,
    /// a divider dragged, a dock toggled, resized or showing another
    /// panel, or anything else that notifies the workspace), in full as
    /// the app quits, and as its window closes or the workspace goes,
    /// whichever is first: kept if it's the last window, else deleted
    /// ([`layout`]). Nothing is saved while a restore is still reading.
    ///
    /// The first save waits for the end of this update, so a restore in
    /// it, before or after, isn't overwritten by the layout it replaces.
    pub fn set_layout(&mut self, layout: WindowLayout, window: &Window, cx: &mut Context<Self>) {
        let ours = window.window_handle().window_id();
        self.layout = Some((layout, ours));
        self.saved_items.clear();
        let at_quit = cx.on_app_quit(|workspace, cx| {
            let entry = workspace.full_entry(cx);
            let saved =
                (workspace.layout.as_mut()).map(|(layout, _)| layout.save_at_quit(entry, cx));
            async move {
                if let Some(saved) = saved {
                    saved.await;
                }
            }
        });
        let this = cx.weak_entity();
        let closed = cx.on_window_closed(move |cx, closed| {
            if closed == ours {
                (this.update(cx, |workspace, cx| workspace.close_layout(cx))).ok();
            }
        });
        let released = cx.on_release(|workspace, cx| workspace.close_layout(cx));
        let changed = cx.observe_self(|workspace, cx| workspace.save_layout(cx));
        self.layout_hooks = vec![at_quit, closed, released, changed];
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            (this.update(cx, |workspace, cx| workspace.save_layout(cx))).ok();
        });
    }

    /// Adds the app's own keys to the saved entry: `save` gives them each
    /// time it's built. They come back in the entry given to
    /// [`Workspace::restore`], for the app to read. A change to them alone
    /// is saved as the workspace next notifies.
    pub fn on_save(&mut self, save: impl Fn(&App) -> Vec<(&'static str, Value)> + 'static) {
        self.on_save.push(Box::new(save));
    }

    /// The workspace's saved entry: the app's keys ([`Workspace::on_save`]),
    /// `panes` ([`layout::save_panes`]) and `docks`, each dock's state
    /// ([`DockState::save`]) by its position, `left`, `right` or `bottom`.
    pub fn entry(&self, cx: &App) -> Value {
        let mut entry = serde_json::Map::new();
        for save in &self.on_save {
            for (key, value) in save(cx) {
                entry.insert(key.into(), value);
            }
        }
        let docks = (DOCKS.iter()).filter_map(|(position, name)| {
            let dock = self.docks.get(position)?;
            Some((*name, dock.read(cx).dock().save()))
        });
        let panes = layout::save_panes(&self.group, &self.active, cx);
        entry.insert("panes".into(), panes);
        entry.insert("docks".into(), Value::from_iter(docks));
        Value::Object(entry)
    }

    /// Whether a restore's items are still being read.
    pub fn restoring(&self) -> bool {
        self.restoring.is_some()
    }

    /// Puts back the layout `saved` holds, as [`Workspace::entry`] gave
    /// it: its panes, made with [`Workspace::new_pane`] in place of the
    /// workspace's, and the active one, at once, their items once they're
    /// read ([`layout::restore_panes`]), and each dock's state, for the
    /// docks with panels by now. The panes still empty then close, unless an
    /// item was opened in them meanwhile, and focus goes to the active item. A restore still reading is dropped. False,
    /// changing nothing, if `saved` has no panes.
    pub fn restore(&mut self, saved: &Value, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let new_pane = &self.new_pane;
        let mut new_pane = |window: &mut Window, cx: &mut App| new_pane(window, cx);
        let Some(restored) = (saved.get("panes"))
            .and_then(|panes| layout::restore_panes(panes, &mut new_pane, window, cx))
        else {
            return false;
        };
        for pane in self.group.panes().into_iter().cloned().collect::<Vec<_>>() {
            cx.emit(WorkspaceEvent::PaneRemoved(pane.entity_id()));
        }
        self.panes.clear();
        self.group.set_root(restored.root);
        self.active = restored.active;
        for pane in self.group.panes().into_iter().cloned().collect::<Vec<_>>() {
            self.watch(&pane, window, cx);
            cx.emit(WorkspaceEvent::PaneAdded(pane));
        }
        for (position, name) in DOCKS {
            let (Some(dock), Some(saved)) = (
                self.docks.get(&position),
                saved.get("docks").and_then(|docks| docks.get(name)),
            ) else {
                continue;
            };
            dock.update(cx, |dock, cx| {
                dock.dock_mut().restore(saved);
                cx.notify();
            });
        }
        self.focus_active(window, cx);
        self.follow_active(window, cx);
        let filled = restored.filled;
        let filling = cx.spawn_in(window, async move |workspace, cx| {
            let empty = filled.await;
            (workspace.update_in(cx, |workspace, window, cx| {
                if let Some(restoring) = workspace.restoring.take() {
                    restoring.filling.detach();
                }
                // Unless an item was opened in it meanwhile.
                for pane in &empty {
                    if pane.read(cx).pane().items().is_empty() {
                        workspace.close_pane(pane, window, cx);
                    }
                }
                workspace.focus_active(window, cx);
                workspace.follow_active(window, cx);
                workspace.save_layout(cx);
            }))
            .ok();
        });
        self.restoring = Some(Restoring {
            filling,
            saved: saved.clone(),
        });
        true
    }

    /// The entry to save in full: while a restore reads, the one it
    /// restores.
    fn full_entry(&self, cx: &App) -> Value {
        match &self.restoring {
            Some(restoring) => restoring.saved.clone(),
            None => self.entry(cx),
        }
    }

    /// Saves the entry if its shape changed, unless a restore is reading.
    fn save_layout(&mut self, cx: &mut App) {
        if self.layout.is_none() || self.restoring.is_some() {
            return;
        }
        let entry = self.entry(cx);
        let items: Vec<Vec<ItemId>> = (self.group.panes().into_iter())
            .map(|pane| {
                (pane.read(cx).pane().items().iter())
                    .map(|item| item.id())
                    .collect()
            })
            .collect();
        let Some((layout, _)) = &mut self.layout else {
            return;
        };
        if items != self.saved_items {
            self.saved_items = items;
            layout.save(entry, cx);
        } else {
            layout.save_if_changed(entry, cx);
        }
    }

    /// The window has closed: its entry is kept if no other window is open,
    /// else deleted.
    fn close_layout(&mut self, cx: &mut App) {
        if let Some((layout, ours)) = self.layout.take() {
            let entry = self.full_entry(cx);
            // The workspace can go before its window does.
            let others_open = (cx.windows().iter()).any(|window| window.window_id() != ours);
            layout.close(entry, others_open, cx);
        }
    }
}

impl Workspace<ItemPane> {
    /// A workspace of kiln's own panes, [`ItemPane`]s, starting with one
    /// empty: see [`Workspace::new`].
    pub fn item_panes(
        context: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let new_pane = |_: &mut Window, cx: &mut App| cx.new(|cx| ItemPane::new(ui_theme(cx), cx));
        let first = new_pane(window, cx);
        Self::new(context, first, new_pane, window, cx)
    }
}

impl<P: PaneView> EventEmitter<WorkspaceEvent<P>> for Workspace<P> {}

impl<P: PaneView> Render for Workspace<P> {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.shell.clone()
    }
}

impl<P: PaneView> Focusable for Workspace<P> {
    /// The active item's, or the active pane's while it's empty.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.active_focus(cx)
    }
}

fn active_item_id<P: PaneView>(pane: &Entity<P>, cx: &App) -> Option<ItemId> {
    pane.read(cx).pane().active_item().map(|item| item.id())
}

/// The status bar: the workspace's items over the active item's.
struct StatusBar<P: PaneView>(WeakEntity<Workspace<P>>);

impl<P: PaneView> Render for StatusBar<P> {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = (self.0.upgrade())
            .map(|workspace| workspace.read(cx).status_items(cx))
            .unwrap_or_default();
        status::status_bar(items, cx)
    }
}

/// The shell's centre: the workspace's panes, laid out by its group.
struct Panes<P: PaneView> {
    workspace: WeakEntity<Workspace<P>>,
    /// Focused only once the workspace has gone.
    focus: FocusHandle,
}

impl<P: PaneView> Render for Panes<P> {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(workspace) = self.workspace.upgrade() else {
            return div().into_any_element();
        };
        // Drawn in the workspace's context, which its dividers' drags
        // update.
        workspace.update(cx, |workspace, cx| {
            (workspace.group).render(&ui_theme(cx), |workspace| &mut workspace.group, cx)
        })
    }
}

impl<P: PaneView> Focusable for Panes<P> {
    /// The workspace's: where focus goes back to when a modal, such as the
    /// palette, is dismissed.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.workspace.upgrade() {
            Some(workspace) => workspace.read(cx).active_focus(cx),
            None => self.focus.clone(),
        }
    }
}
