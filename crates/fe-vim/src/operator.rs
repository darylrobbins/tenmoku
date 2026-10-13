//! `vim.operate` and `vim.put`: operators over motions and text objects,
//! and putting registers, against an [`EditorModel`](fe_editor::EditorModel).
//!
//! Each caret's target resolves to a [`Span`], adjusted as `:help
//! exclusive` and nvim do. An operator that changes text is one `fe_buffer`
//! transaction for every caret; the registers take the primary (last)
//! caret's text.

use std::{
    ops::{Range, RangeInclusive},
    sync::Arc,
};

use fe_buffer::{Anchor, BufferSnapshot, Origin, ResolvedSelection, TransactionId};
use fe_command::{Actor, CommandError, Dispatched, Effect};
use fe_editor::{EditorModel, commands::origin};
use fe_text::{Bias, Text};

use crate::{
    Change, JumpMotion, Motion, MoveArgs, OperateArgs, Operator, PutArgs, RangeClass, Register,
    Registers, Target, VimState, VimTarget, Write, cmdline, g_operators, history, insert,
    line_motions, marks,
    motion::{self, Cursor},
    text_object, visual,
};
use unicode_segmentation::UnicodeSegmentation;

/// The most text one put may insert.
pub(crate) const MAX_PUT: usize = 64 << 20;

/// Applies `edits` with `origin`: the transaction they made, and the
/// snapshot right after them. Catching the model up as the edit ends
/// measures text, which may edit the buffer through another model
/// (`EditorModel::set_char_widths`), so positions in the edited text are
/// taken in this snapshot, not the model's caught-up text: see [`land`].
pub(crate) fn apply<S: Into<Arc<str>>>(
    model: &mut EditorModel,
    edits: impl IntoIterator<Item = (Range<usize>, S)>,
    origin: Origin,
) -> (Option<TransactionId>, BufferSnapshot) {
    let applied = {
        let mut buffer = model.buffer_mut();
        let id = buffer.edit(edits, origin);
        (id, buffer.snapshot())
    };
    #[cfg(test)]
    measured(model, true);
    applied
}

/// [`apply`] for edits in `steps`, each in the text the one before leaves,
/// as one transaction: the model catches up (measuring text) only once the
/// last is in, so nothing edits the buffer between them. Returns the
/// transaction once it closes, if it changed anything.
pub(crate) fn apply_steps(
    model: &mut EditorModel,
    steps: impl IntoIterator<Item = Vec<(Range<usize>, String)>>,
    origin: Origin,
) -> (Option<TransactionId>, BufferSnapshot) {
    let applied = {
        let mut buffer = model.buffer_mut();
        buffer.start_transaction(origin.clone());
        for edits in steps {
            buffer.edit(edits, origin.clone());
        }
        (buffer.end_transaction(), buffer.snapshot())
    };
    #[cfg(test)]
    measured(model, true);
    applied
}

/// Catches the model up (`EditorModel::sync_display`), which measures text
/// as [`apply`] does.
pub(crate) fn sync(model: &mut EditorModel) {
    model.sync_display();
    #[cfg(test)]
    measured(model, false);
}

/// Puts a normal-mode caret at each of `heads`, offsets in `snapshot`, the
/// text right after an edit (see [`apply`]): anchored there, each stays on
/// its grapheme in the text the model caught up to, text put in before it
/// meanwhile included, and lands on the grapheme it's in.
pub(crate) fn land(
    model: &mut EditorModel,
    snapshot: &BufferSnapshot,
    heads: impl IntoIterator<Item = usize>,
) {
    landed_at(model, snapshot, heads);
}

/// [`land`], returning where each caret went, before carets that met
/// merged.
pub(crate) fn landed_at(
    model: &mut EditorModel,
    snapshot: &BufferSnapshot,
    heads: impl IntoIterator<Item = usize>,
) -> Vec<usize> {
    let anchors: Vec<Anchor> = heads
        .into_iter()
        .map(|head| snapshot.anchor_after(head))
        .collect();
    let text = model.text().clone();
    let heads: Vec<usize> = model
        .snapshot()
        .resolve_many(&anchors)
        .into_iter()
        .map(|head| Cursor::at(&text, head).offset())
        .collect();
    model.set_selections(heads.iter().map(|&head| caret(head)));
    heads
}

/// What [`MEASURING`] runs.
#[cfg(test)]
pub(crate) type Measure = Box<dyn FnMut(bool)>;

#[cfg(test)]
thread_local! {
    /// What measuring text does in tests, standing in for char widths,
    /// whose `CharWidth` fe-vim's tests can't build: called with `true` once
    /// [`apply`]'s catch-up ends, and with `false` once [`sync`]'s does. The
    /// model then catches up with whatever it edited, as the catch-up would
    /// have.
    pub(crate) static MEASURING: std::cell::RefCell<Option<Measure>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn measured(model: &mut EditorModel, edited: bool) {
    let Some(mut measuring) = MEASURING.take() else {
        return;
    };
    measuring(edited);
    MEASURING.set(Some(measuring));
    model.sync_display();
}

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
pub(crate) struct Op {
    /// Bytes removed by a delete (rows with a line break each).
    range: Range<usize>,
    linewise: bool,
    rows: RangeInclusive<u32>,
    first_non_blank: bool,
}

pub(crate) fn row_end(text: &Text, row: u32) -> usize {
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
pub(crate) fn grapheme_len(text: &Text, offset: usize) -> usize {
    let cursor = Cursor::at(text, offset);
    match cursor.offset() == offset && cursor.line.len() > 0 {
        true => cursor.line.grapheme(cursor.ix).len(),
        false => 0,
    }
}

/// Resolves a motion target for the caret at `head`, as an operator takes
/// it.
pub(crate) fn motion_span(
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
        // A failed `b` leaves the caret where it got to, as do `ge gE gj gk`
        // if they moved it.
        return match motion {
            Motion::WordBackward | Motion::BigWordBackward => Span::Move(cursor.offset()),
            motion if line_motions::moves_when_failing(motion) && cursor.offset() != head => {
                Span::Move(cursor.offset())
            }
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
        // A search with a line offset lands at column 0, where a yank
        // before the caret leaves it.
        RangeClass::Linewise => Span::Lines {
            rows: row(lo)..=row(hi),
            first_non_blank: false,
            from: matches!(motion, Motion::Search(_)).then_some(lo),
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
    pub(crate) fn new(text: &Text, span: &Span, empty: bool) -> Option<Self> {
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
    pub(crate) fn register(&self, text: &Text) -> Register {
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
pub(crate) fn map_through_deletes(offset: usize, removed: &[Range<usize>]) -> usize {
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
pub(crate) fn merge(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
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

pub(crate) fn caret(offset: usize) -> ResolvedSelection {
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

/// The goal a `gj` or `gk` that failed having moved the caret from `head`
/// keeps where it left it, as nvim keeps its curswant.
pub(crate) fn kept_goal(
    text: &Text,
    head: usize,
    motion: Motion,
    count: Option<u32>,
    tab: usize,
) -> Option<usize> {
    let mut cursor = Cursor::at(text, head);
    cursor.op = true;
    let args = MoveArgs {
        motion,
        extend: false,
        count,
    };
    motion::motion(&mut cursor, args, None, None, tab)
        .err()
        .flatten()
}

/// Once an operator put its carets, by caret at `landed` (the very heads it
/// gave the model, as [`landed_at`] returns them, so any carets that merged
/// share one): the goal a failed
/// `gj` or `gk` keeps there (`kept`, by caret), saved for the carets as
/// they merged. The others' goals are set afresh.
pub(crate) fn keep_goals(
    model: &EditorModel,
    state: &mut VimState,
    landed: &[usize],
    kept: &[Option<usize>],
) {
    state.display_goals = None;
    let kept: Vec<(usize, usize)> = landed
        .iter()
        .zip(kept)
        .filter_map(|(&at, goal)| Some((at, (*goal)?)))
        .collect();
    if kept.is_empty() {
        return;
    }
    let goals = model
        .selections()
        .iter()
        .map(|selection| {
            let goal = kept.iter().find(|(at, _)| *at == selection.head());
            (selection.head(), goal.map(|(_, goal)| *goal))
        })
        .collect();
    state.goals = Some((model.snapshot().version(), goals));
}

/// Nothing to act on: only the carets [`moved_to`] moves, move, each
/// keeping the goal in `kept` if it has one there.
fn move_only(
    model: &mut EditorModel,
    state: &mut VimState,
    text: &Text,
    before: &[ResolvedSelection],
    spans: &[Span],
    kept: &[Option<usize>],
) -> Dispatched<TransactionId> {
    let carets: Vec<ResolvedSelection> = before
        .iter()
        .zip(spans)
        .map(|(selection, span)| match moved_to(span) {
            Some(offset) => caret(Cursor::at(text, offset).offset()),
            None => selection.clone(),
        })
        .collect();
    let landed: Vec<usize> = carets.iter().map(ResolvedSelection::head).collect();
    model.set_selections(carets);
    let moved = model.selections() != before;
    // An operator over nothing still sets the goal column afresh; a failed
    // one doesn't.
    if moved || spans.iter().any(|span| moved_to(span).is_some()) {
        state.goals = None;
        keep_goals(model, state, &landed, kept);
    }
    Dispatched::new(if moved {
        Effect::Changed
    } else {
        Effect::Nothing
    })
}

/// Runs `f`, putting `.`'s last change back if it fails: a command that
/// fails (a register that can't be written, a put too big) isn't a change.
fn keeping_last_change<R>(
    target: &mut VimTarget<'_>,
    f: impl FnOnce(&mut VimTarget<'_>) -> Result<R, CommandError>,
) -> Result<R, CommandError> {
    let last = (target.state.last_change.clone(), target.state.changes);
    let result = f(target);
    if result.is_err() {
        (target.state.last_change, target.state.changes) = last;
    }
    result
}

/// `vim.operate`: over a motion or text object in normal mode, or over the
/// selection in a visual mode (see `visual::operate`). Over a motion or
/// object, an operator but `d y c > <` is `g_operators::operate`'s.
pub(crate) fn operate(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let searching = matches!(
        args.target,
        Target::Motion(
            Motion::Search(_)
                | Motion::SearchWord(_)
                | Motion::SearchNext
                | Motion::SearchPrevious
                | Motion::SearchWordForward
                | Motion::SearchWordBackward
        )
    );
    let mut dispatched = operate_over(target, actor, args)?;
    // A search that changed nothing still set the last search and the
    // highlight.
    if searching && dispatched.effect == Effect::Nothing {
        dispatched.effect = Effect::Notify;
    }
    Ok(dispatched)
}

fn operate_over(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    mut args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    // `n N * #` are the search they run, which `.` finds again when it
    // repeats the operator: the args keep them. A search target is checked
    // as `/` checks it, and a mark's must be set.
    if let Target::Motion(motion) = args.target {
        let checked = cmdline::resolve(target, motion)
            .and_then(|(motion, keep)| cmdline::check_motion_search(target, motion, keep));
        if let Err(message) = checked.and_then(|()| marks::resolve(target, motion).map(drop)) {
            return cmdline::failed(target.state, message);
        }
    }
    // `/` is read-only: an operator that writes a register into it does
    // nothing (leaving a visual mode), as in nvim; the others ignore it.
    if args.register == Some('/') {
        if !matches!(
            args.operator,
            Operator::Delete | Operator::Yank | Operator::Change
        ) {
            args.register = None;
        } else {
            let exited = match args.target.is_selection() {
                true => visual::exit(target)?.effect,
                false => Effect::Nothing,
            };
            target.state.fail("register / is read-only");
            return Ok(Dispatched::new(match exited {
                Effect::Nothing => Effect::Notify,
                exited => exited,
            }));
        }
    }
    if args.target.is_selection() {
        return keeping_last_change(target, |target| visual::operate(target, actor, args));
    }
    if !matches!(
        args.operator,
        Operator::Delete | Operator::Yank | Operator::Change | Operator::Indent | Operator::Outdent
    ) {
        return keeping_last_change(target, |target| g_operators::operate(target, actor, args));
    }
    keeping_last_change(target, |target| {
        operate_unless_failing(target, actor, args, None)
    })
}

/// `d y c > <` over a visual selection's `span`, from one caret at the vim
/// cursor, its goal column held in the state: the operator code as for a
/// motion, without the adjustments motions get (`:help exclusive`). A count
/// is how many times `>` and `<` shift.
pub(crate) fn operate_span(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
    span: Span,
) -> Result<Dispatched<TransactionId>, CommandError> {
    operate_unless_failing(target, actor, args, Some(span))
}

fn operate_unless_failing(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
    selected: Option<Span>,
) -> Result<Dispatched<TransactionId>, CommandError> {
    if let Some(name) = args.register {
        Registers::check(name).map_err(|error| CommandError::new(error.to_string()))?;
    }
    // The target as it's measured: `n N * #` as the search they are, and a
    // mark's motion at the mark.
    let measured = match args.target {
        Target::Motion(motion) => {
            let motion = cmdline::resolve(target, motion).map_or(motion, |(resolved, _)| resolved);
            Target::Motion(marks::resolve(target, motion).unwrap_or(motion))
        }
        other => other,
    };
    let jump = match measured {
        Target::Motion(motion) => marks::jump(target, motion, args.count),
        _ => None,
    };
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
    // A `'` motion's mark, whose row's first non-blank a yank's caret goes
    // to if it's above; `-` goes to the row it lands on as one does.
    let mark = |head: usize| match measured {
        Target::Motion(Motion::MarkAt {
            offset,
            exact: false,
        }) => Some(offset),
        Target::Motion(Motion::PrevLineFirstNonBlank) => {
            let up = args.count.unwrap_or(1).max(1);
            Some(text.row_start(text.offset_to_point(head).row.saturating_sub(up)))
        }
        _ => None,
    };
    // Each caret's span, and where a charwise one started, for a yank's
    // caret once it's taken whole rows.
    let (spans, starts): (Vec<Span>, Vec<Option<usize>>) = before
        .iter()
        .map(|selection| {
            // Another origin's edit may have left the caret inside a
            // grapheme: it's on that grapheme.
            let head = Cursor::at(&text, selection.head()).offset();
            let span = match measured {
                // `cw` on a non-blank is `ce` (`:help cw`).
                Target::Motion(motion @ (Motion::WordForward | Motion::BigWordForward))
                    if args.operator == Operator::Change =>
                {
                    insert::change_word(&text, head, motion, args.count.unwrap_or(1))
                        .unwrap_or_else(|| {
                            motion_span(&text, head, motion, args.count, last_find, tab)
                        })
                }
                Target::Motion(motion) => {
                    motion_span(&text, head, motion, args.count, last_find, tab)
                }
                Target::TextObject(object) => {
                    text_object::select(&text, head, object, args.count.unwrap_or(1))
                }
                Target::Selection | Target::SelectedLines | Target::SelectedToEnd => {
                    selected.clone().unwrap_or(Span::Failed)
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
                (Span::Lines { .. }, _) => mark(head).map(|offset| {
                    let mut cursor = Cursor::at(&text, offset);
                    cursor.ix = cursor.line.first_non_blank();
                    cursor.offset().min(head)
                }),
                _ => None,
            };
            // `gs` is inclusive as a `v` makes a motion (`` dv`z ``), which
            // `:help d`'s exception doesn't take.
            let forced = matches!(
                args.target,
                Target::Motion(Motion::Jump(JumpMotion { forward: true, .. }))
            );
            let span = match args.operator {
                Operator::Delete if selected.is_none() && !forced => whole_rows(&text, span),
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
    // Where a failed `gj` or `gk` moved a caret, it keeps the motion's goal.
    let kept: Vec<Option<usize>> = before
        .iter()
        .zip(&spans)
        .map(|(selection, span)| match (span, measured) {
            (Span::Move(_), Target::Motion(motion @ (Motion::DisplayDown | Motion::DisplayUp))) => {
                let head = Cursor::at(&text, selection.head()).offset();
                kept_goal(&text, head, motion, args.count, tab)
            }
            _ => None,
        })
        .collect();
    let row = |text: &Text, offset: usize| text.offset_to_point(offset).row;
    // `.` repeats an operator whose target resolved, even over nothing (`x`
    // on an empty row); a failed one isn't a change. `c` is kept when its
    // session ends, and a yank never.
    let resolved = spans
        .iter()
        .any(|span| matches!(span, Span::Chars(_) | Span::Lines { .. }));
    state.failed |= !resolved;
    // A jump's target sets the jump mark, which the change then follows.
    if let Some(from) = jump.filter(|_| resolved) {
        state.marks.jumped(from);
    }
    // The carets' ranges that overlap go as one deletion, whose first and
    // last rows alone keep their marks.
    let cuts = merged(&spans);
    if let Operator::Delete | Operator::Change = args.operator {
        for range in &cuts {
            state.marks.cutting(model.snapshot(), range.clone());
        }
        for span in &spans {
            match span {
                Span::Lines { rows, .. } if args.operator == Operator::Delete => {
                    let (first, last) = (*rows.start(), *rows.end());
                    state.marks.deleting(model.snapshot(), first, last);
                }
                _ => {}
            }
        }
    }
    if resolved
        && matches!(
            args.operator,
            Operator::Delete | Operator::Indent | Operator::Outdent
        )
    {
        state.set_last_change(Change::Operate(args).into());
    }
    if args.operator == Operator::Change {
        if spans
            .iter()
            .all(|span| matches!(span, Span::Failed | Span::Move(_)))
        {
            return Ok(move_only(model, state, &text, &before, &spans, &kept));
        }
        for span in &spans {
            if let Span::Lines { rows, .. } = span {
                let (first, last) = (*rows.start(), *rows.end());
                state.marks.changing(model.snapshot(), first, last);
            }
        }
        let starts = cuts.iter().map(|range| range.start);
        state.marks.typing(model.snapshot(), starts);
        return insert::change(
            model,
            &mut **host,
            state,
            registers,
            actor,
            args,
            &before,
            &spans,
        );
    }
    // Where the change begins for each caret, for undo: the start of what it
    // acts on, or for whole rows the first one at the caret's column.
    let begins = |text: &Text, firsts: Vec<Option<(u32, Option<usize>)>>| -> Vec<(u32, usize)> {
        before
            .iter()
            .zip(firsts)
            .map(|(selection, first)| {
                let (row, column) = history::pair(text, selection.head());
                match first {
                    Some((first, None)) => history::caret_on_row(text, first, column),
                    Some((first, Some(start))) => (first, start - text.row_start(first)),
                    None => (row, column),
                }
            })
            .collect()
    };
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
            return Ok(move_only(model, state, &text, &before, &spans, &kept));
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
        let times = match selected {
            Some(_) => args.count.unwrap_or(1).max(1) as usize,
            None => 1,
        };
        let edits: Vec<(Range<usize>, String)> = shifted
            .into_iter()
            .filter_map(|row| shift(&text, row, indent, tab, tab.saturating_mul(times)))
            .collect();
        let (transaction, snapshot) = apply(model, edits, origin(actor));
        if let Some(id) = transaction {
            let firsts = rows
                .iter()
                .map(|rows| rows.as_ref().map(|rows| (*rows.start(), None)))
                .collect();
            state.carets.record(id, begins(&text, firsts));
        }
        // `'[` and `']` span the rows, from where the shift began: a
        // selection's start, else the caret's column.
        let (first, last) = rows
            .iter()
            .flatten()
            .fold((u32::MAX, 0), |(first, last), rows| {
                (first.min(*rows.start()), last.max(*rows.end()))
            });
        let began = spans.iter().zip(&starts).zip(&before).find_map(
            |((span, start), selection)| match span {
                Span::Chars(range) => Some(range.start),
                Span::Lines { .. } => Some(start.unwrap_or(selection.head())),
                Span::Move(_) | Span::Failed => None,
            },
        );
        let column = began.map_or(0, |at| history::pair(&text, at).1);
        marks::shifted(
            state,
            model,
            transaction.map(|_| snapshot.version()),
            (first, last),
            column,
        );
        let after = snapshot.text().clone();
        // The caret goes to the first row at its goal column; `$` aims it at
        // the row's end.
        let carets: Vec<usize> = before
            .iter()
            .zip(&rows)
            .zip(&spans)
            .zip(&goals)
            .map(|(((selection, rows), span), goal)| {
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
                on_row(&after, at, goal, tab, false)
            })
            .collect();
        let landed = landed_at(model, &snapshot, carets);
        keep_goals(model, state, &landed, &kept);
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
        return Ok(move_only(model, state, &text, &before, &spans, &kept));
    }
    state.goals = None;
    // The registers take the primary (last) caret's text; if its target
    // failed, they're left alone while the other carets still act.
    let primary = ops.last().and_then(Option::as_ref);
    let write = primary.map(|primary| primary.register(&text));
    // A delete on a buffer a linewise delete emptied has nothing to take.
    let emptied = state.is_emptied(model);
    let write = write.and_then(|value| match args.operator {
        Operator::Yank => Some((Write::Yank, value)),
        Operator::Delete if emptied => None,
        Operator::Delete => {
            let small = !value.linewise && !value.text.contains('\n');
            let percent = args.target.writes_numbered();
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
            // A yank edits nothing, so it sets `'[` and `']` itself.
            if let Some(op) = primary {
                let yanked = match op.linewise {
                    true => marks::Yanked::Rows(*op.rows.start(), *op.rows.end()),
                    false => marks::Yanked::Chars {
                        start: op.range.start,
                        last: text.clip_offset(op.range.end.max(1) - 1, Bias::Left),
                    },
                };
                marks::yanked(state, model, yanked);
            }
            let carets: Vec<usize> = before
                .iter()
                .zip(&ops)
                .zip(&spans)
                .zip(&goals)
                .zip(&starts)
                .map(|((((selection, op), span), goal), start)| {
                    let head = selection.head();
                    match (op, span) {
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
                    }
                })
                .collect();
            model.set_selections(carets.iter().map(|&at| caret(at)));
            keep_goals(model, state, &carets, &kept);
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
            let (transaction, snapshot) = apply(
                model,
                removed.iter().map(|range| (range.clone(), "")),
                origin(actor),
            );
            if let Some(id) = transaction {
                let firsts = ops
                    .iter()
                    .map(|op| {
                        op.as_ref().map(|op| match op.linewise {
                            true => (*op.rows.start(), None),
                            false => (row(&text, op.range.start), Some(op.range.start)),
                        })
                    })
                    .collect();
                state.carets.record(id, begins(&text, firsts));
                // Where whole rows' delete began, for `'[`: a selection's
                // start, else the caret's column.
                let began =
                    ops.iter()
                        .zip(&starts)
                        .zip(&before)
                        .find_map(|((op, start), selection)| {
                            op.as_ref().filter(|op| op.linewise)?;
                            Some(start.unwrap_or(selection.head()))
                        });
                let note = match began {
                    Some(at) => marks::Note::Deleted {
                        column: history::pair(&text, at).1,
                    },
                    None => marks::Note::Chars,
                };
                state.marks.note(snapshot.version(), note);
            }
            let after = snapshot.text().clone();
            if ops.iter().flatten().any(|op| op.linewise) && after.is_empty() {
                state.emptied = Some(snapshot.version());
            }
            let carets: Vec<usize> = before
                .iter()
                .zip(&ops)
                .zip(&spans)
                .zip(&goals)
                .map(|(((selection, op), span), goal)| match (op, span) {
                    (Some(op), _) => {
                        let at = map_through_deletes(op.range.start, &removed);
                        match op.linewise {
                            true => on_row(&after, row(&after, at), *goal, tab, op.first_non_blank),
                            false => Cursor::at(&after, at).offset(),
                        }
                    }
                    (None, span) => {
                        let at = moved_to(span).unwrap_or(selection.head());
                        Cursor::at(&after, map_through_deletes(at, &removed)).offset()
                    }
                })
                .collect();
            let landed = landed_at(model, &snapshot, carets);
            keep_goals(model, state, &landed, &kept);
            Ok(Dispatched::new(Effect::Changed)
                .with_transactions(transaction.into_iter().collect()))
        }
        _ => unreachable!("shifted or changed above, or refused"),
    }
}

/// `>>` or `<<` on `row`: its leading blanks rewritten as spaces, `by`
/// columns (a 'shiftwidth', the tab size, per shift) more or less wide.
/// Empty rows are left alone, as are rows whose indent wouldn't change.
fn shift(
    text: &Text,
    row: u32,
    indent: bool,
    tab: usize,
    by: usize,
) -> Option<(Range<usize>, String)> {
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
        true => width.saturating_add(by),
        false => width.saturating_sub(by),
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
    keeping_last_change(target, |target| put_unless_failing(target, actor, args))
}

fn put_unless_failing(
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
    if args.register == Some('/') && registers.last_search().is_none() {
        return Ok(state.fail("E35: No previous regular expression"));
    }
    let value = registers
        .read(args.register, &mut **host)
        .map_err(|error| CommandError::new(error.to_string()))?;
    state.set_last_change(Change::Put(args).into());
    let Some(value) = value.filter(|value| !value.text.is_empty() || value.block_width.is_some())
    else {
        // nvim's E353 (`_` holds nothing, without an error).
        state.failed |= args.register != Some('_');
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
    let carets_before = history::pairs(model);
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
    if let Some(width) = value.block_width {
        // A block goes in once, from the primary caret's column: after its
        // grapheme for `p` (on an empty row, at its start), or at it for `P`.
        let head = Cursor::at(&text, model.primary_selection().head());
        let tab = model.tab_size().max(1);
        let column = match (args.before, head.line.len()) {
            (false, len) if len > 0 => {
                let start = head.line.start_column(head.ix, tab);
                start + head.line.width(head.ix, start, tab)
            }
            _ => head.line.start_column(head.ix, tab),
        };
        if visual::put_size(&payload, (width, count), column, tab).is_none_or(|size| size > MAX_PUT)
        {
            return Err(CommandError::new(format!(
                "putting {count} copies of a {width} column block would insert more than {MAX_PUT} bytes"
            )));
        }
        let rows: Vec<&str> = payload.split('\n').collect();
        let (edits, [start, ..]) =
            visual::put_block(&text, (head.row, column), &rows, (width, count), tab);
        let (transaction, snapshot) = apply(model, edits, origin(actor));
        if let Some(id) = transaction {
            state.carets.record(id, carets_before);
        }
        land(model, &snapshot, [start]);
        state.goals = None;
        return Ok(
            Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect())
        );
    }
    let row = |offset: usize| text.offset_to_point(offset).row;
    // Each caret's insertion point, text, and where its caret goes: an
    // offset into the inserted text, or (`true`) the first non-blank of the
    // row starting there.
    let mut puts: Vec<(usize, String, (usize, bool))> = Vec::with_capacity(before.len());
    for selection in &before {
        let head = Cursor::at(&text, selection.head()).offset();
        let repeated = payload.repeat(count);
        if value.linewise {
            let row = row(head);
            // As `O` does, the rows go above the marks of the row they go
            // in at.
            match args.before {
                true => state.marks.opening(model.snapshot(), row),
                false if row + 1 < text.row_count() => {
                    state.marks.opening(model.snapshot(), row + 1)
                }
                false => {}
            }
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
    let (transaction, snapshot) = apply(
        model,
        puts.iter()
            .map(|(at, inserted, _)| (*at..*at, inserted.as_str())),
        origin(actor),
    );
    if let Some(id) = transaction {
        state.carets.record(id, carets_before);
        // The rows the first put in, for `'[` and `']`.
        if let (true, Some((at, _, (first, _)))) = (value.linewise, puts.first()) {
            let row = row(*at) + *first as u32;
            let rows = (payload.matches('\n').count() * count) as u32;
            let rows = (row, row + rows.max(1) - 1);
            state
                .marks
                .note(snapshot.version(), marks::Note::Put { rows });
        }
    }
    let after = snapshot.text().clone();
    let mut shift = 0;
    let carets: Vec<usize> = puts
        .iter()
        .map(|(at, inserted, (land, first_non_blank))| {
            let start = at + shift;
            shift += inserted.len();
            let land = start + land;
            match first_non_blank {
                true => on_row(&after, after.offset_to_point(land).row, None, 1, true),
                false => Cursor::at(&after, land).offset(),
            }
        })
        .collect();
    land(model, &snapshot, carets);
    state.goals = None;
    Ok(Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect()))
}

/// The carets' character ranges, those overlapping or touching as the one
/// deletion they make, in order.
fn merged(spans: &[Span]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = spans
        .iter()
        .filter_map(|span| match span {
            Span::Chars(range) => Some(range.clone()),
            _ => None,
        })
        .collect();
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}
