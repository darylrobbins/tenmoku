//! Consecutive typing outside an open transaction, grouped into one undo
//! step. A run of keystrokes extends one transaction through
//! [`Buffer::edit_in`] until a caret move, a pause, a new word, a line
//! break, another edit of the model's origin, an undo, redo, save or a
//! save's preparation ends it.

use std::{
    ops::Range,
    time::{Duration, Instant},
};

use fe_buffer::{Buffer, BufferEvent, Origin, SelectionSet, Subscription, TransactionId};

use crate::EditorModel;

/// How long typing may pause and still join the step before.
pub const TYPING_PAUSE: Duration = Duration::from_secs(1);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Kind {
    Insert,
    Delete,
}

/// What a keystroke typed, as grouping sees it.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Typed {
    kind: Kind,
    /// Whether the text starts with a non-blank char, so it begins a word
    /// after a blank.
    starts_word: bool,
    /// Whether the text ends with a blank.
    ends_blank: bool,
}

impl Typed {
    /// Typed `text`, unless it breaks a line (or is empty), which is a step
    /// of its own.
    pub(crate) fn insert(text: &str) -> Option<Self> {
        let first = text.chars().next()?;
        let last = text.chars().next_back()?;
        (!text.contains(['\n', '\r'])).then_some(Typed {
            kind: Kind::Insert,
            starts_word: !first.is_whitespace(),
            ends_blank: last.is_whitespace(),
        })
    }

    pub(crate) fn delete() -> Self {
        Typed {
            kind: Kind::Delete,
            starts_word: false,
            ends_blank: false,
        }
    }
}

/// The run of typing the next keystroke may join.
pub(crate) struct TypingGroup {
    transaction: TransactionId,
    origin: Origin,
    kind: Kind,
    /// The carets its last edit left; `None` until
    /// [`EditorModel::typing_left`] places them.
    carets: Option<SelectionSet>,
    /// When its last edit landed, on the model's clock.
    at: Instant,
    ends_blank: bool,
    /// The buffer's events since its last edit.
    events: Subscription,
    /// The models' count of prepared saves when it started.
    save_mark: u64,
}

impl TypingGroup {
    /// Whether an edit since the group's last one ends it: one of its
    /// origin, from any model of the buffer or around them, or a save.
    fn interrupted(&self) -> bool {
        self.events.drain().into_iter().any(|event| match event {
            BufferEvent::Edited { origin, .. } => origin == self.origin,
            BufferEvent::Saved { .. } => true,
        })
    }
}

impl EditorModel {
    /// Sets the clock typing pauses are measured on; [`Instant::now`] by
    /// default.
    pub fn set_clock(&mut self, clock: std::rc::Rc<dyn Fn() -> Instant>) {
        self.clock = clock;
    }

    /// Ends the run of typing, so the next keystroke is a step of its own.
    pub(crate) fn end_typing(&mut self) {
        self.typing = None;
    }

    /// Ends the run of typing if something else edited, saved, or it paused.
    /// Keeps its events from piling up while others edit.
    pub(crate) fn poll_typing(&mut self) {
        if let Some(group) = &self.typing
            && (group.interrupted()
                || (self.clock)().saturating_duration_since(group.at) >= TYPING_PAUSE)
        {
            self.typing = None;
        }
    }

    /// Applies `edits` to `buffer`, joining the run of typing if `typed`
    /// continues it, or as a step of its own, which starts a run if
    /// `typed`. Inside an open transaction the edits join it, as any do,
    /// and no run is kept. [`EditorModel::typing_left`] then records the
    /// carets.
    pub(crate) fn edit_typed(
        &mut self,
        buffer: &mut Buffer,
        edits: Vec<(Range<usize>, String)>,
        typed: Option<Typed>,
    ) -> Option<TransactionId> {
        let now = (self.clock)();
        let open = buffer.open_transaction().is_some();
        let typed = typed.filter(|_| !open);
        let group = self
            .typing
            .take()
            .filter(|group| typed.is_some_and(|typed| self.continues(group, typed, now)));
        if let (Some(mut group), Some(typed)) = (group, typed)
            && let Ok(id) = buffer.edit_in(group.transaction, edits.iter().cloned())
        {
            // Its own edit isn't news.
            group.events.drain();
            if id.is_some() {
                group.at = now;
                group.ends_blank = typed.ends_blank;
            }
            self.typing = Some(group);
            return id;
        }
        let id = buffer.edit(edits, self.origin.clone());
        if let (Some(transaction), Some(typed)) = (id, typed) {
            self.typing = Some(TypingGroup {
                transaction,
                origin: self.origin.clone(),
                kind: typed.kind,
                carets: None,
                at: now,
                ends_blank: typed.ends_blank,
                events: buffer.subscribe(),
                save_mark: self.history.borrow().save_marks,
            });
        }
        id
    }

    /// Records the carets the last edit left, which the next keystroke must
    /// start from to join its run.
    pub(crate) fn typing_left(&mut self, carets: &SelectionSet) {
        if let Some(group) = &mut self.typing {
            group.carets = Some(carets.clone());
        }
    }

    /// Whether `typed`, now, continues `group`.
    fn continues(&self, group: &TypingGroup, typed: Typed, now: Instant) -> bool {
        if group.interrupted()
            || group.save_mark != self.history.borrow().save_marks
            || group.origin != self.origin
            || group.kind != typed.kind
            || (typed.starts_word && group.ends_blank && typed.kind == Kind::Insert)
            || now.saturating_duration_since(group.at) >= TYPING_PAUSE
        {
            return false;
        }
        let selections = self.selections();
        let left = group
            .carets
            .as_ref()
            .map(|carets| carets.resolve(&self.snapshot));
        selections
            .iter()
            .all(|selection| selection.range.is_empty())
            && left.is_some_and(|left| left == selections)
    }
}

#[cfg(test)]
#[path = "typing_tests.rs"]
mod tests;
