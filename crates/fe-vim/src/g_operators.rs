//! The `g` operators: `gu gU g~ g? gq gw` (a feature module; see
//! [`crate::feature`]): [`Operator::Lowercase`], [`Operator::Uppercase`] and
//! [`Operator::ToggleCase`] over a motion or object, and
//! [`Operator::Rot13`], [`Operator::Format`] and [`Operator::FormatKeep`].
//!
//! A stub: no key reaches them yet. Over a motion or object every operator
//! but `d y c > <` fails, as it did before this module; over a selection
//! `g?` leaves each char as it is, and `gq` and `gw` fail.

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, Dispatched};

use crate::{OperateArgs, VimTarget, args::Operator, feature::Feature};

pub(crate) const FEATURE: Feature = Feature::NONE;

/// `vim.operate` with an operator other than `d y c > <` over a motion or
/// object.
pub(crate) fn operate(
    _target: &mut VimTarget<'_>,
    _actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    Err(refused(args.operator))
}

/// `gq` or `gw` over a visual selection.
pub(crate) fn format_selection(
    _target: &mut VimTarget<'_>,
    _actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    Err(refused(args.operator))
}

/// `ch` as `g?` leaves it.
pub(crate) fn rot13(ch: char) -> char {
    ch
}

fn refused(operator: Operator) -> CommandError {
    CommandError::new(format!("{operator:?} acts only on a visual selection"))
}
