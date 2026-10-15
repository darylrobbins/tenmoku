// SPDX-License-Identifier: LicenseRef-Proprietary

//! The conversations themselves, apart from the views that show them: each
//! a [`Thread`] entity, so two views of one (a split) show the same
//! messages, and the app's list of them, [`Threads`], a GPUI global that
//! the conversations panel observes.

use gpui::{App, AppContext as _, BorrowAppContext as _, Entity, Global, SharedString};
use kiln::command::Value;

/// A conversation: its title and the messages sent in it.
pub(crate) struct Thread {
    pub(crate) id: u64,
    pub(crate) title: SharedString,
    pub(crate) messages: Vec<SharedString>,
}

impl Thread {
    /// Sends `text`: views of the thread redraw, as they observe it.
    pub(crate) fn send(&mut self, text: impl Into<SharedString>, cx: &mut gpui::Context<Self>) {
        self.messages.push(text.into());
        cx.notify();
    }
}

/// Every conversation, in the order they were started.
#[derive(Default)]
pub(crate) struct Threads {
    list: Vec<Entity<Thread>>,
    next_id: u64,
}

impl Global for Threads {}

impl Threads {
    /// Starts a conversation titled `title`.
    pub(crate) fn add(
        title: impl Into<SharedString>,
        messages: Vec<SharedString>,
        cx: &mut App,
    ) -> Entity<Thread> {
        let id = cx.default_global::<Self>().next_id;
        Self::insert(id, title.into(), messages, cx)
    }

    fn insert(
        id: u64,
        title: SharedString,
        messages: Vec<SharedString>,
        cx: &mut App,
    ) -> Entity<Thread> {
        let thread = cx.new(|_| Thread {
            id,
            title,
            messages,
        });
        // `update_global` notifies those observing the list.
        cx.update_global::<Self, _>(|threads, _| {
            threads.next_id = threads.next_id.max(id + 1);
            threads.list.push(thread.clone());
        });
        thread
    }

    pub(crate) fn all(cx: &App) -> Vec<Entity<Thread>> {
        cx.try_global::<Self>()
            .map(|threads| threads.list.clone())
            .unwrap_or_default()
    }

    /// The conversation with `id`, if there is one.
    pub(crate) fn find(id: u64, cx: &App) -> Option<Entity<Thread>> {
        Self::all(cx)
            .into_iter()
            .find(|thread| thread.read(cx).id == id)
    }

    /// The conversations a saved layout kept ([`Threads::save`]), or, with
    /// none, two to start with.
    pub(crate) fn load(saved: Option<&Value>, cx: &mut App) {
        cx.set_global(Self::default());
        let saved = saved
            .and_then(Value::as_array)
            .filter(|list| !list.is_empty());
        let Some(saved) = saved else {
            Self::add("General", vec!["Welcome to Kiln Chat.".into()], cx);
            Self::add("Random", Vec::new(), cx);
            return;
        };
        for thread in saved {
            let (Some(id), Some(title)) = (thread["id"].as_u64(), thread["title"].as_str()) else {
                continue;
            };
            let messages = (thread["messages"].as_array().into_iter().flatten())
                .filter_map(|text| Some(SharedString::from(text.as_str()?.to_owned())))
                .collect();
            Self::insert(id, title.to_owned().into(), messages, cx);
        }
    }

    /// Every conversation, for the window's saved layout.
    pub(crate) fn save(cx: &App) -> Value {
        Value::from_iter(Self::all(cx).into_iter().map(|thread| {
            let thread = thread.read(cx);
            let messages = thread
                .messages
                .iter()
                .map(|text| Value::from(text.as_ref()));
            Value::from_iter([
                ("id", Value::from(thread.id)),
                ("title", Value::from(thread.title.as_ref())),
                ("messages", Value::from_iter(messages)),
            ])
        }))
    }
}
