//! The notification inbox: every toast a window shows is kept here, split
//! into *Needs you* (it has an action or unanswered choices that nobody has
//! acted on, answered or dismissed) and *Earlier* (everything else).
//!
//! It knows nothing of GPUI and holds no toast: its [`Notification`]s are
//! plain data, so a toast's question is answered, or answered with none, by
//! the queue as before. A toast timing out leaves its notification where it
//! is: an action not taken still needs you. At most [`CAP`] are kept.

#![allow(dead_code, reason = "the bell and popup read it later")]

use std::time::SystemTime;

use fe_command::CommandId;

use super::{Level, Toast, ToastId};

/// How many notifications the inbox keeps: past that, the oldest Earlier
/// one goes, and the oldest of all if none is Earlier.
pub(crate) const CAP: usize = 200;

/// Who raised a notification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum NotificationSource {
    #[default]
    App,
    Lsp,
    Task,
    Git,
    Agent,
}

/// A notification in an inbox, for as long as it's kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NotificationId(u64);

/// Where a notification stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    /// Not yet acted on, answered or dismissed.
    Open,
    /// Its action was taken.
    Acted,
    /// One of its choices was picked.
    Answered,
    /// Closed without either.
    Dismissed,
}

/// How a notification was dealt with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resolution {
    Acted,
    Answered,
    Dismissed,
}

/// One toast, kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Notification {
    pub id: NotificationId,
    pub level: Level,
    pub source: NotificationSource,
    pub title: String,
    pub detail: Option<String>,
    /// A button's label, and the command it runs.
    pub action: Option<(String, CommandId)>,
    /// Whether it asks a question, with a button per answer.
    pub choices: bool,
    /// When it was last raised: an identical toast merged into it moves it.
    pub at: SystemTime,
    /// How many times it was raised: 1, or more for identical toasts merged.
    pub count: usize,
    pub state: State,
    /// The queue's toast for it, while a merge may still find one.
    toast: Option<ToastId>,
}

impl Notification {
    /// Whether it waits on the user: it has an action or unanswered choices
    /// and is still open.
    pub(crate) fn needs_you(&self) -> bool {
        self.state == State::Open && (self.action.is_some() || self.choices)
    }
}

/// A window's notifications, oldest first.
#[derive(Debug, Default)]
pub(crate) struct NotificationInbox {
    items: Vec<Notification>,
    next_id: u64,
}

impl NotificationInbox {
    /// Records `toast`, raised at `at` and held by the queue as `queued`:
    /// `merged` if the queue counted it into a toast it already had, which
    /// counts that toast's notification up instead.
    pub(crate) fn record(
        &mut self,
        toast: &Toast,
        queued: ToastId,
        merged: bool,
        at: SystemTime,
    ) -> NotificationId {
        let existing = (self.items.iter())
            .position(|item| merged && item.toast == Some(queued) && item.state == State::Open);
        if let Some(at_index) = existing {
            // Raised again: it's the newest now, in order and for the cap.
            let mut item = self.items.remove(at_index);
            item.count += 1;
            item.at = at;
            let id = item.id;
            self.items.push(item);
            return id;
        }
        let id = NotificationId(self.next_id);
        self.next_id += 1;
        self.items.push(Notification {
            id,
            level: toast.level,
            source: toast.source.unwrap_or_default(),
            title: toast.message.clone(),
            detail: toast.detail.clone(),
            action: toast.action.clone(),
            choices: toast.choices.as_ref().is_some_and(|c| !c.titles.is_empty()),
            at,
            count: 1,
            state: State::Open,
            toast: Some(queued),
        });
        self.trim();
        id
    }

    /// Every notification, oldest first.
    pub(crate) fn all(&self) -> &[Notification] {
        &self.items
    }

    /// The notifications that need the user, oldest first.
    pub(crate) fn needs_you(&self) -> impl Iterator<Item = &Notification> {
        self.items.iter().filter(|item| item.needs_you())
    }

    /// The rest, oldest first.
    pub(crate) fn earlier(&self) -> impl Iterator<Item = &Notification> {
        self.items.iter().filter(|item| !item.needs_you())
    }

    /// How many need the user, for the bell.
    pub(crate) fn needs_you_count(&self) -> usize {
        self.needs_you().count()
    }

    /// Resolves notification `id`, moving it to Earlier. False if it's gone
    /// or was resolved already.
    pub(crate) fn resolve(&mut self, id: NotificationId, how: Resolution) -> bool {
        (self.items.iter_mut())
            .find(|item| item.id == id)
            .is_some_and(|item| item.resolve(how))
    }

    /// Resolves the notification of queue toast `toast`, which is the one
    /// being acted on, answered or dismissed there.
    pub(crate) fn resolve_toast(&mut self, toast: ToastId, how: Resolution) -> bool {
        (self.items.iter_mut())
            .find(|item| item.toast == Some(toast))
            .is_some_and(|item| item.resolve(how))
    }

    /// Drops the oldest Earlier notification while over [`CAP`], else the
    /// oldest.
    fn trim(&mut self) {
        while self.items.len() > CAP {
            let at = (self.items.iter())
                .position(|item| !item.needs_you())
                .unwrap_or(0);
            self.items.remove(at);
        }
    }
}

impl Notification {
    fn resolve(&mut self, how: Resolution) -> bool {
        if self.state != State::Open {
            return false;
        }
        self.state = match how {
            Resolution::Acted => State::Acted,
            Resolution::Answered => State::Answered,
            Resolution::Dismissed => State::Dismissed,
        };
        true
    }
}
