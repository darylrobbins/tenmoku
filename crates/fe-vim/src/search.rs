//! Search patterns: Vim's pattern language, translated to a [`regex::Regex`]
//! and matched one row at a time. The crate docs list what translates.
//! [`Pattern::parse_across`] also translates the atoms that match a line
//! break (`\n`, `\_x`, `\_.`, `\_[...]`), and [`Pattern::find_across`]
//! matches a pattern holding one over a window of rows at a time, as
//! [`matches_across_until`] lists its matches for a host to paint.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use fe_text::{Bias, Text};
use kiln_l10n::tr;
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

thread_local! {
    // How many regex calls [`RowMatches`] has made on this thread.
    static REGEX_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };

    // The regex-call budget for [`set_call_limit`]; `u64::MAX` for none.
    static CALL_LIMIT: std::cell::Cell<u64> = const { std::cell::Cell::new(u64::MAX) };

    // The work units [`Deadline::work`] has been given on this thread.
    static WORK_DONE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };

    // The work budget for [`set_work_limit`]; `u64::MAX` for none.
    static WORK_LIMIT: std::cell::Cell<u64> = const { std::cell::Cell::new(u64::MAX) };
}

/// How many work units a [`Deadline`] lets pass between samples of the clock.
const WORK_PER_SAMPLE: u64 = 256;

/// How many work units [`Deadline::work`] has counted on this thread. For
/// tests, as [`regex_calls`] is.
#[doc(hidden)]
pub fn work_done() -> u64 {
    WORK_DONE.with(|work| work.get())
}

/// Gives each [`Deadline`] made from now on, on this thread, a budget of
/// `limit` work units (the bytes and pieces a `:s` preview's replacement
/// handles), or none with `None`: it passes once that many have been counted
/// since it started, as if its time had run out. Like [`set_call_limit`], for
/// tests that need an interruption at a counted point of the work.
#[doc(hidden)]
pub fn set_work_limit(limit: Option<u64>) {
    WORK_LIMIT.with(|cell| cell.set(limit.unwrap_or(u64::MAX)));
}

/// How many regex calls searches have made so far on this thread, each
/// finding one match or finding none more on a row. For tests to count the
/// work a search does instead of timing it: they take it before and after.
/// Per thread, so another test's searches don't add to it.
#[doc(hidden)]
pub fn regex_calls() -> u64 {
    REGEX_CALLS.with(|calls| calls.get())
}

/// Gives each [`Deadline`] made from now on, on this thread, a budget of
/// `limit` regex calls, or none with `None` (the default): it passes once that
/// many calls have been made since it started, as if its time had run out. For
/// tests that need an interruption at a counted point of the work instead of
/// a measured time; nothing else sets it. Per thread, so it doesn't cut the
/// searches of tests running alongside.
#[doc(hidden)]
pub fn set_call_limit(limit: Option<u64>) {
    CALL_LIMIT.with(|cell| cell.set(limit.unwrap_or(u64::MAX)));
}

/// Sets how long a `:s` or a search may spend matching before it stops with
/// `Interrupted`, process-wide: [`DEFAULT_TIME_LIMIT`] until a host or a test
/// changes it. Each command reads it once, when it starts, so a change
/// applies from the next command.
///
/// The limit is checked between regex calls, each of which scans at most
/// one row (a window of rows for [`Pattern::find_across`]), so a command
/// can overrun it by one such scan. A command that passes it stops: `:s` changes nothing ([`crate::substitute`]'s `run`)
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
        f.write_str(&tr!("vim-interrupted"))
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
    /// What says it has passed whatever the time: see [`Deadline::cancelled_by`].
    cancelled: Option<Box<dyn Fn() -> bool + Send>>,
    /// The [`work_done`] count it passes at, from [`set_work_limit`].
    work_end: Option<u64>,
    /// Work counted since the clock was last sampled.
    unsampled: std::cell::Cell<u64>,
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
            cancelled: None,
            work_end: None,
            unsampled: std::cell::Cell::new(0),
            ran_out: std::cell::Cell::new(false),
        }
    }

    /// A deadline with no time limit that passes once `cancelled` says so,
    /// which it asks before every regex call, those a match iteration makes
    /// for candidates it rejects too, so stale background work stops within a
    /// row.
    pub fn cancelled_by(cancelled: impl Fn() -> bool + Send + 'static) -> Self {
        Self {
            cancelled: Some(Box::new(cancelled)),
            ..Self::never()
        }
    }

    /// A deadline `limit` from now: one that's past [`Instant`]'s range
    /// (`Duration::MAX`) never passes.
    pub(crate) fn within(limit: Duration) -> Self {
        let calls_end = match CALL_LIMIT.with(|cell| cell.get()) {
            u64::MAX => None,
            calls => regex_calls().checked_add(calls),
        };
        Self {
            end: Instant::now().checked_add(limit),
            calls_end,
            cancelled: None,
            work_end: match WORK_LIMIT.with(|cell| cell.get()) {
                u64::MAX => None,
                work => work_done().checked_add(work),
            },
            unsampled: std::cell::Cell::new(0),
            ran_out: std::cell::Cell::new(false),
        }
    }

    /// Whether it is past, which it records.
    fn passed(&self) -> bool {
        if !self.ran_out.get()
            && (self.end.is_some_and(|end| Instant::now() >= end)
                || self.calls_end.is_some_and(|end| regex_calls() >= end)
                || self.cancelled.as_ref().is_some_and(|cancelled| cancelled())
                || self.work_end.is_some_and(|end| work_done() >= end))
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

    /// Counts `units` of work that make no regex call, sampling the clock
    /// once [`WORK_PER_SAMPLE`] have gone by since it last was: `Err` if it
    /// ran out.
    pub(crate) fn work(&self, units: usize) -> Result<(), Interrupted> {
        let units = units as u64;
        WORK_DONE.with(|work| work.set(work.get().saturating_add(units)));
        let unsampled = self.unsampled.get().saturating_add(units);
        // A work limit is counted exactly, the clock in batches.
        if unsampled >= WORK_PER_SAMPLE || self.work_end.is_some() {
            self.unsampled.set(0);
            self.passed();
        } else {
            self.unsampled.set(unsampled);
        }
        match self.ran_out() {
            true => Err(Interrupted),
            false => Ok(()),
        }
    }

    /// `Err` if it ran out, sampling the clock to find out.
    pub(crate) fn check(&self) -> Result<(), Interrupted> {
        match self.passed() {
            true => Err(Interrupted),
            false => Ok(()),
        }
    }
}

/// A search pattern in Vim's syntax, translated to a [`Regex`] that matches
/// within one row, or for one that [`Pattern::spans_rows`] over several.
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
    /// How many line breaks a match may take.
    reach: Reach,
}

/// How many line breaks a [`Pattern`]'s match may take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reach {
    /// It matches within one row: none.
    Row,
    /// It [`Pattern::spans_rows`], with at most this many: a match starting
    /// on a row ends by this many rows on.
    Breaks(u32),
    /// It spans rows, with no bound.
    Unbounded,
}

/// The most rows past its first a [`Reach::Breaks`] may reach before the
/// pattern is matched as [`Reach::Unbounded`], against the rest of the text.
const MAX_BREAKS: u32 = 100_000;

/// How many line breaks a match of `tokens` (a multi-row pattern's) may
/// take at most: each of `\n`, `\_x`, `\_.` and `\_[...]` one, a
/// sequence their sum, alternatives their most, and a repeat its count of
/// them, unbounded for `*`, `\+` and `\{n,}` of one that takes any.
fn reach(tokens: &[Token]) -> Reach {
    /// One group's branches: the most breaks of those ended, then of the
    /// one going on, and of its last piece (`None` for no bound).
    struct Level {
        most: Option<u32>,
        branch: Option<u32>,
        last: Option<u32>,
    }
    const NEW: Level = Level {
        most: Some(0),
        branch: Some(0),
        last: Some(0),
    };
    let max = |a: Option<u32>, b: Option<u32>| a.zip(b).map(|(a, b)| a.max(b));
    let add = |a: Option<u32>, b: Option<u32>| a.zip(b).map(|(a, b)| a.saturating_add(b));
    let piece = |level: &mut Level, breaks: Option<u32>| {
        level.branch = add(level.branch, breaks);
        level.last = breaks;
    };
    let mut levels = vec![NEW];
    for token in tokens {
        let nested = levels.len() > 1;
        let level = levels.last_mut().expect("the pattern's level");
        match token {
            Token::Open { .. } => levels.push(NEW),
            Token::Close if nested => {
                let group = levels.pop().expect("an open group");
                let level = levels.last_mut().expect("the pattern's level");
                piece(level, max(group.most, group.branch));
            }
            Token::Alt => {
                level.most = max(level.most, level.branch);
                (level.branch, level.last) = (Some(0), Some(0));
            }
            &Token::Multi { multi, .. } => {
                let most = match multi {
                    Multi::Star | Multi::Plus => None,
                    Multi::Optional => Some(1),
                    Multi::Brace { max, .. } => max,
                };
                // The piece before, repeated: written out, it was there once.
                let repeated = match (level.last, most) {
                    (Some(0), _) => Some(0),
                    (Some(last), Some(most)) => Some(last.saturating_mul(most)),
                    _ => None,
                };
                level.branch = level
                    .branch
                    .zip(level.last)
                    .and_then(|(branch, last)| add(Some(branch - last), repeated));
                level.last = repeated;
            }
            Token::Newline | Token::OrNewline(_) => piece(level, Some(1)),
            _ => piece(level, Some(0)),
        }
    }
    let level = levels.swap_remove(0);
    match max(level.most, level.branch) {
        Some(breaks) if breaks <= MAX_BREAKS => Reach::Breaks(breaks),
        _ => Reach::Unbounded,
    }
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
        /// The capture groups behind each `\zs` and `\ze` written (a
        /// copy for each time the translation wrote one), in the order
        /// written.
        zs: Vec<usize>,
        ze: Vec<usize>,
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

/// Why [`Pattern::parse`] refused a pattern. Each holds its whole message,
/// in the active locale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternError {
    /// An atom of Vim's that fe-vim doesn't translate
    /// ([`PatternError::unsupported`]), or what it doesn't do.
    Unsupported(String),
    /// A pattern Vim rejects too.
    Invalid(String),
    /// A [`FindQuery`]'s regex that doesn't compile, with the first line of
    /// `regex`'s message.
    Regex(String),
    /// Past `regex`'s size limit, or nested too deeply.
    TooLarge(String),
}

impl PatternError {
    /// [`PatternError::Unsupported`] for `atom`, as typed.
    pub fn unsupported(atom: &str) -> Self {
        Self::Unsupported(tr!("vim-pattern-not-supported", atom = atom).into())
    }
}

impl std::error::Error for PatternError {}

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (Self::Unsupported(message)
        | Self::Invalid(message)
        | Self::Regex(message)
        | Self::TooLarge(message)) = self;
        f.write_str(message)
    }
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
        Self::compile(pattern, ignore_case, false)
    }

    /// [`Pattern::parse_with`], also reading the atoms that match a line
    /// break, as `:help /\_x` has them: `\n`; `\_s`, `\_a` and the rest of
    /// the backslash classes, and `\_.`, each the class or a line break;
    /// `\_[...]`, and a collection holding `\n` (but for one starting `^`,
    /// where nvim ignores it); `\_^` and `\_$`, the start and end of a row
    /// anywhere; and `^` after `\n` and `$` before it, as at a branch's
    /// start and end. A pattern holding any of them [`Pattern::spans_rows`],
    /// and only [`Pattern::find_across`] (with the searches of `/`, `?`, `n`
    /// and `N`) and [`matches_across_until`] match it over rows; the other
    /// searches match within one row, as they do every pattern.
    pub fn parse_across(pattern: &str, ignore_case: bool) -> Result<Self, PatternError> {
        Self::compile(pattern, ignore_case, true)
    }

    /// Whether the pattern holds an atom that matches a line break, or `\_^`
    /// or `\_$` ([`Pattern::parse_across`]).
    pub fn spans_rows(&self) -> bool {
        self.reach != Reach::Row
    }

    fn compile(pattern: &str, ignore_case: bool, across: bool) -> Result<Self, PatternError> {
        if pattern.is_empty() {
            return Err(PatternError::Invalid(tr!("vim-pattern-empty").into()));
        }
        let (mut tokens, case, rows) = Lexer::new(pattern, across).run()?;
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
            rows,
        };
        let source = unique_names(&translator.pattern()?);
        let reach = match rows {
            true => reach(&tokens),
            false => Reach::Row,
        };
        // A multi-row pattern's `^` and `$` are a row's start and end
        // anywhere in the haystack.
        let rows_key = if rows { "\0rows" } else { "" };
        if translator.inexact {
            return Ok(Self {
                engine: Engine::Backtrack(Arc::new(backtrack::compile(
                    &tokens,
                    ignore_case,
                    rows,
                )?)),
                word,
                whole: false,
                key: format!("{source}\0{ignore_case}\0backtrack\0{word:?}{rows_key}"),
                reach,
            });
        }
        let mut builder = RegexBuilder::new(&source);
        builder.multi_line(rows);
        let regex = build(builder, |error| {
            // Past the nesting limit: its message's last line says so.
            PatternError::TooLarge(
                tr!(
                    "vim-pattern-too-large-regex",
                    message = error.to_string().lines().last().unwrap_or_default().trim()
                )
                .into(),
            )
        })?;
        let mut slots = vec![vec![1]];
        slots.resize(MAX_GROUPS + 1, Vec::new());
        let (mut zs, mut ze) = (Vec::new(), Vec::new());
        for (at, name) in regex.capture_names().enumerate() {
            match name {
                Some(name) if name.starts_with("zs_") => zs.push(at),
                Some(name) if name.starts_with("ze_") => ze.push(at),
                _ => {}
            }
            let number = name
                .and_then(|name| name.strip_prefix('v'))
                .and_then(|name| name.split('_').next())
                .and_then(|number| number.parse::<usize>().ok());
            if let Some(number) = number {
                slots[number].push(at);
            }
        }
        Ok(Self {
            engine: Engine::Regex {
                regex,
                slots,
                zs,
                ze,
            },
            word,
            whole: false,
            key: format!("{source}\0{ignore_case}\0{word:?}{rows_key}"),
            reach,
        })
    }

    /// The pattern for a find bar's `query`: its text literal, or with
    /// `regex` in the `regex` crate's syntax (never Vim's), case-insensitive
    /// unless `match_case`. Matching is row by row, so a `\n` in a regex
    /// never matches. With `whole_word` a match is kept only between
    /// characters that aren't word characters (Unicode `\w`).
    pub fn from_query(query: &FindQuery) -> Result<Self, PatternError> {
        if query.text.is_empty() {
            return Err(PatternError::Invalid(tr!("vim-pattern-empty").into()));
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
            PatternError::Regex(
                tr!(
                    "vim-pattern-invalid-regex",
                    message = message.lines().next().unwrap_or_default()
                )
                .into(),
            )
        })?;
        let slots = (0..regex.captures_len()).map(|at| vec![at]).collect();
        Ok(Self {
            engine: Engine::Regex {
                regex,
                slots,
                zs: Vec::new(),
                ze: Vec::new(),
            },
            word: (false, false),
            whole: query.whole_word,
            key: format!("{source}\0{}\0{}", query.match_case, query.whole_word),
            reach: Reach::Row,
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
            first = in_time(self.matches(hay, deadline).next(), deadline)?;
            if let Some(found) = &first
                && found.start >= column
            {
                return Ok(Some(shift(found.clone(), start)));
            }
            if first.is_some()
                && let Some(found) =
                    in_time(self.matches_from(hay, column, deadline).next(), deadline)?
            {
                return Ok(Some(shift(found, start)));
            }
        } else {
            let mut matches = self.matches(hay, deadline);
            while let Some(found) = in_time(matches.next(), deadline)? {
                if found.start >= past && !on_last {
                    return Ok(Some(shift(found, start)));
                }
                first.get_or_insert(found);
                if on_last {
                    break;
                }
            }
        }
        // What `from`'s row offers once the search wraps back to it.
        let wrapped = first.map(|found| shift(found, start));
        for row in (at.row + 1..rows).chain(0..at.row) {
            let (start, hay) = row_text(text, row, &mut buf);
            if let Some(found) = in_time(self.matches(hay, deadline).next(), deadline)? {
                return Ok(Some(shift(found, start)));
            }
        }
        Ok(wrapped)
    }

    /// The match `/` (`forward`) or `?` would go to from `from`, as nvim's
    /// search finds it, for a pattern that [`Pattern::spans_rows`]: one
    /// that may start on a row and end on a later one. The text's rows are
    /// matched as one haystack, with a line break after each, the last row
    /// included, as nvim matches one there as if the text ended in one: a
    /// match that takes it ends one past the text's end.
    ///
    /// A row's matches are those starting on it (at its end too, before
    /// its line break), found from its start, each search resuming where
    /// the last match ended, and none after one that ends on a later row.
    /// Going forward the match is the first on `from`'s row starting a
    /// whole character on from `from` (from its last character, only one
    /// that `\zs` starts on a later row), else the first on a later row,
    /// else, wrapping, the first from the text's start; going back, the
    /// last starting before `from` on its row or an earlier one, else,
    /// wrapping, the last from the text's end. A match `\zs` starts after
    /// the last row's line break starts one past the text's end, where nvim
    /// puts the cursor on the text's last character.
    ///
    /// Rows are read a window at a time, holding as many rows past the
    /// first a match may start on as its matches can reach (for one with
    /// no bound, like `\_s\+`, the rest of the text), so a search for the
    /// next row with a match scans a window at most, and one that finds
    /// none goes on from the first row it couldn't rule out: for a pattern
    /// with a bound the rows are scanned a bounded number of times each.
    /// A match attempt may read as far as its matches can reach, so a
    /// pattern like `a\_.*b` reads on to the text's end from each row with
    /// an `a`, as in nvim. `Err(Interrupted)` once `deadline` has passed,
    /// which is read between regex calls. A pattern that doesn't span rows
    /// is searched as [`Pattern::find`] searches.
    pub fn find_across(
        &self,
        text: &Text,
        from: usize,
        forward: bool,
        deadline: &Deadline,
    ) -> Result<Option<Range<usize>>, Interrupted> {
        if self.reach == Reach::Row {
            return self.try_find_by(text, from, forward, deadline);
        }
        let found = Across::new(self, text, deadline).next(from, forward, false)?;
        Ok(found.map(|(found, _)| found))
    }

    /// A walk from match to match of a pattern that [`Pattern::spans_rows`],
    /// as [`Pattern::find_across`] searches, its rows read once for every
    /// search it makes, under `deadline`.
    pub(crate) fn across<'a>(&'a self, text: &'a Text, deadline: &'a Deadline) -> Across<'a> {
        Across::new(self, text, deadline)
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
            row_end: hay.len(),
            beyond: None,
            across: false,
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
            emitted: 0,
            bounds: 0..0,
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

/// `found`, what a regex call gave, unless `deadline` has passed, which it
/// reads: a call that overran it counts for nothing, whether it found a
/// match or none.
fn in_time<T>(found: T, deadline: &Deadline) -> Result<T, Interrupted> {
    match deadline.expired() {
        true => Err(Interrupted),
        false => Ok(found),
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

/// The searches of a pattern that [`Pattern::spans_rows`]
/// ([`Pattern::find_across`]), reading the text into one [`Window`] for all
/// of them, under one deadline, and listing each row's matches once for all
/// of them, as far as they have needed: a count's steps along a row go on
/// from where the last stopped.
pub(crate) struct Across<'a> {
    pattern: &'a Pattern,
    window: Window,
    deadline: &'a Deadline,
    /// The rows listed, but for those found to have no match.
    rows: HashMap<u32, Listed>,
    /// The rows found to have no match, as stretches: each start's end.
    /// Kept as stretches rather than a row each, so a search that passes a
    /// row with none, which one regex call can rule out with the rows
    /// after it, keeps no more than one with a match does.
    none: BTreeMap<u32, u32>,
}

/// What an [`Across`] knows of a row without listing it again.
enum Told {
    /// It has a match listed.
    Matches,
    /// It has none, nor has any row from the first given on.
    None(u32),
    Unknown,
}

/// A row's matches as far as an [`Across`] has listed them, in the text.
struct Listed {
    found: Vec<Range<usize>>,
    /// Where the row's next search starts, `None` once it has none left.
    resume: Option<usize>,
    /// Where the last match listed ended as `\zs` and `\ze` bound it, which
    /// the next may not start before.
    emitted: usize,
}

impl<'a> Across<'a> {
    fn new(pattern: &'a Pattern, text: &'a Text, deadline: &'a Deadline) -> Self {
        Self {
            pattern,
            window: Window::new(text, pattern.reach),
            deadline,
            rows: HashMap::new(),
            none: BTreeMap::new(),
        }
    }

    /// The match `/` (`forward`) or `?` goes to from `from`, as
    /// [`Pattern::find_across`] finds it, and whether the search went round
    /// an end of the text for it. With `by_end` (an `e` offset's first
    /// search, as nvim's) matches compare by their last character, not
    /// their start: going forward, the first on `from`'s row that ends past
    /// the start of the character after `from` (any that ends on a later
    /// row) is taken; going back, the last that ends at or before `from`.
    /// Going back from a row's start, the search starts on the row before,
    /// as nvim's does; gone round the text's end, it takes any match from
    /// the last row back to `from`'s.
    pub(crate) fn next(
        &mut self,
        from: usize,
        forward: bool,
        by_end: bool,
    ) -> Result<Option<(Range<usize>, bool)>, Interrupted> {
        let text = &self.window.text.clone();
        let at = text.offset_to_point(text.clip_offset(from.min(text.len()), Bias::Left));
        let from = text.point_to_offset(at);
        let rows = text.row_count();
        if !forward {
            let before = |found: &Range<usize>| match by_end {
                true => found.end <= from,
                false => found.start < from,
            };
            if at.column > 0 {
                let (found, _) = self.list_row(at.row, |found| {
                    found.last().is_some_and(|last| !before(last))
                })?;
                if let Some(ix) = found.partition_point(before).checked_sub(1) {
                    return Ok(Some((found[ix].clone(), false)));
                }
            }
            if let Some(found) = self.last_in_rows(0..at.row, before)? {
                return Ok(Some((found, false)));
            }
            // Gone round, the search stops at the row it started on, as
            // nvim's does: rows above it it has searched already.
            let found = self.last_in_rows(at.row..rows, |_| true)?;
            return Ok(found.map(|found| (found, true)));
        }
        self.window.hold(at.row, self.deadline)?;
        let (start, end) = (text.row_start(at.row), row_end(text, at.row));
        let row = &self.window.hay[start - self.window.base..end - self.window.base];
        let column = from - start;
        let past = start + column + unit(&row[column..]).unwrap_or(0);
        // From the row's last character only a match that `\zs` starts on
        // a later row is past it, or with `by_end` one that ends on one.
        let taken = |found: &Range<usize>| match by_end {
            true => found.end > past,
            false => (found.start >= past && past < end) || found.start > end,
        };
        let (found, _) = self.list_row(at.row, |found| found.last().is_some_and(taken))?;
        // Matches go on in order, by start and end alike.
        if let Some(found) = found.get(found.partition_point(|found| !taken(found))) {
            return Ok(Some((found.clone(), false)));
        }
        if let Some(found) = self.first_in_rows(at.row + 1..rows)? {
            return Ok(Some((found, false)));
        }
        let found = self.first_in_rows(0..at.row + 1)?;
        Ok(found.map(|found| (found, true)))
    }

    /// The text's first match (`forward`) or its last.
    pub(crate) fn outermost(&mut self, forward: bool) -> Result<Option<Range<usize>>, Interrupted> {
        let rows = self.window.text.row_count();
        match forward {
            true => self.first_in_rows(0..rows),
            false => self.last_in_rows(0..rows, |_| true),
        }
    }

    /// The first match on the first of `rows` with one.
    fn first_in_rows(&mut self, rows: Range<u32>) -> Result<Option<Range<usize>>, Interrupted> {
        let mut row = rows.start;
        while row < rows.end {
            let (found, next) = self.list_row(row, |found| !found.is_empty())?;
            if let Some(found) = found.first() {
                return Ok(Some(found.clone()));
            }
            match next {
                Some(next) => row = next,
                None => break,
            }
        }
        Ok(None)
    }

    /// The last match that is `before` the search's start on the last of
    /// `rows` with one: of a row's matches, listed in order, those up to the
    /// first that isn't. The rows are listed forward a window at a time,
    /// from the last window back.
    fn last_in_rows(
        &mut self,
        rows: Range<u32>,
        before: impl Fn(&Range<usize>) -> bool,
    ) -> Result<Option<Range<usize>>, Interrupted> {
        let mut end = rows.end;
        while end > rows.start {
            // Back over the rows listed already, by what they hold, so a
            // count's steps back don't list again from the window's start.
            match self.told(end - 1) {
                Told::Matches => {
                    let row = end - 1;
                    let (found, _) =
                        self.list_row(row, |found| found.last().is_some_and(|last| !before(last)))?;
                    if let Some(ix) = found.partition_point(&before).checked_sub(1) {
                        return Ok(Some(found[ix].clone()));
                    }
                    end = row;
                    continue;
                }
                Told::None(first) => {
                    end = first.max(rows.start);
                    continue;
                }
                Told::Unknown => {}
            }
            let start = match self.pattern.reach {
                Reach::Breaks(breaks) => {
                    end.saturating_sub(WINDOW_ROWS.max(breaks)).max(rows.start)
                }
                _ => rows.start,
            };
            let mut last = None;
            let mut row = start;
            loop {
                let (found, next) =
                    self.list_row(row, |found| found.last().is_some_and(|last| !before(last)))?;
                if let Some(ix) = found.partition_point(&before).checked_sub(1) {
                    last = Some(found[ix].clone());
                }
                match next {
                    Some(next) if next < end => row = next,
                    _ => break,
                }
            }
            if last.is_some() {
                return Ok(last);
            }
            end = start;
        }
        Ok(None)
    }

    /// `row`'s matches as [`Pattern::find_across`] lists them (as offsets
    /// in the text), as far as `enough` of them, and the next row to list:
    /// with none on `row`, the first on which one may start, as far as the
    /// window can tell; `None` past the last. `Err` if the deadline has run
    /// out, before or while listing them: it is read after every regex
    /// call, so one that overran it counts for nothing.
    fn list_row(
        &mut self,
        row: u32,
        enough: impl Fn(&[Range<usize>]) -> bool,
    ) -> Result<(&[Range<usize>], Option<u32>), Interrupted> {
        let next = self.list(row, enough)?;
        Ok((self.listed(row), next))
    }

    /// [`Across::list_row`], adding `row`'s matches to `found`, those
    /// listed before the deadline passed too if it does.
    fn list_row_onto(
        &mut self,
        row: u32,
        enough: impl Fn(&[Range<usize>]) -> bool,
        found: &mut Vec<Range<usize>>,
    ) -> Result<Option<u32>, Interrupted> {
        let next = self.list(row, enough);
        found.extend_from_slice(self.listed(row));
        next
    }

    /// What listing `row` found, if it has been listed.
    fn told(&self, row: u32) -> Told {
        if let Some(none) = self.none_around(row) {
            return Told::None(none.start);
        }
        match self.rows.get(&row) {
            Some(listed) if !listed.found.is_empty() => Told::Matches,
            _ => Told::Unknown,
        }
    }

    /// The stretch of rows with no match that holds `row`, if one does.
    fn none_around(&self, row: u32) -> Option<Range<u32>> {
        let (&first, &end) = self.none.range(..=row).next_back()?;
        (row < end).then_some(first..end)
    }

    /// Records that rows `rows` have no match, joining the stretches it
    /// meets.
    fn none_in(&mut self, rows: Range<u32>) {
        let (mut first, mut end) = (rows.start, rows.end);
        if let Some((&before, &before_end)) = self.none.range(..=first).next_back()
            && before_end >= first
        {
            first = before;
            end = end.max(before_end);
        }
        while let Some((&after, &after_end)) = self.none.range(first..=end).next() {
            self.none.remove(&after);
            end = end.max(after_end);
        }
        self.none.insert(first, end);
    }

    /// `row`'s matches listed so far.
    fn listed(&self, row: u32) -> &[Range<usize>] {
        self.rows.get(&row).map_or(&[], |listed| &listed.found)
    }

    /// Lists `row`'s matches on from where its listing last stopped, until
    /// `enough` of them or none left, and gives the next row to list
    /// ([`Across::list_row`]). A regex call that overran the deadline lists
    /// nothing and leaves the row's next search where it was. A row found
    /// to have no match isn't searched again: the next row to list is the
    /// first past the stretch of them it is in.
    fn list(
        &mut self,
        row: u32,
        enough: impl Fn(&[Range<usize>]) -> bool,
    ) -> Result<Option<u32>, Interrupted> {
        if self.deadline.expired() {
            return Err(Interrupted);
        }
        let text = &self.window.text.clone();
        if let Some(none) = self.none_around(row) {
            return Ok((none.end < text.row_count()).then_some(none.end));
        }
        let mut listed = self.rows.remove(&row).unwrap_or_else(|| Listed {
            found: Vec::new(),
            resume: Some(text.row_start(row)),
            emitted: 0,
        });
        let next = self.list_on(row, &mut listed, enough);
        let ruled_out = listed.found.is_empty() && listed.resume.is_none();
        if !ruled_out {
            self.rows.insert(row, listed);
        }
        let next = next?.max(row + 1).min(text.row_count());
        // A row with no match is kept with the rows up to the next that
        // may have one, which the same call ruled out.
        if ruled_out {
            self.none_in(row..next);
        }
        Ok((next < text.row_count()).then_some(next))
    }

    /// [`Across::list`] on `listed`, `row`'s, giving the next row to list,
    /// which may be before `row + 1`.
    fn list_on(
        &mut self,
        row: u32,
        listed: &mut Listed,
        enough: impl Fn(&[Range<usize>]) -> bool,
    ) -> Result<u32, Interrupted> {
        let Some(at) = listed.resume.filter(|_| !enough(&listed.found)) else {
            return Ok(row + 1);
        };
        let window = &mut self.window;
        window.hold(row, self.deadline)?;
        let (text, base) = (&window.text, window.base);
        let mut matches = self
            .pattern
            .matches_from(&window.hay, at - base, self.deadline);
        matches.row_end = row_end(text, row) - base;
        matches.across = true;
        matches.emitted = listed.emitted.saturating_sub(base);
        loop {
            let next = matches.next();
            if self.deadline.expired() {
                return Err(Interrupted);
            }
            let Some(next) = next else {
                listed.resume = None;
                break;
            };
            listed.found.push(base + next.start..base + next.end);
            listed.resume = matches.at.map(|at| base + at);
            listed.emitted = base + matches.emitted;
            if listed.resume.is_none() || enough(&listed.found) {
                break;
            }
        }
        if !listed.found.is_empty() {
            return Ok(row + 1);
        }
        Ok(match matches.beyond {
            Some(beyond) => window.row_of(base + beyond).min(window.told_through() + 1),
            None => window.told_through() + 1,
        })
    }
}

/// How many rows a [`Window`] reads past those a row's matches can reach,
/// so that one search for the next row with a match scans that many.
const WINDOW_ROWS: u32 = 256;

/// Rows of a text read into one haystack for a pattern that spans rows
/// ([`Pattern::find_across`]): rows `first..=last`, with the line breaks
/// between them, and after the text's last row the one nvim matches there.
/// A match starting on a row is told from the rows its [`Reach`] can take
/// it to: past the window's last row the text's next line break is
/// missing, which reads as the end of the haystack does to every anchor
/// and boundary, so the line break is the only difference.
struct Window {
    text: Text,
    reach: Reach,
    /// Empty while `first > last`.
    first: u32,
    last: u32,
    /// `first`'s offset in the text.
    base: usize,
    hay: String,
}

impl Window {
    fn new(text: &Text, reach: Reach) -> Self {
        Self {
            text: text.clone(),
            reach,
            first: 1,
            last: 0,
            base: 0,
            hay: String::new(),
        }
    }

    /// The most line breaks a match takes, `None` for no bound.
    fn breaks(&self) -> Option<u32> {
        match self.reach {
            Reach::Breaks(breaks) => Some(breaks),
            Reach::Row => Some(0),
            Reach::Unbounded => None,
        }
    }

    /// Reads rows until it holds `row` and those its matches can reach (all
    /// the rest for an unbounded reach), and [`WINDOW_ROWS`] more. It starts
    /// again from `row` for a row before its first, or for one so far past
    /// it that the rows held before are as many as it would read. `Err` once
    /// `deadline` has passed, which it reads between the text's chunks: what
    /// it holds nothing then.
    fn hold(&mut self, row: u32, deadline: &Deadline) -> Result<(), Interrupted> {
        let last_row = self.text.row_count() - 1;
        let through = match self.breaks() {
            Some(breaks) => row.saturating_add(breaks).min(last_row),
            None => last_row,
        };
        let stale = match self.breaks() {
            Some(breaks) => row.saturating_sub(self.first) > breaks.saturating_add(WINDOW_ROWS),
            None => false,
        };
        if self.first > self.last || row < self.first || stale {
            self.first = row;
            self.base = self.text.row_start(row);
            self.hay.clear();
            self.read(through.saturating_add(WINDOW_ROWS).min(last_row), deadline)
        } else if through > self.last {
            self.read(through.saturating_add(WINDOW_ROWS).min(last_row), deadline)
        } else {
            Ok(())
        }
    }

    /// Reads on through row `last`, which it doesn't hold yet, unless the
    /// deadline passes first, which leaves it holding nothing.
    fn read(&mut self, last: u32, deadline: &Deadline) -> Result<(), Interrupted> {
        let from = self.base + self.hay.len();
        for chunk in self.text.chunks_in_range(from..row_end(&self.text, last)) {
            if deadline.expired() {
                (self.first, self.last) = (1, 0);
                self.hay.clear();
                return Err(Interrupted);
            }
            #[cfg(test)]
            WINDOW_CHUNKS.with(|chunks| chunks.set(chunks.get() + 1));
            self.hay.push_str(chunk);
        }
        if last + 1 == self.text.row_count() {
            self.hay.push('\n');
        }
        self.last = last;
        Ok(())
    }

    /// The last row on which where the matches start, and how, the window
    /// tells: a row whose matches can reach past it can't be told.
    fn told_through(&self) -> u32 {
        match self.breaks() {
            Some(breaks) if self.last + 1 < self.text.row_count() => {
                self.last.saturating_sub(breaks)
            }
            _ => self.last,
        }
    }

    /// The row of `at`, an offset in the text or the one after it.
    fn row_of(&self, at: usize) -> u32 {
        self.text.offset_to_point(at.min(self.text.len())).row
    }

    /// Replaces `range` of the text with `new`. What it holds takes the
    /// edit in too if the edit is all in it, else it holds nothing.
    fn edit(&mut self, range: Range<usize>, new: &str) {
        let held = self.first <= self.last
            && self.base <= range.start
            && range.end <= row_end(&self.text, self.last);
        let row = |text: &Text, at: usize| text.offset_to_point(at).row;
        let joined = row(&self.text, range.end) - row(&self.text, range.start);
        let at = range.start - self.base.min(range.start);
        self.text.replace(range.clone(), new);
        if !held {
            (self.first, self.last) = (1, 0);
            self.hay.clear();
            return;
        }
        self.hay.replace_range(at..at + range.len(), new);
        let broken = u32::try_from(new.matches('\n').count()).unwrap_or(u32::MAX);
        self.last = (self.last + broken).saturating_sub(joined);
    }
}

/// Where `row` ends in `text`, before its line break.
fn row_end(text: &Text, row: u32) -> usize {
    if row + 1 < text.row_count() {
        text.row_start(row + 1) - 1
    } else {
        text.len()
    }
}

/// `pattern` translated as a search's, ignoring case if `ignore_case` unless
/// it says otherwise, and reading the atoms that match a line break
/// ([`Pattern::parse_across`]), the last one kept on each thread: a search
/// motion asks once for each caret.
pub(crate) fn parsed(pattern: SearchPattern, ignore_case: bool) -> Result<Pattern, PatternError> {
    type Kept = ((SearchPattern, bool), Result<Pattern, PatternError>);
    thread_local! {
        static LAST: RefCell<Option<Kept>> = const { RefCell::new(None) };
    }
    let key = (pattern, ignore_case);
    LAST.with_borrow_mut(|last| match last {
        Some((kept, parsed)) if *kept == key => parsed.clone(),
        _ => last
            .insert((key, Pattern::parse_across(pattern.as_str(), ignore_case)))
            .1
            .clone(),
    })
}

/// Whether `pattern` has an uppercase letter, which turns 'ignorecase' off
/// under 'smartcase': nvim's `pat_has_uppercase()`. A letter after a
/// backslash (`\S`, `\_S`, `\%V`) doesn't count, unless the pattern is
/// very magic (`\v`) where it ends, which then skips `%X` and `_X`
/// instead. As in nvim, only `\v` and `\V` change how it reads the
/// pattern, and the last one written outside a collection decides for all
/// of it: nvim's `skip_regexp_ex()` steps over a collection (`[`, or after
/// `\V` `\[`) whole, so a `\v` or `\V` in one is no switch.
pub fn has_uppercase(pattern: &str) -> bool {
    let (mut very_magic, mut very_nomagic) = (false, false);
    let mut at = 0;
    while let Some(ch) = pattern[at..].chars().next() {
        at += ch.len_utf8();
        let collection = match ch {
            '[' => !very_nomagic,
            '\\' => match pattern[at..].chars().next() {
                Some(next) => {
                    at += next.len_utf8();
                    match next {
                        'v' => (very_magic, very_nomagic) = (true, false),
                        'V' => (very_magic, very_nomagic) = (false, true),
                        _ => {}
                    }
                    next == '[' && very_nomagic
                }
                None => false,
            },
            _ => false,
        };
        if collection {
            at += collection_len(&pattern[at..]);
        }
    }
    let upper = |ch: char| !ch.to_lowercase().eq(std::iter::once(ch));
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        let skips = match ch {
            '\\' if !very_magic => true,
            '%' | '_' if very_magic => true,
            _ if upper(ch) => return true,
            _ => false,
        };
        if !skips {
            continue;
        }
        // `\_X` and `\%X` skip their `X` too.
        let escaped = chars.next();
        if ch == '\\' && matches!(escaped, Some('_' | '%')) && chars.peek().is_some() {
            chars.next();
        }
    }
    false
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
    let found = in_time(pattern.matches(hay, deadline).next(), deadline)?;
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

/// Gives `visit` the matches [`matches_reaching_until`] finds, one at a time,
/// searching no further once it breaks, so a host filtering them does no more
/// work than it needs and allocates nothing for those it leaves out. Past
/// `deadline` it stops giving any: ask it afterwards whether it passed.
pub fn visit_reaching(
    text: &Text,
    row: u32,
    window: Range<usize>,
    pattern: &Pattern,
    deadline: &mut Deadline,
    mut visit: impl FnMut(Range<usize>) -> std::ops::ControlFlow<()>,
) {
    if row >= text.row_count() || deadline.expired() {
        return;
    }
    let mut buf = String::new();
    let (start, hay) = row_text(text, row, &mut buf);
    for found in reaching_in(start, hay, window, pattern, deadline) {
        if visit(found).is_break() {
            return;
        }
    }
}

/// For a host painting the matches of a pattern that [`Pattern::spans_rows`]
/// on rows it shows: `shown` holds each buffer row shown, in order, with the
/// part of it shown (offsets in the text). The matches are those
/// [`Pattern::find_across`] lists on the shown rows, from each one's start,
/// which may go on over later rows, and those starting on rows before the
/// first that reach into its shown part: looked for from the nearest rows
/// back, as far as the pattern's matches reach (the text's start for one
/// with no bound). A match starting inside the one before it is left out,
/// so none overlap. The last shown row's are listed only until one starts
/// past its shown part, the others' whole, as a match may reach a later
/// shown row. In order. The shown rows are listed first, then those before
/// them, until `deadline` passes, which the caller may ask whether it
/// [`Deadline::ran_out`]: then those listed by then, the matches of the
/// regex calls before the one that overran it.
pub fn matches_across_until(
    text: &Text,
    shown: &[(u32, Range<usize>)],
    pattern: &Pattern,
    deadline: &mut Deadline,
) -> Vec<Range<usize>> {
    let Some((first, first_shown)) = shown.first().cloned() else {
        return Vec::new();
    };
    let mut walk = Across::new(pattern, text, deadline);
    let mut on_shown = Vec::new();
    for (ix, (row, part)) in shown.iter().enumerate() {
        let last = ix + 1 == shown.len();
        let enough = |found: &[Range<usize>]| {
            last && found.last().is_some_and(|found| found.start > part.end)
        };
        if walk.list_row_onto(*row, enough, &mut on_shown).is_err() {
            break;
        }
    }
    let reaches = |found: &Range<usize>| found.end > first_shown.start;
    let lowest = match pattern.reach {
        Reach::Breaks(breaks) => first.saturating_sub(breaks),
        _ => 0,
    };
    // Back from the first shown row a stretch at a time, each twice as long
    // as the last, each listed forward.
    let mut earlier = Vec::new();
    let (mut end, mut stretch) = (first, 64u32);
    while end > lowest {
        let start = end.saturating_sub(stretch).max(lowest);
        let mut found_here = Vec::new();
        let mut row = start;
        let mut interrupted = false;
        while row < end {
            match walk.list_row_onto(row, |_| false, &mut found_here) {
                Ok(Some(next)) => row = next,
                Ok(None) => break,
                Err(Interrupted) => {
                    interrupted = true;
                    break;
                }
            }
        }
        found_here.retain(reaches);
        found_here.append(&mut earlier);
        earlier = found_here;
        if interrupted {
            break;
        }
        (end, stretch) = (start, stretch.saturating_mul(2));
    }
    let mut kept: Vec<Range<usize>> = Vec::new();
    for found in earlier.into_iter().chain(on_shown) {
        if kept.last().is_none_or(|last| found.start >= last.end) {
            kept.push(found);
        }
    }
    kept
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
    reaching_in(start, hay, window, pattern, deadline)
        .take(cap)
        .collect()
}

/// The matches in `hay`, the row at `start`, that reach `window`: see
/// [`matches_reaching`].
fn reaching_in<'a>(
    start: usize,
    hay: &'a str,
    window: Range<usize>,
    pattern: &'a Pattern,
    deadline: &'a Deadline,
) -> impl Iterator<Item = Range<usize>> + 'a {
    pattern
        .matches(hay, deadline)
        .up_to(window.end.saturating_sub(start))
        .map(move |range| start + range.start..start + range.end)
        .take_while({
            let end = window.end;
            move |range| range.start <= end
        })
        .filter(move |range| match range.is_empty() {
            true => window.start <= range.start,
            false => range.end > window.start && range.start < window.end,
        })
}

#[cfg(test)]
thread_local! {
    /// How many rows [`Pattern::find`] and [`matches_in`] have searched on
    /// this thread.
    pub(crate) static ROWS_SEARCHED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many chunks of the text [`Window`]s have read on this thread.
    pub(crate) static WINDOW_CHUNKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
    /// Where the row ends in `hay`, which for [`Pattern::find_across`] holds
    /// the rows after it too: a match starts at or before it, and the next
    /// search before it.
    row_end: usize,
    /// The start of a match found starting past `row_end`, which ended the
    /// row's matches: where the first of later rows' starts, if `hay`
    /// holds enough of them to tell ([`Window`]).
    beyond: Option<usize>,
    /// Whether `hay` holds later rows ([`Pattern::find_across`]), where
    /// each search after a match goes on from where `\zs` and `\ze` bound
    /// it, not from the whole match.
    across: bool,
    at: Option<usize>,
    /// The last match's groups, read through [`RowMatches::group`] if
    /// `groups`.
    found: Found,
    groups: bool,
    /// For `:s`: where the last non-empty match ended, if one did.
    last_end: Option<Option<usize>>,
    /// Where the last match returned ended, for `\zs` and `\ze` to keep
    /// the matches from overlapping.
    emitted: usize,
    /// The last match returned, which is group 0.
    bounds: Range<usize>,
}

impl RowMatches<'_> {
    /// Stops searching once no match can start at or before `end` in the
    /// row, so what follows that costs at most the one regex call that finds
    /// the first candidate past it, rejected ones (not a whole word) included.
    /// The row stays whole for anchors and boundaries to see.
    pub(crate) fn up_to(mut self, end: usize) -> Self {
        // One past it, as the next search has to start before the limit: an
        // empty match at `end` is still to be found.
        self.row_end = self.row_end.min(end.saturating_add(1));
        self
    }

    /// Group `n`'s range in the last match (0 is the whole match), if it
    /// took part; `None` if the groups weren't wanted.
    pub(crate) fn group(&self, n: usize) -> Option<Range<usize>> {
        if !self.groups {
            return None;
        }
        if n == 0 {
            return Some(self.bounds.clone());
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

impl RowMatches<'_> {
    /// `whole`, the last match, as `\zs` and `\ze` bound it: from where the
    /// last `\zs` passed matched (else its start) to where `\ze` did (else
    /// its end), and never before the last match returned ended.
    fn bounded(&mut self, whole: Range<usize>) -> Range<usize> {
        let (zs, ze) = match (self.engine, &self.found) {
            (Engine::Regex { zs, ze, .. }, Found::Regex(locs)) => {
                // Of copies that took part, the one latest in the row.
                let last = |groups: &[usize]| {
                    groups
                        .iter()
                        .filter_map(|&at| locs.get(at))
                        .map(|(start, _)| start)
                        .max()
                };
                (last(zs), last(ze))
            }
            (Engine::Backtrack(_), Found::Backtrack { slots, .. }) => (
                backtrack::mark(slots, backtrack::ZS),
                backtrack::mark(slots, backtrack::ZE),
            ),
            _ => unreachable!("an engine's own state"),
        };
        if zs.is_none() && ze.is_none() {
            return whole;
        }
        let start = zs.unwrap_or(whole.start).max(self.emitted);
        let end = ze.unwrap_or(whole.end).max(start);
        self.emitted = end;
        start..end
    }
}

impl Iterator for RowMatches<'_> {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Range<usize>> {
        loop {
            let at = self.at.take()?;
            if self.deadline.passed() {
                return None;
            }
            REGEX_CALLS.with(|calls| calls.set(calls.get() + 1));
            let found = match (self.engine, &mut self.found) {
                (Engine::Regex { regex, slots, .. }, Found::Regex(locs)) => {
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
            if found.start > self.row_end {
                self.beyond = Some(found.start);
                return None;
            }
            let next = if found.is_empty() {
                unit(&self.hay[found.start..]).map_or(usize::MAX, |unit| found.start + unit)
            } else {
                found.end
            };
            let (start, end) = (self.word.0, self.word.1);
            let whole_match = found.clone();
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
                self.at = (on < self.row_end || (self.whole && on == self.row_end)).then_some(on);
                continue;
            }
            if let Some(last_end) = &mut self.last_end {
                *last_end = (!found.is_empty()).then_some(found.end);
            }
            self.bounds = self.bounded(whole_match);
            // Over rows the next search goes on from the match as `\zs` and
            // `\ze` bound it, a whole character on from an empty one, as
            // nvim's does.
            let next = match (self.across, self.bounds.is_empty()) {
                (false, _) => next,
                (true, false) => self.bounds.end,
                (true, true) => {
                    let at = self.bounds.start;
                    unit(&self.hay[at..]).map_or(usize::MAX, |unit| at + unit)
                }
            };
            // After a match that ends on a later row the row has no more,
            // as in nvim.
            self.at = (next < self.row_end).then_some(next);
            return Some(self.bounds.clone());
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
            PatternError::TooLarge(tr!("vim-pattern-compiled-over", limit = limit).into())
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
    /// `\n`: a line break.
    Newline,
    /// `\_x`, `\_.` or `\_[...]` (or a collection holding `\n`): the `.`,
    /// class or collection it holds, or a line break.
    OrNewline(Box<Token>),
    WordStart,
    WordEnd,
    /// `\zs`: where the match starts, as the search reports it.
    MatchStart,
    /// `\ze`: where the match ends, as the search reports it.
    MatchEnd,
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
    /// Whether atoms that match a line break are read ([`Pattern::parse_across`]),
    /// else refused as unsupported.
    across: bool,
    /// Whether the pattern holds one: `\n`, `\_x`, `\_.`, `\_[...]`, a
    /// collection with `\n`, `\_^` or `\_$`.
    rows: bool,
    tokens: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn new(pattern: &'a str, across: bool) -> Self {
        Self {
            pattern,
            at: 0,
            magic: Magic::On,
            ignore_case: false,
            match_case: false,
            across,
            rows: false,
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

    /// The pattern's tokens; whether it ignores case: `Some(true)` if it
    /// holds `\c` (which wins over `\C`, wherever either is), `Some(false)`
    /// if it holds only `\C`, `None` if neither; and whether it holds an
    /// atom that matches a line break ([`Lexer::rows`]).
    fn run(mut self) -> Result<(Vec<Token>, Option<bool>, bool), PatternError> {
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
        // After `\n` a magic `^` starts a row, and before it a `$` ends
        // one, as at a branch's start and end (`:help /^`).
        let real: Vec<usize> = (0..self.tokens.len())
            .filter(|&ix| self.tokens[ix] != Token::Flag)
            .collect();
        for (at, &ix) in real.iter().enumerate() {
            let newline =
                |other: Option<&usize>| other.is_some_and(|&o| self.tokens[o] == Token::Newline);
            let starts = newline(at.checked_sub(1).and_then(|at| real.get(at)));
            let ends = newline(real.get(at + 1));
            match &mut self.tokens[ix] {
                Token::Caret { anywhere } if starts => *anywhere = true,
                Token::Dollar { anywhere } if ends => *anywhere = true,
                _ => {}
            }
        }
        Ok((self.tokens, case, self.rows))
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
            '[' => match collection(self.pattern, self.at, self.across)? {
                Some((set, end, newline)) => {
                    self.at = end;
                    self.rows |= newline;
                    match newline {
                        true => Token::OrNewline(Box::new(Token::Set(set))),
                        false => Token::Set(set),
                    }
                }
                // With no `]` to end it, `[` is itself.
                None => Token::Char('['),
            },
            '%' if self.peek() == Some('(') => {
                self.bump();
                Token::Open { capture: false }
            }
            '%' if matches!(self.peek(), Some('d' | 'o' | 'x' | 'u' | 'U')) => {
                let code = self.bump().unwrap_or_default();
                let (radix, most) = match code {
                    'd' => (10, usize::MAX),
                    'o' => (8, 3),
                    'x' => (16, 2),
                    'u' => (16, 4),
                    _ => (16, 8),
                };
                let mut value: u64 = 0;
                let mut taken = 0;
                while taken < most && !(code == 'o' && value >= 0o40) {
                    let Some(digit) = self.peek().and_then(|ch| ch.to_digit(radix)) else {
                        break;
                    };
                    self.bump();
                    value = value * u64::from(radix) + u64::from(digit);
                    taken += 1;
                    if value > i32::MAX as u64 {
                        return Err(PatternError::Invalid(tr!("vim-pattern-code-atom").into()));
                    }
                }
                if taken == 0 {
                    return Err(PatternError::Invalid(tr!("vim-pattern-code-atom").into()));
                }
                // nvim matches a NUL for 0 and 10, an artefact of its line
                // storage; rows here have no newline. A non-`char` matches nothing.
                match char::from_u32(value as u32) {
                    Some(ch) if value != 0 && value != 10 => Token::Char(ch),
                    _ => return Err(self.unsupported(start)),
                }
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
            'z' if matches!(self.peek(), Some('s' | 'e')) => match self.bump() {
                Some('s') => Token::MatchStart,
                _ => Token::MatchEnd,
            },
            'n' if self.across => {
                self.rows = true;
                Token::Newline
            }
            '_' if self.across => {
                let token = match self.bump() {
                    Some('^') => Token::Caret { anywhere: true },
                    Some('$') => Token::Dollar { anywhere: true },
                    Some('.') => Token::OrNewline(Box::new(Token::Any)),
                    Some('[') => match collection(self.pattern, self.at, true)? {
                        Some((set, end, _)) => {
                            self.at = end;
                            Token::OrNewline(Box::new(Token::Set(set)))
                        }
                        None => {
                            return Err(PatternError::Invalid(
                                tr!("vim-pattern-missing-bracket").into(),
                            ));
                        }
                    },
                    Some(letter) => match class(letter) {
                        Some(class) => Token::OrNewline(Box::new(Token::Class(class))),
                        None => return Err(self.unsupported(start)),
                    },
                    None => return Err(self.unsupported(start)),
                };
                self.rows = true;
                token
            }
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
            return Err(PatternError::Invalid(
                tr!("vim-pattern-brace-syntax").into(),
            ));
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
            .map_err(|_| PatternError::TooLarge(tr!("vim-pattern-count", digits = digits).into()))
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
        PatternError::unsupported(&self.pattern[start..self.at])
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
/// `regex` class, the offset after the `]`, and whether it holds `\n`, a
/// line break, which `across` reads ([`Lexer::across`]) and a `^` first
/// leaves out (`[^\n]` is `.`, as in nvim); `None` if no `]` ends it, when
/// Vim takes the `[` as itself. Errors inside count only once a `]` makes
/// it a collection.
fn collection(
    pattern: &str,
    at: usize,
    across: bool,
) -> Result<Option<(Collection, usize, bool)>, PatternError> {
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
    let mut newline = false;
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
                    return Err(PatternError::unsupported(&format!("[:{name}:]-")));
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
                    error.get_or_insert(PatternError::unsupported(&format!("[:{name}:]")));
                    rest = after;
                }
                None => {
                    error.get_or_insert(PatternError::unsupported("[:"));
                    rest = name;
                }
            }
            continue;
        }
        if rest.starts_with("[=") || rest.starts_with("[.") {
            error.get_or_insert(PatternError::unsupported(&rest[..2]));
            rest = &rest[2..];
            continue;
        }
        // `\n` is a line break, but as a range's end, or followed by `-`
        // to start one, the character 0x0a, which no row holds, as in
        // nvim: `[\n-a]` is 0x0a to `a`.
        if across
            && let Some(after) = rest.strip_prefix("\\n")
            && !after.starts_with('-')
        {
            newline |= !negated;
            rest = after;
            continue;
        }
        let low = match member(&mut rest, across) {
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
                return Err(PatternError::unsupported(&format!(
                    "{low}-{}",
                    &after[..end]
                )));
            }
            Some(after) if !after.is_empty() && !after.starts_with(']') => {
                rest = after;
                match member(&mut rest, across) {
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
            error.get_or_insert(PatternError::Invalid(
                tr!("vim-pattern-reverse-range").into(),
            ));
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
    Ok(Some((set, pattern.len() - rest.len(), newline)))
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
    /// bare base. With `rows`, for a haystack of several rows, never a line
    /// break: a collection matches one only with `\_` or `\n`
    /// ([`Token::OrNewline`]). `None` for one with no member.
    fn syntax(&self, ignore_case: bool, marks: bool, rows: bool) -> Option<String> {
        if !self.negated && self.ascii.is_empty() && self.folded.is_empty() && self.cased.is_empty()
        {
            return None;
        }
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
            (true, false) if rows => r"[^\n\p{M}",
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
            (false, false) if rows => r"]--[\n\p{M}]]",
            (false, false) => r"]--\p{M}]",
            (false, true) => r"]&&\p{M}]",
        });
        Some(class)
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
/// `\\ \] \^ \-`, and if `across` `\n`, a range's end: 0x0a). A
/// backslash before anything else is itself.
fn member(rest: &mut &str, across: bool) -> Result<char, PatternError> {
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
        'n' if across => Some('\n'),
        'n' => {
            *rest = &after[1..];
            return Err(PatternError::unsupported("\\n"));
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
            return Err(PatternError::unsupported(&format!(
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
        .ok_or_else(|| PatternError::unsupported(&format!("\\{code}{}", &digits[..taken])))
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
    /// Whether the pattern is matched over several rows at once
    /// ([`Pattern::find_across`]), so that only the atoms written to match
    /// a line break do.
    rows: bool,
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
        return Err(PatternError::Unsupported(
            tr!("vim-pattern-composing-marks", limit = MAX_ORDERED_MARKS).into(),
        ));
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
                Some(Token::Close) => {
                    return Err(PatternError::Invalid(
                        tr!("vim-pattern-unmatched-close").into(),
                    ));
                }
                None => {
                    return match opened {
                        Opened::Capture => Err(PatternError::Invalid(
                            tr!("vim-pattern-unmatched-open").into(),
                        )),
                        Opened::Group => Err(PatternError::Invalid(
                            tr!("vim-pattern-unmatched-group").into(),
                        )),
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
                    tr!("vim-pattern-alternatives").into(),
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
                tr!("vim-pattern-alternatives").into(),
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
            return Err(PatternError::Invalid(
                tr!("vim-pattern-multi-after-multi").into(),
            ));
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
                        tr!("vim-pattern-multi-after-varying-atom").into(),
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
                return Err(PatternError::unsupported("* after \\%("));
            }
            Token::Multi {
                multi: Multi::Star,
                bare: true,
            } if first || star_here => regex::escape("*"),
            Token::Multi { .. } => {
                return Err(PatternError::Invalid(
                    tr!("vim-pattern-misplaced-multi").into(),
                ));
            }
            // A mark here follows no atom that takes characters, and one
            // that starts a branch is the same: any character carrying it.
            Token::Char(mark) if is_mark(mark) => {
                self.next -= 1;
                let marks = self.written_marks();
                let base = if self.rows { r"[^\n\p{M}]" } else { r"\P{M}" };
                format!("{base}{}", carrying(&marks)?)
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
            Token::Any | Token::Class(_) | Token::Set(_) => self
                .one_character(&token, after_bare)?
                .expect("only a collection holding `\\n` has no member"),
            Token::Newline => r"\n".into(),
            // `\_.` doesn't take the marks written after it, as `.` does:
            // as after `\n`, they start an atom of their own, any character
            // carrying them, as in nvim.
            Token::OrNewline(inner) if *inner == Token::Any => {
                let any = self.character(r"[^\n\p{M}]", after_bare.then_some(r"\p{M}"), false)?;
                format!(r"(?:{any}|\n)")
            }
            Token::OrNewline(inner) => match self.one_character(&inner, after_bare)? {
                Some(text) => format!(r"(?:{text}|\n)"),
                None => r"\n".into(),
            },
            Token::WordStart => r"\b{start}".into(),
            Token::WordEnd => r"\b{end}".into(),
            // Empty groups the matcher reads the match's bounds from; they
            // have names, not numbers, so `\1` to `\9` stay as written.
            Token::MatchStart => "(?P<vimzs>)".into(),
            Token::MatchEnd => "(?P<vimze>)".into(),
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

    /// A `.`, backslash class or collection (`token`), after a bare base or
    /// not, as the one character (and its marks) it takes; `None` for a
    /// collection with no member, which only `\n` in it makes match.
    fn one_character(
        &mut self,
        token: &Token,
        after_bare: bool,
    ) -> Result<Option<String>, PatternError> {
        Ok(Some(match token {
            Token::Any => self.character(r"[^\n\p{M}]", after_bare.then_some(r"\p{M}"), true)?,
            Token::Class(class) => match class.strip_prefix("[^") {
                Some(members) => {
                    let newline = if self.rows { r"\n" } else { "" };
                    self.character(
                        &format!(r"[^{newline}\p{{M}}{members}"),
                        after_bare
                            .then(|| format!(r"[\p{{M}}&&[^{members}]"))
                            .as_deref(),
                        false,
                    )?
                }
                None => self.character(class, None, false)?,
            },
            Token::Set(set) => {
                let Some(whole) = set.syntax(self.ignore_case, false, self.rows) else {
                    return Ok(None);
                };
                let lone = after_bare
                    .then(|| set.syntax(self.ignore_case, true, self.rows))
                    .flatten();
                self.character(&whole, lone.as_deref(), true)?
            }
            _ => unreachable!("not one character"),
        }))
    }

    /// A group's contents after its `\(` or `\%(`, through its `\)`, as
    /// the ways it can end: one for each run of alternatives that end after
    /// a bare base, or not, together.
    fn group(&mut self, capture: bool, bare: bool) -> Result<Vec<Path>, PatternError> {
        if capture {
            self.groups += 1;
            if self.groups > MAX_GROUPS {
                return Err(PatternError::Invalid(
                    tr!("vim-pattern-too-many-groups").into(),
                ));
            }
        }
        let number = self.groups;
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(PatternError::TooLarge(
                tr!("vim-pattern-nested-too-deeply").into(),
            ));
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
    let marks = source
        .replace("(?P<vimzs>", "(?P<zs>")
        .replace("(?P<vimze>", "(?P<ze>");
    let (mut zs, mut ze) = (0, 0);
    let source = {
        let mut named = String::with_capacity(marks.len());
        let mut rest = marks.as_str();
        while let Some(at) = rest.find("(?P<z") {
            let (kind, count) = match &rest[at + 5..at + 6] {
                "s" => ("zs", &mut zs),
                _ => ("ze", &mut ze),
            };
            named.push_str(&rest[..at]);
            named.push_str(&format!("(?P<{kind}_{count}>"));
            *count += 1;
            rest = &rest[at + 7..];
        }
        named.push_str(rest);
        named
    };
    let mut rest = source.as_str();
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

    /// How many line breaks a multi-row pattern's match can take, which
    /// decides how many rows past a match's first a window must hold.
    #[test]
    fn reach_counts_the_line_breaks_a_match_takes() {
        for (pattern, reach) in [
            ("abc", Reach::Row),
            (r"[^\n]", Reach::Row),
            (r"x\_$", Reach::Breaks(0)),
            (r"\_^", Reach::Breaks(0)),
            (r"c\nd", Reach::Breaks(1)),
            (r"[\n]", Reach::Breaks(1)),
            (r"a*\n", Reach::Breaks(1)),
            (r"\%(\n\)\=x", Reach::Breaks(1)),
            (r"\n\n", Reach::Breaks(2)),
            (r"\n\{2,3}b\|\_s", Reach::Breaks(3)),
            (r"\(a\n\|\n\n\n\)\{2}", Reach::Breaks(6)),
            (r"\n\{0}", Reach::Breaks(0)),
            (r"\_s\+", Reach::Unbounded),
            (r"a\_.\{-}z", Reach::Unbounded),
            (r"\n*", Reach::Unbounded),
            (r"\%(\n\{400}\)\{300}", Reach::Unbounded),
        ] {
            let parsed = Pattern::parse_across(pattern, false).unwrap();
            assert_eq!(parsed.reach, reach, "{pattern}");
        }
        assert_eq!(
            Pattern::parse_across(r"a\_[b", false).unwrap_err(),
            PatternError::Invalid(tr!("vim-pattern-missing-bracket").into())
        );
        for (pattern, atom) in [(r"a\_y", r"\_y"), (r"a\_", r"\_")] {
            assert_eq!(
                Pattern::parse_across(pattern, false).unwrap_err(),
                PatternError::unsupported(atom)
            );
        }
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
mod uppercase_tests {
    use super::has_uppercase;

    /// nvim's `pat_has_uppercase()`: an escaped letter doesn't count, nor
    /// `\_X` or `\%X`, but in a very magic pattern it does, while `_X` and
    /// `%X` don't.
    #[test]
    fn has_uppercase_reads_patterns_as_nvim_does() {
        for (pattern, upper) in [
            ("hello", false),
            ("Hello", true),
            ("émile", false),
            ("Émile", true),
            (r"\Sello", false),
            (r"\_Sello", false),
            (r"\%Vello", false),
            (r"\cHello", true),
            (r"\v\Sello", true),
            (r"\v%Vello_Xx", false),
            (r"\v\Sello\V", false),
            ("[A-Z]", true),
            ("\\", false),
            // A `\v` or `\V` in a collection switches nothing: nvim 0.12.4
            // with 'ic' 'scs' on "x", "vhello", "Vhello" lands
            // `/\v[\V]hello` on "Vhello" (exact case), and on "x",
            // "vSELLO", "vsello" lands `/[\v]\Sello` on "vSELLO" (case
            // ignored), as `/\V\[\v]\Sello`; `/\V[\v]\Sello`, whose `[`
            // starts nothing, on "x", "[]xELLO", "[]xello" lands on
            // "[]xello".
            (r"\v[\V]hello", true),
            (r"[\v]\Sello", false),
            (r"\V\[\v]\Sello", false),
            (r"\V[\v]\Sello", true),
        ] {
            assert_eq!(has_uppercase(pattern), upper, "{pattern}");
        }
    }

    /// The cases above as searches, against what nvim 0.12.4 does with
    /// them under 'ignorecase' and 'smartcase'.
    #[test]
    fn collections_hold_no_magic_switch_for_smartcase() {
        for (buffer, keys, want) in [
            ("x\nvhello\nVhello", r"/\v[\V]hello<CR>", (2, 0)),
            ("x\nvSELLO\nvsello", r"/[\v]\Sello<CR>", (1, 0)),
            ("x\nvSELLO\nvsello", r"/\V\[\v]\Sello<CR>", (1, 0)),
            ("x\n[]xELLO\n[]xello", r"/\V[\v]\Sello<CR>", (2, 0)),
        ] {
            let mut vim = crate::HeadlessVim::new(buffer, (0, 0));
            vim.set_search_case(true, true);
            vim.feed(keys).unwrap();
            assert_eq!(vim.cursor(), want, "{keys} on {buffer:?}");
        }
    }
}

#[cfg(test)]
#[path = "tests/search_incremental.rs"]
mod incremental_tests;

/// Patterns that span rows: what nvim 0.12.4 does with a collection's `\n`
/// as a range's end, marks after `\_.`, and `\zs` and `\ze` over rows (each
/// probed with `searchpos()` in `nvim --clean --headless`), and searches
/// stopping at their deadline.
#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod across_tests {
    use super::*;

    fn found(pattern: &str, buffer: &str, from: usize, forward: bool) -> Option<Range<usize>> {
        let pattern = Pattern::parse_across(pattern, false).unwrap();
        (pattern.find_across(&Text::new(buffer), from, forward, &Deadline::never())).unwrap()
    }

    /// The text's first match: from its end, round to its start.
    fn first(pattern: &str, buffer: &str) -> Option<Range<usize>> {
        found(pattern, buffer, buffer.len(), true)
    }

    /// `\n` in a collection is a line break, but the character 0x0a, which
    /// no row holds, as a range's end or followed by `-`: ordinary,
    /// negated and `\_[...]` collections alike.
    #[test]
    fn a_collection_range_ends_at_the_character_not_a_line_break() {
        for (buffer, pattern, want) in [
            ("x\n5\na", r"[\n-a]", Some(2..3)),
            ("x\n5\na", r"[\n-a]\+", Some(2..3)),
            ("x\n5\na", r"\_[\n-a]", Some(1..2)),
            ("x\n5\na", r"\_[\n-a]\+", Some(1..6)),
            ("x\n5\na", r"[^\n-a]", Some(0..1)),
            ("a-\nb", r"[^\n-a]", Some(3..4)),
            ("a-\nb", r"\_[^\n-a]", Some(2..3)),
            ("x\n-\nb", r"[\n-]", Some(2..3)),
            ("x\n5\na", r"[\n-]", None),
            ("x\n5\na", r"[-\n]", Some(1..2)),
            ("x\n5\na", r"[\n\n-a]", Some(1..2)),
            ("x\n\u{1}y", r"[\x01-\n]", Some(2..3)),
            ("x\n5\na", r"[\n-\n]", None),
            ("x\n5\na", r"\_[\n-\n]", Some(1..2)),
        ] {
            assert_eq!(first(pattern, buffer), want, "{pattern} on {buffer:?}");
        }
        assert!(matches!(
            Pattern::parse_across(r"[a-\n]", false),
            Err(PatternError::Invalid(_))
        ));
    }

    /// Marks written after `\_.` aren't its: as after `\n` and `\_s`, they
    /// are any character carrying them, never a bare line break.
    #[test]
    fn marks_after_any_or_newline_are_an_atom_of_their_own() {
        for (buffer, pattern, want) in [
            ("ab\ncd", "\\_.\u{301}", None),
            ("xa\u{301}b\ncd", "\\_.\u{301}", Some(0..4)),
            ("a\nb\u{301}", "\\_.\u{301}", Some(1..5)),
            ("ab\ncd\u{301}e", "\\_.\u{301}", Some(3..7)),
            ("a\nb\u{301}", "\\_s\u{301}", Some(1..5)),
            ("a\nb\u{301}", "\\n\u{301}", Some(1..5)),
            ("ab\ncd", "\\_.\u{301}b", None),
            ("xa\u{301}b\ncd", "\\_.\u{301}b", Some(0..5)),
            // `.` takes them, as it always has.
            ("xa\u{301}b\ncd", ".\u{301}", Some(1..4)),
        ] {
            assert_eq!(first(pattern, buffer), want, "{pattern:?} on {buffer:?}");
        }
    }

    /// Over rows each search after a match goes on from where `\zs` and
    /// `\ze` bound it, and a match crossing a row ends its matches only if
    /// its bounded part does.
    #[test]
    fn matches_over_rows_go_on_from_their_bounds() {
        for (buffer, pattern, from, forward, want) in [
            ("aa\nb", r"a\ze\_.*b", 0, true, 1..2),
            ("aa\nb", r"a\ze\_.*b", 1, false, 0..1),
            ("aa\nb", r"a\ze\_.*b", 3, false, 1..2),
            ("abcd\nx", r"\_.\zs", 0, true, 1..1),
            ("abcd\nx", r"\_.\zs", 1, true, 3..3),
            ("abcd\nx", r"\_.\zs", 3, true, 6..6),
            ("abcd\nx", r"\_.\zs", 3, false, 1..1),
            ("abcd\nx", r"\_.\zs", 5, false, 3..3),
            ("ab\nab\nc", r"a\zeb\_.*c", 0, true, 3..4),
            ("ab\nab\nc", r"a\zeb\_.*c", 6, false, 3..4),
            ("abab\nc", r"a\zeb\_.*c", 0, true, 2..3),
            ("abab\nc", r"a\zeb\_.*c", 2, true, 0..1),
            ("xab\ncd", r"a\zsb\nc", 0, true, 2..5),
            ("ab\ncd", r"b\n\zsc", 0, true, 3..4),
            ("ab\ncd", r"b\n\zsc", 4, false, 3..4),
        ] {
            let case = (pattern, buffer, from, forward);
            assert_eq!(
                found(pattern, buffer, from, forward),
                Some(want),
                "{case:?}"
            );
        }
    }

    /// A search is interrupted by a regex call that spends the last of its
    /// budget, its last one included, whether that call found a match or
    /// nothing: what it found counts for nothing.
    #[test]
    fn the_last_regex_call_past_the_budget_interrupts_the_search() {
        let text = Text::new("abc\ndef\nabc\ndef");
        for (pattern, from, forward) in [
            (r"c\nd", 0, true),
            (r"c\nd", 14, false),
            (r"x\ny", 0, true),
            (r"x\ny", 14, false),
            (r"\_.\zs", 1, true),
        ] {
            let pattern = Pattern::parse_across(pattern, false).unwrap();
            let before = regex_calls();
            let unlimited = pattern.find_across(&text, from, forward, &Deadline::never());
            let calls = regex_calls() - before;
            assert!(calls > 0);
            set_call_limit(Some(calls));
            let at_budget = pattern.find_across(&text, from, forward, &Deadline::new());
            set_call_limit(Some(calls + 1));
            let within = pattern.find_across(&text, from, forward, &Deadline::new());
            set_call_limit(None);
            let case = (pattern.regex_str(), from, forward);
            assert_eq!(at_budget, Err(Interrupted), "{case:?}");
            assert_eq!(within, unlimited, "{case:?}");
        }
    }

    /// The matches [`matches_across_until`] lists with a budget of `calls`
    /// regex calls, whether it ran out, and how many the whole list takes.
    fn painted(
        buffer: &str,
        shown: &[(u32, Range<usize>)],
        pattern: &str,
        calls: Option<u64>,
    ) -> (Vec<Range<usize>>, bool, u64) {
        let (text, pattern) = (
            Text::new(buffer),
            Pattern::parse_across(pattern, false).unwrap(),
        );
        set_call_limit(calls);
        let mut deadline = Deadline::new();
        let before = regex_calls();
        let found = matches_across_until(&text, shown, &pattern, &mut deadline);
        let spent = regex_calls() - before;
        set_call_limit(None);
        deadline.expired();
        (found, deadline.ran_out(), spent)
    }

    /// Run out while listing a row's matches, painting keeps those of the
    /// calls before the one that overran: the matches as far as the budget
    /// went, for each budget.
    #[test]
    fn painting_keeps_a_row_s_matches_listed_before_the_deadline() {
        let (buffer, pattern) = ("a a a\nx", r"a\|\n");
        let shown = [(0, 0..5), (1, 6..7)];
        let (all, ran_out, calls) = painted(buffer, &shown, pattern, None);
        // A match ending at a row's end ends its matches, as in nvim: the
        // line break after the text's last row is the next.
        assert_eq!(
            (all.as_slice(), ran_out),
            (&[0..1, 2..3, 4..5, 7..8][..], false)
        );
        let (found, ran_out, _) = painted(buffer, &shown, pattern, Some(2));
        assert_eq!((found, ran_out), (vec![0..1], true));
        for budget in 1..=calls {
            let (found, ran_out, _) = painted(buffer, &shown, pattern, Some(budget));
            assert_eq!(found, all[..found.len()], "budget {budget}");
            assert!(ran_out, "budget {budget}");
        }
    }

    /// Run out looking back over the rows before the first shown, painting
    /// keeps the matches of rows listed before it from there, with the
    /// shown rows', which are listed first.
    #[test]
    fn painting_keeps_the_rows_looked_back_over_before_the_deadline() {
        let (buffer, pattern) = ("a\na\nx\nb", r"a\n\_.*b\|b");
        let shown = [(3, 6..7)];
        let (all, ran_out, calls) = painted(buffer, &shown, pattern, None);
        assert_eq!((all.as_slice(), ran_out), (&[0..7][..], false));
        // Short of the last call, on the row with none, the first row's
        // match from the stretch it was interrupted in.
        let (found, ran_out, _) = painted(buffer, &shown, pattern, Some(calls - 1));
        assert_eq!((found, ran_out), (vec![0..7], true));
        for budget in 1..=calls {
            let (found, ran_out, _) = painted(buffer, &shown, pattern, Some(budget));
            assert!(ran_out, "budget {budget}");
            assert!(found.len() <= 1, "budget {budget}: {found:?}");
            // The shown row's `b`, until the row above's match covers it.
            assert!(
                found.iter().all(|found| [0..7, 6..7].contains(found)),
                "{found:?}"
            );
        }
    }

    /// A search whose deadline has passed reads none of the text into its
    /// window, even for a pattern whose window holds the rest of the text,
    /// and a window stops reading once it passes.
    #[test]
    fn a_window_reads_nothing_past_the_deadline() {
        let buffer = vec!["a b c"; 10_000].join("\n");
        let text = Text::new(&buffer);
        let pattern = Pattern::parse_across(r"b\_s\+x", false).unwrap();
        assert_eq!(pattern.reach, Reach::Unbounded);
        for forward in [true, false] {
            WINDOW_CHUNKS.set(0);
            let deadline = Deadline::within(Duration::ZERO);
            assert_eq!(
                pattern.find_across(&text, 6, forward, &deadline),
                Err(Interrupted)
            );
            assert_eq!(WINDOW_CHUNKS.get(), 0, "forward {forward}");
        }
        let mut window = Window::new(&text, pattern.reach);
        let deadline = Deadline::never();
        deadline.give_up();
        assert_eq!(window.hold(3, &deadline), Err(Interrupted));
        assert!(window.first > window.last && window.hay.is_empty());
        window.hold(3, &Deadline::never()).unwrap();
        assert_eq!((window.first, window.last), (3, 9_999));
        assert_eq!(window.hay.len(), buffer.len() + 1 - text.row_start(3));
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;

    #[test]
    fn check_samples_the_clock() {
        // Nothing else has looked at the clock, yet time has passed.
        let past = Deadline::after(Duration::ZERO);
        assert!(!past.ran_out());
        assert_eq!(past.check(), Err(Interrupted));
        assert!(Deadline::never().check().is_ok());
    }

    #[test]
    fn work_samples_the_clock_in_batches() {
        let past = Deadline::after(Duration::ZERO);
        assert!(past.work(1).is_ok());
        assert_eq!(past.work(WORK_PER_SAMPLE as usize), Err(Interrupted));
    }
}
