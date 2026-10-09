//! `vim.undo` and `vim.redo`, and the carets they restore.
//!
//! `fe_buffer` undoes text, not carets. So for every transaction fe-vim
//! produces, [`Carets`] keeps the carets just before it, as (row, byte
//! column) pairs rather than anchors: an anchor inside text that `dd`
//! deleted collapses, while nvim's redo goes back to the old row and column.
//! Undo and redo both put the carets back where they were before the change,
//! as nvim does. Like nvim, each caret is taken where the change begins: an
//! operator's at the start of what it acts on (for whole rows, the first
//! row at the caret's column), and an insert session's where typing starts
//! (`o` and `O`: where the caret was). The others' (`J r ~ p P`) begin at
//! the caret.

use std::collections::VecDeque;

use fe_buffer::{ResolvedSelection, TransactionId};
use fe_command::{Actor, CommandError, Dispatched, Effect};
use fe_editor::{EditorModel, commands::origin};
use fe_text::Text;

use crate::{CountArgs, VimTarget, motion::Cursor};

/// How many transactions [`Carets`] remembers.
pub(crate) const KEPT: usize = 1000;

/// The carets before each of the last [`KEPT`] transactions fe-vim produced.
#[derive(Clone, Debug, Default)]
pub(crate) struct Carets {
    entries: VecDeque<(TransactionId, Vec<(u32, usize)>)>,
    /// An insert session's, held apart until its transaction closes: one
    /// that changed nothing isn't kept, and mustn't push a real one out.
    pending: Option<Pending>,
}

/// A transaction's carets, held until it closes.
pub(crate) type Pending = (TransactionId, Vec<(u32, usize)>);

impl Carets {
    /// Notes `carets` as those before `id`, unless `id` already has some: a
    /// transaction is recorded by its first command (inside `.`'s, the first
    /// command it replays).
    pub(crate) fn record(&mut self, id: TransactionId, carets: Vec<(u32, usize)>) {
        if self.get(id).is_some() {
            return;
        }
        if self.entries.len() == KEPT {
            self.entries.pop_front();
        }
        self.entries.push_back((id, carets));
    }

    /// Notes `carets` for the open transaction `id`, to be kept by
    /// [`Carets::close`] if it changes anything.
    pub(crate) fn record_pending(&mut self, id: TransactionId, carets: Vec<(u32, usize)>) {
        let noted = self.pending.as_ref().is_some_and(|(at, _)| *at == id);
        if !noted && self.get(id).is_none() {
            self.pending = Some((id, carets));
        }
    }

    /// The pending transaction closed as `kept`: `None` if it changed
    /// nothing, and `fe_buffer` dropped it.
    pub(crate) fn close(&mut self, kept: Option<TransactionId>) {
        if let Some((id, carets)) = self.pending.take()
            && kept == Some(id)
        {
            self.record(id, carets);
        }
    }

    /// Lets go of the pending record, for a session a clone is still ending
    /// (see `insert::Closing`): a newer session may note its own meanwhile.
    pub(crate) fn take_pending(&mut self) -> Option<Pending> {
        self.pending.take()
    }

    /// A [`Carets::take_pending`] record's transaction was published as
    /// `kept`. It holds the carets before the transaction's first command,
    /// so it replaces what commands that joined the transaction noted
    /// meanwhile, wherever that is: `kept` is `None` when the transaction
    /// changed nothing, but also when a newer session or edit held it open
    /// across the publication, and that one may have closed or noted since.
    pub(crate) fn settle(&mut self, pending: Option<Pending>, kept: Option<TransactionId>) {
        let Some((id, carets)) = pending else {
            return;
        };
        if let Some(entry) = self.entries.iter_mut().rev().find(|(at, _)| *at == id) {
            entry.1 = carets;
        } else if let Some((at, noted)) = &mut self.pending
            && *at == id
        {
            *noted = carets;
        } else if kept == Some(id) {
            self.record(id, carets);
        }
    }

    pub(crate) fn get(&self, id: TransactionId) -> Option<&[(u32, usize)]> {
        self.entries
            .iter()
            .rev()
            .find(|(at, _)| *at == id)
            .map(|(_, carets)| carets.as_slice())
    }

    /// The oldest transaction noted, and its carets.
    #[cfg(test)]
    pub(crate) fn oldest(&self) -> Option<&(TransactionId, Vec<(u32, usize)>)> {
        self.entries.front()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// `offset` as a row and a byte column.
pub(crate) fn pair(text: &Text, offset: usize) -> (u32, usize) {
    let point = text.offset_to_point(offset);
    (point.row, point.column as usize)
}

/// Every caret's head as a row and a byte column.
pub(crate) fn pairs(model: &EditorModel) -> Vec<(u32, usize)> {
    let text = model.text();
    model
        .selections()
        .iter()
        .map(|selection| pair(text, selection.head()))
        .collect()
}

/// A caret at `column` on `row` of `text`, made a normal-mode caret, as a
/// pair: nvim remembers a real position, clamped to the row it was on.
pub(crate) fn caret_on_row(text: &Text, row: u32, column: usize) -> (u32, usize) {
    pair(text, clamped(text, (row, column)))
}

/// The normal-mode caret for a recorded pair: the row clamped to the last,
/// the column to that row's last grapheme.
fn clamped(text: &Text, (row, column): (u32, usize)) -> usize {
    let row = row.min(text.row_count() - 1);
    let column = column.min(text.row_len(row) as usize);
    Cursor::at(text, text.row_start(row) + column).offset()
}

/// `vim.undo`, or `vim.redo` if `redo`: `count` steps of the model's own
/// history for `actor`'s origin, stopping when there's nothing left.
pub(crate) fn undo(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: CountArgs,
    redo: bool,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget { model, state, .. } = target;
    crate::insert::adopt_closing(state);
    let mut reverted = Vec::new();
    for _ in 0..args.count.unwrap_or(1).max(1) {
        let (step, _) = model.run_as(origin(actor), |model| match redo {
            true => model.redo(),
            false => model.undo(),
        });
        match step {
            Ok(Some(id)) => reverted.push(id),
            Ok(None) => break,
            // A conflict after some steps stops there, as running out does.
            Err(_) if !reverted.is_empty() => break,
            Err(error) => return Err(CommandError::new(error.to_string())),
        }
    }
    let Some(&last) = reverted.last() else {
        return Ok(Dispatched::new(Effect::Nothing));
    };
    // The carets before the last transaction reverted or reapplied; without
    // a record, where the model put them, made normal-mode carets.
    let text = model.text().clone();
    let carets: Vec<usize> = match state.carets.get(last) {
        Some(pairs) => pairs.iter().map(|&pair| clamped(&text, pair)).collect(),
        None => model
            .selections()
            .iter()
            .map(|selection| Cursor::at(&text, selection.head()).offset())
            .collect(),
    };
    model.set_selections(carets.into_iter().map(|offset| ResolvedSelection {
        range: offset..offset,
        reversed: false,
    }));
    state.goals = None;
    Ok(Dispatched::new(Effect::Changed).with_transactions(reverted))
}
