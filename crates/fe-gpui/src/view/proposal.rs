//! Proposed changes shown in the editor: each pending or stale hunk of a
//! [`ProposedChange`] as a diff block under the text it replaces, its new
//! text in rows of their own and its old rows banded as removed. Laid out
//! and painted by [`crate::element`]'s `proposal`. Read-only: accepting and
//! rejecting is the host's, through the proposal.
//!
//! Proposals are shared, so several views of one buffer (splits) show the
//! same one, and a hunk resolved through one goes from them all.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::ProposedChange;
use gpui::Context;

use super::EditorView;

impl EditorView {
    /// Shows `proposals` from the next frame, in place of those shown
    /// before. One made against another buffer is left out, with a warning.
    pub fn set_proposals(
        &mut self,
        mut proposals: Vec<Rc<RefCell<ProposedChange>>>,
        cx: &mut Context<Self>,
    ) {
        let buffer = self.model.snapshot().id();
        proposals.retain(|proposal| {
            let theirs = proposal.borrow().buffer();
            if theirs != buffer {
                log::warn!("a proposal for buffer {theirs:?} isn't shown in a view of {buffer:?}");
            }
            theirs == buffer
        });
        self.proposals = proposals;
        cx.notify();
    }

    /// The proposals shown.
    pub fn proposals(&self) -> &[Rc<RefCell<ProposedChange>>] {
        &self.proposals
    }
}
