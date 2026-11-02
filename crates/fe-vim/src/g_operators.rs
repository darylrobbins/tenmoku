//! The `g` operators (a feature module; see [`crate::feature`]): `gu gU g~
//! g?` ([`Operator::Lowercase`], [`Operator::Uppercase`],
//! [`Operator::ToggleCase`], [`Operator::Rot13`]) change the case of what a
//! motion, text object or visual selection covers, or rot13 it, and `gq gw`
//! ([`Operator::Format`], [`Operator::FormatKeep`]) format its rows.
//!
//! `g` then `u U ~ ? q w` starts the operator, or in a visual mode applies
//! it to the selection. Doubled (`gugu` or `guu`, `gUgU` or `gUU`, `g~g~`
//! or `g~~`, `g?g?` or `g??`, `gqgq` or `gqq`, `gwgw`) it takes `count`
//! rows. `gww` is `gw` over `w`, as nvim reads it: the caret's row, as
//! `gwgw`, but a count before it counts words.
//!
//! The case operators change each grapheme's first character by nvim's
//! tables, as visual `u U ~` do, write no register, and leave the caret at
//! the start of what they cover: over rows, where the motion started or
//! ended, whichever comes first (`gUk` keeps the column; `guu` goes to the
//! first non-blank if the caret is past it), at the same byte column even
//! when a character before it grew (`ß` to `ẞ`). As in nvim, an exclusive
//! motion that stays at column 0 (`gUh`, `gU0`) takes the grapheme there on
//! the first row and the whole row on any other, and a text object that
//! isn't there sets the goal column afresh.
//!
//! `gq` and `gw` act on whole rows (`:help gq`): those the motion's ends
//! are on, the last one being the row before when an exclusive motion ended
//! at its column 0 (`:help exclusive`; for `w`, when the last word it moved
//! over was an empty row). Rows that are empty or hold only blanks part
//! paragraphs and stay as they are. Each paragraph's rows are joined as `J`
//! joins them (one space between, none after a blank or before a `)`), then
//! broken at blanks so no row passes column 79 (nvim's with 'textwidth' 0 in
//! an 80-column window): the blanks where a row breaks go, and a word longer
//! than that keeps a row to itself. Every row takes the first row's indent.
//! As nvim does, the rows are joined and broken first, new rows taking the
//! indent as it is, and marks, `'<` and `'>` go with their text; then each
//! row's indent becomes spaces, which marks keep their byte columns through.
//! `gw`'s caret goes with its text throughout. `gq` leaves the caret on the
//! first non-blank of the last row it formatted, or of the row after when an
//! exclusive motion ended there.
//!
//! Each is one undo step, even when it changes nothing, as in nvim: `u`
//! puts the caret back where the case operators left it, or where it was
//! before `gq` and `gw`. `.` repeats them.
//!
//! `'[` and `']` go to what the case operators cover, whether or not they
//! change it: the first character to the last of a charwise span (an
//! exclusive motion's end isn't covered), or over rows the first row's
//! column 0 to the last row's last byte; visual `u U ~ g?` use the
//! selection's ends, or its rows. A case operator that changes nothing
//! leaves `'.` where it was. Known limits: blockwise `u U ~ g?` (in
//! `visual/block.rs`) don't set them so, nvim putting `'[` at the block's
//! top left and `']` at its bottom right; `gq` and `gw` set them as any
//! change does; and `'.` after a case operator over rows that does change
//! text is at the first changed character, where nvim has the first row's
//! column 0.
//!
//! Known differences from nvim, at the edges: its undo puts back the marks
//! a change moved, which fe-vim's doesn't (`J` alike); with a tab in the
//! first row's indent, nvim sometimes leaves marks on that row where they
//! were instead of with their text; and a `.` after a case operator over
//! rows that wrap and hold wide characters can take a row more in nvim.

use std::ops::{Range, RangeInclusive};

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use fe_editor::{EditorModel, commands::origin};
use fe_text::{Bias, Text};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    Change, JoinArgs, Motion, MoveArgs, OperateArgs, Operator, Pending, RangeClass, Step, Target,
    VimCommands, VimKey, VimState, VimTarget, cmdline,
    commands::follow,
    edit,
    feature::Feature,
    history, insert, marks,
    motion::{self, Cursor},
    operator::{self, Span},
    reducer::invocation,
    text_object, visual,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    g,
    register,
    ..Feature::NONE
};

/// The widest a formatted row gets: nvim's 'textwidth' 0 in an 80-column
/// window.
const WIDTH: usize = 79;

/// The operator `ch` names after `g`.
fn named(ch: char) -> Option<Operator> {
    Some(match ch {
        'u' => Operator::Lowercase,
        'U' => Operator::Uppercase,
        '~' => Operator::ToggleCase,
        '?' => Operator::Rot13,
        'q' => Operator::Format,
        'w' => Operator::FormatKeep,
        _ => return None,
    })
}

/// `u U ~ ? q w` after `g`: starts the operator, or with the same one
/// pending takes `count` rows (`gugu`); in a visual mode it acts on the
/// selection.
fn g(pending: &Pending, key: VimKey) -> Option<Step> {
    let VimKey::Char(ch) = key else {
        return None;
    };
    // `gJ`: joins rows as they are; an operator pending cancels it.
    if ch == 'J' {
        return match pending.operator() {
            _ if pending.is_visual() => Some(
                pending
                    .clone()
                    .operate(Operator::JoinRaw, Target::Selection),
            ),
            None => Some(invocation(
                "vim.join_raw",
                JoinArgs {
                    count: pending.count(),
                    raw: true,
                },
            )),
            Some(_) => Some(Step::Cancel),
        };
    }
    let operator = named(ch)?;
    let pending = pending.clone();
    match pending.operator() {
        _ if pending.is_visual() => Some(pending.operate(operator, Target::Selection)),
        None => Some(pending.with_operator(operator)),
        Some(pending_operator) if pending_operator == operator => {
            Some(pending.operate(operator, Target::Motion(Motion::Line)))
        }
        Some(_) => None,
    }
}

/// `vim.join_raw`.
fn register(registry: &mut Registry<VimCommands>) {
    let context = ContextPredicate::parse("editor && vim.mode == normal")
        .expect("vim's context predicate parses");
    registry
        .register_with_args(
            "vim.join_raw",
            "Vim Join Lines Without Spaces",
            |target: &mut VimTarget<'_>, actor: &Actor, args: JoinArgs| {
                follow(target);
                insert::adopt_closing(target.state);
                edit::join_raw(target, actor, args)
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Joins `count` rows (at least two) from each caret's as `gJ` does: keeping their indentation, putting in no space and taking none out.",
        )
        .context(context)
        .mutates_buffer(true);
}

/// The doubled short forms `guu gUU g~~ g?? gqq`: `u U ~ ? q` with its
/// own operator pending. (`w` never gets here: `gww` is `gw` over `w`.)
fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    let VimKey::Char(ch) = key else {
        return None;
    };
    let operator = named(ch)?;
    (!pending.is_visual() && pending.operator() == Some(operator)).then(|| {
        pending
            .clone()
            .operate(operator, Target::Motion(Motion::Line))
    })
}

/// What one caret's target covers.
struct Covered {
    /// Where the caret was.
    head: usize,
    span: Span,
    /// The rows it covers, for `gq` and `gw`.
    rows: Option<RangeInclusive<u32>>,
    /// Where a case operator over rows leaves the caret.
    rows_caret: usize,
    /// The target was an exclusive motion that ended at column 0 of the row
    /// after its rows, where `gq` leaves the caret.
    ended_after: bool,
}

/// `vim.operate` with an operator other than `d y c > <` over a motion or
/// object.
pub(crate) fn operate(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    // `J r i a` act only on a visual selection.
    if !matches!(
        args.operator,
        Operator::Lowercase
            | Operator::Uppercase
            | Operator::ToggleCase
            | Operator::Rot13
            | Operator::Format
            | Operator::FormatKeep
    ) {
        return Err(CommandError::new(format!(
            "{:?} acts only on a visual selection",
            args.operator
        )));
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
    let VimTarget { model, state, .. } = target;
    operator::sync(model);
    let text = model.text().clone();
    let covered = cover(model, state, &text, measured, args);
    // Where a failed `gj` or `gk` moved a caret, it keeps the motion's goal.
    let tab = model.tab_size().max(1);
    let kept: Vec<Option<usize>> = covered
        .iter()
        .map(|covered| match (&covered.span, measured) {
            (Span::Move(_), Target::Motion(motion @ (Motion::DisplayDown | Motion::DisplayUp))) => {
                operator::kept_goal(&text, covered.head, motion, args.count, tab)
            }
            _ => None,
        })
        .collect();
    let resolved = covered
        .iter()
        .any(|covered| matches!(covered.span, Span::Chars(_) | Span::Lines { .. }));
    state.failed |= !resolved;
    if !resolved {
        // A failed `b` still moves the caret, as for any operator.
        let before = model.selections();
        let carets: Vec<_> = covered
            .iter()
            .zip(&before)
            .map(|(covered, selection)| match covered.span {
                Span::Move(offset) => operator::caret(Cursor::at(&text, offset).offset()),
                _ => selection.clone(),
            })
            .collect();
        let landed: Vec<usize> = carets.iter().map(|caret| caret.head()).collect();
        model.set_selections(carets);
        let moved = model.selections() != before;
        // A text object that isn't there sets the goal column afresh too,
        // as in nvim.
        // So does a paragraph or section motion that ran out without moving.
        let ran_out = matches!(
            measured,
            Target::Motion(
                Motion::ParagraphForward | Motion::ParagraphBackward | Motion::Section { .. }
            )
        ) && covered
            .iter()
            .any(|covered| matches!(covered.span, Span::Failed));
        if moved || ran_out || matches!(measured, Target::TextObject(_)) {
            state.goals = None;
        }
        operator::keep_goals(model, state, &landed, &kept);
        return Ok(Dispatched::new(match moved {
            true => Effect::Changed,
            false => Effect::Nothing,
        }));
    }
    if let Some(from) = jump {
        state.marks.jumped(from);
    }
    state.set_last_change(Change::Operate(args).into());
    if let Operator::Format | Operator::FormatKeep = args.operator {
        let targets = covered
            .iter()
            .map(|covered| match (&covered.rows, &covered.span) {
                (Some(rows), _) => Rows::Over(rows.clone(), covered.ended_after),
                // A failed `b` still moves the caret.
                (None, Span::Move(offset)) => Rows::Failed(Cursor::at(&text, *offset).offset()),
                (None, _) => Rows::Failed(covered.head),
            })
            .collect();
        let heads = covered.iter().map(|covered| covered.head).collect();
        return Ok(format(
            model,
            state,
            actor,
            args.operator,
            heads,
            targets,
            &kept,
        ));
    }
    let mut ranges = Vec::new();
    let carets: Vec<usize> = covered
        .iter()
        .map(|covered| match &covered.span {
            Span::Chars(range) => {
                ranges.push(range.clone());
                Cursor::at(&text, range.start).offset()
            }
            Span::Lines { rows, .. } => {
                ranges.push(text.row_start(*rows.start())..operator::row_end(&text, *rows.end()));
                covered.rows_caret
            }
            Span::Move(offset) => Cursor::at(&text, *offset).offset(),
            Span::Failed => covered.head,
        })
        .collect();
    let edits = operator::merge(ranges)
        .into_iter()
        .flat_map(|range| recase(&text, range, args.operator))
        .collect();
    let (transaction, after, changed) = apply_noted(state, model, edits, actor);
    let spans = covered.iter().filter_map(|covered| match &covered.span {
        Span::Chars(range) => {
            let last = range.end.max(range.start + 1) - 1;
            let last = text.clip_offset(last, Bias::Left);
            Some((
                history::pair(&text, range.start),
                history::pair(&text, last.max(range.start)),
            ))
        }
        Span::Lines { rows, .. } => {
            let last = *rows.end();
            Some((
                (*rows.start(), 0),
                (last, (text.row_len(last) as usize).saturating_sub(1)),
            ))
        }
        Span::Move(_) | Span::Failed => None,
    });
    let spans: Vec<_> = spans.collect();
    if let (Some(first), Some(last)) = (spans.first(), spans.last()) {
        let version = changed.then(|| after.version());
        marks::recased(state, model, version, first.0, last.1);
    }
    let begins: Vec<_> = carets.iter().map(|&at| history::pair(&text, at)).collect();
    // As in nvim, the caret keeps its byte column, which a longer character
    // before it (`ß` to `ẞ`) moves it off.
    let changed = after.text();
    let landings = begins.iter().map(|&(row, column)| {
        let at = changed.row_start(row) + column.min(changed.row_len(row) as usize);
        changed.clip_offset(at, Bias::Left)
    });
    let landings: Vec<usize> = landings.collect();
    if let Some(id) = transaction {
        state.carets.record(id, begins);
    }
    let landed = operator::landed_at(model, &after, landings);
    state.goals = None;
    operator::keep_goals(model, state, &landed, &kept);
    Ok(Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect()))
}

/// `gq` or `gw` over a visual selection: its rows, leaving the visual mode.
pub(crate) fn format_selection(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    target.model.sync_display();
    let Some(area) = visual::live_area(target.model, target.state) else {
        return Err(CommandError::new("no visual selection to format"));
    };
    let (Some(first), Some(last)) = (area.rows.first(), area.rows.last()) else {
        return Err(CommandError::new("no visual selection to format"));
    };
    let rows = first.0..=last.0;
    // The vim cursor, which `gw` keeps; `.` lays the selection from where
    // it was typed, its anchor, and keeps that.
    let head = match target.state.replaying {
        true => target.state.visual_anchor(target.model),
        false => target.state.visual_cursor(target.model),
    };
    let head = head.unwrap_or_else(|| target.model.primary_selection().head());
    visual::exit(target)?;
    let VimTarget { model, state, .. } = target;
    let dispatched = format(
        model,
        state,
        actor,
        args.operator,
        vec![head],
        vec![Rows::Over(rows, false)],
        &[],
    );
    state.set_last_change(Change::Operate(args).into());
    if !state.replaying {
        state.visual_extent = Some(area.extent);
    }
    Ok(dispatched)
}

/// `ch` as `g?` leaves it: the 26 ASCII letters each way round.
pub(crate) fn rot13(ch: char) -> char {
    match ch {
        'a'..='z' => (b'a' + (ch as u8 - b'a' + 13) % 26) as char,
        'A'..='Z' => (b'A' + (ch as u8 - b'A' + 13) % 26) as char,
        _ => ch,
    }
}

/// What each caret's target covers, as an operator measures it.
fn cover(
    model: &EditorModel,
    state: &mut VimState,
    text: &Text,
    measured: Target,
    args: OperateArgs,
) -> Vec<Covered> {
    let tab = model.tab_size().max(1);
    let before = model.selections();
    let held = motion::valid_goals(state, model.snapshot().version(), &before)
        .map(|(_, goals)| goals.clone());
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
    let row = |offset: usize| text.offset_to_point(offset).row;
    before
        .iter()
        .enumerate()
        .map(|(ix, selection)| {
            // Another origin's edit may have left the caret inside a
            // grapheme: it's on that grapheme.
            let head = Cursor::at(text, selection.head()).offset();
            // The goal column vim keeps, or the caret's own.
            let goal = held.as_ref().and_then(|goals| goals[ix].1).or_else(|| {
                let cursor = Cursor::at(text, head);
                Some(cursor.line.goal_column(cursor.ix, tab))
            });
            let mut covered = Covered {
                head,
                span: Span::Failed,
                rows: None,
                rows_caret: text.row_start(row(head)),
                ended_after: false,
            };
            match measured {
                Target::Motion(motion) => {
                    covered.span =
                        operator::motion_span(text, head, motion, args.count, last_find, tab);
                    let mut cursor = Cursor::at(text, head);
                    cursor.op = true;
                    let moved = MoveArgs {
                        motion,
                        extend: false,
                        count: args.count,
                    };
                    let reached = motion::motion(&mut cursor, moved, last_find, goal, tab)
                        .map(|_| cursor.offset());
                    let word_row = cursor.word_row;
                    let stays = matches!(&covered.span, Span::Chars(range) if range.is_empty());
                    if stays
                        && motion.class() == RangeClass::Exclusive
                        && text.row_start(row(head)) == head
                    {
                        covered.span = match row(head) {
                            0 => Span::Chars(head..head + operator::grapheme_len(text, head)),
                            at => Span::Lines {
                                rows: at..=at,
                                first_non_blank: false,
                                from: None,
                            },
                        };
                    }
                    let reached = reached.unwrap_or(head);
                    let (lo, hi) = (reached.min(head), reached.max(head));
                    covered.rows_caret = Cursor::at(text, lo).offset();
                    covered.rows = rows_of(text, &covered.span).map(|rows| {
                        // An inclusive motion takes the row it ends on,
                        // even an empty one.
                        match motion.class() {
                            RangeClass::Inclusive => *rows.start()..=row(hi).max(*rows.end()),
                            _ => rows,
                        }
                    });
                    // An exclusive motion that ended at column 0, which
                    // `:help exclusive` moved back to the row before. `w`
                    // ends after the last word it moved over instead (`:help
                    // word`); after an empty row, `gq` still takes the row
                    // after it as moved back from.
                    covered.ended_after = match motion {
                        motion
                            if matches!(motion.word(), Some((Motion::WordForward, _)))
                                && word_row > row(head)
                                && text.row_len(word_row) == 0 =>
                        {
                            if let Some(rows) = &mut covered.rows {
                                *rows = *rows.start()..=word_row;
                            }
                            true
                        }
                        motion if matches!(motion.word(), Some((Motion::WordForward, _))) => false,
                        _ if motion.class() == RangeClass::Exclusive => {
                            let ended = row(hi) > row(lo) && text.row_start(row(hi)) == hi;
                            if let Some(rows) = &mut covered.rows {
                                *rows = row(lo)..=row(hi) - u32::from(ended);
                            }
                            ended
                        }
                        _ => false,
                    };
                }
                Target::TextObject(object) => {
                    covered.span =
                        text_object::select(text, head, object, args.count.unwrap_or(1), tab);
                    covered.rows = rows_of(text, &covered.span);
                    // Rows from an object: `i{`'s, ending before a row that
                    // starts with its `}`.
                    if let Span::Lines {
                        rows,
                        first_non_blank,
                        ..
                    } = &covered.span
                    {
                        covered.rows_caret = text.row_start(*rows.start());
                        covered.ended_after = *first_non_blank;
                    }
                }
                Target::Selection | Target::SelectedLines | Target::SelectedToEnd => {}
            }
            covered
        })
        .collect()
}

/// The rows `span` covers, if any.
fn rows_of(text: &Text, span: &Span) -> Option<RangeInclusive<u32>> {
    let row = |offset: usize| text.offset_to_point(offset).row;
    match span {
        Span::Chars(range) => Some(row(range.start)..=row(range.end.max(range.start + 1) - 1)),
        Span::Lines { rows, .. } => Some(rows.clone()),
        Span::Move(_) | Span::Failed => None,
    }
}

/// The edits that recase `range` (its line breaks aside) for `operator`:
/// each grapheme's first character.
fn recase(text: &Text, range: Range<usize>, operator: Operator) -> Vec<(Range<usize>, String)> {
    let mut edits = Vec::new();
    for (ix, grapheme) in text.slice_to_string(range.clone()).grapheme_indices(true) {
        let mut chars = grapheme.chars();
        let Some(first) = chars.next() else {
            continue;
        };
        let changed = match operator {
            Operator::Lowercase => edit::lowered(first),
            Operator::Uppercase => edit::raised(first),
            Operator::Rot13 => rot13(first),
            Operator::ToggleCase => edit::toggled(first),
            _ => first,
        };
        if changed == first {
            continue;
        }
        // One edit each, so a caret on one stays on it.
        let at = range.start + ix;
        let new = std::iter::once(changed).chain(chars).collect();
        edits.push((at..at + grapheme.len(), new));
    }
    edits
}

/// One caret's part in `gq` or `gw`.
enum Rows {
    /// The rows it formats, and whether its motion ended at column 0 of the
    /// row after them.
    Over(RangeInclusive<u32>, bool),
    /// Its target failed: where the caret goes.
    Failed(usize),
}

/// `gq` (or `gw`, `operator` `FormatKeep`) from the carets at `heads` over
/// each one's [`Rows`].
fn format(
    model: &mut EditorModel,
    state: &mut VimState,
    actor: &Actor,
    operator: Operator,
    heads: Vec<usize>,
    targets: Vec<Rows>,
    kept: &[Option<usize>],
) -> Dispatched<TransactionId> {
    let text = model.text().clone();
    let tab = model.tab_size().max(1);
    // The carets' rows, merged where they overlap (only: rows next to each
    // other format apart).
    let mut spans: Vec<RangeInclusive<u32>> = targets
        .iter()
        .filter_map(|target| match target {
            Rows::Over(rows, _) => Some(rows.clone()),
            Rows::Failed(_) => None,
        })
        .collect();
    spans.sort_by_key(|rows| *rows.start());
    let mut merged: Vec<RangeInclusive<u32>> = Vec::new();
    for rows in spans {
        match merged.last_mut() {
            Some(last) if *rows.start() <= *last.end() => {
                *last = *last.start()..=*rows.end().max(last.end());
            }
            _ => merged.push(rows),
        }
    }
    // Each paragraph's joins and breaks, its first row once they're made
    // and how many rows it then takes, and its rows before with how many
    // rows it and those before it gained.
    let mut reflows = Vec::new();
    let mut formatted: Vec<(u32, u32)> = Vec::new();
    let mut gained: Vec<(RangeInclusive<u32>, i64)> = Vec::new();
    let mut total = 0;
    for rows in &merged {
        for (first, last) in paragraphs(&text, rows.clone()) {
            let (edits, rows) = paragraph(&text, first, last, tab);
            reflows.extend(edits);
            formatted.push(((i64::from(first) + total) as u32, rows));
            total += i64::from(rows) - i64::from(last - first + 1);
            gained.push((first..=last, total));
        }
    }
    // Where `row` goes: a paragraph's row to its last once formatted (where
    // another caret's rows overlapping it took it), any other row by what
    // the paragraphs before it gained.
    let row_after = |row: u32| {
        let at = gained.partition_point(|(rows, _)| *rows.end() < row);
        match gained.get(at) {
            Some((rows, gained)) if rows.contains(&row) => (i64::from(*rows.end()) + gained) as u32,
            _ => {
                let gained = at.checked_sub(1).map_or(0, |at| gained[at].1);
                (i64::from(row) + gained) as u32
            }
        }
    };
    let snapshot = model.snapshot().clone();
    for rows in &merged {
        state.marks.reflowing(&snapshot, *rows.start(), *rows.end());
    }
    let visual_marks = merged
        .iter()
        .map(|rows| visual::reflowing(state, &snapshot, *rows.start(), *rows.end()))
        .collect::<Vec<_>>();
    // As nvim does, the rows are joined and broken first, new rows taking
    // the first's indent as it is, and marks go with their text; then each
    // row's indent becomes spaces, which marks keep their columns through.
    // Measuring text after each step may let another origin edit the
    // buffer: what comes after the joins is worked out in the text they
    // leave (`joined`), and anchored there.
    let origin = origin(actor);
    // Its id even inside `.`'s, which the transaction then is.
    let id = model.buffer_mut().start_transaction(origin.clone());
    let reflowed = !reflows.is_empty();
    let joined = match reflowed {
        true => operator::apply(model, reflows, origin.clone()).1,
        false => snapshot.clone(),
    };
    state.marks.follow(&joined);
    for marks in visual_marks {
        visual::reflowed(state, &joined, marks);
    }
    // The indents that aren't spaces, anchored, with what they were.
    let indents: Vec<_> = formatted
        .iter()
        .flat_map(|&(first, rows)| first..first + rows)
        .filter(|&row| spaced_indent(joined.text(), row, tab).is_some())
        .map(|row| {
            let start = joined.text().row_start(row);
            let indent = indent(joined.text(), row);
            let end = joined.anchor_before(start + indent.len());
            (joined.anchor_after(start), end, indent)
        })
        .collect();
    // Where each caret goes: `gq`'s to its row's first non-blank, which
    // stays that through the indent's change; `gw`'s with its text.
    let landings: Vec<_> = targets
        .iter()
        .zip(&heads)
        .map(|(target, &head)| match (operator, target) {
            (Operator::Format, Rows::Over(rows, ended_after)) => {
                let text = joined.text();
                let row = row_after(*rows.end()) + u32::from(*ended_after);
                let row = row.min(text.row_count() - 1);
                let mut cursor = Cursor::at(text, text.row_start(row));
                cursor.go_to_first_non_blank();
                joined.anchor_after(cursor.offset())
            }
            (_, Rows::Over(..)) => snapshot.anchor_before(head),
            (_, Rows::Failed(at)) => snapshot.anchor_before(*at),
        })
        .collect();
    let mut after = joined.clone();
    // Only those another origin left as they were meanwhile, if it edited:
    // still a whole row's indent.
    let now = model.snapshot().clone();
    let indents: Vec<_> = indents
        .into_iter()
        .filter_map(|(start, end, was)| {
            let text = now.text();
            let (start, end) = (now.resolve(&start), now.resolve(&end));
            let row = text.offset_to_point(start).row;
            let kept = text.row_start(row) == start
                && end == start + was.len()
                && indent(text, row) == was;
            kept.then(|| spaced_indent(text, row, tab)).flatten()
        })
        .collect();
    if !indents.is_empty() {
        after = operator::apply(model, indents, origin).1;
    } else if !reflowed {
        after = apply(model, Vec::new(), actor).1;
    }
    // Never empty: with nothing to change, [`apply`] made its step.
    model.buffer_mut().end_transaction();
    let begins = heads
        .iter()
        .map(|&head| history::pair(&text, head))
        .collect();
    state.carets.record(id, begins);
    let transaction = Some(id);
    let carets: Vec<usize> = landings
        .iter()
        .map(|landing| after.resolve(landing))
        .collect();
    let landed = operator::landed_at(model, &after, carets);
    state.goals = None;
    operator::keep_goals(model, state, &landed, kept);
    Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect())
}

/// Whether `row` holds only blanks, if anything: it parts paragraphs.
fn blank(text: &Text, row: u32) -> bool {
    text.row_to_string(row)
        .bytes()
        .all(|byte| byte == b' ' || byte == b'\t')
}

/// The paragraphs in `rows`: each one's first and last row.
fn paragraphs(text: &Text, rows: RangeInclusive<u32>) -> Vec<(u32, u32)> {
    let mut found: Vec<(u32, u32)> = Vec::new();
    let mut open = false;
    for row in rows {
        match (blank(text, row), open, found.last_mut()) {
            (true, _, _) => open = false,
            (false, true, Some((_, last))) => *last = row,
            (false, _, _) => {
                found.push((row, row));
                open = true;
            }
        }
    }
    found
}

/// The edits that join and break the paragraph on the rows from `first`
/// through `last` (none blank), new rows taking the first's indent, and how
/// many rows it then takes.
fn paragraph(text: &Text, first: u32, last: u32, tab: usize) -> (Vec<(Range<usize>, String)>, u32) {
    let is_blank = |ch: char| ch == ' ' || ch == '\t';
    // Its words, as offsets, with their rows.
    let mut words: Vec<(Range<usize>, u32)> = Vec::new();
    for row in first..=last {
        let start = text.row_start(row);
        let line = text.row_to_string(row);
        let mut word: Option<usize> = None;
        for (ix, ch) in line.char_indices().chain([(line.len(), ' ')]) {
            match (is_blank(ch), word) {
                (true, Some(from)) => {
                    words.push((start + from..start + ix, row));
                    word = None;
                }
                (false, None) => word = Some(ix),
                _ => {}
            }
        }
    }
    let strings: Vec<String> = words
        .iter()
        .map(|(range, _)| text.slice_to_string(range.clone()))
        .collect();
    let word = |ix: usize| strings[ix].as_str();
    // What goes before each word once the rows are joined as `J` joins
    // them: the blanks between on a row; across rows, those the row ended
    // with, or a space unless before a `)`.
    let joined: Vec<String> = (1..words.len())
        .map(|ix| {
            let gap = text.slice_to_string(words[ix - 1].0.end..words[ix].0.start);
            if words[ix - 1].1 == words[ix].1 {
                return gap;
            }
            let ended = gap.split('\n').next().unwrap_or_default().to_string();
            match ended.is_empty() && !word(ix).starts_with(')') {
                true => " ".to_string(),
                false => ended,
            }
        })
        .collect();
    // Each word's width with those joined to it with nothing between, which
    // a row can't break: worked out from the last word back, once.
    let mut unbroken = vec![0; words.len()];
    for ix in (0..words.len()).rev() {
        let joined_on = match joined.get(ix) {
            Some(between) if between.is_empty() => unbroken[ix + 1],
            _ => 0,
        };
        unbroken[ix] = visual::width_from(word(ix), 0, tab) + joined_on;
    }
    let indent = text.slice_to_string(text.row_start(first)..words[0].0.start);
    let width = visual::width_from(&indent, 0, tab);
    let mut edits = Vec::new();
    let mut column = width + visual::width_from(word(0), width, tab);
    let mut rows = 1;
    for ix in 1..words.len() {
        let between = &joined[ix - 1];
        let spaced = visual::width_from(between, column, tab);
        let new = match !between.is_empty() && column + spaced + unbroken[ix] > WIDTH {
            true => {
                rows += 1;
                column = width;
                format!("\n{indent}")
            }
            false => {
                column += spaced;
                between.clone()
            }
        };
        column += visual::width_from(word(ix), column, tab);
        edits.extend(changed(text, words[ix - 1].0.end..words[ix].0.start, &new));
    }
    (edits, rows)
}

/// `row`'s indent: the blanks it starts with.
fn indent(text: &Text, row: u32) -> String {
    let line = text.row_to_string(row);
    let blanks = line.len() - line.trim_start_matches([' ', '\t']).len();
    line[..blanks].to_string()
}

/// The edit that makes `row`'s indent spaces, if it isn't.
fn spaced_indent(text: &Text, row: u32, tab: usize) -> Option<(Range<usize>, String)> {
    let start = text.row_start(row);
    let indent = indent(text, row);
    let spaces = " ".repeat(visual::width_from(&indent, 0, tab));
    changed(text, start..start + indent.len(), &spaces)
}

/// The edit that turns `range` of `text` into `new`, if it changes it: only
/// the part between what they start and end with in common.
fn changed(text: &Text, range: Range<usize>, new: &str) -> Option<(Range<usize>, String)> {
    let old = text.slice_to_string(range.clone());
    let prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = old[prefix..]
        .bytes()
        .rev()
        .zip(new[prefix..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    (old != new).then(|| {
        (
            range.start + prefix..range.end - suffix,
            new[prefix..new.len() - suffix].to_string(),
        )
    })
}

/// Applies `edits` with `actor`'s origin. With none (the text is as it
/// was), it's still an undo step, as nvim's is: a character put in at the
/// text's end and taken out again, which no mark or caret sees. Visual `u U
/// ~ g?` make theirs with it too.
pub(crate) fn apply(
    model: &mut EditorModel,
    edits: Vec<(Range<usize>, String)>,
    actor: &Actor,
) -> (Option<TransactionId>, fe_buffer::BufferSnapshot) {
    if !edits.is_empty() {
        return operator::apply(model, edits, origin(actor));
    }
    let end = model.text().len();
    let steps = [
        vec![(end..end, " ".to_string())],
        vec![(end..end + 1, String::new())],
    ];
    // Its id even inside `.`'s transaction, which it then joins.
    let id = model.buffer_mut().start_transaction(origin(actor));
    let (_, snapshot) = operator::apply_steps(model, steps, origin(actor));
    model.buffer_mut().end_transaction();
    (Some(id), snapshot)
}

/// [`apply`] for the case operators, which also notes the batches of the
/// undo step it makes when nothing changes ([`marks::Note::Unchanged`]) and
/// says whether it changed anything.
pub(crate) fn apply_noted(
    state: &mut VimState,
    model: &mut EditorModel,
    edits: Vec<(Range<usize>, String)>,
    actor: &Actor,
) -> (Option<TransactionId>, fe_buffer::BufferSnapshot, bool) {
    if !edits.is_empty() {
        let (id, snapshot) = operator::apply(model, edits, origin(actor));
        return (id, snapshot, true);
    }
    state.marks.watch(model);
    let end = model.text().len();
    let id = {
        let mut buffer = model.buffer_mut();
        let id = buffer.start_transaction(origin(actor));
        for edits in [
            vec![(end..end, " ".to_string())],
            vec![(end..end + 1, String::new())],
        ] {
            buffer.edit(edits, origin(actor));
            state
                .marks
                .note(buffer.snapshot().version(), marks::Note::Unchanged);
        }
        buffer.end_transaction();
        id
    };
    let snapshot = model.buffer_mut().snapshot();
    (Some(id), snapshot, false)
}
