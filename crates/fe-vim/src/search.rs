//! Search patterns: Vim's pattern language, translated to a [`regex::Regex`]
//! and matched one row at a time. The crate docs list what translates.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use fe_text::{Bias, Text};
use regex::{CaptureLocations, Regex, RegexBuilder};
use unicode_segmentation::UnicodeSegmentation;

mod backtrack;
pub mod replace;

use crate::args::SearchPattern;
use crate::motion::{self, Class, Line};

/// How deep groups may nest, short of `regex`'s own nesting limit.
const MAX_DEPTH: usize = 60;
/// How many `\(` a pattern may hold, as in Vim.
const MAX_GROUPS: usize = 9;
/// How long one command may spend matching a pattern by default: nvim's
/// `'redrawtime'`.
pub const DEFAULT_TIME_LIMIT: Duration = Duration::from_secs(2);

/// The limit, in nanoseconds, for [`set_time_limit`] and [`time_limit`].
static TIME_LIMIT: AtomicU64 = AtomicU64::new(DEFAULT_TIME_LIMIT.as_nanos() as u64);

/// How many regex calls [`RowMatches`] has made, process-wide.
static REGEX_CALLS: AtomicU64 = AtomicU64::new(0);

/// How many regex calls searches have made so far in this process, each
/// finding one match or finding none more on a row. For tests to count the
/// work a search does instead of timing it: they take it before and after.
#[doc(hidden)]
pub fn regex_calls() -> u64 {
    REGEX_CALLS.load(Ordering::Relaxed)
}

/// The regex-call budget for [`set_call_limit`]; `u64::MAX` for none.
static CALL_LIMIT: AtomicU64 = AtomicU64::new(u64::MAX);

/// Gives each [`Deadline`] made from now on a budget of `limit` regex calls,
/// process-wide, or none with `None` (the default): it passes once that many
/// calls have been made since it started, as if its time had run out. For
/// tests that need an interruption at a counted point of the work instead of
/// a measured time; nothing else sets it.
#[doc(hidden)]
pub fn set_call_limit(limit: Option<u64>) {
    CALL_LIMIT.store(limit.unwrap_or(u64::MAX), Ordering::Relaxed);
}

/// Sets how long a `:s` or a search may spend matching before it stops with
/// `Interrupted`, process-wide: [`DEFAULT_TIME_LIMIT`] until a host or a test
/// changes it. Each command reads it once, when it starts, so a change
/// applies from the next command.
///
/// The limit is checked between regex calls, each of which scans at most
/// one row, so a command can overrun it by one row's scan. A command that
/// passes it stops: `:s` changes nothing ([`crate::substitute`]'s `run`)
/// and a search doesn't move the cursor ([`Pattern::try_find`]); both leave
/// the message `Interrupted`.
pub fn set_time_limit(limit: Duration) {
    let nanos = u64::try_from(limit.as_nanos()).unwrap_or(u64::MAX);
    TIME_LIMIT.store(nanos, Ordering::Relaxed);
}

/// The limit [`set_time_limit`] set.
pub fn time_limit() -> Duration {
    match TIME_LIMIT.load(Ordering::Relaxed) {
        // What a limit too long to count in nanoseconds is stored as.
        u64::MAX => Duration::MAX,
        nanos => Duration::from_nanos(nanos),
    }
}

/// A command that passed [`time_limit`] before it finished matching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interrupted;

impl std::fmt::Display for Interrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Interrupted")
    }
}

impl std::error::Error for Interrupted {}

/// When one operation, counted from its start, must stop matching, and
/// whether it had to. Wall-clock time: reading rows and building edits count
/// as well as the regex calls.
pub struct Deadline {
    /// `None` for one that never passes.
    end: Option<Instant>,
    /// The [`regex_calls`] count it passes at, from [`set_call_limit`].
    calls_end: Option<u64>,
    ran_out: std::cell::Cell<bool>,
}

impl Default for Deadline {
    fn default() -> Self {
        Self::new()
    }
}

impl Deadline {
    /// A deadline [`time_limit`] from now.
    pub fn new() -> Self {
        Self::within(time_limit())
    }

    /// A deadline `limit` from now.
    pub fn after(limit: Duration) -> Self {
        Self::within(limit)
    }

    /// A deadline that never passes.
    pub fn never() -> Self {
        Self {
            end: None,
            calls_end: None,
            ran_out: std::cell::Cell::new(false),
        }
    }

    /// A deadline `limit` from now: one that's past [`Instant`]'s range
    /// (`Duration::MAX`) never passes.
    pub(crate) fn within(limit: Duration) -> Self {
        let calls_end = match CALL_LIMIT.load(Ordering::Relaxed) {
            u64::MAX => None,
            calls => regex_calls().checked_add(calls),
        };
        Self {
            end: Instant::now().checked_add(limit),
            calls_end,
            ran_out: std::cell::Cell::new(false),
        }
    }

    /// Whether it is past, which it records.
    fn passed(&self) -> bool {
        if !self.ran_out.get()
            && (self.end.is_some_and(|end| Instant::now() >= end)
                || self.calls_end.is_some_and(|end| regex_calls() >= end))
        {
            self.ran_out.set(true);
        }
        self.ran_out.get()
    }

    /// Records that the operation had to stop, as an expired deadline does.
    pub(crate) fn give_up(&self) {
        self.ran_out.set(true);
    }

    /// Whether it is past now: samples the clock, for loops that make no
    /// regex call to do it (stepping through listed matches, reading rows).
    pub fn expired(&self) -> bool {
        self.passed()
    }

    /// Whether a match iteration was cut short by it.
    pub fn ran_out(&self) -> bool {
        self.ran_out.get()
    }

    /// `Err` if it ran out.
    pub(crate) fn check(&self) -> Result<(), Interrupted> {
        match self.ran_out() {
            true => Err(Interrupted),
            false => Ok(()),
        }
    }
}

/// A search pattern in Vim's syntax, translated to a [`Regex`] that matches
/// within one row.
#[derive(Clone, Debug)]
pub struct Pattern {
    engine: Engine,
    /// Whether the pattern begins with `\<` and ends with `\>` (and has no
    /// other), which are checked on each match by character class, as
    /// Vim does, not left to `regex`'s word boundaries.
    word: (bool, bool),
    /// Whether a match must stand alone, between characters that aren't
    /// word characters (a [`FindQuery`]'s `whole_word`).
    whole: bool,
    /// The translated regex's source with the flags it's built with.
    key: String,
}

/// What matches a [`Pattern`].
#[derive(Clone, Debug)]
enum Engine {
    Regex {
        regex: Regex,
        /// The `regex` capture groups behind each of a pattern's: slot 0 is
        /// the whole match (group 1 for a Vim pattern, which
        /// [`Translator::pattern`] wraps in a group followed by a boundary,
        /// 0 for a find bar's), slot `n` `\(` number `n`. A group the
        /// translation had to write more than once (see [`Translator`]) has
        /// a group for each copy, in the order written.
        slots: Vec<Vec<usize>>,
    },
    /// A pattern whose match priority one regex can't write
    /// ([`Translator::inexact`]).
    Backtrack(Arc<backtrack::Program>),
}

/// A find bar's query: what [`Pattern::from_query`] builds a pattern from.
/// The fields mean what they do in `fe_project::search::SearchQuery`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindQuery {
    pub text: String,
    /// `text` is a `regex`-crate pattern, not literal text.
    pub regex: bool,
    pub match_case: bool,
    pub whole_word: bool,
}

/// Why [`Pattern::parse`] refused a pattern.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    /// An atom of Vim's that fe-vim doesn't translate, as written.
    #[error("pattern not supported: {0}")]
    Unsupported(String),
    /// A pattern Vim rejects too.
    #[error("invalid pattern: {0}")]
    Invalid(&'static str),
    /// A [`FindQuery`]'s regex that doesn't compile: the first line of
    /// `regex`'s message.
    #[error("invalid regex: {0}")]
    Regex(String),
    /// Past `regex`'s size limit, or nested too deeply.
    #[error("pattern too large: {0}")]
    TooLarge(String),
}

impl Pattern {
    /// Translates `pattern`, in Vim's default 'magic' syntax, matching case
    /// exactly unless it holds `\c`.
    pub fn parse(pattern: &str) -> Result<Self, PatternError> {
        Self::parse_with(pattern, false)
    }

    /// [`Pattern::parse`], ignoring case if `ignore_case` (`:s`'s `i` flag;
    /// `I` is `false`) unless the pattern says otherwise: `\c` ignores case
    /// and `\C` matches it, `\c` winning.
    pub fn parse_with(pattern: &str, ignore_case: bool) -> Result<Self, PatternError> {
        if pattern.is_empty() {
            return Err(PatternError::Invalid("empty pattern"));
        }
        let (mut tokens, case) = Lexer::new(pattern).run()?;
        let ignore_case = case.unwrap_or(ignore_case);
        let word = word_bounds(&mut tokens);
        let mut translator = Translator {
            tokens: &tokens,
            next: 0,
            depth: 0,
            groups: 0,
            ignore_case,
            budget: 0,
            inexact: false,
        };
        let source = unique_names(&translator.pattern()?);
        if translator.inexact {
            return Ok(Self {
                engine: Engine::Backtrack(Arc::new(backtrack::compile(&tokens, ignore_case)?)),
                word,
                whole: false,
                key: format!("{source}\0{ignore_case}\0backtrack\0{word:?}"),
            });
        }
        let builder = RegexBuilder::new(&source);
        let regex = build(builder, |error| {
            // Past the nesting limit: its message's last line says so.
            PatternError::TooLarge(
                error
                    .to_string()
                    .lines()
                    .last()
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            )
        })?;
        let mut slots = vec![vec![1]];
        slots.resize(MAX_GROUPS + 1, Vec::new());
        for (at, name) in regex.capture_names().enumerate() {
            let number = name
                .and_then(|name| name.strip_prefix('v'))
                .and_then(|name| name.split('_').next())
                .and_then(|number| number.parse::<usize>().ok());
            if let Some(number) = number {
                slots[number].push(at);
            }
        }
        Ok(Self {
            engine: Engine::Regex { regex, slots },
            word,
            whole: false,
            key: format!("{source}\0{ignore_case}\0{word:?}"),
        })
    }

    /// The pattern for a find bar's `query`: its text literal, or with
    /// `regex` in the `regex` crate's syntax (never Vim's), case-insensitive
    /// unless `match_case`. Matching is row by row, so a `\n` in a regex
    /// never matches. With `whole_word` a match is kept only between
    /// characters that aren't word characters (Unicode `\w`).
    pub fn from_query(query: &FindQuery) -> Result<Self, PatternError> {
        if query.text.is_empty() {
            return Err(PatternError::Invalid("empty pattern"));
        }
        let source = if query.regex {
            query.text.clone()
        } else {
            regex::escape(&query.text)
        };
        let mut builder = RegexBuilder::new(&source);
        builder.case_insensitive(!query.match_case);
        let regex = build(builder, |error| {
            let message = error.to_string();
            PatternError::Regex(message.lines().next().unwrap_or_default().to_string())
        })?;
        let slots = (0..regex.captures_len()).map(|at| vec![at]).collect();
        Ok(Self {
            engine: Engine::Regex { regex, slots },
            word: (false, false),
            whole: query.whole_word,
            key: format!("{source}\0{}\0{}", query.match_case, query.whole_word),
        })
    }

    /// The translated regex's source, with the flags it is built with:
    /// what two patterns that match alike share.
    pub fn regex_str(&self) -> &str {
        &self.key
    }

    /// The first match starting at or after `from`, then row by row to the
    /// end and round to the start, wrapping once: [`Pattern::find`] going
    /// forward, but taking a match at `from`. Each row is searched once.
    pub fn find_from(&self, text: &Text, from: usize) -> Option<Range<usize>> {
        self.forward(text, from, true, &Deadline::new())
            .ok()
            .flatten()
    }

    /// The match `/` (`forward`) or `?` would go to from `from`: the first
    /// match after it, or the last before it, on its row and then row by
    /// row towards the end (the start, going back), wrapping once past it
    /// to come back round to `from`'s row. Going forward, the search starts
    /// a whole character on from `from`, as in Vim, so a match starting
    /// before that (at `from` itself, say) is taken only after wrapping, and
    /// so is any on `from`'s row from its last character, where the search
    /// starts at the next row. Each row is searched once.
    ///
    /// Past [`time_limit`] this gives up and returns `None`;
    /// [`Pattern::try_find`] says so.
    pub fn find(&self, text: &Text, from: usize, forward: bool) -> Option<Range<usize>> {
        self.try_find(text, from, forward).ok().flatten()
    }

    /// [`Pattern::find`], but `Err(Interrupted)` if the search passed
    /// [`time_limit`] (counted from this call) before it could tell where
    /// the match is, which a pattern that scans to the end of the row from
    /// each of a long row's starts can do. `Ok(None)` is no match.
    pub fn try_find(
        &self,
        text: &Text,
        from: usize,
        forward: bool,
    ) -> Result<Option<Range<usize>>, Interrupted> {
        self.try_find_by(text, from, forward, &Deadline::new())
    }

    pub(crate) fn try_find_by(
        &self,
        text: &Text,
        from: usize,
        forward: bool,
        deadline: &Deadline,
    ) -> Result<Option<Range<usize>>, Interrupted> {
        if forward {
            return self.forward(text, from, false, deadline);
        }
        let at = text.offset_to_point(text.clip_offset(from.min(text.len()), Bias::Left));
        let rows = text.row_count();
        // `from`'s row is listed only as far as `from`, unless the search
        // wraps back to it.
        let from = text.point_to_offset(at);
        let mut here = RowList::new(text, at.row);
        let before = here.list_until(self, deadline, |_, unlisted| unlisted >= from)?;
        if let Some(found) = before.iter().rev().find(|found| found.start < from) {
            return Ok(Some(found.clone()));
        }
        let mut buf = String::new();
        let shift = |range: Range<usize>, start: usize| start + range.start..start + range.end;
        for row in (0..at.row).rev().chain((at.row + 1..rows).rev()) {
            let (start, hay) = row_text(text, row, &mut buf);
            let found = self.matches(hay, deadline).last();
            if deadline.expired() {
                return Err(Interrupted);
            }
            if let Some(found) = found {
                return Ok(Some(shift(found, start)));
            }
        }
        Ok(here
            .list_until(self, deadline, |_, _| false)?
            .last()
            .cloned())
    }

    /// The forward walk of [`Pattern::find`] and [`Pattern::find_from`]:
    /// `inclusive` takes a match at `from`; otherwise, as in Vim, the search
    /// starts a whole character on from `from` (from wherever in a character
    /// it is), and from the row's last character that's the next row.
    pub(crate) fn forward(
        &self,
        text: &Text,
        from: usize,
        inclusive: bool,
        deadline: &Deadline,
    ) -> Result<Option<Range<usize>>, Interrupted> {
        let at = text.offset_to_point(text.clip_offset(from.min(text.len()), Bias::Left));
        let column = at.column as usize;
        let rows = text.row_count();
        let mut buf = String::new();
        let (start, hay) = row_text(text, at.row, &mut buf);
        let shift = |range: Range<usize>, start: usize| start + range.start..start + range.end;
        let past = if inclusive {
            column
        } else {
            column + unit(&hay[column..]).unwrap_or(0)
        };
        let on_last = !inclusive && past >= hay.len();
        let mut first = None;
        if inclusive {
            // The row's first match: if it starts at or after `from` it is
            // the answer; else look on from `from`, and fall back to it
            // once wrapped. Each stretch of the row is searched once.
            first = self.matches(hay, deadline).next();
            if let Some(found) = &first
                && found.start >= column
            {
                return Ok(Some(shift(found.clone(), start)));
            }
            if first.is_some()
                && let Some(found) = self.matches_from(hay, column, deadline).next()
            {
                return Ok(Some(shift(found, start)));
            }
        } else {
            for found in self.matches(hay, deadline) {
                if found.start >= past && !on_last {
                    return Ok(Some(shift(found, start)));
                }
                first.get_or_insert(found);
                if on_last {
                    break;
                }
            }
        }
        deadline.check()?;
        // What `from`'s row offers once the search wraps back to it.
        let wrapped = first.map(|found| shift(found, start));
        for row in (at.row + 1..rows).chain(0..at.row) {
            let (start, hay) = row_text(text, row, &mut buf);
            let found = self.matches(hay, deadline).next();
            deadline.check()?;
            if let Some(found) = found {
                return Ok(Some(shift(found, start)));
            }
        }
        Ok(wrapped)
    }

    /// Where `count` searches from `from` land, each from the match the
    /// last found (`3/foo`): the start of the last match, or `None` if
    /// there is none. Past the last match the searches go round the text
    /// again, so once a match comes back the rest is counted round, not
    /// searched. After the first search, each row's matches are listed
    /// once and stepped through, so the work is bounded by the text's
    /// matches and rows, not the count, and a row's matches are listed only
    /// as far as a step needs. Past [`time_limit`] (counted from this call)
    /// it gives up and returns `None`.
    pub fn nth(&self, text: &Text, from: usize, forward: bool, count: usize) -> Option<usize> {
        self.nth_by(text, from, forward, count, &Deadline::new())
    }

    pub(crate) fn nth_by(
        &self,
        text: &Text,
        from: usize,
        forward: bool,
        count: usize,
        deadline: &Deadline,
    ) -> Option<usize> {
        let first = self.try_find_by(text, from, forward, deadline).ok()??.start;
        let mut rows = Rows {
            pattern: self,
            text,
            deadline,
            listed: HashMap::new(),
        };
        let mut seen = HashMap::from([(first, 0)]);
        let mut found = vec![first];
        for step in 1..count {
            let at = *found.last().expect("one match was found");
            let next = rows.step(at, forward).ok()?;
            if deadline.expired() {
                return None;
            }
            if let Some(&first) = seen.get(&next) {
                let cycle = &found[first..];
                return Some(cycle[(count - 1 - first) % cycle.len()]);
            }
            seen.insert(next, step);
            found.push(next);
        }
        found.last().copied()
    }

    /// The matches in one row, as Vim finds them: from its start, each
    /// search resuming at the end of the last match, or one whole character
    /// (an extended grapheme cluster) on from an empty one, and not at the
    /// row's end once past its start.
    fn matches<'a>(&'a self, hay: &'a str, deadline: &'a Deadline) -> RowMatches<'a> {
        self.matches_from(hay, 0, deadline)
    }

    /// [`Pattern::matches`] with the first search starting at `at`, with the
    /// text before it still there for anchors and boundaries to see.
    fn matches_from<'a>(
        &'a self,
        hay: &'a str,
        at: usize,
        deadline: &'a Deadline,
    ) -> RowMatches<'a> {
        RowMatches {
            deadline,
            engine: &self.engine,
            word: self.word,
            whole: self.whole,
            hay,
            at: Some(at),
            found: match &self.engine {
                Engine::Regex { regex, .. } => Found::Regex(regex.capture_locations()),
                Engine::Backtrack(_) => Found::Backtrack {
                    slots: backtrack::empty_slots(),
                    scratch: backtrack::Scratch::default(),
                },
            },
            groups: false,
            last_end: None,
        }
    }

    /// The matches in one row as `:s` takes them, each with its groups
    /// ([`RowMatches::group`]) if `groups`: as [`Pattern::matches`] finds
    /// them, but an
    /// empty match where the last match ended is skipped, the search going
    /// on a whole character later, as in nvim (`:s/b*/-/g` makes `abc`
    /// `-a-c`).
    pub(crate) fn substitutions<'a>(
        &'a self,
        hay: &'a str,
        groups: bool,
        deadline: &'a Deadline,
    ) -> RowMatches<'a> {
        RowMatches {
            groups,
            last_end: Some(None),
            ..self.matches(hay, deadline)
        }
    }
}

/// The rows [`Pattern::nth`] has searched: each row's matches, listed as
/// far as a step has needed.
struct Rows<'a> {
    pattern: &'a Pattern,
    text: &'a Text,
    deadline: &'a Deadline,
    listed: HashMap<u32, RowList<'a>>,
}

impl<'a> Rows<'a> {
    /// `row`, its matches listed until `enough` holds
    /// ([`RowList::list_until`]): `Err` if the deadline ran out first, or
    /// had already, listed or not.
    fn row(
        &mut self,
        row: u32,
        enough: impl Fn(&[Range<usize>], usize) -> bool,
    ) -> Result<&RowList<'a>, Interrupted> {
        let text = self.text;
        let list = self
            .listed
            .entry(row)
            .or_insert_with(|| RowList::new(text, row));
        list.list_until(self.pattern, self.deadline, enough)?;
        Ok(list)
    }

    /// Where [`Pattern::find`] goes from the match starting at `at`, by
    /// the same rules, from the rows' lists.
    fn step(&mut self, at: usize, forward: bool) -> Result<usize, Interrupted> {
        let from_row = self.text.offset_to_point(at).row;
        let rows = self.text.row_count();
        // Listed as far as `at`: every match before it, and its own.
        let here = self.row(from_row, |_, unlisted| unlisted > at)?;
        let ix = here.listed().partition_point(|found| found.start < at);
        if forward {
            let past = match ix < here.listed().len() {
                true => here.past(ix),
                false => at,
            };
            if past < here.end() {
                let here = self.row(from_row, |found, _| starts_from(found, past))?;
                if let Some(next) = here.listed()[ix..].iter().find(|found| found.start >= past) {
                    return Ok(next.start);
                }
            }
            for row in (from_row + 1..rows).chain(0..=from_row) {
                if let Some(next) = self
                    .row(row, |found, _| !found.is_empty())?
                    .listed()
                    .first()
                {
                    return Ok(next.start);
                }
            }
        } else {
            if let Some(ix) = ix.checked_sub(1) {
                return Ok(here.listed()[ix].start);
            }
            for row in (0..from_row).rev().chain((from_row..rows).rev()) {
                if let Some(next) = self.row(row, |_, _| false)?.listed().last() {
                    return Ok(next.start);
                }
            }
        }
        Ok(at)
    }
}

/// Whether the last of `found` starts at or after `at`, so that no match
/// listed after it can come before.
pub(crate) fn starts_from(found: &[Range<usize>], at: usize) -> bool {
    found.last().is_some_and(|found| found.start >= at)
}

/// One row's matches, as [`Pattern::matches`] finds them, listed only as
/// far as a caller has needed: a search that wants an early match never
/// waits on a later one, which a pattern that backtracks through the rest
/// of the row can take past the deadline to find or rule out. The row is
/// read once, and its text kept while matches are left to list.
pub(crate) struct RowList<'a> {
    /// The row's offset in the text, and its end.
    start: usize,
    end: usize,
    hay: Cow<'a, str>,
    /// The matches listed, as ranges in the text, and where a forward
    /// search from each one's start begins: a whole character on.
    found: Vec<Range<usize>>,
    past: Vec<usize>,
    /// Where the row's next search starts, `None` once it has none left.
    resume: Option<usize>,
}

impl<'a> RowList<'a> {
    /// `row` of `text`, read but with no match listed yet.
    pub(crate) fn new(text: &'a Text, row: u32) -> Self {
        let (start, hay) = row_kept(text, row);
        Self {
            start,
            end: start + hay.len(),
            hay,
            found: Vec::new(),
            past: Vec::new(),
            resume: Some(0),
        }
    }

    /// The row's end, as an offset in the text.
    pub(crate) fn end(&self) -> usize {
        self.end
    }

    /// The matches listed so far, in order.
    pub(crate) fn listed(&self) -> &[Range<usize>] {
        &self.found
    }

    /// Where a forward search from the start of listed match `ix` begins.
    pub(crate) fn past(&self, ix: usize) -> usize {
        self.past[ix]
    }

    /// Whether every one of the row's matches is listed.
    #[cfg(test)]
    pub(crate) fn done(&self) -> bool {
        self.resume.is_none()
    }

    /// Lists the row's matches on until `enough` holds, or the row has none
    /// left, and gives them all. `enough` is given the matches listed and
    /// where any match not listed yet starts, at the earliest (the next
    /// search's start), so a search that wants a match before some place
    /// can stop once none is left there. `Err` if `deadline` has run out,
    /// which it reads first, or runs out before then, read again after
    /// every regex call: a call that overran it, whether it found a match
    /// or nothing more, counts for nothing, and the row isn't taken as done.
    pub(crate) fn list_until(
        &mut self,
        pattern: &Pattern,
        deadline: &Deadline,
        enough: impl Fn(&[Range<usize>], usize) -> bool,
    ) -> Result<&[Range<usize>], Interrupted> {
        if deadline.expired() {
            return Err(Interrupted);
        }
        if let Some(at) = self.resume
            && !enough(&self.found, self.start + at)
        {
            let mut matches = pattern.matches_from(&self.hay, at, deadline);
            loop {
                let found = matches.next();
                if deadline.expired() {
                    return Err(Interrupted);
                }
                let Some(found) = found else {
                    self.resume = None;
                    break;
                };
                let past = found.start + unit(&self.hay[found.start..]).unwrap_or(0);
                self.found
                    .push(self.start + found.start..self.start + found.end);
                self.past.push(self.start + past);
                self.resume = matches.at;
                match self.resume {
                    Some(at) if !enough(&self.found, self.start + at) => {}
                    _ => break,
                }
            }
            if self.resume.is_none() {
                self.hay = Cow::Borrowed("");
            }
        }
        Ok(&self.found)
    }
}

/// `pattern` translated, the last one kept on each thread: a search motion
/// asks once for each caret.
pub(crate) fn parsed(pattern: SearchPattern) -> Result<Pattern, PatternError> {
    thread_local! {
        static LAST: RefCell<Option<(SearchPattern, Result<Pattern, PatternError>)>> =
            const { RefCell::new(None) };
    }
    LAST.with_borrow_mut(|last| match last {
        Some((kept, parsed)) if *kept == pattern => parsed.clone(),
        _ => last
            .insert((pattern, Pattern::parse(pattern.as_str())))
            .1
            .clone(),
    })
}

/// Splits a search's line, what's typed after `/` or `?` (the
/// `delimiter`), into its pattern and, if a delimiter ends the pattern,
/// what follows that (a search offset, `/foo/e`). As Vim reads the line, a
/// delimiter escaped with a backslash or inside a collection doesn't end
/// it: `[` starts one, but after `\V` (very nomagic, until `\v`, `\m` or
/// `\M`) `\[` does, as in nvim (nomagic's `[` too), and it runs to its
/// `]`, or to the line's end if none closes it. After `?`, `\?` is a `?`.
pub fn split_search(line: &str, delimiter: char) -> (String, Option<&str>) {
    let mut pattern = String::new();
    let mut very_nomagic = false;
    let mut at = 0;
    while let Some(ch) = line[at..].chars().next() {
        at += ch.len_utf8();
        if ch == delimiter {
            return (pattern, Some(&line[at..]));
        }
        let collection = match ch {
            '\\' => {
                let Some(next) = line[at..].chars().next() else {
                    pattern.push(ch);
                    break;
                };
                at += next.len_utf8();
                match next {
                    'v' | 'm' | 'M' => very_nomagic = false,
                    'V' => very_nomagic = true,
                    _ => {}
                }
                if next == '?' && delimiter == '?' {
                    pattern.push('?');
                    continue;
                }
                pattern.push(ch);
                pattern.push(next);
                next == '[' && very_nomagic
            }
            _ => {
                pattern.push(ch);
                ch == '[' && !very_nomagic
            }
        };
        if collection {
            let end = at + collection_len(&line[at..]);
            pattern.push_str(&line[at..end]);
            at = end;
        }
    }
    (pattern, None)
}

/// How much of `rest`, what follows a collection's `[`, the collection
/// takes, its `]` included: `^` and then `]` or `-` first are members, a
/// backslash takes the next character, `[:name:]` is one member. All of it
/// if no `]` closes it.
fn collection_len(rest: &str) -> usize {
    let mut at = usize::from(rest.starts_with('^'));
    if rest[at..].starts_with([']', '-']) {
        at += 1;
    }
    while let Some(ch) = rest[at..].chars().next() {
        if rest[at..].starts_with("[:")
            && let Some(end) = rest[at + 2..].find(":]")
        {
            at += 2 + end + 2;
            continue;
        }
        at += ch.len_utf8();
        match ch {
            ']' => return at,
            '\\' => at += rest[at..].chars().next().map_or(0, char::len_utf8),
            _ => {}
        }
    }
    rest.len()
}

/// What `*` (`forward`) or `#` searches for at byte `at`, and where from:
/// the keyword under or after `at` on its row, as `\<word\>` and from its
/// start, or with none, the non-blank text there, from its start. The
/// pattern has the characters magic gives a meaning escaped, and `\<` and
/// `\>` only where a keyword begins or ends it. `None` if the rest of the
/// row is blank.
pub(crate) fn word_at(text: &Text, at: usize, forward: bool) -> Option<(usize, String)> {
    let at = text.offset_to_point(text.clip_offset(at.min(text.len()), Bias::Left));
    let line = Line::new(text, at.row);
    let n = line.len();
    let ix = line
        .starts
        .partition_point(|&start| start <= at.column as usize);
    let ix = ix.saturating_sub(1);
    let kind = |ix: usize| motion::class(line.grapheme(ix), false);
    let keyword = |ix: usize| matches!(kind(ix), Class::Of(c) if c >= 2);
    // The keyword at or after `ix`, or failing that the non-blank text: a
    // run of one class either way, as nvim takes it.
    let mut start = (ix..n)
        .find(|&ix| keyword(ix))
        .or_else(|| (ix..n).find(|&ix| kind(ix) != Class::Blank))?;
    let this = kind(start);
    while start > 0 && kind(start - 1) == this {
        start -= 1;
    }
    let end = (start..n).find(|&ix| kind(ix) != this).unwrap_or(n);
    let first = line.starts[start];
    let last = line.starts.get(end).copied().unwrap_or(line.text.len());
    let mut pattern = String::new();
    if keyword(start) {
        pattern.push_str("\\<");
    }
    for ch in line.text[first..last].chars() {
        if "\\.*$^~[]".contains(ch) || ch == if forward { '/' } else { '?' } {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    if keyword(end - 1) {
        pattern.push_str("\\>");
    }
    Some((line.start + first, pattern))
}

/// Every match on `rows` (clamped to the text), in order, at most `cap`
/// of them, for a host to paint; it stops at [`time_limit`] (counted from
/// the call), with the matches so far. A zero-width match is an empty range.
pub fn matches_in(
    text: &Text,
    rows: Range<u32>,
    pattern: &Pattern,
    cap: usize,
) -> Vec<Range<usize>> {
    matches_in_until(text, rows, pattern, cap, &mut Deadline::new())
}

/// [`matches_in`] against `deadline`, which the caller may share across
/// calls and ask afterwards whether it [`Deadline::ran_out`].
pub fn matches_in_until(
    text: &Text,
    rows: Range<u32>,
    pattern: &Pattern,
    cap: usize,
    deadline: &mut Deadline,
) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut buf = String::new();
    for row in rows.start..rows.end.min(text.row_count()) {
        let room = cap - found.len();
        if room == 0 || deadline.expired() {
            break;
        }
        let (start, hay) = row_text(text, row, &mut buf);
        found.extend(
            pattern
                .matches(hay, deadline)
                .take(room)
                .map(|range| start + range.start..start + range.end),
        );
    }
    found
}

/// The first match on `row` (an offset range in the text), for an ex
/// command that reads rows one by one under one `deadline`: `Err` if it
/// ran out before the row could be told, never a miss in its place.
pub(crate) fn first_in_row(
    text: &Text,
    row: u32,
    pattern: &Pattern,
    deadline: &Deadline,
) -> Result<Option<Range<usize>>, Interrupted> {
    let mut buf = String::new();
    let (start, hay) = row_text(text, row, &mut buf);
    let found = pattern.matches(hay, deadline).next();
    deadline.check()?;
    Ok(found.map(|range| start + range.start..start + range.end))
}

/// The matches on `row` that reach `window` (offsets in the text), in
/// order, at most `cap` of them. They are the matches [`matches_in`] finds
/// on the row, searched from its start: a non-empty one reaches the window
/// if it ends after its start and starts before its end, an empty one if it
/// is at either end or between. Stops at the first match starting past the
/// window, or at [`time_limit`].
pub fn matches_reaching(
    text: &Text,
    row: u32,
    window: Range<usize>,
    pattern: &Pattern,
    cap: usize,
) -> Vec<Range<usize>> {
    reaching(text, row, window, pattern, cap, &Deadline::new())
}

/// [`matches_reaching`] against `deadline`, which the caller may share
/// across rows and ask afterwards whether it [`Deadline::ran_out`].
pub fn matches_reaching_until(
    text: &Text,
    row: u32,
    window: Range<usize>,
    pattern: &Pattern,
    cap: usize,
    deadline: &mut Deadline,
) -> Vec<Range<usize>> {
    reaching(text, row, window, pattern, cap, deadline)
}

/// [`matches_reaching`] against `deadline`, which started before the row is
/// read: gathering a long row's chunks is part of the budget.
fn reaching(
    text: &Text,
    row: u32,
    window: Range<usize>,
    pattern: &Pattern,
    cap: usize,
    deadline: &Deadline,
) -> Vec<Range<usize>> {
    if row >= text.row_count() || deadline.expired() {
        return Vec::new();
    }
    let mut buf = String::new();
    let (start, hay) = row_text(text, row, &mut buf);
    pattern
        .matches(hay, deadline)
        .map(|range| start + range.start..start + range.end)
        .take_while(|range| range.start <= window.end)
        .filter(|range| match range.is_empty() {
            true => window.start <= range.start,
            false => range.end > window.start && range.start < window.end,
        })
        .take(cap)
        .collect()
}

#[cfg(test)]
thread_local! {
    /// How many rows [`Pattern::find`] and [`matches_in`] have searched on
    /// this thread.
    pub(crate) static ROWS_SEARCHED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `row`'s offset and text, without its line break: borrowed from `text`
/// when it lies in one chunk, else gathered into `buf`.
pub(crate) fn row_text<'a>(text: &'a Text, row: u32, buf: &'a mut String) -> (usize, &'a str) {
    let (start, mut chunks) = row_chunks(text, row);
    match (chunks.next(), chunks.next()) {
        (None, _) => (start, ""),
        (Some(only), None) => (start, only),
        (Some(first), Some(second)) => {
            buf.clear();
            buf.push_str(first);
            buf.push_str(second);
            buf.extend(chunks);
            (start, buf.as_str())
        }
    }
}

/// [`row_text`] to keep: borrowed from `text` when it lies in one chunk,
/// else gathered.
fn row_kept(text: &Text, row: u32) -> (usize, Cow<'_, str>) {
    let (start, mut chunks) = row_chunks(text, row);
    let hay = match (chunks.next(), chunks.next()) {
        (None, _) => Cow::Borrowed(""),
        (Some(only), None) => Cow::Borrowed(only),
        (Some(first), Some(second)) => {
            Cow::Owned([first, second].into_iter().chain(chunks).collect())
        }
    };
    (start, hay)
}

/// `row`'s offset and the chunks of its text, without its line break.
fn row_chunks(text: &Text, row: u32) -> (usize, impl Iterator<Item = &str>) {
    #[cfg(test)]
    ROWS_SEARCHED.with(|rows| rows.set(rows.get() + 1));
    let start = text.row_start(row);
    let end = if row + 1 < text.row_count() {
        text.row_start(row + 1) - 1
    } else {
        text.len()
    };
    (start, text.chunks_in_range(start..end))
}

/// The length of the character `text` starts with, a whole one as Vim
/// counts them: an extended grapheme cluster, except that Vim also joins
/// nonspacing and enclosing marks (and what follows them as it would follow
/// a letter) to a control character.
fn unit(text: &str) -> Option<usize> {
    static CONTROL_MARK: OnceLock<Regex> = OnceLock::new();
    let first = text.graphemes(true).next()?.len();
    let control_mark = CONTROL_MARK
        .get_or_init(|| Regex::new(r"^\p{gcb=Control}[\p{Mn}\p{Me}]").expect("a valid regex"));
    if !control_mark.is_match(text) {
        return Some(first);
    }
    Some(first + text[first..].graphemes(true).next().map_or(0, str::len))
}

/// See [`Pattern::matches`] and [`Pattern::substitutions`].
pub(crate) struct RowMatches<'a> {
    deadline: &'a Deadline,
    engine: &'a Engine,
    word: (bool, bool),
    whole: bool,
    hay: &'a str,
    at: Option<usize>,
    /// The last match's groups, read through [`RowMatches::group`] if
    /// `groups`.
    found: Found,
    groups: bool,
    /// For `:s`: where the last non-empty match ended, if one did.
    last_end: Option<Option<usize>>,
}

impl RowMatches<'_> {
    /// Group `n`'s range in the last match (0 is the whole match), if it
    /// took part; `None` if the groups weren't wanted.
    pub(crate) fn group(&self, n: usize) -> Option<Range<usize>> {
        if !self.groups {
            return None;
        }
        let (Engine::Regex { slots, .. }, Found::Regex(locs)) = (self.engine, &self.found) else {
            let Found::Backtrack { slots, .. } = &self.found else {
                unreachable!("an engine's own state");
            };
            return backtrack::group(slots, n);
        };
        // A group written twice: the copy that took part last in the text,
        // which isn't the last one written (`\(a\|.\)\+` on "xa" ends on
        // the first copy). Of copies that end together, the later starting
        // one (an empty one) came last.
        let (start, end) = slots
            .get(n)?
            .iter()
            .filter_map(|&at| locs.get(at))
            .max_by_key(|&(start, end)| (end, start))?;
        Some(start..end)
    }
}

/// What a [`RowMatches`] keeps of its last match.
enum Found {
    Regex(CaptureLocations),
    Backtrack {
        slots: backtrack::Slots,
        scratch: backtrack::Scratch,
    },
}

impl Iterator for RowMatches<'_> {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Range<usize>> {
        loop {
            let at = self.at.take()?;
            if self.deadline.passed() {
                return None;
            }
            REGEX_CALLS.fetch_add(1, Ordering::Relaxed);
            let found = match (self.engine, &mut self.found) {
                (Engine::Regex { regex, slots }, Found::Regex(locs)) => {
                    regex.captures_read_at(locs, self.hay, at)?;
                    let (start, end) = locs.get(slots[0][0]).expect("a match has its group");
                    start..end
                }
                (Engine::Backtrack(program), Found::Backtrack { slots, scratch }) => {
                    match program.find_at(self.hay, at, self.deadline, scratch, slots) {
                        Ok(true) => backtrack::group(slots, 0).expect("a match has its span"),
                        Ok(false) | Err(backtrack::GaveUp) => return None,
                    }
                }
                _ => unreachable!("an engine's own state"),
            };
            let next = if found.is_empty() {
                unit(&self.hay[found.start..]).map_or(usize::MAX, |unit| found.start + unit)
            } else {
                found.end
            };
            let (start, end) = (self.word.0, self.word.1);
            let skipped = found.is_empty() && self.last_end == Some(Some(found.start));
            if skipped
                || (start && !word_starts(self.hay, found.start))
                || (end && !word_ends(self.hay, found.end))
                || (self.whole && !stands_alone(self.hay, found.clone()))
            {
                // Not a whole word, or empty where `:s`'s last match ended:
                // the next search starts a character on.
                let rest = &self.hay[found.start..];
                let step = if self.whole {
                    rest.chars().next().map_or(1, char::len_utf8)
                } else {
                    unit(rest).unwrap_or(1)
                };
                let on = found.start + step;
                // A whole-word query's `$` can still match at the row's end.
                self.at =
                    (on < self.hay.len() || (self.whole && on == self.hay.len())).then_some(on);
                continue;
            }
            if let Some(last_end) = &mut self.last_end {
                *last_end = (!found.is_empty()).then_some(found.end);
            }
            self.at = (next < self.hay.len()).then_some(next);
            return Some(found);
        }
    }
}

/// Compiles `builder`'s regex; a size-limit failure is `TooLarge`, any other
/// `other`'s.
fn build(
    builder: RegexBuilder,
    other: impl FnOnce(regex::Error) -> PatternError,
) -> Result<Regex, PatternError> {
    builder.build().map_err(|error| match error {
        regex::Error::CompiledTooBig(limit) => {
            PatternError::TooLarge(format!("over {limit} bytes compiled"))
        }
        error => other(error),
    })
}

/// Whether neither the character before `found` in `hay` nor the one after
/// it is a word character.
fn stands_alone(hay: &str, found: Range<usize>) -> bool {
    let word = |ch: Option<char>| ch.is_some_and(regex_syntax::is_word_character);
    !word(hay[..found.start].chars().next_back()) && !word(hay[found.end..].chars().next())
}

/// Takes a leading `\<` and a trailing `\>` off `tokens` if they are the
/// only ones, nothing branches and what is between is literal text, for [`word_starts`] and [`word_ends`] to
/// check; else leaves them to `regex`'s word boundaries.
fn word_bounds(tokens: &mut Vec<Token>) -> (bool, bool) {
    let marks = |token: &Token| matches!(token, Token::WordStart | Token::WordEnd);
    let real: Vec<usize> = (0..tokens.len())
        .filter(|&ix| !matches!(tokens[ix], Token::Flag))
        .collect();
    let one_branch = !tokens.iter().any(|token| matches!(token, Token::Alt));
    let start = real
        .first()
        .is_some_and(|&ix| tokens[ix] == Token::WordStart);
    let end = real.last().is_some_and(|&ix| tokens[ix] == Token::WordEnd) && real.len() > 1;
    let inner = real
        .len()
        .saturating_sub(usize::from(start) + usize::from(end));
    let others = real
        .iter()
        .enumerate()
        .filter(|&(at, &ix)| {
            marks(&tokens[ix]) && !(at == 0 && start || at + 1 == real.len() && end)
        })
        .count();
    // Only literal text: a quantifier needs to retry other endpoints, and
    // `^` and `$` read the marks beside them, which taking those off changes.
    let literal = real.iter().all(|&ix| {
        matches!(
            tokens[ix],
            Token::Char(_) | Token::WordStart | Token::WordEnd
        )
    });
    if !one_branch || others > 0 || inner == 0 || !(start || end) || !literal {
        return (false, false);
    }
    if end {
        tokens.remove(real[real.len() - 1]);
    }
    if start {
        tokens.remove(real[0]);
    }
    (start, end)
}

/// Whether a word starts at `at` in `hay`: a keyword character, after a
/// character of another class.
fn word_starts(hay: &str, at: usize) -> bool {
    let Some(next) = hay[at..].graphemes(true).next() else {
        return false;
    };
    let class = motion::class(next, false);
    matches!(class, Class::Of(c) if c >= 2)
        && hay[..at]
            .graphemes(true)
            .next_back()
            .is_none_or(|prev| motion::class(prev, false) != class)
}

/// Whether a word ends at `at` in `hay`: after a keyword character, before
/// one of another class.
fn word_ends(hay: &str, at: usize) -> bool {
    let Some(prev) = hay[..at].graphemes(true).next_back() else {
        return false;
    };
    let class = motion::class(prev, false);
    matches!(class, Class::Of(c) if c >= 2)
        && hay[at..]
            .graphemes(true)
            .next()
            .is_none_or(|next| motion::class(next, false) != class)
}

/// Vim's 'magic' levels: which characters are special without a backslash.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Magic {
    /// `\v`: every ASCII character but letters, digits and `_`.
    Very,
    /// The default (`\m`): `^ $ . * [ ~`.
    On,
    /// `\M`: `^ $`.
    Off,
    /// `\V`: none.
    VeryOff,
}

impl Magic {
    /// Whether `op`, one of [`OPERATORS`], is special without a backslash.
    fn bare(self, op: char) -> bool {
        match self {
            Self::Very => true,
            Self::On => matches!(op, '^' | '$' | '.' | '*' | '[' | '~'),
            Self::Off => matches!(op, '^' | '$'),
            Self::VeryOff => false,
        }
    }
}

/// The characters that are special at some 'magic' level, with or without a
/// backslash; any other punctuation is itself either way.
const OPERATORS: &str = "^$.*[~+=?{()|<>%@&";

/// A multi: how many of the atom before it to match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Multi {
    Star,
    Plus,
    Optional,
    Brace {
        min: u32,
        max: Option<u32>,
        lazy: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    /// A character matched as itself.
    Char(char),
    /// `.`.
    Any,
    /// A backslash class, as `regex` syntax.
    Class(&'static str),
    Set(Collection),
    WordStart,
    WordEnd,
    /// A special `^`: start-of-line anywhere (`\v`, `\V`), or only at the
    /// start of a branch and else itself.
    Caret {
        anywhere: bool,
    },
    /// A special `$`: end-of-line anywhere (`\v`, `\V`), or only at the end
    /// of a branch and else itself.
    Dollar {
        anywhere: bool,
    },
    /// `bare` when written without a backslash: only a bare `*` is itself
    /// where nothing comes before it.
    Multi {
        multi: Multi,
        bare: bool,
    },
    /// `\c \C \v \m \M \V`, which match nothing; a multi can't follow one.
    Flag,
    Open {
        capture: bool,
    },
    Close,
    Alt,
}

struct Lexer<'a> {
    pattern: &'a str,
    /// Byte offset of the next character.
    at: usize,
    magic: Magic,
    ignore_case: bool,
    /// The pattern holds `\C`.
    match_case: bool,
    tokens: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn new(pattern: &'a str) -> Self {
        Self {
            pattern,
            at: 0,
            magic: Magic::On,
            ignore_case: false,
            match_case: false,
            tokens: Vec::new(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.pattern[self.at..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.at += ch.len_utf8();
        Some(ch)
    }

    /// The pattern's tokens, and whether it ignores case: `Some(true)` if it
    /// holds `\c` (which wins over `\C`, wherever either is), `Some(false)`
    /// if it holds only `\C`, `None` if neither.
    fn run(mut self) -> Result<(Vec<Token>, Option<bool>), PatternError> {
        while let Some(ch) = self.bump() {
            let start = self.at - ch.len_utf8();
            let (escaped, ch) = match ch {
                // A trailing backslash is itself, as in Vim.
                '\\' => match self.bump() {
                    Some(next) => (true, next),
                    None => (false, '\\'),
                },
                ch => (false, ch),
            };
            if OPERATORS.contains(ch) {
                if escaped != self.magic.bare(ch) {
                    self.operator(ch, start, !escaped)?;
                } else {
                    self.tokens.push(Token::Char(ch));
                }
            } else if escaped && (ch.is_ascii_alphanumeric() || ch == '_') {
                self.escape(ch, start)?;
            } else if escaped && !ch.is_ascii() {
                return Err(self.unsupported(start));
            } else {
                self.tokens.push(Token::Char(ch));
            }
        }
        let case = match (self.ignore_case, self.match_case) {
            (true, _) => Some(true),
            (false, true) => Some(false),
            (false, false) => None,
        };
        Ok((self.tokens, case))
    }

    /// A special `op` from [`OPERATORS`], which began at `start`, `bare` if
    /// written without a backslash.
    fn operator(&mut self, op: char, start: usize, bare: bool) -> Result<(), PatternError> {
        let very = matches!(self.magic, Magic::Very | Magic::VeryOff);
        let multi = |multi| Token::Multi { multi, bare };
        let token = match op {
            '.' => Token::Any,
            '*' => multi(Multi::Star),
            '+' => multi(Multi::Plus),
            '=' | '?' => multi(Multi::Optional),
            '{' => multi(self.brace()?),
            '(' => Token::Open { capture: true },
            ')' => Token::Close,
            '|' => Token::Alt,
            '<' => Token::WordStart,
            '>' => Token::WordEnd,
            '^' => Token::Caret { anywhere: very },
            '$' => Token::Dollar { anywhere: very },
            // `~` is the last substitute string, and there is none yet.
            '~' => Token::Char('~'),
            '[' => match collection(self.pattern, self.at)? {
                Some((set, end)) => {
                    self.at = end;
                    Token::Set(set)
                }
                // With no `]` to end it, `[` is itself.
                None => Token::Char('['),
            },
            '%' if self.peek() == Some('(') => {
                self.bump();
                Token::Open { capture: false }
            }
            '%' => {
                self.skip_while(12, |ch| ch.is_ascii_digit() || "<>.'".contains(ch));
                self.bump();
                return Err(self.unsupported(start));
            }
            '@' => {
                self.skip_while(12, |ch| ch.is_ascii_digit() || ch == '<');
                self.bump();
                return Err(self.unsupported(start));
            }
            _ => return Err(self.unsupported(start)),
        };
        self.tokens.push(token);
        Ok(())
    }

    /// A backslash and a letter, digit or `_`, which began at `start`.
    fn escape(&mut self, ch: char, start: usize) -> Result<(), PatternError> {
        let token = match ch {
            'e' => Token::Char('\u{1b}'),
            't' => Token::Char('\t'),
            'r' => Token::Char('\r'),
            'b' => Token::Char('\u{8}'),
            'c' => {
                self.ignore_case = true;
                Token::Flag
            }
            // Match case: `\c` wins over it.
            'C' => {
                self.match_case = true;
                Token::Flag
            }
            'v' | 'm' | 'M' | 'V' => {
                self.magic = match ch {
                    'v' => Magic::Very,
                    'm' => Magic::On,
                    'M' => Magic::Off,
                    _ => Magic::VeryOff,
                };
                Token::Flag
            }
            // `\_x` and `\zx` name their atom with the character after.
            '_' | 'z' => {
                self.bump();
                return Err(self.unsupported(start));
            }
            ch => match class(ch) {
                Some(class) => Token::Class(class),
                None => return Err(self.unsupported(start)),
            },
        };
        self.tokens.push(token);
        Ok(())
    }

    /// The rest of a `\{…}` multi, after the `{`: `-` for as few as
    /// possible, then `n,m`, `n`, `n,`, `,m` or nothing, then `}` or `\}`.
    fn brace(&mut self) -> Result<Multi, PatternError> {
        let lazy = self.peek() == Some('-');
        if lazy {
            self.bump();
        }
        let min = self.count()?;
        let comma = self.peek() == Some(',');
        let max = if comma {
            self.bump();
            self.count()?
        } else {
            min
        };
        if self.peek() == Some('\\') {
            self.bump();
        }
        if self.bump() != Some('}') {
            return Err(PatternError::Invalid("syntax error in \\{...}"));
        }
        let min_count = min.unwrap_or(0);
        Ok(Multi::Brace {
            // Vim swaps bounds given the wrong way round.
            min: max.map_or(min_count, |max| min_count.min(max)),
            max: max.map(|max| min_count.max(max)),
            lazy,
        })
    }

    /// The digits of a count, if any.
    fn count(&mut self) -> Result<Option<u32>, PatternError> {
        let start = self.at;
        self.skip_while(usize::MAX, |ch| ch.is_ascii_digit());
        let digits = &self.pattern[start..self.at];
        if digits.is_empty() {
            return Ok(None);
        }
        digits
            .parse()
            .map(Some)
            .map_err(|_| PatternError::TooLarge(format!("count {digits}")))
    }

    fn skip_while(&mut self, most: usize, keep: impl Fn(char) -> bool) {
        for _ in 0..most {
            match self.peek() {
                Some(ch) if keep(ch) => {
                    self.bump();
                }
                _ => break,
            }
        }
    }

    /// The error naming the atom from `start` to here.
    fn unsupported(&self, start: usize) -> PatternError {
        PatternError::Unsupported(self.pattern[start..self.at].to_string())
    }
}

/// The class a backslash and `letter` name (`\s`, `\d`, …), as a `regex`
/// class. `\c` doesn't apply to these, as in Vim.
fn class(letter: char) -> Option<&'static str> {
    Some(match letter {
        's' => r"[\t ]",
        'S' => r"[^\t ]",
        'd' => "[0-9]",
        'D' => "[^0-9]",
        'w' => "[0-9A-Za-z_]",
        'W' => "[^0-9A-Za-z_]",
        'a' => "[A-Za-z]",
        'A' => "[^A-Za-z]",
        'l' => "[a-z]",
        'L' => "[^a-z]",
        'u' => "[A-Z]",
        'U' => "[^A-Z]",
        'x' => "[0-9A-Fa-f]",
        'X' => "[^0-9A-Fa-f]",
        'o' => "[0-7]",
        'O' => "[^0-7]",
        'h' => "[A-Za-z_]",
        'H' => "[^A-Za-z_]",
        _ => return None,
    })
}

/// The ranges `[:name:]` stands for in a collection: the ASCII ones only;
/// `upper` and `lower` are Unicode, and handled by [`case_class`].
fn named_class(name: &str) -> Option<&'static [(char, char)]> {
    Some(match name {
        "alnum" => &[('0', '9'), ('A', 'Z'), ('a', 'z')],
        "alpha" => &[('A', 'Z'), ('a', 'z')],
        "blank" => &[('\t', '\t'), (' ', ' ')],
        "cntrl" => &[('\0', '\u{1f}'), ('\u{7f}', '\u{7f}')],
        "digit" => &[('0', '9')],
        "graph" => &[('!', '~')],
        "punct" => &[('!', '/'), (':', '@'), ('[', '`'), ('{', '~')],
        "space" => &[('\t', '\r'), (' ', ' ')],
        "xdigit" => &[('0', '9'), ('A', 'F'), ('a', 'f')],
        "return" => &[('\r', '\r')],
        "tab" => &[('\t', '\t')],
        "escape" => &[('\u{1b}', '\u{1b}')],
        "backspace" => &[('\u{8}', '\u{8}')],
        _ => return None,
    })
}

/// The ranges `[:upper:]` (`upper`) or `[:lower:]` stands for, as nvim has
/// them: a character with a case mapping to a different character, or
/// another character's one-character mapping, built once over all `char`s.
fn case_class(upper: bool) -> &'static [(char, char)] {
    static TABLES: OnceLock<[Vec<(char, char)>; 2]> = OnceLock::new();
    &TABLES.get_or_init(|| {
        let mut member = [vec![false; 0x110000], vec![false; 0x110000]];
        for ch in '\0'..=char::MAX {
            let up = single(ch.to_uppercase());
            let low = single(ch.to_lowercase());
            if low != Some(ch) {
                member[0][ch as usize] = true;
            }
            if let Some(up) = up.filter(|&up| up != ch) {
                member[0][up as usize] = true;
                member[1][ch as usize] = true;
            }
            if let Some(low) = low.filter(|&low| low != ch) {
                member[1][low as usize] = true;
            }
        }
        member.map(|member| {
            let mut ranges: Vec<(char, char)> = Vec::new();
            for ch in '\0'..=char::MAX {
                if !member[ch as usize] {
                    continue;
                }
                match ranges.last_mut() {
                    Some(last) if last.1 as u32 + 1 == ch as u32 => last.1 = ch,
                    _ => ranges.push((ch, ch)),
                }
            }
            ranges
        })
    })[usize::from(!upper)]
}

/// The collection from byte `at` (just after its `[`) to its `]`, as a
/// `regex` class, and the offset after the `]`; `None` if no `]` ends it,
/// when Vim takes the `[` as itself. Errors inside count only once a `]`
/// makes it a collection.
fn collection(pattern: &str, at: usize) -> Result<Option<(Collection, usize)>, PatternError> {
    let mut rest = &pattern[at..];
    let negated = rest.starts_with('^');
    if negated {
        rest = &rest[1..];
    }
    let mut set = Collection {
        negated,
        ascii: Vec::new(),
        folded: Vec::new(),
        cased: Vec::new(),
    };
    // Which of `a-z A-Z 0-9 _` the members are, or `None` once one isn't.
    let mut shape = Some(0u8);
    let mut error = None;
    // A `]` first is itself (and may start a range).
    let mut first = true;
    loop {
        let Some(ch) = rest.chars().next() else {
            return Ok(None);
        };
        let leading = std::mem::take(&mut first);
        if ch == ']' && !leading {
            rest = &rest[1..];
            break;
        }
        let item_start = rest;
        if let Some(name) = rest.strip_prefix("[:") {
            match name.split_once(":]") {
                // A class can't start a range.
                Some((name, after)) if after.starts_with('-') && !after.starts_with("-]") => {
                    return Err(PatternError::Unsupported(format!("[:{name}:]-")));
                }
                Some((name @ ("upper" | "lower"), after)) => {
                    shape = None;
                    set.cased.extend_from_slice(case_class(name == "upper"));
                    rest = after;
                }
                Some((name, after)) if named_class(name).is_some() => {
                    shape = None;
                    set.ascii
                        .extend_from_slice(named_class(name).unwrap_or_default());
                    rest = after;
                }
                Some((name, after)) => {
                    error.get_or_insert(PatternError::Unsupported(format!("[:{name}:]")));
                    rest = after;
                }
                None => {
                    error.get_or_insert(PatternError::Unsupported("[:".into()));
                    rest = name;
                }
            }
            continue;
        }
        if rest.starts_with("[=") || rest.starts_with("[.") {
            error.get_or_insert(PatternError::Unsupported(rest[..2].to_string()));
            rest = &rest[2..];
            continue;
        }
        let low = match member(&mut rest) {
            Ok(low) => low,
            Err(err) => {
                error.get_or_insert(err);
                continue;
            }
        };
        // A `-` between two members makes a range; first or last, it's itself.
        let high = match rest.strip_prefix('-') {
            // Vim reads these range ends its own way: a backslash before
            // `\ ] ^ -` doesn't escape, so `]` may end the collection, a
            // second `-` may be itself, and a class can't end a range.
            // Refused at once, as `]` may end it.
            Some(after)
                if ["\\\\", "\\]", "\\^", "\\-", "-", "[:", "[=", "[."]
                    .iter()
                    .any(|end| after.starts_with(end)) =>
            {
                let end = if after.starts_with('-') { 1 } else { 2 };
                return Err(PatternError::Unsupported(format!(
                    "{low}-{}",
                    &after[..end]
                )));
            }
            Some(after) if !after.is_empty() && !after.starts_with(']') => {
                rest = after;
                match member(&mut rest) {
                    Ok(high) => high,
                    Err(err) => {
                        error.get_or_insert(err);
                        continue;
                    }
                }
            }
            _ => low,
        };
        if low > high {
            error.get_or_insert(PatternError::Invalid("reverse range in a collection"));
        } else {
            set.folded.push((low, high));
            let written = &item_start[..item_start.len() - rest.len()];
            shape = shape.filter(|_| !written.contains('\\')).and_then(|shape| {
                let part = match (low, high) {
                    ('a', 'z') => 1,
                    ('A', 'Z') => 2,
                    ('0', '9') => 4,
                    ('_', '_') => 8,
                    _ => return None,
                };
                Some(shape | part)
            });
        }
        debug_assert!(rest.len() < item_start.len());
    }
    if let Some(error) = error {
        return Err(error);
    }
    // Members spelling out `\l`, `\u`, `\a`, `\h` or `\w` (`[a-z]`,
    // `[A-Z]`, `[a-zA-Z]`, `[a-zA-Z_]`, `[0-9A-Za-z_]`, in any order, as
    // plain characters, with no `[:name:]` class) fold only within ASCII, as
    // the class would: `\c[a-z]` doesn't take the Kelvin sign, where
    // `\c[a-y]`, `\c[\x61-z]` and `\c[a-z[:digit:]]` do.
    if matches!(shape, Some(1 | 2 | 3 | 11 | 15)) {
        set.ascii.append(&mut set.folded);
    }
    Ok(Some((set, pattern.len() - rest.len())))
}

/// A collection's members, kept apart until the whole pattern says
/// whether `\c` folds them.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Collection {
    negated: bool,
    /// `[:name:]` classes and the members that spell out a class, which
    /// `\c` folds only within ASCII.
    ascii: Vec<(char, char)>,
    /// Other members, which `\c` folds as it folds literals.
    folded: Vec<(char, char)>,
    /// `[:upper:]` and `[:lower:]`, which neither `\c` nor 'ignorecase' folds.
    cased: Vec<(char, char)>,
}

impl Collection {
    /// As a `regex` class, without the marks in it (a character carries
    /// them, and isn't one); or, if `marks`, only those, for a mark after a
    /// bare base.
    fn syntax(&self, ignore_case: bool, marks: bool) -> String {
        let mut ranges: Vec<(char, char)> = self
            .ascii
            .iter()
            .chain(&self.folded)
            .chain(&self.cased)
            .copied()
            .collect();
        if ignore_case {
            for &(low, high) in &self.ascii {
                for ch in (low..=high).take_while(char::is_ascii) {
                    let other = if ch.is_ascii_lowercase() {
                        ch.to_ascii_uppercase()
                    } else {
                        ch.to_ascii_lowercase()
                    };
                    ranges.push((other, other));
                }
            }
            let folds = folds();
            let mut seen = vec![false; folds.groups.len()];
            for &(low, high) in &self.folded {
                for (_, &group) in folds.group_of.range(low..=high) {
                    if !std::mem::replace(&mut seen[group], true) {
                        ranges.extend(folds.groups[group].iter().map(|&ch| (ch, ch)));
                    }
                }
            }
        }
        // Merged, so a wide range under `\c` stays one range.
        ranges.sort_unstable();
        let mut merged: Vec<(char, char)> = Vec::with_capacity(ranges.len());
        for (low, high) in ranges {
            match merged.last_mut() {
                Some(last) if u32::from(low) <= u32::from(last.1).saturating_add(1) => {
                    last.1 = last.1.max(high);
                }
                _ => merged.push((low, high)),
            }
        }
        let mut class = String::from(match (self.negated, marks) {
            (true, false) => r"[^\p{M}",
            (true, true) => r"[\p{M}&&[^",
            (false, _) => "[[",
        });
        for (low, high) in merged {
            class.push_str(&format!("\\x{{{:x}}}", low as u32));
            if high != low {
                class.push_str(&format!("-\\x{{{:x}}}", high as u32));
            }
        }
        class.push_str(match (self.negated, marks) {
            (true, false) => "]",
            (true, true) => "]]",
            (false, false) => r"]--\p{M}]",
            (false, true) => r"]&&\p{M}]",
        });
        class
    }
}

/// Characters `\c` takes for one another, in groups of two or more: those
/// whose uppercase is one character with the same lowercase, as nvim folds
/// case (`k`, `K` and the Kelvin sign; `s`, `S` and long s), but for the few
/// nvim leaves alone ([`UNFOLDED`]).
struct Folds {
    groups: Vec<Vec<char>>,
    /// Each grouped character's group.
    group_of: BTreeMap<char, usize>,
}

/// Characters nvim's `\c` matches only with themselves, though their
/// uppercase's lowercase is another character's: dotless `ı` and capital
/// sharp s `ẞ`.
const UNFOLDED: &[char] = &['\u{131}', '\u{1e9e}'];

/// The one character `chars` holds, if one.
fn single(mut chars: impl Iterator<Item = char>) -> Option<char> {
    match (chars.next(), chars.next()) {
        (Some(ch), None) => Some(ch),
        _ => None,
    }
}

/// Past this, no character has another case.
const LAST_CASED: u32 = 0x1_efff;

fn folds() -> &'static Folds {
    static FOLDS: OnceLock<Folds> = OnceLock::new();
    FOLDS.get_or_init(|| {
        let mut by_key: BTreeMap<char, Vec<char>> = BTreeMap::new();
        for ch in (0..=LAST_CASED).filter_map(char::from_u32) {
            if UNFOLDED.contains(&ch) {
                continue;
            }
            // One whose uppercase is several characters (`ß`, `ᾀ`) folds
            // to nothing else.
            let key = single(ch.to_uppercase())
                .map_or(ch, |upper| single(upper.to_lowercase()).unwrap_or(upper));
            by_key.entry(key).or_default().push(ch);
        }
        let mut folds = Folds {
            groups: Vec::new(),
            group_of: BTreeMap::new(),
        };
        for group in by_key.into_values().filter(|group| group.len() > 1) {
            for &ch in &group {
                folds.group_of.insert(ch, folds.groups.len());
            }
            folds.groups.push(group);
        }
        folds
    })
}

/// One character of a collection, taking it off `rest`: itself, or a
/// backslash code (`\e \t \r \b`, `\d123 \o40 \x20 € \U1234abcd`,
/// `\\ \] \^ \-`). A backslash before anything else is itself.
fn member(rest: &mut &str) -> Result<char, PatternError> {
    let mut chars = rest.chars();
    let ch = chars.next().unwrap_or_default();
    let after = chars.as_str();
    if ch != '\\' {
        *rest = after;
        return Ok(ch);
    }
    let Some(code) = after.chars().next() else {
        *rest = after;
        return Ok('\\');
    };
    let digits = &after[code.len_utf8()..];
    let simple = match code {
        'e' => Some('\u{1b}'),
        't' => Some('\t'),
        'r' => Some('\r'),
        'b' => Some('\u{8}'),
        '\\' | ']' | '^' | '-' => Some(code),
        'n' => {
            *rest = &after[1..];
            return Err(PatternError::Unsupported("\\n".into()));
        }
        _ => None,
    };
    if let Some(simple) = simple {
        *rest = &after[1..];
        return Ok(simple);
    }
    let (radix, most) = match code {
        'd' => (10, usize::MAX),
        'o' => (8, 3),
        'x' => (16, 2),
        'u' => (16, 4),
        'U' => (16, 8),
        _ => {
            *rest = after;
            return Ok('\\');
        }
    };
    let mut value: u64 = 0;
    let mut taken = 0;
    for digit in digits.chars().take(most) {
        let Some(digit) = digit.to_digit(radix) else {
            break;
        };
        let next = value * u64::from(radix) + u64::from(digit);
        // `\o` stops before going past 0o377.
        if (code == 'o' && next > 0o377) || next > u64::from(u32::MAX) {
            if code == 'o' {
                break;
            }
            *rest = &digits[taken + 1..];
            return Err(PatternError::Unsupported(format!(
                "\\{code}{}",
                &digits[..=taken]
            )));
        }
        value = next;
        taken += 1;
    }
    if taken == 0 {
        // No digits: the backslash is itself, and the letter comes next.
        *rest = after;
        return Ok('\\');
    }
    *rest = &digits[taken..];
    char::from_u32(value as u32)
        .ok_or_else(|| PatternError::Unsupported(format!("\\{code}{}", &digits[..taken])))
}

/// What comes before a branch, which decides what a `*` starting it means.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Opened {
    Pattern,
    Capture,
    Group,
    Alt,
}

/// Turns tokens into `regex` syntax.
struct Translator<'a> {
    tokens: &'a [Token],
    next: usize,
    depth: usize,
    groups: usize,
    /// Whether the pattern holds `\c`.
    ignore_case: bool,
    /// How many pieces have been translated, each once for every way the
    /// branch before it can end.
    budget: usize,
    /// Whether a repeat can't be written as one `regex` pattern in nvim's
    /// order: the pattern is then matched by [`backtrack`], and the
    /// translation is only checked for what it rejects.
    inexact: bool,
}

/// A way through some of a pattern, in `regex` syntax, and how it ends:
/// whether on a literal base written without composing characters (`a`,
/// not `a` and U+0301), which the next atom may follow into the marks after
/// it: nvim takes `a.` to match `a` and a mark, where `.` otherwise takes
/// a whole character. What comes next is translated for each way, which
/// `\%(a\|.\).` does apart (`a` then a `.` that takes the marks, or `.`
/// then one that doesn't).
#[derive(Clone, Debug, PartialEq)]
struct Path {
    text: String,
    bare: bool,
}

/// One piece of a branch, as it translates after a bare base or not.
#[derive(Debug, PartialEq)]
struct Piece {
    paths: Vec<Path>,
    /// A `^` a bare `*` follows, which is itself.
    star_is_char: bool,
}

/// How many ways through a branch may end differently, and how many pieces
/// to translate over all of them, before the pattern is too much.
const MAX_PATHS: usize = 16;
const MAX_PIECES: usize = 10_000;
/// How many different marks may be written after one character: they match
/// in any order, which is a permutation of each.
const MAX_ORDERED_MARKS: usize = 5;

/// What follows a base character's marks, and a `.`, class or collection's
/// character: the composing characters after it.
const MARKS: &str = r"\p{M}*";

/// What follows a character's base when the pattern writes `marks`: each of
/// them somewhere among its marks, in any order (nvim doesn't care) and
/// written twice no more than once, and any others.
fn carrying(marks: &[char]) -> Result<String, PatternError> {
    let mut distinct: Vec<char> = Vec::new();
    for &mark in marks {
        if !distinct.contains(&mark) {
            distinct.push(mark);
        }
    }
    if distinct.len() > MAX_ORDERED_MARKS {
        return Err(PatternError::Unsupported(format!(
            "more than {MAX_ORDERED_MARKS} different composing characters on one character"
        )));
    }
    let each = |mark: &char| format!(r"\p{{M}}*?\x{{{:x}}}", *mark as u32);
    let mut order: Vec<usize> = (0..distinct.len()).collect();
    let mut alternatives = Vec::new();
    loop {
        let ways = order.iter().map(|&at| each(&distinct[at]));
        alternatives.push(ways.collect::<String>());
        if !next_permutation(&mut order) {
            break;
        }
    }
    Ok(format!("(?:{}){MARKS}", alternatives.join("|")))
}

/// Steps `order` to its next permutation in lexicographic order, or says
/// there is none.
fn next_permutation(order: &mut [usize]) -> bool {
    let Some(pivot) = order.windows(2).rposition(|pair| pair[0] < pair[1]) else {
        return false;
    };
    let swap = order
        .iter()
        .rposition(|&at| at > order[pivot])
        .unwrap_or(pivot + 1);
    order.swap(pivot, swap);
    order[pivot + 1..].reverse();
    true
}

/// Whether `ch` is a composing character: a Unicode mark.
fn is_mark(ch: char) -> bool {
    static MARK: OnceLock<Regex> = OnceLock::new();
    !ch.is_ascii()
        && MARK
            .get_or_init(|| Regex::new(r"^\p{M}$").expect("a valid regex"))
            .is_match(ch.encode_utf8(&mut [0; 4]))
}

impl Translator<'_> {
    /// The whole pattern, as the `regex` syntax [`Pattern`] compiles: group
    /// 1 is the match, and what follows it is a non-mark or the row's end,
    /// so no match ends inside a character.
    fn pattern(&mut self) -> Result<String, PatternError> {
        let paths = self.alternation(Opened::Pattern, false)?;
        let source = paths
            .iter()
            .map(|path| path.text.as_str())
            .collect::<Vec<_>>()
            .join("|");
        Ok(format!(r"({source})(?:\P{{M}}|$)"))
    }

    /// The composing characters written after an atom, taken off the tokens.
    fn written_marks(&mut self) -> Vec<char> {
        let mut marks = Vec::new();
        while let Some(&Token::Char(mark)) = self.peek() {
            if !is_mark(mark) {
                break;
            }
            self.next += 1;
            marks.push(mark);
        }
        marks
    }

    /// One character as a `.`, class or collection takes it: `whole` and
    /// the marks after it, or if the pattern writes marks, one that carries
    /// them. After a bare base also `lone`, the marks by themselves.
    fn character(
        &mut self,
        whole: &str,
        lone: Option<&str>,
        carries: bool,
    ) -> Result<String, PatternError> {
        // A backslash class doesn't take the marks written after it: they
        // start an atom of their own.
        let marks = if carries {
            self.written_marks()
        } else {
            Vec::new()
        };
        Ok(match lone {
            _ if !marks.is_empty() => format!("{whole}{}", carrying(&marks)?),
            Some(lone) => format!("(?:{whole}{MARKS}|{lone}{MARKS})"),
            None => format!("{whole}{MARKS}"),
        })
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.next)
    }

    /// Branches separated by `\|`, up to the `\)` closing `opened` (left
    /// for the caller) or the end of the pattern. Every branch starts where
    /// the first does, after a bare base or not (`bare`), and the ways they
    /// end are all the paths of the alternation, in order.
    fn alternation(&mut self, opened: Opened, bare: bool) -> Result<Vec<Path>, PatternError> {
        let mut paths = self.branch(opened, bare)?;
        loop {
            match self.peek() {
                Some(Token::Alt) => {
                    self.next += 1;
                    paths.extend(self.branch(Opened::Alt, bare)?);
                }
                Some(Token::Close) if opened != Opened::Pattern => return Ok(paths),
                Some(Token::Close) => return Err(PatternError::Invalid("unmatched \\)")),
                None => {
                    return match opened {
                        Opened::Capture => Err(PatternError::Invalid("unmatched \\(")),
                        Opened::Group => Err(PatternError::Invalid("unmatched \\%(")),
                        Opened::Pattern | Opened::Alt => Ok(paths),
                    };
                }
                Some(_) => unreachable!("a branch ends at `\\|`, `\\)` or the end"),
            }
        }
    }

    /// Pieces (an atom and maybe a multi) up to `\|`, `\)` or the end, the
    /// first after a bare base or not (`bare`). What a piece may do depends
    /// on what comes before it, so a branch that can end in more than one
    /// way (`\%(a\|.\)` ends after a bare base or not) goes on once for
    /// each, until the pieces after it come out the same either way.
    fn branch(&mut self, opened: Opened, bare: bool) -> Result<Vec<Path>, PatternError> {
        // What every way through the branch so far begins with, and then
        // how each differs.
        let mut shared = String::new();
        let mut paths = vec![Path {
            text: String::new(),
            bare,
        }];
        // Nothing but flags yet.
        let mut first = true;
        // A bare `*` right after `^` is itself.
        let mut star_is_char = false;
        while let Some(token) = self.peek() {
            let (ends, flag) = (
                matches!(token, Token::Alt | Token::Close),
                matches!(token, Token::Flag),
            );
            let star_here = std::mem::replace(&mut star_is_char, false);
            if ends {
                break;
            }
            if flag {
                self.next += 1;
                continue;
            }
            let (start, groups) = (self.next, self.groups);
            // The piece after each distinct way the branch has ended.
            let mut pieces: Vec<(bool, Piece)> = Vec::new();
            let mut end = (start, groups);
            for path in &paths {
                if pieces.iter().any(|(bare, _)| *bare == path.bare) {
                    continue;
                }
                (self.next, self.groups) = (start, groups);
                let piece = self.piece(opened, first, star_here, path.bare)?;
                end = (self.next, self.groups);
                pieces.push((path.bare, piece));
            }
            (self.next, self.groups) = end;
            first = false;
            star_is_char = pieces[0].1.star_is_char;
            let after = if pieces.iter().all(|(_, piece)| piece == &pieces[0].1) {
                // The same whichever way the branch ended: they rejoin.
                match &paths[..] {
                    [path] => shared.push_str(&path.text),
                    _ => {
                        let ways: Vec<&str> = paths.iter().map(|path| path.text.as_str()).collect();
                        shared.push_str(&format!("(?:{})", ways.join("|")));
                    }
                }
                pieces.swap_remove(0).1.paths
            } else {
                let mut after = Vec::new();
                for path in &paths {
                    let piece = pieces.iter().find(|(bare, _)| *bare == path.bare);
                    for next in &piece.expect("a piece for every way").1.paths {
                        after.push(Path {
                            text: format!("{}{}", path.text, next.text),
                            bare: next.bare,
                        });
                    }
                }
                after
            };
            paths = join_paths(after, true);
            if paths.len() > MAX_PATHS {
                return Err(PatternError::TooLarge(
                    "alternatives that end in different ways".into(),
                ));
            }
            if let [path] = &mut paths[..] {
                shared.push_str(&std::mem::take(&mut path.text));
            }
        }
        for path in &mut paths {
            path.text.insert_str(0, &shared);
        }
        Ok(paths)
    }

    /// One piece — an atom and the multi after it — as it translates after
    /// a bare base or not (`bare`), leaving `self.next` after it.
    fn piece(
        &mut self,
        opened: Opened,
        first: bool,
        star_here: bool,
        bare: bool,
    ) -> Result<Piece, PatternError> {
        self.budget += 1;
        if self.budget > MAX_PIECES {
            return Err(PatternError::TooLarge(
                "alternatives that end in different ways".into(),
            ));
        }
        let atom_at = (self.next, self.groups);
        let (atom, start_of_line) = self.atom(opened, first, star_here, bare)?;
        let multi = match self.peek() {
            Some(&Token::Multi { multi, bare: plain })
                if !(start_of_line && plain && multi == Multi::Star) =>
            {
                multi
            }
            // After `^`, a bare `*` is itself; any other multi repeats the
            // start-of-line.
            Some(Token::Multi { .. }) => {
                return Ok(Piece {
                    paths: atom,
                    star_is_char: true,
                });
            }
            _ => {
                return Ok(Piece {
                    paths: atom,
                    star_is_char: false,
                });
            }
        };
        self.next += 1;
        if matches!(self.peek(), Some(Token::Multi { .. })) {
            return Err(PatternError::Invalid("a multi can't follow a multi"));
        }
        let paths = self.repeat(atom, atom_at, (opened, first), multi, bare)?;
        Ok(Piece {
            paths,
            star_is_char: false,
        })
    }

    /// An atom `multi` times, its first time after a bare base or not
    /// (`bare`) and the rest after how it ended: `atom` is the atom as
    /// that first translated, which began at `at` (a token and group
    /// count). A bare base's marks are a `.`'s first time only, not
    /// something to split between its repeats.
    fn repeat(
        &mut self,
        atom: Vec<Path>,
        at: (usize, usize),
        (opened, first): (Opened, bool),
        multi: Multi,
        bare: bool,
    ) -> Result<Vec<Path>, PatternError> {
        let (min, max, lazy) = match multi {
            Multi::Star => (0, None, false),
            Multi::Plus => (1, None, false),
            Multi::Optional => (0, Some(1), false),
            Multi::Brace { min, max, lazy } => (min, max, lazy),
        };
        let ends = (self.next, self.groups);
        // `regex` records no empty time of an open-ended repeat, and an
        // empty time ends nvim's repeat there with its captures set, so a
        // group that can match nothing is written out in order as well.
        let empty_last = max.is_none()
            && !lazy
            && atom
                .iter()
                .any(|path| path.text.contains("(?P<vimgroup") && matches_empty(&path.text));
        if atom.len() > 1 || empty_last {
            // Nvim's order of trying the ways of such a repeat can't be
            // written as one `regex` pattern: [`backtrack`] matches the
            // pattern. What follows is translated as if the repeat ended as
            // it began; the text is never used.
            self.inexact = true;
            return Ok(vec![nothing(bare)]);
        }
        // An atom that ends in different ways goes on as if it always ended
        // bare when it can, each time.
        let once = match &atom[..] {
            [path] => path.clone(),
            _ => Path {
                text: format!(
                    "(?:{})",
                    atom.iter()
                        .map(|path| path.text.as_str())
                        .collect::<Vec<_>>()
                        .join("|")
                ),
                bare: atom.iter().any(|path| path.bare),
            },
        };
        let can_skip = min == 0;
        let can_run = max != Some(0);
        let can_repeat = max.is_none_or(|max| max >= 2);
        // What the second and later times are.
        let rest = if !can_repeat || atom.len() != 1 || once.bare == bare {
            once.text.clone()
        } else {
            (self.next, self.groups) = at;
            let again = self.atom(opened, first, false, once.bare)?.0;
            (self.next, self.groups) = ends;
            match &again[..] {
                [path] if path.bare == once.bare => path.text.clone(),
                _ => {
                    return Err(PatternError::Unsupported(
                        "a multi after an atom that ends differently each time".into(),
                    ));
                }
            }
        };
        if can_skip && can_run && once.bare == bare && (!can_repeat || rest == once.text) {
            // Skipped or not, it ends the same, and every time is the same.
            return Ok(vec![Path {
                text: format!("(?:{}){}", once.text, multi_syntax(multi)),
                bare: once.bare,
            }]);
        }
        let times = |lo: u32, hi: Option<u32>| match (lo, hi) {
            (lo, Some(hi)) if lo == hi => format!("{{{lo}}}"),
            (lo, Some(hi)) => format!("{{{lo},{hi}}}{}", if lazy { "?" } else { "" }),
            (lo, None) => format!("{{{lo},}}{}", if lazy { "?" } else { "" }),
        };
        // At least once.
        let run = if !can_repeat {
            format!("(?:{})", once.text)
        } else if rest == once.text {
            format!("(?:{}){}", once.text, times(min.max(1), max))
        } else {
            let more = times(min.max(1) - 1, max.map(|max| max - 1));
            format!("(?:{})(?:{rest}){more}", once.text)
        };
        if can_skip && can_run && once.bare == bare {
            // Skipped or not, it ends the same.
            return Ok(vec![Path {
                text: format!("(?:{run}){}", if lazy { "??" } else { "?" }),
                bare,
            }]);
        }
        let skip = Path {
            text: String::new(),
            bare,
        };
        let run = Path {
            text: run,
            bare: once.bare,
        };
        Ok(match (can_skip, can_run, lazy) {
            (true, true, true) => vec![skip, run],
            (true, true, false) => vec![run, skip],
            (true, false, _) => vec![skip],
            (false, ..) => vec![run],
        })
    }

    /// One atom, which starts at `self.next`, as it translates after a bare
    /// base or not (`after_bare`); and whether it was a `^`.
    fn atom(
        &mut self,
        opened: Opened,
        first: bool,
        star_here: bool,
        after_bare: bool,
    ) -> Result<(Vec<Path>, bool), PatternError> {
        let token = self.peek().cloned().expect("an atom to translate");
        self.next += 1;
        let mut start_of_line = false;
        let mut bare = false;
        let text = match token {
            Token::Alt | Token::Close | Token::Flag => unreachable!("not an atom"),
            Token::Multi {
                multi: Multi::Star,
                bare: true,
            } if first && opened == Opened::Group => {
                return Err(PatternError::Unsupported("* after \\%(".into()));
            }
            Token::Multi {
                multi: Multi::Star,
                bare: true,
            } if first || star_here => regex::escape("*"),
            Token::Multi { .. } => return Err(PatternError::Invalid("misplaced multi")),
            // A mark here follows no atom that takes characters, and one
            // that starts a branch is the same: any character carrying it.
            Token::Char(mark) if is_mark(mark) => {
                self.next -= 1;
                let marks = self.written_marks();
                format!(r"\P{{M}}{}", carrying(&marks)?)
            }
            Token::Char(ch) => {
                // Alone, a base matches only itself without marks: the
                // boundary after the pattern or the next atom sees to it.
                // With marks written it isn't folded, as in nvim.
                let marks = self.written_marks();
                bare = marks.is_empty();
                let folded = self.ignore_case.then(|| folds().group_of.get(&ch));
                match folded {
                    Some(Some(&group)) if bare => {
                        let group = &folds().groups[group];
                        let members = group.iter().map(|ch| format!("\\x{{{:x}}}", *ch as u32));
                        format!("[{}]", members.collect::<String>())
                    }
                    _ if bare => regex::escape(ch.encode_utf8(&mut [0; 4])),
                    _ => format!(
                        "{}{}",
                        regex::escape(ch.encode_utf8(&mut [0; 4])),
                        carrying(&marks)?
                    ),
                }
            }
            Token::Any => self.character(r"[^\n\p{M}]", after_bare.then_some(r"\p{M}"), true)?,
            Token::Class(class) => match class.strip_prefix("[^") {
                Some(members) => self.character(
                    &format!(r"[^\p{{M}}{members}"),
                    after_bare
                        .then(|| format!(r"[\p{{M}}&&[^{members}]"))
                        .as_deref(),
                    false,
                )?,
                None => self.character(class, None, false)?,
            },
            Token::Set(set) => self.character(
                &set.syntax(self.ignore_case, false),
                after_bare
                    .then(|| set.syntax(self.ignore_case, true))
                    .as_deref(),
                true,
            )?,
            Token::WordStart => r"\b{start}".into(),
            Token::WordEnd => r"\b{end}".into(),
            Token::Caret { anywhere } if anywhere || first => {
                start_of_line = true;
                "^".into()
            }
            Token::Caret { .. } => regex::escape("^"),
            Token::Dollar { anywhere } => {
                let mut rest = self.tokens[self.next..].iter();
                let last = match rest.find(|token| **token != Token::Flag) {
                    None | Some(Token::Alt | Token::Close) => true,
                    Some(_) => false,
                };
                if anywhere || last {
                    "$".into()
                } else {
                    regex::escape("$")
                }
            }
            Token::Open { capture } => return Ok((self.group(capture, after_bare)?, false)),
        };
        Ok((vec![Path { text, bare }], start_of_line))
    }

    /// A group's contents after its `\(` or `\%(`, through its `\)`, as
    /// the ways it can end: one for each run of alternatives that end after
    /// a bare base, or not, together.
    fn group(&mut self, capture: bool, bare: bool) -> Result<Vec<Path>, PatternError> {
        if capture {
            self.groups += 1;
            if self.groups > MAX_GROUPS {
                return Err(PatternError::Invalid("too many \\("));
            }
        }
        let number = self.groups;
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(PatternError::TooLarge("groups nested too deeply".into()));
        }
        let inner = self.alternation(
            if capture {
                Opened::Capture
            } else {
                Opened::Group
            },
            bare,
        )?;
        // `alternation` returned at the closing `\)`.
        self.next += 1;
        self.depth -= 1;
        // `\(` captures, for `:s`'s `\1` to `\9`; the copies of its name
        // are made unique by [`unique_names`].
        Ok(join_paths(inner, false)
            .into_iter()
            .map(|path| Path {
                text: match capture {
                    true => format!("(?P<vimgroup{number}>{})", path.text),
                    false => format!("(?:{})", path.text),
                },
                bare: path.bare,
            })
            .collect())
    }
}

/// Whether `text`, a translation, can match the empty string.
fn matches_empty(text: &str) -> bool {
    Regex::new(&format!("^(?:{})$", unique_names(text))).is_ok_and(|regex| regex.is_match(""))
}

fn nothing(bare: bool) -> Path {
    Path {
        text: String::new(),
        bare,
    }
}

/// `paths` with each run of neighbours that end the same way made one
/// path, their texts alternatives (in a group of their own if `wrap`).
fn join_paths(paths: Vec<Path>, wrap: bool) -> Vec<Path> {
    let mut joined: Vec<(Vec<String>, bool)> = Vec::new();
    for path in paths {
        match joined.last_mut() {
            Some((texts, bare)) if *bare == path.bare => texts.push(path.text),
            _ => joined.push((vec![path.text], path.bare)),
        }
    }
    joined
        .into_iter()
        .map(|(mut texts, bare)| Path {
            text: match texts.len() {
                1 => texts.remove(0),
                _ if wrap => format!("(?:{})", texts.join("|")),
                _ => texts.join("|"),
            },
            bare,
        })
        .collect()
}

/// `source` with each `(?P<vimgroupN>` renamed `(?P<vN_k>`, `k` counting
/// the copies of group `N` from 0, so `regex` takes every one.
fn unique_names(source: &str) -> String {
    const OPEN: &str = "(?P<vimgroup";
    let mut seen = [0usize; MAX_GROUPS + 1];
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at]);
        let digits = &rest[at + OPEN.len()..];
        let len = digits.find('>').expect("a group name ends");
        let number: usize = digits[..len].parse().expect("a group number");
        out.push_str(&format!("(?P<v{number}_{}>", seen[number]));
        seen[number] += 1;
        rest = &digits[len + 1..];
    }
    out.push_str(rest);
    out
}

fn multi_syntax(multi: Multi) -> String {
    match multi {
        Multi::Star => "*".into(),
        Multi::Plus => "+".into(),
        Multi::Optional => "?".into(),
        Multi::Brace { min, max, lazy } => {
            let lazy = if lazy { "?" } else { "" };
            match max {
                Some(max) if max == min => format!("{{{min}}}"),
                Some(max) => format!("{{{min},{max}}}{lazy}"),
                None => format!("{{{min},}}{lazy}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Rows` whose rows are all listed already still stops at an expired
    /// deadline, whichever way it steps.
    #[test]
    fn rows_step_polls_the_clock_through_listed_rows() {
        let text = Text::new("a\na\na");
        let pattern = Pattern::parse("a").unwrap();
        let listed = |row: u32| {
            let mut list = RowList::new(&text, row);
            list.list_until(&pattern, &Deadline::new(), |_, _| false)
                .unwrap();
            assert!(list.done());
            list
        };
        for (from, forward) in [(0, true), (4, false)] {
            let deadline = Deadline::within(Duration::ZERO);
            let mut rows = Rows {
                pattern: &pattern,
                text: &text,
                deadline: &deadline,
                listed: (0..3).map(|row| (row, listed(row))).collect(),
            };
            assert_eq!(
                rows.step(from, forward),
                Err(Interrupted),
                "forward {forward}"
            );
        }
        let deadline = Deadline::within(Duration::ZERO);
        assert_eq!(pattern.nth_by(&text, 0, true, 3, &deadline), None);
    }

    /// A row isn't read once the budget is spent.
    #[test]
    fn matches_reaching_spends_its_budget_before_reading_the_row() {
        let text = Text::new("aaa");
        let pattern = Pattern::parse("a").unwrap();
        ROWS_SEARCHED.set(0);
        let found = reaching(
            &text,
            0,
            0..3,
            &pattern,
            9,
            &Deadline::within(Duration::ZERO),
        );
        assert!(found.is_empty());
        assert_eq!(ROWS_SEARCHED.get(), 0);
    }

    /// `Rows::step` stops when listing a row ran out of time, before it
    /// reads another, whichever way it goes.
    #[test]
    fn rows_step_stops_at_expiry_before_reading_another_row() {
        let text = Text::new("a\nbbbb\nbbbb\na");
        let pattern = Pattern::parse("a").unwrap();
        for (from, forward) in [(0, true), (text.len(), false), (7, true), (7, false)] {
            let deadline = Deadline::within(Duration::ZERO);
            let mut rows = Rows {
                pattern: &pattern,
                text: &text,
                deadline: &deadline,
                listed: HashMap::new(),
            };
            ROWS_SEARCHED.set(0);
            assert_eq!(rows.step(from, forward), Err(Interrupted));
            assert_eq!(ROWS_SEARCHED.get(), 1, "from {from}, forward {forward}");
        }
    }

    /// A pattern matched by backtracking stops at the deadline, however
    /// many ways the row can be tried.
    #[test]
    fn backtracking_stops_at_the_deadline() {
        let pattern = Pattern::parse(r"\(a\|.\|a\)\{0,}c").unwrap();
        assert!(matches!(pattern.engine, Engine::Backtrack(_)));
        let hay = "a".repeat(60);
        let deadline = Deadline::within(Duration::from_millis(20));
        assert_eq!(pattern.matches(&hay, &deadline).next(), None);
        assert!(deadline.ran_out());
    }

    /// A limit past `Instant`'s range never runs out, instead of at once.
    #[test]
    fn a_limit_too_long_to_count_never_runs_out() {
        let pattern = Pattern::parse("a").unwrap();
        for deadline in [Deadline::within(Duration::MAX), Deadline::never()] {
            assert!(!deadline.expired());
            assert_eq!(pattern.matches("aa", &deadline).count(), 2);
            assert!(!deadline.ran_out());
        }
    }

    /// A counted repeat of nothing, or of many distinct composing leaves,
    /// is refused or compiled at once, not worked through.
    #[test]
    fn backtracking_patterns_are_bounded_to_build() {
        for pattern in [
            r"\%(a\|.\)*\%(\)\{4294967295}".to_string(),
            r"\%(a\|.\)*\%(\%(\%(\)\{99999}\)\{99999}\)\{99999}".to_string(),
        ] {
            assert!(Pattern::parse(&pattern).is_ok(), "{pattern}");
        }
        let marks: String = ('\u{4e00}'..)
            .take(3000)
            .map(|ch| format!("{ch}\u{301}"))
            .collect();
        let many = format!(r"\%(a\|.\)*{marks}");
        assert!(matches!(
            Pattern::parse(&many),
            Err(PatternError::TooLarge(_))
        ));
    }
}

#[cfg(test)]
#[path = "tests/search_incremental.rs"]
mod incremental_tests;
