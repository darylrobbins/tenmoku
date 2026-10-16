// SPDX-License-Identifier: LicenseRef-Proprietary

//! The conversations themselves, apart from the views that show them: the
//! app's list of [`Thread`]s, [`Threads`], a model kept in the state
//! directory; the panel listing them, [`ConversationList`], which opens
//! the one picked and selects the one the window shows; and the status
//! bar's count. A conversation's place in the list is its ID.

use kiln::gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, SharedString, Window,
    prelude::*,
};
use kiln::ui::{List, ListDelegate, ListEvent, ListRow, RowState};
use kiln::{Cx, View, WorkspaceExt as _, model, serde::*};

use crate::conversation::{Conversation, ConversationState};

/// A conversation: its title and the messages sent in it.
#[derive(Clone, Serialize, Deserialize)]
#[serde(crate = "kiln::serde")]
pub(crate) struct Thread {
    pub(crate) title: SharedString,
    pub(crate) messages: Vec<String>,
}

/// Every conversation, in the order they were started. None is removed.
#[kiln::model(persist)]
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(crate = "kiln::serde")]
pub(crate) struct Threads(pub(crate) Vec<Thread>);

impl Threads {
    pub(crate) fn get(ix: usize, cx: &App) -> Option<&Thread> {
        model::entity_if::<Self>(cx)?.read(cx).0.get(ix)
    }

    /// Starts a conversation titled `title`, giving its ID.
    pub(crate) fn add(title: &str, messages: &[&str], cx: &mut App) -> usize {
        let messages = messages.iter().map(ToString::to_string).collect();
        model::update(cx, |all: &mut Self, _| {
            all.0.push(Thread {
                title: title.into(),
                messages,
            });
            all.0.len() - 1
        })
    }

    pub(crate) fn save(cx: &App) -> Self {
        model::entity_if::<Self>(cx).map_or_else(Self::default, |all| all.read(cx).clone())
    }
}

/// `count` of `noun`: `1 message`, `2 messages`.
pub(crate) fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

/// How many conversations there are.
#[kiln::status(id = "chat.conversations")]
fn conversation_count(cx: &App) -> String {
    plural(
        model::entity_if::<Threads>(cx).map_or(0, |all| all.read(cx).0.len()),
        "conversation",
    )
}

/// The conversations panel, a list of titles.
#[kiln::panel(id = "chat.conversations", title = "Conversations", icon = ListTree, new = ConversationList::new)]
pub(crate) struct ConversationList(pub(crate) Entity<List<Rows>>);

impl ConversationList {
    fn new(cx: &mut Cx<Self>) -> Self {
        let list = cx.new(|cx| List::new(Rows(Vec::new()), cx));
        // What the list picks opens in the window's active pane.
        let (window, cx) = (&mut *cx.window, &mut *cx.cx);
        cx.subscribe_in(&list, window, |_, _, event, window, cx| {
            let (ListEvent::Selected(ix) | ListEvent::Confirmed(ix)) = *event;
            let workspace = kiln::with_cx(window, cx, |cx| cx.workspace());
            let state = ConversationState::new(ix);
            workspace.update(cx, |ws, cx| ws.open::<Conversation>(state, window, cx));
        })
        .detach();
        Self(list, Default::default())
    }
}

impl View for ConversationList {
    /// Lists every conversation, selecting the one the window shows.
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoElement {
        // Showing the panel focuses it: pass that on, so the arrow keys reach the list.
        if self.1.focus_handle(cx).is_focused(cx.window) {
            cx.window.focus(&self.0.focus_handle(cx), cx.cx);
        }
        let all = cx.watch::<Threads>().0.clone();
        let titles = all.into_iter().map(|t| t.title).collect();
        let shown = cx.workspace().read(cx).active_state::<Conversation>(cx);
        self.0.update(cx.cx, |list, cx| {
            if list.delegate().0 != titles {
                list.delegate_mut().0 = titles;
                list.rows_changed(cx);
            }
            let shown = shown.map(|state| state.thread);
            if shown.is_some() && list.selected() != shown {
                list.set_selected(shown, cx);
            }
        });
        self.0.clone()
    }
}

/// The list's rows: every conversation's title.
pub(crate) struct Rows(pub(crate) Vec<SharedString>);

impl ListDelegate for Rows {
    fn count(&self) -> usize {
        self.0.len()
    }

    fn render_row(
        &mut self,
        ix: usize,
        state: RowState,
        _: &mut Window,
        _: &mut Context<List<Self>>,
    ) -> AnyElement {
        let row = ListRow::new(state).px_3();
        row.child(self.0[ix].clone()).into_any_element()
    }
}
