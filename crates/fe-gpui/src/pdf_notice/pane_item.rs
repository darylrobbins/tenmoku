//! [`PdfNotice`] as kiln-shell's [`PaneItem`]: never dirty, nothing to
//! save, read-only in the status bar. A split is another notice of the same
//! file; a saved layout keeps its path and drops a file that's gone.

use std::path::{Path, PathBuf};

use fe_command::Value;
use gpui::{AnyView, App, AppContext as _, Context, Entity, SharedString, Task, Window};
use kiln_shell::{Icon, PaneItem, StatusItem, StatusSide};

use super::PdfNotice;
use crate::{
    READ_ONLY_STATUS,
    pane_item::{file_name, restore_style},
};

impl PaneItem for PdfNotice {
    const KIND: &'static str = "fastedit.pdf_notice";

    fn tab_title(&self, _: &App) -> SharedString {
        file_name(Some(self.path())).into()
    }

    /// `paper.pdf — Tenmoku`.
    fn window_title(&self, cx: &App) -> SharedString {
        let title = self.tab_title(cx);
        kiln_l10n::tr!(
            "gpui-window-title",
            title = title,
            app = fe_config::APP_NAME
        )
    }

    // Never dirty, so `is_dirty`, `may_close`, `close_revision` and `save`
    // keep their defaults: closing never asks, and there's nothing to save.

    /// Another notice of the same file.
    fn shares_document(&self, other: &AnyView, cx: &App) -> bool {
        other
            .clone()
            .downcast::<PdfNotice>()
            .is_ok_and(|other| other.read(cx).path() == self.path())
    }

    fn path(&self, _: &App) -> Option<&Path> {
        Some(PdfNotice::path(self))
    }

    /// `Read-only`.
    fn status_items(&self, _: &App) -> Vec<StatusItem> {
        vec![
            StatusItem::new(
                READ_ONLY_STATUS,
                StatusSide::Right,
                50,
                kiln_l10n::tr!("gpui-status-read-only"),
            )
            .icon(Icon::Lock),
        ]
    }

    /// A notice of the same file.
    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let (path, theme) = (self.path().to_owned(), self.theme.clone());
        Some(cx.new(|cx| PdfNotice::new(path, theme, cx)))
    }

    /// Its file's path. A path that isn't UTF-8 isn't saved.
    fn serialize(&self, _: &App) -> Option<Value> {
        let path = self.path().to_str()?;
        Some(Value::from_iter([("path", Value::from(path))]))
    }

    /// A notice of `state`'s file in the theme of
    /// [`crate::set_restore_style`]. A file that's gone isn't restored.
    fn restore(state: Value, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        let Some(path) = state.get("path").and_then(Value::as_str).map(PathBuf::from) else {
            return Task::ready(None);
        };
        if !path.is_file() {
            log::info!("not restoring {}: gone", path.display());
            return Task::ready(None);
        }
        let (theme, _) = restore_style(cx);
        Task::ready(Some(cx.new(|cx| PdfNotice::new(path, theme, cx))))
    }
}
