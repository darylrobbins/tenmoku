//! The code actions at the active editor's caret or selection
//! (`lsp.code_actions`). A stub until its card: the command is registered,
//! and does nothing yet.

use gpui::{Context, Window};

use crate::Workbench;

impl Workbench {
    /// Lists the code actions for the active editor's selection and its
    /// diagnostics, to pick one to apply.
    pub(crate) fn show_code_actions(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        log::debug!("lsp.code_actions isn't implemented yet");
    }
}
