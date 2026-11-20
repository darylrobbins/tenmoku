//! The notification inbox: every toast a window shows is kept here, split
//! into *Needs you* (it has an action or unanswered choices that nobody has
//! acted on, answered or dismissed) and *Earlier* (everything else).
//!
//! It knows nothing of GPUI and holds no toast: its [`Notification`]s are
//! plain data, so a toast's question is answered, or answered with none, by
//! the queue as before. A toast timing out leaves its notification where it
//! is: an action not taken still needs you. At most [`CAP`] are kept.

use std::time::{Duration, SystemTime};

use fe_command::CommandId;
use fe_config::NotificationLevel;

use super::{Level, Toast, ToastId};

/// How many notifications the inbox keeps: past that, the oldest Earlier
/// one goes, and the oldest of all if none is Earlier.
pub(crate) const CAP: usize = 200;

/// How long there must be no key or mouse input before Focus mode lets the
/// toasts it holds show.
pub(crate) const FOCUS_IDLE: Duration = Duration::from_secs(20);

/// What becomes of a toast when it's raised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Disposition {
    /// It shows as a toast.
    Show,
    /// Focus mode keeps it in the inbox until the user is idle, then it
    /// shows.
    Hold,
    /// It goes to the inbox alone.
    Inbox,
}

/// Whether `toast` is urgent: it has an action or a question, or is an
/// error. Focus mode never holds one.
pub(crate) fn urgent(toast: &Toast) -> bool {
    toast.action.is_some()
        || toast.choices.as_ref().is_some_and(|c| !c.titles.is_empty())
        || toast.level == Level::Error
}

/// What `workbench.notifications` (`level`) and Focus mode (`focus`) make of
/// `toast`: `all` shows every one, `needs_you` those that are urgent and
/// `off` only errors, the rest going to the inbox alone; with Focus mode on,
/// a toast that would show but isn't urgent is held.
pub(crate) fn disposition(level: NotificationLevel, focus: bool, toast: &Toast) -> Disposition {
    match level {
        NotificationLevel::Off if toast.level != Level::Error => Disposition::Inbox,
        NotificationLevel::NeedsYou if !urgent(toast) => Disposition::Inbox,
        _ if focus && !urgent(toast) => Disposition::Hold,
        _ => Disposition::Show,
    }
}

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

impl NotificationId {
    /// The id as a number, distinct among a window's notifications: for
    /// element ids that follow a notification wherever its row moves.
    pub(crate) fn value(self) -> u64 {
        self.0
    }
}

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
    /// The arguments its action's command runs with.
    pub action_args: Option<serde_json::Value>,
    /// Whether it asks a question, with a button per answer.
    pub choices: bool,
    /// The question's answers, each a button's title: empty for none.
    pub answers: Vec<String>,
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
        self.insert(toast, Some(queued), at)
    }

    /// Keeps `toast` as a new notification.
    fn insert(&mut self, toast: &Toast, queued: Option<ToastId>, at: SystemTime) -> NotificationId {
        let id = NotificationId(self.next_id);
        self.next_id += 1;
        self.items.push(Notification {
            id,
            level: toast.level,
            source: toast.source.unwrap_or_default(),
            title: toast.message.clone(),
            detail: toast.detail.clone(),
            action: toast.action.clone(),
            action_args: toast.action_args.clone(),
            choices: toast.choices.as_ref().is_some_and(|c| !c.titles.is_empty()),
            answers: (toast.choices.as_ref()).map_or_else(Vec::new, |c| c.titles.clone()),
            at,
            count: 1,
            state: State::Open,
            toast: queued,
        });
        self.trim();
        id
    }

    /// Records `toast`, raised at `at`, which the queue doesn't hold: for the
    /// inbox alone, or held until it's shown ([`NotificationInbox::attach`]).
    pub(crate) fn record_unqueued(&mut self, toast: &Toast, at: SystemTime) -> NotificationId {
        self.insert(toast, None, at)
    }

    /// Notes that the queue now holds `toast` for notification `id`, a held
    /// one shown at last.
    pub(crate) fn attach(&mut self, id: NotificationId, toast: ToastId) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.toast = Some(toast);
        }
    }

    /// Notes that the queue counted held notification `id`'s toast into
    /// `toast`, which it already had: `id` goes, and the notification of
    /// `toast` counts up as a toast raised again does. Returns the notification
    /// that now stands for `id`'s.
    pub(crate) fn merge_held(
        &mut self,
        id: NotificationId,
        toast: ToastId,
        at: SystemTime,
    ) -> NotificationId {
        let Some(held) = self.items.iter().position(|item| item.id == id) else {
            return id;
        };
        let held = self.items.remove(held);
        let existing = (self.items.iter())
            .position(|item| item.toast == Some(toast) && item.state == State::Open);
        match existing {
            Some(at_index) => {
                let mut item = self.items.remove(at_index);
                item.count += held.count;
                item.at = at;
                let survivor = item.id;
                self.items.push(item);
                survivor
            }
            // Nothing of the queue's to count into: it stays as it was.
            None => {
                let mut held = held;
                held.toast = Some(toast);
                let survivor = held.id;
                self.items.push(held);
                survivor
            }
        }
    }

    /// Whether notification `id` is kept and still open.
    pub(crate) fn is_open(&self, id: NotificationId) -> bool {
        (self.items.iter()).any(|item| item.id == id && item.state == State::Open)
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

    /// The queue's toast for notification `id`, if it may still be there.
    pub(crate) fn toast_of(&self, id: NotificationId) -> Option<ToastId> {
        (self.items.iter())
            .find(|item| item.id == id)
            .and_then(|item| item.toast)
    }

    /// Removes notification `id`, as the inbox's dismiss does to an Earlier
    /// one. False if it's gone.
    pub(crate) fn remove(&mut self, id: NotificationId) -> bool {
        let before = self.items.len();
        self.items.retain(|item| item.id != id);
        self.items.len() != before
    }

    /// Removes every Earlier notification, as the inbox's Clear earlier
    /// does, returning the queue's toasts they had: some may still show.
    pub(crate) fn clear_earlier(&mut self) -> Vec<ToastId> {
        let toasts = (self.earlier()).filter_map(|item| item.toast).collect();
        self.items.retain(Notification::needs_you);
        toasts
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
