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
//! [`Mark`](crate::Mark)s, which hold a byte column on a row as nvim's do.

use std::ops::Range;

use fe_buffer::{Anchor, BufferSnapshot, ResolvedSelection, TransactionId};
use fe_command::{Actor, CommandError, Dispatched, Effect};
use fe_editor::{EditorModel, commands::origin};
use fe_text::{Bias, Text};

use crate::{
    Change, JoinArgs, Mode, Motion, MoveArgs, OperateArgs, Operator, PutArgs, Register, Registers,
    SelectArgs, SwapArgs, Target, TextObject, VimState, VimTarget, VisualArgs, VisualKind, Write,
    edit, history, insert, line_motions,
    motion::{self, Class, Cursor, Line},
    operator::{self, Span, row_end},
    text_object,
};

mod block;
mod object;
mod operate;
mod put;
mod quote;

use block::{Block, at_column, column_of, operate_block, recased_text};
pub(crate) use block::{insertion, spaced, width_from};
pub(crate) use object::select_object;
use operate::{extent, recased};
pub(crate) use operate::{joined_marks, live_area, move_marks, operate, repeat, replay_area};
pub(crate) use put::{Over, put, put_block, put_size, replace};

/// A visual mode's ends, as anchors (see [`Ends`]).
#[derive(Clone, Debug)]
pub(crate) struct Visual {
    anchor: Anchor,
    cursor: Anchor,
    /// The cursor's goal column (`usize::MAX` after `$`), as in normal mode.
    goal: Option<usize>,
    /// The display column along its row `gj` and `gk` aimed for with the
    /// goal (see [`crate::line_motions::DisplayGoal`]): `select` clears it.
    along: Option<usize>,
    /// Whether a text object that grows from its own selection made this
    /// one (`ia`): `select` clears it.
    by_object: bool,
}

/// A position as nvim keeps a mark: a byte column on a row. The row is
/// anchored at its start, so it follows rows put in or taken out above it
/// and moves up with a row deleted or joined to the one above; edits within
/// the row leave the column alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pos {
    row: Anchor,
    column: usize,
    /// On the row after the anchor's, past the end of the text: where nvim
    /// keeps a mark on rows a visual change deleted at the end (or that
    /// `:s` joined through it, the anchor then the text's end).
    past_end: bool,
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

    /// Its row and byte column in `snapshot`.
    pub(crate) fn position(&self, snapshot: &BufferSnapshot) -> (u32, usize) {
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
    /// Which of `'<` and `'>` `:delmarks` deleted.
    deleted: [bool; 2],
}

impl LastVisual {
    /// Deletes `'>` (`end`) or `'<` as `:delmarks` does: nvim keeps the
    /// extent's anchor and cursor apart, and both marks then read the one
    /// left (the cursor end after `'<` goes, the anchor after `'>` does).
    /// Returns whether both are deleted.
    pub(crate) fn delete(&mut self, end: bool) -> bool {
        self.deleted[usize::from(end)] = true;
        let (cursor, anchor) = match self.cursor_first {
            true => (self.start.clone(), self.end.clone()),
            false => (self.end.clone(), self.start.clone()),
        };
        let left = if end { anchor } else { cursor };
        (self.start, self.end) = (left.clone(), left);
        self.deleted == [true, true]
    }

    /// Its start and end, for `'<` and `'>`, and whether it's linewise.
    pub(crate) fn marks(&self) -> ([Pos; 2], bool) {
        let ends = [self.start.clone(), self.end.clone()];
        (ends, self.kind == VisualKind::Line)
    }
}

/// Before rows go in below `row`, at its end: `'<` and `'>` on it stay
/// there even if it's empty (where a row's start is its end), as nvim's do.
/// [`reanchored`] sets them as any others once the rows are in.
pub(crate) fn opening_below(state: &mut VimState, snapshot: &BufferSnapshot, row: u32) {
    let Some(last) = state.last_visual.as_mut() else {
        return;
    };
    let text = snapshot.text();
    if text.row_len(row) > 0 {
        return;
    }
    for pos in [&mut last.start, &mut last.end] {
        if !pos.past_end && pos.position(snapshot).0 == row {
            pos.row = snapshot.anchor_before(text.row_start(row));
        }
    }
}

/// `'<` and `'>` anchored afresh at their rows' starts in `snapshot`, as
/// [`Pos::at`] anchors them, after [`opening_below`].
pub(crate) fn reanchored(state: &mut VimState, snapshot: &BufferSnapshot) {
    let Some(last) = state.last_visual.as_mut() else {
        return;
    };
    let text = snapshot.text();
    for pos in [&mut last.start, &mut last.end] {
        if !pos.past_end {
            let row = pos.position(snapshot).0;
            pos.row = snapshot.anchor_after(text.row_start(row));
        }
    }
}

/// Before rows are rewritten in place, each row `moved` maps to another
/// taking its text (`]e`, `[e`): `'<` and `'>` on them go with their text,
/// as nvim's `:move` moves them.
pub(crate) fn moving_rows(
    state: &mut VimState,
    snapshot: &BufferSnapshot,
    moved: &dyn Fn(u32) -> Option<u32>,
) {
    let Some(last) = state.last_visual.as_mut() else {
        return;
    };
    let text = snapshot.text();
    for pos in [&mut last.start, &mut last.end] {
        let (row, _) = pos.position(snapshot);
        if let Some(to) = moved(row).filter(|_| !pos.past_end) {
            pos.row = snapshot.anchor_after(text.row_start(to));
        }
    }
}

/// Before a change joins the rows from `first` (past the first row) to the
/// end of the text onto the row before them, deleting them (`:s` over
/// rows): `'<` and `'>` on them go past the last row of the text the change
/// leaves, the line breaks it puts in counted, keeping their columns, as
/// nvim's do.
pub(crate) fn joining_at_end(state: &mut VimState, snapshot: &BufferSnapshot, first: u32) {
    let Some(marks) = state.last_visual.as_mut() else {
        return;
    };
    let text = snapshot.text();
    for pos in [&mut marks.start, &mut marks.end] {
        if first > 0 && !pos.past_end && pos.position(snapshot).0 >= first {
            pos.row = snapshot.anchor_after(text.len());
            pos.past_end = true;
        }
    }
}

/// `'<` and `'>` as `gq` reflows rows: see [`reflowing`].
pub(crate) struct Reflowing([Option<(Anchor, usize)>; 2]);

/// Before `gq` reflows the rows from `first` through `last`, joining and
/// breaking them only at blanks: where `'<` and `'>` on them are, which go
/// with their text, as nvim's do, once [`reflowed`].
pub(crate) fn reflowing(
    state: &VimState,
    snapshot: &BufferSnapshot,
    first: u32,
    last: u32,
) -> Reflowing {
    let text = snapshot.text();
    let at = |pos: &Pos| {
        let (row, column) = pos.position(snapshot);
        if pos.past_end || !(first..=last).contains(&row) {
            return None;
        }
        // On its row, at most at its end, and on a character's start.
        let start = text.row_start(row);
        let len = text.row_len(row) as usize;
        let offset = text.clip_offset(start + column.min(len), Bias::Left);
        Some((snapshot.anchor_before(offset), column - (offset - start)))
    };
    Reflowing(match &state.last_visual {
        Some(marks) => [at(&marks.start), at(&marks.end)],
        None => [None, None],
    })
}

/// After `gq`'s reflow: `'<` and `'>` where their text went.
pub(crate) fn reflowed(state: &mut VimState, snapshot: &BufferSnapshot, reflowing: Reflowing) {
    let Some(marks) = state.last_visual.as_mut() else {
        return;
    };
    let [start, end] = reflowing.0;
    for (pos, at) in [(&mut marks.start, start), (&mut marks.end, end)] {
        if let Some((anchor, past)) = at {
            let moved = Pos::at(snapshot, snapshot.resolve(&anchor));
            *pos = Pos {
                column: moved.column + past,
                ..moved
            };
        }
    }
}

/// How much text the last change over a selection took, for `.`
/// (`:help visual-repeat`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
    let mut cursor = Cursor::whole(text, offset);
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
    resolve_ends(model, state, kind)
}

/// What a put over the visual selection replaces, as the marks see it.
pub(crate) enum Replaced {
    /// Characters across rows: the first row's marks stay, the last row's
    /// are joined to it.
    Chars(Range<usize>),
    /// Whole rows from the first through the last, whose marks go.
    Rows(u32, u32),
}

/// What a put over the visual selection replaces: `None` in a block (which
/// takes no rows) or outside a visual mode.
pub(crate) fn replaced(model: &mut EditorModel, state: &VimState) -> Option<Replaced> {
    model.sync_display();
    let ends = ends(model, state)?;
    let text = model.text();
    Some(match ends.kind {
        VisualKind::Block => return None,
        VisualKind::Char => Replaced::Chars(ends.range(text, VisualKind::Char)),
        VisualKind::Line => {
            let (first, last) = ends.rows(text);
            Replaced::Rows(first, last)
        }
    })
}

/// [`ends`], also while a command line suspends a visual mode: for what
/// only reports or paints them, never a visual command.
fn held_ends(model: &EditorModel, state: &VimState) -> Option<Ends> {
    let kind = match (state.mode, &state.cmdline) {
        (Mode::Visual(kind), _) => kind,
        (Mode::CmdLine(_), Some(line)) => match line.from {
            Mode::Visual(kind) => kind,
            _ => return None,
        },
        _ => return None,
    };
    resolve_ends(model, state, kind)
}

fn resolve_ends(model: &EditorModel, state: &VimState, kind: VisualKind) -> Option<Ends> {
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
        along: None,
        by_object: false,
    });
    state.goals = None;
    state.display_goals = None;
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
            deleted: [false; 2],
        });
    }
    state.mode = Mode::Normal;
    state.visual = None;
}

/// The extent leaving the visual mode now would make the last, `'<` and
/// `'>`; outside a visual mode, the last one.
pub(crate) fn extent_now(model: &EditorModel, state: &VimState) -> Option<LastVisual> {
    let Some(ends) = ends(model, state) else {
        return state.last_visual.clone();
    };
    let snapshot = model.snapshot();
    Some(LastVisual {
        kind: ends.kind,
        start: Pos::at(snapshot, ends.lo()),
        end: Pos::at(snapshot, ends.hi()),
        cursor_first: ends.cursor < ends.anchor,
        goal: ends.goal,
        deleted: [false; 2],
    })
}

/// Adopts `selection`, which the model has just set, as vim's extent: a
/// non-empty one is charwise visual from its start (its end if reversed) to
/// its last grapheme; an empty one is the normal-mode caret there, leaving
/// a visual mode if one was on. The model's selection ends up equal to
/// `selection`, so the expand history stays live.
pub(crate) fn adopt(model: &mut EditorModel, state: &mut VimState, selection: ResolvedSelection) {
    let text = model.text();
    let ResolvedSelection { range, reversed } = selection;
    if range.is_empty() {
        if let Some(ends) = ends(model, state) {
            leave(model, state, ends);
        }
        let at = position(text, range.start);
        to_normal(
            model,
            state,
            Ends {
                kind: VisualKind::Char,
                anchor: at,
                cursor: at,
                goal: None,
            },
        );
        // The exact offset, which normal-mode clamping would pull in off a
        // row's end: enabling vim keeps carets there.
        if model.primary_selection().range != range {
            let caret = range.start;
            model.set_selections([operator::caret(caret)]);
            state.goals = Some((model.snapshot().version(), vec![(caret, None)]));
        }
        return;
    }
    let last = position(text, text.clip_offset(range.end - 1, Bias::Left));
    let first = position(text, range.start);
    let (anchor, cursor) = match reversed && first != last {
        true => (last, first),
        false => (first, last),
    };
    let ends = Ends {
        kind: VisualKind::Char,
        anchor,
        cursor,
        goal: None,
    };
    select(model, state, ends);
}

/// A normal-mode caret at the vim cursor, keeping its goal column.
fn to_normal(model: &mut EditorModel, state: &mut VimState, ends: Ends) {
    let caret = Cursor::at(model.text(), ends.cursor).offset();
    model.set_selections([operator::caret(caret)]);
    state.goals = Some((model.snapshot().version(), vec![(caret, ends.goal)]));
    state.display_goals = None;
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

/// `:` in a visual mode: back to normal mode, keeping the extent for `'<`
/// and `'>`, with the caret where nvim leaves it for the ex line: at the
/// start of a charwise extent, at a block's top-left corner, and for a
/// linewise one on the cursor if it's before the anchor, else at the start
/// of the first row. Returns whether a visual mode was on.
pub(crate) fn leave_for_ex(target: &mut VimTarget<'_>) -> bool {
    let VimTarget { model, state, .. } = target;
    model.sync_display();
    let Some(ends) = ends(model, state) else {
        return false;
    };
    leave(model, state, ends);
    let text = model.text();
    let cursor = match ends.kind {
        VisualKind::Char => ends.lo(),
        VisualKind::Line if ends.cursor < ends.anchor => ends.cursor,
        VisualKind::Line => text.row_start(ends.rows(text).0),
        VisualKind::Block => {
            let tab = model.tab_size().max(1);
            let left = column_of(text, ends.anchor, tab).min(column_of(text, ends.cursor, tab));
            at_column(text, ends.rows(text).0, left, tab)
        }
    };
    let ends = Ends {
        cursor,
        goal: None,
        ..ends
    };
    to_normal(model, state, ends);
    true
}

/// Where nvim's `g@` over the visual selection puts `'[` and `']`, as
/// offsets: a charwise extent's ends; for rows, the anchor's row start and
/// the cursor, in order; a block's top-left corner and its right column on
/// its last row. `None` outside a visual mode.
pub(crate) fn operator_ends(model: &EditorModel, state: &VimState) -> Option<(usize, usize)> {
    let ends = ends(model, state)?;
    let text = model.text();
    Some(match ends.kind {
        VisualKind::Char => (ends.lo(), ends.hi()),
        VisualKind::Line => {
            let anchor = text.row_start(text.offset_to_point(ends.anchor).row);
            (anchor.min(ends.cursor), anchor.max(ends.cursor))
        }
        VisualKind::Block => {
            let tab = model.tab_size().max(1);
            let (first, last) = ends.rows(text);
            let (anchor, cursor) = (
                column_of(text, ends.anchor, tab),
                column_of(text, ends.cursor, tab),
            );
            (
                at_column(text, first, anchor.min(cursor), tab),
                at_column(text, last, anchor.max(cursor), tab),
            )
        }
    })
}

/// Selects the rows from `first` through `last` linewise, as nvim's
/// `{first}GV{last}G` does from the primary caret: each end on its row at
/// the caret's goal column. `'<` and `'>` take the extent even under `.`,
/// as running those keys sets them (nvim's `gc` object, `feature/comment.rs`).
pub(crate) fn select_rows(model: &mut EditorModel, state: &mut VimState, first: u32, last: u32) {
    model.sync_display();
    let goal = held_goal(model, state);
    let text = model.text();
    let tab = model.tab_size().max(1);
    let on_row = |row: u32| {
        let mut cursor = Cursor::at(text, text.row_start(row));
        cursor.vertical(row, goal, tab);
        cursor.offset()
    };
    let ends = Ends {
        kind: VisualKind::Line,
        anchor: on_row(first),
        cursor: on_row(last),
        goal,
    };
    select(model, state, ends);
    let snapshot = model.snapshot();
    state.last_visual = Some(LastVisual {
        kind: ends.kind,
        start: Pos::at(snapshot, ends.lo()),
        end: Pos::at(snapshot, ends.hi()),
        cursor_first: false,
        goal,
        deleted: [false; 2],
    });
}

/// The goal a cursor set afresh takes before the anchor: the first column of
/// the character it is on, as nvim shows it there; none after the anchor.
fn before_anchor_goal(cursor: &Cursor<'_>, anchor: usize, tab: usize) -> Option<usize> {
    (cursor.offset() < anchor).then(|| cursor.line.start_column(cursor.ix, tab))
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
        goal: before_anchor_goal(
            &cursor_at(model.text(), ends.anchor),
            ends.cursor,
            model.tab_size().max(1),
        ),
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
        state.failed = true;
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
    cursor.visual = true;
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
        | Motion::SearchWord(_)
        | Motion::Jump(_)
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
        // From where the cursor stands, even past the row's last character.
        Motion::Unmatched { .. } => motion::motion(&mut cursor, args, state.last_find, goal, tab),
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
        Err(_) => {
            state.failed |= motion::errs(&text, ends.cursor, args, state.last_find, tab);
            cursor = cursor_at(&text, ends.cursor);
        }
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
        Some(before_anchor_goal(&cursor, ends.anchor, tab).unwrap_or_else(|| goal_of(&cursor, tab)))
    });
    state.failed |= motion::word_fails(&text, ends.cursor, args, tab);
    let moved_to = Ends {
        cursor: cursor.offset(),
        goal,
        ..ends
    };
    select(model, state, moved_to);
    // Only `gj` and `gk` leave one, aiming for the display column they hold.
    let along = line_motions::alongs()
        .iter()
        .rev()
        .find(|kept| Some(kept.goal) == goal && kept.head == cursor.offset())
        .map(|kept| kept.along);
    if let Some(visual) = state.visual.as_mut() {
        visual.along = along;
    }
    moved_to.cursor != ends.cursor
}

/// Puts the visual cursor at `offset`, keeping its goal column (as `j` and
/// `k` would have), for `{count}zt` and the like. False if it didn't move.
pub(crate) fn jump_cursor(model: &mut EditorModel, state: &mut VimState, offset: usize) -> bool {
    model.sync_display();
    let Some(ends) = ends(model, state) else {
        return false;
    };
    let text = model.text().clone();
    let tab = model.tab_size().max(1);
    let offset = position(&text, offset);
    let goal = ends
        .goal
        .unwrap_or_else(|| goal_of(&cursor_at(&text, ends.cursor), tab));
    let moved_to = Ends {
        cursor: offset,
        goal: Some(goal),
        ..ends
    };
    select(model, state, moved_to);
    offset != ends.cursor
}

impl VimState {
    /// Whether a text object that grows from its own selection made the
    /// visual selection, which hasn't moved since.
    pub(crate) fn visual_by_object(&self) -> bool {
        self.visual.as_ref().is_some_and(|visual| visual.by_object)
    }

    /// Marks the visual selection as the one a text object just made.
    pub(crate) fn set_visual_by_object(&mut self) {
        if let Some(visual) = self.visual.as_mut() {
            visual.by_object = true;
        }
    }

    /// The display column `gj` and `gk` left the visual cursor aiming for.
    pub(crate) fn visual_along(&self) -> Option<usize> {
        let visual = self.visual.as_ref()?;
        visual.goal.and(visual.along)
    }

    /// In a visual mode, the vim cursor's offset in `model`'s text: where
    /// the host paints the block cursor, which isn't the model selection's
    /// head (a charwise selection takes the cursor's grapheme too). It may
    /// be a row's end, after `$`.
    pub fn visual_cursor(&self, model: &EditorModel) -> Option<usize> {
        held_ends(model, self).map(|ends| ends.cursor)
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
        let Some(ends) = held_ends(model, self) else {
            return;
        };
        let selections = ends.selections(model.text(), model.tab_size().max(1));
        if model.selections() == selections {
            return;
        }
        if matches!(self.mode, Mode::Visual(_)) {
            select(model, self, ends);
        } else {
            // A command line over the selection: repaint it, keeping the mode.
            model.set_selections(selections);
        }
    }

    /// In a visual mode, the anchor's offset: the end the cursor didn't
    /// move.
    pub fn visual_anchor(&self, model: &EditorModel) -> Option<usize> {
        held_ends(model, self).map(|ends| ends.anchor)
    }

    /// Enters the visual mode of `kind` over `anchor` and `cursor`, or moves
    /// the current one there, as a host does for a mouse drag. Each offset
    /// is taken at the grapheme holding it (or a row's end). Pending input
    /// is dropped. In insert or replace mode it does nothing: end the
    /// session first.
    pub fn select_visual(
        &mut self,
        model: &mut EditorModel,
        kind: VisualKind,
        anchor: usize,
        cursor: usize,
    ) {
        if matches!(self.mode, Mode::Insert | Mode::Replace) {
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

    /// Moves the visual ends to `anchor` and `cursor`, as a host does once
    /// the text under them was replaced (each offset is taken at the
    /// grapheme holding it, or a row's end). Unlike [`Self::select_visual`]
    /// it keeps the cursor's goal column (a block extended with `$` stays so
    /// on rows of other lengths) and the mode, also while a `/` or `?`
    /// command line suspends the visual mode, with the command line and
    /// pending input untouched. Returns whether a visual mode was held;
    /// otherwise it does nothing.
    pub fn remap_visual(&mut self, model: &mut EditorModel, anchor: usize, cursor: usize) -> bool {
        model.sync_display();
        let Some(held) = held_ends(model, self) else {
            return false;
        };
        let text = model.text();
        let clip = |offset: usize| position(text, offset.min(text.len()));
        let ends = Ends {
            anchor: clip(anchor),
            cursor: clip(cursor),
            ..held
        };
        if matches!(self.mode, Mode::Visual(_)) {
            select(model, self, ends);
        } else {
            let snapshot = model.snapshot();
            if let Some(visual) = &mut self.visual {
                visual.anchor = snapshot.anchor_after(ends.anchor);
                visual.cursor = snapshot.anchor_after(ends.cursor);
                visual.along = None;
            }
            let selections = ends.selections(model.text(), model.tab_size().max(1));
            model.set_selections(selections);
        }
        true
    }
}

#[cfg(test)]
mod remap_tests {
    //! `VimState::remap_visual` moves the visual ends keeping the goal and the
    //! mode, also under a `/` or `?` command line.

    use crate::{HeadlessVim, Mode, VisualKind};

    fn selected(vim: &HeadlessVim) -> Vec<(usize, usize)> {
        (vim.model().selections().iter())
            .map(|selection| (selection.range.start, selection.range.end))
            .collect()
    }

    #[test]
    fn remap_moves_a_visual_selection() {
        let mut vim = HeadlessVim::new("abcdef\nghi", (0, 1));
        vim.feed("vl").unwrap();
        let mut state = vim.state().clone();
        assert!(state.remap_visual(vim.model_mut(), 2, 4));
        assert_eq!(state.visual_anchor(vim.model()), Some(2));
        assert_eq!(state.visual_cursor(vim.model()), Some(4));
        assert_eq!(selected(&vim), [(2, 5)]);
    }

    #[test]
    fn remap_outside_visual_does_nothing() {
        let mut vim = HeadlessVim::new("abc", (0, 1));
        let mut state = vim.state().clone();
        assert!(!state.remap_visual(vim.model_mut(), 0, 2));
        assert_eq!(state.mode(), Mode::Normal);
    }

    #[test]
    fn remap_keeps_a_search_suspended_visual() {
        let mut vim = HeadlessVim::new("abcdef\nghi", (0, 1));
        vim.feed("vl/").unwrap();
        assert!(matches!(vim.state().mode(), Mode::CmdLine(_)));
        let mut state = vim.state().clone();
        assert!(state.remap_visual(vim.model_mut(), 0, 3));
        assert!(matches!(state.mode(), Mode::CmdLine(_)));
        assert_eq!(state.visual_anchor(vim.model()), Some(0));
        assert_eq!(state.visual_cursor(vim.model()), Some(3));
        assert_eq!(selected(&vim), [(0, 4)]);
    }

    #[test]
    fn remap_keeps_the_block_dollar_goal() {
        let mut vim = HeadlessVim::new("abcdef\nabc", (0, 1));
        vim.feed("<C-v>j$").unwrap();
        let before = selected(&vim);
        assert_eq!(before, [(1, 6), (8, 10)]);
        let mut state = vim.state().clone();
        let (anchor, cursor) = (
            state.visual_anchor(vim.model()).unwrap(),
            state.visual_cursor(vim.model()).unwrap(),
        );
        assert!(state.remap_visual(vim.model_mut(), anchor, cursor));
        assert_eq!(state.mode(), Mode::Visual(VisualKind::Block));
        assert_eq!(selected(&vim), before);
    }
}
