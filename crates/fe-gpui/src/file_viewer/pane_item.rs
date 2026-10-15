//! [`FileViewer`] as kiln-shell's [`PaneItem`]: a read-only tab over a
//! large file. It's never dirty and has nothing to save; its status bar
//! says so, beside the cursor in the editor's [`status::CURSOR`] item. A
//! split is another viewer over the same view and index; a saved layout
//! keeps its path and head.

use std::path::PathBuf;

use fe_command::Value;
use gpui::{AnyView, App, AppContext as _, Context, Entity, SharedString, Task, Window};
use kiln_shell::{Icon, PaneItem, StatusItem, StatusSide};

use super::{FileViewer, RowTarget, open};
use crate::pane_item::{file_name, restore_style, status};

/// The id of the `Read-only` status item: on the right, before the cursor.
pub(crate) const READ_ONLY_STATUS: &str = "read_only";

/// The banner a restored viewer shows when its saved state has none.
const RESTORED_BANNER: &str = "Read only: too large to edit";

impl PaneItem for FileViewer {
    const KIND: &'static str = "fastedit.file_viewer";

    fn tab_title(&self, _: &App) -> SharedString {
        file_name(Some(self.view.path())).into()
    }

    /// `big.log — fastedit`.
    fn window_title(&self, cx: &App) -> SharedString {
        format!("{} — fastedit", self.tab_title(cx)).into()
    }

    // Never dirty, so `is_dirty`, `may_close`, `close_revision` and `save`
    // keep their defaults: closing never asks, and there's nothing to save.

    /// Another viewer of the same file.
    fn shares_document(&self, other: &AnyView, cx: &App) -> bool {
        other
            .clone()
            .downcast::<FileViewer>()
            .is_ok_and(|other| other.read(cx).view.path() == self.view.path())
    }

    fn path(&self, _: &App) -> Option<&std::path::Path> {
        Some(self.view.path())
    }

    /// `Read-only`, and the cursor as the editor shows it.
    fn status_items(&self, _: &App) -> Vec<StatusItem> {
        vec![
            StatusItem::new(READ_ONLY_STATUS, StatusSide::Right, 50, "Read-only").icon(Icon::Lock),
            StatusItem::new(
                status::CURSOR,
                StatusSide::Right,
                100,
                self.cursor_text().cloned().unwrap_or_else(|| "…".into()),
            ),
        ]
    }

    /// A viewer over the same view and index, at the same selection, in
    /// this one's theme and under the host's banner.
    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let (view, index) = (self.view.clone(), self.index.clone());
        let (banner, theme) = (self.host_banner.clone(), self.theme.clone());
        let (anchor, head) = (self.anchor(), self.model.head());
        Some(cx.new(|cx| {
            let mut viewer = FileViewer::new(view, index, banner, theme, cx);
            viewer.restore_selection(anchor, head, None, cx);
            viewer
        }))
    }

    /// Its file's path, its head and the host's banner. A path that isn't
    /// UTF-8 isn't saved.
    fn serialize(&self, _: &App) -> Option<Value> {
        let path = self.view.path().to_str()?;
        Some(Value::from_iter([
            ("path", Value::from(path)),
            ("head", Value::from(self.model.head())),
            ("banner", Value::from(self.host_banner.as_ref())),
        ]))
    }

    /// Opens `state`'s file and starts its index off the main thread, in
    /// the theme of [`crate::set_restore_style`], its head put back,
    /// clipped to the text. A file that's gone, or can't be read, isn't
    /// restored.
    fn restore(state: Value, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        let Some(path) = state.get("path").and_then(Value::as_str).map(PathBuf::from) else {
            return Task::ready(None);
        };
        let opening = cx.background_spawn({
            let path = path.clone();
            async move { open(&path) }
        });
        cx.spawn(async move |cx| {
            let (view, index) = match opening.await {
                Ok(opened) => opened,
                Err(error) => {
                    log::info!("not restoring {}: {error}", path.display());
                    return None;
                }
            };
            cx.update(|cx| {
                let (theme, _) = restore_style(cx);
                let banner = state
                    .get("banner")
                    .and_then(Value::as_str)
                    .unwrap_or(RESTORED_BANNER)
                    .to_owned();
                let viewer = cx.new(|cx| FileViewer::new(view, index, banner.into(), theme, cx));
                viewer.update(cx, |viewer, cx| place_head(viewer, &state, cx));
                Some(viewer)
            })
        })
    }

    /// Puts back the head `state` saved, clipped to the text.
    fn apply_state(&mut self, state: &Value, _: &mut Window, cx: &mut Context<Self>) {
        place_head(self, state, cx);
    }
}

/// Puts `viewer`'s head where `state` says, as its index reaches it, and
/// scrolls to it.
fn place_head(viewer: &mut FileViewer, state: &Value, cx: &mut Context<FileViewer>) {
    if let Some(row) = state.get("row").and_then(Value::as_u64) {
        let column = state.get("column").and_then(Value::as_u64).unwrap_or(0);
        viewer.restore_row(
            RowTarget {
                row: u32::try_from(row).unwrap_or(u32::MAX),
                column: usize::try_from(column).unwrap_or(usize::MAX),
                within_row: state
                    .get("within_row")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            },
            cx,
        );
        return;
    }
    let Some(head) = state.get("head").and_then(Value::as_u64) else {
        return;
    };
    let head = usize::try_from(head).unwrap_or(usize::MAX);
    viewer.restore_selection(head, head, None, cx);
}
