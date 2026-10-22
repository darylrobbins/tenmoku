//! Replacements for a find bar's [`Pattern`]: a template parsed once
//! ([`Replacement`]) and expanded against a match ([`Pattern::replacement_at`],
//! [`replace_all`]); and the matches `:s` takes over rows for a pattern that
//! [`Pattern::spans_rows`] ([`Walk`]).
//!
//! Semantics against `:s`: the find bar's pattern is the `regex` crate's
//! syntax with case from its Match Case toggle (off is case-insensitive
//! whatever the query holds, no smartcase), and its replacement uses `$n`
//! groups; `:s` keeps Vim's syntax (`\(\)`, `\1`, `&`, `\c`, 'ignorecase' off
//! by default). Both match within one row, and for equivalent patterns they
//! produce the same text; only `:s` takes a pattern that spans rows.

use std::ops::Range;

use fe_text::Text;

use super::{
    Deadline, Engine, Interrupted, Pattern, RowMatches, Window, row_end, row_text, time_limit, unit,
};

/// A replacement template, parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replacement {
    parts: Vec<Part>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Literal(String),
    /// Group `n`; 0 is the whole match.
    Group(usize),
    Named(String),
}

impl Replacement {
    /// `template` as a replacement. With `regex` false it is literal text.
    /// With `regex` (VS Code's rules): `$0` and `$&` are the whole match,
    /// `$1`..`$99` a group (at most two digits, so `$1a` is group 1 then `a`;
    /// a group the pattern lacks expands to nothing), `${name}` a named
    /// group, `$$` a `$`, `\n` a line break, `\t` a tab, `\\` a backslash;
    /// anything else is literal.
    pub fn parse(template: &str, regex: bool) -> Self {
        if !regex {
            return Self {
                parts: vec![Part::Literal(template.to_string())],
            };
        }
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut chars = template.char_indices().peekable();
        while let Some((at, ch)) = chars.next() {
            let part = match (ch, chars.peek().map(|&(_, next)| next)) {
                ('$', Some('$')) => {
                    chars.next();
                    literal.push('$');
                    continue;
                }
                ('$', Some('&')) => {
                    chars.next();
                    Part::Group(0)
                }
                ('$', Some(digit)) if digit.is_ascii_digit() => {
                    let mut number = 0;
                    for _ in 0..2 {
                        match chars.peek() {
                            Some(&(_, digit)) if digit.is_ascii_digit() => {
                                number = number * 10 + digit as usize - '0' as usize;
                                chars.next();
                            }
                            _ => break,
                        }
                    }
                    Part::Group(number)
                }
                ('$', Some('{')) => {
                    let rest = &template[at + 2..];
                    match rest.find('}').filter(|&end| end > 0) {
                        Some(end) => {
                            let name = &rest[..end];
                            while chars.next_if(|&(i, _)| i < at + 2 + end + 1).is_some() {}
                            match name.parse::<usize>() {
                                Ok(number) => Part::Group(number),
                                Err(_) => Part::Named(name.to_string()),
                            }
                        }
                        None => {
                            literal.push('$');
                            continue;
                        }
                    }
                }
                ('\\', Some(escaped @ ('n' | 't' | '\\'))) => {
                    chars.next();
                    literal.push(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        _ => '\\',
                    });
                    continue;
                }
                _ => {
                    literal.push(ch);
                    continue;
                }
            };
            if !literal.is_empty() {
                parts.push(Part::Literal(std::mem::take(&mut literal)));
            }
            parts.push(part);
        }
        if !literal.is_empty() {
            parts.push(Part::Literal(literal));
        }
        Self { parts }
    }

    /// The text for the match `matches` has just found in `hay`.
    fn expand(&self, pattern: &Pattern, matches: &RowMatches<'_>, hay: &str) -> String {
        let mut out = String::new();
        for part in &self.parts {
            let group = match part {
                Part::Literal(text) => {
                    out.push_str(text);
                    continue;
                }
                Part::Group(number) => matches.group(*number),
                Part::Named(name) => match &pattern.engine {
                    Engine::Regex { regex, .. } => regex
                        .capture_names()
                        .position(|candidate| candidate == Some(name.as_str()))
                        .and_then(|number| matches.group(number)),
                    Engine::Backtrack(_) => None,
                },
            };
            if let Some(group) = group {
                out.push_str(&hay[group]);
            }
        }
        out
    }
}

impl Pattern {
    /// The row's matches with their groups, as [`super::matches_in`] finds them.
    fn grouped<'a>(&'a self, hay: &'a str, deadline: &'a Deadline) -> RowMatches<'a> {
        RowMatches {
            groups: true,
            ..self.matches(hay, deadline)
        }
    }

    /// The expansion of `with` if `range` is exactly a match this pattern
    /// finds ([`super::matches_in`] on its row, whole-word filter included),
    /// else `None`.
    pub fn replacement_at(
        &self,
        text: &Text,
        range: Range<usize>,
        with: &Replacement,
    ) -> Option<String> {
        if range.start > range.end || range.end > text.len() {
            return None;
        }
        let row = text.offset_to_point(range.start).row;
        let deadline = Deadline::new();
        let mut buf = String::new();
        let (start, hay) = row_text(text, row, &mut buf);
        if range.end > start + hay.len() {
            return None;
        }
        let mut matches = self.grouped(hay, &deadline);
        while let Some(found) = matches.next() {
            if start + found.start > range.start {
                break;
            }
            if start + found.start == range.start && start + found.end == range.end {
                return Some(with.expand(self, &matches, hay));
            }
        }
        None
    }
}

/// Every match in `text`, in order, with its expansion by `with`: row by row
/// exactly as [`super::matches_in`] finds them (empty matches once each).
/// Past [`time_limit`], counted from the call, `Err(Interrupted)`, never a
/// partial list.
pub fn replace_all(
    text: &Text,
    pattern: &Pattern,
    with: &Replacement,
) -> Result<Vec<(Range<usize>, String)>, Interrupted> {
    let deadline = Deadline::within(time_limit());
    let mut edits = Vec::new();
    let mut buf = String::new();
    for row in 0..text.row_count() {
        if deadline.expired() {
            return Err(Interrupted);
        }
        let (start, hay) = row_text(text, row, &mut buf);
        let mut matches = pattern.grouped(hay, &deadline);
        while let Some(found) = matches.next() {
            edits.push((
                start + found.start..start + found.end,
                with.expand(pattern, &matches, hay),
            ));
        }
        if deadline.expired() {
            return Err(Interrupted);
        }
    }
    Ok(edits)
}

/// A match `:s` takes over rows ([`Walk::next`]), as offsets in the text
/// the walk reads.
#[derive(Clone, Debug)]
pub(crate) struct Taken {
    /// The match. One that takes the line break nvim matches after the
    /// last row ends at the text's end, `past_end` saying so.
    pub(crate) range: Range<usize>,
    past_end: bool,
    /// Groups 0 to 9, as many as the [`Finder`] was asked for, each empty
    /// if it took no part.
    groups: Vec<String>,
    /// Which of the walk's lines (nvim's: one for each row a search starts
    /// afresh on) the match is on, counting from 0.
    pub(crate) pass: u32,
    /// The row searched, and the rows the match starts and ends on.
    searched: u32,
    pub(crate) start_row: u32,
    pub(crate) end_row: u32,
}

impl Taken {
    /// Group `n`'s text: empty if it took no part or wasn't asked for.
    pub(crate) fn group(&self, n: usize) -> &str {
        self.groups.get(n).map_or("", String::as_str)
    }

    /// Whether it ends on a later row than it starts on: replaced, it joins
    /// them.
    pub(crate) fn joins(&self) -> bool {
        self.end_row > self.start_row
    }
}

/// What `:s` did with the match [`Walk::next`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Took {
    /// Replaced it.
    Replaced,
    /// Counted it (the `n` flag) or was told no (the `c` flag's `n`).
    Passed,
}

/// The searches of a `:s` over rows for a pattern that
/// [`Pattern::spans_rows`], as nvim's `:s` makes them: each row of the
/// range from its start, and in a row, each search from where the last
/// match ended, to every match with `g` or only the first. A match may end
/// on a later row, past the range's last too; replaced, it joins its rows,
/// and the next search goes on in its last row even without `g` (in
/// nvim's words, again after joining), if that row is in the range, else
/// it is the last. Counted or told no (`n`), it ends its row's searches,
/// the next starting on the next row. An empty match where the last ended
/// is skipped, the search going on a whole character later, ending the row
/// without `g`. A match that `\zs` starts on a later row is taken, and the
/// next row searched afresh is the one after the row searched. No match is
/// taken that starts before the last replaced ended.
///
/// The walk is plain positions in the text: a caller that replaces a match
/// before searching on tells it so ([`Walk::edited`]); one that replaces
/// them all at the end reads one text throughout.
#[derive(Clone, Debug)]
pub(crate) struct Walk {
    /// The range's last row.
    last: u32,
    global: bool,
    /// The next search, `None` once there is none.
    next: Option<Search>,
    /// Where the last match replaced ended.
    guard: usize,
    /// How many lines the walk has started searching.
    passes: u32,
}

#[derive(Clone, Copy, Debug)]
struct Search {
    /// The row searched, and where in the text the search starts.
    row: u32,
    from: usize,
    /// A row's first search, from its start, or one after a match:
    /// `after` holds whether it starts where that match ended, where an
    /// empty match is skipped.
    first: bool,
    after: bool,
}

impl Walk {
    /// A walk over `rows` of `text`, to every match on a row with `global`.
    pub(crate) fn new(text: &Text, rows: Range<u32>, global: bool) -> Self {
        let rows = rows.start..rows.end.min(text.row_count());
        let next = (!rows.is_empty()).then(|| Search::row(text, rows.start));
        Self {
            last: rows.end.saturating_sub(1),
            global,
            next,
            guard: 0,
            passes: 0,
        }
    }

    /// The next match, `None` once the walk is over; `Err` once the
    /// finder's deadline has passed. Each one found must be told what
    /// became of it ([`Walk::took`]) before the next is asked for.
    pub(crate) fn next(&mut self, finder: &mut Finder<'_>) -> Result<Option<Taken>, Interrupted> {
        let text = finder.window.text;
        loop {
            let Some(search) = self.next else {
                return Ok(None);
            };
            // After a match `\zs` started on a later row, the next row
            // searched may hold it: what it replaced isn't searched again.
            let from = search.from.max(self.guard);
            let row = text
                .offset_to_point(from.min(text.len()))
                .row
                .max(search.row);
            if search.first && row > self.last {
                self.next = None;
                return Ok(None);
            }
            let Some((range, groups)) = finder.search(row, from)? else {
                let after = match search.first {
                    true => finder.missed.max(row + 1),
                    false => row + 1,
                };
                self.next = (after < text.row_count()).then(|| Search::row(text, after));
                continue;
            };
            // A match `\zs` starts past the last row's line break ends the
            // walk, as in nvim.
            if range.start > text.len() {
                self.next = None;
                return Ok(None);
            }
            if search.after && range.is_empty() && range.start == from {
                let lastone = range.start == row_end(text, row) || !self.global || row > self.last;
                self.next = match lastone {
                    true => (row + 1 < text.row_count()).then(|| Search::row(text, row + 1)),
                    false => Some(Search {
                        row,
                        from: finder.after(range.start),
                        first: false,
                        after: false,
                    }),
                };
                continue;
            }
            if search.first {
                self.passes += 1;
            }
            let row_of = |at: usize| text.offset_to_point(at.min(text.len())).row;
            return Ok(Some(Taken {
                past_end: range.end > text.len(),
                range: range.start..range.end.min(text.len()),
                groups,
                pass: self.passes - 1,
                searched: row,
                start_row: row_of(range.start),
                end_row: row_of(range.end),
            }));
        }
    }

    /// Goes on from `taken`, which `:s` replaced or passed (as positions in
    /// `text`, before any replacement).
    pub(crate) fn took(&mut self, text: &Text, taken: &Taken, took: Took) {
        let mut again = false;
        let mut lastone = taken.past_end;
        let mut row = taken.start_row;
        match took {
            // A match over rows ends the row's searches.
            Took::Passed => lastone |= taken.joins(),
            Took::Replaced => {
                self.guard = taken.range.end;
                if taken.joins() {
                    row = taken.end_row;
                    match taken.end_row <= self.last {
                        true => again = true,
                        // Past the range's end, no more.
                        false => self.global = false,
                    }
                }
            }
        }
        lastone |= taken.start_row > self.last || !(self.global || again);
        if !lastone {
            self.next = Some(Search {
                row,
                from: taken.range.end,
                first: false,
                after: true,
            });
            return;
        }
        // nvim goes back to the row it searched for a match `\zs` started
        // on a later row.
        let next = match taken.start_row > taken.searched {
            true => taken.searched + 1,
            false => row + 1,
        };
        self.next = (next < text.row_count()).then(|| Search::row(text, next));
    }

    /// Takes in `range` of `before` replaced by `new`, making `after`: what
    /// is left to search moves with the text.
    pub(crate) fn edited(&mut self, before: &Text, range: Range<usize>, new: &str, after: &Text) {
        let offset = |at: usize| match at {
            _ if at >= range.end => at - range.len() + new.len(),
            _ if at <= range.start => at,
            _ => range.start + new.len(),
        };
        self.last = after
            .offset_to_point(offset(row_end(before, self.last)))
            .row;
        self.guard = offset(self.guard);
        if let Some(search) = &mut self.next {
            search.from = offset(search.from);
            search.row = after.offset_to_point(search.from).row;
        }
    }
}

impl Search {
    /// A search from `row`'s start.
    fn row(text: &Text, row: u32) -> Self {
        Self {
            row,
            from: text.row_start(row),
            first: true,
            after: false,
        }
    }
}

/// A match [`Finder::search`] found, and its groups' text.
type Hit = (Range<usize>, Vec<String>);

/// What a [`Walk`] reads its matches with: `text` read a window of rows at
/// a time, as [`Pattern::find_across`] reads it, under `deadline`.
pub(crate) struct Finder<'a> {
    pattern: &'a Pattern,
    window: Window<'a>,
    deadline: &'a Deadline,
    /// How many groups a match gives, from group 0.
    groups: usize,
    /// After a search that found nothing, the first row on which a match
    /// may start, as far as the window could tell.
    missed: u32,
}

impl<'a> Finder<'a> {
    /// A finder of `pattern`'s matches in `text` with `groups` groups each
    /// (at most 10: the whole match and `\1` to `\9`).
    pub(crate) fn new(
        pattern: &'a Pattern,
        text: &'a Text,
        deadline: &'a Deadline,
        groups: usize,
    ) -> Self {
        Self {
            pattern,
            window: Window::new(text, pattern.reach),
            deadline,
            groups: groups.min(10),
            missed: 0,
        }
    }

    /// The first match starting on `row` at or after `from`, or that `\zs`
    /// starts on a later row, with its groups' text; with none, `None`,
    /// [`Finder::missed`] set.
    fn search(&mut self, row: u32, from: usize) -> Result<Option<Hit>, Interrupted> {
        let window = &mut self.window;
        window.hold(row, self.deadline)?;
        let (text, base) = (window.text, window.base);
        let mut matches = self
            .pattern
            .matches_from(&window.hay, from - base, self.deadline);
        matches.row_end = row_end(text, row) - base;
        matches.across = true;
        matches.groups = self.groups > 0;
        matches.emitted = from - base;
        let found = matches.next();
        self.deadline.check()?;
        let Some(found) = found else {
            self.missed = match matches.beyond {
                Some(beyond) => window.row_of(base + beyond).min(window.told_through() + 1),
                None => window.told_through() + 1,
            };
            return Ok(None);
        };
        let groups = (0..self.groups)
            .map(|n| {
                matches
                    .group(n)
                    .map_or_else(String::new, |group| window.hay[group].to_string())
            })
            .collect();
        Ok(Some((base + found.start..base + found.end, groups)))
    }

    /// Where a search one whole character on from `at` starts.
    fn after(&self, at: usize) -> usize {
        let hay = &self.window.hay[at - self.window.base..];
        at + unit(hay).unwrap_or(1)
    }
}
