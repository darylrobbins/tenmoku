//! `ie ae` (the whole buffer, as textobj-entire gives them) and `ii ai aI`
//! (an indent level, as vim-indent-object gives them). All are linewise.
//!
//! `ae` is every row; `ie` every row but the leading and trailing blank
//! ones (all rows if every row is blank). `ii` is the rows around the
//! cursor's row whose indent is at least its, blank rows between them
//! included, none at either end; on a blank row it takes the level of the
//! next non-blank row. `ai` adds the row above, `aI` the rows above and
//! below. A count takes that many levels out: each further level is the
//! indent of the non-blank row above the rows so far.
//!
//! In a visual mode, the object again when the selection already is that
//! level takes the next level out. A tab goes to the next multiple of the tab size.

use std::ops::RangeInclusive;

use fe_buffer::TransactionId;
use fe_command::{CommandError, Dispatched, Effect};
use fe_text::Text;

use kiln_l10n::tr;

use crate::{
    Mode, Pending, SelectArgs, TextObject, VimTarget, VisualKind, feature::Feature, operator::Span,
};

pub(crate) const FEATURE: Feature = Feature {
    object,
    ..Feature::NONE
};

/// `e` and `i` after `i` or `a`, and `I` after `a`.
fn object(_pending: &Pending, ch: char, inner: bool) -> Option<TextObject> {
    Some(match (ch, inner) {
        ('e', true) => TextObject::InnerEntire,
        ('e', false) => TextObject::AEntire,
        ('i', true) => TextObject::InnerIndent,
        ('i', false) => TextObject::AIndent,
        ('I', false) => TextObject::ABigIndent,
        _ => return None,
    })
}

/// Whether `object` is one of this module's.
pub(crate) fn is_object(object: TextObject) -> bool {
    matches!(
        object,
        TextObject::InnerEntire
            | TextObject::AEntire
            | TextObject::InnerIndent
            | TextObject::AIndent
            | TextObject::ABigIndent
    )
}

/// The row's text, without its line break.
fn row_text(text: &Text, row: u32) -> String {
    let start = text.row_start(row);
    text.slice_to_string(start..start + text.row_len(row) as usize)
}

fn blank(text: &Text, row: u32) -> bool {
    row_text(text, row).trim().is_empty()
}

/// A row's indent in columns, a tab going to the next multiple of `tab`.
fn indent(text: &Text, row: u32, tab: usize) -> usize {
    row_text(text, row)
        .chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .fold(0, |column, ch| match ch {
            '\t' => column + tab - column % tab,
            _ => column + 1,
        })
}

/// The rows `object` covers around `row`, with `levels` more levels out for
/// the indent objects (`0` is the level of `row`'s own); `None` if there's
/// nothing to select.
fn rows(
    text: &Text,
    row: u32,
    object: TextObject,
    levels: u32,
    tab: usize,
) -> Option<RangeInclusive<u32>> {
    let last = text.row_count() - 1;
    match object {
        TextObject::AEntire => Some(0..=last),
        TextObject::InnerEntire => {
            let first = (0..=last).find(|&row| !blank(text, row));
            Some(match first {
                None => 0..=last,
                Some(first) => {
                    let end = (first..=last).rev().find(|&row| !blank(text, row))?;
                    first..=end
                }
            })
        }
        _ => {
            let inner = inner_indent(text, row, levels, tab)?;
            let (mut first, mut end) = (*inner.start(), *inner.end());
            if object != TextObject::InnerIndent {
                first = first.saturating_sub(1);
            }
            if object == TextObject::ABigIndent {
                end = (end + 1).min(last);
            }
            Some(first..=end)
        }
    }
}

/// The rows of `ii` around `row`, `levels` further levels out.
fn inner_indent(text: &Text, row: u32, levels: u32, tab: usize) -> Option<RangeInclusive<u32>> {
    let last = text.row_count() - 1;
    // On a blank row, the next non-blank row's level.
    let row = (row..=last).find(|&row| !blank(text, row))?;
    let mut level = indent(text, row, tab);
    let (mut first, mut end) = (row, row);
    for step in 0..=levels {
        if step > 0 {
            // The next level is the nearest non-blank row above's.
            let above = (0..first).rev().find(|&row| !blank(text, row));
            match above {
                Some(above) if indent(text, above, tab) < level => level = indent(text, above, tab),
                _ => break,
            }
        }
        while first > 0 && (blank(text, first - 1) || indent(text, first - 1, tab) >= level) {
            first -= 1;
        }
        while end < last && (blank(text, end + 1) || indent(text, end + 1, tab) >= level) {
            end += 1;
        }
        // No blank rows at either end.
        while first < end && blank(text, first) {
            first += 1;
        }
        while end > first && blank(text, end) {
            end -= 1;
        }
    }
    Some(first..=end)
}

/// What `object` covers around the caret at `offset`, `count` times, as
/// `text_object::select` says.
pub(crate) fn select(
    text: &Text,
    offset: usize,
    object: TextObject,
    count: u32,
    tab: usize,
) -> Span {
    let row = text.offset_to_point(offset).row;
    match rows(text, row, object, count.max(1) - 1, tab) {
        Some(rows) => Span::Lines {
            from: Some(text.row_start(*rows.start())),
            rows,
            first_non_blank: false,
        },
        None => Span::Failed,
    }
}

/// `vim.select` with `ie ae ii ai aI`: the rows of the object around the
/// cursor, selected linewise. Where the selection already is those rows, an
/// indent object takes the next level out; at the outermost it fails.
pub(crate) fn select_visual(
    target: &mut VimTarget<'_>,
    args: SelectArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget { model, state, .. } = target;
    model.sync_display();
    let (Mode::Visual(kind), Some(anchor), Some(cursor)) = (
        state.mode,
        state.visual_anchor(model),
        state.visual_cursor(model),
    ) else {
        return Err(CommandError::new(tr!("vim-no-visual-selection")));
    };
    let text = model.text().clone();
    let row_of = |offset: usize| text.offset_to_point(offset).row;
    let selected = row_of(anchor).min(row_of(cursor))..=row_of(anchor).max(row_of(cursor));
    let tab = model.tab_size().max(1);
    let count = args.count.unwrap_or(1).max(1);
    let indent_object = matches!(
        args.text_object,
        TextObject::InnerIndent | TextObject::AIndent | TextObject::ABigIndent
    );
    let object = args.text_object;
    let found = 'found: {
        // A linewise selection that already is the object at some level: the
        // next level out. Its inner rows are the selection less the header
        // and footer rows the object adds; their shallowest row names the
        // level.
        if indent_object && kind == VisualKind::Line {
            let (first, last) = (*selected.start(), *selected.end());
            let inners = [
                (first, last),
                (first + 1, last),
                (first, last.saturating_sub(1)),
                (first + 1, last.saturating_sub(1)),
            ];
            for (from, to) in inners {
                if from > to {
                    continue;
                }
                let shallowest = (from..=to)
                    .filter(|&row| !blank(&text, row))
                    .min_by_key(|&row| indent(&text, row, tab));
                let Some(base) = shallowest else { continue };
                if rows(&text, base, object, 0, tab).as_ref() == Some(&selected) {
                    break 'found rows(&text, base, object, count, tab)
                        .filter(|next| *next != selected);
                }
            }
        }
        rows(&text, row_of(cursor), object, count - 1, tab)
    };
    let Some(found) = found else {
        state.failed = true;
        return Ok(Dispatched::new(Effect::Nothing));
    };
    state.select_visual(
        model,
        VisualKind::Line,
        text.row_start(*found.start()),
        text.row_start(*found.end()),
    );
    Ok(Dispatched::new(Effect::Changed))
}
