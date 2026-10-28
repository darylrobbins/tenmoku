//! Close Saved Editors, Close Editors in Group and Close All Editors (VS
//! Code's `closeUnmodifiedEditors`, `closeEditorsInGroup` and
//! `closeAllEditors`). Saved tabs close at once ([`Pane::close_matching`]);
//! the others close once their tabs agree, as a pane's do
//! ([`kiln_shell::close`]): a dirty one asks whether to discard, unless
//! another tab outside the request shows its buffer, and a Cancel stops
//! the close.

use std::rc::Rc;

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
        let now = scope(&panes, &self.panes(cx), cx);
        let (weak, workbench) = (
            panes.iter().map(Entity::downgrade).collect::<Vec<_>>(),
            cx.weak_entity(),
        );
        let ids: Vec<ItemId> = now.closing.iter().map(|item| item.id()).collect();
        let (live_ids, commit_ids, commit_weak) = (ids.clone(), ids.clone(), weak.clone());
        let live: LiveItems = Rc::new(move |cx| {
            let workbench = workbench.upgrade()?;
            let panes: Vec<_> = weak.iter().filter_map(|pane| pane.upgrade()).collect();
            let mut items = scope(&panes, &workbench.read(cx).panes(cx), cx);
            items.closing.retain(|item| live_ids.contains(&item.id()));
            Some(items)
        });
        let commit = move |window: Option<&mut Window>, cx: &mut App| {
            let Some(window) = window else {
                return;
            };
            for pane in commit_weak.iter().filter_map(|pane| pane.upgrade()) {
                pane.update(cx, |pane, cx| {
                    pane.take_ids(&commit_ids, window, cx);
                    cx.notify();
                });
            }
        };
        let request =
            CloseRequest::new(CloseKind::Items).scope(window.window_handle(), Some(now), live);
        match close::confirm(request, commit, Some(window), cx) {
            Closing::Now => {
                for pane in &panes {
                    pane.update(cx, |pane, cx| {
                        pane.take_ids(&ids, window, cx);
                        cx.notify();
                    });
                }
            }
            Closing::Refused => {}
            Closing::Later(closing) => closing.detach(),
        }
    }
}

/// The items of `closing` panes, and of every pane open.
fn scope(closing: &[Entity<Pane>], open: &[Entity<Pane>], cx: &App) -> Items {
    let items = |panes: &[Entity<Pane>]| -> Vec<Box<dyn ItemHandle>> {
        (panes.iter())
            .flat_map(|pane| pane.read(cx).pane().items().to_vec())
            .collect()
    };
    Items {
        closing: items(closing),
        open: items(open),
    }
}
