//! [`EditorView`] as kiln-shell's [`PaneItem`]: what a pane shows in an
//! editor's tab (its file's name, a dirty dot), the window's title while
//! it's active, and its file, saving, splitting and saved state, from its
//! [`EditorModel`](fe_editor::EditorModel).

use std::path::Path;

use fe_command::Value;
use gpui::{AnyView, App, AppContext as _, Context, Entity, SharedString, Task, Window};
use kiln_shell::PaneItem;

use crate::EditorView;

/// A tab's label for a buffer at `path`: its file name, or `untitled`.
pub fn file_name(path: Option<&Path>) -> String {
    path.and_then(Path::file_name).map_or_else(
        || "untitled".into(),
        |name| name.to_string_lossy().into_owned(),
    )
}

impl PaneItem for EditorView {
    const KIND: &'static str = "fastedit.editor";

    fn tab_title(&self, _: &App) -> SharedString {
        file_name(self.model.path()).into()
    }

    /// `main.rs — fastedit`, `main.rs ● — fastedit` with unsaved changes.
    fn window_title(&self, cx: &App) -> SharedString {
        let dirty = if self.is_dirty(cx) { " ●" } else { "" };
        format!("{}{dirty} — fastedit", self.tab_title(cx)).into()
    }

    fn is_dirty(&self, _: &App) -> bool {
        self.model.is_dirty()
    }

    /// Another editor over its buffer ([`EditorModel::split`](fe_editor::EditorModel::split)).
    fn shares_document(&self, other: &AnyView, cx: &App) -> bool {
        other
            .clone()
            .downcast::<EditorView>()
            .is_ok_and(|other| other.read(cx).model.shares_buffer_with(&self.model))
    }

    // `may_close` keeps the default: a dirty editor stays open.

    /// What `editor.save` does: [`EditorModel::save`](fe_editor::EditorModel::save),
    /// which is synchronous, so the task is ready at once.
    fn save(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Task<gpui::Result<()>>> {
        let saved = self.model.save().map_err(Into::into);
        cx.notify();
        Some(Task::ready(saved))
    }

    fn path(&self, _: &App) -> Option<&Path> {
        self.model.path()
    }

    /// An editor over the same buffer, with its own view of it, in this
    /// one's theme.
    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let model = self.model.split();
        let theme = self.theme.clone();
        Some(cx.new(|cx| EditorView::new(model, theme, cx)))
    }

    /// Its file's path and its caret, the primary selection's head. An
    /// untitled buffer, or one at a path that isn't UTF-8, isn't saved.
    fn serialize(&self, _: &App) -> Option<Value> {
        let path = self.model.path()?.to_str()?;
        let caret = self.model.primary_selection().head();
        Some(Value::from_iter([
            ("path", Value::from(path)),
            ("caret", Value::from(caret)),
        ]))
    }
}

#[cfg(test)]
#[path = "pane_item_tests.rs"]
mod tests;
