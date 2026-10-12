//! Prose: the paragraph and sentence motions `{ } ( )` and objects `ip ap
//! is as` (a feature module; see [`crate::feature`]).
//!
//! A stub: no key reaches them yet. Each motion fails as a failed motion
//! does and each object selects nothing, changing nothing.

use fe_text::Text;

use crate::{
    MoveArgs, TextObject,
    args::{Motion, RangeClass},
    feature::Feature,
    motion::Cursor,
    operator::Span,
};

pub(crate) const FEATURE: Feature = Feature::NONE;

/// `motion`'s class as an operator's target ([`Motion::class`]).
pub(crate) fn class(_motion: Motion) -> RangeClass {
    RangeClass::Exclusive
}

/// Whether `motion` is a jump ([`Motion::is_jump`]).
pub(crate) fn is_jump(_motion: Motion) -> bool {
    false
}

/// Whether a delete over `motion` writes register `1` even within a row
/// ([`crate::Target::writes_numbered`]).
pub(crate) fn writes_numbered(_motion: Motion) -> bool {
    false
}

/// Moves one caret by `args.motion`, as `motion::motion` does: `Ok` holds
/// the goal column it keeps, `Err` the goal to keep when it fails.
pub(crate) fn motion(
    _cursor: &mut Cursor,
    _args: MoveArgs,
    goal: Option<usize>,
    _tab: usize,
) -> Result<Option<usize>, Option<usize>> {
    Err(goal)
}

/// What `object` covers around the caret at `offset`, `count` times, as
/// `text_object::select` says.
pub(crate) fn select(_text: &Text, _offset: usize, _object: TextObject, _count: u32) -> Span {
    Span::Failed
}
