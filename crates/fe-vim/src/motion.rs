//! Vim's motions over an [`EditorModel`]'s text, for every caret.
//!
//! A normal-mode caret sits at the start of a grapheme, never past a
//! non-empty line's last one. Counts step graphemes; goal columns are
//! display columns with tabs expanded to the model's tab size. Every motion
//! is bounded by the text, not the count: rows are computed directly, and
//! repeated motions stop at the first step that doesn't move.

use std::sync::OnceLock;

use fe_buffer::ResolvedSelection;
use fe_editor::EditorModel;
use fe_text::Text;
use unicode_segmentation::UnicodeSegmentation;

use crate::{Motion, MoveArgs, VimState};

/// Moves every caret by `args`; returns whether any selection changed.
pub(crate) fn apply(model: &mut EditorModel, state: &mut VimState, args: MoveArgs) -> bool {
    // Caught up first, so the positions read are the ones moved from.
    model.sync_display();
    let text = model.text().clone();
    let version = model.snapshot().version();
    let before = model.selections();
    // Goals hold only while the carets are where vim left them.
    let goals = state.goals.as_ref().filter(|(at, goals)| {
        *at == version
            && goals
                .iter()
                .map(|goal| goal.0)
                .eq(before.iter().map(|s| s.head()))
    });
    let tab = model.tab_size().max(1);
    let mut moved = Vec::with_capacity(before.len());
    for (ix, selection) in before.iter().enumerate() {
        let mut cursor = Cursor::at(&text, selection.head());
        let goal = goals.and_then(|(_, goals)| goals[ix].1);
        let goal = motion(&mut cursor, args, goal, tab);
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
    after != before
}

/// Moves one caret; returns the goal column it keeps (`None` resets it).
fn motion(cursor: &mut Cursor, args: MoveArgs, goal: Option<usize>, tab: usize) -> Option<usize> {
    let count = args.count.unwrap_or(1).max(1);
    let n = count as usize;
    let (row, last_row) = (cursor.row, cursor.text.row_count() - 1);
    let line = &cursor.line;
    match args.motion {
        // A motion that fails (`h` at the line start, `l` at its end, an `f`
        // with no match) leaves the goal alone, as in Vim.
        Motion::Left | Motion::Right => {
            let ix = match args.motion {
                Motion::Left => cursor.ix.saturating_sub(n),
                _ => cursor.ix.saturating_add(n).min(line.last()),
            };
            if ix == cursor.ix {
                return goal;
            }
            cursor.ix = ix;
        }
        // Vim fails `k` on the first row and `j` on the last, keeping the
        // goal; from elsewhere a count past the end clamps.
        Motion::Up if row == 0 => return goal,
        Motion::Down if row == last_row => return goal,
        Motion::Up => return cursor.vertical(row.saturating_sub(count), goal, tab),
        Motion::Down => return cursor.vertical(row.saturating_add(count).min(last_row), goal, tab),
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
            return cursor.vertical(to_row, goal, tab);
        }
        Motion::LineStart => cursor.ix = 0,
        Motion::FirstNonBlank => cursor.ix = line.first_non_blank(),
        // Vim fails a `{count}$` from the last row (still aiming at line
        // ends), and clamps one from above.
        Motion::LineEnd if count > 1 && row == last_row => return Some(usize::MAX),
        Motion::LineEnd => {
            cursor.enter(row.saturating_add(count - 1).min(last_row));
            cursor.ix = cursor.line.last();
            return Some(usize::MAX);
        }
        Motion::FindForward(ch) | Motion::TillForward(ch) => {
            let till = matches!(args.motion, Motion::TillForward(_));
            let mut found = (cursor.ix + 1..line.len()).filter(|&ix| line.starts_with(ix, ch));
            // `t` onto the very next grapheme fails too; `T` doesn't.
            match found.nth(n - 1).map(|ix| ix - usize::from(till)) {
                Some(ix) if ix != cursor.ix => cursor.ix = ix,
                _ => return goal,
            }
        }
        Motion::FindBackward(ch) | Motion::TillBackward(ch) => {
            let till = matches!(args.motion, Motion::TillBackward(_));
            let mut found = (0..cursor.ix).rev().filter(|&ix| line.starts_with(ix, ch));
            match found.nth(n - 1) {
                // `T` steps one character past the match, which inside a
                // grapheme with combining marks is the match itself.
                Some(ix) => {
                    cursor.ix = ix + usize::from(till && line.grapheme(ix).chars().nth(1).is_none())
                }
                None => return goal,
            }
        }
        Motion::WordForward | Motion::WordBackward | Motion::WordEnd => {
            cursor.words(args.motion, count, false)
        }
        Motion::BigWordForward => cursor.words(Motion::WordForward, count, true),
        Motion::BigWordBackward => cursor.words(Motion::WordBackward, count, true),
        Motion::BigWordEnd => cursor.words(Motion::WordEnd, count, true),
        // `dd`'s target. As a move it is Vim's `_`: the first non-blank
        // `count - 1` rows down.
        Motion::Line => {
            cursor.enter(row.saturating_add(count - 1).min(last_row));
            cursor.ix = cursor.line.first_non_blank();
        }
    }
    None
}

/// Vim's word classes: `Of` holds nvim's `charclass()` (1 punctuation, 2
/// word, 3 emoji, others a script's own class). An empty row is a word of
/// its own; for `W B E`, every non-blank is one class.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Class {
    Blank,
    Of(u32),
    EmptyRow,
}

/// nvim's `charclass()` and `strdisplaywidth()` for every code point, as hex
/// runs `start:class:width`, written by the ignored
/// `record_char_table_with_neovim` test (`tests/support/chars.lua`).
const CHARS: &str = include_str!("chars.txt");

/// `ch`'s class and display width, from [`CHARS`].
fn char_info(ch: char) -> (u32, usize) {
    static RUNS: OnceLock<Vec<(u32, u32, usize)>> = OnceLock::new();
    let runs = RUNS.get_or_init(|| {
        let hex = |field: &str| u32::from_str_radix(field, 16).expect("chars.txt is hex");
        let run = |run: &str| {
            let mut fields = run.split(':').map(hex);
            let mut next = || fields.next().expect("chars.txt runs are start:class:width");
            (next(), next(), next() as usize)
        };
        CHARS.split_whitespace().map(run).collect()
    });
    let ix = runs.partition_point(|run| run.0 <= u32::from(ch));
    let (_, class, width) = runs[ix - 1];
    (class, width)
}

/// A grapheme's class is its first character's, as in nvim.
fn class(grapheme: &str, big: bool) -> Class {
    match char_info(grapheme.chars().next().unwrap_or(' ')).0 {
        0 => Class::Blank,
        _ if big => Class::Of(2),
        class => Class::Of(class),
    }
}

/// One row's text and the byte offset of each grapheme in it.
struct Line {
    start: usize,
    text: String,
    starts: Vec<usize>,
}

impl Line {
    fn new(text: &Text, row: u32) -> Self {
        let line = text.row_to_string(row);
        let starts = line.grapheme_indices(true).map(|(ix, _)| ix).collect();
        let start = text.row_start(row);
        Self {
            start,
            text: line,
            starts,
        }
    }

    fn len(&self) -> usize {
        self.starts.len()
    }

    /// The last grapheme's index; 0 on an empty row.
    fn last(&self) -> usize {
        self.len().saturating_sub(1)
    }

    fn grapheme(&self, ix: usize) -> &str {
        let end = self.starts.get(ix + 1).copied().unwrap_or(self.text.len());
        &self.text[self.starts[ix]..end]
    }

    /// Whether grapheme `ix`'s base character is `ch`, as `f` matches.
    fn starts_with(&self, ix: usize, ch: char) -> bool {
        self.grapheme(ix).starts_with(ch)
    }

    /// `^`: the first non-blank grapheme, or the last if all are blank.
    /// Only a space or a tab is blank here, unlike for `w`.
    fn first_non_blank(&self) -> usize {
        (0..self.len())
            .find(|&ix| !matches!(self.grapheme(ix), " " | "\t"))
            .unwrap_or(self.last())
    }

    /// The display width of grapheme `ix` starting at display `column`.
    fn width(&self, ix: usize, column: usize, tab: usize) -> usize {
        let grapheme = self.grapheme(ix);
        let ch = grapheme.chars().next().unwrap_or(' ');
        match ch {
            '\t' => tab - column % tab,
            // Emoji presentation (VS16) makes it two columns wide.
            _ if grapheme.contains('\u{fe0f}') => char_info(ch).1.max(2),
            _ => char_info(ch).1,
        }
    }

    /// Vim's goal column for a normal-mode caret on grapheme `ix`: the
    /// display column it starts at, except for a tab, whose last column it
    /// is (where the caret shows).
    fn goal_column(&self, ix: usize, tab: usize) -> usize {
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
struct Cursor<'a> {
    text: &'a Text,
    row: u32,
    ix: usize,
    line: Line,
}

impl<'a> Cursor<'a> {
    /// The normal-mode caret for `offset`: on the grapheme holding it,
    /// clamped to the row's last.
    fn at(text: &'a Text, offset: usize) -> Self {
        let row = text.offset_to_point(offset).row;
        let line = Line::new(text, row);
        let column = offset.saturating_sub(line.start);
        let ix = line.starts.partition_point(|&start| start <= column);
        let ix = ix.saturating_sub(1).min(line.last());
        Self {
            text,
            row,
            ix,
            line,
        }
    }

    fn offset(&self) -> usize {
        self.line.start + self.line.starts.get(self.ix).copied().unwrap_or(0)
    }

    fn enter(&mut self, row: u32) {
        if row != self.row {
            self.row = row;
            self.line = Line::new(self.text, row);
        }
    }

    /// To the grapheme on `row` covering the goal column (by default, the
    /// one this caret is on), or the row's last; returns the goal.
    fn vertical(&mut self, row: u32, goal: Option<usize>, tab: usize) -> Option<usize> {
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

    fn class(&self, big: bool) -> Class {
        match self.line.len() {
            0 => Class::EmptyRow,
            _ => class(self.line.grapheme(self.ix), big),
        }
    }

    /// The class of grapheme `ix` on this row, if it has one.
    fn class_on_row(&self, ix: usize, big: bool) -> Option<Class> {
        (ix < self.line.len()).then(|| class(self.line.grapheme(ix), big))
    }

    /// To the next grapheme, or the next row's first; false at the end.
    fn forward(&mut self) -> bool {
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
    fn backward(&mut self) -> bool {
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
    fn words(&mut self, motion: Motion, count: u32, big: bool) {
        for _ in 0..count {
            let before = (self.row, self.ix);
            match motion {
                Motion::WordForward => self.word_forward(big),
                Motion::WordBackward => self.word_backward(big),
                _ => self.word_end(big),
            }
            if (self.row, self.ix) == before {
                break;
            }
        }
    }

    /// `w`: past this word, then past blanks to the next word or empty row.
    /// Off the end of the text, it stays on the last grapheme.
    fn word_forward(&mut self, big: bool) {
        let (class, row) = (self.class(big), self.row);
        if !matches!(class, Class::Blank | Class::EmptyRow) {
            while self.forward() {
                if self.row != row || self.class(big) != class {
                    break;
                }
            }
        } else {
            self.forward();
        }
        while self.class(big) == Class::Blank && self.forward() {}
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
