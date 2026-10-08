//! The visual modes, `v` and `V`: `vim.visual`, `vim.visual_exit`,
//! `vim.visual_swap`, `vim.reselect`, `vim.select`, `vim.move` in a visual
//! mode, and the operators over the selection (`Target::Selection` and
//! `Target::SelectedLines`).
//!
//! The vim cursor and the anchor are positions: a grapheme's start, or a
//! row's end, where `$`, `l` and a vertical move past the row's last
//! grapheme leave the cursor. A charwise selection takes the grapheme at
//! each end, and at a row's end its line break. They're kept as anchors, and
//! the model's selection mirrors them, so the editor paints and copies what
//! vim would take.
//!
//! Leaving a visual mode keeps its extent for `gv`, `'<` and `'>` as
//! [`Mark`]s, which hold a byte column on a row as nvim's do.

use std::ops::Range;

use fe_buffer::{Anchor, BufferSnapshot, ResolvedSelection, TransactionId};
use fe_command::{Actor, CommandError, Dispatched, Effect};
use fe_editor::{EditorModel, commands::origin};
use fe_text::Text;

use crate::{
    Change, CountArgs, Mode, Motion, MoveArgs, OperateArgs, Operator, PutArgs, Register, Registers,
    SelectArgs, SwapArgs, Target, TextObject, VimState, VimTarget, VisualArgs, VisualKind, Write,
    edit, history, insert,
    motion::{self, Class, Cursor, Line},
    operator::{self, Span, row_end},
    text_object,
};

/// A visual mode's ends, as anchors (see [`Ends`]).
#[derive(Clone, Debug)]
pub(crate) struct Visual {
    anchor: Anchor,
    cursor: Anchor,
    /// The cursor's goal column (`usize::MAX` after `$`), as in normal mode.
    goal: Option<usize>,
}

/// A position as nvim keeps a mark: a byte column on a row. The row is
/// anchored at its start, so it follows rows put in or taken out above it
/// and moves up with a row deleted or joined to the one above; edits within
/// the row leave the column alone.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Pos {
    row: Anchor,
    column: usize,
    /// On the row after the anchor's, past the end of the text: where nvim
    /// keeps a mark on rows a visual change deleted at the end.
    past_end: bool,
}

/// `'<` or `'>`: the start or end of the last visual extent. Like nvim's,
/// it's whichever end of it comes first or last when it's read, as edits may
/// have moved them past each other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    ends: [Pos; 2],
    end: bool,
    linewise: bool,
}

impl Mark {
    /// Its row and byte column in `snapshot` (the editor's, or a newer one),
    /// as nvim's `getpos()` reports them: the column may lie past the row's
    /// end, and a linewise extent's `'<` is at its row's column 0 and its
    /// `'>` on its row's last byte.
    pub fn position(&self, snapshot: &BufferSnapshot) -> (u32, usize) {
        let [a, b] = [&self.ends[0], &self.ends[1]].map(|pos| pos.position(snapshot));
        let (row, column) = if self.end { a.max(b) } else { a.min(b) };
        let text = snapshot.text();
        let last = |row| match row < text.row_count() {
            true => (text.row_len(row) as usize).saturating_sub(1),
            false => 0,
        };
        match (self.linewise, self.end) {
            (true, false) => (row, 0),
            (true, true) => (row, last(row)),
            (false, _) => (row, column),
        }
    }
}

impl Pos {
    fn at(snapshot: &BufferSnapshot, offset: usize) -> Self {
        let text = snapshot.text();
        let row = text.offset_to_point(offset).row;
        Self {
            row: snapshot.anchor_after(text.row_start(row)),
            column: offset - text.row_start(row),
            past_end: false,
        }
    }

    fn position(&self, snapshot: &BufferSnapshot) -> (u32, usize) {
        let row = snapshot
            .text()
            .offset_to_point(snapshot.resolve(&self.row))
            .row;
        (row + u32::from(self.past_end), self.column)
    }

    /// The visual position it stands for in `snapshot`.
    fn offset(&self, snapshot: &BufferSnapshot) -> usize {
        let text = snapshot.text();
        let (row, column) = self.position(snapshot);
        let row = row.min(text.row_count() - 1);
        let column = column.min(text.row_len(row) as usize);
        position(text, text.row_start(row) + column)
    }
}

/// The last visual mode's extent: `gv` selects it again, and `'<` and `'>`
/// are its start and end.
#[derive(Clone, Debug)]
pub(crate) struct LastVisual {
    kind: VisualKind,
    start: Pos,
    end: Pos,
    /// The cursor was the start.
    cursor_first: bool,
    goal: Option<usize>,
}

/// How much text the last change over a selection took, for `.`
/// (`:help visual-repeat`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Extent {
    /// This many whole rows.
    Lines(u32),
    /// Charwise over `rows` rows. `last` is how many display columns it
    /// took on the last row (from the start, on one row; from the row's
    /// start otherwise), a row's end being one; `None` after `$`, to the
    /// row's end.
    Chars { rows: u32, last: Option<usize> },
    /// A block of this many rows and display columns; `None` after `$`, to
    /// each row's end.
    Block { rows: u32, columns: Option<usize> },
}

/// A visual mode's ends as offsets.
#[derive(Copy, Clone, Debug)]
struct Ends {
    kind: VisualKind,
    anchor: usize,
    cursor: usize,
    goal: Option<usize>,
}

impl Ends {
    fn lo(&self) -> usize {
        self.anchor.min(self.cursor)
    }

    fn hi(&self) -> usize {
        self.anchor.max(self.cursor)
    }

    /// The rows it covers.
    fn rows(&self, text: &Text) -> (u32, u32) {
        let row = |offset| text.offset_to_point(offset).row;
        (row(self.lo()), row(self.hi()))
    }

    /// The text it selects: charwise, from its start to the end of what's at
    /// its end; otherwise its rows without the last line break.
    fn range(&self, text: &Text, kind: VisualKind) -> Range<usize> {
        match kind {
            VisualKind::Char => self.lo()..end_of(text, self.hi()),
            VisualKind::Line | VisualKind::Block => {
                let (first, last) = self.rows(text);
                text.row_start(first)..row_end(text, last)
            }
        }
    }

    /// The model's selections mirroring it: its range, or a block's text on
    /// each row (a caret at the cursor if it takes none).
    fn selections(&self, text: &Text, tab: usize) -> Vec<ResolvedSelection> {
        let reversed = self.cursor < self.anchor;
        let ranges = match self.kind {
            VisualKind::Block => Block::of(text, self, tab)
                .cuts(text, tab)
                .into_iter()
                .map(|cut| cut.range)
                .filter(|range| !range.is_empty())
                .collect(),
            kind => vec![self.range(text, kind)],
        };
        match ranges.is_empty() {
            true => vec![operator::caret(self.cursor)],
            false => ranges
                .into_iter()
                .map(|range| ResolvedSelection { range, reversed })
                .collect(),
        }
    }
}

/// A caret at `offset` that may stand on its row's end: a visual position.
fn cursor_at(text: &Text, offset: usize) -> Cursor<'_> {
    let mut cursor = Cursor::at(text, offset);
    if offset == row_end(text, cursor.row) {
        cursor.ix = cursor.line.len();
    }
    cursor
}

/// The visual position holding `offset`: its grapheme's start, or its row's
/// end.
fn position(text: &Text, offset: usize) -> usize {
    cursor_at(text, offset).offset()
}

/// The end of what a selection takes at position `offset`: its grapheme, or
/// at a row's end its line break (none on the last row).
fn end_of(text: &Text, offset: usize) -> usize {
    let cursor = cursor_at(text, offset);
    match cursor.ix < cursor.line.len() {
        true => offset + cursor.line.grapheme(cursor.ix).len(),
        false if cursor.row + 1 < text.row_count() => offset + 1,
        false => offset,
    }
}

/// The display column a caret at this position keeps: a row's end is past
/// its last column.
fn goal_of(cursor: &Cursor, tab: usize) -> usize {
    match cursor.ix == cursor.line.len() {
        true => cursor.line.start_column(cursor.ix, tab),
        false => cursor.line.goal_column(cursor.ix, tab),
    }
}

/// The visual ends in the model's text, if a visual mode is on.
fn ends(model: &EditorModel, state: &VimState) -> Option<Ends> {
    let Mode::Visual(kind) = state.mode else {
        return None;
    };
    let visual = state.visual.as_ref()?;
    let snapshot = model.snapshot();
    let text = model.text();
    Some(Ends {
        kind,
        anchor: position(text, snapshot.resolve(&visual.anchor)),
        cursor: position(text, snapshot.resolve(&visual.cursor)),
        goal: visual.goal,
    })
}

/// Enters (or stays in) a visual mode with `ends`, the model's selection
/// mirroring it. Several carets collapse to this one.
fn select(model: &mut EditorModel, state: &mut VimState, ends: Ends) {
    let snapshot = model.snapshot();
    state.mode = Mode::Visual(ends.kind);
    state.visual = Some(Visual {
        anchor: snapshot.anchor_after(ends.anchor),
        cursor: snapshot.anchor_after(ends.cursor),
        goal: ends.goal,
    });
    state.goals = None;
    let selections = ends.selections(model.text(), model.tab_size().max(1));
    model.set_selections(selections);
}

/// Leaves the visual mode, keeping its extent for `gv`, `'<` and `'>`
/// (unless `.` entered it: as in nvim, `.` leaves them alone).
fn leave(model: &EditorModel, state: &mut VimState, ends: Ends) {
    let snapshot = model.snapshot();
    let (start, end) = (Pos::at(snapshot, ends.lo()), Pos::at(snapshot, ends.hi()));
    if !state.replaying {
        state.last_visual = Some(LastVisual {
            kind: ends.kind,
            start,
            end,
            cursor_first: ends.cursor < ends.anchor,
            goal: ends.goal,
        });
    }
    state.mode = Mode::Normal;
    state.visual = None;
}

/// A normal-mode caret at the vim cursor, keeping its goal column.
fn to_normal(model: &mut EditorModel, state: &mut VimState, ends: Ends) {
    let caret = Cursor::at(model.text(), ends.cursor).offset();
    model.set_selections([operator::caret(caret)]);
    state.goals = Some((model.snapshot().version(), vec![(caret, ends.goal)]));
}

/// The goal column normal mode holds for the primary caret, if any.
fn held_goal(model: &EditorModel, state: &VimState) -> Option<usize> {
    let selections = model.selections();
    motion::valid_goals(state, model.snapshot().version(), &selections)
        .and_then(|(_, goals)| goals.last()?.1)
}

fn effect(changed: bool) -> Result<Dispatched<TransactionId>, CommandError> {
    Ok(Dispatched::new(match changed {
        true => Effect::Changed,
        false => Effect::Nothing,
    }))
}

/// `vim.visual`: from normal mode, enters the visual mode of `kind` at the
/// primary caret; from the other visual kind, switches to it; from this
/// kind, leaves it.
pub(crate) fn visual(
    target: &mut VimTarget<'_>,
    args: VisualArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget { model, state, .. } = target;
    model.sync_display();
    match ends(model, state) {
        Some(ends) if ends.kind == args.kind => {
            leave(model, state, ends);
            to_normal(model, state, ends);
        }
        Some(ends) => select(
            model,
            state,
            Ends {
                kind: args.kind,
                ..ends
            },
        ),
        None => {
            let goal = held_goal(model, state);
            let head = Cursor::at(model.text(), model.primary_selection().head()).offset();
            let ends = Ends {
                kind: args.kind,
                anchor: head,
                cursor: head,
                goal,
            };
            select(model, state, ends);
        }
    }
    effect(true)
}

/// `vim.visual_exit`: back to normal mode, the caret on the vim cursor,
/// aiming at the column it's at, as nvim's `Esc` leaves it (after `$` too).
pub(crate) fn exit(target: &mut VimTarget<'_>) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget { model, state, .. } = target;
    model.sync_display();
    let Some(ends) = ends(model, state) else {
        return effect(false);
    };
    leave(model, state, ends);
    to_normal(model, state, Ends { goal: None, ..ends });
    effect(true)
}

/// `vim.visual_swap`: the cursor goes to the other end; in a block with
/// `same_row` (`O`), to the other corner on its row, the anchor taking the
/// cursor's column.
pub(crate) fn swap(
    target: &mut VimTarget<'_>,
    args: SwapArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget { model, state, .. } = target;
    model.sync_display();
    let Some(ends) = ends(model, state) else {
        return effect(false);
    };
    let mut swapped = Ends {
        anchor: ends.cursor,
        cursor: ends.anchor,
        goal: None,
        ..ends
    };
    if ends.kind == VisualKind::Block && args.same_row {
        let text = model.text();
        let tab = model.tab_size().max(1);
        // The cursor's column is the one it aims at, past a short row's end.
        let span = |offset: usize| {
            let at = cursor_at(text, offset);
            let start = at.line.start_column(at.ix, tab);
            match at.ix < at.line.len() {
                true => (start, start + at.line.width(at.ix, start, tab).max(1) - 1),
                false => (start, start),
            }
        };
        let (anchor, anchor_end) = span(ends.anchor);
        let (cursor, cursor_end) = span(ends.cursor);
        let (left, right) = (cursor.min(anchor), cursor_end.max(anchor_end));
        // Which side the cursor is on goes by the last column it aims at.
        let aim = ends.goal.filter(|&goal| goal != usize::MAX);
        let aim_end = aim.map_or(cursor_end, |aim| aim.max(cursor_end));
        let (cursor, anchor) = match aim_end < anchor_end {
            true => (right, left),
            false => (left, right),
        };
        let on = |offset: usize, column: usize| {
            let row = text.offset_to_point(offset).row;
            match column < width_from(&text.row_to_string(row), 0, tab) {
                true => at_column(text, row, column, tab),
                false => row_end(text, row),
            }
        };
        swapped.cursor = on(ends.cursor, cursor);
        // A cursor its row is too short to move on stays at the row's end,
        // the column it then aims at, and leaves the anchor too.
        let row = text.offset_to_point(ends.cursor).row;
        let width = width_from(&text.row_to_string(row), 0, tab);
        let stays = cursor >= width && swapped.cursor == ends.cursor;
        swapped.goal = Some(if stays { width } else { cursor });
        swapped.anchor = match stays {
            true => ends.anchor,
            false => on(ends.anchor, anchor),
        };
    }
    select(model, state, swapped);
    effect(swapped.cursor != ends.cursor)
}

/// `vim.reselect`: the last visual extent and kind again. In a visual mode
/// the current extent becomes the last, as nvim swaps them. Nothing happens
/// with no last extent.
pub(crate) fn reselect(
    target: &mut VimTarget<'_>,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget { model, state, .. } = target;
    model.sync_display();
    let Some(last) = state.last_visual.clone() else {
        return effect(false);
    };
    if let Some(ends) = ends(model, state) {
        leave(model, state, ends);
    }
    let snapshot = model.snapshot();
    let (start, end) = (last.start.offset(snapshot), last.end.offset(snapshot));
    let (anchor, cursor) = match last.cursor_first {
        true => (end, start),
        false => (start, end),
    };
    let ends = Ends {
        kind: last.kind,
        anchor,
        cursor,
        goal: last.goal,
    };
    select(model, state, ends);
    effect(true)
}

/// `vim.move` in a visual mode: moves the vim cursor, keeping the anchor
/// whatever `extend` says. As nvim's, the cursor may go to a row's end: `l`
/// past the last grapheme, `$`, and a vertical move whose goal column is past
/// the row's text.
pub(crate) fn move_cursor(model: &mut EditorModel, state: &mut VimState, args: MoveArgs) -> bool {
    model.sync_display();
    let Some(ends) = ends(model, state) else {
        return false;
    };
    let text = model.text().clone();
    let tab = model.tab_size().max(1);
    let mut cursor = cursor_at(&text, ends.cursor);
    let goal = Some(ends.goal.unwrap_or_else(|| goal_of(&cursor, tab)));
    let n = args.count.unwrap_or(1).max(1) as usize;
    let vertical = match args.motion {
        Motion::Up | Motion::Down | Motion::DocumentStart | Motion::DocumentEnd => true,
        Motion::MatchPair => args.count.is_some(),
        _ => false,
    };
    // Whether a word motion ran into the end of the text.
    let mut ran_out = false;
    let moved = match args.motion {
        Motion::Right => match cursor.ix.saturating_add(n).min(cursor.line.len()) {
            ix if ix == cursor.ix => Err(goal),
            ix => {
                cursor.ix = ix;
                Ok(None)
            }
        },
        // Back from a row's end, the whole row is before the cursor.
        Motion::Left
        | Motion::FindForward(_)
        | Motion::FindBackward(_)
        | Motion::TillForward(_)
        | Motion::TillBackward(_)
        | Motion::RepeatFind
        | Motion::RepeatFindReverse => {
            motion::motion(&mut cursor, args, state.last_find, goal, tab)
        }
        // From a row's end, the word ending the row is the first one back.
        Motion::WordBackward | Motion::BigWordBackward
            if cursor.ix == cursor.line.len() && cursor.ix > 0 =>
        {
            let big = args.motion == Motion::BigWordBackward;
            cursor.ix -= 1;
            let class = cursor.class(big);
            let starts = cursor.ix == 0 || cursor.class_on_row(cursor.ix - 1, big) != Some(class);
            let steps = match class != Class::Blank && starts {
                true => n - 1,
                false => n,
            };
            cursor.words(Motion::WordBackward, steps as u32, big);
            Ok(None)
        }
        Motion::WordEnd | Motion::BigWordEnd => {
            cursor.ix = cursor.ix.min(cursor.line.last());
            let big = args.motion == Motion::BigWordEnd;
            for _ in 0..n {
                let before = (cursor.row, cursor.ix);
                cursor.words(Motion::WordEnd, 1, big);
                // Stopping on a blank, it ran out of words.
                if (cursor.row, cursor.ix) == before || cursor.class(big) == Class::Blank {
                    ran_out = true;
                    break;
                }
            }
            Ok(None)
        }
        _ => {
            cursor.ix = cursor.ix.min(cursor.line.last());
            let moved = motion::motion(&mut cursor, args, state.last_find, goal, tab);
            ran_out = cursor.hit_end;
            moved
        }
    };
    // Past the row's text, or the text's, the cursor stands on the row's end.
    let past = |goal| vertical && goal >= cursor.line.start_column(cursor.line.len(), tab);
    match moved {
        Ok(Some(goal)) if args.motion == Motion::LineEnd || past(goal) => {
            cursor.ix = cursor.line.len()
        }
        Ok(_) if ran_out => cursor.ix = cursor.line.len(),
        // A failed motion leaves the cursor where it was.
        Err(_) => cursor = cursor_at(&text, ends.cursor),
        _ => {}
    }
    if let Motion::FindForward(_)
    | Motion::FindBackward(_)
    | Motion::TillForward(_)
    | Motion::TillBackward(_) = args.motion
    {
        state.last_find = Some(args.motion);
    }
    // A goal set afresh on a tab is its first column if the cursor is
    // before the anchor, as nvim shows it there, else its last.
    let goal = moved.unwrap_or_else(|kept| kept).or_else(|| {
        Some(match cursor.offset() < ends.anchor {
            true => cursor.line.start_column(cursor.ix, tab),
            false => goal_of(&cursor, tab),
        })
    });
    let moved_to = Ends {
        cursor: cursor.offset(),
        goal,
        ..ends
    };
    select(model, state, moved_to);
    moved_to.cursor != ends.cursor
}

/// The positions a text object's span takes: its start and the position its
/// end is taken at. `None` for a failed object.
fn object_ends(text: &Text, span: Span, object: TextObject) -> Option<(usize, usize)> {
    use TextObject::*;
    match span {
        // An empty inside: nothing for `i(` and `i{`, the quotes for `i"`,
        // the position itself for a word object (an empty row).
        Span::Chars(range) if range.is_empty() => match object {
            InnerParen | InnerBrace => None,
            InnerDoubleQuote | InnerSingleQuote => Some((range.start, range.start - 1)),
            _ => Some((range.start, range.start)),
        },
        // `i(` and `i{` end at a row's end when the bracket starts the next
        // row: a visual selection takes that row's end too.
        Span::Chars(range)
            if matches!(object, TextObject::InnerParen | TextObject::InnerBrace)
                && range.end < text.len()
                && range.end == row_end(text, text.offset_to_point(range.end).row) =>
        {
            Some((range.start, range.end))
        }
        Span::Chars(range) => {
            // Ending at a row's start, a word object ends on that (empty)
            // row; any other ends with a line break, at the row's end before.
            let words = matches!(
                object,
                TextObject::InnerWord
                    | TextObject::AWord
                    | TextObject::InnerBigWord
                    | TextObject::ABigWord
            );
            let last = match text.offset_to_point(range.end).column == 0 {
                true if words => range.end,
                true => range.end - 1,
                false => Cursor::at(text, range.end - 1).offset(),
            };
            Some((range.start, last))
        }
        Span::Lines { rows, .. } => {
            Some((text.row_start(*rows.start()), row_end(text, *rows.end())))
        }
        _ => None,
    }
}

/// `vim.select`: a text object typed in a visual mode. Over one position it
/// selects the object there; otherwise a word object adds `count` more from
/// the cursor on, the way it faces, and any other selects the smallest one
/// around the selection that's larger than it. The mode becomes charwise.
pub(crate) fn select_object(
    target: &mut VimTarget<'_>,
    args: SelectArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    use TextObject::*;
    let VimTarget { model, state, .. } = target;
    model.sync_display();
    let Some(ends) = ends(model, state) else {
        return effect(false);
    };
    let text = model.text().clone();
    let count = args.count.unwrap_or(1).max(1);
    let object = |offset: usize, count: u32| {
        let span = text_object::select(&text, offset, args.text_object, count);
        object_ends(&text, span, args.text_object)
    };
    let words = matches!(
        args.text_object,
        InnerWord | AWord | InnerBigWord | ABigWord
    );
    let forward = ends.cursor > ends.anchor;
    let big = matches!(args.text_object, InnerBigWord | ABigWord);
    let mut kind = VisualKind::Char;
    let inner = matches!(args.text_object, InnerWord | InnerBigWord);
    // A row's end, or a last row that's empty: `iw` grows back from it.
    let at_end = {
        let at = cursor_at(&text, ends.cursor);
        match at.line.len() {
            0 => at.row > 0 && at.row + 1 == text.row_count(),
            len => at.ix == len,
        }
    };
    let (anchor, cursor) = if ends.anchor == ends.cursor && inner && at_end {
        (
            ends.anchor,
            back_inner_words(&text, ends.cursor, count, big, true),
        )
    } else if ends.anchor == ends.cursor {
        match object(ends.cursor, 1) {
            // More `iw` than one go on as the selection grows.
            Some((start, _))
                if count > 1 && matches!(args.text_object, InnerWord | InnerBigWord) =>
            {
                let end = inner_words(&text, ends.cursor, count, big, false);
                (start, end.unwrap_or_else(|| position(&text, text.len())))
            }
            Some(_) if count > 1 => match object(ends.cursor, count) {
                Some(found) => found,
                None => return effect(false),
            },
            Some(found) => found,
            None => return effect(false),
        }
    } else if !forward && matches!(args.text_object, AWord | ABigWord) {
        (ends.anchor, back_a_words(&text, ends.cursor, count, big))
    } else if !forward && inner {
        // At the start of the text there's nothing to grow onto.
        match back_inner_words(&text, ends.cursor, count, big, false) {
            back if back == ends.cursor => return effect(false),
            back => (ends.anchor, back),
        }
    } else if forward && inner {
        match inner_words(&text, ends.cursor, count, big, true) {
            Some(end) => (ends.anchor, end),
            // Past the end of the text, onto the last row's end.
            None => {
                kind = ends.kind;
                (ends.anchor, position(&text, text.len()))
            }
        }
    } else if words {
        let mut from = cursor_at(&text, ends.cursor);
        let stepped = match forward {
            true => from.forward(),
            false => from.backward(),
        };
        match object(from.offset(), count).filter(|_| stepped) {
            Some((start, _)) if !forward => (ends.anchor, start),
            Some((_, last)) => (ends.anchor, last),
            // Forward past the end of the text, onto the last row's end.
            None if forward => {
                kind = ends.kind;
                (ends.anchor, position(&text, text.len()))
            }
            None => return effect(false),
        }
    } else {
        // The object around the cursor, or if that's the selection, the
        // next one out (for `i"`, with its quotes), if it's larger.
        let covers = |(start, last): (usize, usize)| (start, end_of(&text, last));
        let current = (ends.lo(), end_of(&text, ends.hi()));
        let quotes = !matches!(args.text_object, InnerParen | AParen | InnerBrace | ABrace);
        let found = object(ends.cursor, count).and_then(|found| match covers(found) == current {
            true => object(ends.cursor, count.saturating_add(1)),
            false => (!quotes).then_some(found),
        });
        match found.map(|found| (found, covers(found))) {
            Some((found, (start, end)))
                if start <= current.0 && end >= current.1 && (start, end) != current =>
            {
                found
            }
            _ => return effect(false),
        }
    };
    let selected = Ends {
        kind,
        anchor,
        cursor,
        goal: None,
    };
    select(model, state, selected);
    effect(true)
}

/// `iw` (`iW` if `big`) in a visual mode, `count` items from the cursor at
/// `offset`, returning where the cursor goes, or `None` past the end of the
/// text. An item is a run of one class on a row, or an empty row, which a
/// selection takes up to the next row's start; each after the first (and
/// the first, when the selection `grows`) starts one position on.
fn inner_words(text: &Text, offset: usize, count: u32, big: bool, grows: bool) -> Option<usize> {
    let mut cursor = cursor_at(text, offset);
    for item in 0..count {
        let further = grows || item > 0;
        if further && !cursor.forward() {
            return None;
        }
        if cursor.line.len() == 0 {
            if further && cursor.row + 1 < text.row_count() {
                cursor.enter(cursor.row + 1);
                cursor.ix = 0;
            }
        } else {
            cursor.ix = cursor.ix.min(cursor.line.last());
            let class = cursor.class(big);
            while cursor.class_on_row(cursor.ix + 1, big) == Some(class) {
                cursor.ix += 1;
            }
        }
    }
    Some(cursor.offset())
}

/// `iw` (`iW` if `big`) `count` times back from the cursor at `offset`, as
/// a visual selection grows the way it faces: each item one position back,
/// then to the start of its run on the row. From a row's `end` (or a last row
/// that's empty) the first item is only the position before it.
fn back_inner_words(text: &Text, offset: usize, count: u32, big: bool, end: bool) -> usize {
    let mut cursor = cursor_at(text, offset);
    for item in 0..count {
        if !cursor.backward() {
            break;
        }
        if item > 0 || !end {
            let class = cursor.class_on_row(cursor.ix, big);
            while cursor.ix > 0 && cursor.class_on_row(cursor.ix - 1, big) == class {
                cursor.ix -= 1;
            }
        }
    }
    cursor.offset()
}

/// `aw` (`aW` if `big`) `count` times back from the cursor at `offset`, as a
/// visual selection grows the way it faces: before it, a word and the blanks
/// before that, or blanks and the word before them. Returns where the
/// cursor goes.
fn back_a_words(text: &Text, offset: usize, count: u32, big: bool) -> usize {
    let mut cursor = cursor_at(text, offset);
    let blank = |cursor: &Cursor| cursor.class_on_row(cursor.ix, big) == Some(Class::Blank);
    // Back to the start of the run holding the cursor, on its row.
    let run_start = |cursor: &mut Cursor| {
        let class = cursor.class_on_row(cursor.ix, big);
        while cursor.ix > 0 && cursor.class_on_row(cursor.ix - 1, big) == class {
            cursor.ix -= 1;
        }
    };
    for _ in 0..count {
        if !cursor.backward() {
            break;
        }
        let blanks_first = blank(&cursor);
        if blanks_first {
            run_start(&mut cursor);
            if !cursor.backward() {
                break;
            }
        }
        run_start(&mut cursor);
        if !blanks_first
            && cursor.ix > 0
            && cursor.class_on_row(cursor.ix - 1, big) == Some(Class::Blank)
        {
            cursor.ix -= 1;
            run_start(&mut cursor);
        }
    }
    cursor.offset()
}

/// What a change over `ends` took, for `.`.
fn extent(text: &Text, ends: Ends, lines: bool, tab: usize) -> Extent {
    let (first, last) = ends.rows(text);
    let rows = last - first + 1;
    if lines {
        return Extent::Lines(rows);
    }
    if ends.kind == VisualKind::Block {
        let block = Block::of(text, &ends, tab);
        let columns = block.right.map(|right| right - block.left + 1);
        return Extent::Block { rows, columns };
    }
    let (lo, hi) = (cursor_at(text, ends.lo()), cursor_at(text, ends.hi()));
    let column = |cursor: &Cursor| cursor.line.start_column(cursor.ix, tab);
    // Past the end of what's at the end: a row's end is one column.
    let end = match hi.ix < hi.line.len() {
        true => hi.line.start_column(hi.ix + 1, tab),
        false => column(&hi) + 1,
    };
    let last = match (ends.goal == Some(usize::MAX), rows) {
        (true, _) => None,
        (false, 1) => Some(end - column(&lo)),
        (false, _) => Some(end),
    };
    Extent::Chars { rows, last }
}

/// The ends `extent` takes from a normal-mode caret at `head`, whose goal
/// column is `goal`.
fn extent_from(text: &Text, head: usize, goal: Option<usize>, extent: &Extent, tab: usize) -> Ends {
    let mut cursor = cursor_at(text, head);
    let last_row = text.row_count() - 1;
    let (kind, goal) = match *extent {
        // At the caret's goal column, which a linewise delete goes back to.
        Extent::Lines(rows) => {
            let row = cursor.row.saturating_add(rows - 1).min(last_row);
            let goal = cursor.vertical(row, goal, tab);
            (VisualKind::Line, goal)
        }
        // After a one-row `$` change, nvim 0.12.4's `.` goes to the row's
        // end from display column 0 or 1, and from further along (the
        // caret's column; on a tab, its last) takes the row from its start to
        // the caret.
        Extent::Chars {
            rows: 1,
            last: None,
        } if cursor.line.goal_column(cursor.ix, tab) >= 2 => {
            let row_start = text.row_start(cursor.row);
            return Ends {
                kind: VisualKind::Char,
                anchor: head,
                cursor: row_start,
                goal: None,
            };
        }
        // To the grapheme covering the last column, past the row's text on
        // its end. Rows past the end of the text stop at the last.
        // The same columns from the caret's, or after `$` to each row's end.
        // From a caret whose goal is the row's end, to each row's end.
        Extent::Block { rows, columns } => {
            let columns = columns.filter(|_| goal != Some(usize::MAX));
            let row = cursor.row.saturating_add(rows - 1).min(last_row);
            let from = cursor.line.start_column(cursor.ix, tab);
            let column = columns.map_or(usize::MAX, |columns| from + columns - 1);
            cursor.vertical(row, Some(column), tab);
            (VisualKind::Block, columns.is_none().then_some(usize::MAX))
        }
        Extent::Chars { rows, last } => {
            let from = match rows {
                1 => cursor.line.start_column(cursor.ix, tab),
                _ => 0,
            };
            let row = cursor.row.saturating_add(rows - 1).min(last_row);
            let column = last.map(|columns| from + columns - 1);
            cursor.vertical(row, Some(column.unwrap_or(usize::MAX)), tab);
            if column
                .is_none_or(|column| column >= cursor.line.start_column(cursor.line.len(), tab))
            {
                cursor.ix = cursor.line.len();
            }
            (VisualKind::Char, last.is_none().then_some(usize::MAX))
        }
    };
    Ends {
        kind,
        anchor: head,
        cursor: cursor.offset(),
        goal,
    }
}

/// `.` after a change over a selection: the same operator over as much
/// text from the primary caret (`:help visual-repeat`). As in nvim, `'<`
/// and `'>` stay where they were.
pub(crate) fn repeat(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let Some(extent) = target.state.visual_extent.clone() else {
        return effect(false);
    };
    target.model.sync_display();
    let text = target.model.text().clone();
    let tab = target.model.tab_size().max(1);
    let head = Cursor::at(&text, target.model.primary_selection().head()).offset();
    let goal = held_goal(target.model, target.state);
    let ends = extent_from(&text, head, goal, &extent, tab);
    select(target.model, target.state, ends);
    // A block is as wide as it was (after `$`, to each row's end),
    // whatever its last row's length, from the column the caret starts at.
    let columns = match extent {
        Extent::Block { columns, .. } => {
            let columns = columns.filter(|_| goal != Some(usize::MAX));
            Some((column_of(&text, head, tab), columns))
        }
        _ => None,
    };
    let repeated = operate_as_wide(target, actor, args, columns);
    // A change that failed (a shift too large) leaves normal mode as it was.
    if repeated.is_err() {
        target.state.mode = Mode::Normal;
        target.state.visual = None;
        target.model.set_selections([operator::caret(head)]);
    }
    repeated
}

/// `vim.operate` over the selection: `Target::Selection` takes it as its
/// kind does, `Target::SelectedLines` its rows. The mode becomes normal
/// (insert for `c`), keeping the extent for `gv`, `'<` and `'>`. Each change
/// is one transaction with the actor's origin.
pub(crate) fn operate(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    operate_as_wide(target, actor, args, None)
}

/// [`operate`], over a block of these display columns (its left one and how
/// many, `None` to each row's end) if given.
fn operate_as_wide(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
    columns: Option<(usize, Option<usize>)>,
) -> Result<Dispatched<TransactionId>, CommandError> {
    target.model.sync_display();
    let Some(ends) = ends(target.model, target.state) else {
        return Err(CommandError::new("no visual selection to operate on"));
    };
    // `I` and `A` act only over a block.
    if matches!(args.operator, Operator::Insert | Operator::Append)
        && ends.kind != VisualKind::Block
    {
        return effect(false);
    }
    let text = target.model.text().clone();
    let lines = ends.kind != VisualKind::Char || args.target != Target::Selection;
    let (first, last) = ends.rows(&text);
    // `>` puts in a count of 'shiftwidth's on every row: bounded as a put is.
    if args.operator == Operator::Indent {
        let by = target
            .model
            .tab_size()
            .max(1)
            .checked_mul(args.count.unwrap_or(1) as usize);
        let size = by.and_then(|by| by.checked_mul((last - first + 1) as usize));
        if size.is_none_or(|size| size > operator::MAX_PUT) {
            return Err(CommandError::new(format!(
                "shifting {} rows {} times would insert more than {} bytes",
                last - first + 1,
                args.count.unwrap_or(1),
                operator::MAX_PUT
            )));
        }
    }
    // `S`, `R` and `J` take a block's rows; the rest act on the block.
    let rows_of_block = matches!(
        (args.operator, args.target),
        (Operator::Change, Target::SelectedLines) | (Operator::Join, _)
    );
    if ends.kind == VisualKind::Block && !rows_of_block {
        return operate_block(target, actor, args, ends, columns);
    }
    let anchor_row = text.offset_to_point(ends.anchor).row;
    // Where a yank, `~`, `u` or `U` leaves the caret: the start, which for
    // rows is the first's, unless the cursor is above the anchor.
    let start = match lines {
        true if text.offset_to_point(ends.cursor).row < anchor_row => ends.cursor,
        true => text.row_start(first),
        false => ends.lo(),
    };
    let span = match lines {
        true => Span::Lines {
            rows: first..=last,
            first_non_blank: false,
            from: Some(start),
        },
        false => Span::Chars(ends.lo()..end_of(&text, ends.hi())),
    };
    let extent = extent(&text, ends, lines, target.model.tab_size().max(1));
    leave(target.model, target.state, ends);
    // One caret at the vim cursor, with its goal column, as the operators
    // read them.
    let tab = target.model.tab_size().max(1);
    let goal = ends
        .goal
        .unwrap_or_else(|| goal_of(&cursor_at(&text, ends.cursor), tab));
    to_normal(
        target.model,
        target.state,
        Ends {
            goal: Some(goal),
            ..ends
        },
    );
    // nvim moves the marks (this extent's, or through `.` an earlier
    // one's) on rows a change joins or replaces: on a row joined to the
    // first, to where its text lands there; on rows `c` replaced, to the row
    // after the first; on rows deleted at the end of the text, to the first
    // of them, past the new end.
    let snapshot = target.model.snapshot();
    let at_end = last + 1 == text.row_count();
    let first_row = snapshot.anchor_after(text.row_start(first));
    let marks = target.state.last_visual.clone().expect("a visual extent");
    let rows = [&marks.start, &marks.end].map(|pos| pos.position(snapshot).0);
    // The row a charwise change joins to the first: where what it takes ends.
    let joined = text.offset_to_point(end_of(&text, ends.hi())).row;
    let landings = match args.operator {
        Operator::Join => join_landings(&text, first, (last - first + 1).max(2)),
        _ => Vec::new(),
    };
    let moved = |pos: &Pos, row: u32| {
        let past = |anchor| Pos {
            row: anchor,
            past_end: true,
            ..pos.clone()
        };
        Some(match args.operator {
            Operator::Delete | Operator::Change if !lines && first < row && row <= joined => Pos {
                row: first_row,
                column: ends.lo() - text.row_start(first) + pos.column,
                past_end: false,
            },
            // Marks past the end, too, stay past it.
            Operator::Delete if lines && at_end && first > 0 && row >= first => {
                past(snapshot.anchor_after(text.row_start(first - 1)))
            }
            Operator::Delete if lines && at_end && row > last => Pos {
                past_end: false,
                ..pos.clone()
            },
            Operator::Change if lines && first < row && row <= last && at_end => past(first_row),
            Operator::Change if lines && first < row && row <= last => Pos {
                row: snapshot.anchor_after(text.row_start(last + 1)),
                ..pos.clone()
            },
            Operator::Join if first < row && row <= first + landings.len() as u32 => {
                let landing = landings[(row - first - 1) as usize];
                Pos {
                    row: first_row,
                    column: landing.column(pos.column),
                    past_end: false,
                }
            }
            _ => return None,
        })
    };
    let moved = [moved(&marks.start, rows[0]), moved(&marks.end, rows[1])];
    let dispatched = match args.operator {
        Operator::Delete
        | Operator::Yank
        | Operator::Change
        | Operator::Indent
        | Operator::Outdent => operator::operate_span(target, actor, args, span)?,
        // A linewise `J` starts from the first row's start, where it stays if
        // it joins nothing; `.` joins as `J` does, from the caret.
        Operator::Join => {
            let at = match ends.kind == VisualKind::Line && !target.state.replaying {
                true => text.row_start(first),
                false => Cursor::at(&text, ends.lo()).offset(),
            };
            target.model.set_selections([operator::caret(at)]);
            let join = Some((last - first + 1).max(2));
            edit::join(target, actor, CountArgs { count: join })?
        }
        Operator::Insert | Operator::Append => unreachable!("refused but over a block"),
        Operator::ToggleCase | Operator::Lowercase | Operator::Uppercase | Operator::Replace(_) => {
            let range = ends.range(&text, if lines { VisualKind::Line } else { ends.kind });
            let land = match args.operator {
                Operator::Replace(_) => range.start,
                _ => start,
            };
            recase(target, actor, args.operator, range, land)
        }
    };
    if let Some(marks) = target.state.last_visual.as_mut() {
        let [start, end] = moved;
        marks.start = start.unwrap_or_else(|| marks.start.clone());
        marks.end = end.unwrap_or_else(|| marks.end.clone());
    }
    if args.operator != Operator::Yank {
        target.state.set_last_change(Change::Operate(args).into());
        if !target.state.replaying {
            target.state.visual_extent = Some(extent);
        }
    }
    Ok(dispatched)
}

/// Where `J` puts a joined row's text on the first row.
#[derive(Copy, Clone, Debug)]
struct Landing {
    /// The column its text (without indentation) starts at.
    at: usize,
    /// The indentation `J` dropped from it.
    indent: usize,
    /// Whether `J` put a space before it.
    space: bool,
}

impl Landing {
    /// Where nvim moves a mark at `column` on the row: with its text, or
    /// where the rows met if it was in the dropped indentation.
    fn column(self, column: usize) -> usize {
        match column >= self.indent {
            true => self.at + column - self.indent,
            false => self.at - usize::from(self.space),
        }
    }
}

/// Where `J` over `rows` rows from `first` (as `edit::join` joins them)
/// puts each row after the first, read from the text before it joins.
fn join_landings(text: &Text, first: u32, rows: u32) -> Vec<Landing> {
    let blank = |ch: char| ch == ' ' || ch == '\t';
    let last = first.saturating_add(rows - 1).min(text.row_count() - 1);
    let line = text.row_to_string(first);
    let (mut len, mut empty, mut after_blank) =
        (line.len(), line.is_empty(), line.ends_with(blank));
    (first + 1..=last)
        .map(|row| {
            let next = text.row_to_string(row);
            let rest = next.trim_start_matches(blank);
            let space = !rest.is_empty() && !empty && !after_blank && !rest.starts_with(')');
            let landing = Landing {
                at: len + usize::from(space),
                indent: next.len() - rest.len(),
                space,
            };
            len = landing.at + rest.len();
            empty &= rest.is_empty();
            after_blank = rest.ends_with(blank);
            landing
        })
        .collect()
}

/// `~`, `u`, `U` or `r{char}` over `range`: each grapheme's first character
/// recased, or each grapheme but line breaks replaced; the caret goes to
/// `land`.
fn recase(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    operator: Operator,
    range: Range<usize>,
    land: usize,
) -> Dispatched<TransactionId> {
    use unicode_segmentation::UnicodeSegmentation;
    let VimTarget { model, state, .. } = target;
    let text = model.text().clone();
    let mut edits = Vec::new();
    for (ix, grapheme) in text.slice_to_string(range.clone()).grapheme_indices(true) {
        let new: String = match operator {
            _ if grapheme == "\n" => continue,
            Operator::Replace(ch) => ch.to_string(),
            _ => recased_text(grapheme, operator),
        };
        if new != grapheme {
            let at = range.start + ix;
            edits.push((at..at + grapheme.len(), new));
        }
    }
    let carets_before = vec![history::pair(&text, land)];
    let landing = model.snapshot().anchor_before(land);
    let (transaction, snapshot) = operator::apply(model, edits, origin(actor));
    operator::land(model, &snapshot, [snapshot.resolve(&landing)]);
    if let Some(id) = transaction {
        state.carets.record(id, carets_before);
    }
    state.goals = None;
    Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect())
}

/// `ch` as `~`, `u` or `U` leave it, by nvim's tables.
fn recased(ch: char, operator: Operator) -> char {
    match operator {
        Operator::Lowercase => edit::lowered(ch),
        Operator::Uppercase => edit::raised(ch),
        _ => edit::toggled(ch),
    }
}

impl VimState {
    /// In a visual mode, the vim cursor's offset in `model`'s text: where
    /// the host paints the block cursor, which isn't the model selection's
    /// head (a charwise selection takes the cursor's grapheme too). It may
    /// be a row's end, after `$`.
    pub fn visual_cursor(&self, model: &EditorModel) -> Option<usize> {
        ends(model, self).map(|ends| ends.cursor)
    }

    /// Paints the visual selection again from the vim cursor and anchor,
    /// which follow other origins' edits as anchors, while the model's
    /// selection follows them by its own range. After an edit they can
    /// disagree (deleting the selected text leaves an empty model selection
    /// while the visual one takes what follows it), so the host calls this
    /// once the model has caught up with other origins' edits, before it
    /// paints, as `HeadlessVim` does before each key: what's painted and
    /// copied is then what an operator takes. Outside a visual mode it does
    /// nothing.
    pub fn sync_visual(&mut self, model: &mut EditorModel) {
        model.sync_display();
        if let Some(ends) = ends(model, self)
            && model.selections() != ends.selections(model.text(), model.tab_size().max(1))
        {
            select(model, self, ends);
        }
    }

    /// In a visual mode, the anchor's offset: the end the cursor didn't
    /// move.
    pub fn visual_anchor(&self, model: &EditorModel) -> Option<usize> {
        ends(model, self).map(|ends| ends.anchor)
    }

    /// Enters the visual mode of `kind` over `anchor` and `cursor`, or moves
    /// the current one there, as a host does for a mouse drag. Each offset
    /// is taken at the grapheme holding it (or a row's end). Pending input
    /// is dropped. In insert mode it does nothing: end the session first.
    pub fn select_visual(
        &mut self,
        model: &mut EditorModel,
        kind: VisualKind,
        anchor: usize,
        cursor: usize,
    ) {
        if self.mode == Mode::Insert {
            return;
        }
        model.sync_display();
        let text = model.text();
        let clip = |offset: usize| position(text, offset.min(text.len()));
        let ends = Ends {
            kind,
            anchor: clip(anchor),
            cursor: clip(cursor),
            goal: None,
        };
        self.pending = Default::default();
        select(model, self, ends);
    }

    /// `'<` or `'>`: the start and end of the last visual extent, set when
    /// a visual mode is left by any path. A linewise extent's `'<` is on its
    /// first row's column 0, and its `'>` on its last row's last byte.
    pub fn mark(&self, name: char) -> Option<Mark> {
        let last = self.last_visual.as_ref()?;
        let end = match name {
            '<' => false,
            '>' => true,
            _ => return None,
        };
        Some(Mark {
            ends: [last.start.clone(), last.end.clone()],
            end,
            linewise: last.kind == VisualKind::Line,
        })
    }
}

/// A block: display columns `left` to `right` (`None` after `$`: to each
/// row's end) on rows `first` to `last`.
#[derive(Copy, Clone, Debug)]
struct Block {
    first: u32,
    last: u32,
    left: usize,
    right: Option<usize>,
}

/// What a block takes on one row.
#[derive(Clone, Debug, Default)]
struct Cut {
    /// The bytes of the graphemes it overlaps; empty at the row's end if
    /// none do.
    range: Range<usize>,
    /// How many bytes a yank takes: those graphemes, but a grapheme partly
    /// inside it as spaces for its columns inside; on a row ending before
    /// its left edge, spaces as wide as the block (see [`Block::cuts`]).
    taken: usize,
    /// The display columns it covers.
    width: usize,
    /// The columns of graphemes partly inside it that lie before and after
    /// it, which a delete leaves as spaces.
    before: usize,
    after: usize,
    /// Whether the row reaches its right edge.
    beyond: bool,
    /// Whether the row ends before its left edge.
    short: bool,
}

impl Block {
    /// The block between `ends`: each end takes its grapheme's columns (a
    /// row's end is one column).
    fn of(text: &Text, ends: &Ends, tab: usize) -> Self {
        let (first, last) = ends.rows(text);
        let columns = |offset| {
            let at = cursor_at(text, offset);
            let start = at.line.start_column(at.ix, tab);
            let width = match at.ix < at.line.len() {
                true => at.line.width(at.ix, start, tab).max(1),
                false => 1,
            };
            (start, start + width - 1)
        };
        let (anchor, cursor) = (columns(ends.anchor), columns(ends.cursor));
        Self {
            first,
            last,
            left: anchor.0.min(cursor.0),
            right: (ends.goal != Some(usize::MAX)).then_some(anchor.1.max(cursor.1)),
        }
    }

    /// What it takes on each of its rows. A row ending before its left edge
    /// takes spaces, as nvim's yank does: as wide as the block, or after `$`
    /// one wider.
    fn cuts(&self, text: &Text, tab: usize) -> Vec<Cut> {
        let mut cuts: Vec<Cut> = (self.first..=self.last)
            .map(|row| self.cut(text, row, tab, None))
            .collect();
        if self.right.is_none() {
            let spaces = self.width(&cuts) + 1;
            for cut in cuts.iter_mut().filter(|cut| cut.short) {
                cut.taken = spaces;
            }
        }
        cuts
    }

    /// What it takes on `row`, pushing what a yank takes there onto `taken`
    /// if given (but a short row's spaces).
    fn cut(&self, text: &Text, row: u32, tab: usize, mut taken: Option<&mut String>) -> Cut {
        let line = Line::new(text, row);
        let end = self.right.map_or(usize::MAX, |right| right + 1);
        let row_end = line.start + line.text.len();
        let mut cut = Cut {
            range: row_end..row_end,
            ..Cut::default()
        };
        let mut column = 0;
        for ix in 0..line.len() {
            let start = column;
            column += line.width(ix, start, tab);
            if start >= end {
                cut.beyond = true;
                break;
            }
            if column <= self.left {
                continue;
            }
            let (offset, grapheme) = (line.start + line.starts[ix], line.grapheme(ix));
            if cut.range.start == row_end {
                cut.range.start = offset;
            }
            cut.range.end = offset + grapheme.len();
            let inside = column.min(end) - start.max(self.left);
            cut.width += inside;
            let partly = start < self.left || column > end;
            if partly {
                cut.before += self.left.saturating_sub(start);
                cut.after += column.saturating_sub(end);
            }
            cut.taken += if partly { inside } else { grapheme.len() };
            if let Some(taken) = taken.as_deref_mut() {
                match partly {
                    true => taken.extend(std::iter::repeat_n(' ', inside)),
                    false => taken.push_str(grapheme),
                }
            }
        }
        cut.short = column < self.left;
        if let Some(right) = self.right.filter(|_| cut.short) {
            cut.taken = right - self.left + 1;
        }
        cut.beyond |= end <= column;
        cut
    }

    /// Its width: after `$`, the widest row's, at least one.
    fn width(&self, cuts: &[Cut]) -> usize {
        match self.right {
            Some(right) => right - self.left + 1,
            None => cuts.iter().map(|cut| cut.width).max().unwrap_or(0).max(1),
        }
    }

    /// What a yank of it writes, its rows joined, built only once its size
    /// is known: as wide as the block on short rows, it can be far larger
    /// than the text. More than [`operator::MAX_PUT`] bytes, which no put
    /// could put back, are refused.
    fn register(&self, text: &Text, cuts: &[Cut], tab: usize) -> Result<Register, CommandError> {
        let size = cuts.iter().try_fold(0usize, |size, cut| {
            size.checked_add(cut.taken)?.checked_add(1)
        });
        if size.is_none_or(|size| size > operator::MAX_PUT) {
            return Err(CommandError::new(format!(
                "a block of {} rows would take more than {} bytes",
                cuts.len(),
                operator::MAX_PUT
            )));
        }
        let rows: Vec<String> = (self.first..)
            .zip(cuts)
            .map(|(row, cut)| {
                let mut taken = String::with_capacity(cut.taken);
                match cut.short {
                    true => taken.extend(std::iter::repeat_n(' ', cut.taken)),
                    false => _ = self.cut(text, row, tab, Some(&mut taken)),
                }
                taken
            })
            .collect();
        Ok(Register::block(&rows, self.width(cuts)))
    }

    /// The position at its top-left corner in `text`: the grapheme there,
    /// or the row's end.
    fn corner(&self, text: &Text, tab: usize) -> usize {
        match self.left < width_from(&text.row_to_string(self.first), 0, tab) {
            true => at_column(text, self.first, self.left, tab),
            false => row_end(text, self.first),
        }
    }
}

/// The normal-mode caret on `row` at display `column`: the grapheme covering
/// it, or the row's last.
fn at_column(text: &Text, row: u32, column: usize, tab: usize) -> usize {
    let mut cursor = Cursor::at(text, text.row_start(row));
    cursor.vertical(row, Some(column), tab);
    cursor.offset()
}

/// The display column `offset` starts at on its row.
fn column_of(text: &Text, offset: usize, tab: usize) -> usize {
    let at = cursor_at(text, offset);
    at.line.start_column(at.ix, tab)
}

/// How wide `text` is from display `column`.
pub(crate) fn width_from(text: &str, column: usize, tab: usize) -> usize {
    let line = Line::of(0, text.to_string());
    (0..line.len()).fold(column, |at, ix| at + line.width(ix, at, tab)) - column
}

/// Where text goes in at display `column` on `row` (`None`: at its end): the
/// bytes to replace, how many spaces replace them, and after how many of
/// those the text goes. A grapheme straddling the column takes the text
/// after it if `pass` (`A` on a block's first row, where nvim's cursor can't
/// stop inside it); otherwise a tab becomes spaces either side of it, as
/// nvim splits it, and before a wide character go spaces up to the column,
/// and the text. A row short of the column is padded with spaces if `pad`,
/// and otherwise has no place for it.
pub(crate) fn insertion(
    text: &Text,
    row: u32,
    (column, pass): (Option<usize>, bool),
    tab: usize,
    pad: bool,
) -> Option<(Range<usize>, usize, usize)> {
    let line = Line::new(text, row);
    let end = line.start + line.text.len();
    let Some(column) = column else {
        return Some((end..end, 0, 0));
    };
    let mut start = 0;
    for ix in 0..line.len() {
        let offset = line.start + line.starts[ix];
        let width = line.width(ix, start, tab);
        if start == column {
            return Some((offset..offset, 0, 0));
        }
        let end = offset + line.grapheme(ix).len();
        match line.grapheme(ix) {
            _ if start + width <= column => {}
            _ if pass => return Some((end..end, 0, 0)),
            "\t" => return Some((offset..end, width, column - start)),
            _ => return Some((offset..offset, column - start, column - start)),
        }
        start += width;
    }
    match start == column || pad {
        true => Some((end..end, column - start, column - start)),
        false => None,
    }
}

/// `typed` in `spaces` spaces, after `at` of them.
pub(crate) fn spaced(typed: &str, (spaces, at): (usize, usize)) -> String {
    let mut with = String::with_capacity(spaces + typed.len());
    with.extend(std::iter::repeat_n(' ', at));
    with.push_str(typed);
    with.extend(std::iter::repeat_n(' ', spaces - at));
    with
}

/// `>` or `<` on `row` of a block from display column `left`, by `by`
/// columns (`:help v_b_>`): `>` rewrites the blanks around `left` (those
/// before it, and a tab straddling it, included) as spaces `by` wider; `<`
/// removes up to `by` columns of the blanks from `left` on, keeping those
/// that still fit whole and a straddling character's columns before `left`
/// as spaces.
/// Empty rows and rows ending before `left` are left alone.
fn shift_block(
    text: &Text,
    row: u32,
    (left, by): (usize, usize),
    indent: bool,
    tab: usize,
) -> Option<(Range<usize>, String)> {
    let line = Line::new(text, row);
    let width = |ix: usize, column: usize| line.width(ix, column, tab);
    let byte = |ix: usize| line.start + line.starts.get(ix).copied().unwrap_or(line.text.len());
    let blank = |ix: usize| matches!(line.grapheme(ix), " " | "\t");
    // To the grapheme covering `left`, and the blanks from there.
    let mut columns = vec![0];
    let mut ix = 0;
    while ix < line.len() && columns[ix] + width(ix, columns[ix]) <= left {
        columns.push(columns[ix] + width(ix, columns[ix]));
        ix += 1;
    }
    if line.len() == 0 || (ix == line.len() && columns[ix] < left) {
        return None;
    }
    // A wide character straddling `left` is blanks to `<`, as to nvim: it
    // stays only if it still fits.
    let (mut blanks, mut end) = (ix, columns[ix]);
    if !indent && ix < line.len() && columns[ix] < left && !blank(ix) {
        end += width(ix, end);
        blanks += 1;
    }
    while blanks < line.len() && blank(blanks) {
        end += width(blanks, end);
        blanks += 1;
    }
    if indent {
        let mut from = ix;
        while from > 0 && blank(from - 1) {
            from -= 1;
        }
        let spaces = " ".repeat(end - columns[from] + by);
        return Some((byte(from)..byte(blanks), spaces));
    }
    let to = end - end.saturating_sub(left.max(columns[ix])).min(by);
    let (mut kept, mut at) = (ix, columns[ix]);
    while kept < blanks && at + width(kept, at) <= to {
        at += width(kept, at);
        kept += 1;
    }
    let before = match kept == ix && at < left && blanks > ix {
        true => left - std::mem::replace(&mut at, left),
        false => 0,
    };
    let range = byte(kept)..byte(blanks);
    let spaces = " ".repeat(before + to - at);
    (!range.is_empty() || !spaces.is_empty()).then_some((range, spaces))
}

/// An upper bound on what [`put_block`] inserts for `payload`'s rows,
/// `width` wide, `count` times at display `column`, a split grapheme's
/// spaces and a new row's line break included: `None` on overflow.
pub(crate) fn put_size(
    payload: &str,
    (width, count): (usize, usize),
    column: usize,
    tab: usize,
) -> Option<usize> {
    payload.split('\n').try_fold(0usize, |size, piece| {
        let copies = piece.len().checked_add(width)?.checked_mul(count)?;
        size.checked_add(copies)?
            .checked_add(column)?
            .checked_add(split(tab))?
            .checked_add(1)
    })
}

/// The most spaces a grapheme split by a block's edge leaves: a tab's or
/// the widest character's columns.
fn split(tab: usize) -> usize {
    tab.max(motion::widest())
}

/// An upper bound on what [`put`] inserts: `payload`, a register of
/// `kind`'s width if blockwise, `count` times over a selection of `kind` on
/// `rows` rows from display `column`, with the spaces deleting a block
/// leaves; `None` on overflow.
fn put_bound(
    payload: &str,
    (width, kind): (Option<usize>, VisualKind),
    (rows, column): (usize, usize),
    count: usize,
    tab: usize,
) -> Option<usize> {
    let deleted = match kind {
        VisualKind::Block => rows.checked_mul(split(tab).checked_mul(2)?)?,
        _ => 0,
    };
    let put = match (width, kind) {
        (Some(_), VisualKind::Line) => payload.len().checked_add(1)?.checked_mul(count)?,
        (Some(width), _) => put_size(payload, (width, count), column, tab)?,
        (None, VisualKind::Block) => payload
            .len()
            .checked_mul(count)?
            .checked_add(split(tab))?
            .checked_mul(rows)?,
        (None, VisualKind::Line) => payload.len().checked_add(1)?.checked_mul(count)?,
        (None, _) => payload.len().checked_mul(count)?.checked_add(1)?,
    };
    deleted.checked_add(put)
}

/// Puts a block's `rows`, `width` columns wide, `count` times side by side at
/// display `column` from row `first` on, as `p` and `P` put a blockwise
/// register: each copy padded to the width when text follows it, rows short
/// of the column padded with spaces, a grapheme straddling it split, and
/// rows past the end of the text added. Returns the edits, where the first
/// and the last row's text start once they're in, and how long the last
/// row's is.
pub(crate) fn put_block(
    text: &Text,
    (first, column): (u32, usize),
    rows: &[&str],
    (width, count): (usize, usize),
    tab: usize,
) -> (Vec<(Range<usize>, String)>, [usize; 3]) {
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let (mut shift, mut starts) = (0isize, [0; 3]);
    let mut added = String::new();
    for (ix, piece) in rows.iter().enumerate() {
        let row = first.saturating_add(ix as u32);
        let (range, mut with, at, follows) = match row < text.row_count() {
            true => {
                let (range, spaces, at) =
                    insertion(text, row, (Some(column), false), tab, true).expect("a padded row");
                let follows = at < spaces || range.end < row_end(text, row);
                (range, spaced("", (spaces, at)), at, follows)
            }
            false => {
                let new = format!("\n{}", " ".repeat(column));
                (text.len()..text.len(), new, column + 1, false)
            }
        };
        // Padded to the width, each tab in it counted as a whole tab stop, as
        // nvim measures it.
        let line = Line::of(0, piece.to_string());
        let wide: usize = (0..line.len()).map(|ix| line.width(ix, 0, tab)).sum();
        let pad = width.saturating_sub(wide);
        let mut copies = String::new();
        for copy in 0..count {
            copies.push_str(piece);
            if copy + 1 < count || follows {
                copies.extend(std::iter::repeat_n(' ', pad));
            }
        }
        with.insert_str(at, &copies);
        starts[2] = copies.len();
        let start = match row < text.row_count() {
            true => range.start,
            false => text.len() + added.len(),
        };
        starts[1] = start.checked_add_signed(shift).expect("sorted edits") + at;
        if ix == 0 {
            starts[0] = starts[1];
        }
        match row < text.row_count() {
            true => {
                shift += with.len() as isize - range.len() as isize;
                edits.push((range, with));
            }
            false => added.push_str(&with),
        }
    }
    if !added.is_empty() {
        edits.push((text.len()..text.len(), added));
    }
    (edits, starts)
}

/// `vim.operate` over a block, but for `S`, `R` and `J`, which take its rows
/// as the other kinds do: each operator acts on what the block takes on
/// each row ([`Block::cut`], `:help blockwise-operators`), `D` and `C` to
/// the rows' ends, and leaves the caret on its top-left corner; `c`, `I` and
/// `A` open an insert session on its first row, whose text `Esc` types on
/// the others.
fn operate_block(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
    ends: Ends,
    columns: Option<(usize, Option<usize>)>,
) -> Result<Dispatched<TransactionId>, CommandError> {
    if let Some(name) = args.register {
        Registers::check(name).map_err(|error| CommandError::new(error.to_string()))?;
    }
    let VimTarget {
        model,
        host,
        state,
        registers,
    } = target;
    let text = model.text().clone();
    let tab = model.tab_size().max(1);
    let mut block = Block::of(&text, &ends, tab);
    if let Some((left, width)) = columns {
        block.left = left;
        block.right = width.map(|width| left + width - 1);
    }
    if args.target == Target::SelectedToEnd {
        block.right = None;
    }
    let cuts = block.cuts(&text, tab);
    let rows = block.last - block.first + 1;
    let extent = Extent::Block {
        rows,
        columns: block.right.map(|right| right - block.left + 1),
    };
    let empty = rows == 1 && cuts[0].taken == 0;
    let write = match args.operator {
        Operator::Yank => Some(Write::Yank),
        // A delete of nothing writes nothing.
        Operator::Delete if empty => None,
        Operator::Delete | Operator::Change => Some(Write::Delete {
            small: rows == 1,
            numbered: rows > 1,
        }),
        _ => None,
    };
    // What it writes, before anything changes (`_` writes nothing).
    let write = match write {
        Some(write) if args.register != Some('_') => {
            Some((write, block.register(&text, &cuts, tab)?))
        }
        _ => None,
    };
    leave(model, state, ends);
    state.goals = None;
    if let Some((write, value)) = write {
        registers
            .write(args.register, write, value, &mut **host)
            .map_err(|error| CommandError::new(error.to_string()))?;
    }
    let cut_rows = || cuts.iter().filter(|cut| !cut.range.is_empty());
    let deletes = || {
        cut_rows()
            .map(|cut| (cut.range.clone(), " ".repeat(cut.before + cut.after)))
            .collect::<Vec<_>>()
    };
    let edits: Vec<(Range<usize>, String)> = match args.operator {
        Operator::Delete => deletes(),
        // Each column becomes `ch`, as many as fit of a wide one, the rest
        // spaces where the row goes on past the block.
        Operator::Replace(ch) => cut_rows()
            .map(|cut| {
                // A tab fills one column, as nvim's `r<Tab>` does.
                let wide = match ch {
                    '\t' => 1,
                    ch => width_from(ch.encode_utf8(&mut [0; 4]), 0, tab).max(1),
                };
                let rest = match cut.beyond {
                    true => cut.width % wide,
                    false => 0,
                };
                let mut with = " ".repeat(cut.before);
                with.extend(std::iter::repeat_n(ch, cut.width / wide));
                with.extend(std::iter::repeat_n(' ', rest + cut.after));
                (cut.range.clone(), with)
            })
            .collect(),
        Operator::ToggleCase | Operator::Lowercase | Operator::Uppercase => cut_rows()
            .filter_map(|cut| {
                let old = text.slice_to_string(cut.range.clone());
                let new = recased_text(&old, args.operator);
                (new != old).then(|| (cut.range.clone(), new))
            })
            .collect(),
        Operator::Indent | Operator::Outdent => {
            let by = tab.saturating_mul(args.count.unwrap_or(1).max(1) as usize);
            let indent = args.operator == Operator::Indent;
            (block.first..=block.last)
                .filter_map(|row| shift_block(&text, row, (block.left, by), indent, tab))
                .collect()
        }
        Operator::Change | Operator::Insert | Operator::Append => {
            // `c` deletes the block and types at its left column on the rows
            // reaching it; `I` types there, `A` after the block (padding
            // short rows), each making room on the first row.
            let (column, pad) = match args.operator {
                Operator::Append => (block.right.map(|right| right + 1), true),
                _ => (Some(block.left), false),
            };
            let opening = match (args.operator, &cuts[0]) {
                (Operator::Change, first) if !first.range.is_empty() => {
                    (deletes(), first.range.start + first.before)
                }
                (Operator::Change, first) => (deletes(), first.range.start),
                _ => match insertion(&text, block.first, (column, pad), tab, pad) {
                    Some((range, spaces, at)) => {
                        let start = range.start;
                        (vec![(range, spaced("", (spaces, at)))], start + at)
                    }
                    None => (Vec::new(), row_end(&text, block.first)),
                },
            };
            // A tab or wide character straddling `I`'s column on the first
            // row takes the text before it, and nvim then types it on no
            // other row. `A` on the first row goes after a grapheme straddling
            // its column, and nvim types that on the others too.
            let prefix = match (&opening.0[..], column) {
                ([(range, with)], Some(column))
                    if with.is_empty() && column_of(&text, range.start, tab) > column =>
                {
                    let passed = Cursor::at(&text, range.start - 1);
                    passed.line.grapheme(passed.ix).to_string()
                }
                _ => String::new(),
            };
            let straddled = match (args.operator, &opening.0[..]) {
                (Operator::Insert, [(range, with)]) => {
                    !with.is_empty() && range.start < row_end(&text, block.first)
                }
                _ => false,
            };
            let (opening, others) = match straddled {
                true => (
                    (Vec::new(), opening.0[0].0.start),
                    block.first + 1..=block.first,
                ),
                false => (opening, block.first + 1..=block.last),
            };
            let corner = (args.operator != Operator::Change && !straddled).then_some(block.left);
            let edge = match (args.operator, column) {
                (Operator::Change, _) => insert::Edge::Fixed,
                (Operator::Insert, _) => insert::Edge::Left,
                (_, Some(_)) => insert::Edge::Right,
                (_, None) => insert::Edge::End(
                    (block.first..=block.last)
                        .map(|row| width_from(&text.row_to_string(row), 0, tab))
                        .max()
                        .unwrap_or(0),
                ),
            };
            // The other rows' places, in the text the opening edits leave.
            let rows = move |opened: &BufferSnapshot| insert::BlockRows {
                rows: others
                    .filter_map(|row| block_row(opened, row, (column, pad), tab))
                    .collect(),
                prefix,
                corner,
                text: opened.text().clone(),
                edge,
                tab,
            };
            // `c` types once, `I` and `A` as many times as their count.
            let count = match args.operator {
                Operator::Change => 1,
                _ => args.count.unwrap_or(1).max(1),
            };
            let change = (Change::Operate(args), count);
            insert::block(model, state, origin(actor), change, opening, rows);
            if !state.replaying {
                state.visual_extent = Some(extent);
            }
            return Ok(Dispatched::new(Effect::Changed));
        }
        Operator::Yank | Operator::Join => Vec::new(),
    };
    let corner = block.corner(&text, tab);
    let begins = vec![history::pair(&text, corner)];
    let (transaction, snapshot) = operator::apply(model, edits, origin(actor));
    if let Some(id) = transaction {
        state.carets.record(id, begins);
    }
    // The caret goes to the corner's byte column as it was, as nvim's does
    // (after `>` and `<` even past the row's new end), but after a delete to
    // where the block's left column now is.
    let after = snapshot.text();
    let column = match (args.operator, &cuts[0]) {
        (Operator::Delete, first) if !first.range.is_empty() => {
            first.range.start + first.before - text.row_start(block.first)
        }
        _ => corner - text.row_start(block.first),
    };
    let at = after.row_start(block.first) + column.min(after.row_len(block.first) as usize);
    match args.operator {
        Operator::Indent | Operator::Outdent => {
            let at = snapshot.anchor_after(at);
            let head = model.snapshot().resolve(&at);
            model.set_selections([operator::caret(head)]);
        }
        _ => operator::land(model, &snapshot, [at]),
    }
    if args.operator != Operator::Yank {
        state.set_last_change(Change::Operate(args).into());
        if !state.replaying {
            state.visual_extent = Some(extent);
        }
    }
    Ok(Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect()))
}

/// Where `Esc` types on `row` of a block: as [`insertion`] places `column`
/// in `snapshot`, the text a session's opening edits leave, `pad`ding a
/// short row (see [`insert::BlockRow`]).
fn block_row(
    snapshot: &BufferSnapshot,
    row: u32,
    (column, pad): (Option<usize>, bool),
    tab: usize,
) -> Option<insert::BlockRow> {
    let text = snapshot.text();
    let (range, spaces, at) = insertion(text, row, (column, false), tab, pad)?;
    let (start, end) = (text.row_start(row), row_end(text, row));
    let ending = (row + 1 < text.row_count())
        .then(|| snapshot.anchor_after(end)..snapshot.anchor_before(end + 1));
    Some(insert::BlockRow {
        ends: snapshot.anchor_after(start)..snapshot.anchor_before(end),
        ending,
        text: start..end,
        place: range.start - start..range.end - start,
        spaces: (spaces, at),
    })
}

/// `text` with each grapheme's first character as `~`, `u` or `U` leave it.
fn recased_text(text: &str, operator: Operator) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    text.graphemes(true)
        .flat_map(|grapheme| {
            let mut chars = grapheme.chars();
            let first = chars.next().map(|ch| recased(ch, operator));
            first.into_iter().chain(chars)
        })
        .collect()
}

/// `vim.put` in a visual mode (`:help v_p`): the selection replaced by the
/// register, `count` times, as one transaction. `p` leaves what it replaced
/// in the registers as a delete would; `P` leaves them alone. Lines put over
/// characters go on rows of their own, and over a block below it (`P`:
/// above); a one-row text goes on every row of a block reaching its left
/// column. `'<` and `'>` go to the start and the last character of what
/// went in (on a block's first row, for text put on every row). The put is
/// taken in the text the delete leaves, and both go in together, so nothing
/// edits the buffer between them; one too big is refused before anything
/// changes, the visual mode included.
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
    // `_` holds empty characters, not nothing.
    let unset = value.is_none() && args.register != Some('_');
    let value = value.unwrap_or_default();
    model.sync_display();
    let Some(ends) = ends(model, state) else {
        return effect(false);
    };
    let count = args.count.unwrap_or(1).max(1) as usize;
    let mut payload = value.text.replace("\r\n", "\n").replace('\r', "\n");
    if value.linewise && !payload.ends_with('\n') {
        payload.push('\n');
    }
    let text = model.text().clone();
    let tab = model.tab_size().max(1);
    let (first, last) = ends.rows(&text);
    let cursor_row = text.offset_to_point(ends.cursor).row;
    let block = Block::of(&text, &ends, tab);
    // All it may put in, rows, copies, padding and split tabs included, is
    // bounded before anything changes.
    let column = match ends.kind {
        VisualKind::Block => block.left,
        _ => column_of(&text, ends.lo(), tab),
    };
    let rows = (last - first + 1) as usize;
    let size = put_bound(
        &payload,
        (value.block_width, ends.kind),
        (rows, column),
        count,
        tab,
    );
    if size.is_none_or(|size| size > operator::MAX_PUT) {
        return Err(CommandError::new(format!(
            "putting {count} copies of {} bytes would insert more than {} bytes",
            payload.len(),
            operator::MAX_PUT
        )));
    }
    // What's replaced, as a delete takes it (for `p`, which keeps it), where
    // the put goes once it's deleted: the selection's start (a block's left
    // column on its first row), and where undo puts the caret back.
    type Removed = Vec<(Range<usize>, String)>;
    let (deleted, removed, at, begin): (Option<Register>, Removed, usize, _) = match ends.kind {
        VisualKind::Block => {
            let cuts = block.cuts(&text, tab);
            let first = &cuts[0];
            let at = first.range.start + first.before * usize::from(!first.range.is_empty());
            let removed = cuts
                .iter()
                .filter(|cut| !cut.range.is_empty())
                .map(|cut| (cut.range.clone(), " ".repeat(cut.before + cut.after)))
                .collect();
            let deleted = match args.before {
                true => None,
                false => Some(block.register(&text, &cuts, tab)?),
            };
            (
                deleted,
                removed,
                at,
                (block.first, at - text.row_start(block.first)),
            )
        }
        kind => {
            let range = ends.range(&text, kind);
            // The selection's start, whatever the delete takes before it;
            // over lines with the cursor on the first of several, the
            // cursor, whose column nvim's undo keeps.
            let begin = match kind {
                VisualKind::Line if first < last && cursor_row == first => {
                    history::pair(&text, ends.cursor)
                }
                _ => history::pair(&text, range.start),
            };
            let deleted = (!args.before).then(|| {
                let mut taken = text.slice_to_string(range.clone());
                if kind == VisualKind::Line {
                    taken.push('\n');
                }
                Register::new(taken, kind == VisualKind::Line)
            });
            // With nothing in the register, whole lines go as `d` takes
            // them, a line break with them.
            let range = match (kind, unset) {
                (VisualKind::Line, true) if last + 1 < text.row_count() => {
                    range.start..text.row_start(last + 1)
                }
                (VisualKind::Line, true) if first > 0 => row_end(&text, first - 1)..range.end,
                _ => range,
            };
            let at = range.start;
            (deleted, vec![(range, String::new())], at, begin)
        }
    };
    // The text the delete leaves, which the put goes into. The delete
    // changes nothing before the selection's start, and a block's none of
    // its rows' line breaks: `at`, the first row and the block's stay.
    let mut emptied = text.clone();
    for (range, with) in removed.iter().rev() {
        emptied.replace(range.clone(), with);
    }
    let repeated = payload.repeat(count);
    let lines = |text: &str| text.strip_suffix('\n').unwrap_or(text).to_string();
    // The edits, where the put text starts and its last character (once
    // they're in), and whether the caret goes to the first non-blank.
    let (edits, start, end, first_non_blank) = match (value.block_width, ends.kind) {
        (Some(_), VisualKind::Line) => {
            let rows = lines(&(payload.clone() + "\n").repeat(count));
            (vec![(at..at, rows.clone())], at, at + rows.len(), true)
        }
        (Some(width), _) => {
            let rows: Vec<&str> = payload.split('\n').collect();
            let column = column_of(&emptied, at, tab);
            let (edits, [_, last, len]) =
                put_block(&emptied, (first, column), &rows, (width, count), tab);
            (edits, at, last + len, false)
        }
        (None, kind) if value.linewise => {
            // A line break that goes in before the lines, ending the row
            // they follow (not their own empty first row).
            let body = lines(&repeated);
            let (at, put, ending) = match kind {
                VisualKind::Char => (at, format!("\n{repeated}"), 1),
                VisualKind::Line => (at, body.clone(), 0),
                _ if args.before => (emptied.row_start(first), repeated.clone(), 0),
                _ => (row_end(&emptied, cursor_row), format!("\n{body}"), 1),
            };
            let start = at + ending;
            (vec![(at..at, put)], start, start + body.len(), true)
        }
        (None, VisualKind::Block) if !payload.contains('\n') => {
            let edits = (first..=last)
                .filter_map(|row| {
                    let place = (Some(block.left), false);
                    let (range, spaces, at) = insertion(&emptied, row, place, tab, false)?;
                    Some((range, spaced(&repeated, (spaces, at))))
                })
                .collect();
            (edits, at, at + repeated.len(), false)
        }
        // Over lines, each copy of characters on rows of its own.
        (None, kind) => {
            let blank = kind == VisualKind::Line;
            let put = match blank {
                true => {
                    let mut rows = format!("{payload}\n").repeat(count);
                    rows.pop();
                    rows
                }
                false => repeated.clone(),
            };
            (vec![(at..at, put.clone())], at, at + put.len(), blank)
        }
    };
    let extent = extent(&text, ends, ends.kind == VisualKind::Line, tab);
    leave(model, state, ends);
    let begins = vec![begin];
    let (transaction, snapshot) = operator::apply_steps(model, [removed, edits], origin(actor));
    if let Some(id) = transaction {
        state.carets.record(id, begins);
    }
    let after = snapshot.text().clone();
    // The first and last characters put in (a line break ending it is at
    // the next row's start).
    let start = Cursor::at(&after, start).offset();
    let end = match after.slice_to_string(start..end).ends_with('\n') {
        true => end,
        false => Cursor::at(&after, end.saturating_sub(1).max(start)).offset(),
    };
    let land = match (first_non_blank, value.block_width) {
        (true, _) => {
            let mut cursor = Cursor::at(&after, start);
            cursor.ix = cursor.line.first_non_blank();
            cursor.offset()
        }
        (false, Some(_)) => start,
        (false, None) if repeated.contains('\n') => start,
        (false, None) => end,
    };
    operator::land(model, &snapshot, [land]);
    if let Some(marks) = state.last_visual.as_mut() {
        marks.start = Pos::at(&snapshot, start);
        marks.end = Pos::at(&snapshot, end);
    }
    // `p` leaves what it replaced in the registers, as a delete would; a
    // delete of nothing writes nothing.
    if let Some(deleted) = deleted.filter(|deleted| deleted.linewise || !deleted.text.is_empty()) {
        let small = !deleted.linewise && !deleted.text.contains('\n');
        let write = Write::Delete {
            small,
            numbered: !small,
        };
        registers
            .write(None, write, deleted, &mut **host)
            .map_err(|error| CommandError::new(error.to_string()))?;
    }
    // `.` deletes as much text again, as nvim's does.
    state.set_last_change(
        Change::Operate(OperateArgs {
            operator: Operator::Delete,
            target: Target::Selection,
            count: None,
            register: args.before.then_some('_'),
        })
        .into(),
    );
    state.visual_extent = Some(extent);
    state.goals = None;
    Ok(Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect()))
}
