//! Exchanging two regions (a feature module; see [`crate::feature`]), as
//! the vim-exchange plugin documents it: `cx{motion}` or `cx{object}`
//! ([`Operator::Exchange`]) marks what it covers the first time and swaps
//! it with the marked text the second, `cxx` takes `count` rows as `cc`
//! would, and `cxc` clears the mark (`vim.exchange_clear`).
//!
//! The first `cx` changes nothing: it keeps the region so it follows edits
//! made before the second, and as `g@` does, puts the caret at `'[` and
//! sets `'[` and `']` to where the target starts and ends. Characters are
//! anchored on the [`VimState`](crate::VimState), text typed at either edge
//! staying out of them. Rows are kept as their first and last row's marks,
//! which follow edits as `a` to `z` do (a row opened above an empty one, as
//! `O` opens it, moves it down; one opened below doesn't), and as anchors
//! over their text: deleting a boundary row leaves the rows that survive
//! (up to the other boundary), a line break typed in the last row leaves
//! the text split off it in the region, and deleting everything clears the
//! mark. The second swaps the two
//! regions' text in one undo step, leaving the registers alone, clears the
//! mark and puts the caret at the start of what now fills its own region
//! (on its first non-blank, for rows).
//!
//! Each region's text takes the kind of the region it goes into: rows going
//! where characters were go in without their last line break, and
//! characters going where rows were take those rows' text, the rows' line
//! breaks staying. When both are rows, each moved row takes the indentation
//! of the region it replaces in place of its own region's (the leading
//! blanks of each one's first non-blank row), as vim-exchange's
//! `g:exchange_indent` does by default; a row not starting with its
//! region's indentation, and a blank one, keep theirs.
//!
//! If one region contains the other (or they're the same), the contained
//! one's text replaces the container's. Regions that overlap otherwise
//! fail ("Exchange aborted: overlapping text"), clearing the mark. Regions
//! that only touch swap.
//!
//! `.` repeats `cx` over the same target, so `cxiw` then `w.` swaps two
//! words; a `cx` that fails leaves what `.` repeated. `cxgc` takes the run
//! of commented rows the `gc` object selects. There's no visual-mode key:
//! vim-exchange's visual `X` is vim's "delete the rows", which stays.
//!
//! Known differences: the marked region isn't highlighted; it marks and
//! swaps at the primary caret only; a count typed after `c` (`c2x`) is
//! taken as one before it, as `2cx`; the `x` of `cx`, `cxx` and the `c` of
//! `cxc` are read as operator-pending keys, so an `omap` of them applies,
//! where vim-exchange maps `cx` whole in normal mode; and a mark in another
//! buffer, or past what the buffer's history keeps, is dropped, the next
//! `cx` marking anew.

use std::ops::Range;

use fe_buffer::{Anchor, BufferId, BufferSnapshot, TransactionId};
use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use fe_editor::{EditorModel, commands::origin};

use crate::{
    Change, Mode, Motion, MoveArgs, OperateArgs, Operator, Pending, Step, Target, VimCommands,
    VimKey, VimState, VimTarget, VisualKind, cmdline,
    feature::{Feature, comment},
    history,
    marks::{self, Place},
    motion::{self, Cursor},
    operator::{self, Span},
    reducer::invocation,
    text_object, visual,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    register,
    ..Feature::NONE
};

const OPERATOR: Operator = Operator::Exchange;

/// What `cx` reports when the regions overlap.
const OVERLAPPING: &str = "Exchange aborted: overlapping text";

/// What the first `cx` marked, in its buffer: characters, anchored, or
/// rows, anchored too, with their first and last kept as places
/// (`Marks::exchange`) that follow edits as `a` to `z` do. The anchors
/// give what survives once a boundary row is deleted, or a row is split.
#[derive(Clone, Debug)]
pub(crate) struct Marked {
    buffer: BufferId,
    anchored: Anchored,
}

/// A region anchored in its text: text put in at either edge stays out of
/// it.
#[derive(Clone, Debug)]
struct Anchored {
    start: Anchor,
    end: Anchor,
    linewise: bool,
}

/// A region in one text: `range` is what its text is, a row's line break
/// left out, so rows are their rows' text.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Region {
    range: Range<usize>,
    linewise: bool,
}

impl Region {
    fn contains(&self, other: &Region) -> bool {
        self.range.start <= other.range.start && other.range.end <= self.range.end
    }

    fn overlaps(&self, other: &Region) -> bool {
        self.range.start < other.range.end && other.range.start < self.range.end
    }

    fn anchored(&self, snapshot: &BufferSnapshot) -> Anchored {
        Anchored {
            start: snapshot.anchor_after(self.range.start),
            end: snapshot.anchor_before(self.range.end),
            linewise: self.linewise,
        }
    }
}

impl Anchored {
    /// Its anchors' offsets in `snapshot`, if they can be resolved there:
    /// text put in where it was empty leaves it empty, at its end.
    fn offsets(&self, snapshot: &BufferSnapshot) -> Option<(usize, usize)> {
        let resolvable = |anchor: &Anchor| {
            anchor.version() <= snapshot.version() && anchor.version() >= snapshot.oldest_version()
        };
        if !resolvable(&self.start) || !resolvable(&self.end) {
            return None;
        }
        let end = snapshot.resolve(&self.end);
        Some((snapshot.resolve(&self.start).min(end), end))
    }

    /// Where it is in `snapshot`, if it can be resolved there.
    fn region(&self, snapshot: &BufferSnapshot) -> Option<Region> {
        let (start, end) = self.offsets(snapshot)?;
        let text = snapshot.text();
        let row = |offset| text.offset_to_point(offset).row;
        Some(Region {
            range: match self.linewise {
                true => text.row_start(row(start))..operator::row_end(text, row(end)),
                false => start..end,
            },
            linewise: self.linewise,
        })
    }
}

/// Takes what the first `cx` marked, as it is in `model`'s text: `None` if
/// nothing is, or it can't be found there (another buffer's, or both its
/// rows deleted).
fn take_marked(state: &mut VimState, model: &mut EditorModel) -> Option<Region> {
    let marked = state.exchange.take();
    let snapshot = model.snapshot().clone();
    let marked = marked.filter(|marked| marked.buffer == snapshot.id());
    let Some(marked) = marked else {
        state.marks.exchange = None;
        return None;
    };
    if !marked.anchored.linewise {
        return marked.anchored.region(&snapshot);
    }
    marks::catch_up(state);
    let [first, last] = state
        .marks
        .exchange
        .take()?
        .map(|place| Some(place?.position(&snapshot)?.0));
    let (start, end) = marked.anchored.offsets(&snapshot)?;
    let text = snapshot.text();
    let row = |offset| text.offset_to_point(offset).row;
    // A boundary row that lives is where its place says (the anchor
    // reaching further for the last, when a row was split); a deleted one
    // leaves the rows its anchor fell back to: the next row's start, or
    // for the last, the line break after the row before.
    if first.is_none() && last.is_none() && start == end {
        return None;
    }
    let first = first.unwrap_or_else(|| row(start));
    let last = match last {
        Some(last) => last.max(row(end)),
        None if end > start && end == text.row_start(row(end)) => row(end) - 1,
        None => row(end),
    };
    if last < first {
        return None;
    }
    Some(Region {
        range: text.row_start(first)..operator::row_end(text, last),
        linewise: true,
    })
}

/// Marks `region` of `snapshot`, the model's text, for the next `cx`.
fn keep(state: &mut VimState, model: &mut EditorModel, snapshot: &BufferSnapshot, region: &Region) {
    state.exchange = Some(Marked {
        buffer: snapshot.id(),
        anchored: region.anchored(snapshot),
    });
    state.marks.exchange = None;
    if region.linewise {
        // Followed from here on, as `m` sets a mark.
        state.marks.watch(model);
        marks::catch_up(state);
        let text = snapshot.text();
        let row = |offset| text.offset_to_point(offset).row;
        let place = |offset| Some(Place::at(snapshot, text.row_start(row(offset))));
        state.marks.exchange = Some([place(region.range.start), place(region.range.end)]);
    }
}

/// `x` with `c` pending starts `cx`; with `cx` pending, `x` takes `count`
/// rows (`cxx`) and `c` clears the mark (`cxc`).
fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    if pending.is_visual() {
        return None;
    }
    match (pending.operator()?, key) {
        (Operator::Change, VimKey::Char('x')) => Some(pending.clone().with_operator(OPERATOR)),
        (OPERATOR, VimKey::Char('x')) => Some(
            pending
                .clone()
                .operate(OPERATOR, Target::Motion(Motion::Line)),
        ),
        (OPERATOR, VimKey::Char('c')) => Some(invocation("vim.exchange_clear", ())),
        _ => None,
    }
}

fn register(registry: &mut Registry<VimCommands>) {
    let normal = ContextPredicate::parse("editor && vim.mode == normal")
        .expect("vim's context predicate parses");
    registry
        .register(
            "vim.exchange_clear",
            "Vim Exchange Clear",
            |target: &mut VimTarget<'_>, _: &Actor| {
                let had = target.state.exchange.take().is_some();
                target.state.marks.exchange = None;
                Ok::<_, CommandError>(Dispatched::new(match had {
                    true => Effect::Changed,
                    false => Effect::Nothing,
                }))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Clears the region the exchange operator (cx) marked, as vim-exchange's cxc does, so the next cx marks anew.",
        )
        .context(normal)
        .mutates_buffer(false);
}

/// `vim.operate` with `cx` over a motion or text object: marks what it
/// covers at the primary caret, or with a region marked, swaps the two.
/// Over a selection (what the `gc` object selects) it takes its rows,
/// leaving the visual mode; a selection not of rows takes nothing.
pub(crate) fn operate(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    if args.target.is_selection() {
        return selection(target, actor, args);
    }
    // `n N * #` as the search they are, and a mark's motion at the mark.
    let measured = match args.target {
        Target::Motion(motion) => {
            let motion = cmdline::resolve(target, motion).map_or(motion, |(resolved, _)| resolved);
            Target::Motion(marks::resolve(target, motion).unwrap_or(motion))
        }
        other => other,
    };
    operator::sync(target.model);
    let snapshot = target.model.snapshot().clone();
    let text = snapshot.text().clone();
    let tab = target.model.tab_size().max(1);
    let head = Cursor::at(&text, target.model.primary_selection().head()).offset();
    let state = &mut *target.state;
    let (span, reached) = match measured {
        Target::Motion(motion) => {
            let last_find = state.last_find;
            if let Motion::FindForward(_)
            | Motion::FindBackward(_)
            | Motion::TillForward(_)
            | Motion::TillBackward(_) = motion
            {
                state.last_find = Some(motion);
            }
            let span = operator::motion_span(&text, head, motion, args.count, last_find, tab);
            // Where the motion lands, for where rows it covers start.
            let mut cursor = Cursor::at(&text, head);
            cursor.op = true;
            let moved = MoveArgs {
                motion,
                extend: false,
                count: args.count,
            };
            let reached = motion::motion(&mut cursor, moved, last_find, None, tab)
                .ok()
                .map(|_| cursor.offset());
            (span, reached)
        }
        Target::TextObject(object) => (
            text_object::select(&text, head, object, args.count.unwrap_or(1), tab),
            None,
        ),
        Target::Selection | Target::SelectedLines | Target::SelectedToEnd => (Span::Failed, None),
    };
    exchange(target, actor, args, span, head, reached)
}

/// `cx` over the rows a linewise selection covers.
fn selection(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    operator::sync(target.model);
    let rows = match target.state.mode {
        Mode::Visual(VisualKind::Line) => visual::live_area(target.model, target.state)
            .and_then(|area| Some((area.rows.first()?.0, area.rows.last()?.0))),
        _ => None,
    };
    visual::exit(target)?;
    let Some((first, last)) = rows else {
        target.state.failed = true;
        return Ok(Dispatched::new(Effect::Changed));
    };
    let span = Span::Lines {
        rows: first..=last,
        first_non_blank: false,
        from: None,
    };
    let head = target.model.text().row_start(first);
    exchange(target, actor, args, span, head, None)
}

/// `cx` over `span`, measured for the caret at `head`, a motion having
/// `reached` there: marks it, or with a region marked, swaps the two.
fn exchange(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
    span: Span,
    head: usize,
    reached: Option<usize>,
) -> Result<Dispatched<TransactionId>, CommandError> {
    operator::sync(target.model);
    let snapshot = target.model.snapshot().clone();
    let text = snapshot.text();
    let state = &mut *target.state;
    let region = match &span {
        Span::Failed => {
            state.failed = true;
            return Ok(Dispatched::new(Effect::Nothing));
        }
        // A failed `b` still moves the caret, as for any operator.
        Span::Move(offset) => {
            state.failed = true;
            let before = target.model.selections();
            let caret = Cursor::at(text, *offset).offset();
            target.model.set_selections([operator::caret(caret)]);
            state.goals = None;
            return Ok(Dispatched::new(match target.model.selections() != before {
                true => Effect::Changed,
                false => Effect::Nothing,
            }));
        }
        Span::Chars(range) => Region {
            range: range.clone(),
            linewise: false,
        },
        Span::Lines { rows, .. } => Region {
            range: text.row_start(*rows.start())..operator::row_end(text, *rows.end()),
            linewise: true,
        },
    };
    // `.` repeats a `cx` that marked or swapped; one that failed leaves
    // what it repeated.
    let Some(first) = take_marked(target.state, target.model) else {
        target.state.set_last_change(Change::Operate(args).into());
        return Ok(mark(target, &snapshot, region, &span, head, reached));
    };
    if first.overlaps(&region) && !first.contains(&region) && !region.contains(&first) {
        return Ok(target.state.fail(OVERLAPPING));
    }
    swap(target, actor, args, &snapshot, (first, region))
}

/// The first `cx`: keeps `region` and puts the caret and `'[` and `']`
/// where `g@` over `span` would.
fn mark(
    target: &mut VimTarget<'_>,
    snapshot: &BufferSnapshot,
    region: Region,
    span: &Span,
    head: usize,
    reached: Option<usize>,
) -> Dispatched<TransactionId> {
    let VimTarget { model, state, .. } = target;
    keep(state, model, snapshot, &region);
    let text = snapshot.text();
    if let Some((start, end)) = comment::ends(text, span, head, reached) {
        let caret = Cursor::at(text, start).offset();
        model.set_selections([operator::caret(caret)]);
        state.goals = None;
        let ends = (history::pair(text, start), history::pair(text, end));
        marks::recased(state, model, None, ends.0, ends.1);
    }
    Dispatched::new(Effect::Changed)
}

/// The second `cx`: `first`'s text and `second`'s swapped, or the
/// contained one's in place of the other, as one transaction. Both are
/// anchored in `snapshot` and found again in the text the edits go into.
fn swap(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
    snapshot: &BufferSnapshot,
    (first, second): (Region, Region),
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget { model, state, .. } = target;
    let anchored = (first.anchored(snapshot), second.anchored(snapshot));
    // Where what fills `second` starts once they're in, and where the
    // first edit begins, in the text before.
    let mut placed: Option<(Region, usize, (u32, usize))> = None;
    let (transaction, snapshot) = operator::apply_plan(model, origin(actor), |snapshot| {
        let (Some(first), Some(second)) =
            (anchored.0.region(snapshot), anchored.1.region(snapshot))
        else {
            return Vec::new();
        };
        let text = snapshot.text();
        let of = |region: &Region| text.slice_to_string(region.range.clone());
        let edits = if second.contains(&first) {
            vec![(second.range.clone(), of(&first))]
        } else if first.contains(&second) {
            vec![(first.range.clone(), of(&second))]
        } else if first.overlaps(&second) {
            return Vec::new();
        } else {
            let (into_first, into_second) = match first.linewise && second.linewise {
                true => (
                    reindented(&of(&second), &of(&first)),
                    reindented(&of(&first), &of(&second)),
                ),
                false => (of(&second), of(&first)),
            };
            let mut edits = vec![
                (first.range.clone(), into_first),
                (second.range.clone(), into_second),
            ];
            edits.sort_by_key(|(range, _)| range.start);
            edits
        };
        // After a contained region's text replaced its container, that's
        // where; else past what replaced `first`, if that's before it.
        let filled = match first.contains(&second) {
            true => first.range.start,
            false => edits
                .iter()
                .take_while(|(range, _)| range.start < second.range.start)
                .fold(second.range.start, |at, (range, new)| {
                    at + new.len() - range.len()
                }),
        };
        let begin = history::pair(text, edits[0].0.start);
        placed = Some((second, filled, begin));
        edits
    });
    let Some((second, filled, begin)) = placed else {
        return Ok(state.fail(OVERLAPPING));
    };
    let after = snapshot.text();
    let filled = filled.min(after.len());
    let caret = match second.linewise {
        true => {
            let mut cursor = Cursor::at(after, after.row_start(after.offset_to_point(filled).row));
            cursor.go_to_first_non_blank();
            cursor.offset()
        }
        false => filled,
    };
    operator::land(model, &snapshot, [caret]);
    state.goals = None;
    state.set_last_change(Change::Operate(args).into());
    if let Some(id) = transaction {
        // `u` puts the caret where the first change begins.
        state.carets.record(id, vec![begin]);
    }
    Ok(Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect()))
}

/// Rows `moved` with the indentation of `replaced`'s first non-blank row in
/// place of their own's: each row starting with that indentation and not
/// blank. Either with no non-blank row, `moved` as it is.
fn reindented(moved: &str, replaced: &str) -> String {
    let indent = |rows: &str| -> Option<String> {
        rows.split('\n')
            .find(|row| !row.trim().is_empty())
            .map(|row| {
                row.chars()
                    .take_while(|ch| *ch == ' ' || *ch == '\t')
                    .collect()
            })
    };
    let (Some(own), Some(new)) = (indent(moved), indent(replaced)) else {
        return moved.to_string();
    };
    if own == new {
        return moved.to_string();
    }
    moved
        .split('\n')
        .map(|row| match row.strip_prefix(own.as_str()) {
            Some(rest) if !row.trim().is_empty() => format!("{new}{rest}"),
            _ => row.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}
