//! Finding text in the grid and its scrollback, on alacritty's regex search.
//! The query and its matches are in this crate's own types; [`Terminal::find`]
//! and [`Terminal::find_all_visible`] run them.
//!
//! [`Terminal::find`]: crate::Terminal::find
//! [`Terminal::find_all_visible`]: crate::Terminal::find_all_visible

use std::fmt;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};

/// What to find.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TerminalQuery {
    pub text: String,
    /// `text` is a regex; otherwise it's matched literally.
    pub regex: bool,
    /// `None` is smart case: case-insensitive unless `text` has an uppercase
    /// letter.
    pub case_sensitive: Option<bool>,
}

impl TerminalQuery {
    /// A literal, smart-case query.
    pub fn literal(text: impl Into<String>) -> TerminalQuery {
        TerminalQuery {
            text: text.into(),
            regex: false,
            case_sensitive: None,
        }
    }

    /// A smart-case regex query.
    pub fn regex(text: impl Into<String>) -> TerminalQuery {
        TerminalQuery {
            text: text.into(),
            regex: true,
            case_sensitive: None,
        }
    }

    /// The pattern alacritty compiles: `text`, escaped unless it's a regex,
    /// with a case flag when `case_sensitive` is set.
    fn pattern(&self) -> String {
        let text = if self.regex {
            self.text.clone()
        } else {
            escape(&self.text)
        };
        match self.case_sensitive {
            None => text,
            Some(true) => format!("(?-i){text}"),
            Some(false) => format!("(?i){text}"),
        }
    }
}

/// `text` with every regex metacharacter escaped.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '.'
                | '+'
                | '*'
                | '?'
                | '('
                | ')'
                | '|'
                | '['
                | ']'
                | '{'
                | '}'
                | '^'
                | '$'
                | '#'
                | '&'
                | '-'
                | '~'
        ) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// A query that doesn't compile, with the regex engine's message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryError(pub String);

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for QueryError {}

/// A cell in the grid. Line 0 is the screen's top row, lines below it are the
/// rest of the screen, and negative lines are scrollback (-1 is the newest).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GridPoint {
    pub line: i32,
    pub col: usize,
}

/// A match, from its first cell to its last (inclusive). It may span lines
/// where the text soft-wrapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TerminalMatch {
    pub start: GridPoint,
    pub end: GridPoint,
}

/// `range` in this crate's types. Not a `From` impl, which would put
/// alacritty's types in the public API.
pub(crate) fn to_match(range: &Match) -> TerminalMatch {
    let point = |point: &Point| GridPoint {
        line: point.line.0,
        col: point.column.0,
    };
    TerminalMatch {
        start: point(range.start()),
        end: point(range.end()),
    }
}

/// The most matches [`find_all_visible`] returns.
const MAX_VISIBLE_MATCHES: usize = 1_000;

/// `query` compiled, or `None` for an empty query, which matches nothing.
pub(crate) fn compile(query: &TerminalQuery) -> Result<Option<RegexSearch>, QueryError> {
    if query.text.is_empty() {
        return Ok(None);
    }
    RegexSearch::new(&query.pattern())
        .map(Some)
        .map_err(|error| QueryError(error.to_string()))
}

/// `query`'s next match from `origin` in `forward`'s direction, wrapping
/// through the whole grid once: forward, the first whose `side` end is at or
/// after `origin`; back, the last whose `side` end is at or before it.
pub(crate) fn find_from<T>(
    term: &Term<T>,
    query: &TerminalQuery,
    origin: Point,
    forward: bool,
    side: Side,
) -> Result<Option<Match>, QueryError> {
    let Some(mut regex) = compile(query)? else {
        return Ok(None);
    };
    let direction = if forward {
        Direction::Right
    } else {
        Direction::Left
    };
    Ok(term.search_next(&mut regex, origin, direction, side, None))
}

/// The next match from the selection, else the cursor, in `forward`'s
/// direction, wrapping through the whole grid once. A match is selected and
/// scrolled into view; with none, nothing changes.
pub(crate) fn find<T: EventListener>(
    term: &mut Term<T>,
    query: &TerminalQuery,
    forward: bool,
) -> Result<Option<TerminalMatch>, QueryError> {
    let selected = term.selection.as_ref().and_then(|s| s.to_range(term));
    // Past the selection, so the selected match isn't found again.
    let origin = match (selected, forward) {
        (Some(range), true) => range.end.add(term, Boundary::None, 1),
        (Some(range), false) => range.start.sub(term, Boundary::None, 1),
        (None, _) => term.grid().cursor.point,
    };
    let side = if forward { Side::Left } else { Side::Right };
    let Some(found) = find_from(term, query, origin, forward, side)? else {
        return Ok(None);
    };

    let (start, end) = (*found.start(), *found.end());
    let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
    selection.update(end, Side::Right);
    term.selection = Some(selection);
    if start.line >= 0 {
        if term.grid().display_offset() != 0 {
            term.scroll_display(Scroll::Bottom);
        }
    } else {
        term.scroll_to_point(start);
    }
    Ok(Some(to_match(&found)))
}

/// The matches touching the visible rows, top to bottom, at most
/// [`MAX_VISIBLE_MATCHES`].
pub(crate) fn find_all_visible<T>(
    term: &Term<T>,
    query: &TerminalQuery,
) -> Result<Vec<TerminalMatch>, QueryError> {
    let Some(mut regex) = compile(query)? else {
        return Ok(Vec::new());
    };
    let top = Line(-(term.grid().display_offset() as i32));
    let bottom = top + (term.screen_lines() as i32 - 1);
    // From the start of the top row's wrapped line to the end of the bottom
    // row's, so a match that wraps into view is found whole.
    let start = term.line_search_left(Point::new(top, Column(0)));
    let end = term.line_search_right(Point::new(bottom, term.last_column()));
    let matches = RegexIter::new(start, end, Direction::Right, term, &mut regex)
        .skip_while(|found| found.end().line < top)
        .take_while(|found| found.start().line <= bottom)
        .take(MAX_VISIBLE_MATCHES)
        .map(|found| to_match(&found))
        .collect();
    Ok(matches)
}
