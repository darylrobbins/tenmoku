//! Dragging the project tree's rows onto a folder, as VS Code's explorer
//! and JetBrains' project view do: a drop moves them there, and with Option
//! (alt) held copies them.
//!
//! A row dragged from outside the selection is dragged alone; one of the
//! selected rows drags them all (those inside another selected folder go
//! with it), drawn under the pointer as "3 items". A drop on a folder's row
//! goes in that folder, on a file's in the file's folder, and on the
//! root's row or the space below the rows in the root. Which row the
//! pointer is over is worked out from the list's last layout and its
//! scroll now, as the drag starts, as the pointer moves and as the panel
//! draws, so rows scrolled under a still pointer count. While the pointer
//! is over a folder a drop would go in, that folder's row and the rows
//! under it are tinted; a collapsed folder under the pointer for
//! [`SPRING_DELAY`] expands. Escape (with Option held too) stops the drag,
//! and nothing drops.
//!
//! A drop is a paste of what's dragged, cut or copied, in that folder
//! ([`clipboard`](super::clipboard)): a move waits for language servers'
//! `willRename` edits, is refused while something acts on either path,
//! and has its editors follow; a copy takes ` copy` names as a paste's
//! does. A move never takes another name: one whose name is taken in the
//! folder isn't made, and a toast says so. Nothing drops into an entry
//! itself or a folder inside it, nor in the folder an entry is in already,
//! nor in a folder that couldn't be read: the paste's toast says the entry
//! can't go there.
//!
//! The drag is mouse-only: the keys' way to the same is cut and paste.

use std::{path::PathBuf, rc::Rc, time::Duration};

use fe_project::{EntryKind, NodeId};
use gpui::{
    Bounds, Context, DragMoveEvent, EntityId, KeyDownEvent, Pixels, Point, SharedString, Task,
    Window,
};
use kiln_l10n::tr;

use super::{ProjectPanel, clipboard::ClipKind};
use crate::workbench::toast::{self, Toast};

/// How long a collapsed folder stays under a dragged row before it
/// expands.
pub(super) const SPRING_DELAY: Duration = Duration::from_millis(700);

/// The key context the panel's element sets while a drag is under way: no
/// binding names it, so escape reaches [`ProjectPanel::on_drag_key`].
pub(super) const DRAG_KEY_CONTEXT: &str = kiln_l10n::verbatim!("project_panel_drag");

/// What a drag of the tree's rows carries: the panel it started in and the
/// entries' paths, under the tree's (canonical) root, in the list's order.
#[derive(Clone, Debug)]
pub(crate) struct DraggedEntries {
    panel: EntityId,
    paths: Rc<Vec<PathBuf>>,
}

/// Where the pointer is while rows are dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Over {
    /// Outside the tree's rows and the space below them.
    Outside,
    /// Over the root's row or the space below the rows.
    Root,
    /// Over this row.
    Row(NodeId),
}

/// A drag of the tree's rows, while it's under way.
pub(super) struct Drag {
    /// What's dragged.
    paths: Rc<Vec<PathBuf>>,
    /// Where the panel was laid out, as the last move over the window saw
    /// it: until one has, the pointer is taken to be inside it.
    bounds: Option<Bounds<Pixels>>,
    /// Where the pointer is.
    over: Over,
    /// The folder a drop would go in now, if a drop there would do
    /// anything.
    target: Option<NodeId>,
    /// The collapsed folder under the pointer, and the wait for it to
    /// expand.
    spring: Option<(NodeId, Task<()>)>,
}

impl ProjectPanel {
    /// What a drag of row `id` carries, and what it's drawn as if not its
    /// own icon and name: with several rows selected (`selection`, their
    /// paths, less those inside another, so maybe one), the selection if
    /// the row is one of them, else the row alone.
    pub(super) fn dragged(
        &self,
        id: NodeId,
        selected: bool,
        selection: Option<&Rc<Vec<PathBuf>>>,
        cx: &Context<Self>,
    ) -> Option<(DraggedEntries, Option<SharedString>)> {
        let panel = cx.entity_id();
        if let Some(selection) = selection.filter(|_| selected) {
            let ghost = match selection.as_slice() {
                [one] => one
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned().into()),
                _ => Some(tr!("workbench-project-drag-count", count = selection.len())),
            };
            let paths = selection.clone();
            return Some((DraggedEntries { panel, paths }, ghost));
        }
        let path = self.tree()?.path(id)?;
        let paths = Rc::new(vec![path]);
        Some((DraggedEntries { panel, paths }, None))
    }

    /// The paths a drag of the selected rows carries: each selected row's,
    /// less those inside another, which go with it.
    pub(super) fn dragged_selection(&self) -> Rc<Vec<PathBuf>> {
        let entries = self.selected_entries().map(|(_, entries)| entries);
        let paths = super::ops::topmost(entries.unwrap_or_default());
        Rc::new(paths.into_iter().map(|(path, _)| path).collect())
    }

    /// A drag of `dragged` started, the pointer where the window has it:
    /// the panel takes it in at once, so a folder already under the
    /// pointer is the target and escape stops it, before the pointer
    /// moves again.
    pub(super) fn drag_started(
        &mut self,
        dragged: &DraggedEntries,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if dragged.panel != cx.entity_id() {
            return;
        }
        self.drag = Some(Drag {
            paths: dragged.paths.clone(),
            bounds: None,
            over: Over::Outside,
            target: None,
            spring: None,
        });
        self.settle_drag(window.mouse_position(), window, cx);
        cx.notify();
    }

    /// The pointer moved while something is dragged, inside the panel's
    /// `event.bounds` or not.
    pub(super) fn drag_moved(
        &mut self,
        event: &DragMoveEvent<DraggedEntries>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.drag(cx).panel != cx.entity_id() {
            return;
        }
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        drag.bounds = Some(event.bounds);
        if self.settle_drag(event.event.position, window, cx) {
            cx.notify();
        }
    }

    /// Where `position` is in the panel, as the last frame laid out its
    /// list, scrolled as it is now: over a row, the root (its row, the
    /// space below the rows, a new entry's field), or outside.
    fn over_at(&self, position: Point<Pixels>) -> Over {
        let inside = (self.drag.as_ref().and_then(|drag| drag.bounds))
            .is_none_or(|bounds| bounds.contains(&position));
        if !inside {
            return Over::Outside;
        }
        let state = self.scroll.0.borrow();
        let list = state.base_handle.bounds();
        if !list.contains(&position) {
            return Over::Root;
        }
        let field = self.list_field();
        let count = self.list_count() + usize::from(field.is_some());
        let Some(size) = state.last_item_size.filter(|_| count > 0) else {
            return Over::Root;
        };
        let height = size.contents.height / count as f32;
        if height <= Pixels::ZERO {
            return Over::Root;
        }
        let y = position.y - list.origin.y - state.base_handle.offset().y;
        if y < Pixels::ZERO {
            return Over::Root;
        }
        let at = (y / height).floor() as usize;
        let at = match field {
            Some((field, _)) if at == field => return Over::Root,
            Some((field, _)) if at > field => at - 1,
            _ => at,
        };
        let row = (self.list_rows(at..at + 1).first()).and_then(|&ix| {
            let tree = self.tree()?;
            tree.rows(ix..ix + 1).next().map(|row| row.id)
        });
        row.map_or(Over::Root, Over::Row)
    }

    /// Takes in where the pointer is, at `position`: the folder a drop
    /// would go in, tinted, and the collapsed folder under the pointer,
    /// which expands after [`SPRING_DELAY`] unless the row under the
    /// pointer changes first. Whether either changed.
    fn settle_drag(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.drag.is_none() {
            return false;
        }
        let over = self.over_at(position);
        let target = self.drop_folder(over).filter(|&dir| {
            let paths = self.drag.as_ref().map(|drag| drag.paths.clone());
            paths.is_some_and(|paths| self.drop_would_act(&paths, dir))
        });
        let spring = match over {
            Over::Row(id) => (self.tree().and_then(|tree| tree.row(id)))
                .filter(|row| row.kind == EntryKind::Dir && !row.expanded && row.error.is_none())
                .map(|_| id),
            _ => None,
        };
        let Some(drag) = self.drag.as_mut() else {
            return false;
        };
        let changed = drag.over != over || drag.target != target;
        drag.over = over;
        drag.target = target;
        match spring {
            Some(id) if drag.spring.as_ref().is_some_and(|(at, _)| *at == id) => {}
            Some(id) => {
                let wait = cx.spawn_in(window, async move |panel, cx| {
                    cx.background_executor().timer(SPRING_DELAY).await;
                    (panel.update_in(cx, |panel, window, cx| panel.spring_open(id, window, cx)))
                        .ok();
                });
                drag.spring = Some((id, wait));
            }
            None => drag.spring = None,
        }
        changed
    }

    /// Takes in where the pointer is again as the panel draws: rows move
    /// under a pointer that doesn't, as the list scrolls or a listing
    /// lands.
    pub(super) fn settle_drag_drawn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.drag.is_some() {
            self.settle_drag(window.mouse_position(), window, cx);
        }
    }

    /// The wait for collapsed folder `id` under the pointer is over: it
    /// expands, if the pointer, where the window has it now, is over it
    /// still.
    fn spring_open(&mut self, id: NodeId, window: &mut Window, cx: &mut Context<Self>) {
        if self.settle_drag(window.mouse_position(), window, cx) {
            cx.notify();
        }
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        if drag.over != Over::Row(id) {
            return;
        }
        drag.spring = None;
        if cx.has_active_drag() {
            self.expand(id, cx);
            cx.notify();
        }
    }

    /// The folder a drop `over` there goes in: a folder's row's own, a
    /// file's row's folder, or the root.
    fn drop_folder(&self, over: Over) -> Option<NodeId> {
        let tree = self.tree()?;
        match over {
            Over::Outside => None,
            Over::Root => Some(tree.root()),
            Over::Row(id) => match tree.row(id)?.kind {
                EntryKind::Dir => Some(id),
                // A symlink to a folder too: a drop never goes through one.
                _ => tree.find(tree.path(id)?.parent()?),
            },
        }
    }

    /// Whether dropping `paths` in folder `dir` would move or copy any of
    /// them: not if it couldn't be read, nor for one it's inside or that's
    /// in it already.
    fn drop_would_act(&self, paths: &[PathBuf], dir: NodeId) -> bool {
        let Some(tree) = self.tree() else {
            return false;
        };
        if tree.row(dir).is_some_and(|row| row.error.is_some()) {
            return false;
        }
        let Some(dir) = tree.path(dir) else {
            return false;
        };
        (paths.iter()).any(|path| !dir.starts_with(path) && path.parent() != Some(&dir))
    }

    /// The folder a drop would go in, while rows are dragged over one
    /// where it would do anything, to tint it and the rows under it.
    pub(super) fn drop_target(&self) -> Option<NodeId> {
        self.drag.as_ref()?.target
    }

    /// `dragged` dropped `over` there, copied if Option (alt) is held:
    /// see the module's docs.
    pub(super) fn dropped(
        &mut self,
        dragged: &DraggedEntries,
        over: Over,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag = None;
        cx.notify();
        if dragged.panel != cx.entity_id() {
            return;
        }
        let kind = match window.modifiers().alt {
            true => ClipKind::Copy,
            false => ClipKind::Cut,
        };
        let Some(dir) = self.drop_folder(over) else {
            return;
        };
        let Some(tree) = self.tree() else {
            return;
        };
        let unlistable = tree.row(dir).is_some_and(|row| row.error.is_some());
        let Some(dir) = tree.path(dir) else {
            return;
        };
        let root = tree.root_path().to_owned();
        self.cancel_reveal();
        self.close_menu(cx);
        let sources = dragged.paths.to_vec();
        let several = sources.len() > 1;
        // Into itself or a folder inside it, into the folder it's in
        // already, or into a folder that couldn't be read: not dropped, and
        // the paste's toast says so.
        let (refused, sources): (Vec<PathBuf>, Vec<PathBuf>) =
            (sources.into_iter()).partition(|source| {
                unlistable || dir.starts_with(source) || source.parent() == Some(&dir)
            });
        for source in refused {
            let name = super::clipboard::file_name(&source);
            let message = match kind {
                ClipKind::Cut => tr!("workbench-project-paste-into-itself-cut", name = name),
                ClipKind::Copy => tr!("workbench-project-paste-into-itself-copy", name = name),
            };
            toast::show(cx, Toast::error(message));
        }
        if sources.is_empty() {
            return;
        }
        self.drop_into(kind, sources, several, dir, root, window, cx);
    }

    /// Escape while rows are dragged: the drag stops, and nothing drops.
    pub(super) fn on_drag_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        // With any modifier: Option (alt) held for a copy too.
        if self.drag.is_none() || keystroke.key != "escape" {
            return;
        }
        self.drag = None;
        cx.stop_active_drag(window);
        cx.notify();
        cx.stop_propagation();
    }

    /// Forgets the drag once none is under way: dropped elsewhere, or
    /// stopped. Called as the panel draws.
    pub(super) fn end_drag(&mut self, cx: &Context<Self>) {
        if self.drag.is_some() && !cx.has_active_drag() {
            self.drag = None;
        }
    }

    /// Whether rows are being dragged, for the panel's key context.
    pub(super) fn dragging(&self) -> bool {
        self.drag.is_some()
    }
}

#[cfg(test)]
impl ProjectPanel {
    /// The folder a drop would go in now, by its path under the tree's
    /// root, while rows are dragged over one where it would do anything.
    pub(crate) fn drop_target_path(&self) -> Option<PathBuf> {
        self.tree()?.path(self.drop_target()?)
    }

    /// Whether a drag of the tree's rows is under way.
    pub(crate) fn is_dragging(&self) -> bool {
        self.dragging()
    }
}
