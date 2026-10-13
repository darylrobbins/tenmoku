//! Proposed changes shown in the editor: each pending or stale hunk of a
//! [`ProposedChange`] as a diff block under the text it replaces, its new
//! text in rows of their own and its old rows banded as removed. Laid out
//! and painted by [`crate::element`]'s `proposal`, with Accept and Reject
//! buttons in each block's header row.
//!
//! Accepting a hunk, by its button or [`EditorView::accept_hunk`], is the
//! user's own edit: one transaction under [`Origin::User`], so `editor.undo`
//! reverts it and `editor.redo` applies it again, while the hunk stays
//! accepted. Each accept or reject is told to subscribers as a
//! [`ProposalEvent`].
//!
//! Proposals are shared, so several views of one buffer (splits) show the
//! same one, and a hunk resolved through one goes from them all.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{HunkId, Origin, ProposalError, ProposedChange, TransactionId};
use gpui::{Bounds, Context, EventEmitter, Pixels, Point};

use super::EditorView;

/// What an [`EditorView`] tells its subscribers of the proposals' hunks the
/// user, or the host through the view, accepted or rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposalEvent {
    /// `hunk`'s new text went in as `transaction`, the user's.
    Accepted {
        hunk: HunkId,
        transaction: TransactionId,
    },
    /// `hunk` was rejected; the text wasn't touched.
    Rejected { hunk: HunkId },
}

impl EventEmitter<ProposalEvent> for EditorView {}

/// A button in a hunk's block's header row.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProposalButton {
    Accept,
    Reject,
}

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

    /// Accepts hunk `id` of a proposal the view shows, as its Accept button
    /// does: its new text goes in as one transaction of the user's, apart
    /// from any open one such as vim's insert session, the carets staying
    /// where they are, and [`ProposalEvent::Accepted`] is emitted.
    ///
    /// # Errors
    /// [`ProposalError::Stale`] if its old text is no longer in place,
    /// [`ProposalError::NotPending`] if it was already accepted or rejected
    /// or no proposal shown holds it. Then nothing changes and nothing is
    /// emitted.
    ///
    /// A read-only view shows no Accept button, but the host may still
    /// accept through it.
    pub fn accept_hunk(
        &mut self,
        id: HunkId,
        cx: &mut Context<Self>,
    ) -> Result<TransactionId, ProposalError> {
        let proposal = self.holding(id).ok_or(ProposalError::NotPending)?;
        // The guard's drop catches the model up.
        let transaction =
            (proposal.borrow_mut()).accept_as(&mut self.model.buffer_mut(), id, Origin::User)?;
        // Caught up with the edit now, not at the next frame.
        self.sync_find(cx);
        self.sync_completion(cx);
        self.resolved(
            ProposalEvent::Accepted {
                hunk: id,
                transaction,
            },
            cx,
        );
        Ok(transaction)
    }

    /// Rejects hunk `id` of a proposal the view shows, pending or stale, as
    /// its Reject button does, leaving the text be, and emits
    /// [`ProposalEvent::Rejected`].
    ///
    /// # Errors
    /// [`ProposalError::NotPending`] if it was already accepted or rejected
    /// or no proposal shown holds it. Then nothing is emitted.
    pub fn reject_hunk(&mut self, id: HunkId, cx: &mut Context<Self>) -> Result<(), ProposalError> {
        let proposal = self.holding(id).ok_or(ProposalError::NotPending)?;
        proposal.borrow_mut().reject(id)?;
        self.resolved(ProposalEvent::Rejected { hunk: id }, cx);
        Ok(())
    }

    /// The proposal shown that holds hunk `id`.
    fn holding(&self, id: HunkId) -> Option<Rc<RefCell<ProposedChange>>> {
        let buffer = self.model.buffer();
        let current = buffer.current();
        (self.proposals.iter())
            .find(|proposal| {
                let hunks = proposal.borrow().hunks(current);
                hunks.iter().any(|hunk| hunk.id == id)
            })
            .cloned()
    }

    /// Tells subscribers of `event`, and has every window drawn again: the
    /// hunk's block goes from every view showing its proposal, in this
    /// window or another.
    fn resolved(&mut self, event: ProposalEvent, cx: &mut Context<Self>) {
        cx.emit(event);
        cx.notify();
        cx.refresh_windows();
    }

    /// Presses the button the last frame painted at `position`, if any,
    /// returning whether there was one. A button whose hunk was resolved
    /// since does nothing.
    pub(crate) fn press_proposal_button(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(&(id, button, _)) = (self.proposal_buttons.iter()).find(
            |(_, _, bounds): &&(HunkId, ProposalButton, Bounds<Pixels>)| bounds.contains(&position),
        ) else {
            return false;
        };
        let pressed = match button {
            // The last frame's button may predate read-only.
            ProposalButton::Accept if self.read_only => return true,
            ProposalButton::Accept => self.accept_hunk(id, cx).map(drop),
            ProposalButton::Reject => self.reject_hunk(id, cx),
        };
        if let Err(error) = pressed {
            log::debug!("{button:?} on hunk {id:?}: {error}");
        }
        true
    }
}
