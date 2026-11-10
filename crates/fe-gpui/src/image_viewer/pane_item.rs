//! [`ImageViewer`] as kiln-shell's [`PaneItem`]: never dirty, nothing to
//! save. Its status bar shows the picture's size in pixels, the file's
//! size and the zoom; clicking the zoom toggles Fit and 100%. A split is
//! another viewer of the same file; a saved layout keeps its path and
//! zoom, and drops a file that's gone.

use std::path::{Path, PathBuf};

use fe_command::{Invocation, Value};
use gpui::{AnyView, App, AppContext as _, Context, Entity, SharedString, Task, Window};
use kiln_shell::{PaneItem, StatusItem, StatusSide};

use super::{ImageViewer, ZOOM_ACTUAL, ZOOM_FIT, Zoom, size_text, zoom_of};
use crate::pane_item::{file_name, restore_style};

/// The ids of the viewer's status bar items, on the right.
pub mod status {
    /// `640 × 480`: priority 100.
    pub const DIMENSIONS: &str = "image_dimensions";
    /// The file's size, such as `12 KiB`: priority 200.
    pub const FILE_SIZE: &str = "image_file_size";
    /// `Fit` or `150%`; a click runs `image.zoom_actual` from Fit and
    /// `image.zoom_fit` otherwise: priority 300.
    pub const ZOOM: &str = "image_zoom";
}

impl PaneItem for ImageViewer {
    const KIND: &'static str = "fastedit.image_viewer";

    fn tab_title(&self, _: &App) -> SharedString {
        file_name(Some(self.path())).into()
    }

    /// `logo.png — Tenmoku`.
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

    /// Another viewer of the same file.
    fn shares_document(&self, other: &AnyView, cx: &App) -> bool {
        other
            .clone()
            .downcast::<ImageViewer>()
            .is_ok_and(|other| other.read(cx).path() == self.path())
    }

    fn path(&self, _: &App) -> Option<&Path> {
        Some(ImageViewer::path(self))
    }

    /// The pixel size, the file size and the zoom.
    fn status_items(&self, _: &App) -> Vec<StatusItem> {
        let mut items = Vec::new();
        if let Some((width, height)) = self.pixel_size() {
            items.push(StatusItem::new(
                status::DIMENSIONS,
                StatusSide::Right,
                100,
                kiln_l10n::tr!("gpui-image-dimensions", width = width, height = height),
            ));
        }
        if let Some(len) = self.file_len() {
            items.push(StatusItem::new(
                status::FILE_SIZE,
                StatusSide::Right,
                200,
                size_text(len),
            ));
        }
        if self.pixel_size().is_some() {
            let (command, tooltip) = match self.zoom() {
                Zoom::Fit => (
                    ZOOM_ACTUAL,
                    kiln_l10n::tr!("gpui-image-actual-size-tooltip"),
                ),
                Zoom::Scale(_) => (ZOOM_FIT, kiln_l10n::tr!("gpui-image-fit-tooltip")),
            };
            items.push(
                StatusItem::new(status::ZOOM, StatusSide::Right, 300, self.zoom_text())
                    .command(Invocation::of(command).expect("a valid command ID"))
                    .tooltip(tooltip),
            );
        }
        items
    }

    /// A viewer of the same file, at this one's zoom.
    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let snapshot = self.snapshot();
        Some(cx.new(|cx| ImageViewer::from_snapshot(snapshot, cx)))
    }

    /// Its file's path and zoom. A path that isn't UTF-8 isn't saved.
    fn serialize(&self, _: &App) -> Option<Value> {
        let path = self.path().to_str()?;
        let zoom = match self.zoom() {
            Zoom::Fit => Value::from("fit"),
            Zoom::Scale(scale) => Value::from(f64::from(scale)),
        };
        let mut state = vec![("path", Value::from(path)), ("zoom", zoom)];
        // Where an SVG may link files, kept across a restore.
        if let Some(root) = self.root().and_then(Path::to_str) {
            state.push(("root", Value::from(root)));
        }
        Some(Value::from_iter(state))
    }

    /// A viewer of `state`'s file in the theme of
    /// [`crate::set_restore_style`], at its zoom. A file that's gone isn't
    /// restored.
    fn restore(state: Value, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        let Some(path) = state.get("path").and_then(Value::as_str).map(PathBuf::from) else {
            return Task::ready(None);
        };
        if !path.is_file() {
            log::info!("not restoring {}: gone", path.display());
            return Task::ready(None);
        }
        let zoom = zoom_of(&state);
        let root = state.get("root").and_then(Value::as_str).map(PathBuf::from);
        let (theme, _) = restore_style(cx);
        Task::ready(Some(
            cx.new(|cx| ImageViewer::in_workspace(path, zoom, root, theme, cx)),
        ))
    }

    /// Puts back the zoom a split's saved state holds.
    fn apply_state(&mut self, state: &Value, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_zoom(state, cx);
    }
}
