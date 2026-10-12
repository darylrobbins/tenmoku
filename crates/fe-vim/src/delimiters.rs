//! Delimiter objects: ``i[ a[ i< a< it at i` a` `` (a feature module; see
//! [`crate::feature`]).
//!
//! A stub: no key reaches them yet. Each selects nothing, changing nothing.

use fe_text::Text;

use crate::{TextObject, feature::Feature, operator::Span};

pub(crate) const FEATURE: Feature = Feature::NONE;

/// What `object` covers around the caret at `offset`, `count` times, as
/// `text_object::select` says.
pub(crate) fn select(_text: &Text, _offset: usize, _object: TextObject, _count: u32) -> Span {
    Span::Failed
}
