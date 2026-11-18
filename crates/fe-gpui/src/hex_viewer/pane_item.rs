//! [`HexViewer`] as kiln-shell's [`PaneItem`]: a read-only tab of a file's
//! bytes. It's never dirty and has nothing to save; its status bar says
//! so, beside the offset in the editor's [`status::CURSOR`] item, as the
//! file viewer's does. A split is another viewer over the same
//! [`FileView`]; a saved layout keeps its path, selection and top row.

use std::{path::PathBuf, sync::Arc};

use fe_command::Value;
use fe_fileview::FileView;
use gpui::{AnyView, App, AppContext as _, Context, Entity, SharedString, Task, Window};
use kiln_shell::{Icon, PaneItem, StatusItem, StatusSide};

use super::{HexViewer, inspector::Endian};
use crate::{
    READ_ONLY_STATUS,
    pane_item::{file_name, restore_style, status},
};

impl PaneItem for HexViewer {
    const KIND: &'static str = "fastedit.hex_viewer";

    fn tab_title(&self, _: &App) -> SharedString {
        file_name(Some(self.view.path())).into()
    }

    /// `data.bin — Tenmoku`.
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

    /// Another hex viewer of the same file.
    fn shares_document(&self, other: &AnyView, cx: &App) -> bool {
        other
            .clone()
            .downcast::<HexViewer>()
            .is_ok_and(|other| other.read(cx).view.path() == self.view.path())
    }

    fn path(&self, _: &App) -> Option<&std::path::Path> {
        Some(self.view.path())
    }

    /// `Read-only`, and the selection's offset.
    fn status_items(&self, _: &App) -> Vec<StatusItem> {
        vec![
            StatusItem::new(
                READ_ONLY_STATUS,
                StatusSide::Right,
                50,
                kiln_l10n::tr!("gpui-status-read-only"),
            )
            .icon(Icon::Lock),
            StatusItem::new(status::CURSOR, StatusSide::Right, 100, self.cursor_text()),
        ]
    }

    /// A viewer over the same view, at the same selection and top row, in
    /// this one's theme.
    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let (view, theme) = (self.view.clone(), self.theme.clone());
        let (anchor, head, top, column) = (self.anchor, self.head, self.top_row, self.column);
        let (shown, endian) = (self.inspector_shown, self.endian);
        Some(cx.new(|cx| {
            let mut viewer = HexViewer::new(view, theme, cx);
            viewer.put_back(anchor, head, Some(top));
            viewer.column = column;
            viewer.set_inspector(shown, endian);
            viewer
        }))
    }

    /// Its file's path, the selection's ends and the top row. A path that
    /// isn't UTF-8 isn't saved.
    fn serialize(&self, _: &App) -> Option<Value> {
        let path = self.view.path().to_str()?;
        Some(Value::from_iter([
            ("path", Value::from(path)),
            ("anchor", Value::from(self.anchor)),
            ("head", Value::from(self.head)),
            ("top_row", Value::from(self.top_row)),
            ("inspector", Value::Bool(self.inspector_shown)),
            (
                "endian",
                Value::from(match self.endian {
                    Endian::Little => "little",
                    Endian::Big => "big",
                }),
            ),
        ]))
    }

    /// Opens `state`'s file off the main thread, in the theme of
    /// [`crate::set_restore_style`], its selection put back clipped to the
    /// file. A file that's gone, or can't be opened, isn't restored.
    fn restore(state: Value, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        let Some(path) = state.get("path").and_then(Value::as_str).map(PathBuf::from) else {
            return Task::ready(None);
        };
        let opening = cx.background_spawn({
            let path = path.clone();
            async move { FileView::open(&path) }
        });
        cx.spawn(async move |cx| {
            let view = match opening.await {
                Ok(view) => Arc::new(view),
                Err(error) => {
                    log::info!("not restoring {}: {error}", path.display());
                    return None;
                }
            };
            cx.update(|cx| {
                let (theme, _) = restore_style(cx);
                let viewer = cx.new(|cx| HexViewer::new(view, theme, cx));
                viewer.update(cx, |viewer, cx| place(viewer, &state, cx));
                Some(viewer)
            })
        })
    }

    /// Puts back the selection and top row `state` saved, clipped.
    fn apply_state(&mut self, state: &Value, _: &mut Window, cx: &mut Context<Self>) {
        place(self, state, cx);
    }
}

/// Puts `viewer`'s selection and top row where `state` says, clipped to its
/// file.
pub(super) fn place(viewer: &mut HexViewer, state: &Value, cx: &mut Context<HexViewer>) {
    // A state saved before the inspector has neither: shown, little-endian.
    let shown = state
        .get("inspector")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let endian = match state.get("endian").and_then(Value::as_str) {
        Some("big") => Endian::Big,
        _ => Endian::Little,
    };
    viewer.set_inspector(shown, endian);
    let head = state.get("head").and_then(Value::as_u64);
    let anchor = state.get("anchor").and_then(Value::as_u64).or(head);
    let (Some(anchor), Some(head)) = (anchor, head) else {
        return;
    };
    let top = state.get("top_row").and_then(Value::as_u64);
    viewer.put_back(anchor, head, top);
    cx.notify();
}
