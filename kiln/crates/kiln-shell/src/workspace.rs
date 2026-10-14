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
//! A host draws the workspace and adds its own around it, hearing
//! [`WorkspaceEvent`]s; [`Workspace::shell`] and [`Workspace::group_mut`]
//! reach the parts.

use std::{collections::HashMap, rc::Rc};

use gpui::{
    App, AppContext as _, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable,
    IntoElement, Render, SharedString, Subscription, WeakEntity, Window, div,
};
use kiln_platform::Capabilities;
use kiln_ui::ui_theme;

use crate::{
    Direction, ItemHandle, ItemId, ItemPane, MenuBarView, PaneEvent, PaneGroup, PaneView, Shell,
    close::{self, WindowItems},
    pane::{Peers, TabDropped},
};

/// Makes an empty pane, not yet in a group.
type NewPane<P> = Box<dyn Fn(&mut Window, &mut App) -> Entity<P>>;

/// What a workspace tells its host.
pub enum WorkspaceEvent<P = ItemPane> {
    /// The active pane, or its active item, is another one.
    ActiveChanged,
    /// A pane joined the group, split off by a command or a dropped tab.
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
        let workspace = cx.weak_entity();
        let centre = cx.new(|cx| Panes {
            workspace: workspace.clone(),
            focus: cx.focus_handle(),
        });
        let shell = cx.new(|cx| {
            let mut shell = Shell::new(context, ui_theme(cx), window, cx)
                .platform_chrome(Capabilities::current(), cx);
            shell.set_center(centre, cx);
            shell
        });
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
