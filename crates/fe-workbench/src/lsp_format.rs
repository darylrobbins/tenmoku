//! Formatting the active editor's document or selection through its
//! language server (`lsp.format_document`, `lsp.format_selection`). A stub
//! until its card: the commands are registered, and do nothing yet.

use gpui::{Context, Window};

use crate::Workbench;

impl Workbench {
    /// Formats the active editor's document, or only its selection if
    /// `selection`.
    pub(crate) fn format_active(
        &mut self,
        selection: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        let command = match selection {
            true => "lsp.format_selection",
            false => "lsp.format_document",
        };
        log::debug!("{command} isn't implemented yet");
    }
}
