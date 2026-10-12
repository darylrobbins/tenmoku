//! Renaming the symbol at the caret through its language server
//! (`lsp.rename`). A stub until its card: the command is registered, and
//! does nothing yet.

use gpui::{Context, Window};

use crate::Workbench;

impl Workbench {
    /// Asks for a new name for the symbol at the active editor's caret,
    /// and renames it across the project.
    pub(crate) fn rename_symbol(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        log::debug!("lsp.rename isn't implemented yet");
    }
}
