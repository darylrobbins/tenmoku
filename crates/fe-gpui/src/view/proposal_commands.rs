//! The proposal commands: next and previous hunk, accept and reject the hunk
//! at the caret, accept all and reject all, registered through fe-command so
//! they get key bindings and the host can run them. The view runs one the
//! editor's registry doesn't know against [`registry`]
//! ([`EditorView::run_command`]); each op calls the host API here.
//!
//! The hunk at the caret is the one whose old rows hold the primary caret's
//! row, or a pure insertion whose block sits directly below that row or
//! starts on it. Only pending and stale hunks count.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{HunkId, HunkStatus, HunkView, Origin, ProposedChange};
use fe_command::{
    Actor, CommandError, CommandTarget, ContextPredicate, Dispatched, Effect, Registry,
};
use gpui::Context;

use super::{EditorView, ProposalEvent};

/// What a proposal command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProposalOp {
    NextHunk,
    PreviousHunk,
    AcceptHunk,
    RejectHunk,
    AcceptAll,
    RejectAll,
}

/// The [`CommandTarget`] of proposal commands: the op to do.
pub(crate) struct ProposalCommands;

impl CommandTarget for ProposalCommands {
    type Tx = ();
    type Target<'a> = Option<ProposalOp>;
}

pub(crate) fn registry() -> Registry<ProposalCommands> {
    use ProposalOp::*;
    let commands = [
        (
            "proposal.next_hunk",
            "Next Proposed Change",
            NextHunk,
            false,
        ),
        (
            "proposal.previous_hunk",
            "Previous Proposed Change",
            PreviousHunk,
            false,
        ),
        (
            "proposal.accept_hunk",
            "Accept Proposed Change",
            AcceptHunk,
            true,
        ),
        (
            "proposal.reject_hunk",
            "Reject Proposed Change",
            RejectHunk,
            false,
        ),
        (
            "proposal.accept_all",
            "Accept All Proposed Changes",
            AcceptAll,
            true,
        ),
        (
            "proposal.reject_all",
            "Reject All Proposed Changes",
            RejectAll,
            false,
        ),
    ];
    let mut registry = Registry::new();
    for (id, title, op, mutates) in commands {
        registry
            .register(
                id,
                title,
                move |target: &mut Option<ProposalOp>, _: &Actor| {
                    *target = Some(op);
                    Ok(Dispatched::new(Effect::Nothing))
                },
            )
            .expect("proposal command IDs are valid and distinct")
            .context(ContextPredicate::identifier(fe_editor::commands::CONTEXT))
            .mutates_buffer(mutates);
    }
    registry
}

/// A pending or stale hunk the view shows.
struct Shown {
    hunk: HunkView,
    /// The row its old text starts on.
    start: u32,
    /// The rows that count as at the caret: its old rows, or for a pure
    /// insertion the row its block sits below and the row it starts on.
    rows: (u32, u32),
}

impl Shown {
    fn at(&self, row: u32) -> bool {
        (self.rows.0..=self.rows.1).contains(&row)
    }
}

impl EditorView {
    /// The pending and stale hunks of the proposals shown, in text order.
    /// A proposal anchored in a later snapshot than the view's has none yet.
    fn open_hunks(&self) -> Vec<Shown> {
        let snapshot = self.model.snapshot();
        let text = self.model.text();
        let mut shown = Vec::new();
        for proposal in &self.proposals {
            let proposal = proposal.borrow();
            if proposal.buffer() != snapshot.id() || proposal.version() > snapshot.version() {
                continue;
            }
            for hunk in proposal.hunks(snapshot) {
                if !matches!(hunk.status, HunkStatus::Pending | HunkStatus::Stale) {
                    continue;
                }
                let start = text.offset_to_point(hunk.range.start).row;
                let end = text.offset_to_point(hunk.range.end);
                let rows = if hunk.range.is_empty() {
                    let below = if end.column == 0 {
                        end.row.checked_sub(1)
                    } else {
                        None
                    };
                    (below.unwrap_or(start).min(start), start)
                } else {
                    let last = if end.column == 0 {
                        end.row - 1
                    } else {
                        end.row
                    };
                    (start, last)
                };
                shown.push(Shown { hunk, start, rows });
            }
        }
        shown.sort_by_key(|shown| shown.hunk.range.start);
        shown
    }

    /// Whether the view shows a pending or stale hunk, which its key context
    /// says with the flag `proposal`.
    pub(crate) fn shows_open_hunk(&self) -> bool {
        !self.open_hunks().is_empty()
    }

    fn caret_row(&self) -> u32 {
        let head = self.model.primary_selection().head();
        self.model.text().offset_to_point(head).row
    }

    /// The hunk at the caret: of those whose old rows hold its row, the
    /// first, else a pure insertion beside it.
    fn hunk_at_caret(&self) -> Option<HunkId> {
        let row = self.caret_row();
        let shown = self.open_hunks();
        let held = |shown: &&Shown| !shown.hunk.range.is_empty() && shown.at(row);
        let found = shown.iter().find(held);
        found
            .or_else(|| shown.iter().find(|shown| shown.at(row)))
            .map(|shown| shown.hunk.id)
    }

    /// Moves to the start of the first hunk on a row after the caret's
    /// (`forward`) or before it, wrapping around the document, with one caret,
    /// and has it scrolled into view. Returns whether there was a hunk.
    fn step_hunk(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        let row = self.caret_row();
        let shown = self.open_hunks();
        let target = if forward {
            let after = shown.iter().find(|shown| shown.start > row);
            after.or(shown.first())
        } else {
            let before = shown.iter().rev().find(|shown| shown.start < row);
            before.or(shown.last())
        };
        let Some(target) = target else {
            return false;
        };
        self.cancel_vim(cx);
        self.composition = None;
        let offset = target.hunk.range.start;
        let start_row = target.start;
        self.model.unfold_rows(start_row..=start_row);
        self.model.set_cursor(offset);
        self.changed(cx);
        true
    }

    /// Moves the caret to the start of the next hunk of the proposals shown,
    /// wrapping around the document. Returns whether there was one.
    pub fn next_hunk(&mut self, cx: &mut Context<Self>) -> bool {
        self.step_hunk(true, cx)
    }

    /// As [`EditorView::next_hunk`], to the previous one.
    pub fn previous_hunk(&mut self, cx: &mut Context<Self>) -> bool {
        self.step_hunk(false, cx)
    }

    /// Accepts every pending hunk of the proposals shown whose old text is
    /// still in place, one transaction of the user's per proposal, and emits
    /// [`ProposalEvent::Accepted`] for each with it. Stale hunks stay.
    /// Returns how many were accepted.
    ///
    /// Like [`EditorView::accept_hunk`], a read-only view accepts for the
    /// host.
    pub fn accept_all(&mut self, cx: &mut Context<Self>) -> usize {
        let mut events = Vec::new();
        let proposals: Vec<Rc<RefCell<ProposedChange>>> = self.proposals.clone();
        for proposal in proposals {
            let snapshot = self.model.snapshot();
            let pending: Vec<HunkId> = {
                let proposal = proposal.borrow();
                if proposal.buffer() != snapshot.id() || proposal.version() > snapshot.version() {
                    continue;
                }
                (proposal.hunks(snapshot).into_iter())
                    .filter(|hunk| hunk.status == HunkStatus::Pending)
                    .map(|hunk| hunk.id)
                    .collect()
            };
            // The guard's drop catches the model up.
            let (transaction, stale) =
                (proposal.borrow_mut()).accept_all_as(&mut self.model.buffer_mut(), Origin::User);
            let Some(transaction) = transaction else {
                continue;
            };
            for hunk in pending.into_iter().filter(|hunk| !stale.contains(hunk)) {
                events.push(ProposalEvent::Accepted { hunk, transaction });
            }
        }
        if events.is_empty() {
            return 0;
        }
        self.sync_find(cx);
        self.sync_completion(cx);
        let accepted = events.len();
        for event in events {
            cx.emit(event);
        }
        cx.notify();
        cx.refresh_windows();
        accepted
    }

    /// Rejects every pending and stale hunk of the proposals shown, leaving
    /// the text be, and emits [`ProposalEvent::Rejected`] for each. Returns
    /// how many.
    pub fn reject_all(&mut self, cx: &mut Context<Self>) -> usize {
        let hunks = self.open_hunks();
        let mut rejected = 0;
        for shown in hunks {
            rejected += usize::from(self.reject_hunk(shown.hunk.id, cx).is_ok());
        }
        rejected
    }

    /// Does `op`, as its command: the effect, or why it failed.
    pub(crate) fn run_proposal_op(
        &mut self,
        op: ProposalOp,
        cx: &mut Context<Self>,
    ) -> Result<Effect, CommandError> {
        let moved = |moved: bool| {
            if moved {
                Effect::Changed
            } else {
                Effect::Nothing
            }
        };
        let done = |done: bool| {
            if done {
                Effect::Notify
            } else {
                Effect::Nothing
            }
        };
        Ok(match op {
            ProposalOp::NextHunk => moved(self.next_hunk(cx)),
            ProposalOp::PreviousHunk => moved(self.previous_hunk(cx)),
            ProposalOp::AcceptHunk => match self.hunk_at_caret() {
                Some(id) => {
                    self.accept_hunk(id, cx)
                        .map_err(|error| CommandError::new(error.to_string()))?;
                    Effect::Notify
                }
                None => Effect::Nothing,
            },
            ProposalOp::RejectHunk => match self.hunk_at_caret() {
                Some(id) => {
                    self.reject_hunk(id, cx)
                        .map_err(|error| CommandError::new(error.to_string()))?;
                    Effect::Notify
                }
                None => Effect::Nothing,
            },
            ProposalOp::AcceptAll => done(self.accept_all(cx) > 0),
            ProposalOp::RejectAll => done(self.reject_all(cx) > 0),
        })
    }
}
