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
use fe_text::Text;
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
    match marks::resolve(target, args.motion) {
        Ok(motion) => args.motion = motion,
        Err(message) => return target.state.fail(message),
    }
    if let Some(from) = marks::jump(target, args.motion, args.count)
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
    let count = args.count.unwrap_or(1).max(1);
    let n = count as usize;
    let (row, last_row) = (cursor.row, cursor.text.row_count() - 1);
    let line = &cursor.line;
    match args.motion {
        // A motion that fails leaves the goal alone, as in Vim. For an
        // operator, `l` may step past the last grapheme: `x` takes it.
        Motion::Left | Motion::Right => {
            let ix = match args.motion {
                Motion::Left => cursor.ix.saturating_sub(n),
                _ if cursor.op => cursor.ix.saturating_add(n).min(line.len()),
                _ => cursor.ix.saturating_add(n).min(line.last()),
            };
            // An operator's `h` at the row's start (or `l` at its end) only
            // has nothing to act on: `>h` still shifts the row.
            if ix == cursor.ix {
                return if cursor.op { Ok(goal) } else { Err(goal) };
            }
            cursor.ix = ix;
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
        Motion::LineStart => cursor.ix = 0,
        Motion::FirstNonBlank => cursor.ix = line.first_non_blank(),
        // Vim fails a `{count}$` from the last row (still aiming at line
        // ends), and clamps one from above.
        Motion::LineEnd if count > 1 && row == last_row => return Err(Some(usize::MAX)),
        Motion::LineEnd => {
            cursor.enter(row.saturating_add(count - 1).min(last_row));
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
            let landed = search::parsed(search.pattern)
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
        | Motion::Mark(_) => return Err(goal),
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
}

impl Line {
    pub(crate) fn new(text: &Text, row: u32) -> Self {
        Self::of(text.row_start(row), text.row_to_string(row))
    }

    /// `text` as a row starting at byte `start`.
    pub(crate) fn of(start: usize, text: String) -> Self {
        let starts = text.grapheme_indices(true).map(|(ix, _)| ix).collect();
        Self {
            start,
            text,
            starts,
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
        (0..ix).fold(0, |column, i| column + self.width(i, column, tab))
    }

    /// Vim's goal column for a normal-mode caret on grapheme `ix`: the
    /// display column it starts at, except for a tab, whose last column it
    /// is (where the caret shows).
    pub(crate) fn goal_column(&self, ix: usize, tab: usize) -> usize {
        if self.len() == 0 {
            return 0;
        }
        let start = (0..ix).fold(0, |column, i| column + self.width(i, column, tab));
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
}

impl<'a> Cursor<'a> {
    /// The normal-mode caret for `offset`: on the grapheme holding it,
    /// clamped to the row's last.
    pub(crate) fn at(text: &'a Text, offset: usize) -> Self {
        let row = text.offset_to_point(offset).row;
        let mut cursor = Self {
            text,
            row,
            ix: 0,
            line: Line::new(text, row),
            op: false,
            visual: false,
            word_row: row,
            hit_end: false,
            stuck_at_start: false,
            rows_matched: false,
        };
        cursor.goto(offset);
        cursor
    }

    /// To the grapheme holding `offset`, clamped to its row's last.
    pub(crate) fn goto(&mut self, offset: usize) {
        self.enter(self.text.offset_to_point(offset).row);
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
        let line = &self.line;
        match find {
            Motion::FindForward(ch) | Motion::TillForward(ch) => {
                let till = matches!(find, Motion::TillForward(_));
                let from = self.ix + 1 + usize::from(till && skip);
                let mut found = (from..line.len()).filter(|&ix| line.starts_with(ix, ch));
                // `t` onto the very next grapheme fails too, though an
                // operator then takes the caret's own; `T` doesn't.
                match found.nth(n - 1).map(|ix| ix - usize::from(till)) {
                    Some(ix) if ix != self.ix || self.op => self.ix = ix,
                    _ => return false,
                }
            }
            Motion::FindBackward(ch) | Motion::TillBackward(ch) => {
                let till = matches!(find, Motion::TillBackward(_));
                let to = self.ix.saturating_sub(usize::from(till && skip));
                let mut found = (0..to).rev().filter(|&ix| line.starts_with(ix, ch));
                match found.nth(n - 1) {
                    // `T` steps one character past the match, which inside a
                    // grapheme with combining marks is the match itself.
                    Some(ix) => {
                        self.ix =
                            ix + usize::from(till && line.grapheme(ix).chars().nth(1).is_none())
                    }
                    None => return false,
                }
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn enter(&mut self, row: u32) {
        if row != self.row {
            self.row = row;
            self.line = Line::new(self.text, row);
        }
    }

    /// To the grapheme on `row` covering the goal column (by default, the
    /// one this caret is on), or the row's last; returns the goal.
    pub(crate) fn vertical(&mut self, row: u32, goal: Option<usize>, tab: usize) -> Option<usize> {
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
        if self.ix + 1 < self.line.len() {
            self.ix += 1;
        } else if self.row + 1 < self.text.row_count() {
            self.enter(self.row + 1);
            self.ix = 0;
        } else {
            return false;
        }
        true
    }

    /// To the previous grapheme, or the previous row's last; false at the
    /// start.
    pub(crate) fn backward(&mut self) -> bool {
        if self.ix > 0 {
            self.ix -= 1;
        } else if self.row > 0 {
            self.enter(self.row - 1);
            self.ix = self.line.last();
        } else {
            return false;
        }
        true
    }

    /// `w`, `b` or `e` (`W B E` if `big`) `count` times, stopping once a step
    /// doesn't move.
    pub(crate) fn words(&mut self, motion: Motion, count: u32, big: bool) {
        for step in 0..count {
            let before = (self.row, self.ix);
            self.word_row = self.row;
            match motion {
                Motion::WordForward => self.word_forward(big),
                Motion::WordBackward => self.word_backward(big),
                _ => self.word_end(big),
            }
            if (self.row, self.ix) == before {
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
        while self.class_on_row(self.ix + 1, big) == Some(class) {
            self.ix += 1;
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
        while class != Class::EmptyRow
            && self.ix > 0
            && self.class_on_row(self.ix - 1, big) == Some(class)
        {
            self.ix -= 1;
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
        let bytes = text.row_to_string(row).into_bytes();
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
            start: text.row_start(row),
            bytes,
            escaped,
            quoted,
        }
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
        let scan = Scan::new(text, row);
        let len = scan.bytes.len();
        // A search starting on a char literal's quote counts its bracket.
        let touches = |ix: usize| {
            row == first && (from - scan.start + 1 == ix || from - scan.start == ix + 1)
        };
        let ixs: Box<dyn Iterator<Item = usize>> = match (row == first, forward) {
            (true, true) => Box::new(from - scan.start + 1..len),
            (true, false) => Box::new((0..(from - scan.start).min(len)).rev()),
            (false, true) => Box::new(0..len),
            (false, false) => Box::new((0..len).rev()),
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
    let scan = Scan::new(text, row);
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
    let directive = directive(bytes);
    if let Some((hash, _)) = directive
        && col <= hash
    {
        return conditional(text, row);
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
    let kind = match () {
        _ if word.starts_with(b"endif") => Directive::Endif,
        _ if word.starts_with(b"if") => Directive::If,
        _ if word.starts_with(b"el") => Directive::Else,
        _ => return None,
    };
    Some((hash, kind))
}

/// From the conditional on `row`: the next `#else`, `#elif` or `#endif` at
/// its level, or from an `#endif`, its `#if`. The `#`'s offset, as rows.
fn conditional(text: &Text, row: u32) -> Option<Match> {
    let (_, kind) = directive(&text.row_to_string(row).into_bytes())?;
    let forward = kind != Directive::Endif;
    let mut depth = 0usize;
    let rows: Box<dyn Iterator<Item = u32>> = match forward {
        true => Box::new(row + 1..text.row_count()),
        false => Box::new((0..row).rev()),
    };
    for other in rows {
        let Some((hash, found)) = directive(&text.row_to_string(other).into_bytes()) else {
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
