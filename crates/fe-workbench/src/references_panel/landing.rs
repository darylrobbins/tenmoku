//! Where the panel's opens took the caret, so the next open from there
//! isn't recorded in Back history: going through the list, then back,
//! returns at once to where the caret was before.
//!
//! An open in a pane is that pane's destination until the next one there.
//! It's pending while the pane reads its file for it, and lands when the
//! pane applies its reveal to an editor, which [`Pane::revealed`] tells by
//! the open's serial. It's dropped if the reveal never lands: another open
//! in the pane, a tab shown in the read's place, or a failed read. Once
//! landed, it's left for good by the first change to the editor's primary
//! selection, by another open in the pane (Back and Forward included), or by
//! the pane showing anything else in the editor's place: the caret coming
//! back there is the user's choice. Only a pending destination, or a landed
//! one not left since, holds the caret.

use std::{collections::HashMap, path::PathBuf};

use fe_buffer::ResolvedSelection;

use gpui::{App, Context, Entity, EntityId, Subscription};

use super::ReferencesPanel;
use crate::{Pane, Workbench};

/// The destination of the panel's last open in each pane, by pane.
#[derive(Default)]
pub(super) struct Landings(HashMap<EntityId, Destination>);

struct Destination {
    /// The pane's [`Pane::open_serial`] for the open.
    serial: u64,
    /// The file opened.
    path: PathBuf,
    state: State,
    /// The pane while pending, then the editor landed in too.
    watching: Vec<Subscription>,
}

#[derive(Debug, PartialEq, Eq)]
enum State {
    /// The pane is reading the file for this open.
    Pending,
    /// The reveal was applied to `editor`, selecting `selection`, when the
    /// pane's [`Pane::shown_serial`] became `shown`.
    Landed {
        editor: EntityId,
        selection: ResolvedSelection,
        shown: u64,
    },
    /// The editor's selection has changed since it landed.
    Moved,
    /// The reveal will never land.
    Dropped,
}

impl Landings {
    pub(super) fn clear(&mut self) {
        self.0.clear();
    }

    /// Whether the caret of `pane`, the workbench's active one, is where
    /// the panel's last open in it took it, or will take it once read.
    pub(super) fn holds_caret(
        &mut self,
        pane: &Entity<Pane>,
        workbench: &Workbench,
        cx: &App,
    ) -> bool {
        let Some(destination) = self.0.get_mut(&pane.entity_id()) else {
            return false;
        };
        let pane = pane.read(cx);
        destination.follow(pane);
        match &destination.state {
            State::Pending => true,
            State::Landed {
                editor,
                selection,
                shown,
            } => {
                let left =
                    pane.open_serial() != destination.serial || pane.shown_serial() != *shown;
                !left
                    && workbench.active_editor(cx).is_some_and(|active| {
                        active.entity_id() == *editor
                            && active.read(cx).model().primary_selection() == *selection
                    })
            }
            State::Moved | State::Dropped => false,
        }
    }

    /// Makes `pane`'s destination the open of `path` just made in it, if
    /// one was: its [`Pane::open_serial`] was `before`.
    pub(super) fn opened(
        &mut self,
        pane: &Entity<Pane>,
        before: u64,
        path: PathBuf,
        cx: &mut Context<ReferencesPanel>,
    ) {
        let id = pane.entity_id();
        let serial = pane.read(cx).open_serial();
        if serial == before {
            self.0.remove(&id);
            return;
        }
        let watch = cx.observe(pane, move |panel, pane, cx| {
            panel.landings.pane_changed(&pane, cx)
        });
        let destination = Destination {
            serial,
            path,
            state: State::Pending,
            watching: vec![watch],
        };
        self.0.insert(id, destination);
        self.pane_changed(pane, cx);
    }

    /// Follows `pane`'s destination as the pane changes, watching the
    /// editor it lands in.
    fn pane_changed(&mut self, pane: &Entity<Pane>, cx: &mut Context<ReferencesPanel>) {
        let id = pane.entity_id();
        let Some(destination) = self.0.get_mut(&id) else {
            return;
        };
        if destination.state != State::Pending {
            return;
        }
        destination.follow(pane.read(cx));
        let State::Landed { editor, .. } = destination.state else {
            return;
        };
        let tabs = pane.read(cx).tabs().iter();
        let Some(editor) = tabs
            .filter_map(|tab| tab.editor().cloned())
            .find(|tab| tab.entity_id() == editor)
        else {
            destination.state = State::Moved;
            return;
        };
        // It may have moved before the pane said it landed.
        destination.editor_changed(&editor, cx);
        let watch = cx.observe(&editor, move |panel, editor, cx| {
            if let Some(destination) = panel.landings.0.get_mut(&id) {
                destination.editor_changed(&editor, cx);
            }
        });
        destination.watching.push(watch);
    }
}

impl Destination {
    /// Lands or drops a pending destination, as `pane` has it now.
    fn follow(&mut self, pane: &Pane) {
        if self.state != State::Pending {
            return;
        }
        if let Some(revealed) = pane
            .revealed()
            .filter(|revealed| revealed.serial == self.serial)
        {
            self.state = State::Landed {
                editor: revealed.editor,
                selection: revealed.selection.clone(),
                shown: revealed.shown,
            };
        } else if pane.open_serial() != self.serial || !pane.is_opening(&self.path) {
            self.state = State::Dropped;
        }
    }

    /// Moves a landed destination for good once `editor`'s selection
    /// changes.
    fn editor_changed(&mut self, editor: &Entity<fe_gpui::EditorView>, cx: &App) {
        let State::Landed {
            editor: landed,
            selection,
            ..
        } = &self.state
        else {
            return;
        };
        if *landed == editor.entity_id()
            && editor.read(cx).model().primary_selection() != *selection
        {
            self.state = State::Moved;
        }
    }
}
