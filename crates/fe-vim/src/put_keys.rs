//! `Y`, `gp`, `gP`, `]p`, `[p`, `]P` and `[P`, a feature module; see [`crate::feature`].
//!
//! `Y` is nvim's default mapping `y$` (`:help Y-default`): it yanks to the
//! line's end, count and register kept. `gp` and `gP` (`:help gp`) put as `p`
//! and `P` do, leaving the caret just after the new text.
//! `]p` and its kin put lines with the indent of the caret's row (`:help ]p`).

use crate::{
    Pending, Step, VimKey,
    args::{Motion, Operator, PutArgs, Target},
    feature::Feature,
    reducer::invocation,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    g,
    bracket: Some(bracket),
    ..Feature::NONE
};

/// `Y` in normal mode, not after an operator.
fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    if key != VimKey::Char('Y') || pending.is_visual() || pending.has_operator() {
        return None;
    }
    Some(
        pending
            .clone()
            .operate(Operator::Yank, Target::Motion(Motion::LineEnd)),
    )
}

/// `p` and `P` after `g`.
fn g(pending: &Pending, key: VimKey) -> Option<Step> {
    let before = match key {
        VimKey::Char('p') => false,
        VimKey::Char('P') => true,
        _ => return None,
    };
    if pending.is_visual() || pending.has_operator() {
        return None;
    }
    Some(invocation(
        "vim.put",
        PutArgs {
            before,
            cursor_after: true,
            indent: false,
            count: pending.count(),
            register: pending.register(),
        },
    ))
}

/// `p` after `]`, and `p`, `P` after `[` (and `P` after `]`): the lines put
/// with their indent moved to the caret row's; a charwise register is put
/// as `p` or `P` do.
fn bracket(pending: &Pending, forward: bool, key: VimKey) -> Option<Step> {
    let before = match (forward, key) {
        (true, VimKey::Char('p')) => false,
        (_, VimKey::Char('p' | 'P')) => true,
        _ => return None,
    };
    if pending.is_visual() || pending.has_operator() {
        return None;
    }
    Some(invocation(
        "vim.put",
        PutArgs {
            before,
            cursor_after: false,
            indent: true,
            count: pending.count(),
            register: pending.register(),
        },
    ))
}
