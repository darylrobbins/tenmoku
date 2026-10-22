//! Vim's motions over an [`EditorModel`]'s text, for every caret.
//!
//! A normal-mode caret sits at the start of a grapheme, never past a
//! non-empty line's last one. Counts step graphemes; goal columns are
//! display columns with tabs expanded to the model's tab size. Every motion
//! is bounded by the text, not the count: rows are computed directly, and
//! repeated motions stop at the first step that doesn't move.

use std::sync::OnceLock;

use fe_buffer::{ResolvedSelection, TransactionId, Version};
use fe_command::{Dispatched, Effect};
use fe_editor::EditorModel;
use fe_text::{Bias, Text};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    Mode, Motion, MoveArgs, VimState, VimTarget, cmdline, folds, jump, line_motions, marks, prose,
    search, state::Goals, unmatched, visual,
};

/// `vim.move`: [`apply`], once a search motion passes
/// [`cmdline::check_search`] and a mark motion finds its mark (if either
/// doesn't, nothing moves and the error is the [`VimState::message`]). A
/// jump that gets anywhere sets the jump mark to where it left from. `zj`
/// and `zk` move between the candidates [`folds::with_candidates`] finds.
pub(crate) fn move_command(
    target: &mut VimTarget<'_>,
    args: MoveArgs,
) -> Dispatched<TransactionId> {
    folds::with_candidates(target, Some(args.motion), |target| {
        move_carets(target, args)
    })
}

fn move_carets(target: &mut VimTarget<'_>, args: MoveArgs) -> Dispatched<TransactionId> {
    let mut args = args;
    crate::insert::began_line(target.state, args.motion);
    let keep = match cmdline::resolve(target, args.motion) {
        Ok((motion, keep)) => {
            args.motion = motion;
            keep
        }
        Err(message) => return target.state.fail(message),
    };
    let searching = matches!(args.motion, Motion::Search(_) | Motion::SearchWord(_));
    if let Err(message) = cmdline::check_motion_search(target, args.motion, keep) {
        return target.state.fail(message);
    }
    // A position, as a list's row goes to, resolves to a mark's motion but
    // isn't a jump.
    let positioned = matches!(args.motion, Motion::Position { .. });
    match marks::resolve(target, args.motion) {
        Ok(motion) => args.motion = motion,
        Err(message) => return target.state.fail(message),
    }
    if !positioned
        && let Some(from) = marks::jump(target, args.motion, args.count)
        && jump::lands(target, args.motion, args.count)
    {
        target.state.marks.jumped(from);
    }
    // A search that stays put still set the last search and the highlight.
    Dispatched::new(match (apply(target.model, target.state, args), searching) {
        (true, _) => Effect::Changed,
        (false, true) => Effect::Notify,
        (false, false) => Effect::Nothing,
    })
}

/// Moves every caret by `args` (in a visual mode, the vim cursor); returns
/// whether any selection changed.
pub(crate) fn apply(model: &mut EditorModel, state: &mut VimState, args: MoveArgs) -> bool {
    if let Mode::Visual(_) = state.mode {
        return visual::move_cursor(model, state, args);
    }
    // Caught up first, so the positions read are the ones moved from.
    model.sync_display();
    let text = model.text().clone();
    let version = model.snapshot().version();
    let before = model.selections();
    // Goals hold only while the carets are where vim left them.
    let goals = valid_goals(state, version, &before);
    let tab = model.tab_size().max(1);
    let mut moved = Vec::with_capacity(before.len());
    let mut failed = false;
    for (ix, selection) in before.iter().enumerate() {
        let mut cursor = Cursor::at(&text, selection.head());
        let goal = goals.and_then(|(_, goals)| goals[ix].1);
        let from = cursor.offset();
        let goal = match motion(&mut cursor, args, state.last_find, goal, tab) {
            Ok(goal) => {
                failed |= word_fails(&text, from, args, tab);
                goal
            }
            Err(kept) => {
                failed |= errs(&text, from, args, state.last_find, tab);
                kept
            }
        };
        let head = cursor.offset();
        let tail = if args.extend { selection.tail() } else { head };
        moved.push((selection_from(tail, head), goal));
    }
    if let Motion::FindForward(_)
    | Motion::FindBackward(_)
    | Motion::TillForward(_)
    | Motion::TillBackward(_) = args.motion
    {
        state.last_find = Some(args.motion);
    }
    model.set_selections(moved.iter().map(|(selection, _)| selection.clone()));
    let after = model.selections();
    // Carets that met were merged: each keeps the goal of the first to land.
    let goals = after.iter().map(|selection| {
        let first = moved.iter().find(|(m, _)| m.head() == selection.head());
        (selection.head(), first.and_then(|(_, goal)| *goal))
    });
    state.goals = Some((version, goals.collect()));
    // Only `gj` and `gk` leave a display column: any other motion clears it.
    let alongs = line_motions::alongs();
    state.display_goals = (!alongs.is_empty()).then_some((version, alongs));
    state.failed |= failed;
    after != before
}

/// Whether a word motion (`w b e W B E`) from `from` fails as Vim's error
/// does, stopping a macro, though it goes as far as it can and resets the
/// goal column: `w e W E` when a step of the count can't move (so the last
/// can't), and `b B` only when they can't move at all.
pub(crate) fn word_fails(text: &Text, from: usize, args: MoveArgs, tab: usize) -> bool {
    if !matches!(
        args.motion,
        Motion::WordForward
            | Motion::WordBackward
            | Motion::WordEnd
            | Motion::BigWordForward
            | Motion::BigWordBackward
            | Motion::BigWordEnd
    ) {
        return false;
    }
    let after = |count: u32| {
        let mut cursor = Cursor::at(text, from);
        if count > 0 {
            let args = MoveArgs {
                count: Some(count),
                ..args
            };
            let _ = motion(&mut cursor, args, None, None, tab);
        }
        cursor.offset()
    };
    let count = args.count.unwrap_or(1).max(1);
    match args.motion {
        // From where it is, a visual cursor at a row's end included.
        Motion::WordBackward | Motion::BigWordBackward => after(count) == from,
        _ => after(count) == after(count - 1),
    }
}

/// Whether a motion from `from` that came back `Err` fails as Vim's error
/// does, stopping a macro: all do but `t` and `T` staying put before the
/// adjacent character they find.
pub(crate) fn errs(
    text: &Text,
    from: usize,
    args: MoveArgs,
    last_find: Option<Motion>,
    tab: usize,
) -> bool {
    let find = match args.motion {
        Motion::TillForward(ch) => Motion::FindForward(ch),
        Motion::TillBackward(ch) => Motion::FindBackward(ch),
        _ => return true,
    };
    let args = MoveArgs {
        motion: find,
        ..args
    };
    motion(&mut Cursor::at(text, from), args, last_find, None, tab).is_err()
}

/// The goals `state` holds for `selections`, if they're still where vim left
/// them in this `version` of the text.
pub(crate) fn valid_goals<'a>(
    state: &'a VimState,
    version: Version,
    selections: &[ResolvedSelection],
) -> Option<&'a (Version, Goals)> {
    state.goals.as_ref().filter(|(at, goals)| {
        *at == version
            && goals
                .iter()
                .map(|goal| goal.0)
                .eq(selections.iter().map(|s| s.head()))
    })
}

/// The find `;` (or `,`, if `reverse`) repeats: the last `f F t T`, turned
/// around for `,`.
pub(crate) fn repeated_find(last_find: Option<Motion>, reverse: bool) -> Option<Motion> {
    let find = last_find?;
    Some(match (find, reverse) {
        (find, false) => find,
        (Motion::FindForward(ch), true) => Motion::FindBackward(ch),
        (Motion::FindBackward(ch), true) => Motion::FindForward(ch),
        (Motion::TillForward(ch), true) => Motion::TillBackward(ch),
        (Motion::TillBackward(ch), true) => Motion::TillForward(ch),
        _ => return None,
    })
}

/// Moves one caret. `Ok` holds the goal column it keeps (`None` resets it).
/// `Err` says the motion failed, as Vim beeps (`h` at the line start, `j` on
/// the last row, an `f` with no match, `%` with no bracket), holding the goal
/// to keep: an operator given a failed motion does nothing.
pub(crate) fn motion(
    cursor: &mut Cursor,
    args: MoveArgs,
    last_find: Option<Motion>,
    goal: Option<usize>,
    tab: usize,
) -> Result<Option<usize>, Option<usize>> {
    // Only these read along the row a window at a time; the rest need it
    // whole.
    if !matches!(
        args.motion,
        Motion::WordForward
            | Motion::WordBackward
            | Motion::WordEnd
            | Motion::BigWordForward
            | Motion::BigWordBackward
            | Motion::BigWordEnd
            | Motion::WordEndBackward
            | Motion::BigWordEndBackward
            | Motion::FindForward(_)
            | Motion::FindBackward(_)
            | Motion::TillForward(_)
            | Motion::TillBackward(_)
            | Motion::RepeatFind
            | Motion::RepeatFindReverse
            | Motion::Left
            | Motion::Right
            | Motion::LineStart
            | Motion::LineEnd
    ) && !(args.motion == Motion::MatchPair && args.count.is_none())
    {
        cursor.widen();
    }
    let count = args.count.unwrap_or(1).max(1);
    let n = count as usize;
    let (row, last_row) = (cursor.row, cursor.text.row_count() - 1);
    let line = &cursor.line;
    match args.motion {
        // A motion that fails leaves the goal alone, as in Vim. For an
        // operator, `l` may step past the last grapheme: `x` takes it.
        Motion::Left | Motion::Right => {
            let before = cursor.offset();
            match args.motion {
                _ if cursor.windowed => cursor.slide_along(args.motion == Motion::Right, n),
                Motion::Left => cursor.ix = cursor.ix.saturating_sub(n),
                _ if cursor.op => cursor.ix = cursor.ix.saturating_add(n).min(line.len()),
                _ => cursor.ix = cursor.ix.saturating_add(n).min(line.last()),
            }
            // An operator's `h` at the row's start (or `l` at its end) only
            // has nothing to act on: `>h` still shifts the row.
            if cursor.offset() == before {
                return if cursor.op { Ok(goal) } else { Err(goal) };
            }
        }
        // Vim fails `k` on the first row and `j` on the last, keeping the
        // goal; from elsewhere a count past the end clamps.
        Motion::Up if row == 0 => return Err(goal),
        Motion::Down | Motion::Line if row == last_row && count > 1 => return Err(goal),
        Motion::Down if row == last_row => return Err(goal),
        Motion::Up => return Ok(cursor.vertical(row.saturating_sub(count), goal, tab)),
        Motion::Down => {
            return Ok(cursor.vertical(row.saturating_add(count).min(last_row), goal, tab));
        }
        Motion::DocumentStart | Motion::DocumentEnd => {
            let default = if args.motion == Motion::DocumentEnd {
                last_row + 1
            } else {
                1
            };
            let to_row = args
                .count
                .unwrap_or(default)
                .saturating_sub(1)
                .min(last_row);
            return Ok(cursor.vertical(to_row, goal, tab));
        }
        // `{count}%`: the row `count` percent down, as `:help N%` computes it.
        Motion::MatchPair if args.count.is_some() => {
            if count > 100 {
                return Err(goal);
            }
            let rows = u64::from(last_row) + 1;
            let to_row = (u64::from(count) * rows).div_ceil(100).saturating_sub(1);
            return Ok(cursor.vertical(to_row as u32, goal, tab));
        }
        Motion::MatchPair => match match_pair(cursor.text, cursor.offset()) {
            Some(Match { offset, rows }) => {
                cursor.goto(offset);
                cursor.rows_matched = rows;
            }
            None => return Err(goal),
        },
        Motion::Unmatched { brace, forward } => {
            return unmatched::motion(cursor, brace, forward, count, goal);
        }
        Motion::LineStart => match cursor.windowed {
            true => cursor.goto(cursor.text.row_start(row)),
            false => cursor.ix = 0,
        },
        Motion::FirstNonBlank => cursor.ix = line.first_non_blank(),
        // Vim fails a `{count}$` from the last row (still aiming at line
        // ends), and clamps one from above.
        Motion::LineEnd if count > 1 && row == last_row => return Err(Some(usize::MAX)),
        Motion::LineEnd => {
            let to = row.saturating_add(count - 1).min(last_row);
            if to != row || cursor.windowed {
                let end = cursor.text.row_start(to) + cursor.text.row_len(to) as usize;
                cursor.window(to, end);
            }
            cursor.ix = cursor.line.last();
            return Ok(Some(usize::MAX));
        }
        Motion::FindForward(_)
        | Motion::FindBackward(_)
        | Motion::TillForward(_)
        | Motion::TillBackward(_) => {
            if !cursor.find(args.motion, n, false) {
                return Err(goal);
            }
        }
        // As nvim's 'cpoptions' has no `;`, a repeated `t` or `T` next to its
        // char skips it for the following one.
        Motion::RepeatFind | Motion::RepeatFindReverse => {
            let reverse = args.motion == Motion::RepeatFindReverse;
            match repeated_find(last_find, reverse) {
                Some(find) if cursor.find(find, n, true) => {}
                _ => return Err(goal),
            }
        }
        Motion::WordForward | Motion::WordBackward | Motion::WordEnd => {
            cursor.words(args.motion, count, false)
        }
        Motion::BigWordForward => cursor.words(Motion::WordForward, count, true),
        Motion::BigWordBackward => cursor.words(Motion::WordBackward, count, true),
        Motion::BigWordEnd => cursor.words(Motion::WordEnd, count, true),
        // At a row's end, past its last character, the match is that
        // character, as in nvim. The offset moves on from the match.
        Motion::Search(search) | Motion::SearchWord(search) => {
            let from = match args.motion {
                Motion::SearchWord(_) => {
                    search::word_at(cursor.text, cursor.offset(), search.forward)
                        .map_or(cursor.offset(), |(start, _)| start)
                }
                _ => cursor.offset(),
            };
            let landed = search::parsed(search.pattern, search.ignore_case)
                .is_ok_and(|pattern| cmdline::offset::land(cursor, &pattern, from, search, n));
            if !landed {
                return Err(goal);
            }
        }
        Motion::Jump(motion) => match jump::nth(cursor.text, cursor.offset(), motion, args.count) {
            Some(offset) => cursor.goto(offset),
            None => return Err(goal),
        },
        // Resolved to a `Search` or `MarkAt` before they run.
        Motion::SearchNext
        | Motion::SearchPrevious
        | Motion::SearchWordForward
        | Motion::SearchWordBackward
        | Motion::SearchPartialWordForward
        | Motion::SearchPartialWordBackward
        | Motion::Mark(_)
        | Motion::Position { .. } => return Err(goal),
        // A mark's position is clamped as a caret is.
        Motion::MarkAt { offset, exact } => {
            cursor.goto(offset);
            if !exact {
                cursor.ix = cursor.line.first_non_blank();
            }
        }
        // `dd`'s target. As a move it is Vim's `_`: the first non-blank
        // `count - 1` rows down.
        Motion::Line => {
            cursor.enter(row.saturating_add(count - 1).min(last_row));
            cursor.ix = cursor.line.first_non_blank();
        }
        Motion::ParagraphForward
        | Motion::ParagraphBackward
        | Motion::SentenceForward
        | Motion::SentenceBackward => return prose::motion(cursor, args, goal, tab),
        Motion::WordEndBackward
        | Motion::BigWordEndBackward
        | Motion::NextLineFirstNonBlank
        | Motion::PrevLineFirstNonBlank
        | Motion::Column
        | Motion::WindowTop
        | Motion::WindowMiddle
        | Motion::WindowBottom
        | Motion::DisplayDown
        | Motion::DisplayUp
        | Motion::LastNonBlank
        | Motion::DisplayLineEnd
        | Motion::DisplayMiddle => return line_motions::motion(cursor, args, goal, tab),
        Motion::FoldNext | Motion::FoldPrevious => return folds::motion(cursor, args, goal, tab),
    }
    // nvim fails an operator whose `b` can't move at all, or would step
    // back from an empty first row (from a non-empty one, it just stops at
    // the start).
    if cursor.op && cursor.stuck_at_start {
        return Err(goal);
    }
    Ok(None)
}

/// Vim's word classes: `Of` holds nvim's `charclass()` (1 punctuation, 2
/// word, 3 emoji, others a script's own class). An empty row is a word of
/// its own; for `W B E`, every non-blank is one class.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Blank,
    Of(u32),
    EmptyRow,
}

/// nvim's `charclass()` and `strdisplaywidth()` for every code point, as hex
/// runs `start:class:width`, written by the ignored
/// `record_char_table_with_neovim` test (`tests/support/chars.lua`).
const CHARS: &str = include_str!("chars.txt");

/// The width-1 characters that nvim 0.12.4 shows two columns wide when the
/// next character is U+FE0F (recorded with `strdisplaywidth()`): not a
/// keycap's base, and not once anything comes between.
const VS16_WIDENED: &[std::ops::RangeInclusive<char>] = &[
    '\u{A9}'..='\u{A9}',
    '\u{AD}'..='\u{AE}',
    '\u{61C}'..='\u{61C}',
    '\u{2028}'..='\u{2029}',
    '\u{203C}'..='\u{203C}',
    '\u{2049}'..='\u{2049}',
    '\u{2122}'..='\u{2122}',
    '\u{2139}'..='\u{2139}',
    '\u{2194}'..='\u{2199}',
    '\u{21A9}'..='\u{21AA}',
    '\u{2328}'..='\u{2328}',
    '\u{23CF}'..='\u{23CF}',
    '\u{23ED}'..='\u{23EF}',
    '\u{23F1}'..='\u{23F2}',
    '\u{23F8}'..='\u{23FA}',
    '\u{24C2}'..='\u{24C2}',
    '\u{25AA}'..='\u{25AB}',
    '\u{25B6}'..='\u{25B6}',
    '\u{25C0}'..='\u{25C0}',
    '\u{25FB}'..='\u{25FC}',
    '\u{2600}'..='\u{2604}',
    '\u{260E}'..='\u{260E}',
    '\u{2611}'..='\u{2611}',
    '\u{2618}'..='\u{2618}',
    '\u{261D}'..='\u{261D}',
    '\u{2620}'..='\u{2620}',
    '\u{2622}'..='\u{2623}',
    '\u{2626}'..='\u{2626}',
    '\u{262A}'..='\u{262A}',
    '\u{262E}'..='\u{262F}',
    '\u{2638}'..='\u{263A}',
    '\u{2640}'..='\u{2640}',
    '\u{2642}'..='\u{2642}',
    '\u{265F}'..='\u{2660}',
    '\u{2663}'..='\u{2663}',
    '\u{2665}'..='\u{2666}',
    '\u{2668}'..='\u{2668}',
    '\u{267B}'..='\u{267B}',
    '\u{267E}'..='\u{267E}',
    '\u{2692}'..='\u{2692}',
    '\u{2694}'..='\u{2697}',
    '\u{2699}'..='\u{2699}',
    '\u{269B}'..='\u{269C}',
    '\u{26A0}'..='\u{26A0}',
    '\u{26A7}'..='\u{26A7}',
    '\u{26B0}'..='\u{26B1}',
    '\u{26C8}'..='\u{26C8}',
    '\u{26CF}'..='\u{26CF}',
    '\u{26D1}'..='\u{26D1}',
    '\u{26D3}'..='\u{26D3}',
    '\u{26E9}'..='\u{26E9}',
    '\u{26F0}'..='\u{26F1}',
    '\u{26F4}'..='\u{26F4}',
    '\u{26F7}'..='\u{26F9}',
    '\u{2702}'..='\u{2702}',
    '\u{2708}'..='\u{2709}',
    '\u{270C}'..='\u{270D}',
    '\u{270F}'..='\u{270F}',
    '\u{2712}'..='\u{2712}',
    '\u{2714}'..='\u{2714}',
    '\u{2716}'..='\u{2716}',
    '\u{271D}'..='\u{271D}',
    '\u{2721}'..='\u{2721}',
    '\u{2733}'..='\u{2734}',
    '\u{2744}'..='\u{2744}',
    '\u{2747}'..='\u{2747}',
    '\u{2763}'..='\u{2764}',
    '\u{27A1}'..='\u{27A1}',
    '\u{2934}'..='\u{2935}',
    '\u{2B05}'..='\u{2B07}',
    '\u{13430}'..='\u{1343F}',
    '\u{1BCA0}'..='\u{1BCA3}',
    '\u{1D173}'..='\u{1D17A}',
    '\u{1F170}'..='\u{1F171}',
    '\u{1F17E}'..='\u{1F17F}',
    '\u{E0001}'..='\u{E0001}',
];

/// [`CHARS`]' runs: the first code point of each, its class and width.
fn runs() -> &'static [(u32, u32, usize)] {
    static RUNS: OnceLock<Vec<(u32, u32, usize)>> = OnceLock::new();
    RUNS.get_or_init(|| {
        let hex = |field: &str| u32::from_str_radix(field, 16).expect("chars.txt is hex");
        let run = |run: &str| {
            let mut fields = run.split(':').map(hex);
            let mut next = || fields.next().expect("chars.txt runs are start:class:width");
            (next(), next(), next() as usize)
        };
        CHARS.split_whitespace().map(run).collect()
    })
}

/// `ch`'s class and display width, from [`CHARS`].
fn char_info(ch: char) -> (u32, usize) {
    let runs = runs();
    let ix = runs.partition_point(|run| run.0 <= u32::from(ch));
    let (_, class, width) = runs[ix - 1];
    (class, width)
}

/// The most display columns a grapheme other than a tab takes (a format
/// character shown as `<200b>`, by [`CHARS`]).
pub(crate) fn widest() -> usize {
    let width = runs().iter().filter(|run| run.0 != u32::from('\t'));
    width.map(|run| run.2).max().unwrap_or(1).max(2)
}

/// A grapheme's class is its first character's, as in nvim.
pub(crate) fn class(grapheme: &str, big: bool) -> Class {
    match char_info(grapheme.chars().next().unwrap_or(' ')).0 {
        0 => Class::Blank,
        _ if big => Class::Of(2),
        class => Class::Of(class),
    }
}

/// One row's text and the byte offset of each grapheme in it.
pub(crate) struct Line {
    pub(crate) start: usize,
    pub(crate) text: String,
    pub(crate) starts: Vec<usize>,
    /// For a window of a row ([`Line::around`]): the text, to take the
    /// columns before the window from, and the row's start.
    lead: Option<(Text, usize)>,
    /// The tab width and display columns of that lead, once taken.
    lead_columns: std::cell::Cell<Option<(usize, usize)>>,
}

/// How far each side of a caret a row's text is read at first: the caret's
/// reach, which [`Line::around`] moves out to where the text is sure to
/// break. A row of at most four times this is read whole.
const CARET_WINDOW: usize = 16 * 1024;

/// How many places where the class of an ASCII char changes lie between a
/// window's edge and the reach it must hold: a run, the blanks after it, and
/// one more run, so that `iw` and `aw` on a long run see their ends.
const CLASS_MARGIN: usize = 3;

/// How far past its reach a window looks for blank / non-blank places to end
/// at, so that a WORD isn't cut, before it settles for the end of a word.
const BLANK_REACH: usize = 8 * CARET_WINDOW;

#[cfg(test)]
thread_local! {
    /// The bytes of rows [`Line`]s on this thread have been made of.
    static ROW_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Whether a row of `len` bytes is read in windows, not whole.
fn huge(len: usize) -> bool {
    len > 4 * CARET_WINDOW
}

/// Whether `row` is read in spans for `%` and the bracket objects: it is
/// huge and has no double quote, whose state runs from the row's start.
fn spanned(text: &Text, row: u32) -> bool {
    let len = text.row_len(row) as usize;
    let start = text.row_start(row);
    huge(len)
        && !text
            .chunks_in_range(start..start + len)
            .any(|chunk| chunk.as_bytes().contains(&b'"'))
}

/// Whether every byte is printable ASCII (a column wide), a word at a time.
fn printable(bytes: &[u8]) -> bool {
    const ONES: u64 = 0x0101_0101_0101_0101;
    const HIGHS: u64 = ONES << 7;
    let (words, rest) = bytes.as_chunks::<8>();
    for word in words {
        let word = u64::from_le_bytes(*word);
        // A non-ASCII byte, one below a space, or a DEL.
        let low = word.wrapping_sub(ONES * 0x20) & !word;
        let del = (word ^ (ONES * 0x7f)).wrapping_sub(ONES);
        if (word | low | del) & HIGHS != 0 {
            return false;
        }
    }
    rest.iter().all(|byte| (0x20..0x7f).contains(byte))
}

/// Whether `[prev, next]` is a place where the segmentation of graphemes
/// breaks whatever is around it: between two ASCII bytes. (No Prepend,
/// Extend, ZWJ, SpacingMark or regional indicator is ASCII, so no rule that
/// joins graphemes, or counts what came before, reaches across one. A row
/// holds no line feed for a CR to join.)
fn breaks(prev: u8, next: u8) -> bool {
    prev < 0x80 && next < 0x80
}

/// Whether one of ASCII bytes `prev` and `next` is a blank and the other
/// not: where a WORD begins or ends.
fn blank_change(prev: u8, next: u8) -> bool {
    let blank = |byte: u8| char_info(char::from(byte)).0 == 0;
    blank(prev) != blank(next)
}

/// Whether the class of ASCII bytes `prev` and `next` differ: where a word
/// begins or ends.
fn word_change(prev: u8, next: u8) -> bool {
    char_info(char::from(prev)).0 != char_info(char::from(next)).0
}

/// Whether the ASCII byte before `ix` is a grapheme of its own and `ix`
/// starts another, so that their classes are those of the bytes: not when
/// something before it (a Prepend char) may join it.
fn starts_alone(bytes: &[u8], ix: usize) -> bool {
    ix >= 2 && bytes[ix - 2] < 0x80 && breaks(bytes[ix - 1], bytes[ix])
}

/// The nearest place to `from` in `bytes`, outward (`forward` or back) and
/// at least [`CLASS_MARGIN`] class changes out, where graphemes break.
fn edge(bytes: &[u8], from: usize, forward: bool) -> Option<usize> {
    // Where a WORD ends if there is one in reach, else a word.
    let within = |by: fn(u8, u8) -> bool| edge_by(bytes, from, forward, by);
    let room = match forward {
        true => bytes.len().saturating_sub(from),
        false => from,
    };
    within(blank_change).or_else(|| match room >= BLANK_REACH {
        true => within(word_change).or_else(|| inert_edge(bytes, from, forward)),
        false => None,
    })
}

/// Whether `ch` joins nothing, whatever is next to it: a grapheme of its own
/// beside any other such char (not a Prepend, Extend, ZWJ, SpacingMark,
/// regional indicator, Hangul jamo or syllable).
fn inert(ch: char) -> bool {
    if ch.is_ascii() {
        return true;
    }
    if matches!(ch, '\u{1100}'..='\u{11ff}' | '\u{a960}'..='\u{a97f}' | '\u{ac00}'..='\u{d7ff}') {
        return false;
    }
    let mut buf = [0u8; 4];
    let ch = &*ch.encode_utf8(&mut buf);
    // Joins an `a` after it (Prepend) or before it (Extend, ZWJ,
    // SpacingMark), or its own kind (regional indicators).
    let count = |text: String| text.graphemes(true).count();
    count(format!("{ch}a")) == 2 && count(format!("a{ch}")) == 2 && count(format!("{ch}{ch}")) == 2
}

/// [`edge`] for a window with no class change to end at (a long run of
/// letters, Cyrillic, emoji, tabs or blanks): the nearest place to `from`,
/// outward, between two chars that join nothing.
fn inert_edge(bytes: &[u8], from: usize, forward: bool) -> Option<usize> {
    let text = std::str::from_utf8(bytes).ok()?;
    let from = from.min(text.len());
    let mut at = from;
    while !text.is_char_boundary(at) {
        at = if forward { at + 1 } else { at - 1 };
    }
    let (mut prev, mut next) = (text[..at].chars().next_back(), text[at..].chars().next());
    loop {
        if let (Some(a), Some(b)) = (prev, next)
            && inert(a)
            && inert(b)
        {
            return Some(at);
        }
        if forward {
            let ch = next?;
            at += ch.len_utf8();
            prev = Some(ch);
            next = text[at..].chars().next();
        } else {
            let ch = prev?;
            at -= ch.len_utf8();
            next = Some(ch);
            prev = text[..at].chars().next_back();
        }
    }
}

/// [`edge`] at the [`CLASS_MARGIN`]th place `change` holds.
fn edge_by(bytes: &[u8], from: usize, forward: bool, change: fn(u8, u8) -> bool) -> Option<usize> {
    let mut changes = 0;
    if forward {
        for ix in from.max(1)..bytes.len() {
            if starts_alone(bytes, ix) && change(bytes[ix - 1], bytes[ix]) {
                changes += 1;
                if changes == CLASS_MARGIN {
                    return Some(ix);
                }
            }
        }
    } else {
        for ix in (1..=from.min(bytes.len().saturating_sub(1))).rev() {
            if starts_alone(bytes, ix) && change(bytes[ix - 1], bytes[ix]) {
                changes += 1;
                if changes == CLASS_MARGIN {
                    return Some(ix);
                }
            }
        }
    }
    None
}

impl Line {
    pub(crate) fn new(text: &Text, row: u32) -> Self {
        Self::of(text.row_start(row), text.row_to_string(row))
    }

    /// `row` as far as a caret at `offset` needs it: all of it, or for a
    /// huge row, [`CARET_WINDOW`] bytes each side of `offset` and on to the
    /// nearest edges where its graphemes are sure to be the row's own: between
    /// two ASCII bytes, past a few class changes ([`CLASS_MARGIN`]).
    pub(crate) fn around(text: &Text, row: u32, offset: usize) -> Self {
        let start = text.row_start(row);
        let len = text.row_len(row) as usize;
        if !huge(len) {
            return Self::new(text, row);
        }
        let end = start + len;
        let at = offset.clamp(start, end);
        let (mut back, mut ahead) = (2 * CARET_WINDOW, 2 * CARET_WINDOW);
        loop {
            let lo = text.clip_offset(at.saturating_sub(back).max(start), Bias::Left);
            let hi = text.clip_offset((at + ahead).min(end), Bias::Right);
            let window = text.slice_to_string(lo..hi);
            let bytes = window.as_bytes();
            let caret = at - lo;
            // From the reach inward, the nearest edges there.
            let first = match lo == start {
                true => Some(0),
                false => edge(bytes, caret.saturating_sub(CARET_WINDOW), false),
            };
            let last = match hi == end {
                true => Some(bytes.len()),
                false => edge(bytes, caret + CARET_WINDOW, true),
            };
            let (Some(first), Some(last)) = (first, last) else {
                if first.is_none() {
                    back *= 2;
                }
                if last.is_none() {
                    ahead *= 2;
                }
                continue;
            };
            let mut line = Self::of(lo + first, window[first..last].to_string());
            if lo + first > start {
                line.lead = Some((text.clone(), start));
            }
            return line;
        }
    }

    /// `text` as a row starting at byte `start`.
    pub(crate) fn of(start: usize, text: String) -> Self {
        #[cfg(test)]
        ROW_BYTES.set(ROW_BYTES.get() + text.len());
        let starts = text.grapheme_indices(true).map(|(ix, _)| ix).collect();
        Self {
            start,
            text,
            starts,
            lead: None,
            lead_columns: std::cell::Cell::new(None),
        }
    }

    /// The display columns of the row before this window of it (0 for a
    /// row read whole).
    fn lead_columns(&self, tab: usize) -> usize {
        match (&self.lead, self.lead_columns.get()) {
            (None, _) => 0,
            (_, Some((at, columns))) if at == tab => columns,
            (Some((text, row_start)), _) => {
                // Printable ASCII is a column a byte: no need to cut it into
                // graphemes.
                let mut plain = 0;
                let all_plain = text.chunks_in_range(*row_start..self.start).all(|chunk| {
                    plain += chunk.len();
                    printable(chunk.as_bytes())
                });
                let columns = match all_plain {
                    true => plain,
                    false => {
                        let before =
                            Line::of(*row_start, text.slice_to_string(*row_start..self.start));
                        (0..before.len()).fold(0, |at, ix| at + before.width(ix, at, tab))
                    }
                };
                self.lead_columns.set(Some((tab, columns)));
                columns
            }
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.starts.len()
    }

    /// The last grapheme's index; 0 on an empty row.
    pub(crate) fn last(&self) -> usize {
        self.len().saturating_sub(1)
    }

    pub(crate) fn grapheme(&self, ix: usize) -> &str {
        let end = self.starts.get(ix + 1).copied().unwrap_or(self.text.len());
        &self.text[self.starts[ix]..end]
    }

    /// Whether grapheme `ix`'s base character is `ch`, as `f` matches.
    fn starts_with(&self, ix: usize, ch: char) -> bool {
        self.grapheme(ix).starts_with(ch)
    }

    /// `^`: the first non-blank grapheme, or the last if all are blank.
    /// Only a space or a tab is blank here, unlike for `w`.
    pub(crate) fn first_non_blank(&self) -> usize {
        (0..self.len())
            .find(|&ix| !matches!(self.grapheme(ix), " " | "\t"))
            .unwrap_or(self.last())
    }

    /// The display width of grapheme `ix` starting at display `column`.
    pub(crate) fn width(&self, ix: usize, column: usize, tab: usize) -> usize {
        let grapheme = self.grapheme(ix);
        let ch = grapheme.chars().next().unwrap_or(' ');
        match ch {
            '\t' => tab - column % tab,
            // Emoji presentation (VS16) right after one of these two columns.
            _ if grapheme.chars().nth(1) == Some('\u{fe0f}')
                && VS16_WIDENED.iter().any(|run| run.contains(&ch)) =>
            {
                2
            }
            _ => char_info(ch).1,
        }
    }

    /// The display column grapheme `ix` starts at.
    pub(crate) fn start_column(&self, ix: usize, tab: usize) -> usize {
        (0..ix).fold(self.lead_columns(tab), |column, i| {
            column + self.width(i, column, tab)
        })
    }

    /// Vim's goal column for a normal-mode caret on grapheme `ix`: the
    /// display column it starts at, except for a tab, whose last column it
    /// is (where the caret shows).
    pub(crate) fn goal_column(&self, ix: usize, tab: usize) -> usize {
        if self.len() == 0 {
            return 0;
        }
        let start = self.start_column(ix, tab);
        match self.grapheme(ix) {
            "\t" => start + self.width(ix, start, tab) - 1,
            _ => start,
        }
    }
}

/// A caret: a row and the index of a grapheme in it (0 on an empty row),
/// keeping that row's [`Line`] as it steps.
pub(crate) struct Cursor<'a> {
    pub(crate) text: &'a Text,
    pub(crate) row: u32,
    pub(crate) ix: usize,
    pub(crate) line: Line,
    /// Moving for an operator: `l` may step onto the row's end.
    pub(crate) op: bool,
    /// Moving a visual mode's cursor: a search may land on a row's end.
    pub(crate) visual: bool,
    /// The row of the last word `w` moved over.
    pub(crate) word_row: u32,
    /// Whether `w` ran into the end of the text.
    pub(crate) hit_end: bool,
    /// Whether `b` was stuck at the start of the text: from its first step,
    /// or with steps left on an empty first row.
    pub(crate) stuck_at_start: bool,
    /// Whether `%` matched preprocessor rows, which an operator takes
    /// whole.
    pub(crate) rows_matched: bool,
    /// Whether `line` is only part of the row, a window around the caret.
    windowed: bool,
    /// Whether rows this caret goes to are read in windows too, as the one
    /// it began on is ([`Cursor::widen`] ends that).
    bounded: bool,
}

impl<'a> Cursor<'a> {
    /// The normal-mode caret for `offset`: on the grapheme holding it,
    /// clamped to the row's last. On a huge row `line` holds only a window
    /// around the caret ([`Line::around`]); the motions here that move along
    /// the row slide it, and the rest read the row whole ([`Cursor::widen`]).
    pub(crate) fn at(text: &'a Text, offset: usize) -> Self {
        let row = text.offset_to_point(offset).row;
        let mut cursor = Self {
            text,
            row,
            ix: 0,
            line: Line::around(text, row, offset),
            op: false,
            visual: false,
            word_row: row,
            hit_end: false,
            stuck_at_start: false,
            rows_matched: false,
            windowed: false,
            bounded: true,
        };
        cursor.windowed = cursor.partial();
        cursor.goto(offset);
        cursor
    }

    /// [`Cursor::at`] with the row read whole, for what takes the row's
    /// graphemes by index or its ends as `line`'s.
    pub(crate) fn whole(text: &'a Text, offset: usize) -> Self {
        let mut cursor = Self::at(text, offset);
        cursor.widen();
        cursor
    }

    /// Whether `line` is only part of its row.
    fn partial(&self) -> bool {
        let start = self.text.row_start(self.row);
        let end = start + self.text.row_len(self.row) as usize;
        self.line.start > start || self.line.start + self.line.text.len() < end
    }

    /// Whether a window with more of the row beyond it cuts the runs around
    /// the caret short: fewer than [`CLASS_MARGIN`] changes of class lie
    /// between the caret and an edge, so that `iw` and `aw` can't see their
    /// ends. (Only a window ended without class changes does.)
    pub(crate) fn runs_reach_edge(&self, big: bool) -> bool {
        let reaches = |range: &mut dyn Iterator<Item = usize>| {
            let mut changes = 0;
            let mut prev = self.class_on_row(self.ix, big);
            for ix in range {
                let class = self.class_on_row(ix, big);
                if class != prev {
                    changes += 1;
                    if changes >= CLASS_MARGIN {
                        return false;
                    }
                }
                prev = class;
            }
            true
        };
        self.more_after() && reaches(&mut (self.ix + 1..self.line.len()))
            || self.more_before() && reaches(&mut (0..self.ix).rev())
    }

    /// Whether the row goes on past `line`'s end.
    fn more_after(&self) -> bool {
        self.windowed
            && self.line.start + self.line.text.len()
                < self.text.row_start(self.row) + self.text.row_len(self.row) as usize
    }

    /// Whether the row goes on before `line`'s start.
    fn more_before(&self) -> bool {
        self.windowed && self.line.start > self.text.row_start(self.row)
    }

    /// Reads `row` around `offset` ([`Line::around`]), or whole once the
    /// caret has been widened, leaving `ix` to the caller.
    fn window(&mut self, row: u32, offset: usize) {
        self.row = row;
        self.line = match self.bounded {
            true => Line::around(self.text, row, offset),
            false => Line::new(self.text, row),
        };
        self.windowed = self.partial();
    }

    /// Reads the whole row if only part of it is, keeping the caret (and an
    /// operator's place past the row's last grapheme).
    pub(crate) fn widen(&mut self) {
        self.bounded = false;
        if !self.windowed {
            return;
        }
        let past = self.ix >= self.line.len();
        let offset = self.offset();
        self.windowed = false;
        self.line = Line::new(self.text, self.row);
        self.goto(offset);
        if past && offset == self.line.start + self.line.text.len() {
            self.ix = self.line.len();
        }
    }

    /// To the grapheme holding `offset`, clamped to its row's last.
    pub(crate) fn goto(&mut self, offset: usize) {
        let row = self.text.offset_to_point(offset).row;
        let end = self.line.start + self.line.text.len();
        let inside = offset >= self.line.start && (offset < end || !self.more_after());
        if row != self.row || self.windowed && !inside {
            self.window(row, offset);
        }
        let column = offset.saturating_sub(self.line.start);
        let ix = self.line.starts.partition_point(|&start| start <= column);
        self.ix = ix.saturating_sub(1).min(self.line.last());
    }

    /// The caret's offset; the row's end when an operator's `l` stepped
    /// past its last grapheme.
    pub(crate) fn offset(&self) -> usize {
        let column = self.line.starts.get(self.ix).copied();
        self.line.start + column.unwrap_or(self.line.text.len())
    }

    /// `f F t T` `n` times along the row; false if there aren't `n`
    /// matches. `skip` is `;` and `,`: a till next to its char goes on to
    /// the following one.
    fn find(&mut self, find: Motion, n: usize, skip: bool) -> bool {
        let origin = self.offset();
        let target = match find {
            Motion::FindForward(ch) | Motion::TillForward(ch) => {
                let till = matches!(find, Motion::TillForward(_));
                self.find_forward(ch, n, usize::from(till && skip) + 1)
                    // `t` stops on the grapheme before the match.
                    .map(|(at, before)| if till { before } else { at })
                    // `t` onto the very next grapheme fails too, though an
                    // operator then takes the caret's own; `T` doesn't.
                    .filter(|&at| at != origin || self.op)
            }
            Motion::FindBackward(ch) | Motion::TillBackward(ch) => {
                let till = matches!(find, Motion::TillBackward(_));
                self.find_backward(ch, n, usize::from(till && skip))
                    .map(|at| {
                        // `T` steps one character past the match, which inside a
                        // grapheme with combining marks is the match itself.
                        match till && at.1.chars().nth(1).is_none() {
                            true => at.0 + at.1.len(),
                            false => at.0,
                        }
                    })
            }
            _ => None,
        };
        let Some(target) = target else {
            return false;
        };
        self.goto(target);
        // An operator's `T` may land past the row's last grapheme.
        let (start, len) = (self.text.row_start(self.row), self.text.row_len(self.row));
        if len > 0 && target == start + len as usize {
            self.ix = self.line.len();
        }
        true
    }

    /// The `n`th grapheme starting with `ch`, from `skip` graphemes past the
    /// caret along the row: its offset, and the offset of the grapheme
    /// before it. Windows past this one are read as the search goes on.
    fn find_forward(&self, ch: char, mut n: usize, skip: usize) -> Option<(usize, usize)> {
        let mut owned: Option<Line> = None;
        let mut from = self.ix + skip;
        let mut before = None;
        loop {
            let line = owned.as_ref().unwrap_or(&self.line);
            for ix in from..line.len() {
                if line.starts_with(ix, ch) {
                    n -= 1;
                    if n == 0 {
                        let at = line.start + line.starts[ix];
                        let prev = match ix.checked_sub(1) {
                            Some(prev) => line.start + line.starts[prev],
                            None => before?,
                        };
                        return Some((at, prev));
                    }
                }
            }
            let end = line.start + line.text.len();
            let row_end = self.text.row_start(self.row) + self.text.row_len(self.row) as usize;
            if end >= row_end {
                return None;
            }
            before = line.starts.last().map(|&last| line.start + last);
            from = from.saturating_sub(line.len());
            let next = Line::around(self.text, self.row, end);
            from += next
                .starts
                .partition_point(|&start| next.start + start < end);
            owned = Some(next);
        }
    }

    /// The `n`th grapheme starting with `ch`, from before the one `skip`
    /// graphemes back of the caret, backward along the row: its offset and
    /// text.
    fn find_backward(&self, ch: char, mut n: usize, skip: usize) -> Option<(usize, String)> {
        let mut owned: Option<Line> = None;
        let to = self.ix.saturating_sub(skip);
        let mut limit = self.line.start
            + self
                .line
                .starts
                .get(to)
                .copied()
                .unwrap_or(self.line.text.len());
        loop {
            let line = owned.as_ref().unwrap_or(&self.line);
            let below = line
                .starts
                .partition_point(|&start| line.start + start < limit);
            for ix in (0..below).rev() {
                if line.starts_with(ix, ch) {
                    n -= 1;
                    if n == 0 {
                        return Some((line.start + line.starts[ix], line.grapheme(ix).to_string()));
                    }
                }
            }
            if line.start <= self.text.row_start(self.row) {
                return None;
            }
            limit = line.start;
            let before = self.text.clip_offset(limit - 1, Bias::Left);
            owned = Some(Line::around(self.text, self.row, before));
        }
    }

    pub(crate) fn enter(&mut self, row: u32) {
        if row != self.row {
            self.row = row;
            self.line = Line::new(self.text, row);
            self.windowed = false;
        }
    }

    /// `n` graphemes along a windowed row (`l` or `h`), stopping at its
    /// last or first; an operator's `l` may go on to the row's end.
    fn slide_along(&mut self, forward: bool, n: usize) {
        for _ in 0..n {
            if !self.step(forward) {
                if forward && self.op && self.ix < self.line.len() {
                    self.ix = self.line.len();
                }
                return;
            }
        }
    }

    /// One grapheme along the row, sliding the window on past its edge;
    /// false at the row's start or end.
    fn step(&mut self, forward: bool) -> bool {
        match forward {
            true if self.ix + 1 < self.line.len() => self.ix += 1,
            true if self.more_after() => self.goto(self.line.start + self.line.text.len()),
            false if self.ix > 0 => self.ix -= 1,
            false if self.more_before() => {
                let before = self.text.clip_offset(self.line.start - 1, Bias::Left);
                self.goto(before);
            }
            _ => return false,
        }
        true
    }

    /// To the grapheme on `row` covering the goal column (by default, the
    /// one this caret is on), or the row's last; returns the goal.
    pub(crate) fn vertical(&mut self, row: u32, goal: Option<usize>, tab: usize) -> Option<usize> {
        // The goal column is the whole row's, and the graphemes found in it.
        self.widen();
        let goal = goal.unwrap_or_else(|| self.line.goal_column(self.ix, tab));
        self.enter(row);
        let mut end = 0;
        self.ix = (0..self.line.len())
            .find(|&ix| {
                end += self.line.width(ix, end, tab);
                goal < end
            })
            .unwrap_or(self.line.last());
        Some(goal)
    }

    pub(crate) fn class(&self, big: bool) -> Class {
        match self.line.len() {
            0 => Class::EmptyRow,
            _ => class(self.line.grapheme(self.ix), big),
        }
    }

    /// The class of grapheme `ix` on this row, if it has one.
    pub(crate) fn class_on_row(&self, ix: usize, big: bool) -> Option<Class> {
        (ix < self.line.len()).then(|| class(self.line.grapheme(ix), big))
    }

    /// To the next grapheme, or the next row's first; false at the end.
    pub(crate) fn forward(&mut self) -> bool {
        if self.step(true) {
            return true;
        }
        if self.row + 1 >= self.text.row_count() {
            return false;
        }
        self.window(self.row + 1, self.text.row_start(self.row + 1));
        self.ix = 0;
        true
    }

    /// To the previous grapheme, or the previous row's last; false at the
    /// start.
    pub(crate) fn backward(&mut self) -> bool {
        if self.step(false) {
            return true;
        }
        if self.row == 0 {
            return false;
        }
        let end = self.text.row_start(self.row) - 1;
        self.window(self.row - 1, end);
        self.ix = self.line.last();
        true
    }

    /// `w`, `b` or `e` (`W B E` if `big`) `count` times, stopping once a step
    /// doesn't move.
    pub(crate) fn words(&mut self, motion: Motion, count: u32, big: bool) {
        for step in 0..count {
            let before = (self.row, self.offset());
            self.word_row = self.row;
            match motion {
                Motion::WordForward => self.word_forward(big),
                Motion::WordBackward => self.word_backward(big),
                _ => self.word_end(big),
            }
            if (self.row, self.offset()) == before {
                self.stuck_at_start = motion == Motion::WordBackward
                    && before == (0, 0)
                    && (step == 0 || self.line.len() == 0);
                break;
            }
        }
    }

    /// `w`: past this word, then past blanks to the next word or empty row.
    /// Off the end of the text, it stays on the last grapheme (noting
    /// [`Cursor::hit_end`]).
    fn word_forward(&mut self, big: bool) {
        let (class, row) = (self.class(big), self.row);
        let mut moved = if !matches!(class, Class::Blank | Class::EmptyRow) {
            loop {
                let moved = self.forward();
                if !moved || self.row != row || self.class(big) != class {
                    break moved;
                }
            }
        } else {
            self.forward()
        };
        while moved && self.class(big) == Class::Blank {
            moved = self.forward();
        }
        self.hit_end |= !moved;
    }

    /// `e`: at least one step, past blanks and empty rows, to the end of the
    /// word reached.
    fn word_end(&mut self, big: bool) {
        if !self.forward() {
            return;
        }
        while matches!(self.class(big), Class::Blank | Class::EmptyRow) {
            if !self.forward() {
                return;
            }
        }
        let class = self.class(big);
        while self.step(true) {
            if self.class(big) != class {
                self.step(false);
                break;
            }
        }
    }

    /// `b`: at least one step back, past blanks (stopping on an empty row),
    /// to the start of the word reached.
    fn word_backward(&mut self, big: bool) {
        if !self.backward() {
            return;
        }
        while self.class(big) == Class::Blank && self.backward() {}
        let class = self.class(big);
        while class != Class::EmptyRow && self.step(false) {
            if self.class(big) != class {
                self.step(true);
                break;
            }
        }
    }
}

fn selection_from(tail: usize, head: usize) -> ResolvedSelection {
    ResolvedSelection {
        range: tail.min(head)..tail.max(head),
        reversed: head < tail,
    }
}

/// The bracket pairs `%` and the bracket objects match.
pub(crate) const PAIRS: [(u8, u8); 3] = [(b'(', b')'), (b'[', b']'), (b'{', b'}')];

/// A row's bytes, and for each whether an odd run of backslashes escapes it
/// and whether it's inside a double-quoted string (after an odd number of
/// unescaped `"` on the row, if the row has an even number).
pub(crate) struct Scan {
    pub(crate) start: usize,
    pub(crate) bytes: Vec<u8>,
    pub(crate) escaped: Vec<bool>,
    pub(crate) quoted: Vec<bool>,
}

impl Scan {
    pub(crate) fn new(text: &Text, row: u32) -> Self {
        Self::of(text.row_start(row), text.row_to_string(row).into_bytes())
    }

    /// The bytes at `start` as a row.
    fn of(start: usize, bytes: Vec<u8>) -> Self {
        let (mut escaped, mut quoted) = (Vec::with_capacity(bytes.len()), Vec::new());
        let (mut slashes, mut inside) = (0usize, false);
        for &byte in &bytes {
            let escape = slashes % 2 == 1;
            escaped.push(escape);
            quoted.push(inside);
            if byte == b'"' && !escape {
                inside = !inside;
            }
            slashes = if byte == b'\\' { slashes + 1 } else { 0 };
        }
        // With an odd number of quotes, the row's strings can't be told
        // apart: none count.
        if inside {
            quoted.clear();
        }
        quoted.resize(bytes.len(), false);
        Self {
            start,
            bytes,
            escaped,
            quoted,
        }
    }

    /// Bytes `lo..hi` of `row` for a huge row, with what [`Scan::escaped`]
    /// and [`Scan::char_literal`] read around them: the backslashes before
    /// `lo` and a byte each side. Its strings are told apart from `lo` on:
    /// where a huge row's strings begin is not worth reading the row for.
    fn span(text: &Text, row: u32, lo: usize, hi: usize) -> Self {
        let lo = text.clip_offset(lo, Bias::Left);
        let start = text.row_start(row);
        let end = start + text.row_len(row) as usize;
        let mut back = 256;
        let from = loop {
            let from = text.clip_offset(lo.saturating_sub(back).max(start), Bias::Left);
            let before = text.slice_to_string(from..lo);
            let slashes = before
                .bytes()
                .rev()
                .take_while(|&byte| byte == b'\\')
                .count();
            if slashes < before.len() || from == start {
                let context = lo - slashes - usize::from(slashes < before.len());
                break text.clip_offset(context, Bias::Left);
            }
            back *= 2;
        };
        let hi = text.clip_offset((hi + 1).min(end), Bias::Right);
        Self::of(from, text.slice_to_string(from..hi).into_bytes())
    }

    /// Whether byte `ix` is a bracket quoted as a char, `'('`.
    fn char_literal(&self, ix: usize) -> bool {
        ix > 0 && self.bytes[ix - 1] == b'\'' && self.bytes.get(ix + 1) == Some(&b'\'')
    }

    fn quote_at(&self, ix: usize) -> bool {
        self.bytes[ix] == b'"' && !self.escaped[ix]
    }
}

/// How [`unmatched`] treats double-quoted strings.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Quotes {
    /// Brackets inside strings count like any others.
    Ignore,
    /// Brackets inside strings are skipped: starting outside one.
    Skip,
    /// Only the string started in counts, up to its closing quote.
    Within,
}

impl Quotes {
    /// [`Quotes::Within`] if `quoted`, else [`Quotes::Skip`]: `%` and
    /// matching a bracket found.
    pub(crate) fn from(quoted: bool) -> Self {
        if quoted { Self::Within } else { Self::Skip }
    }
}

/// How [`unmatched`] treats quotes and escapes, from where it starts.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Rules {
    /// Only brackets escaped like this count (`:help %`, without
    /// 'cpoptions' M).
    pub(crate) escaped: bool,
    pub(crate) quotes: Quotes,
    /// Skip brackets quoted as chars, `'('`, as `%` does.
    pub(crate) literals: bool,
}

/// From `from` (not included), the first `close` (`forward`) or `open`
/// (backward) without a partner in between, by `rules`.
pub(crate) fn unmatched(
    text: &Text,
    from: usize,
    forward: bool,
    pair: (u8, u8),
    rules: Rules,
) -> Option<usize> {
    nth_unmatched(text, from, forward, pair, rules, 1)
}

/// [`unmatched`], going on past `n - 1` of them: the `n`th enclosing
/// bracket, in one pass.
pub(crate) fn nth_unmatched(
    text: &Text,
    from: usize,
    forward: bool,
    (open, close): (u8, u8),
    rules: Rules,
    mut n: u32,
) -> Option<usize> {
    let (want, other) = if forward {
        (close, open)
    } else {
        (open, close)
    };
    let first = text.offset_to_point(from).row;
    let mut depth = 0usize;
    let rows: Box<dyn Iterator<Item = u32>> = match forward {
        true => Box::new(first..text.row_count()),
        false => Box::new((0..=first).rev()),
    };
    for row in rows {
        // A string doesn't go on past its row.
        if rules.quotes == Quotes::Within && row != first {
            return None;
        }
        let (start, len) = (text.row_start(row), text.row_len(row) as usize);
        // The part of the row to look over, in the order to look.
        let (lo, hi) = match (row == first, forward) {
            (true, true) => (from + 1, start + len),
            (true, false) => (start, from.min(start + len)),
            (false, _) => (start, start + len),
        };
        let spanned = spanned(text, row);
        let mut spans = Vec::new();
        let step = match spanned {
            true => CARET_WINDOW,
            false => len.max(1),
        };
        let mut at = lo;
        while at < hi {
            spans.push((at, (at + step).min(hi)));
            at += step;
        }
        if !forward {
            spans.reverse();
        }
        for (lo, hi) in spans {
            let scan = match spanned {
                true => Scan::span(text, row, lo, hi),
                false => Scan::new(text, row),
            };
            // A search starting on a char literal's quote counts its bracket.
            let touches = |ix: usize| {
                row == first && (from + 1 == scan.start + ix || from == scan.start + ix + 1)
            };
            let ixs = lo - scan.start..hi - scan.start;
            let ixs: Box<dyn Iterator<Item = usize>> = match forward {
                true => Box::new(ixs),
                false => Box::new(ixs.rev()),
            };
            for ix in ixs {
                if rules.quotes == Quotes::Within && scan.quote_at(ix) {
                    return None;
                }
                let byte = scan.bytes[ix];
                if (byte != want && byte != other)
                    || scan.escaped[ix] != rules.escaped
                    || (rules.quotes == Quotes::Skip && scan.quoted[ix])
                    || (rules.literals && scan.char_literal(ix) && !touches(ix))
                {
                    continue;
                }
                match (byte == want, depth) {
                    (true, 0) if n <= 1 => return Some(scan.start + ix),
                    (true, 0) => n -= 1,
                    (true, _) => depth -= 1,
                    (false, _) => depth += 1,
                }
            }
        }
    }
    None
}

/// Where `%` goes: an offset, and whether it matched preprocessor rows.
pub(crate) struct Match {
    pub(crate) offset: usize,
    pub(crate) rows: bool,
}

/// `%` without a count (`:help %`): with the caret on `/*` or `*/`, the
/// other end of the comment (from `*/`, the nearest `/*` ending before it); on a preprocessor conditional's `#` (or in the
/// blanks before it), the next `#else`, `#elif` or `#endif` at its level,
/// or from `#endif` its `#if`; otherwise from the first bracket at or after
/// the caret on its row, its match; failing that, the conditional again.
pub(crate) fn match_pair(text: &Text, offset: usize) -> Option<Match> {
    let row = text.offset_to_point(offset).row;
    // A huge row is read a window at a time: the bytes around the caret for
    // what is next to it, and its head for a directive.
    let spanned = spanned(text, row);
    let scan = match spanned {
        true => {
            let lo = offset.saturating_sub(2).max(text.row_start(row));
            Scan::span(text, row, lo, offset + 3)
        }
        false => Scan::new(text, row),
    };
    let col = offset - scan.start;
    let bytes = &scan.bytes;
    let at = |ix: usize, byte: u8| bytes.get(ix) == Some(&byte);
    let chars = |offset: usize| Match {
        offset,
        rows: false,
    };
    // A `*` between `/` and `/` closes; a `/` between `*` and `*` opens.
    let opens = at(col, b'/') && at(col + 1, b'*')
        || at(col, b'*') && !at(col + 1, b'/') && col > 0 && at(col - 1, b'/');
    let closes = !opens
        && (at(col, b'*') && at(col + 1, b'/') || at(col, b'/') && col > 0 && at(col - 1, b'*'));
    if opens {
        let from = scan.start + col + if at(col, b'/') { 2 } else { 1 };
        let rest = text.slice_to_string(from..text.len());
        return rest.find("*/").map(|ix| chars(from + ix + 1));
    }
    if closes {
        let close = scan.start + col - usize::from(at(col, b'/'));
        let before = text.slice_to_string(0..close + 2).into_bytes();
        return comment_open(&before, close).map(chars);
    }
    let directive = match spanned {
        true => row_directive(text, row),
        false => directive(bytes),
    };
    if let Some((hash, _)) = directive
        && offset - text.row_start(row) <= hash
    {
        return conditional(text, row);
    }
    if spanned {
        return match_bracket(text, row, offset)
            .or_else(|| directive.and_then(|_| conditional(text, row)));
    }
    let bracket = (col..bytes.len()).find_map(|ix| {
        let byte = bytes[ix];
        PAIRS.iter().find_map(|&pair| match byte {
            _ if byte == pair.0 => Some((ix, pair, true)),
            _ if byte == pair.1 => Some((ix, pair, false)),
            _ => None,
        })
    });
    let Some((ix, pair, forward)) = bracket else {
        return directive.and_then(|_| conditional(text, row));
    };
    let rules = Rules {
        escaped: scan.escaped[ix],
        quotes: Quotes::from(scan.quoted[ix]),
        literals: true,
    };
    unmatched(text, scan.start + ix, forward, pair, rules).map(chars)
}

/// [`directive`] of `row`, reading its head a chunk at a time, only as far
/// as the first non-blank token takes: the blanks, the `#`, the blanks and
/// the keyword.
fn row_directive(text: &Text, row: u32) -> Option<(usize, Directive)> {
    let start = text.row_start(row);
    let len = text.row_len(row) as usize;
    let blank = |byte: &u8| matches!(byte, b' ' | b'\t');
    let (mut hash, mut at) = (None, 0);
    let mut word = [0u8; "endif".len()];
    let mut have = 0;
    'head: for chunk in text.chunks_in_range(start..start + len) {
        for byte in chunk.as_bytes() {
            match hash {
                None if blank(byte) => at += 1,
                None if *byte == b'#' => hash = Some(at),
                None => return None,
                Some(_) if have == 0 && blank(byte) => {}
                Some(_) => {
                    word[have] = *byte;
                    have += 1;
                    if have == word.len() {
                        break 'head;
                    }
                }
            }
        }
    }
    Some((hash?, directive_kind(&word[..have])?))
}

/// `%`'s bracket on a huge row: the match of the first bracket at or after
/// `offset`, found a window of the row at a time.
fn match_bracket(text: &Text, row: u32, offset: usize) -> Option<Match> {
    let end = text.row_start(row) + text.row_len(row) as usize;
    let mut lo = offset;
    while lo < end {
        let hi = (lo + CARET_WINDOW).min(end);
        let scan = Scan::span(text, row, lo, hi);
        let found = (lo - scan.start..hi - scan.start).find_map(|ix| {
            let byte = scan.bytes[ix];
            PAIRS.iter().find_map(|&pair| match byte {
                _ if byte == pair.0 => Some((ix, pair, true)),
                _ if byte == pair.1 => Some((ix, pair, false)),
                _ => None,
            })
        });
        if let Some((ix, pair, forward)) = found {
            let rules = Rules {
                escaped: scan.escaped[ix],
                quotes: Quotes::from(scan.quoted[ix]),
                literals: true,
            };
            return unmatched(text, scan.start + ix, forward, pair, rules).map(|offset| Match {
                offset,
                rows: false,
            });
        }
        lo = hi;
    }
    None
}

/// The `/*` that `%` matches with the `*/` at `close` in `text` (which
/// runs to the end of that `*/`), as nvim's backward scan picks it, in one
/// pass: from the `*/`, back through the text, counting each `/*` that
/// isn't right after a `*` and doesn't sit at or after a `//` line comment
/// on its row; the first `*/` met ends the scan, matching the earliest
/// counted `/*` after it, or, with none counted, an overlapping `/*/`.
fn comment_open(text: &[u8], close: usize) -> Option<usize> {
    let row_start = |ix: usize| {
        text[..ix]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |nl| nl + 1)
    };
    // Where a `//` begins on the row at `start`, unless it ends a `*/` and
    // starts a `/*`; nvim's `check_linecomment`.
    let line_comment = |start: usize| {
        let row = &text[start..];
        let end = row.iter().position(|&b| b == b'\n').unwrap_or(row.len());
        (0..end.saturating_sub(1))
            .find(|&ix| {
                row[ix] == b'/'
                    && row[ix + 1] == b'/'
                    && (ix == 0 || row[ix - 1] != b'*' || row.get(ix + 2) != Some(&b'*'))
            })
            .map_or(usize::MAX, |ix| start + ix)
    };
    let mut start = row_start(close);
    let mut comment = line_comment(start);
    let mut pos = close;
    let mut counted = None;
    loop {
        if pos == start {
            if start == 0 {
                return counted;
            }
            // Onto the previous row's end.
            pos = start - 1;
            start = row_start(pos);
            comment = line_comment(start);
        } else {
            pos -= 1;
        }
        if pos == start {
            continue;
        }
        if text[pos - 1] == b'/'
            && text[pos] == b'*'
            && (pos == start + 1 || text[pos - 2] != b'*')
            && pos < comment
        {
            counted = Some(pos - 1);
        } else if text[pos - 1] == b'*' && text[pos] == b'/' {
            if counted.is_some() {
                return counted;
            }
            return (pos > start + 1 && text[pos - 2] == b'/' && pos <= comment).then(|| pos - 2);
        }
    }
}

/// A preprocessor conditional on a row: its `#`'s column and its kind.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Directive {
    /// `#if`, `#ifdef`, `#ifndef`.
    If,
    /// `#else`, `#elif`.
    Else,
    Endif,
}

fn directive(bytes: &[u8]) -> Option<(usize, Directive)> {
    let blank = |byte: &u8| matches!(byte, b' ' | b'\t');
    let hash = bytes.iter().position(|byte| !blank(byte))?;
    if bytes[hash] != b'#' {
        return None;
    }
    let word = &bytes[hash + 1..];
    let word = &word[word
        .iter()
        .position(|byte| !blank(byte))
        .unwrap_or(word.len())..];
    Some((hash, directive_kind(word)?))
}

/// The kind of directive whose keyword starts `word`.
fn directive_kind(word: &[u8]) -> Option<Directive> {
    match () {
        _ if word.starts_with(b"endif") => Some(Directive::Endif),
        _ if word.starts_with(b"if") => Some(Directive::If),
        _ if word.starts_with(b"el") => Some(Directive::Else),
        _ => None,
    }
}

/// From the conditional on `row`: the next `#else`, `#elif` or `#endif` at
/// its level, or from an `#endif`, its `#if`. The `#`'s offset, as rows.
fn conditional(text: &Text, row: u32) -> Option<Match> {
    let (_, kind) = row_directive(text, row)?;
    let forward = kind != Directive::Endif;
    let mut depth = 0usize;
    let rows: Box<dyn Iterator<Item = u32>> = match forward {
        true => Box::new(row + 1..text.row_count()),
        false => Box::new((0..row).rev()),
    };
    for other in rows {
        let Some((hash, found)) = row_directive(text, other) else {
            continue;
        };
        let opens = if forward {
            Directive::If
        } else {
            Directive::Endif
        };
        let closes = if forward {
            Directive::Endif
        } else {
            Directive::If
        };
        match found {
            _ if found == opens => depth += 1,
            _ if found == closes && depth > 0 => depth -= 1,
            Directive::Else if !forward => {}
            _ if depth == 0 => {
                return Some(Match {
                    offset: text.row_start(other) + hash,
                    rows: true,
                });
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod caret_window_tests {
    use super::*;

    /// 3,000,000 bytes on one row, between two short ones.
    fn huge_row() -> (Text, usize) {
        let row = "ab (c) ".repeat(430_000);
        let text = Text::new(&format!("x\n{row}\ny"));
        let middle = 2 + 7 * 215_000 + 1;
        (text, middle)
    }

    /// The most bytes of the row a key may read: its windows, a few.
    const BUDGET: usize = 16 * CARET_WINDOW;

    fn key(text: &Text, offset: usize, motion: Motion) -> usize {
        let mut cursor = Cursor::at(text, offset);
        let args = MoveArgs {
            motion,
            extend: false,
            count: (motion != Motion::MatchPair).then_some(3),
        };
        let _ = super::motion(&mut cursor, args, None, None, 4);
        cursor.offset()
    }

    #[test]
    fn a_caret_on_a_huge_row_reads_a_window_of_it() {
        let (text, middle) = huge_row();
        ROW_BYTES.set(0);
        let cursor = Cursor::at(&text, middle);
        assert_eq!(cursor.offset(), middle);
        assert_eq!(cursor.line.grapheme(cursor.ix), "b");
        assert!(ROW_BYTES.get() <= BUDGET, "{} bytes read", ROW_BYTES.get());
        // Near either end the window is cut short, not the caret moved.
        for offset in [2, 3, text.len() - 3] {
            ROW_BYTES.set(0);
            let cursor = Cursor::at(&text, offset);
            assert_eq!(cursor.offset(), offset, "{offset}");
            assert!(ROW_BYTES.get() <= BUDGET, "{offset}");
        }
    }

    #[test]
    fn motions_from_the_middle_of_a_huge_row_read_windows_of_it() {
        let (text, middle) = huge_row();
        for motion in [
            Motion::WordForward,
            Motion::WordBackward,
            Motion::WordEnd,
            Motion::WordEndBackward,
            Motion::BigWordForward,
            Motion::FindForward('c'),
            Motion::FindBackward('c'),
            Motion::TillForward('c'),
            Motion::Left,
            Motion::Right,
            Motion::LineStart,
            Motion::LineEnd,
            Motion::MatchPair,
        ] {
            ROW_BYTES.set(0);
            let at = key(&text, middle, motion);
            assert!(
                ROW_BYTES.get() <= 3 * BUDGET,
                "{motion:?} read {} bytes",
                ROW_BYTES.get()
            );
            assert!(at > 0, "{motion:?}");
        }
    }

    /// Letters and punctuation alternating, with no blank to end a WORD at.
    #[test]
    fn a_row_without_blanks_is_read_in_windows_too() {
        let text = Text::new(&format!("x\n{}\ny", "a.".repeat(1_500_000)));
        let middle = 2 + 1_500_000;
        ROW_BYTES.set(0);
        let cursor = Cursor::at(&text, middle);
        assert_eq!(cursor.offset(), middle);
        assert!(
            ROW_BYTES.get() <= 40 * CARET_WINDOW,
            "{} bytes",
            ROW_BYTES.get()
        );
        for motion in [
            Motion::WordForward,
            Motion::WordBackward,
            Motion::WordEnd,
            Motion::WordEndBackward,
            Motion::FindForward('a'),
            Motion::FindBackward('.'),
            Motion::Left,
            Motion::Right,
        ] {
            ROW_BYTES.set(0);
            key(&text, middle, motion);
            assert!(
                ROW_BYTES.get() <= 3 * 40 * CARET_WINDOW,
                "{motion:?} read {} bytes",
                ROW_BYTES.get()
            );
        }
    }

    /// A caret in the middle of a 3 MB run of one kind reads a bounded window.
    #[test]
    fn a_caret_in_a_huge_uninterrupted_run_reads_a_bounded_window() {
        for (kind, unit) in [
            ("letters", "a"),
            ("cyrillic", "\u{436}"),
            ("emoji", "\u{1f600}"),
            ("tabs", "\t"),
            ("blanks", " "),
        ] {
            let run = unit.repeat(3_000_000 / unit.len());
            let text = Text::new(&format!("x\n{run}\ny"));
            let middle = 2 + (run.len() / 2) / unit.len() * unit.len();
            ROW_BYTES.set(0);
            let cursor = Cursor::at(&text, middle);
            assert_eq!(cursor.offset(), middle, "{kind}");
            assert!(
                ROW_BYTES.get() <= 40 * CARET_WINDOW,
                "{kind}: {} bytes",
                ROW_BYTES.get()
            );
        }
    }

    #[test]
    fn a_short_row_is_read_whole() {
        let text = Text::new("ab cd\nef");
        ROW_BYTES.set(0);
        let cursor = Cursor::at(&text, 1);
        assert_eq!(cursor.line.text, "ab cd");
        assert!(!cursor.windowed);
        assert_eq!(ROW_BYTES.get(), 5);
    }

    #[test]
    fn the_window_never_cuts_the_carets_grapheme() {
        // One grapheme of 120,001 bytes in the middle of a 220 KB row.
        let marks = "\u{301}".repeat(60_000);
        let (before, after) = ("x".repeat(50_000), "y".repeat(50_000));
        let text = Text::new(&format!("{before}e{marks}{after}"));
        let start = before.len();
        for offset in [start, start + 2, start + 60_000, start + 120_000 - 1] {
            let cursor = Cursor::at(&text, offset);
            assert_eq!(cursor.offset(), start, "{offset}");
            assert_eq!(cursor.line.grapheme(cursor.ix).len(), 120_001, "{offset}");
        }
    }

    #[test]
    fn the_window_keeps_regional_indicator_pairs_whole() {
        // 80,000 flags' halves (320 KB, 40,000 pairs) between ASCII.
        let flags = "\u{1f1e6}".repeat(80_000);
        let pad = "x".repeat(60_000);
        let text = Text::new(&format!("{pad}{flags}{pad}"));
        let start = pad.len();
        for pair in [0, 1, 9_999, 20_000, 39_999] {
            let first = start + pair * 8;
            for offset in [first, first + 4] {
                let cursor = Cursor::at(&text, offset);
                assert_eq!(cursor.offset(), first, "{offset}");
                assert_eq!(cursor.line.grapheme(cursor.ix).len(), 8, "{offset}");
            }
        }
    }
}

#[cfg(test)]
mod printable_tests {
    use super::printable;

    #[test]
    fn printable_agrees_with_a_byte_at_a_time() {
        let mut bytes = [b'a'; 40];
        assert!(printable(&bytes));
        for at in 0..40 {
            for byte in [0u8, 9, 0x1f, 0x20, 0x7e, 0x7f, 0x80, 0xc3, 0xff] {
                let old = bytes[at];
                bytes[at] = byte;
                assert_eq!(
                    printable(&bytes),
                    (0x20..0x7f).contains(&byte),
                    "{at} {byte}"
                );
                bytes[at] = old;
            }
        }
    }
}
