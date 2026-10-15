// SPDX-License-Identifier: LicenseRef-Proprietary

//! What the chat does in its window, a kiln [`Workspace`] whose tabs are
//! [`Conversation`]s: the window's commands, opening a conversation from
//! the panel or a new one, keeping the conversations in the saved layout,
//! and the panel's selection following the conversation shown. The panes,
//! docks, status bar and saved layout are the workspace's.

use gpui::{Action, App, AppContext as _, Context, Entity, Window};
use kiln::shell::{Direction, DockPosition, DockView as _, ItemHandle, PaneView as _};
use kiln::shell::{Workspace, WorkspaceEvent};
use kiln::ui::{Appearance, ListEvent, ThemeMode as Mode};
use kiln::{platform::command_actions::dispatch_window, prelude::*};

use crate::{conversation::Conversation, threads::*};

// The chat's commands: typed GPUI actions with a title, whose kiln IDs
// are `chat.new_conversation` and so on (`NewConversation::id()`).
// `keymap.toml` and `menus.toml` name them by those IDs, and the command
// palette lists them. Sending is the conversation view's, so it applies
// only where a conversation has focus; the rest are the window's.
kiln::commands!(
    chat,
    [
        /// New Conversation
        NewConversation,
        /// Send Message
        #[command(context = "conversation")]
        SendMessage,
        /// Close Conversation
        CloseConversation,
        /// Split Right
        SplitRight,
        /// Split Down
        SplitDown,
        /// Toggle Conversations
        ToggleConversations,
        /// Toggle Theme
        ToggleTheme,
    ]
);

/// The chat's root key context, where the window's commands apply.
pub(crate) const KEY_CONTEXT: &str = "chat";

/// Registers the chat's commands, the window's handled app-wide, and
/// starts the conversations, as kiln sets the app up.
pub(crate) fn init(cx: &mut App) {
    register(cx);
    on::<NewConversation>(cx, |workspace, window, cx| {
        let title = format!("Conversation {}", Threads::all(cx).len() + 1);
        let thread = Threads::add(&title, Vec::new(), cx);
        open(workspace, thread, window, cx);
    });
    // An empty pane closes itself, unless it's the last; else its active
    // conversation closes, once it agrees.
    on::<CloseConversation>(cx, |workspace, window, cx| {
        let pane = workspace.active_pane().clone();
        if pane.read(cx).pane().items().is_empty() {
            workspace.close_pane(&pane, window, cx);
        } else {
            pane.update(cx, |pane, cx| pane.close_active_item(window, cx));
        }
    });
    on::<SplitRight>(cx, |ws, w, cx| _ = ws.split(Direction::Right, w, cx));
    on::<SplitDown>(cx, |ws, w, cx| _ = ws.split(Direction::Down, w, cx));
    on::<ToggleConversations>(cx, |ws, w, cx| ws.toggle_dock(DockPosition::Left, w, cx));
    cx.on_action(|_: &ToggleTheme, cx| {
        let dark = cx.theme().appearance == Appearance::Dark;
        cx.set_theme_mode(if dark { Mode::Light } else { Mode::Dark });
    });
}

/// The workspace's context, for the window's commands.
type Cx<'a> = Context<'a, Workspace>;

/// Runs `run` for `A` on the workspace of the window `A` was dispatched in,
/// else the active one, once the dispatch is over: GPUI has the window out
/// until then.
fn on<A: Action>(cx: &mut App, run: fn(&mut Workspace, &mut Window, &mut Cx)) {
    cx.on_action(move |_: &A, cx| {
        let window = dispatch_window(cx).or_else(|| cx.active_window());
        if let Some(window) = window.and_then(|window| window.downcast::<Workspace>()) {
            cx.defer(move |cx| _ = window.update(cx, run));
        }
    });
}

/// Sets up each window, as `kiln::app`'s workspace hook: keeps the
/// conversations in its saved layout, opens the first unless a restored
/// layout's are coming, and joins the panel, if it has one, to the window:
/// what the panel picks opens, and the panel selects what the window shows.
pub(crate) fn setup(workspace: &mut Workspace, window: &mut Window, cx: &mut Cx) {
    workspace.persist("threads", Threads::save, |s, _, cx| Threads::restore(s, cx));
    let dock = workspace.dock(DockPosition::Left).cloned();
    let panel = (dock.as_ref()).and_then(|dock| dock.read(cx).panel::<ConversationList>());
    // Layouts saved before `kiln::app` kept the panel's dock as `dock`.
    workspace.on_restore(move |saved, _, cx| {
        if let (Some(dock), Some(saved)) = (&dock, saved.get("dock")) {
            dock.update(cx, |dock, _| dock.dock_mut().restore(saved));
        }
    });
    // Once the restore, if any, has started.
    cx.defer_in(window, |workspace, window, cx| {
        let first = Threads::first(cx);
        if !workspace.restoring() {
            open(workspace, first, window, cx);
        }
    });
    let list = panel.map(|panel| panel.read(cx).0.clone());
    let Some(list) = list else { return };
    cx.subscribe_in(&list, window, |workspace, list, event, window, cx| {
        let (ListEvent::Selected(ix) | ListEvent::Confirmed(ix)) = *event;
        if let Some(thread) = list.read(cx).delegate().0.get(ix).cloned() {
            open(workspace, thread, window, cx);
        }
    })
    .detach();
    // Selects the row without asking to open it.
    cx.subscribe_self(move |workspace, event: &WorkspaceEvent, cx| {
        let item = workspace.active_item(cx);
        let shown = item.and_then(|item| item.downcast_ref::<Conversation>());
        let thread = shown.map(|conversation| conversation.read(cx).thread.clone());
        if let (WorkspaceEvent::ActiveChanged, Some(thread)) = (event, thread) {
            list.update(cx, |list, cx| {
                let ix = list.delegate().0.iter().position(|row| *row == thread);
                list.set_selected(ix, cx);
            });
        }
    })
    .detach();
}

/// Shows `thread` in the active pane: its tab there if it has one, else a
/// new one.
pub(crate) fn open(workspace: &mut Workspace, thread: Entity<Thread>, w: &mut Window, cx: &mut Cx) {
    let shows = |item: &dyn ItemHandle, cx: &App| {
        (item.downcast_ref::<Conversation>())
            .is_some_and(|conversation| conversation.read(cx).thread == thread)
    };
    if !workspace.activate_item_where(shows, w, cx) {
        let item = cx.new(|cx| Conversation::new(thread, w, cx));
        workspace.add_item(Box::new(item), w, cx);
    }
}
