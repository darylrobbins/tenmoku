//! `af if ac ic`: a function or a class (a struct, enum, interface, trait,
//! impl or type too) and its body, from the host's syntax, as
//! nvim-treesitter-textobjects' `@function.outer`/`@function.inner` and
//! `@class.outer`/`@class.inner` give them.
//!
//! The host lists the functions and classes around an offset, innermost
//! first ([`EditorHost::definition_objects`]). `text_object::select` sees
//! only the text, so [`line_motions::with_view`] looks them up around each
//! caret before a command with one of these objects runs, and only then.
//! Without syntax there are none, and the object fails as one nvim doesn't
//! know does: nothing changes.
//!
//! `af` is the `count`th function around the caret, its node, charwise.
//! `if` is the body of the `count`th that has one, without its braces or the
//! blank rows at its edges: linewise when that is whole rows (the braces on
//! rows of their own), else charwise, and an empty range after `{` when the
//! body is empty. `ac` and `ic` are the same for classes. A caret in a
//! class but no function (on a field) has no `af` but an `ac`; one on a
//! function without a body (a trait method's signature) has an `af`, and for
//! `if` the body of the function around it.
//!
//! In a visual mode, over one position the object is the one an operator
//! takes there; else (or over one this object made) the `count`th that
//! holds the selection and is larger than it, so typing the object again
//! grows it to the next enclosing one. The mode becomes charwise.
//!
//! [`EditorHost::definition_objects`]: fe_editor::commands::EditorHost::definition_objects

use std::ops::{Range, RangeInclusive};

use fe_buffer::TransactionId;
use fe_command::{CommandError, Dispatched, Effect};
use fe_editor::commands::SyntaxObject;
use fe_text::Text;

use kiln_l10n::tr;

use crate::{
    Mode, Pending, SelectArgs, TextObject, VimTarget, VisualKind, feature::Feature, line_motions,
    motion::Cursor, operator::Span,
};

pub(crate) const FEATURE: Feature = Feature {
    object,
    ..Feature::NONE
};

/// `f` and `c` after `i` or `a`.
fn object(_pending: &Pending, ch: char, inner: bool) -> Option<TextObject> {
    match (ch, inner) {
        ('f', true) => Some(TextObject::InnerFunction),
        ('f', false) => Some(TextObject::AFunction),
        ('c', true) => Some(TextObject::InnerClass),
        ('c', false) => Some(TextObject::AClass),
        _ => None,
    }
}

/// Whether `object` is one of this module's.
pub(crate) fn is_object(object: TextObject) -> bool {
    matches!(
        object,
        TextObject::InnerFunction
            | TextObject::AFunction
            | TextObject::InnerClass
            | TextObject::AClass
    )
}

/// The ranges `object` takes in `chain` (innermost first): each function's
/// or class's node, or its body, skipping one without; innermost first, the
/// same range only once.
fn ranges(chain: &[SyntaxObject], object: TextObject) -> Vec<Range<usize>> {
    let function = matches!(object, TextObject::InnerFunction | TextObject::AFunction);
    let inner = matches!(object, TextObject::InnerFunction | TextObject::InnerClass);
    let mut ranges: Vec<Range<usize>> = (chain.iter())
        .filter(|found| found.function == function)
        .filter_map(|found| match inner {
            true => found.inner.clone(),
            false => Some(found.outer.clone()),
        })
        .collect();
    ranges.dedup();
    ranges
}

/// What `object` covers around the caret at `offset`, `count` times, as
/// `text_object::select` says.
pub(crate) fn select(text: &Text, offset: usize, object: TextObject, count: u32) -> Span {
    let chain = line_motions::definitions(offset);
    let Some(range) = ranges(&chain, object)
        .into_iter()
        .nth(count.max(1) as usize - 1)
    else {
        return Span::Failed;
    };
    let inner = matches!(object, TextObject::InnerFunction | TextObject::InnerClass);
    match whole_rows(text, &range).filter(|_| inner) {
        Some(rows) => Span::Lines {
            rows,
            first_non_blank: true,
            from: Some(range.start),
        },
        None => Span::Chars(range),
    }
}

/// The rows `range` (not empty) covers if it covers them whole, but for the
/// blanks at their edges.
fn whole_rows(text: &Text, range: &Range<usize>) -> Option<RangeInclusive<u32>> {
    if range.is_empty() {
        return None;
    }
    let blank = |range: Range<usize>| {
        (text.slice_to_string(range).bytes()).all(|byte| byte == b' ' || byte == b'\t')
    };
    let first = text.offset_to_point(range.start).row;
    let end = text.offset_to_point(range.end);
    // Ending at a row's start, it ends with the line break before.
    let last = match end.column {
        0 => end.row - 1,
        _ => end.row,
    };
    let row_end = text.row_start(last) + text.row_len(last) as usize;
    (blank(text.row_start(first)..range.start) && blank(range.end.min(row_end)..row_end))
        .then_some(first..=last)
}

/// The position a visual selection of `range` (not empty) ends at.
fn last(text: &Text, range: &Range<usize>) -> usize {
    match text.offset_to_point(range.end).column {
        0 => range.end - 1,
        _ => Cursor::at(text, range.end - 1).offset(),
    }
}

/// Where the bytes a visual selection ending at position `hi` cover end:
/// past its grapheme, or its row's line break.
fn end_of(text: &Text, hi: usize) -> usize {
    let point = text.offset_to_point(hi);
    if point.column == text.row_len(point.row) {
        return (hi + 1).min(text.len());
    }
    let cursor = Cursor::at(text, hi);
    let column = cursor.line.starts.get(cursor.ix + 1).copied();
    cursor.line.start + column.unwrap_or(cursor.line.text.len())
}

/// `vim.select` with `af if ac ic`: over one position this object didn't
/// make, the `count`th object of the items around the vim cursor, as an
/// operator takes it; else the `count`th that holds the selection and is
/// larger than it. One that's empty, or none, fails.
pub(crate) fn select_visual(
    target: &mut VimTarget<'_>,
    args: SelectArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget { model, state, .. } = target;
    model.sync_display();
    let (Mode::Visual(_), Some(anchor), Some(cursor)) = (
        state.mode,
        state.visual_anchor(model),
        state.visual_cursor(model),
    ) else {
        return Err(CommandError::new(tr!("vim-no-visual-selection")));
    };
    let text = model.text().clone();
    let count = args.count.unwrap_or(1).max(1) as usize;
    let (lo, hi) = (anchor.min(cursor), anchor.max(cursor));
    let selected = lo..end_of(&text, hi);
    let grows = lo != hi || state.visual_by_object();
    let chain = line_motions::definitions(cursor);
    // Over one position, the object of the `count`th item around it, as
    // under an operator: a body needn't hold the cursor (on a function's
    // name). Growing, the `count`th that holds the selection and is larger.
    let found = (ranges(&chain, args.text_object).into_iter())
        .filter(|found| {
            !grows
                || found.start <= selected.start && found.end >= selected.end && *found != selected
        })
        .nth(count - 1);
    let Some(found) = found.filter(|found| !found.is_empty()) else {
        state.failed = true;
        return Ok(Dispatched::new(Effect::Nothing));
    };
    let end = last(&text, &found);
    state.select_visual(model, VisualKind::Char, found.start, end);
    state.set_visual_by_object();
    Ok(Dispatched::new(Effect::Changed))
}
