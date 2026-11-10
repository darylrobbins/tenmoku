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
//!
//! [`vim_command`] writes a find bar's query, and its replacement, as the
//! `/` or `:%s` that finds the same matches and makes the same text, where
//! Vim's syntax can say it exactly.

use std::ops::Range;

use fe_text::Text;

use super::{
    Deadline, Engine, FindQuery, Interrupted, Pattern, RowMatches, Window, row_end, row_text,
    time_limit, unit,
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

    /// It in `:s`'s replacement syntax ('magic'), for between the second
    /// `/` and the third: `&` the whole match, `\1`..`\9` a group, `\r` a
    /// line break, and `\`, `&`, `~` and `/` escaped where they're literal.
    /// `None` with a named group or one past 9, which `:s` can't name.
    pub fn to_vim(&self) -> Option<String> {
        let mut out = String::new();
        for part in &self.parts {
            match *part {
                Part::Literal(ref text) => {
                    let mut chars = text.chars().peekable();
                    while let Some(ch) = chars.next() {
                        match ch {
                            '\\' | '&' | '~' | '/' => {
                                out.push('\\');
                                out.push(ch);
                            }
                            // A line break, as the find bar makes `\r\n` and
                            // a lone `\r` one.
                            '\r' | '\n' => {
                                if ch == '\r' {
                                    chars.next_if_eq(&'\n');
                                }
                                out.push_str("\\r");
                            }
                            ch => out.push(ch),
                        }
                    }
                }
                Part::Group(0) => out.push('&'),
                Part::Group(number @ 1..=9) => {
                    out.push('\\');
                    out.push(char::from_digit(number as u32, 10)?);
                }
                Part::Group(_) | Part::Named(_) => return None,
            }
        }
        Some(out)
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
        // No match past `range.start` is wanted, so none past it is looked
        // for: rejecting candidates there (not whole words, say) could
        // take the rest of a long row.
        let mut matches = self.grouped(hay, &deadline).up_to(range.start - start);
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

/// [`replace_all`] on `rows` alone, each with the part of it that matters: a
/// row is searched from its start, whole for its context, and only until a
/// match starts past that part, and of its matches
/// only those `keep` takes are expanded. Past [`time_limit`], counted from the
/// call, `Err(Interrupted)`, never a partial list.
pub fn replace_where(
    text: &Text,
    rows: impl IntoIterator<Item = (u32, Range<usize>)>,
    pattern: &Pattern,
    with: &Replacement,
    keep: impl Fn(&Range<usize>) -> bool,
) -> Result<Vec<(Range<usize>, String)>, Interrupted> {
    let deadline = Deadline::within(time_limit());
    let mut edits = Vec::new();
    let mut buf = String::new();
    for (row, window) in rows {
        if deadline.expired() {
            return Err(Interrupted);
        }
        let (start, hay) = row_text(text, row, &mut buf);
        let mut matches = pattern
            .grouped(hay, &deadline)
            .up_to(window.end.saturating_sub(start));
        while let Some(found) = matches.next() {
            let found = start + found.start..start + found.end;
            if found.start > window.end {
                break;
            }
            if keep(&found) {
                let new = with.expand(pattern, &matches, hay);
                edits.push((found, new));
            }
        }
        if deadline.expired() {
            return Err(Interrupted);
        }
    }
    Ok(edits)
}

/// The Vim command that does what a find bar does with `query`: with no
/// `with`, the `/` that finds its matches (`/dirty`); with `with`, the `:%s`
/// that replaces every one as Replace All does (`:%s/dirty/needs_paint/gI`).
/// Match Case is `:s`'s `I` or `i`, or for `/` a `\C` or `\c` where
/// `ignores_case(pattern)` (Vim's 'ignorecase' and 'smartcase' for that
/// pattern) would otherwise get it wrong. `None` if the query is empty or
/// doesn't compile, if it can match nothing (`x?`, `^`), where `:s` and
/// Replace All differ, or where Vim's syntax has no exact equivalent
/// ([`vim_pattern`], [`Replacement::to_vim`]).
pub fn vim_command(
    query: &FindQuery,
    with: Option<&Replacement>,
    ignores_case: impl FnOnce(&str) -> bool,
) -> Option<String> {
    // After a match `:s` takes no empty one where it ends, as Replace All
    // does, so no pattern that can match nothing is the same.
    let Engine::Regex { regex, .. } = Pattern::from_query(query).ok()?.engine else {
        return None;
    };
    if regex.is_match("") {
        return None;
    }
    let pattern = vim_pattern(query)?;
    if let Some(with) = with {
        let with = with.to_vim()?;
        let case = if query.match_case { 'I' } else { 'i' };
        return Some(format!(":%s/{pattern}/{with}/g{case}"));
    }
    let case = match (query.match_case, ignores_case(&pattern)) {
        (true, true) => "\\C",
        (false, false) => "\\c",
        _ => "",
    };
    Some(format!("/{pattern}{case}"))
}

/// `query`'s text as a Vim pattern for between `/`s, matching what
/// [`Pattern::from_query`] makes of it but for case, which
/// [`vim_command`] adds. Literal text stays as it is unless it holds one of
/// 'magic''s special characters, and then goes after `\V`; a regex that is
/// more than letters, digits, `_` and spaces is written very magic (`\v`).
/// Whole Word puts a literal between `\<` and `\>`, which agree with the
/// find bar's check (no word character either side) where the query starts
/// and ends with a word character and the text beside a match is ASCII.
///
/// `None` where there's no exact equivalent: a line break in the text;
/// Whole Word on a regex, or on literal text starting or ending with
/// something other than a word character; and in a regex, the classes and
/// escapes whose meaning differs (`\d`, `\w`, `\s` and `\b` are Unicode in
/// the regex crate, ASCII in Vim), flags and named groups, nested classes
/// and set operations, `^` or `$` other than at a branch's ends, and
/// escapes other than `\t` and escaped punctuation.
pub fn vim_pattern(query: &FindQuery) -> Option<String> {
    let text = &query.text;
    if text.is_empty() || text.contains(['\n', '\r']) {
        return None;
    }
    if !query.regex {
        let mut out = String::new();
        if text.contains(['\\', '.', '*', '[', '~', '^', '$']) {
            out.push_str("\\V");
        }
        if query.whole_word {
            let word =
                |ch: Option<char>| ch.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
            if !word(text.chars().next()) || !word(text.chars().next_back()) {
                return None;
            }
            out.push_str("\\<");
        }
        for ch in text.chars() {
            if matches!(ch, '\\' | '/') {
                out.push('\\');
            }
            out.push(ch);
        }
        if query.whole_word {
            out.push_str("\\>");
        }
        return Some(out);
    }
    if query.whole_word {
        return None;
    }
    let plain = |ch: char| ch.is_alphanumeric() || ch == '_' || ch == ' ';
    if text.chars().all(plain) {
        return Some(text.clone());
    }
    regex_to_vim(&text.chars().collect::<Vec<_>>())
}

/// A `regex`-crate pattern, `source`, in Vim's very magic syntax: see
/// [`vim_pattern`].
fn regex_to_vim(source: &[char]) -> Option<String> {
    let mut out = String::from("\\v");
    // Where `^` is the row's start in Vim too: a branch's start.
    let mut branch_start = true;
    let mut at = 0;
    while let Some(&ch) = source.get(at) {
        at += 1;
        let starts_branch = matches!(ch, '|' | '(');
        match ch {
            '\\' => {
                let escaped = *source.get(at)?;
                at += 1;
                match escaped {
                    't' => out.push_str("\\t"),
                    // `\<` and `\>` are word boundaries in the regex crate.
                    '<' | '>' | '_' => return None,
                    escaped if escaped.is_ascii_punctuation() => {
                        out.push('\\');
                        out.push(escaped);
                    }
                    _ => return None,
                }
            }
            '.' | '|' | ')' => out.push(ch),
            '(' => match source.get(at) {
                Some('?') if source.get(at + 1) == Some(&':') => {
                    at += 2;
                    out.push_str("%(");
                }
                Some('?') => return None,
                _ => out.push('('),
            },
            '^' if branch_start => out.push('^'),
            '$' if matches!(source.get(at), None | Some('|' | ')')) => out.push('$'),
            '^' | '$' => return None,
            '*' | '+' | '?' => {
                let lazy = source.get(at) == Some(&'?');
                at += usize::from(lazy);
                out.push_str(match (ch, lazy) {
                    (_, false) => match ch {
                        '*' => "*",
                        '+' => "+",
                        _ => "?",
                    },
                    ('*', true) => "{-}",
                    ('+', true) => "{-1,}",
                    _ => "{-,1}",
                });
            }
            '{' => {
                let close = at + source[at..].iter().position(|&ch| ch == '}')?;
                let bounds: String = source[at..close].iter().collect();
                let (low, high) = bounds.split_once(',').unwrap_or((&bounds, &bounds));
                let digits = |part: &str| part.chars().all(|ch| ch.is_ascii_digit());
                if low.is_empty() || !digits(low) || !digits(high) {
                    return None;
                }
                at = close + 1;
                let lazy = source.get(at) == Some(&'?');
                at += usize::from(lazy);
                out.push('{');
                if lazy {
                    out.push('-');
                }
                out.push_str(&bounds);
                out.push('}');
            }
            '[' => at = class_to_vim(source, at, &mut out)?,
            '\t' => out.push_str("\\t"),
            ch if ch.is_alphanumeric() || ch == '_' || ch == ' ' || !ch.is_ascii() => {
                if ch.is_control() {
                    return None;
                }
                out.push(ch);
            }
            // Literal in the regex crate, special or reserved in very magic.
            ch if ch.is_ascii_punctuation() => {
                out.push('\\');
                out.push(ch);
            }
            _ => return None,
        }
        branch_start = starts_branch;
    }
    Some(out)
}

/// The class starting after the `[` before `at` in `source`, written onto
/// `out` as a Vim collection; where `source` goes on after it.
fn class_to_vim(source: &[char], mut at: usize, out: &mut String) -> Option<usize> {
    out.push('[');
    if source.get(at) == Some(&'^') {
        at += 1;
        out.push('^');
    }
    // A `[` goes last, where no `:`, `=` or `.` can follow it and make it
    // one of Vim's `[:alpha:]`, `[=a=]` or `[.a.]`.
    let mut bracket = false;
    let mut first = true;
    loop {
        let ch = *source.get(at)?;
        if ch == ']' && !first {
            at += 1;
            break;
        }
        first = false;
        let low = class_char(source, &mut at)?;
        // `--` after a member is a set difference.
        if source.get(at..at + 2) == Some(&['-', '-']) {
            return None;
        }
        let range = source.get(at) == Some(&'-') && source.get(at + 1).is_some_and(|&ch| ch != ']');
        if range {
            at += 1;
            let high = class_char(source, &mut at)?;
            if low == '[' || high == '[' {
                return None;
            }
            push_member(low, out);
            out.push('-');
            push_member(high, out);
        } else if low == '[' {
            bracket = true;
        } else {
            push_member(low, out);
        }
    }
    if bracket {
        out.push('[');
    }
    out.push(']');
    Some(at)
}

/// The character a class member at `at` in `source` stands for, moving
/// `at` past it; `None` for what isn't one character (a nested class, a
/// set operation, `\d`) or isn't written the same in Vim.
fn class_char(source: &[char], at: &mut usize) -> Option<char> {
    let ch = *source.get(*at)?;
    *at += 1;
    match ch {
        '\\' => {
            let escaped = *source.get(*at)?;
            *at += 1;
            match escaped {
                't' => Some('\t'),
                '<' | '>' | '_' => None,
                escaped if escaped.is_ascii_punctuation() => Some(escaped),
                _ => None,
            }
        }
        '[' => None,
        '&' | '-' | '~' if source.get(*at) == Some(&ch) => None,
        ch if ch.is_control() && ch != '\t' => None,
        ch => Some(ch),
    }
}

/// `ch` as a member of a Vim collection.
fn push_member(ch: char, out: &mut String) {
    match ch {
        ']' | '^' | '-' | '\\' => {
            out.push('\\');
            out.push(ch);
        }
        '\t' => out.push_str("\\t"),
        ch => out.push(ch),
    }
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

    /// Takes in `range` of `before`, which ends before the match on the row
    /// searched or an earlier one, replaced by `new`, making `after`: the
    /// match moves with the text.
    pub(crate) fn edited(&mut self, before: &Text, range: Range<usize>, new: &str, after: &Text) {
        let offset = |at: usize| at - range.len() + new.len();
        let row = |at: usize| after.offset_to_point(at.min(after.len())).row;
        self.searched = row(offset(row_end(before, self.searched).max(range.end)));
        self.range = offset(self.range.start)..offset(self.range.end);
        self.start_row = row(self.range.start);
        self.end_row = row(self.range.end);
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
/// The walk is plain positions in the text it reads, which its caller keeps
/// as nvim's `:s` keeps its buffer: a line's replacements are put in once
/// nvim writes the line (after a join, before a match on a later row, and
/// before the next row is searched afresh: see `writing` in
/// [`Walk::next`]), and the walk is told so ([`Walk::edited`]).
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
    /// became of it ([`Walk::took`]) before the next is asked for. While
    /// `writing` (a line, as nvim's `:s` writes its replacements), `None`
    /// too before searching a row afresh: nvim writes the line first, and
    /// the search reads what it wrote.
    pub(crate) fn next(
        &mut self,
        finder: &mut Finder<'_>,
        writing: bool,
    ) -> Result<Option<Taken>, Interrupted> {
        let text = &finder.text().clone();
        loop {
            let Some(search) = self.next else {
                return Ok(None);
            };
            if writing && search.first {
                return Ok(None);
            }
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
    window: Window,
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
        let base = window.base;
        let mut matches = self
            .pattern
            .matches_from(&window.hay, from - base, self.deadline);
        matches.row_end = row_end(&window.text, row) - base;
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

    /// The text it reads.
    pub(crate) fn text(&self) -> &Text {
        &self.window.text
    }

    /// Replaces `range` of the text it reads with `new`, reading on in the
    /// text that makes.
    pub(crate) fn edit(&mut self, range: Range<usize>, new: &str) {
        self.window.edit(range, new);
    }

    /// Where a search one whole character on from `at` starts.
    fn after(&self, at: usize) -> usize {
        let hay = &self.window.hay[at - self.window.base..];
        at + unit(hay).unwrap_or(1)
    }
}

#[cfg(test)]
mod vim_command_tests {
    use fe_text::Text;

    use super::{FindQuery, Pattern, Replacement, replace_all, vim_command, vim_pattern};
    use crate::HeadlessVim;

    fn query(text: &str, regex: bool, match_case: bool, whole_word: bool) -> FindQuery {
        FindQuery {
            text: text.to_owned(),
            regex,
            match_case,
            whole_word,
        }
    }

    /// What Replace All makes of `text`, every match of `query` replaced by
    /// `with` (regex syntax with the query's regex).
    fn find_bar(text: &str, query: &FindQuery, with: &str) -> String {
        let pattern = Pattern::from_query(query).unwrap();
        let with = Replacement::parse(with, query.regex);
        let mut text = Text::new(text);
        let edits = replace_all(&text, &pattern, &with).unwrap();
        for (range, new) in edits.into_iter().rev() {
            text.replace(range, &new);
        }
        text.to_string()
    }

    /// What `command`, an ex command, makes of `text` in vim, with
    /// 'ignorecase' and 'smartcase'.
    fn vim(text: &str, command: &str, case: (bool, bool)) -> String {
        let mut vim = HeadlessVim::new(text, (0, 0));
        vim.set_search_case(case.0, case.1);
        let keys = command.replace('<', "<lt>");
        vim.feed(&format!("{keys}<CR>")).unwrap();
        let message = vim.state().message().unwrap_or_default();
        assert!(!message.starts_with('E'), "{command}: {message}");
        vim.text()
    }

    const TEXT: &str = "Dirty dirty DIRTY dirtyx\n\
        a.b axb a/b 1+1 11 [x] x ~ \\ ^a$ a^ a|b a? $\n\
        foo food xfoo foo_ foo-bar <b> =@%&~#!\"',;:`-?{}\n\
        aaa ab abab aXb tab\there\n\
        émoji été";

    /// Each query, with Replace All marking its matches, gives the same
    /// text as its `:%s` in vim, case on and off; and its `/` pattern,
    /// with vim's 'ignorecase' and 'smartcase' either way, finds the same
    /// matches.
    #[test]
    fn commands_find_what_the_find_bar_does() {
        let literal = [
            "dirty", "Dirty", "a.b", "a/b", "1+1", "[x]", "~", "\\", "^a$", "a^", "$", "tab\th",
            "<b>", "=@%&~#!", "été",
        ];
        let regex = [
            "dirt(y)", "d.r", "a|b", "^foo", "here$", "a+?", "a{2,3}", "a{2}", "a{1,}?", "[a-c]+",
            "[^a-z ]", "[]a]", "[a\\]-]", "[-x]", "[\\[x]", "\\.", "a\\.b", "(?:ab)+", "<b>",
            "=@%&~#!", "a/b", "\\t", "\\{\\}", "\\$", "a\\|b", "[\\^a]", "\\-", "é.", "(a)|b$",
            "ab*?", "a\\?",
        ];
        let cases = (literal.iter().map(|text| (*text, false)))
            .chain(regex.iter().map(|text| (*text, true)));
        for (text, regex) in cases {
            // A regex's marks hold the match; literal text's can't.
            let (marked, vim_marked) = match regex {
                true => ("<$0>", "<&>"),
                false => ("<", "<"),
            };
            let with = Replacement::parse(marked, regex);
            for match_case in [false, true] {
                let query = query(text, regex, match_case, false);
                let expected = find_bar(TEXT, &query, marked);
                assert_ne!(expected, TEXT, "{text:?} matches");
                let command = vim_command(&query, Some(&with), |_| unreachable!())
                    .unwrap_or_else(|| panic!("{text:?} has an equivalent"));
                assert_eq!(vim(TEXT, &command, (false, false)), expected, "{command}");
                for case in [(false, false), (true, false), (true, true)] {
                    let ignores = |pattern: &str| {
                        case.0 && (!case.1 || !crate::search::has_uppercase(pattern))
                    };
                    let search = vim_command(&query, None, ignores).unwrap();
                    let pattern = search.strip_prefix('/').unwrap();
                    let command = format!(":%s/{pattern}/{vim_marked}/g");
                    assert_eq!(
                        vim(TEXT, &command, case),
                        expected,
                        "{search} with {case:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_designs_examples() {
        let dirty = query("dirty", false, true, false);
        assert_eq!(
            vim_command(&dirty, None, |_| false).as_deref(),
            Some("/dirty")
        );
        let regex = query("dirty", true, true, false);
        let with = Replacement::parse("needs_paint", true);
        assert_eq!(
            vim_command(&regex, Some(&with), |_| unreachable!()).as_deref(),
            Some(":%s/dirty/needs_paint/gI")
        );
        // Case where 'ignorecase' (and 'smartcase') would get it wrong.
        assert_eq!(
            vim_command(&dirty, None, |_| true).as_deref(),
            Some("/dirty\\C")
        );
        let any_case = query("dirty", false, false, false);
        assert_eq!(
            vim_command(&any_case, None, |_| false).as_deref(),
            Some("/dirty\\c")
        );
        assert_eq!(
            vim_command(&any_case, None, |_| true).as_deref(),
            Some("/dirty")
        );
        assert_eq!(
            vim_command(&any_case, Some(&with), |_| unreachable!()).as_deref(),
            Some(":%s/dirty/needs_paint/gi")
        );
    }

    /// Replacements, literal and with groups, make the same text in vim.
    #[test]
    fn replacements_make_the_same_text() {
        let cases = [
            ("dirty", false, "a&b~c\\d/e$1"),
            ("dirty", false, "two\nrows"),
            ("(d)(i)rty", true, "$2-$1-$0-$&-$$-${1}x-$1a"),
            ("(d)irty", true, "\\n\\t\\\\&~/\\x"),
            ("(d)irty", true, "$9"),
        ];
        for (text, regex, with) in cases {
            let query = query(text, regex, true, false);
            let parsed = Replacement::parse(with, regex);
            let command = vim_command(&query, Some(&parsed), |_| unreachable!()).unwrap();
            assert_eq!(
                vim(TEXT, &command, (false, false)),
                find_bar(TEXT, &query, with),
                "{command}"
            );
        }
    }

    /// Whole Word on literal text between word characters is `\<` and `\>`.
    #[test]
    fn whole_words() {
        for text in ["foo", "foo_", "a", "a.b", "foo-bar"] {
            let query = query(text, false, true, true);
            let with = Replacement::parse("<", false);
            let command = vim_command(&query, Some(&with), |_| unreachable!()).unwrap();
            assert_eq!(
                vim(TEXT, &command, (false, false)),
                find_bar(TEXT, &query, "<"),
                "{command}"
            );
        }
        let foo = query("foo", false, true, true);
        assert_eq!(vim_pattern(&foo).as_deref(), Some("\\<foo\\>"));
        let dotted = query("a.b", false, true, true);
        assert_eq!(vim_pattern(&dotted).as_deref(), Some("\\V\\<a.b\\>"));
    }

    /// What Vim can't say exactly has no command.
    #[test]
    fn no_command_without_an_exact_equivalent() {
        let regexes = [
            "\\d",
            "\\w+",
            "\\s",
            "\\bx",
            "\\Bx",
            "(?i)a",
            "(?P<n>a)",
            "(?<n>a)",
            "[[:alpha:]]",
            "[a&&b]",
            "[a--b]",
            "[\\d]",
            "\\x41",
            "\\u{41}",
            "a^",
            "$a",
            "x?",
            "^",
            "a*",
            "\\<a",
            "a\\>",
            "\\Aa",
            "a\\z",
            "\\pL",
            "a{,2}",
            "\\_",
            "a|",
            "(",
            "",
        ];
        for text in regexes {
            let query = query(text, true, true, false);
            assert_eq!(vim_command(&query, None, |_| false), None, "{text:?}");
        }
        let cases = [
            query("a", true, true, true),
            query("-a", false, true, true),
            query("a-", false, true, true),
            query("é", false, true, true),
            query("a\nb", false, true, false),
            query("", false, true, false),
        ];
        for query in cases {
            assert_eq!(vim_command(&query, None, |_| false), None, "{query:?}");
        }
        let dirty = query("(d)irty", true, true, false);
        for with in ["${name}", "$10"] {
            let with = Replacement::parse(with, true);
            assert_eq!(
                vim_command(&dirty, Some(&with), |_| false),
                None,
                "{with:?}"
            );
        }
    }

    /// Whether a selection is a match to replace looks no further along
    /// its row than its start: rejecting what follows (not whole words)
    /// isn't gone through.
    #[test]
    fn replacement_at_stops_at_the_range() {
        let text = Text::new(&format!("x {}\nfoo\n", "a".repeat(5_000)));
        let query = FindQuery {
            text: "foo|a|x".to_owned(),
            regex: true,
            match_case: true,
            whole_word: true,
        };
        let pattern = Pattern::from_query(&query).unwrap();
        let with = Replacement::parse("y", true);
        let before = crate::search::regex_calls();
        assert_eq!(
            pattern.replacement_at(&text, 0..1, &with).as_deref(),
            Some("y")
        );
        assert_eq!(pattern.replacement_at(&text, 2..3, &with), None);
        let calls = crate::search::regex_calls() - before;
        assert!(calls < 10, "{calls} regex calls");
    }
}
