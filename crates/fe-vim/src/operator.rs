//! `vim.operate` and `vim.put`: operators over motions and text objects,
//! and putting registers, against an [`EditorModel`](fe_editor::EditorModel).
//!
//! Each caret's target resolves to a [`Span`], adjusted as `:help
//! exclusive` and nvim do. An operator that changes text is one `fe_buffer`
//! transaction for every caret; the registers take the primary (last)
//! caret's text.

use std::ops::{Range, RangeInclusive};

use fe_buffer::{ResolvedSelection, TransactionId};
use fe_command::{Actor, CommandError, Dispatched, Effect};
use fe_editor::{EditorModel, commands::origin};
use fe_text::Text;

use crate::{
    Motion, MoveArgs, OperateArgs, Operator, PutArgs, RangeClass, Register, Registers, Target,
    VimState, VimTarget, Write,
    motion::{self, Cursor},
    text_object,
};
use unicode_segmentation::UnicodeSegmentation;

/// The most text one put may insert.
const MAX_PUT: usize = 64 << 20;

/// What an operator acts on for one caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Span {
    /// These bytes, charwise. Empty, it's nothing to delete (the caret
    /// goes to its start), an empty yank, or a row to shift.
    Chars(Range<usize>),
    /// Whole rows. A delete leaves the caret on the row that moves up, at
    /// its goal column, or at its first non-blank (`first_non_blank`: the
    /// rows of an exclusive range ending at column 0, or of `i{`).
    Lines {
        rows: RangeInclusive<u32>,
        first_non_blank: bool,
        /// Where a charwise selection started, for a yank's caret.
        from: Option<usize>,
    },
    /// A target that failed, though nvim moves the caret here: a word count
    /// past the end of the text, or a `b` stuck at its start.
    Move(usize),
    /// A failed motion or a missing object: nothing happens.
    Failed,
}

/// One caret's resolved target, for the operator.
struct Op {
    /// Bytes removed by a delete (rows with a line break each).
    range: Range<usize>,
    linewise: bool,
    rows: RangeInclusive<u32>,
    first_non_blank: bool,
}

fn row_end(text: &Text, row: u32) -> usize {
    text.row_start(row) + text.row_len(row) as usize
}

/// The first non-blank's column on `offset`'s row is at or after it.
fn at_or_before_first_non_blank(text: &Text, offset: usize) -> bool {
    let row = text.offset_to_point(offset).row;
    text.slice_to_string(text.row_start(row)..offset)
        .bytes()
        .all(|byte| byte == b' ' || byte == b'\t')
}

/// The byte length of the grapheme at `offset` (0 at a row's end).
fn grapheme_len(text: &Text, offset: usize) -> usize {
    let cursor = Cursor::at(text, offset);
    match cursor.offset() == offset && cursor.line.len() > 0 {
        true => cursor.line.grapheme(cursor.ix).len(),
        false => 0,
    }
}

/// Resolves a motion target for the caret at `head`, as an operator takes
/// it.
fn motion_span(
    text: &Text,
    head: usize,
    motion: Motion,
    count: Option<u32>,
    last_find: Option<Motion>,
    tab: usize,
) -> Span {
    let effective = match motion {
        Motion::RepeatFind | Motion::RepeatFindReverse => {
            match motion::repeated_find(last_find, motion == Motion::RepeatFindReverse) {
                Some(find) => find,
                None => return Span::Failed,
            }
        }
        motion => motion,
    };
    let class = match motion {
        Motion::MatchPair if count.is_some() => RangeClass::Linewise,
        _ => effective.class(),
    };
    let mut cursor = Cursor::at(text, head);
    cursor.op = true;
    let args = MoveArgs {
        motion,
        extend: false,
        count,
    };
    if motion::motion(&mut cursor, args, last_find, None, tab).is_err() {
        // A failed `b` leaves the caret where it got to.
        return match motion {
            Motion::WordBackward | Motion::BigWordBackward => Span::Move(cursor.offset()),
            _ => Span::Failed,
        };
    }
    let target = cursor.offset();
    let (lo, hi) = (head.min(target), head.max(target));
    let row = |offset: usize| text.offset_to_point(offset).row;
    let class = match cursor.rows_matched {
        true => RangeClass::Linewise,
        false => class,
    };
    match class {
        RangeClass::Linewise => Span::Lines {
            rows: row(lo)..=row(hi),
            first_non_blank: false,
            from: None,
        },
        RangeClass::Inclusive => Span::Chars(lo..hi + grapheme_len(text, hi)),
        // `w` with an operator stops at the end of the last word it moved
        // over, not on the next row (`:help word`).
        RangeClass::Exclusive if matches!(motion, Motion::WordForward | Motion::BigWordForward) => {
            let word_row = cursor.word_row;
            if !cursor.hit_end && row(target) <= word_row {
                return Span::Chars(head..target);
            }
            // Stuck on an empty last row, there's nothing to take.
            if target == head && text.row_len(word_row) == 0 {
                return Span::Chars(head..head);
            }
            // Ending on an empty row is ending at its column 0
            // (`exclusive-linewise`).
            if word_row >= row(head)
                && text.row_len(word_row) == 0
                && at_or_before_first_non_blank(text, head)
            {
                return Span::Lines {
                    rows: row(head)..=word_row,
                    first_non_blank: true,
                    from: None,
                };
            }
            Span::Chars(head..row_end(text, word_row))
        }
        // `:help exclusive`: an exclusive end at column 0 of a later row
        // moves to the end of the row before; from at or before the first
        // non-blank, the rows are taken whole.
        RangeClass::Exclusive if row(hi) > row(lo) && text.row_start(row(hi)) == hi => {
            if at_or_before_first_non_blank(text, lo) {
                Span::Lines {
                    rows: row(lo)..=row(hi) - 1,
                    first_non_blank: true,
                    from: None,
                }
            } else {
                Span::Chars(lo..row_end(text, row(hi) - 1))
            }
        }
        RangeClass::Exclusive => Span::Chars(lo..hi),
    }
}

/// `:help d`: a charwise delete over several rows, from at or before the
/// first row's first non-blank, with only blanks after it on its last row,
/// takes the rows whole.
fn whole_rows(text: &Text, span: Span) -> Span {
    let Span::Chars(range) = &span else {
        return span;
    };
    let (first, last) = (
        text.offset_to_point(range.start).row,
        text.offset_to_point(range.end).row,
    );
    let blank = |range: Range<usize>| {
        text.slice_to_string(range)
            .bytes()
            .all(|byte| byte == b' ' || byte == b'\t')
    };
    if last > first
        && blank(range.end..row_end(text, last))
        && at_or_before_first_non_blank(text, range.start)
    {
        return Span::Lines {
            rows: first..=last,
            first_non_blank: false,
            from: None,
        };
    }
    span
}

impl Op {
    /// The op for `span`. An empty range is one only if `empty` (a yank
    /// writes the registers even with nothing to take; a delete doesn't).
    fn new(text: &Text, span: &Span, empty: bool) -> Option<Self> {
        let row = |offset: usize| text.offset_to_point(offset).row;
        Some(match span {
            Span::Chars(range) if empty || !range.is_empty() => Self {
                range: range.clone(),
                linewise: false,
                rows: row(range.start)..=row(range.end.max(range.start + 1) - 1),
                first_non_blank: false,
            },
            Span::Lines {
                rows,
                first_non_blank,
                ..
            } => {
                let (first, last) = (*rows.start(), *rows.end());
                let range = if last + 1 < text.row_count() {
                    text.row_start(first)..text.row_start(last + 1)
                } else if first > 0 {
                    row_end(text, first - 1)..text.len()
                } else {
                    0..text.len()
                };
                Self {
                    range,
                    linewise: true,
                    rows: rows.clone(),
                    first_non_blank: *first_non_blank,
                }
            }
            _ => return None,
        })
    }

    /// What the registers take: only the primary caret's is built.
    fn register(&self, text: &Text) -> Register {
        match self.linewise {
            true => {
                let (first, last) = (*self.rows.start(), *self.rows.end());
                let mut lines = text.slice_to_string(text.row_start(first)..row_end(text, last));
                lines.push('\n');
                Register::new(lines, true)
            }
            false => Register::new(text.slice_to_string(self.range.clone()), false),
        }
    }
}

/// Where `offset` lands once `removed` (sorted, disjoint) are deleted.
fn map_through_deletes(offset: usize, removed: &[Range<usize>]) -> usize {
    let mut shift = 0;
    for range in removed {
        if range.end <= offset {
            shift += range.len();
        } else if range.start < offset {
            shift += offset - range.start;
        }
    }
    offset - shift
}

/// Sorts `ranges` and merges the overlapping or touching ones.
fn merge(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

fn caret(offset: usize) -> ResolvedSelection {
    ResolvedSelection {
        range: offset..offset,
        reversed: false,
    }
}

/// The caret on `row` at display column `goal`, or its first non-blank.
fn on_row(text: &Text, row: u32, goal: Option<usize>, tab: usize, first_non_blank: bool) -> usize {
    let mut cursor = Cursor::at(text, text.row_start(row));
    match first_non_blank {
        true => cursor.ix = cursor.line.first_non_blank(),
        false => {
            cursor.vertical(row, goal, tab);
        }
    }
    cursor.offset()
}

/// Where a caret with nothing to act on goes: a [`Span::Move`]'s offset, or
/// an empty range's start.
fn moved_to(span: &Span) -> Option<usize> {
    match span {
        Span::Move(offset) => Some(*offset),
        Span::Chars(range) if range.is_empty() => Some(range.start),
        _ => None,
    }
}

/// Nothing to act on: only the carets [`moved_to`] moves, move.
fn move_only(
    model: &mut EditorModel,
    state: &mut VimState,
    text: &Text,
    before: &[ResolvedSelection],
    spans: &[Span],
) -> Dispatched<TransactionId> {
    let carets = before
        .iter()
        .zip(spans)
        .map(|(selection, span)| match moved_to(span) {
            Some(offset) => caret(Cursor::at(text, offset).offset()),
            None => selection.clone(),
        });
    model.set_selections(carets);
    let moved = model.selections() != before;
    // An operator over nothing still sets the goal column afresh; a failed
    // one doesn't.
    if moved || spans.iter().any(|span| moved_to(span).is_some()) {
        state.goals = None;
    }
    Dispatched::new(if moved {
        Effect::Changed
    } else {
        Effect::Nothing
    })
}

/// `vim.operate`.
pub(crate) fn operate(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    if args.operator == Operator::Change {
        return Err(CommandError::new("change needs insert mode (card 3)"));
    }
    if let Some(name) = args.register {
        Registers::check(name).map_err(|error| CommandError::new(error.to_string()))?;
    }
    let VimTarget {
        model,
        host,
        state,
        registers,
    } = target;
    model.sync_display();
    let text = model.text().clone();
    let tab = model.tab_size().max(1);
    let before = model.selections();
    let goals: Vec<Option<usize>> = {
        let held = motion::valid_goals(state, model.snapshot().version(), &before);
        (0..before.len())
            .map(|ix| held.and_then(|(_, goals)| goals[ix].1))
            .collect()
    };
    let last_find = state.last_find;
    if let Target::Motion(
        find @ (Motion::FindForward(_)
        | Motion::FindBackward(_)
        | Motion::TillForward(_)
        | Motion::TillBackward(_)),
    ) = args.target
    {
        state.last_find = Some(find);
    }
    // Each caret's span, and where a charwise one started, for a yank's
    // caret once it's taken whole rows.
    let (spans, starts): (Vec<Span>, Vec<Option<usize>>) = before
        .iter()
        .map(|selection| {
            let head = selection.head();
            let span = match args.target {
                Target::Motion(motion) => {
                    motion_span(&text, head, motion, args.count, last_find, tab)
                }
                Target::TextObject(object) => {
                    text_object::select(&text, head, object, args.count.unwrap_or(1))
                }
            };
            let start = match (&span, args.target) {
                (Span::Chars(range), _) => Some(range.start),
                // A yank over `%`'s rows goes to the match above.
                (
                    Span::Lines {
                        from: Some(from), ..
                    },
                    _,
                ) => Some(*from),
                (Span::Lines { .. }, Target::Motion(Motion::MatchPair)) if args.count.is_none() => {
                    motion::match_pair(&text, head).map(|found| found.offset.min(head))
                }
                _ => None,
            };
            let span = match args.operator {
                Operator::Delete => whole_rows(&text, span),
                _ => span,
            };
            (span, start)
        })
        .unzip();
    // Each caret's goal column: the one vim keeps, or its own.
    let goals: Vec<Option<usize>> = before
        .iter()
        .zip(goals)
        .map(|(selection, goal)| {
            goal.or_else(|| {
                let cursor = Cursor::at(&text, selection.head());
                Some(cursor.line.goal_column(cursor.ix, tab))
            })
        })
        .collect();
    let row = |text: &Text, offset: usize| text.offset_to_point(offset).row;
    if let Operator::Indent | Operator::Outdent = args.operator {
        // Each caret's rows: an empty range (`>h` at column 0) still has
        // its own.
        let rows: Vec<Option<RangeInclusive<u32>>> = spans
            .iter()
            .map(|span| match span {
                Span::Chars(range) => {
                    let last = range.end.max(range.start + 1) - 1;
                    Some(row(&text, range.start)..=row(&text, last))
                }
                Span::Lines { rows, .. } => Some(rows.clone()),
                Span::Move(_) | Span::Failed => None,
            })
            .collect();
        if rows.iter().all(Option::is_none) {
            return Ok(move_only(model, state, &text, &before, &spans));
        }
        state.goals = None;
        let mut shifted: Vec<u32> = rows
            .iter()
            .flatten()
            .flat_map(|rows| rows.clone())
            .collect();
        shifted.sort_unstable();
        shifted.dedup();
        let indent = args.operator == Operator::Indent;
        let edits: Vec<(Range<usize>, String)> = shifted
            .into_iter()
            .filter_map(|row| shift(&text, row, indent, tab))
            .collect();
        let transaction = model
            .buffer_mut()
            .edit(edits.iter().cloned(), origin(actor));
        let after = model.text().clone();
        // The caret goes to the first row at its goal column; `$` aims it at
        // the row's end.
        let carets = before.iter().zip(&rows).zip(&spans).zip(&goals).map(
            |(((selection, rows), span), goal)| {
                let (at, goal) = match (rows, span) {
                    (Some(rows), _) if args.target == Target::Motion(Motion::LineEnd) => {
                        (*rows.start(), Some(usize::MAX))
                    }
                    (Some(rows), _) => (*rows.start(), *goal),
                    // Where a failed target left it, at the same column.
                    (None, Span::Move(offset)) => {
                        let cursor = Cursor::at(&text, *offset);
                        (cursor.row, Some(cursor.line.goal_column(cursor.ix, tab)))
                    }
                    (None, _) => (row(&text, selection.head()), *goal),
                };
                caret(on_row(&after, at, goal, tab, false))
            },
        );
        model.set_selections(carets);
        let effect = match transaction.is_some() || model.selections() != before {
            true => Effect::Changed,
            false => Effect::Nothing,
        };
        return Ok(Dispatched::new(effect).with_transactions(transaction.into_iter().collect()));
    }
    let yank = args.operator == Operator::Yank;
    let ops: Vec<Option<Op>> = spans
        .iter()
        .map(|span| Op::new(&text, span, yank))
        .collect();
    if ops.iter().all(Option::is_none) {
        return Ok(move_only(model, state, &text, &before, &spans));
    }
    state.goals = None;
    // The registers take the primary (last) caret's text; if its target
    // failed, they're left alone while the other carets still act.
    let primary = ops.last().and_then(Option::as_ref);
    let write = primary.map(|primary| primary.register(&text));
    let write = write.and_then(|value| match args.operator {
        Operator::Yank => Some((Write::Yank, value)),
        Operator::Delete => {
            let small = !value.linewise && !value.text.contains('\n');
            let percent = args.target == Target::Motion(Motion::MatchPair);
            Some((
                Write::Delete {
                    small,
                    numbered: !small || percent,
                },
                value,
            ))
        }
        _ => None,
    });
    if let Some((write, value)) = write {
        registers
            .write(args.register, write, value, &mut **host)
            .map_err(|error| CommandError::new(error.to_string()))?;
    }

    match args.operator {
        Operator::Yank => {
            let carets = before
                .iter()
                .zip(&ops)
                .zip(&spans)
                .zip(&goals)
                .zip(&starts)
                .map(|((((selection, op), span), goal), start)| {
                    let head = selection.head();
                    caret(match (op, span) {
                        (Some(op), _) if op.linewise => match (*op.rows.start(), start) {
                            (_, Some(start)) => Cursor::at(&text, *start).offset(),
                            (first, None) if first == row(&text, head) => head,
                            (first, None) => on_row(&text, first, *goal, tab, false),
                        },
                        (Some(op), _) => Cursor::at(&text, op.range.start).offset(),
                        (None, span) => match moved_to(span) {
                            Some(offset) => Cursor::at(&text, offset).offset(),
                            None => head,
                        },
                    })
                });
            model.set_selections(carets);
            let moved = model.selections() != before;
            Ok(Dispatched::new(if moved {
                Effect::Changed
            } else {
                Effect::Nothing
            }))
        }
        Operator::Delete => {
            let mut removed = merge(ops.iter().flatten().map(|op| op.range.clone()).collect());
            // Rows deleted up to the end of the text take the line break
            // before them, even when merged with rows above (two carets'
            // `dd` on the last two rows).
            // The merged run starts with whole rows at a row's start (not
            // at the line break before, as rows deleted alone up to the end
            // do).
            let leads = |start: usize| {
                ops.iter().flatten().any(|op| {
                    op.linewise
                        && op.range.start == start
                        && *op.rows.start() > 0
                        && text.row_start(*op.rows.start()) == start
                })
            };
            if let Some(last) = removed.last_mut()
                && last.end == text.len()
                && leads(last.start)
                && ops
                    .iter()
                    .flatten()
                    .any(|op| op.linewise && *op.rows.end() + 1 == text.row_count())
            {
                last.start -= 1;
            }
            let transaction = model.buffer_mut().edit(
                removed.iter().map(|range| (range.clone(), "")),
                origin(actor),
            );
            let after = model.text().clone();
            let carets = before.iter().zip(&ops).zip(&spans).zip(&goals).map(
                |(((selection, op), span), goal)| {
                    caret(match (op, span) {
                        (Some(op), _) => {
                            let at = map_through_deletes(op.range.start, &removed);
                            match op.linewise {
                                true => {
                                    on_row(&after, row(&after, at), *goal, tab, op.first_non_blank)
                                }
                                false => Cursor::at(&after, at).offset(),
                            }
                        }
                        (None, span) => {
                            let at = moved_to(span).unwrap_or(selection.head());
                            Cursor::at(&after, map_through_deletes(at, &removed)).offset()
                        }
                    })
                },
            );
            model.set_selections(carets);
            Ok(Dispatched::new(Effect::Changed)
                .with_transactions(transaction.into_iter().collect()))
        }
        Operator::Indent | Operator::Outdent | Operator::Change => {
            unreachable!("shifted or refused above")
        }
    }
}

/// `>>` or `<<` on `row`: its leading blanks rewritten as spaces, one
/// 'shiftwidth' (the tab size) more or less wide. Empty rows are left
/// alone, as are rows whose indent wouldn't change.
fn shift(text: &Text, row: u32, indent: bool, tab: usize) -> Option<(Range<usize>, String)> {
    let line = text.row_to_string(row);
    if line.is_empty() {
        return None;
    }
    let blanks = line
        .bytes()
        .take_while(|&byte| byte == b' ' || byte == b'\t')
        .count();
    let width = line[..blanks].bytes().fold(0, |column, byte| match byte {
        b'\t' => column + tab - column % tab,
        _ => column + 1,
    });
    let width = match indent {
        true => width + tab,
        false => width.saturating_sub(tab),
    };
    let spaces = " ".repeat(width);
    let start = text.row_start(row);
    (spaces != line[..blanks]).then(|| (start..start + blanks, spaces))
}

/// `vim.put`: the register after or before each caret (whole lines below
/// or above its row), `count` times, in one transaction.
pub(crate) fn put(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: PutArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget {
        model,
        host,
        state,
        registers,
    } = target;
    let value = registers
        .read(args.register, &mut **host)
        .map_err(|error| CommandError::new(error.to_string()))?;
    let Some(value) = value.filter(|value| !value.text.is_empty()) else {
        return Ok(Dispatched::new(Effect::Nothing));
    };
    // What the buffer will hold: its line breaks, and lines ending in one
    // even if a host wrote them without.
    let mut payload = value.text.replace("\r\n", "\n").replace('\r', "\n");
    if value.linewise && !payload.ends_with('\n') {
        payload.push('\n');
    }
    model.sync_display();
    let text = model.text().clone();
    let before = model.selections();
    let count = args.count.unwrap_or(1).max(1) as usize;
    let size = payload
        .len()
        .checked_mul(count)
        .and_then(|size| size.checked_mul(before.len()));
    if size.is_none_or(|size| size > MAX_PUT) {
        return Err(CommandError::new(format!(
            "putting {count} copies of {} bytes would insert more than {MAX_PUT} bytes",
            payload.len()
        )));
    }
    let row = |offset: usize| text.offset_to_point(offset).row;
    // Each caret's insertion point, text, and where its caret goes: an
    // offset into the inserted text, or (`true`) the first non-blank of the
    // row starting there.
    let mut puts: Vec<(usize, String, (usize, bool))> = Vec::with_capacity(before.len());
    for selection in &before {
        let head = selection.head();
        let repeated = payload.repeat(count);
        if value.linewise {
            let row = row(head);
            // Below the last row, the rows go after a new line break.
            let (at, inserted, first) = match (args.before, row + 1 < text.row_count()) {
                (true, _) => (text.row_start(row), repeated, 0),
                (false, true) => (text.row_start(row + 1), repeated, 0),
                (false, false) => (
                    text.len(),
                    format!("\n{}", &repeated[..repeated.len() - 1]),
                    1,
                ),
            };
            puts.push((at, inserted, (first, true)));
        } else {
            let at = match args.before {
                true => head,
                false => head + grapheme_len(&text, head),
            };
            // A multi-row text leaves the caret at its start; otherwise on
            // its last grapheme.
            let land = match repeated.contains('\n') {
                true => 0,
                false => repeated
                    .grapheme_indices(true)
                    .next_back()
                    .map_or(0, |(ix, _)| ix),
            };
            puts.push((at, repeated, (land, false)));
        }
    }
    puts.sort_by_key(|(at, _, _)| *at);
    puts.dedup_by_key(|(at, _, _)| *at);
    let transaction = model.buffer_mut().edit(
        puts.iter()
            .map(|(at, inserted, _)| (*at..*at, inserted.clone())),
        origin(actor),
    );
    let after = model.text().clone();
    let mut shift = 0;
    let carets: Vec<ResolvedSelection> = puts
        .iter()
        .map(|(at, inserted, (land, first_non_blank))| {
            let start = at + shift;
            shift += inserted.len();
            let land = start + land;
            caret(match first_non_blank {
                true => on_row(&after, after.offset_to_point(land).row, None, 1, true),
                false => Cursor::at(&after, land).offset(),
            })
        })
        .collect();
    model.set_selections(carets);
    state.goals = None;
    Ok(Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect()))
}
