// SPDX-License-Identifier: LicenseRef-Proprietary

//! The conversations panel: a dock panel listing every conversation. A
//! click (or the arrow keys) on one asks the window to open it.

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Window,
};
use kiln::{
    shell::{DockItem, PanelId},
    ui::{Icon, List, ListDelegate, ListEvent, ListRow, RowState},
};

use crate::threads::{Thread, Threads};

pub(crate) const PANEL: PanelId = PanelId("conversations");

/// What the panel asks of the window.
pub(crate) struct OpenConversation(pub(crate) Entity<Thread>);

pub(crate) struct ConversationList {
    list: Entity<List<Rows>>,
    _subscriptions: [Subscription; 2],
}

impl ConversationList {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list = cx.new(|cx| List::new(Rows(Threads::all(cx)), cx));
        let picked = cx.subscribe_in(&list, window, |_, list, event, _, cx| {
            let (ListEvent::Selected(ix) | ListEvent::Confirmed(ix)) = *event;
            let thread = list.read(cx).delegate().0.get(ix).cloned();
            if let Some(thread) = thread {
                cx.emit(OpenConversation(thread));
            }
        });
        // A conversation started anywhere joins the list.
        let started = cx.observe_global::<Threads>(|panel, cx| {
            panel.list.update(cx, |list, cx| {
                list.delegate_mut().0 = Threads::all(cx);
                list.rows_changed(cx);
            });
        });
        Self {
            list,
            _subscriptions: [picked, started],
        }
    }

    /// Selects `thread`'s row, without asking to open it: the window
    /// follows the conversation shown.
    pub(crate) fn select(&mut self, thread: &Entity<Thread>, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            let ix = list.delegate().0.iter().position(|row| row == thread);
            list.set_selected(ix, cx);
        });
    }

    #[allow(dead_code, reason = "for tests/host.rs")]
    pub(crate) fn list(&self) -> &Entity<List<Rows>> {
        &self.list
    }
}

impl EventEmitter<OpenConversation> for ConversationList {}

impl Focusable for ConversationList {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.list.focus_handle(cx)
    }
}

impl Render for ConversationList {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::div().size_full().child(self.list.clone())
    }
}

impl DockItem for ConversationList {
    fn id(&self) -> PanelId {
        PANEL
    }

    fn title(&self, _: &App) -> SharedString {
        "Conversations".into()
    }

    fn icon(&self, _: &App) -> Icon {
        Icon::ListTree
    }

    /// Its rail button's tooltip shows this command's key.
    fn show_command(&self) -> Option<&'static str> {
        Some("chat.toggle_conversations")
    }
}

/// The list's rows: every conversation's title.
pub(crate) struct Rows(Vec<Entity<Thread>>);

impl ListDelegate for Rows {
    fn count(&self) -> usize {
        self.0.len()
    }

    fn render_row(
        &mut self,
        ix: usize,
        state: RowState,
        _: &mut Window,
        cx: &mut Context<List<Self>>,
    ) -> AnyElement {
        let title = self.0[ix].read(cx).title.clone();
        ListRow::new(state).px_3().child(title).into_any_element()
    }
}
