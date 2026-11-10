//! Cursor Undo and Cursor Redo: stepping back through the selections the
//! user made, without touching the text or its undo stack.
//!
//! [`EditorModel::set_selections`] records the selections it replaces.
//! Typing and deleting move the carets through edits, not through it, so
//! they leave no entries; neither does a change that stays in the primary
//! selection's row, which keeps arrowing along a line from filling the
//! stack.

use fe_buffer::{BufferSnapshot, Origin, ResolvedSelection, SelectionSet, Text};
use fe_command::{Effect, Registry};

use crate::{
    EditorModel,
    commands::{Editor, add},
};

/// The most selection sets Cursor Undo steps back through.
pub(crate) const MAX_ENTRIES: usize = 50;

/// Earlier selection sets, as anchors that follow edits, nearest last.
#[derive(Default)]
pub(crate) struct CursorHistory {
    undo: Vec<SelectionSet>,
    redo: Vec<SelectionSet>,
    /// Whether the change being made is a keyboard move or a click, which
    /// joins the one before it in a row.
    pub(crate) coalesce: bool,
}

/// A [`CursorHistory`] as offsets, each set with its primary, to anchor
/// again once edits that leave the text as it was are over.
#[derive(Clone, Debug, Default)]
pub(crate) struct ResolvedHistory {
    undo: Vec<ResolvedSet>,
    redo: Vec<ResolvedSet>,
    coalesce: bool,
}

/// A selection set as offsets, with which is primary.
type ResolvedSet = (Vec<ResolvedSelection>, Option<usize>);

impl CursorHistory {
    /// The sets as offsets in `snapshot`.
    pub(crate) fn resolve(&self, snapshot: &BufferSnapshot) -> ResolvedHistory {
        let resolve = |set: &SelectionSet| {
            let selections = set.resolve(snapshot);
            let primary = set.primary_in(snapshot);
            let primary = primary.and_then(|primary| selections.iter().position(|s| *s == primary));
            (selections, primary)
        };
        ResolvedHistory {
            undo: self.undo.iter().map(resolve).collect(),
            redo: self.redo.iter().map(resolve).collect(),
            coalesce: self.coalesce,
        }
    }

    /// `resolved` anchored in `snapshot`, whose text is the one it was
    /// resolved in.
    pub(crate) fn anchored(resolved: ResolvedHistory, snapshot: &BufferSnapshot) -> Self {
        let anchor = |(selections, primary): ResolvedSet| match primary {
            Some(ix) => SelectionSet::from_resolved_with_primary(snapshot, selections, ix),
            None => SelectionSet::from_resolved(snapshot, selections),
        };
        Self {
            undo: resolved.undo.into_iter().map(anchor).collect(),
            redo: resolved.redo.into_iter().map(anchor).collect(),
            coalesce: resolved.coalesce,
        }
    }

    fn push(stack: &mut Vec<SelectionSet>, set: SelectionSet) {
        if stack.len() == MAX_ENTRIES {
            stack.remove(0);
        }
        stack.push(set);
    }
}

pub(crate) fn register(registry: &mut Registry<Editor>) {
    add(registry, "cursor_undo", false, |model, _| {
        model.cursor_undo();
        Ok(Effect::Changed)
    });
    add(registry, "cursor_redo", false, |model, _| {
        model.cursor_redo();
        Ok(Effect::Changed)
    });
}

impl EditorModel {
    /// [`EditorModel::set_selections`] for a move or a click.
    pub(crate) fn set_selections_moving(
        &mut self,
        selections: impl IntoIterator<Item = ResolvedSelection>,
    ) {
        self.cursor_history.coalesce = true;
        self.set_selections(selections);
        self.cursor_history.coalesce = false;
    }

    /// Notes that the selections are about to become `new`, if the user
    /// made that change. One that leaves the primary selection in its row
    /// and only moves the heads records nothing, past the first entry.
    pub(crate) fn record_cursor_history(&mut self, new: &SelectionSet) {
        if self.origin != Origin::User {
            self.cursor_history.coalesce = false;
            return;
        }
        let old = self.selections();
        let new_resolved = new.resolve(&self.snapshot);
        // The same selections with another primary are a change.
        if old == new_resolved
            && self.selections.primary_in(&self.snapshot) == new.primary_in(&self.snapshot)
        {
            self.cursor_history.coalesce = false;
            return;
        }
        let row_of = |selections: &[ResolvedSelection], text: &Text| {
            selections
                .last()
                .map(|selection| text.offset_to_point(selection.head()).row)
        };
        self.cursor_history.redo.clear();
        let text = self.text();
        let same_row = row_of(&old, text) == row_of(&new_resolved, text);
        // Moving or clicking along a row is one step; any other command
        // that changes the selection is its own.
        let coalesce = std::mem::take(&mut self.cursor_history.coalesce);
        if coalesce
            && !self.cursor_history.undo.is_empty()
            && same_row
            && old.len() == new_resolved.len()
        {
            return;
        }
        let old = self.selections.clone();
        CursorHistory::push(&mut self.cursor_history.undo, old);
    }

    /// Goes back to the selections before the latest recorded change.
    pub fn cursor_undo(&mut self) {
        self.step_cursor_history(true);
    }

    /// Undoes a [`EditorModel::cursor_undo`].
    pub fn cursor_redo(&mut self) {
        self.step_cursor_history(false);
    }

    fn step_cursor_history(&mut self, back: bool) {
        let current = self.selections();
        loop {
            let history = &mut self.cursor_history;
            let Some(set) = (if back {
                &mut history.undo
            } else {
                &mut history.redo
            })
            .pop() else {
                return;
            };
            let len = self.text().len();
            let target: Vec<ResolvedSelection> = set
                .resolve(&self.snapshot)
                .into_iter()
                .map(|selection| ResolvedSelection {
                    range: selection.range.start.min(len)..selection.range.end.min(len),
                    reversed: selection.reversed,
                })
                .collect();
            // `target` is `set` resolved, so the primary's index holds.
            let next = match set.primary_index_in(&self.snapshot) {
                Some(ix) => {
                    SelectionSet::from_resolved_with_primary(&self.snapshot, target.clone(), ix)
                }
                None => SelectionSet::from_resolved(&self.snapshot, target.clone()),
            };
            if target == current
                && next.primary_in(&self.snapshot) == self.selections.primary_in(&self.snapshot)
            {
                continue;
            }
            let from = self.selections.clone();
            let other = if back {
                &mut self.cursor_history.redo
            } else {
                &mut self.cursor_history.undo
            };
            CursorHistory::push(other, from);
            // Not a user change of its own: leave the stacks as stepped.
            self.end_typing();
            self.goals.clear();
            self.column_box = None;
            self.selections = next;
            self.forget_auto_closed();
            self.follow_snippet();
            return;
        }
    }
}

#[cfg(test)]
#[path = "cursor_history_tests.rs"]
mod tests;

/// Cursor Undo and Redo still reach the selections made before an undo
/// history import, whose edits take the text away and back (T483).
#[cfg(test)]
mod import_tests {
    use fe_buffer::{Buffer, Origin};

    use crate::EditorModel;

    #[test]
    fn cursor_undo_reaches_selections_from_before_an_import() {
        let rows: String = (0..30).map(|row| format!("row {row}\n")).collect();
        let mut typed = Buffer::new("");
        typed.edit([(0..0, rows.as_str())], Origin::User);
        let record = typed.export_undo().unwrap();
        let mut model = EditorModel::new(Buffer::new(&rows), None);
        model.cursor_history = super::CursorHistory::default();
        let start = |model: &EditorModel, row| model.text().row_start(row);
        for row in [1, 8, 20] {
            let offset = start(&model, row);
            model.set_cursor(offset);
        }
        let state = model.selection_state();
        model.buffer_mut().import_undo(record).unwrap();
        model.restore_selection_state(state);
        assert_eq!(model.primary_selection().head(), start(&model, 20));
        model.cursor_undo();
        assert_eq!(model.primary_selection().head(), start(&model, 8));
        model.cursor_undo();
        assert_eq!(model.primary_selection().head(), start(&model, 1));
        model.cursor_redo();
        assert_eq!(model.primary_selection().head(), start(&model, 8));
    }
}
