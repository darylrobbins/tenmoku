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

mod block;
mod object;
mod operate;
mod put;

use block::{Block, at_column, column_of, operate_block, recased_text};
pub(crate) use block::{insertion, spaced, width_from};
pub(crate) use object::select_object;
use operate::{extent, recased};
pub(crate) use operate::{operate, repeat};
pub(crate) use put::{put, put_block, put_size};

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
        // Back from a row's end, the whole row is before the cursor; a
        // search from there starts at the next row.
        Motion::Left
        | Motion::Search(_)
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
