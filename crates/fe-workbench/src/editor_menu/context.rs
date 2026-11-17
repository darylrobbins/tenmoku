//! The menu's last group (T1898), for an editor of one of the window's
//! panes, each item where it applies:
//!
//! - **Run Test** (`tests.run_at_cursor`) when there's a test at the caret,
//!   which the click put there, as the command finds it; **Run Tests** when
//!   that's a module. Nothing while the file's language loads or its parse
//!   is behind the text, and nothing said: the command's toasts are for
//!   running it.
//! - **Toggle Breakpoint** (`debug.toggle_breakpoint`) when the editor has
//!   the debugger's breakpoint handlers and a file. It acts on the caret's row, the
//!   row clicked.
//! - **Annotate with Git Blame** (`git.toggle_blame_annotations`), or
//!   **Hide Git Blame** while the editor shows them, for a file in the
//!   window's git repository, as its last status read found it.
//! - **Ask Agent About Selection** (`chat.ask_about_selection`), or
//!   **Ask Agent About This Line** with an empty selection, as the command
//!   attaches the caret's row then. Always: it works in an untrusted folder
//!   too.

use fe_command::ContextStack;
use fe_gpui::{EditorMenuItem, EditorView};
use gpui::{Context, Entity};

use super::Item;
use crate::{Workbench, test_commands::Target, test_runnables::RunnableKind};

impl Workbench {
    /// The last group's items for `editor`, the active editor of the active
    /// pane, their keys read in `contexts`; see the [module](self).
    pub(super) fn editor_menu_context(
        &self,
        editor: &Entity<EditorView>,
        contexts: &ContextStack,
        cx: &mut Context<Self>,
    ) -> Vec<EditorMenuItem> {
        let mut items = Vec::new();
        // The command runs from the active editor, which the click made it.
        let active = self.active_editor(cx) == Some(editor);
        if let Some(found) = active
            .then(|| self.test_file(Target::Cursor, cx).ok())
            .flatten()
        {
            items.push(match found.runnable.kind {
                RunnableKind::Module => Item::RunTests,
                RunnableKind::Test | RunnableKind::File => Item::RunTest,
            });
        }
        let view = editor.read(cx);
        // The command needs a file: it says so for an untitled buffer,
        // which has the handlers too.
        if view.has_breakpoint_handlers() && view.model().path().is_some() {
            items.push(Item::ToggleBreakpoint);
        }
        let path = view.model().path();
        if path.is_some_and(|path| self.in_git_repository(path, cx)) {
            items.push(match self.blame_annotates(editor, cx) {
                true => Item::HideBlame,
                false => Item::AnnotateBlame,
            });
        }
        items.push(match view.model().primary_selection().range.is_empty() {
            true => Item::AskAboutLine,
            false => Item::AskAboutSelection,
        });
        (items.into_iter())
            .map(|item| item.entry(true, contexts, cx))
            .collect()
    }
}
