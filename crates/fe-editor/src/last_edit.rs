//! Go to Last Edit Location, as VS Code's
//! `workbench.action.navigateToLastEditLocation`, within the buffer.
//!
//! The user's edits through the model note where the primary caret ended
//! up, as an anchor, so the place follows later edits; undo, redo and the
//! edits of other origins (formatting, the AI, a reload from disk) don't.
//! Edits a host makes for the user straight to the buffer (a replace, a
//! rename) note theirs with [`BufferMut::note_edits_since`]. A buffer's
//! models, split views of it, share the one place. Jumping doesn't record
//! where the caret was: no editor-local jump does.

use std::{cell::Cell, rc::Rc};

use fe_buffer::{Anchor, Bias, Origin, SelectionSet, Version};
use fe_command::{Effect, Registry};

use crate::{
    BufferMut, EditorModel, caret,
    commands::{Editor, add},
};

/// Where the last user edit ended, shared by a buffer's models.
pub(crate) type LastEdit = Rc<Cell<Option<Anchor>>>;

/// Registers `editor.go_to_last_edit_location`, which doesn't edit.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    add(registry, "go_to_last_edit_location", false, |model, _| {
        model.go_to_last_edit_location();
        Ok(Effect::Changed)
    });
}

impl EditorModel {
    /// Notes the primary head of `selections`, those an edit leaves, as
    /// where the last edit ended, if the user made it. Call before catching
    /// up, which may make a newer edit through another model.
    pub(crate) fn note_edit(&self, selections: &SelectionSet) {
        if let Some(primary) = selections.iter().last() {
            let head = match primary.reversed {
                true => primary.start,
                false => primary.end,
            };
            self.note_edit_at(head);
        }
    }

    /// Notes `anchor` as where the last edit ended, if the user made it.
    pub(crate) fn note_edit_at(&self, anchor: Anchor) {
        if self.origin == Origin::User {
            self.last_edit.set(Some(anchor));
        }
    }

    /// Where the last edit the user made to the buffer, through this model
    /// or a split of it, ended: an offset in [`EditorModel::text`], moved by
    /// the edits since. `None` before any.
    pub fn last_edit_location(&self) -> Option<usize> {
        let anchor = self.last_edit.get()?;
        // A split behind the buffer can't place a newer anchor yet.
        let version = anchor.version();
        (self.snapshot.oldest_version() <= version && version <= self.snapshot.version())
            .then(|| self.snapshot.resolve(&anchor))
    }

    /// Puts a single caret at [`EditorModel::last_edit_location`], opening
    /// a fold hiding it. Returns whether there was one.
    pub fn go_to_last_edit_location(&mut self) -> bool {
        self.sync_display();
        let Some(offset) = self.last_edit_location() else {
            return false;
        };
        let row = self.text().offset_to_point(offset).row;
        self.unfold_rows(row..=row);
        self.set_selections([caret(offset)]);
        true
    }
}

impl BufferMut<'_> {
    /// Notes the end of the last edit the buffer's newest batch since
    /// `version` made as where the last edit ended, if the model's edits are
    /// the user's: for an edit a host makes for the user straight to the
    /// buffer. Call before dropping this, whose catching up may make a newer
    /// edit through another model. Nothing if there's no batch since, or
    /// `version` is past the buffer's log.
    pub fn note_edits_since(&self, version: Version) {
        let snapshot = (**self).snapshot();
        if !self.by_user || version < snapshot.oldest_version() || version > snapshot.version() {
            return;
        }
        let end = (snapshot.edits_since(version).last())
            .and_then(|batch| batch.iter().map(|edit| edit.new.end).max());
        if let Some(end) = end {
            self.last_edit
                .set(Some(snapshot.anchor_at(end, Bias::Left)));
        }
    }
}

#[cfg(test)]
#[path = "last_edit_tests.rs"]
mod tests;
