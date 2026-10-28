//! Prose: the paragraph and sentence motions `{ } ( )` and objects `ip ap
//! is as` (a feature module; see [`crate::feature`]), as nvim 0.12.4 moves
//! and selects with its default options (`:help paragraph`, `:help
//! sentence`; 'cpoptions' without `J`, and no row starting with an nroff
//! macro).
//!
//! In normal mode, the visual modes and after an operator, `}` and `{` go
//! to the `count`th next or previous paragraph boundary: an empty row (one
//! with only blanks isn't), or else the text's last char or its start. `)`
//! and `(` go to the `count`th next or previous sentence start. A sentence
//! ends at `.`, `!` or `?` followed by any number of `)`, `]`, `"` and `'`
//! and then a space, a tab or the row's end, and the next one starts at the
//! first non-blank after that; an empty row is one too, and so is the first
//! non-blank after one. A count that would step on past the text's end (or
//! start) fails, nothing moving and the jump mark staying, though a single
//! step there lands on the text's last char (or first).
//!
//! All four are exclusive, so the `:help exclusive` rules make `d}` from a
//! paragraph's first column linewise; at the text's last char they take
//! it, as nvim's become inclusive there. Each is a jump, setting `''`, and a
//! delete over one writes register `1` even within a row (`:help
//! quote_number`).
//!
//! `ip` and `ap` select whole rows (a visual mode becomes linewise); `is`
//! and `as` select chars (it becomes charwise). For the objects a row with
//! only blanks is a paragraph boundary too. `ip` is the run of text rows,
//! or of blank ones, holding the caret, and a count adds the runs after it;
//! `ap` is a run of text rows with the blank rows after it (or, with none
//! after, those before it), or from a blank row, the blank rows and the text
//! rows after them. `is` is the sentence holding the caret, or the blanks
//! between two sentences; `as` takes a sentence with the blanks after it
//! (or, with none, those before it), or blanks with the sentence after
//! them. Near the text's end nvim's own quirks are kept: a `)` that finds
//! the start it began on looks again from the next position, and an `is`
//! or `as` whose start is past the text takes the range between its ends.
//!
//! In a visual mode over one position `ip` and `ap` select as after an
//! operator (over one row of a linewise selection that's a run of its own
//! they take one run more, that row being selected already); otherwise they
//! add `count` runs from the row past the cursor, the way it faces, keeping
//! the mode, and running out of text keeps what they got to and fails.
//!
//! A visual `is` or `as` over one position selects as after an operator
//! too, made charwise, except where nvim's differs: it takes an empty row as
//! blanks before the next sentence; a sentence of one char it would take
//! alone grows by an item if there are more to take or blanks after it; on
//! a sentence of punctuation alone (`!` before an empty row, say) it keeps
//! the anchor, even at a row's end; and on blanks alone two items or more
//! keep the anchor and go back to the start. Otherwise it grows the way the
//! cursor faces by `count` items (two for `as`), keeping the mode: going
//! forward, a sentence item ends before the blanks before the next sentence
//! and an empty row is two; going back, a sentence item goes to its start
//! and an empty row is one. A cursor at a row's end (after `$`) grows from
//! there, as nvim's does. It never fails, at a text's end keeping what it
//! got to, and a huge count takes no longer than the text (nvim's takes
//! minutes). These rules are fitted to nvim's results over more than
//! 30,000 probes. They match all but 2 of 4,643 fresh ones over prose, and
//! 23 of 3,814 over text made mostly of marks and blanks: more sentences of
//! punctuation alone (several in a row, or starting the text) and some
//! selections next to rows of only blanks (one over them to the text's end,
//! say).
//!
//! An `is` or `as` leaves the jump mark alone, where nvim's moves it to
//! wherever its last search of a sentence started.

use std::{cmp::Ordering, ops::RangeInclusive};

use fe_buffer::TransactionId;
use fe_command::{CommandError, Dispatched, Effect};
use fe_text::Text;

use crate::{
    Mode, MoveArgs, Pending, SelectArgs, Step, TextObject, VimKey, VimTarget, VisualKind,
    args::{Motion, RangeClass},
    feature::Feature,
    motion::{Cursor, Line},
    operator::Span,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    object,
    ..Feature::NONE
};

/// `{ } ( )`, committed as any motion is: moved, extended or operated over.
fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    let motion = match key {
        VimKey::Char('}') => Motion::ParagraphForward,
        VimKey::Char('{') => Motion::ParagraphBackward,
        VimKey::Char(')') => Motion::SentenceForward,
        VimKey::Char('(') => Motion::SentenceBackward,
        _ => return None,
    };
    Some(pending.clone().motion(motion))
}

/// `p` and `s` after `i` or `a`.
fn object(_pending: &Pending, ch: char, inner: bool) -> Option<TextObject> {
    Some(match (ch, inner) {
        ('p', true) => TextObject::InnerParagraph,
        ('p', false) => TextObject::AParagraph,
        ('s', true) => TextObject::InnerSentence,
        ('s', false) => TextObject::ASentence,
        _ => return None,
    })
}

/// `motion`'s class as an operator's target ([`Motion::class`]).
pub(crate) fn class(_motion: Motion) -> RangeClass {
    RangeClass::Exclusive
}

/// Whether `motion` is a jump ([`Motion::is_jump`]).
pub(crate) fn is_jump(_motion: Motion) -> bool {
    true
}

/// Whether a delete over `motion` writes register `1` even within a row
/// ([`crate::Target::writes_numbered`]).
pub(crate) fn writes_numbered(_motion: Motion) -> bool {
    true
}

/// Moves one caret by `args.motion`, as `motion::motion` does: `Ok` holds
/// the goal column it keeps, `Err` the goal to keep when it fails.
pub(crate) fn motion(
    cursor: &mut Cursor,
    args: MoveArgs,
    goal: Option<usize>,
    _tab: usize,
) -> Result<Option<usize>, Option<usize>> {
    let count = args.count.unwrap_or(1).max(1);
    let text = cursor.text;
    // Whether it ran out at the text's end, onto its last char, which an
    // operator then takes.
    let (to, past_end) = match args.motion {
        Motion::ParagraphForward | Motion::ParagraphBackward => {
            let forward = args.motion == Motion::ParagraphForward;
            let row = paragraph(text, cursor.row, count, forward).ok_or(None)?;
            let past_end = forward && row == text.row_count() - 1;
            (text.row_start(row), past_end)
        }
        _ => {
            let forward = args.motion == Motion::SentenceForward;
            let mut at = Walk::at(text, cursor.row, cursor.ix);
            // Steps that get nowhere (at a text's ends, or on punctuation
            // alone) are skipped in rounds, so a huge count is quick.
            let (count, mut cycle) = (u64::from(count), Cycle::default());
            let mut step = 0;
            while step < count {
                step += 1;
                let last = step == count;
                let went = match forward {
                    true => at.sentence_forward(last),
                    false => at.sentence_backward(last),
                };
                if !went {
                    return Err(goal);
                }
                if !last {
                    step += cycle.skip(at.state(forward), count - step);
                }
            }
            (at.offset(), at.past_end)
        }
    };
    cursor.goto(to);
    // A visual cursor stands past the last char after `)` or `(`, not `}`.
    cursor.hit_end = past_end && args.motion != Motion::ParagraphForward;
    if past_end {
        cursor.ix = match cursor.op {
            true => cursor.line.len(),
            false => cursor.line.last(),
        };
    }
    Ok(None)
}

/// Whether `motion` (`count` times) from the caret at `offset` gets
/// anywhere, as a jump must to set the jump mark.
pub(crate) fn lands(text: &Text, offset: usize, motion: Motion, count: Option<u32>) -> bool {
    let args = MoveArgs {
        motion,
        extend: false,
        count,
    };
    self::motion(&mut Cursor::whole(text, offset), args, None, 1).is_ok()
}

/// The row `count` paragraph boundaries from `row`: each step goes on to
/// the next empty row after a row with text (the one it starts on counts),
/// or stops at the text's last row (first, going back). `None` if a step
/// before the last runs out of text.
fn paragraph(text: &Text, row: u32, count: u32, forward: bool) -> Option<u32> {
    let last_row = text.row_count() - 1;
    let empty = |row: u32| text.row_len(row) == 0;
    let mut row = row;
    for step in 1..=count {
        let mut seen_text = false;
        let mut first = true;
        loop {
            seen_text |= !empty(row);
            if !first && seen_text && empty(row) {
                break;
            }
            first = false;
            let at_end = match forward {
                true => row == last_row,
                false => row == 0,
            };
            if at_end {
                if step < count {
                    return None;
                }
                break;
            }
            row = match forward {
                true => row + 1,
                false => row - 1,
            };
        }
    }
    Some(row)
}

/// What `object` covers around the caret at `offset`, `count` times, as
/// `text_object::select` says.
pub(crate) fn select(text: &Text, offset: usize, object: TextObject, count: u32) -> Span {
    let count = count.max(1);
    let row = text.offset_to_point(offset).row;
    match object {
        TextObject::InnerParagraph | TextObject::AParagraph => {
            let around = object == TextObject::AParagraph;
            match paragraph_rows(text, row, count, around) {
                (rows, taken) if taken == count => Span::Lines {
                    from: Some(text.row_start(*rows.start())),
                    rows,
                    first_non_blank: false,
                },
                _ => Span::Failed,
            }
        }
        _ => {
            let cursor = Cursor::whole(text, offset);
            let at = Walk::at(text, cursor.row, cursor.ix);
            sentences(at, count, object == TextObject::ASentence)
        }
    }
}

/// `vim.select` with `ip ap is as`: over one position, the object there
/// (`ip` and `ap` over one row of a linewise selection take one more run if
/// that row is a run of its own, as it's already selected); otherwise, the
/// selection grows from the position past the cursor, the way it faces, by
/// `count` objects, keeping its mode. `ip` and `ap` make the mode linewise
/// and `is` and `as` charwise when they select. Nothing to select fails.
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
        return Err(CommandError::new("no visual selection"));
    };
    let text = model.text().clone();
    let count = args.count.unwrap_or(1).max(1);
    let selected = match args.text_object {
        TextObject::InnerParagraph | TextObject::AParagraph => {
            let around = args.text_object == TextObject::AParagraph;
            let Some((kind, anchor, cursor, done)) =
                visual_paragraphs(&text, kind, anchor, cursor, count, around)
            else {
                state.failed = true;
                return Ok(Dispatched::new(Effect::Nothing));
            };
            state.select_visual(model, kind, anchor, cursor);
            state.failed |= !done;
            return Ok(Dispatched::new(Effect::Changed));
        }
        _ => {
            let around = args.text_object == TextObject::ASentence;
            // Over one position it's made charwise, else it keeps its mode.
            let to_kind = match anchor == cursor {
                true => VisualKind::Char,
                false => kind,
            };
            (
                to_kind,
                visual_sentences(&text, anchor, cursor, count, around),
            )
        }
    };
    let (to_kind, (to_anchor, to_cursor)) = selected;
    if (to_kind, to_anchor, to_cursor) == (kind, anchor, cursor) {
        return Ok(Dispatched::new(Effect::Nothing));
    }
    state.select_visual(model, to_kind, to_anchor, to_cursor);
    Ok(Dispatched::new(Effect::Changed))
}

/// `ip` (or `ap`) in a visual mode of `kind`: the new kind, anchor and
/// cursor (at the start of the row it gets to), and whether it took every
/// run; `None` if it selects nothing. Growing, running out of text keeps
/// the rows it got to.
fn visual_paragraphs(
    text: &Text,
    kind: VisualKind,
    anchor: usize,
    cursor: usize,
    count: u32,
    around: bool,
) -> Option<(VisualKind, usize, usize, bool)> {
    let row = |offset: usize| text.offset_to_point(offset).row;
    let (from, to) = (row(anchor), row(cursor));
    if from == to {
        let (rows, taken) = paragraph_rows(text, to, count, around);
        if taken < count {
            return None;
        }
        // The anchor goes to the first row's start.
        let anchor = match *rows.start() == from {
            true => anchor,
            false => text.row_start(*rows.start()),
        };
        let (mut far, mut done) = (*rows.end(), true);
        // A linewise row that's a run of its own was that run already
        // selected: one run more, if there is one.
        let own_run = run_edge(text, to, false) == to && run_edge(text, to, true) == to;
        if kind == VisualKind::Line && own_run {
            done = far + 1 < text.row_count();
            if done {
                far = run_edge(text, far + 1, true);
            }
        }
        return Some((VisualKind::Line, anchor, text.row_start(far), done));
    }
    let forward = to > from;
    let next = match forward {
        true => (to + 1 < text.row_count()).then(|| to + 1),
        false => to.checked_sub(1),
    };
    let Some(next) = next else {
        return Some((kind, anchor, text.row_start(to), false));
    };
    let Runs { far, taken, .. } = runs(text, next, count, around, forward);
    Some((kind, anchor, text.row_start(far), taken == count))
}

/// `is` (or `as`) in a visual mode: the new anchor and cursor. Over one
/// position it selects as [`initial`] says; otherwise the cursor goes on by
/// `count` objects the way it faces ([`grow_forward`], [`grow_backward`]).
/// It never fails: at the text's ends it keeps what it got to.
fn visual_sentences(
    text: &Text,
    anchor: usize,
    cursor: usize,
    count: u32,
    around: bool,
) -> (usize, usize) {
    let walk = |offset: usize| {
        let at = Cursor::whole(text, offset);
        Walk::at(text, at.row, at.ix)
    };
    let items = items(count, around);
    match cursor.cmp(&anchor) {
        Ordering::Equal => {
            // Both at the text's end after a sentence's last mark: the anchor
            // stays and the cursor takes that mark.
            let at = Walk::visual(text, cursor);
            if at.past_end
                && at.row + 1 == text.row_count()
                && at.ch_before().is_some_and(ends_sentence)
            {
                let mut mark = at.copy();
                mark.past_end = false;
                return (cursor, mark.offset());
            }
            let (start, end) = initial(walk(cursor), count, around);
            (start.end_offset(), end.offset())
        }
        Ordering::Greater => (
            anchor,
            grow_forward(Walk::visual(text, cursor), items).offset(),
        ),
        Ordering::Less => {
            let at = grow_backward(Walk::visual(text, cursor), items, around);
            (anchor, at.offset())
        }
    }
}

/// How many items `count` `is` (or `as`) objects are: `as` is two each, a
/// sentence and blanks, or blanks and a sentence.
fn items(count: u32, around: bool) -> u64 {
    u64::from(count) * if around { 2 } else { 1 }
}

/// The cursor after growing `items` items forward from `cursor`: the next
/// item starts at the position after it, blanks before a sentence or else
/// the sentence holding it.
fn grow_forward(cursor: Walk<'_>, items: u64) -> Walk<'_> {
    let mut after = cursor.copy();
    after.next();
    let in_blanks = after.blank() && {
        let (mut blanks, mut next) = (after.copy(), cursor.copy());
        blanks.skip_blanks();
        next.sentence_forward(true);
        blanks.same(&next)
    };
    let (mut at, sentence_next) = match in_blanks {
        true => (cursor, false),
        false => (after, true),
    };
    at.forward_items(items, sentence_next);
    at
}

/// The cursor after growing `items` items back from `cursor`, nvim's
/// way: a sentence item goes to the sentence's start (the first from the
/// one the cursor's in, which can be after it), a blanks item back over the
/// blanks before a sentence. They alternate, blanks first if the cursor is
/// at a sentence's start or just after blanks before one. For `is` an item
/// of blanks that isn't there (a sentence after an empty row, say) goes to
/// the sentence before instead; for `as` it's an item that moves nowhere.
fn grow_backward(cursor: Walk<'_>, items: u64, around: bool) -> Walk<'_> {
    let mut start = cursor.copy();
    start.sentence_forward(true);
    start.sentence_backward(true);
    let mut before = cursor.copy();
    // Just after blanks before a sentence, that start not after the cursor
    // unless it's on a blank (from a row's end it goes on to it).
    let after_blanks = !cursor.empty() && before.prev() && before.blank() && {
        let (mut blanks, mut next) = (before.copy(), before.copy());
        blanks.skip_blanks();
        next.sentence_forward(true);
        blanks.same(&next) && (cursor.blank() || start.precedes(&cursor))
    };
    let mut blanks_next = start.same(&cursor) || after_blanks;
    let mut at = cursor;
    let mut cycle = Cycle::default();
    let mut item = 0;
    while item < items {
        if blanks_next {
            let mut before = at.copy();
            match around || (before.prev() && before.blank()) {
                true => at.back_to_blanks(),
                false => {
                    at.sentence_backward(true);
                }
            }
        } else if item == 0 {
            at = start.copy();
        } else {
            at.sentence_backward(true);
        }
        item += 1;
        blanks_next = !blanks_next;
        item += cycle.skip(at.state(blanks_next), items - item);
    }
    at
}

/// `is` (or `as`) in a visual mode over one position, `cursor`: the
/// operator's ends ([`sentence_ends`]), except where nvim's differ (see the
/// module docs): on empty rows, on a sentence of one char or of punctuation
/// alone, and on blanks alone. The start can be past the row's last char.
fn initial(cursor: Walk<'_>, count: u32, around: bool) -> (Walk<'_>, Walk<'_>) {
    let text = cursor.text;
    let items = items(count, around);
    if cursor.empty() {
        let row = cursor.row;
        let empty = |row: u32| text.row_len(row) == 0;
        let last = (row..text.row_count()).take_while(|&row| empty(row)).last();
        let last = last.unwrap_or(row);
        if last != row {
            // Not a run's last empty row: up to it, or (oddly) back to the
            // end of the text before a run of three or more, for an item or
            // two (one if the row after the run starts with blanks), else
            // as from the last.
            let blanks_after = last + 1 < text.row_count() && Walk::at(text, last + 1, 0).blank();
            if items > 2 - u64::from(blanks_after) {
                return initial(Walk::at(text, last, 0), count, around);
            }
            let first = (0..=row).rev().take_while(|&row| empty(row)).last();
            let mut to = Walk::at(text, first.unwrap_or(row), 0);
            if last == row + 1 || !to.prev() {
                to = Walk::at(text, last, 0);
            }
            return (cursor, to);
        }
        return match last + 1 < text.row_count() {
            true => empty_row(cursor, count, around),
            false => sentence_ends(cursor, count, around),
        };
    }
    let (start, mut end) = sentence_ends(cursor.copy(), count, around);
    let punctuation = |at: &Walk| {
        at.ch()
            .is_some_and(|ch| ends_sentence(ch) || CLOSERS.contains(ch))
    };
    // Where the operator's start goes on past a sentence of punctuation
    // alone at the cursor, onto one position (or onto an empty row, for an
    // item or two), nvim keeps the anchor.
    let keeps = cursor.precedes(&start) && punctuation(&cursor) && start.same(&end);
    // A sentence of one char taken alone grows by an item when there are
    // more to take, or blanks after it.
    let mut after = cursor.copy();
    let alone = start.same(&cursor) && end.same(&cursor) && !cursor.blank();
    if alone && after.next() && (items >= 2 || after.blank()) {
        end = grow_forward(cursor.copy(), 1);
    }
    if keeps {
        // The one position starts the next item: an empty row is itself (and
        // takes one more if the mark has blanks after it), a sentence goes
        // on over its blanks.
        let mut next = cursor.copy();
        let blanks = next.next() && next.row == cursor.row && next.blank();
        let more = match start.empty() {
            true => items.saturating_sub(2 - u64::from(blanks)),
            false => items,
        };
        return match more {
            0 => (cursor, end),
            _ => (cursor, grow_forward(end, more)),
        };
    }
    // On a sentence of punctuation alone whose operator's range is one
    // position before it, the cursor goes back from there a sentence per
    // item.
    if punctuation(&cursor) && start.same(&end) && start.precedes(&cursor) {
        let mut back = start;
        back.sentences_back(items);
        return (cursor, back);
    }
    // Blanks back to the text's start with nothing after: the anchor stays
    // and the cursor goes to the start, for two items or more.
    if start.at_start() && start.same(&end) && start.blank() && items >= 2 {
        return (cursor, start);
    }
    (start, end)
}

/// [`initial`] on the last of a run of empty rows (or a lone one) with text
/// after it: blanks before the next sentence, which nvim counts with a
/// repeated item, after the first sentence or (when the next row starts
/// with blanks) after the blanks. `as` takes the blanks before the empty
/// row when it ends at a sentence with none after, or, once, alone.
fn empty_row(cursor: Walk<'_>, count: u32, around: bool) -> (Walk<'_>, Walk<'_>) {
    let text = cursor.text;
    let items = items(count, around);
    let mut before = cursor.copy();
    before.back_to_blanks();
    let next_row = Walk::at(text, cursor.row + 1, 0);
    let grow = match next_row.blank() {
        true => items.saturating_sub(1).max(1),
        false if items == 1 => return (cursor.copy(), cursor),
        false if around && count == 1 && !before.same(&cursor) => return (before, cursor),
        false => items.saturating_sub(2).max(1),
    };
    let end = grow_forward(cursor.copy(), grow);
    let mut after = end.copy();
    let blanks_after = end.blank() || (after.next() && after.blank());
    let start = match around && !blanks_after {
        true => before,
        false => cursor,
    };
    (start, end)
}

/// Steps that come back to where they were two steps before go round for
/// good (at a text's end): [`Cycle::skip`] skips whole rounds, so a huge
/// count takes no longer than the text.
#[derive(Default)]
struct Cycle {
    /// The states two steps and one step ago.
    states: [Option<State>; 2],
}

/// A walk's position and which item is next.
type State = (u32, usize, bool, bool);

impl Cycle {
    /// How many of the `left` steps after reaching `state` to skip: an even
    /// number, leaving one or two, if it's going round.
    fn skip(&mut self, state: State, left: u64) -> u64 {
        let skip = match self.states[0] == Some(state) {
            true => left.saturating_sub(1) / 2 * 2,
            false => 0,
        };
        self.states = [self.states[1], Some(state)];
        skip
    }
}

/// Whether `row` has only blanks, if any: a paragraph boundary for `ip`
/// and `ap`.
fn blank_row(text: &Text, row: u32) -> bool {
    text.row_to_string(row)
        .bytes()
        .all(|byte| byte == b' ' || byte == b'\t')
}

/// The rows `ip` (or `ap` if `around`) covers `count` times from `row`:
/// runs of text rows and of blank rows, and how many it took (fewer than
/// `count` if it ran out of text, the rows those it got to).
fn paragraph_rows(text: &Text, row: u32, count: u32, around: bool) -> (RangeInclusive<u32>, u32) {
    let start = run_edge(text, row, false);
    let Runs {
        far,
        trailing,
        taken,
    } = runs(text, start, count, around, true);
    // `ap` with no blank rows after its last paragraph takes those before
    // its first.
    let first = match trailing || blank_row(text, start) || start == 0 {
        true => start,
        false => run_edge(text, start - 1, false),
    };
    (first..=far, taken)
}

/// The last row (first if not `forward`) of the run of text rows, or of
/// blank ones, holding `row`.
fn run_edge(text: &Text, row: u32, forward: bool) -> u32 {
    let blank = blank_row(text, row);
    let same = |row: &u32| blank_row(text, *row) == blank;
    match forward {
        true => (row..text.row_count()).take_while(same).last(),
        false => (0..=row).rev().take_while(same).last(),
    }
    .unwrap_or(row)
}

/// Where [`runs`] got to.
struct Runs {
    far: u32,
    /// Whether the last text rows had blank rows after them.
    trailing: bool,
    /// How many runs (or for `ap`, paragraphs) it took whole, fewer than
    /// asked if it ran out of text.
    taken: u32,
}

/// `count` runs from `row` on (back if not `forward`), the first taken from
/// `row`: for `ap` (`around`), each a run of text rows and the blank rows
/// after it, or blank rows and the text rows after them, which there must
/// be.
fn runs(text: &Text, row: u32, count: u32, around: bool, forward: bool) -> Runs {
    let last_row = text.row_count() - 1;
    let step = |row: u32| match forward {
        true => (row < last_row).then(|| row + 1),
        false => row.checked_sub(1),
    };
    let mut next = Some(row);
    let mut runs = Runs {
        far: row,
        trailing: true,
        taken: 0,
    };
    for _ in 0..count {
        let Some(from) = next else {
            break;
        };
        runs.far = run_edge(text, from, forward);
        next = step(runs.far);
        if around {
            match next {
                Some(row) => {
                    runs.far = run_edge(text, row, forward);
                    next = step(runs.far);
                }
                None if blank_row(text, row) => break,
                None => runs.trailing = false,
            }
        }
        runs.taken += 1;
    }
    runs
}

/// What `is` (or `as` if `around`) covers `count` times from `at`: a
/// sentence or the blanks after it, each one item.
fn sentences(at: Walk, count: u32, around: bool) -> Span {
    let text = at.text;
    let (start, end) = sentence_ends(at, count, around);
    // Through `end`: up to the next position (a row's end taking its line
    // break), or at the text's last, including it. Near the end, `end` can
    // come before `start` (past the text): the range is between them, the
    // later one taken as `end` would have been.
    let mut after = end.copy();
    let inclusive = !after.next();
    let (stop, stop_len) = match inclusive {
        true => (end.end_offset(), end.len()),
        false => (after.offset(), 0),
    };
    let begin = start.end_offset();
    let (from, to, inclusive) = match begin <= stop {
        true => (begin, stop + stop_len, inclusive),
        false => (
            stop,
            begin + if inclusive { start.len() } else { 0 },
            inclusive,
        ),
    };
    // Ending at a later row's start, from at or before the first non-blank
    // the rows are whole, else it ends at the row before's end
    // (`:help exclusive-linewise`).
    let row = |offset: usize| text.offset_to_point(offset).row;
    let (first, last) = (row(from), row(to));
    if !inclusive && last > first && text.row_start(last) == to {
        let indent = text.slice_to_string(text.row_start(first)..from);
        if indent.bytes().all(|byte| byte == b' ' || byte == b'\t') {
            return Span::Lines {
                rows: first..=last - 1,
                first_non_blank: true,
                from: Some(from),
            };
        }
        return Span::Chars(from..text.row_start(last - 1) + text.row_len(last - 1) as usize);
    }
    Span::Chars(from..to)
}

/// The first and last positions `is` (or `as`) covers from `at`.
fn sentence_ends(at: Walk<'_>, count: u32, around: bool) -> (Walk<'_>, Walk<'_>) {
    // On blanks just before the next sentence: they're the first item.
    let (blank, mut end) = at.in_blanks();
    let mut start = at.copy();
    if blank {
        start.back_to_blanks();
    } else {
        end.sentence_backward(true);
        start = end.copy();
    }
    let items = items(count, around) - u64::from(blank && !around);
    if items == 0 {
        end.prev();
    }
    end.forward_items(items, true);
    if around {
        if blank {
            // Blanks then a sentence: none after it.
            end.back_to_blanks();
            if end.ch().is_some_and(is_blank) {
                end.prev();
            }
        } else if !end.ch().is_some_and(is_blank) {
            // A sentence with no blanks after it takes those before it.
            start.back_to_blanks();
        }
    }
    (start, end)
}

/// Closes a sentence after its `.`, `!` or `?`.
const CLOSERS: &str = ")]\"'";

fn ends_sentence(ch: char) -> bool {
    matches!(ch, '.' | '!' | '?')
}

fn is_blank(ch: char) -> bool {
    matches!(ch, ' ' | '\t')
}

/// A position sentences are found over: a grapheme on a row, or an empty
/// row (a row's end is no position of its own, but for a visual cursor
/// there, or a step running into the text's end).
struct Walk<'a> {
    text: &'a Text,
    row: u32,
    ix: usize,
    line: Line,
    /// Whether it's past the row's last char: at the text's end, or a
    /// visual cursor at a row's end.
    past_end: bool,
}

impl<'a> Walk<'a> {
    fn at(text: &'a Text, row: u32, ix: usize) -> Self {
        let line = Line::new(text, row);
        let ix = ix.min(line.last());
        Self {
            text,
            row,
            ix,
            line,
            past_end: false,
        }
    }

    /// At a visual cursor's `offset`, which can be a row's end.
    fn visual(text: &'a Text, offset: usize) -> Self {
        let cursor = Cursor::whole(text, offset);
        let mut at = Walk::at(text, cursor.row, cursor.ix);
        let row_end = text.row_start(at.row) + text.row_len(at.row) as usize;
        at.past_end = !at.empty() && offset == row_end;
        at
    }

    fn offset(&self) -> usize {
        self.line.start + self.line.starts.get(self.ix).copied().unwrap_or(0)
    }

    /// The grapheme's length; 0 on an empty row or past the end.
    fn len(&self) -> usize {
        match self.empty() || self.past_end {
            true => 0,
            false => self.line.grapheme(self.ix).len(),
        }
    }

    /// The offset, past the end its row's end.
    fn end_offset(&self) -> usize {
        match self.past_end {
            true => self.line.start + self.line.text.len(),
            false => self.offset(),
        }
    }

    fn empty(&self) -> bool {
        self.line.len() == 0
    }

    /// The grapheme's base char; `None` on an empty row or past the end.
    fn ch(&self) -> Option<char> {
        (!self.empty() && !self.past_end)
            .then(|| self.line.grapheme(self.ix).chars().next().unwrap_or(' '))
    }

    /// The base char of the grapheme at the position (the last one of its
    /// row, past the end).
    fn ch_before(&self) -> Option<char> {
        (!self.empty()).then(|| self.line.grapheme(self.ix).chars().next().unwrap_or(' '))
    }

    fn copy(&self) -> Walk<'a> {
        Walk {
            past_end: self.past_end,
            ..Walk::at(self.text, self.row, self.ix)
        }
    }

    fn same(&self, other: &Walk) -> bool {
        (self.row, self.ix, self.past_end) == (other.row, other.ix, other.past_end)
    }

    /// Back to the first of the blanks just before this position (across
    /// row ends, not past an empty row).
    fn back_to_blanks(&mut self) {
        while self.prev() {
            if !self.ch().is_some_and(is_blank) {
                self.next();
                break;
            }
        }
    }

    /// Whether this is in the blanks before the next sentence start, and
    /// that start.
    fn in_blanks(&self) -> (bool, Walk<'a>) {
        let mut next = self.copy();
        next.sentence_forward(true);
        let mut on = self.copy();
        while on.ch().is_some_and(is_blank) && on.next() {}
        (on.same(&next), next)
    }

    /// Whether it comes before `other` in the text.
    fn precedes(&self, other: &Walk) -> bool {
        (self.row, self.ix, self.past_end) < (other.row, other.ix, other.past_end)
    }

    /// Back `count` sentence starts (`(`, which can get nowhere).
    fn sentences_back(&mut self, count: u64) {
        let mut cycle = Cycle::default();
        let mut step = 0;
        while step < count {
            self.sentence_backward(true);
            step += 1;
            step += cycle.skip(self.state(false), count - step);
        }
    }

    /// Whether it's on a blank.
    fn blank(&self) -> bool {
        self.ch().is_some_and(is_blank)
    }

    /// Where it is, and which item is next, for [`Cycle`].
    fn state(&self, next: bool) -> State {
        (self.row, self.ix, self.past_end, next)
    }

    /// On by `items` items from the end of one, a sentence first if
    /// `sentence_next`: to the next sentence's start and, for a sentence,
    /// back over the blanks before it, then to the position before (for
    /// blanks only at the last item, so that blanks and the sentence after
    /// them, not yet ended, go on together).
    fn forward_items(&mut self, items: u64, mut sentence_next: bool) {
        let mut cycle = Cycle::default();
        let mut item = 0;
        while item < items {
            item += 1;
            self.sentence_forward(true);
            if sentence_next {
                self.back_to_blanks();
            }
            if item == items || sentence_next {
                self.prev();
            }
            sentence_next = !sentence_next;
            if item < items {
                item += cycle.skip(self.state(sentence_next), items - item);
            }
        }
    }

    fn enter(&mut self, row: u32) {
        self.row = row;
        self.line = Line::new(self.text, row);
    }

    /// To the next position; false at the text's end, past it.
    fn next(&mut self) -> bool {
        if self.past_end {
            if self.row + 1 == self.text.row_count() {
                return false;
            }
            self.past_end = false;
            self.enter(self.row + 1);
            self.ix = 0;
        } else if self.ix + 1 < self.line.len() {
            self.ix += 1;
        } else if self.row + 1 < self.text.row_count() {
            self.enter(self.row + 1);
            self.ix = 0;
        } else {
            // An empty last row is the end itself.
            self.past_end = !self.empty();
            return false;
        }
        true
    }

    /// To the previous position (from past the end, the last); false at
    /// the text's start.
    fn prev(&mut self) -> bool {
        if self.past_end {
            self.past_end = false;
        } else if self.ix > 0 {
            self.ix -= 1;
        } else if self.row > 0 {
            self.enter(self.row - 1);
            self.ix = self.line.last();
        } else {
            return false;
        }
        true
    }

    /// The position before this one, as `(empty row, char)`.
    fn before(&self) -> Option<(bool, Option<char>)> {
        if self.ix > 0 {
            let ch = self.line.grapheme(self.ix - 1).chars().next();
            return Some((false, ch));
        }
        if self.row == 0 {
            return None;
        }
        let line = Line::new(self.text, self.row - 1);
        match line.len() {
            0 => Some((true, None)),
            len => Some((false, line.grapheme(len - 1).chars().next())),
        }
    }

    fn at_end(&self) -> bool {
        self.ix + 1 >= self.line.len() && self.row + 1 == self.text.row_count()
    }

    fn at_start(&self) -> bool {
        self.ix == 0 && self.row == 0
    }

    /// On over blanks (across row ends), stopping at an empty row.
    fn skip_blanks(&mut self) {
        while self.ch().is_some_and(is_blank) && self.next() {}
    }

    /// If a sentence ends here (at its `.`, `!` or `?`), to just past its
    /// closers: the blank after them, the next row's first position, or at
    /// the text's end, the last closer.
    fn sentence_end(&mut self) -> bool {
        if !self.ch().is_some_and(ends_sentence) {
            return false;
        }
        let ix = self.ix;
        loop {
            let row_ends = self.ix + 1 >= self.line.len();
            if !self.next() || row_ends {
                return true;
            }
            match self.ch() {
                Some(ch) if CLOSERS.contains(ch) => {}
                Some(ch) if is_blank(ch) => return true,
                // Still on its row.
                _ => {
                    self.ix = ix;
                    return false;
                }
            }
        }
    }

    /// Back from blanks and sentence-ending punctuation to where the
    /// sentence before them ends, so it's found again: over blanks, closers
    /// that follow punctuation, and at most one `.`, `!` or `?`. Going
    /// forward, it stops short of an empty row, and only backs over a `.`,
    /// `!` or `?` that a blank, a closer or the row's end follows.
    fn back_to_end(&mut self, forward: bool) {
        let punctuation = |ch: char| ends_sentence(ch) || CLOSERS.contains(ch);
        let mut ended = false;
        while let Some(ch) = self.ch().filter(|&ch| {
            is_blank(ch)
                || CLOSERS.contains(ch)
                || (ends_sentence(ch) && (!forward || self.closes()))
        }) {
            let Some((empty, before)) = self.before() else {
                break;
            };
            if (forward && empty) || ended {
                break;
            }
            ended = ends_sentence(ch);
            if CLOSERS.contains(ch) && !before.is_some_and(punctuation) {
                break;
            }
            self.prev();
        }
    }

    /// Whether a blank, a closer or the row's end follows this position.
    fn closes(&self) -> bool {
        match self.ix + 1 < self.line.len() {
            true => {
                let next = self.line.grapheme(self.ix + 1).chars().next();
                next.is_some_and(|ch| is_blank(ch) || CLOSERS.contains(ch))
            }
            false => true,
        }
    }

    /// One `)`: false if it's not the `last` step and there's no text left.
    /// Finding the sentence start it began on, it looks again from the
    /// position after it.
    fn sentence_forward(&mut self, last: bool) -> bool {
        let text_end = self.row + 1 == self.text.row_count();
        if !last && ((self.past_end && text_end) || (self.empty() && self.at_end())) {
            return false;
        }
        if self.past_end && !text_end {
            // From a row's end: the next row's first non-blank, past any
            // empty rows.
            while self.next() && self.empty() {}
            self.skip_blanks();
            return true;
        }
        let from = (self.row, self.ix);
        if !self.next_sentence(last) {
            return false;
        }
        if (self.row, self.ix) != from || self.past_end {
            return true;
        }
        match self.next() {
            true => self.next_sentence(last),
            false => last,
        }
    }

    /// The sentence start `)` finds from here.
    fn next_sentence(&mut self, last: bool) -> bool {
        if self.empty() {
            while self.empty() {
                if !self.next() {
                    return true;
                }
            }
            self.skip_blanks();
            return true;
        }
        self.back_to_end(true);
        loop {
            if self.empty() || self.sentence_end() {
                break;
            }
            if !self.next() {
                return last;
            }
        }
        self.skip_blanks();
        true
    }

    /// One `(`: false if it's not the `last` step and there's no text left.
    fn sentence_backward(&mut self, last: bool) -> bool {
        if self.empty() {
            if !last && self.at_start() {
                return false;
            }
            while self.empty() {
                if !self.prev() {
                    return true;
                }
            }
        } else {
            self.prev();
        }
        self.back_to_end(false);
        let from_row = self.row;
        loop {
            if self.empty() {
                if self.row != from_row {
                    self.enter(self.row + 1);
                    self.ix = 0;
                }
                break;
            }
            if self.sentence_end() {
                break;
            }
            if !self.prev() {
                return last;
            }
        }
        self.skip_blanks();
        true
    }
}
