//! Change End of Line Sequence: LF and CRLF, as VS Code's
//! `workbench.action.editor.changeEOL` without the picker.
//!
//! The text always holds `\n`; the ending is how a save writes each break, so
//! the change leaves the rows and selections alone and marks the buffer
//! modified until it's saved or undone.

use fe_buffer::LineEnding;
use fe_command::{Effect, Registry};

use crate::{
    EditorModel,
    commands::{Editor, add},
};

/// Registers the two commands. They change the buffer.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    add(registry, "set_end_of_line_lf", true, |model, _| {
        model.set_end_of_line(LineEnding::Lf);
        Ok(Effect::Changed)
    });
    add(registry, "set_end_of_line_crlf", true, |model, _| {
        model.set_end_of_line(LineEnding::CrLf);
        Ok(Effect::Changed)
    });
}

impl EditorModel {
    /// Has the buffer save with LF breaks, as one undo step. Nothing
    /// changes if it does already.
    pub fn set_end_of_line_lf(&mut self) {
        self.set_end_of_line(LineEnding::Lf);
    }

    /// Has the buffer save with CRLF breaks, as one undo step. Nothing
    /// changes if it does already.
    pub fn set_end_of_line_crlf(&mut self) {
        self.set_end_of_line(LineEnding::CrLf);
    }

    fn set_end_of_line(&mut self, ending: LineEnding) {
        self.sync_display();
        self.end_typing();
        let origin = self.origin.clone();
        let id = self.buffer.borrow_mut().change_line_ending(ending, origin);
        if let (Some(id), Some(transactions)) = (id, &mut self.transactions) {
            transactions.push(id);
        }
    }
}

#[cfg(test)]
#[path = "eol_commands_tests.rs"]
mod tests;
