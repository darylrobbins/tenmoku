// SPDX-License-Identifier: LicenseRef-Proprietary

//! The chat's window. Its view, [`ChatWindow`], draws kiln-shell's
//! [`Shell`] (the chrome, the dock, the status bar and the modal layer,
//! under the chat's root key context) and handles the window's commands.
//! In the shell's centre is a [`PaneGroup`] of kiln's [`ItemPane`]s, whose
//! tabs are [`Conversation`]s; on its left, a dock with the conversations
//! panel; below, the status bar: the host's item (how many conversations)
//! and the active conversation's (how many messages).

use std::{collections::HashMap, rc::Rc};

use gpui::{
    App, AppContext as _, Context, Entity, EntityId, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, Subscription,
    WeakEntity, Window, div, px,
};
use kiln::{
    command::Value,
    platform::Capabilities,
    prelude::*,
    shell::{
        Direction, Dock, DockPosition, DockState, DockView, ItemHandle, ItemPane, PaneEvent,
        PaneGroup, PaneItem, PaneView, Shell, StatusItem, StatusItems, StatusSide,
        close::{self, WindowItems},
        dock::DockEvent,
        layout::{self, WindowLayout},
        pane::{Peers, TabDropped},
        status,
    },
    ui::{Appearance, ThemeMode, ui_theme},
};

use crate::{
    commands::{
        CloseConversation, NewConversation, SplitDown, SplitRight, ToggleConversations, ToggleTheme,
    },
    conversation::Conversation,
    panel::{ConversationList, OpenConversation},
    setup::APP_NAME,
    threads::{Thread, Threads},
};

/// The chat's root key context, which the shell sets.
pub(crate) const KEY_CONTEXT: &str = "chat";

pub(crate) struct ChatWindow {
    shell: Entity<Shell>,
    group: PaneGroup<ItemPane>,
    /// The pane conversations open in, and whose tab the status bar and
    /// the window's title follow.
    active: Entity<ItemPane>,
    /// The group's panes, which each of them sees.
    peers: Peers<ItemPane>,
    dock: Entity<Dock>,
    panel: Entity<ConversationList>,
    status_bar: Entity<StatusBar>,
    /// The host's own status items, over the active conversation's.
    status: StatusItems,
    /// Where the window's layout is saved, once `main` gives it one.
    layout: Option<WindowLayout>,
    /// Per pane: its notifications, its activation and tabs dropped on it.
    panes: HashMap<EntityId, [Subscription; 3]>,
    _subscriptions: Vec<Subscription>,
}

impl ChatWindow {
    /// The window, its chrome as on a platform with `capabilities`, with an
    /// empty pane: [`ChatWindow::start`] fills it.
    pub(crate) fn new(
        capabilities: Capabilities,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let ui = ui_theme(cx);
        let pane = cx.new(|cx| ItemPane::new(ui, cx));
        let panel = cx.new(|cx| ConversationList::new(window, cx));
        let dock = cx.new(|cx| {
            // Its rail's tooltip finds the key that shows the panel in the
            // chat's context.
            let state = DockState::new(DockPosition::Left, ui)
                .size(px(200.))
                .shortcut_contexts([KEY_CONTEXT]);
            let mut dock = Dock::with_state(state);
            dock.add_panel(panel.clone(), cx);
            dock
        });
        let chat = cx.weak_entity();
        let status_bar = cx.new(|_| StatusBar(chat.clone()));
        let centre = cx.new(|cx| Panes {
            chat: chat.clone(),
            focus: cx.focus_handle(),
        });
        let shell = cx.new(|cx| {
            let mut shell = Shell::new(KEY_CONTEXT, ui, window, cx)
                .platform_chrome(capabilities, cx)
                .text_size(px(13.));
            shell.set_title(APP_NAME, cx);
            shell.set_center(centre, cx);
            shell.set_dock(DockPosition::Left, Some(dock.clone().into()), cx);
            shell.set_status_bar(Some(status_bar.clone().into()), cx);
            shell
        });
        // Closing the window and quitting ask its conversations first.
        let items: WindowItems = Rc::new(move |cx| {
            (chat.upgrade())
                .map(|chat| chat.read(cx).group.items(cx))
                .unwrap_or_default()
        });
        close::set_window_items(window, items, cx);
        let subscriptions = vec![
            cx.subscribe_in(
                &panel,
                window,
                |chat, _, open: &OpenConversation, window, cx| {
                    chat.open(open.0.clone(), window, cx)
                },
            ),
            // A rail click hid the panel that had focus.
            cx.subscribe_in(&dock, window, |chat, _, _: &DockEvent, window, cx| {
                chat.focus_active(window, cx)
            }),
            cx.observe_global::<Threads>(Self::count_conversations),
        ];
        let mut chat = Self {
            shell,
            group: PaneGroup::new(pane.clone()),
            active: pane.clone(),
            peers: Peers::default(),
            dock,
            panel,
            status_bar,
            status: StatusItems::default(),
            layout: None,
            panes: HashMap::new(),
            _subscriptions: subscriptions,
        };
        chat.watch(&pane, window, cx);
        chat.count_conversations(cx);
        chat
    }

    /// Restores the layout `saved`, if it has panes, else opens the first
    /// conversation.
    pub(crate) fn start(
        &mut self,
        saved: Option<&Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let restored = saved.is_some_and(|saved| self.restore(saved, window, cx));
        if !restored && let Some(first) = Threads::all(cx).first().cloned() {
            self.open(first, window, cx);
        }
        self.follow_active(window, cx);
    }

    #[allow(dead_code, reason = "for tests/host.rs")]
    pub(crate) fn shell(&self) -> &Entity<Shell> {
        &self.shell
    }

    #[allow(dead_code, reason = "for tests/host.rs")]
    pub(crate) fn group(&self) -> &PaneGroup<ItemPane> {
        &self.group
    }

    #[allow(dead_code, reason = "for tests/host.rs")]
    pub(crate) fn active_pane(&self) -> &Entity<ItemPane> {
        &self.active
    }

    #[allow(dead_code, reason = "for tests/host.rs")]
    pub(crate) fn panel(&self) -> &Entity<ConversationList> {
        &self.panel
    }

    /// The active pane's active conversation, if it has one.
    pub(crate) fn active_conversation(&self, cx: &App) -> Option<Entity<Conversation>> {
        self.active.read(cx).pane().active_item()?.downcast()
    }

    /// The status bar's items: the host's, over the active conversation's.
    pub(crate) fn status_items(&self, cx: &App) -> Vec<StatusItem> {
        (self.status).items_for(self.active.read(cx).pane().active_item(), cx)
    }

    /// Shows `thread` in the active pane: its tab there if it has one,
    /// else a new one.
    pub(crate) fn open(
        &mut self,
        thread: Entity<Thread>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.active.clone();
        let shown = (pane.read(cx).pane().items().iter()).position(|item| {
            item.downcast::<Conversation>()
                .is_some_and(|item| *item.read(cx).thread() == thread)
        });
        pane.update(cx, |pane, cx| match shown {
            Some(ix) => {
                pane.activate_item(ix, window, cx);
                cx.notify();
            }
            None => {
                let item = cx.new(|cx| Conversation::new(thread, window, cx));
                pane.insert_item(Box::new(item), None, window, cx);
            }
        });
    }

    fn new_conversation(
        &mut self,
        _: &NewConversation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = format!("Conversation {}", Threads::all(cx).len() + 1);
        let thread = Threads::add(title, Vec::new(), cx);
        self.open(thread, window, cx);
    }

    /// Closes the active conversation, once it agrees ([`close`]); an
    /// empty pane closes itself, unless it's the last.
    fn close_conversation(
        &mut self,
        _: &CloseConversation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.active.clone();
        if pane.read(cx).pane().items().is_empty() {
            self.close_pane(&pane, window, cx);
        } else {
            pane.update(cx, |pane, cx| pane.close_active_item(window, cx));
        }
    }

    fn split_right(&mut self, _: &SplitRight, window: &mut Window, cx: &mut Context<Self>) {
        self.split(Direction::Right, window, cx);
    }

    fn split_down(&mut self, _: &SplitDown, window: &mut Window, cx: &mut Context<Self>) {
        self.split(Direction::Down, window, cx);
    }

    /// Splits a new pane off the active one, showing the active
    /// conversation in a second view, and makes it the active pane.
    fn split(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let old = self.active.clone();
        let item = old
            .read(cx)
            .pane()
            .active_item()
            .map(ItemHandle::boxed_clone);
        let copy = item.and_then(|item| item.split(window, cx));
        let new = old.update(cx, |pane, cx| cx.new(|cx| pane.new_peer(cx)));
        self.group.split(&old, new.clone(), direction);
        self.watch(&new, window, cx);
        self.active = new.clone();
        match copy {
            Some(copy) => new.update(cx, |pane, cx| pane.insert_item(copy, None, window, cx)),
            None => window.focus(&new.focus_handle(cx), cx),
        }
        self.follow_active(window, cx);
    }

    fn toggle_conversations(
        &mut self,
        _: &ToggleConversations,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dock.update(cx, |dock, cx| dock.toggle(cx));
        // Focus doesn't stay in a panel that's hidden.
        if self.dock.read(cx).dock().hidden_panel_focused(window, cx) {
            self.focus_active(window, cx);
        }
        self.save_layout(cx);
    }

    fn toggle_theme(&mut self, _: &ToggleTheme, _: &mut Window, cx: &mut Context<Self>) {
        let dark = cx.theme().appearance == Appearance::Dark;
        cx.set_theme_mode(if dark {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        });
    }

    /// Joins `pane`, already in the group, to the window's panes: they all
    /// see it, and the window follows its changes and its clicks.
    fn watch(&mut self, pane: &Entity<ItemPane>, window: &mut Window, cx: &mut Context<Self>) {
        let peers = self.peers.clone();
        pane.update(cx, |pane, _| pane.pane_mut().set_peers(peers));
        let changed = cx.observe_in(pane, window, Self::pane_changed);
        let activated = cx.subscribe_in(pane, window, |chat, pane, _: &PaneEvent, window, cx| {
            if chat.active != *pane {
                chat.active = pane.clone();
                chat.follow_active(window, cx);
            }
        });
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

    /// A pane, or a conversation in it, changed: a pane its last tab left
    /// closes, unless it's the last one.
    fn pane_changed(
        &mut self,
        pane: Entity<ItemPane>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let emptied = pane.update(cx, |pane, _| pane.pane_mut().take_emptied());
        if emptied && self.group.panes().len() > 1 {
            self.close_pane(&pane, window, cx);
        }
        self.follow_active(window, cx);
    }

    /// A tab was dropped on a pane, or on an edge of one to split it.
    fn tab_dropped(
        &mut self,
        _: &Entity<ItemPane>,
        dropped: &TabDropped<ItemPane>,
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
            }
            self.active = pane;
            self.follow_active(window, cx);
        }
    }

    /// Closes `pane`, unless it's the last, making the nearest pane active
    /// if it was.
    fn close_pane(&mut self, pane: &Entity<ItemPane>, window: &mut Window, cx: &mut Context<Self>) {
        if self.group.panes().len() < 2 {
            return;
        }
        let nearest = self.group.nearest(pane, cx);
        self.group.remove(pane);
        self.panes.remove(&pane.entity_id());
        self.sync_peers();
        if self.active == *pane {
            self.active = nearest.unwrap_or_else(|| self.group.panes()[0].clone());
            self.focus_active(window, cx);
        }
        self.follow_active(window, cx);
    }

    /// What has focus when the active pane does: its conversation's field,
    /// or the pane itself while it's empty.
    fn active_focus(&self, cx: &App) -> FocusHandle {
        match self.active.read(cx).pane().active_item() {
            Some(item) => item.item_focus_handle(cx),
            None => self.active.focus_handle(cx),
        }
    }

    pub(crate) fn focus_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.active_focus(cx), cx);
    }

    /// The window's title, the status bar, the panel's selection and the
    /// saved layout follow the active conversation.
    fn follow_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let conversation = self.active_conversation(cx);
        let title = match &conversation {
            Some(conversation) => conversation.read(cx).window_title(cx),
            None => APP_NAME.into(),
        };
        window.set_window_title(&title);
        self.shell
            .update(cx, |shell, cx| shell.set_title(title, cx));
        if let Some(conversation) = conversation {
            let thread = conversation.read(cx).thread().clone();
            self.panel.update(cx, |panel, cx| panel.select(&thread, cx));
        }
        self.status_bar.update(cx, |_, cx| cx.notify());
        self.save_layout(cx);
        cx.notify();
    }

    /// The host's status item: how many conversations there are.
    fn count_conversations(&mut self, cx: &mut Context<Self>) {
        let text = match Threads::all(cx).len() {
            1 => "1 conversation".to_owned(),
            count => format!("{count} conversations"),
        };
        let item = StatusItem::new("chat.conversations", StatusSide::Left, 0, text);
        self.status.set(item);
        self.status_bar.update(cx, |_, cx| cx.notify());
    }

    // Saved layouts.

    /// Saves the window's layout as its shape changes, and in full as the
    /// app quits; as the window closes, the layout is kept if it's the last
    /// window ([`layout`]).
    #[allow(dead_code, reason = "main's")]
    pub(crate) fn set_layout(
        &mut self,
        layout: WindowLayout,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.layout = Some(layout);
        let at_quit = cx.on_app_quit(|chat, cx| {
            let entry = chat.entry(cx);
            let saved = (chat.layout.as_mut()).map(|layout| layout.save_at_quit(entry, cx));
            async move {
                if let Some(saved) = saved {
                    saved.await;
                }
            }
        });
        let ours = window.window_handle().window_id();
        let chat = cx.weak_entity();
        let closed = cx.on_window_closed(move |cx, closed| {
            if closed == ours {
                chat.update(cx, |chat, cx| chat.close_layout(cx)).ok();
            }
        });
        self._subscriptions.extend([at_quit, closed]);
        self.save_layout(cx);
    }

    /// The window's saved entry: its panes, its dock, and the
    /// conversations themselves.
    pub(crate) fn entry(&self, cx: &App) -> Value {
        Value::from_iter([
            ("panes", layout::save_panes(&self.group, &self.active, cx)),
            ("dock", self.dock.read(cx).dock().save()),
            ("threads", Threads::save(cx)),
        ])
    }

    fn save_layout(&mut self, cx: &mut Context<Self>) {
        let entry = self.entry(cx);
        if let Some(layout) = &mut self.layout {
            layout.save_if_changed(entry, cx);
        }
    }

    #[allow(dead_code, reason = "main's")]
    fn close_layout(&mut self, cx: &mut App) {
        if let Some(layout) = self.layout.take() {
            let entry = self.entry(cx);
            let others_open = !cx.windows().is_empty();
            layout.close(entry, others_open, cx);
        }
    }

    /// Puts back the panes and dock of `saved`; false if it has no panes.
    fn restore(&mut self, saved: &Value, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let ui = ui_theme(cx);
        let mut new_pane = |_: &mut Window, cx: &mut App| cx.new(|cx| ItemPane::new(ui, cx));
        let Some(restored) = layout::restore_panes(&saved["panes"], &mut new_pane, window, cx)
        else {
            return false;
        };
        self.panes.clear();
        self.group.set_root(restored.root);
        self.active = restored.active;
        for pane in self.group.panes().into_iter().cloned().collect::<Vec<_>>() {
            self.watch(&pane, window, cx);
        }
        (self.dock).update(cx, |dock, cx| {
            dock.dock_mut().restore(&saved["dock"]);
            cx.notify();
        });
        // The conversations land later: the panes left empty then close.
        let filled = restored.filled;
        cx.spawn_in(window, async move |chat, cx| {
            let empty = filled.await;
            chat.update_in(cx, |chat, window, cx| {
                for pane in &empty {
                    chat.close_pane(pane, window, cx);
                }
                chat.focus_active(window, cx);
            })
            .ok();
        })
        .detach();
        true
    }
}

impl Render for ChatWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The window's commands, wherever focus is in it: in a
        // conversation, the panel, or the palette's caller.
        div()
            .size_full()
            .on_action(cx.listener(Self::new_conversation))
            .on_action(cx.listener(Self::close_conversation))
            .on_action(cx.listener(Self::split_right))
            .on_action(cx.listener(Self::split_down))
            .on_action(cx.listener(Self::toggle_conversations))
            .on_action(cx.listener(Self::toggle_theme))
            .child(self.shell.clone())
    }
}

/// The shell's centre: the window's panes, laid out by its group.
struct Panes {
    chat: WeakEntity<ChatWindow>,
    /// Focused only once the window's view has gone.
    focus: FocusHandle,
}

impl Render for Panes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(chat) = self.chat.upgrade() else {
            return div().into_any_element();
        };
        // Drawn in the window's context, which its dividers' drags update.
        chat.update(cx, |chat, cx| {
            (chat.group).render(&ui_theme(cx), |chat| &mut chat.group, cx)
        })
    }
}

impl Focusable for Panes {
    /// The active conversation's: where focus goes back to when a modal,
    /// such as the palette, is dismissed.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.chat.upgrade() {
            Some(chat) => chat.read(cx).active_focus(cx),
            None => self.focus.clone(),
        }
    }
}

/// The status bar: the window's items, drawn by kiln-shell's
/// [`status::status_bar`].
struct StatusBar(WeakEntity<ChatWindow>);

impl Render for StatusBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = (self.0.upgrade())
            .map(|chat| chat.read(cx).status_items(cx))
            .unwrap_or_default();
        status::status_bar(items, cx)
    }
}
