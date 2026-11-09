//! Peek References (`editor.peek_references`, shift-F12), as VS Code's
//! editor.action.goToReferences: the uses of the word at the caret open in
//! the peek under the caret's line, a list of them on the left grouped by
//! file, and the selected one's source on the right, highlighted.
//!
//! The list is a [`ReferencesPanel`] of the peek's own, which runs the
//! query Find All References runs and appends its results as they arrive;
//! the peek's targets follow its rows and the excerpt its selection. The
//! list has focus: Up and Down move the selection, a click selects too, and
//! Enter or a double-click opens the selected reference in the pane, at the
//! excerpt's selection once it shows, and closes the peek. Its locations
//! are where the answer put them, as the References panel opens them. A
//! search over with nothing found closes the peek, saying so in a toast;
//! with no symbol at the caret or no folder open, none opens, and a toast
//! says that. In a diff's side, which the pane doesn't draw a peek over, it
//! finds all references in the panel instead.

use gpui::{AppContext as _, Context, Entity, Focusable as _, Subscription, Window};

use super::{PeekView, Target, close};
use crate::{
    ReferencesPanel, Workbench,
    references_panel::{OpenPeeked, word_at},
    workbench::toast,
};

#[cfg(test)]
thread_local! {
    /// The next peek's search holds its batches on this, in tests.
    pub(crate) static HOLD_REFERENCES: std::cell::RefCell<Option<async_channel::Receiver<()>>> =
        const { std::cell::RefCell::new(None) };
}

impl Workbench {
    /// `editor.peek_references`: see the [module docs](self).
    pub(crate) fn peek_references(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_peek(cx) {
            return self.find_references(window, cx);
        }
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        // A go-to-definition still running would take focus from the peek.
        self.nav_state.goto.next();
        let not_found =
            |workbench: &mut Self, status: &str, window: &mut Window, cx: &mut Context<Self>| {
                workbench.close_peek_in(&editor, window, cx);
                toast::show(cx, toast::Toast::info(status.to_owned()));
            };
        let Some(ready) = self.nav_state.start(cx) else {
            return not_found(self, "No folder open", window, cx);
        };
        let request = self.nav_request(cx);
        let word = (request.as_ref())
            .and_then(|request| word_at(&request.text, request.offset, request.language));
        let (Some(request), Some(word)) = (request, word) else {
            return not_found(self, "No symbol at the caret", window, cx);
        };
        let workbench = cx.entity().downgrade();
        let panel = cx.new(|cx| ReferencesPanel::peeked(workbench, cx));
        #[cfg(test)]
        if let Some(hold) = HOLD_REFERENCES.with(|hold| hold.borrow_mut().take()) {
            panel.update(cx, |panel, _| panel.test.hold = Some(hold));
        }
        self.open_peek_on(&editor, Vec::new(), Some(panel.clone()), window, cx);
        let navigation = self.navigation().clone();
        panel.update(cx, |panel, cx| {
            panel.run(request, word, ready, navigation, cx)
        });
    }
}

impl Workbench {
    /// Closes the peeks showing references, cancelling their searches: the
    /// workspace root changed, and their locations are relative to the old
    /// one.
    pub(crate) fn close_reference_peeks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for pane in self.panes(cx) {
            let peek = pane.read(cx).peek.as_ref().map(|peek| peek.view.clone());
            let Some(view) = peek else {
                continue;
            };
            let Some(list) = view.read(cx).references.clone() else {
                continue;
            };
            let focused = view.update(cx, |view, cx| {
                view.closed = true;
                view.focus.contains_focused(window, cx)
                    || list.focus_handle(cx).contains_focused(window, cx)
            });
            list.update(cx, |list, cx| list.clear(cx));
            close(&pane, focused, window, cx);
        }
    }
}

impl PeekView {
    /// Follows `panel`, the peek's list: its rows and selection, and its
    /// opens.
    pub(super) fn follow(
        panel: &Entity<ReferencesPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        vec![
            cx.observe_in(panel, window, Self::references_changed),
            cx.subscribe_in(panel, window, |view, _, _: &OpenPeeked, window, cx| {
                view.open_reference(window, cx)
            }),
        ]
    }

    /// The list changed: references found since go on the targets, and the
    /// one selected shows; with the search over and none found, the peek
    /// closes, saying so.
    fn references_changed(
        &mut self,
        panel: Entity<ReferencesPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closed {
            return;
        }
        let list = panel.read(cx);
        if let Some(status) = list.found_none() {
            toast::show(cx, toast::Toast::info(status));
            return self.close(window, cx);
        }
        let found = (list.found().skip(self.targets.len()))
            .map(|location| Target::at(location.clone()))
            .collect::<Vec<_>>();
        let selected = list.selected_reference();
        self.read_streamed(&found, window, cx);
        self.targets.extend(found);
        if let Some(ix) = selected
            && (ix != self.selected || self.path.is_none())
        {
            self.reselect(ix, window, cx);
        }
    }

    /// Opens the selected reference, as Enter in the list does: at the
    /// excerpt's selection, or where it is if the excerpt is still being
    /// read.
    fn open_reference(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.excerpt.is_some() {
            return self.open(window, cx);
        }
        let Some(target) = self.targets.get(self.selected) else {
            return;
        };
        let location = target.location.clone();
        self.closed = true;
        let pane = self.pane.upgrade();
        (self.workbench)
            .update(cx, |workbench, cx| {
                if let Some(pane) = pane {
                    close(&pane, false, window, cx);
                }
                workbench.list_jump(true, &location, window, cx);
            })
            .ok();
    }

    /// Peek References' list.
    #[cfg(test)]
    pub(crate) fn references(&self) -> Option<&Entity<ReferencesPanel>> {
        self.references.as_ref()
    }
}

#[cfg(test)]
#[path = "peek_references_tests.rs"]
mod peek_references_tests;
