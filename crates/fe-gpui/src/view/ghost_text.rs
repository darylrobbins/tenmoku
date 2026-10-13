//! Ghost text: a suggestion the host sets ([`EditorView::set_ghost_text`]),
//! shown dimmed at the caret but not part of the buffer, its first line
//! inline after the caret and its other lines in rows of their own below
//! the caret's row. Laid out and painted by [`crate::element`]'s
//! `ghost_text`.
//!
//! Each view holds its own, as it follows that view's caret. Typing what it
//! suggests consumes it; anything else that ends it
//! ([`fe_editor::ghost_text`]), or leaving vim's insert mode, drops it
//! before the next frame. While an input method composes, the composition
//! takes the caret's inlay and the suggestion hides.

use fe_editor::ghost_text::GhostText;
use gpui::Context;

use super::EditorView;

impl EditorView {
    /// Shows `ghost` from the next frame, in place of any shown before, or
    /// nothing with `None`. One that doesn't show at the caret now, such as
    /// one for another buffer, is dropped with a warning.
    pub fn set_ghost_text(&mut self, ghost: Option<GhostText>, cx: &mut Context<Self>) {
        // Made against the buffer as it is, which the model may trail.
        self.model.sync_display();
        self.ghost_text = ghost.filter(|ghost| {
            let live = ghost.is_live(&self.model);
            if !live {
                let buffer = self.model.snapshot().id();
                log::warn!(
                    "a suggestion that doesn't show at the caret of {buffer:?}, \
                     or is for another buffer, isn't shown"
                );
            }
            live
        });
        cx.notify();
    }

    /// The suggestion shown, if any.
    pub fn ghost_text(&self) -> Option<&GhostText> {
        self.ghost_text.as_ref()
    }

    /// Drops a suggestion that's over, or that vim's normal or visual mode
    /// hides, for a frame about to be drawn.
    pub(crate) fn drop_stale_ghost_text(&mut self) {
        let accepts_text = self.accepts_text();
        if (self.ghost_text.as_ref())
            .is_some_and(|ghost| !accepts_text || !ghost.is_live(&self.model))
        {
            self.ghost_text = None;
        }
    }
}
