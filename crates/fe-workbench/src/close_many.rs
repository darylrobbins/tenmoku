//! Close Saved Editors, Close Editors in Group and Close All Editors (VS
//! Code's `closeUnmodifiedEditors`, `closeEditorsInGroup` and
//! `closeAllEditors`). Saved tabs close at once ([`Pane::close_matching`]);
//! the others close once their tabs agree, as a pane's do
//! ([`kiln_shell::close`]): a dirty one asks whether to discard, unless
//! another tab outside the request shows its buffer, and a Cancel stops
//! the close. Pinned tabs stay open through each ([`Pane::is_pinned`]).
//! File > Close Folder closes them all, pinned ones too, then the folder.

use std::{cell::Cell, path::Path, rc::Rc};

use gpui::{App, Context, Entity, Window};
use kiln_shell::{
    CloseKind, CloseRequest, Closing, ItemHandle, ItemId, PaneView as _,
    close::{self, Items, LiveItems},
};

use crate::{Pane, Workbench};

impl Workbench {
    /// Closes every tab without unsaved changes in the active pane.
    pub(crate) fn close_saved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pane().clone().update(cx, |pane, cx| {
            pane.close_matching(false, |item, cx| !item.is_dirty(cx), window, cx);
            // Else the workspace closes a pane its last tab left.
            pane.pane_mut().take_emptied();
            cx.notify();
        });
    }

    /// Closes every tab of the active pane, or of every pane, keeping the
    /// panes.
    pub(crate) fn close_editors(&mut self, all: bool, window: &mut Window, cx: &mut Context<Self>) {
        let panes = if all {
            self.panes(cx)
        } else {
            vec![self.pane().clone()]
        };
        self.close_tabs(panes, false, |_, _, _| {}, window, cx);
    }

    /// File > Close Folder: closes every tab of every pane, pinned ones
    /// included, asking as Close All Editors does, then leaves the window
    /// with no folder ([`Workbench::close_root`]). A Cancel keeps the
    /// folder and every tab; so does the root changing while it asks. With
    /// no folder open it does nothing.
    pub fn close_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root().map(Path::to_owned) else {
            return;
        };
        let panes = self.panes(cx);
        let then = move |workbench: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            if workbench.root() == Some(root.as_path()) {
                workbench.close_root(window, cx);
            }
        };
        self.close_tabs(panes, true, then, window, cx);
    }

    /// Closes the tabs of `panes`, the pinned ones too if `pinned`, once
    /// they agree, then runs `then`; a Cancel stops both.
    fn close_tabs(
        &mut self,
        panes: Vec<Entity<Pane>>,
        pinned: bool,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = scope(&panes, &self.panes(cx), pinned, cx);
        let (weak, workbench) = (
            panes.iter().map(Entity::downgrade).collect::<Vec<_>>(),
            cx.weak_entity(),
        );
        let ids: Vec<ItemId> = now.closing.iter().map(|item| item.id()).collect();
        let (live_ids, commit_ids, commit_weak) = (ids.clone(), ids.clone(), weak.clone());
        let commit_workbench = workbench.clone();
        // Whichever of the commit and the close now runs it.
        type Then = Box<dyn FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>)>;
        let then: Rc<Cell<Option<Then>>> = Rc::new(Cell::new(Some(Box::new(then))));
        let commit_then = then.clone();
        let live: LiveItems = Rc::new(move |cx| {
            let workbench = workbench.upgrade()?;
            let panes: Vec<_> = weak.iter().filter_map(|pane| pane.upgrade()).collect();
            let mut items = scope(&panes, &workbench.read(cx).panes(cx), pinned, cx);
            items.closing.retain(|item| live_ids.contains(&item.id()));
            Some(items)
        });
        let commit = move |window: Option<&mut Window>, cx: &mut App| {
            let Some(window) = window else {
                return;
            };
            for pane in commit_weak.iter().filter_map(|pane| pane.upgrade()) {
                pane.update(cx, |pane, cx| {
                    if pinned {
                        pane.take_ids(&commit_ids, window, cx);
                    } else {
                        // Not a tab pinned while the close was asking,
                        // which the live items left out too.
                        pane.take_unpinned_ids(&commit_ids, window, cx);
                    }
                    cx.notify();
                });
            }
            if let Some(then) = commit_then.take() {
                (commit_workbench.update(cx, |workbench, cx| then(workbench, window, cx))).ok();
            }
        };
        let request =
            CloseRequest::new(CloseKind::Bulk).scope(window.window_handle(), Some(now), live);
        match close::confirm(request, commit, Some(window), cx) {
            Closing::Now => {
                for pane in &panes {
                    pane.update(cx, |pane, cx| {
                        pane.take_ids(&ids, window, cx);
                        cx.notify();
                    });
                }
                if let Some(then) = then.take() {
                    then(self, window, cx);
                }
            }
            Closing::Refused => {}
            Closing::Later(closing) => closing.detach(),
        }
    }
}

/// The items of `closing` panes, the pinned ones only if `pinned`, and
/// the items of every pane open.
fn scope(closing: &[Entity<Pane>], open: &[Entity<Pane>], pinned: bool, cx: &App) -> Items {
    let items = |panes: &[Entity<Pane>], pinned: bool| -> Vec<Box<dyn ItemHandle>> {
        (panes.iter())
            .flat_map(|pane| {
                let pane = pane.read(cx);
                (pane.pane().items().iter().enumerate())
                    .filter(|(ix, _)| pinned || !pane.is_pinned(*ix))
                    .map(|(_, item)| item.boxed_clone())
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    Items {
        closing: items(closing, pinned),
        open: items(open, true),
    }
}
