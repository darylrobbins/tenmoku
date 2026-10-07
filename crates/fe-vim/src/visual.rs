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
    Change, CountArgs, Mode, Motion, MoveArgs, OperateArgs, Operator, SelectArgs, Target,
    TextObject, VimState, VimTarget, VisualArgs, VisualKind, edit, history,
    motion::{self, Class, Cursor},
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
    /// its end; linewise, its rows without the last line break.
    fn range(&self, text: &Text, kind: VisualKind) -> Range<usize> {
        match kind {
            VisualKind::Char => self.lo()..end_of(text, self.hi()),
            VisualKind::Line => {
                let (first, last) = self.rows(text);
                text.row_start(first)..row_end(text, last)
            }
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
    let range = ends.range(model.text(), ends.kind);
    model.set_selections([ResolvedSelection {
        range,
        reversed: ends.cursor < ends.anchor,
    }]);
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

/// `vim.visual_exit`: back to normal mode, the caret on the vim cursor.
pub(crate) fn exit(target: &mut VimTarget<'_>) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget { model, state, .. } = target;
    model.sync_display();
    let Some(ends) = ends(model, state) else {
        return effect(false);
    };
    leave(model, state, ends);
    to_normal(model, state, ends);
    effect(true)
}

/// `vim.visual_swap`: the cursor goes to the other end.
pub(crate) fn swap(target: &mut VimTarget<'_>) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget { model, state, .. } = target;
    model.sync_display();
    let Some(ends) = ends(model, state) else {
        return effect(false);
    };
    let swapped = Ends {
        anchor: ends.cursor,
        cursor: ends.anchor,
        goal: None,
        ..ends
    };
    select(model, state, swapped);
    effect(ends.anchor != ends.cursor)
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
            for _ in 0..n {
                let before = (cursor.row, cursor.ix);
                cursor.words(Motion::WordEnd, 1, args.motion == Motion::BigWordEnd);
                if (cursor.row, cursor.ix) == before {
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
    let moved_to = Ends {
        cursor: cursor.offset(),
        goal: moved.unwrap_or_else(|kept| kept),
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
    let repeated = operate(target, actor, args);
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
    target.model.sync_display();
    let Some(ends) = ends(target.model, target.state) else {
        return Err(CommandError::new("no visual selection to operate on"));
    };
    let text = target.model.text().clone();
    let lines = ends.kind == VisualKind::Line || args.target == Target::SelectedLines;
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
            let at = match lines && !target.state.replaying {
                true => text.row_start(first),
                false => Cursor::at(&text, ends.lo()).offset(),
            };
            target.model.set_selections([operator::caret(at)]);
            let join = Some((last - first + 1).max(2));
            edit::join(target, actor, CountArgs { count: join })?
        }
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
        target.state.last_change = Some(Change::Operate(args).into());
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
            _ => {
                let mut chars = grapheme.chars();
                let first = chars.next().map(|ch| recased(ch, operator));
                first.into_iter().chain(chars).collect()
            }
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
        if let Some(ends) = ends(model, self) {
            let range = ends.range(model.text(), ends.kind);
            let reversed = ends.cursor < ends.anchor;
            if model.selections() != [ResolvedSelection { range, reversed }] {
                select(model, self, ends);
            }
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
