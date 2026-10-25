//! Reopen Closed Editor (`workbench.reopen_closed_editor`, as VS Code's
//! `workbench.action.reopenClosedEditor`): the window's file editors closed
//! last, newest on top, each with its selections, scroll and pane as it
//! closed. Every pane of a window holds the one stack
//! ([`Pane::closed_editors`]), which [`Pane`] pushes to whenever an
//! editor of a file leaves it without going to another pane: Close Tab, the
//! tab's ×, vim's `:q`, Close Pane. A copy of the file left open in
//! another tab doesn't stop it: the reopen then shows that tab. An untitled buffer, a viewer of a file too large to
//! edit, a diff and a Markdown preview aren't pushed, nor is a preview tab
//! that the next preview replaces.

use std::{cell::RefCell, collections::VecDeque, path::PathBuf, rc::Rc};

use fe_buffer::ResolvedSelection;
use fe_gpui::EditorView;
use gpui::{App, Context, Entity, WeakEntity, Window};

use crate::{Pane, Workbench};

/// How many closed editors the stack keeps, as VS Code does: past it, the
/// oldest goes.
pub(crate) const CAPACITY: usize = 20;

/// A window's closed editors, shared by its panes.
pub(crate) type SharedClosedEditors = Rc<RefCell<ClosedEditors>>;

/// The stack: at most [`CAPACITY`] entries, one per file.
#[derive(Default)]
pub(crate) struct ClosedEditors(VecDeque<ClosedEditor>);

impl ClosedEditors {
    /// Puts `closed` on top, in the place of an entry of its file.
    pub(crate) fn push(&mut self, closed: ClosedEditor) {
        self.0.retain(|entry| entry.path != closed.path);
        if self.0.len() == CAPACITY {
            self.0.pop_front();
        }
        self.0.push_back(closed);
    }

    /// Takes the newest entry.
    pub(crate) fn pop(&mut self) -> Option<ClosedEditor> {
        self.0.pop_back()
    }

    /// The entries' files, oldest first.
    #[cfg(test)]
    pub(crate) fn paths(&self) -> Vec<PathBuf> {
        self.0.iter().map(|entry| entry.path.clone()).collect()
    }
}

/// A file editor as it closed.
#[derive(Clone)]
pub(crate) struct ClosedEditor {
    pub path: PathBuf,
    pub view: ViewState,
    /// The pane it was in.
    pub pane: WeakEntity<Pane>,
}

impl ClosedEditor {
    /// `editor` as it is now, in `pane`; `None` for an untitled one.
    pub(crate) fn of(
        editor: &Entity<EditorView>,
        pane: WeakEntity<Pane>,
        cx: &App,
    ) -> Option<Self> {
        let editor = editor.read(cx);
        Some(Self {
            path: editor.model().path()?.to_owned(),
            view: ViewState {
                selections: editor.model().selections(),
                scroll_top: editor.scroll_top(),
            },
            pane,
        })
    }
}

/// What a reopened editor gets back: its selections and how far down it
/// was scrolled.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ViewState {
    pub selections: Vec<ResolvedSelection>,
    /// [`EditorView::scroll_top`].
    pub scroll_top: f32,
}

impl ViewState {
    /// The primary selection, which the reopen reveals first.
    pub(crate) fn primary(&self) -> Option<&ResolvedSelection> {
        self.selections.last()
    }

    /// Restores the selections, clamped to the text as it is now, and the
    /// scroll, which the next frame clamps.
    pub(crate) fn apply(&self, editor: &Entity<EditorView>, cx: &mut App) {
        editor.update(cx, |editor, cx| {
            if !self.selections.is_empty() {
                (editor.model_mut()).set_selections(self.selections.iter().cloned());
            }
            editor.set_scroll_top(self.scroll_top, cx);
        });
    }
}

impl Workbench {
    /// Reopens the newest closed editor: just shows its file's tab if it's
    /// open again (in its old pane first), skips one whose file is gone for
    /// the next, and otherwise opens the file in its old pane, or the
    /// active one if that's closed, with the selections and scroll it had.
    /// Nothing if none is left.
    pub(crate) fn reopen_closed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let closed = self.pane().read(cx).closed_editors().clone();
        let panes = self.panes(cx);
        loop {
            let Some(entry) = closed.borrow_mut().pop() else {
                return;
            };
            let old = entry.pane.entity_id();
            let mut open: Vec<_> = (panes.iter())
                .filter_map(|pane| Some((pane.clone(), pane.read(cx).position(&entry.path)?)))
                .collect();
            open.sort_by_key(|(pane, _)| pane.entity_id() != old);
            if let Some((pane, ix)) = open.into_iter().next() {
                pane.update(cx, |pane, cx| {
                    pane.activate(ix, window, cx);
                    cx.notify();
                });
                self.activate(pane, window, cx);
                return;
            }
            if !entry.path.is_file() {
                continue;
            }
            let pane = (entry.pane.upgrade())
                .filter(|pane| panes.contains(pane))
                .unwrap_or_else(|| self.pane().clone());
            self.activate(pane.clone(), window, cx);
            pane.update(cx, |pane, cx| {
                pane.reopen(entry, window, cx);
                cx.notify();
            });
            return;
        }
    }
}
