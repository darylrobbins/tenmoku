// SPDX-License-Identifier: LicenseRef-Proprietary

//! The conversations themselves, apart from the views that show them: each
//! a [`Thread`] entity, so two views of one (a split) show the same
//! messages, and the app's list of them, [`Threads`], a GPUI global; and
//! the dock panel listing them, [`ConversationList`]. The window opens the
//! one a click (or the arrow keys) picks in its list. The window's saved layout keeps them
//! as a list of `Thread`s. None is ever removed, so a conversation's place
//! in the list is its ID (as the `id` layouts saved before `kiln::app`
//! hold, which is ignored).

use gpui::{AnyElement, App, Entity, FocusHandle, Focusable, Global, SharedString, prelude::*};
use gpui::{Window, div};
use kiln::serde::{Deserialize, Serialize};
use kiln::shell::{DockItem, PanelId, StatusItem, StatusSide};
use kiln::ui::{Icon, List, ListDelegate, ListRow, RowState};

/// A conversation: its title and the messages sent in it.
#[derive(Clone, Serialize, Deserialize)]
#[serde(crate = "kiln::serde")]
pub(crate) struct Thread {
    pub(crate) title: SharedString,
    pub(crate) messages: Vec<SharedString>,
}

/// Every conversation, in the order they were started.
#[derive(Default)]
pub(crate) struct Threads(Vec<Entity<Thread>>);

impl Global for Threads {}

impl Threads {
    /// Starts a conversation titled `title`.
    pub(crate) fn add(title: &str, messages: Vec<SharedString>, cx: &mut App) -> Entity<Thread> {
        let title = title.to_owned().into();
        let thread = cx.new(|_| Thread { title, messages });
        // `default_global` notifies those observing the list.
        cx.default_global::<Self>().0.push(thread.clone());
        thread
    }

    pub(crate) fn all(cx: &App) -> Vec<Entity<Thread>> {
        cx.try_global::<Self>().map_or(vec![], |all| all.0.clone())
    }

    /// The conversations a saved layout kept ([`Threads::save`]) past
    /// those there are: every window's entry holds the app's whole list, so
    /// one restored before, or this run's own, is already the list, and its
    /// conversations stay as they are.
    pub(crate) fn restore(saved: Vec<Thread>, cx: &mut App) {
        for thread in saved.into_iter().skip(Self::all(cx).len()) {
            Self::add(&thread.title, thread.messages, cx);
        }
    }

    /// The first conversation, starting two if there are none yet.
    pub(crate) fn first(cx: &mut App) -> Entity<Thread> {
        if Self::all(cx).is_empty() {
            Self::add("General", vec!["Welcome to Kiln Chat.".into()], cx);
            Self::add("Random", Vec::new(), cx);
        }
        Self::all(cx)[0].clone()
    }

    /// Every conversation, for the window's saved layout.
    pub(crate) fn save(cx: &App) -> Vec<Thread> {
        let all = Self::all(cx).into_iter();
        all.map(|thread| thread.read(cx).clone()).collect()
    }

    /// The status bar's item: how many conversations there are.
    pub(crate) fn count(cx: &App) -> Option<StatusItem> {
        let text = plural(cx.try_global::<Self>()?.0.len(), "conversation");
        let item = StatusItem::new("chat.conversations", StatusSide::Left, 0, text);
        Some(item)
    }
}

/// `count` of `noun`: `1 message`, `2 messages`.
pub(crate) fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

/// The conversations panel.
pub(crate) struct ConversationList(pub(crate) Entity<List<Rows>>);

impl ConversationList {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let list = cx.new(|cx| List::new(Rows(Threads::all(cx)), cx));
        // A conversation started anywhere joins the list.
        cx.observe_global::<Threads>(|panel, cx| {
            panel.0.update(cx, |list, cx| {
                list.delegate_mut().0 = Threads::all(cx);
                list.rows_changed(cx);
            });
        })
        .detach();
        Self(list)
    }
}

impl Focusable for ConversationList {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.0.focus_handle(cx)
    }
}

impl Render for ConversationList {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

impl DockItem for ConversationList {
    fn id(&self) -> PanelId {
        PanelId("conversations")
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
pub(crate) struct Rows(pub(crate) Vec<Entity<Thread>>);

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
