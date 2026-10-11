//! Jump: `gs{c1}{c2}` and `gS{c1}{c2}`, the two-char jump as a motion (a
//! feature module; see [`crate::feature`]).
//!
//! In normal mode, the visual modes and after an operator, `gs` and `gS`
//! (with whatever count, register or operator is pending) wait for two chars
//! (`vim.pending_kind` `jump`; `<Esc>` cancels with nothing changed), then
//! commit `vim.move` (extending in a visual mode) or `vim.operate` with the
//! motion `{"jump": {"chars": "ab", "forward": true}}` ([`JumpMotion`]) and
//! the pending count. It goes to the `count`th match after the caret
//! (`gs`), or before it (`gS`), over the whole text with no wrap, landing on
//! `{c1}`; [`label::targets`] says what matches. With no such match it
//! fails as a failed motion does: nothing changes, not even the jump mark.
//!
//! As an operator's target `gs` is inclusive and `gS` exclusive, as `f` and
//! `F` are, and a delete writes register `1` even within a row, as after
//! `` ` ``. Both are jumps, setting `''`. `.` finds the match again from
//! the caret it repeats at.

pub mod label;

use std::ops::ControlFlow;

use fe_text::Text;

use crate::{
    Pending, Step, VimKey, VimTarget,
    args::{JumpChars, JumpMotion, Motion},
    feature::Feature,
    motion::Cursor,
    operator,
};

pub(crate) const FEATURE: Feature = Feature { g, ..Feature::NONE };

/// `gs` or `gS`, the chars typed so far.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct JumpPrefix {
    forward: bool,
    first: Option<char>,
}

impl JumpPrefix {
    /// The next typed char: the prefix still waiting for the second, or
    /// the motion once both are in.
    pub(crate) fn then(self, ch: char) -> ControlFlow<Motion, Self> {
        match self.first {
            None => ControlFlow::Continue(Self {
                first: Some(ch),
                ..self
            }),
            Some(first) => ControlFlow::Break(Motion::Jump(JumpMotion {
                chars: JumpChars([first, ch]),
                forward: self.forward,
            })),
        }
    }
}

/// `s` and `S` after `g`.
fn g(pending: &Pending, key: VimKey) -> Option<Step> {
    let forward = match key {
        VimKey::Char('s') => true,
        VimKey::Char('S') => false,
        _ => return None,
    };
    let prefix = JumpPrefix {
        forward,
        first: None,
    };
    Some(Step::Wait(pending.clone().jumping(prefix)))
}

/// The `count`th match of `jump` from `caret`, over the whole text.
pub(crate) fn nth(
    text: &Text,
    caret: usize,
    jump: JumpMotion,
    count: Option<u32>,
) -> Option<usize> {
    let n = count.unwrap_or(1).max(1) as usize;
    label::matches(text, 0..text.row_count(), caret, jump.chars.0, jump.forward).nth(n - 1)
}

/// Whether `motion`, if it's a jump, finds its match from the vim cursor or
/// primary caret: one that doesn't goes nowhere and sets no jump mark.
pub(crate) fn lands(target: &mut VimTarget<'_>, motion: Motion, count: Option<u32>) -> bool {
    let Motion::Jump(jump) = motion else {
        return true;
    };
    target.model.sync_display();
    let visual = target.state.visual_cursor(target.model);
    let head = visual.unwrap_or_else(|| target.model.primary_selection().head());
    let text = target.model.text();
    let cursor = Cursor::at(text, head);
    // A visual cursor at a row's end (after `$`) jumps from there.
    let caret = match visual.is_some() && head == operator::row_end(text, cursor.row) {
        true => head,
        false => cursor.offset(),
    };
    nth(text, caret, jump, count).is_some()
}
