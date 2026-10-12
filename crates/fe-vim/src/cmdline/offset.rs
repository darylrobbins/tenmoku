//! Search offsets (`:help search-offset`): reading one from a search line,
//! and where a search with one lands.
//!
//! What nvim does, by probing: a character offset (`e+N`, `s-N`) is first
//! taken back from the caret, N characters the other way, and the search
//! goes from there, so `n` doesn't find the match it's on again; stepping
//! off either end of the text there searches from before its start (or
//! after its end), taking a match at either end. An `e` offset compares
//! matches by their last character, not their first: the first match whose
//! last character is after that place, or the last whose last character is
//! before it; an empty match compares as the character before it on its row
//! (as itself at the row's start), though the offset counts from the match
//! itself. A line offset searches from the caret as a plain search does.
//! Past the first match a count goes on match by match, with an `e` offset
//! each search from where the last match lands. `n` and `N` that would stay
//! on the caret without going round an end of the text go one match further
//! (but not with a line offset). Then the offset moves from the match:
//! characters step across rows (an empty row is one, and from a match at a
//! row's end the next row's first character is the first step), stopping
//! at the text's ends (in a visual mode, past the last character); rows
//! clamp to the text, at column 0.

use std::{collections::HashMap, ops::Range};

use fe_text::{Bias, Text};

use crate::{
    Search, SearchOffset,
    motion::{Cursor, Line},
    search::{Deadline, Interrupted, Pattern, RowList, starts_from},
};

/// The offset `text` (what follows a search's closing delimiter) gives:
/// `None` for one fe-vim doesn't take, a `;` chained search among them.
/// Empty text is no offset.
pub(crate) fn parse(text: &str) -> Option<SearchOffset> {
    let (kind, number): (fn(i32) -> SearchOffset, &str) = match text.chars().next() {
        None => return Some(SearchOffset::None),
        Some('e') => (SearchOffset::End, &text[1..]),
        Some('s' | 'b') => (SearchOffset::Start, &text[1..]),
        Some(_) => (SearchOffset::Lines, text),
    };
    let (negative, digits) = match number.as_bytes().first() {
        Some(b'+') => (false, &number[1..]),
        Some(b'-') => (true, &number[1..]),
        _ => (false, number),
    };
    let n = match digits {
        // A bare sign is 1; nothing at all after `e`, `s` or `b` is 0.
        "" if digits.len() < number.len() => 1,
        "" => 0,
        _ if digits.bytes().all(|byte| byte.is_ascii_digit()) => digits.parse::<i32>().ok()?,
        _ => return None,
    };
    Some(kind(if negative { -n } else { n }))
}

/// Moves `cursor` to where `count` searches for `search` from `from` land,
/// with its offset; false (the cursor where it was) if nothing matches or
/// the search ran out of time. `n` and `N` that would leave the cursor on
/// `from` without going round an end of the text go one match further, as
/// nvim's do (an offset cut short at an end of the text can leave them
/// there); with a line offset they stay. One deadline,
/// [`time_limit`](crate::search::time_limit) from the call, covers every
/// search it makes.
pub(crate) fn land(
    cursor: &mut Cursor,
    pattern: &Pattern,
    from: usize,
    search: Search,
    count: usize,
) -> bool {
    let deadline = Deadline::new();
    if !place(cursor, pattern, from, search, count, &deadline) {
        return false;
    }
    let lines = matches!(search.offset, SearchOffset::Lines(_));
    if search.again && !lines && cursor.offset() == from {
        let wrapped = nth_match(cursor.text, pattern, from, search, count, &deadline)
            .is_none_or(|(_, wrapped)| wrapped);
        if !wrapped {
            return place(cursor, pattern, from, search, count + 1, &deadline);
        }
    }
    true
}

/// [`land`] without going on past `from`.
fn place(
    cursor: &mut Cursor,
    pattern: &Pattern,
    from: usize,
    search: Search,
    count: usize,
    deadline: &Deadline,
) -> bool {
    let text = cursor.text;
    let forward = search.forward;
    let base = match search.offset {
        SearchOffset::End(_) => nth_match(text, pattern, from, search, count, deadline)
            .map(|(found, _)| last_char(text, found)),
        SearchOffset::Start(n) => by_start(text, pattern, from, forward, n, count, deadline),
        SearchOffset::None | SearchOffset::Lines(_) => {
            by_start(text, pattern, from, forward, 0, count, deadline)
        }
    };
    let Some(base) = base else {
        return false;
    };
    match search.offset {
        SearchOffset::Lines(n) => {
            let row = i64::from(text.offset_to_point(base).row) + i64::from(n);
            let last = i64::from(text.row_count() - 1);
            cursor.enter(row.clamp(0, last) as u32);
            cursor.ix = 0;
        }
        SearchOffset::End(n) | SearchOffset::Start(n) => step(cursor, base, n),
        SearchOffset::None => step(cursor, base, 0),
    }
    true
}

/// Where the match `count` searches find from `from` starts, with `n`
/// characters taken back first.
fn by_start(
    text: &Text,
    pattern: &Pattern,
    from: usize,
    forward: bool,
    n: i32,
    count: usize,
    deadline: &Deadline,
) -> Option<usize> {
    let first = match taken_back(text, from, n) {
        Some(at) => return pattern.nth_by(text, at, forward, count, deadline),
        // From off the start or the end, the search takes the text's first
        // match going forward and its last going back.
        None if forward => pattern.forward(text, 0, true, deadline).ok()??.start,
        None => pattern.try_find_by(text, 0, false, deadline).ok()??.start,
    };
    match count {
        0 | 1 => Some(first),
        _ => pattern.nth_by(text, first, forward, count - 1, deadline),
    }
}

/// The match `count` searches for `search` find from `from` (its character
/// offset taken back first), and whether any of them went round an end of
/// the text. Each search after the first goes on from the last match, as
/// nvim's do: forward, to the next match a row's search finds (with an `e`
/// offset, an empty match right after the last one counts, though a search
/// from that place, `n` say, would pass it); back, from where the last one
/// landed (with an `e` offset, its last character, so an empty match finds
/// itself again). Once a match comes back the rest
/// of the count goes round, not searched, so the work is bounded by the
/// text's matches and rows, not the count, and each row's matches are
/// listed only as far as a step needs. `None` if nothing matches, or past
/// `deadline`.
pub(crate) fn nth_match(
    text: &Text,
    pattern: &Pattern,
    from: usize,
    search: Search,
    count: usize,
    deadline: &Deadline,
) -> Option<(Range<usize>, bool)> {
    let (by_end, n) = match search.offset {
        SearchOffset::End(n) => (true, n),
        SearchOffset::Start(n) => (false, n),
        SearchOffset::None | SearchOffset::Lines(_) => (false, 0),
    };
    let mut walk = Walk {
        text,
        pattern,
        forward: search.forward,
        by_end,
        deadline,
        rows: HashMap::new(),
        lines: HashMap::new(),
    };
    let (first, mut wrapped) = match taken_back(text, from, n) {
        Some(at) => walk.next(at).ok()??,
        // Off the start going forward, or the end going back, the first
        // search takes the text's first or last match without going round;
        // the other way it goes round.
        None => (walk.outermost().ok()??, search.forward != (n > 0)),
    };
    let mut seen = HashMap::from([(first.start, 0)]);
    let mut found = vec![first];
    for step in 1..count {
        if walk.deadline.expired() {
            return None;
        }
        let last = found.last().expect("one match was found").clone();
        let next = match search.forward {
            true => walk.after(&last),
            false => {
                let origin = walk.landed(last);
                walk.next(origin)
            }
        };
        let (next, crossed) = next.ok()??;
        if walk.deadline.expired() {
            return None;
        }
        wrapped |= crossed;
        if let Some(&first) = seen.get(&next.start) {
            let cycle = &found[first..];
            return Some((cycle[(count - 1 - first) % cycle.len()].clone(), wrapped));
        }
        seen.insert(next.start, step);
        found.push(next);
    }
    Some((found.pop()?, wrapped))
}

/// A search's matches row by row, each row read once and its matches listed
/// as far as a step has needed, for going from match to match.
struct Walk<'a> {
    text: &'a Text,
    pattern: &'a Pattern,
    forward: bool,
    /// Whether matches compare by their last character (an `e` offset) or
    /// their start.
    by_end: bool,
    deadline: &'a Deadline,
    rows: HashMap<u32, RowList<'a>>,
    lines: HashMap<u32, Line>,
}

impl<'a> Walk<'a> {
    /// `row`'s matches, listed until `enough` holds of them or the row has
    /// none left ([`RowList::list_until`]), under the walk's one deadline:
    /// `Err` once that has passed, the clock read for every row, listed
    /// already or not.
    fn matches(
        &mut self,
        row: u32,
        enough: impl Fn(&[Range<usize>], usize) -> bool,
    ) -> Result<&[Range<usize>], Interrupted> {
        let text = self.text;
        self.rows
            .entry(row)
            .or_insert_with(|| RowList::new(text, row))
            .list_until(self.pattern, self.deadline, enough)
    }

    /// `row`'s first match, if it has one.
    fn first(&mut self, row: u32) -> Result<Option<Range<usize>>, Interrupted> {
        Ok(self
            .matches(row, |found, _| !found.is_empty())?
            .first()
            .cloned())
    }

    /// `row`'s last match, if it has one: all of them listed.
    fn last(&mut self, row: u32) -> Result<Option<Range<usize>>, Interrupted> {
        Ok(self.matches(row, |_, _| false)?.last().cloned())
    }

    fn line(&mut self, row: u32) -> &Line {
        let text = self.text;
        self.lines
            .entry(row)
            .or_insert_with(|| Line::new(text, row))
    }

    /// Where the next search goes from after `found`: its start, or with an
    /// `e` offset its last character (an empty match itself).
    fn landed(&mut self, found: Range<usize>) -> usize {
        if !self.by_end || found.is_empty() {
            return found.start;
        }
        let line = self.line(self.text.offset_to_point(found.start).row);
        let column = found.end - 1 - line.start;
        let ix = line.starts.partition_point(|&start| start <= column);
        line.start + line.starts[ix.saturating_sub(1)]
    }

    /// The text's first match (going forward) or its last.
    fn outermost(&mut self) -> Result<Option<Range<usize>>, Interrupted> {
        let rows = self.text.row_count();
        if self.forward {
            for row in 0..rows {
                if let Some(found) = self.first(row)? {
                    return Ok(Some(found));
                }
            }
        } else {
            for row in (0..rows).rev() {
                if let Some(found) = self.last(row)? {
                    return Ok(Some(found));
                }
            }
        }
        Ok(None)
    }

    /// The next match from `origin`, and whether the search went round an
    /// end of the text for it: going forward, the first whose start (or
    /// last character) is after the character at `origin`, and from a
    /// row's last character the next row's first; going back, the last
    /// whose start (or last character) is before it. An empty match's last
    /// character is the one before it on its row, or itself at the row's
    /// start. Either way a match qualifies going forward if it ends past the
    /// start of the character after `origin`, and going back if it ends at
    /// or before `origin` (past the row's start).
    fn next(&mut self, origin: usize) -> Result<Option<(Range<usize>, bool)>, Interrupted> {
        let (by_end, rows) = (self.by_end, self.text.row_count());
        let row = self.text.offset_to_point(origin).row;
        let line = self.line(row);
        let (start, end) = (line.start, line.start + line.text.len());
        let after = line.starts.partition_point(|&at| at <= origin - start);
        let next = start + line.starts.get(after).copied().unwrap_or(end - start);
        // A row is listed only as far as the match wanted: going forward,
        // until it is listed; going back, until no match left to list can
        // come before `origin`, by its start (or by its end, which is at
        // or after its start).
        let ends_past =
            |found: &[Range<usize>], at| found.last().is_some_and(|found| found.end > at);
        if self.forward {
            if next < end {
                let found = match by_end {
                    true => self.matches(row, |found, _| ends_past(found, next))?,
                    false => self.matches(row, |found, _| starts_from(found, next))?,
                };
                let ix = match by_end {
                    true => found.partition_point(|found| found.end <= next),
                    false => found.partition_point(|found| found.start < next),
                };
                if let Some(found) = found.get(ix) {
                    return Ok(Some((found.clone(), false)));
                }
            }
            return self.first_after(row);
        }
        let (found, ix) = match by_end {
            true if origin > start => {
                let found = self.matches(row, |found, unlisted| {
                    unlisted > origin || ends_past(found, origin)
                })?;
                (found, found.partition_point(|found| found.end <= origin))
            }
            true => (self.matches(row, |_, _| true)?, 0),
            false => {
                let found = self.matches(row, |_, unlisted| unlisted >= origin)?;
                (found, found.partition_point(|found| found.start < origin))
            }
        };
        if let Some(ix) = ix.checked_sub(1) {
            return Ok(Some((found[ix].clone(), false)));
        }
        let earlier = (0..row).rev().map(|row| (row, false));
        for (row, wrapped) in earlier.chain((row..rows).rev().map(|row| (row, true))) {
            if let Some(found) = self.last(row)? {
                return Ok(Some((found, wrapped)));
            }
        }
        Ok(None)
    }

    /// The match after `found` (one of a row's matches) as a row's search
    /// finds them going forward: the next on its row, else the first on a
    /// later row, round the text.
    fn after(&mut self, found: &Range<usize>) -> Result<Option<(Range<usize>, bool)>, Interrupted> {
        let (row, at) = (self.text.offset_to_point(found.start).row, found.start);
        let matches = self.matches(row, |found, _| {
            found.last().is_some_and(|last| last.start > at)
        })?;
        let ix = matches.partition_point(|next| next.start <= at);
        if let Some(next) = matches.get(ix) {
            return Ok(Some((next.clone(), false)));
        }
        self.first_after(row)
    }

    /// The first match on a row after `row`, round the text to `row` itself,
    /// and whether it went round.
    fn first_after(&mut self, row: u32) -> Result<Option<(Range<usize>, bool)>, Interrupted> {
        let rows = self.text.row_count();
        let later = (row + 1..rows).map(|row| (row, false));
        for (row, wrapped) in later.chain((0..=row).map(|row| (row, true))) {
            if let Some(found) = self.first(row)? {
                return Ok(Some((found, wrapped)));
            }
        }
        Ok(None)
    }
}

/// `from` with `n` characters taken back, `None` if that steps off either
/// end of the text.
fn taken_back(text: &Text, from: usize, n: i32) -> Option<usize> {
    if n == 0 {
        return Some(from);
    }
    let mut cursor = Cursor::at(text, from);
    onto(&mut cursor, from);
    for _ in 0..n.unsigned_abs() {
        let moved = match n > 0 {
            true => cursor.backward(),
            false => cursor.forward(),
        };
        if !moved {
            return None;
        }
    }
    Some(cursor.offset())
}

/// Where an `e` offset counts from: a match's last character, or an empty
/// match itself.
fn last_char(text: &Text, found: Range<usize>) -> usize {
    match found.is_empty() {
        true => found.start,
        false => Cursor::at(text, text.clip_offset(found.end - 1, Bias::Left)).offset(),
    }
}

/// Moves `cursor` to `at`, a row's end if `at` is one.
fn onto(cursor: &mut Cursor, at: usize) {
    cursor.goto(at);
    if at >= cursor.line.start + cursor.line.text.len() && cursor.line.len() > 0 {
        cursor.ix = cursor.line.len();
    }
}

/// Moves `cursor` to `at` (a row's end if a match is there), then `n`
/// characters on (back if negative), as far as the text goes, and onto a
/// character; in a visual mode, a step on from the text's last character
/// and a row's end stay there, as nvim's cursor may.
fn step(cursor: &mut Cursor, at: usize, n: i32) {
    onto(cursor, at);
    for _ in 0..n.unsigned_abs() {
        let moved = match n > 0 {
            true => cursor.forward(),
            false => cursor.backward(),
        };
        if !moved {
            if n > 0 && cursor.visual {
                cursor.ix = cursor.line.len();
            }
            break;
        }
    }
    if !cursor.visual {
        cursor.ix = cursor.ix.min(cursor.line.last());
    }
}

#[cfg(test)]
#[path = "offset_tests.rs"]
mod tests;
