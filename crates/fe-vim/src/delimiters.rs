//! Delimiter objects: ``i[ a[ i< a< it at i` a` `` (a feature module; see
//! [`crate::feature`]), as nvim 0.12.4 selects them (the `delimiters`
//! corpus records it).
//!
//! - `i[ a[` (and `i] a]`) and `i< a<` (and `i> a>`) are `i( a(` over
//!   other brackets: the `count`th unmatched opening bracket before the
//!   caret and its match, or with none around the caret, the next pair.
//! - ``i` a` `` are `i" a"` over backticks, on the caret's row.
//! - `it at` are tag blocks (`:help tag-blocks`): the `count`th `<name …>`
//!   around the caret, through its `</name>`. An opening tag's name runs
//!   to a blank, `>` or `/` and may hold `<` (`<x<y>`); one with `!` in it
//!   isn't a tag. A closing tag is `</`, then anything but `>`, to a `>` on
//!   the row. Names match case-blind (as `\c` folds them); one name's pairs
//!   nest if it ends in a word char, another's are left alone. A name char
//!   with composing chars after it (a grapheme of several) matches only the
//!   same char with those composing chars and maybe more, in any order.
//!   Composing chars after an opening tag's `<` are the `<`'s: the name is
//!   what follows, and the tag nests in no other; with nothing after
//!   (`<ͅ>`), taking the tag fails the object. A name holding `\ . * [ ~`
//!   pairs as a pattern, case-blind whatever flags it holds: `<a.b>` with
//!   `</axb>`, and with `<axb>` nested, matches overlapping, a char joined
//!   by a ZWJ or modifier kept whole and unfolded (each such char its own
//!   requirement, and none a magic char: a `.` joined to others pairs with
//!   nothing); `\k` and `\K` read as nvim's keyword chars; one that isn't a
//!   pattern on its own (`<a[>`, `<a\|b>`) pairs with nothing, and one whose
//!   pairing runs out of time fails the object.
//!   `<name/>` doesn't count, nor does an opening tag without a partner
//!   (`<br>`), but going back from the caret a closing tag of any name
//!   cancels the opening tag before it (so a stray `</b>` hides the `<p>`
//!   around it). `it` takes what's between the tags, from the first `>` of
//!   the opening tag outside a quoted attribute; with an empty inside it
//!   takes nothing at the closing tag. `at` takes both tags too. From a
//!   row's indent the caret starts at the first non-blank. Within a `<…` on
//!   its row (no `>` yet) it goes on to the next `>` (so on an opening tag,
//!   a tag in its attributes counts too), or within a `</…`, back to before
//!   it.
//!
//! In a visual mode ([`select_visual`]) the bracket and backtick objects
//! grow as `i(` and `i"` do. A tag block is found from the position before
//! the selection, going out until its closing tag starts at or after the
//! selection's end; an `it` that would select the selection again takes the
//! tags too.

use std::{cell::RefCell, collections::HashMap, ops::Range, rc::Rc, sync::OnceLock};

use fe_text::Text;
use regex::Regex;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    Pending, TextObject,
    feature::Feature,
    motion::{Class, Cursor, PAIRS, class},
    operator::{Span, row_end},
    search::{Deadline, Interrupted, Pattern, RowList, first_in_row, row_text},
    text_object::{brackets, quotes},
};

pub(crate) const FEATURE: Feature = Feature {
    object,
    ..Feature::NONE
};

/// The object the char after `i` (`inner`) or `a` names.
fn object(_: &Pending, ch: char, inner: bool) -> Option<TextObject> {
    use TextObject::*;
    let (i, a) = match ch {
        '[' | ']' => (InnerBracket, ABracket),
        '<' | '>' => (InnerAngle, AAngle),
        't' => (InnerTag, ATag),
        '`' => (InnerBacktick, ABacktick),
        _ => return None,
    };
    Some(if inner { i } else { a })
}

const ANGLES: (u8, u8) = (b'<', b'>');

/// What `object` covers around the caret at `offset`, `count` times, as
/// `text_object::select` says.
pub(crate) fn select(text: &Text, offset: usize, object: TextObject, count: u32) -> Span {
    use TextObject::*;
    let count = count.max(1);
    let inner = matches!(object, InnerBracket | InnerAngle | InnerTag | InnerBacktick);
    match object {
        InnerBracket | ABracket => brackets(text, offset, PAIRS[1], count, inner),
        InnerAngle | AAngle => brackets(text, offset, ANGLES, count, inner),
        InnerBacktick | ABacktick => quotes(text, offset, b'`', count, inner),
        InnerTag | ATag => match around(text, offset, count) {
            Some((block, _)) if inner => Span::Chars(block.inner..block.close),
            Some((block, _)) => Span::Chars(block.start..block.end),
            None => Span::Failed,
        },
        _ => Span::Failed,
    }
}

/// `object` typed in a visual mode from `anchor` to `cursor` (positions):
/// the new anchor and cursor, or `None` if it fails.
pub(crate) fn select_visual(
    text: &Text,
    anchor: usize,
    cursor: usize,
    object: TextObject,
    count: u32,
) -> Option<(usize, usize)> {
    use TextObject::*;
    let count = count.max(1);
    let (lo, hi) = (anchor.min(cursor), anchor.max(cursor));
    if !matches!(object, InnerTag | ATag) {
        return grow(text, lo, hi, cursor, object, count);
    }
    let (block, clock) = match lo == hi {
        true => around(text, lo, count)?,
        false => around_selection(text, lo, hi, count)?,
    };
    let whole = (block.start, last(text, block.start, block.end));
    let inside = (block.inner, last(text, block.inner, block.close));
    clock.check().ok()?;
    Some(match object == ATag || inside == (lo, hi) {
        true => whole,
        false => inside,
    })
}

/// The bracket or backtick `object` in a visual mode, as `i(` and `i"`
/// grow: over one position, the object there (the `count`th); otherwise
/// the smallest around the cursor that's larger than the selection, the
/// next one out if that's the selection (for backticks, only then).
fn grow(
    text: &Text,
    lo: usize,
    hi: usize,
    cursor: usize,
    object: TextObject,
    count: u32,
) -> Option<(usize, usize)> {
    let quotes = matches!(object, TextObject::InnerBacktick | TextObject::ABacktick);
    let inner = matches!(object, TextObject::InnerBracket | TextObject::InnerAngle);
    let around = |count: u32| ends(text, select(text, cursor, object, count), quotes, inner);
    if lo == hi {
        around(1)?;
        return around(count);
    }
    let covers = |(start, last): (usize, usize)| (start, end_of(text, last));
    let current = (lo, end_of(text, hi));
    let found = around(count).and_then(|found| match covers(found) == current {
        true => around(count.saturating_add(1)),
        false => (!quotes).then_some(found),
    })?;
    let (start, end) = covers(found);
    (start <= current.0 && end >= current.1 && (start, end) != current).then_some(found)
}

/// The positions a bracket or backtick span takes in a visual mode: its
/// start and the position its end is taken at. An empty inside is nothing
/// for brackets and the quotes for backticks; an `inner` bracket object's
/// inside ending at a row's end before the closing bracket takes that row's
/// line break.
fn ends(text: &Text, span: Span, quotes: bool, inner: bool) -> Option<(usize, usize)> {
    match span {
        Span::Chars(range) if range.is_empty() => quotes.then(|| (range.start, range.start - 1)),
        Span::Chars(range)
            if inner
                && range.end < text.len()
                && range.end == row_end(text, text.offset_to_point(range.end).row) =>
        {
            Some((range.start, range.end))
        }
        Span::Chars(range) => Some((range.start, last(text, range.start, range.end))),
        Span::Lines { rows, .. } => {
            Some((text.row_start(*rows.start()), row_end(text, *rows.end())))
        }
        _ => None,
    }
}

/// The position a selection of `start..end` ends at: its last grapheme's,
/// or a line break's (the row end before) if it ends at a row's start; for
/// an empty one, `start`.
fn last(text: &Text, start: usize, end: usize) -> usize {
    match end > start {
        false => start,
        true if text.offset_to_point(end).column == 0 => end - 1,
        true => Cursor::at(text, end - 1).offset(),
    }
}

/// The end of what a selection takes at position `offset`: its grapheme, or
/// at a row's end its line break (none on the last row).
fn end_of(text: &Text, offset: usize) -> usize {
    let cursor = Cursor::at(text, offset);
    if offset == row_end(text, cursor.row) {
        return match cursor.row + 1 < text.row_count() {
            true => offset + 1,
            false => offset,
        };
    }
    offset + cursor.line.grapheme(cursor.ix).len()
}

/// A tag block: `<name …>` at `start`, its name ending at `name_end`, and
/// its `</name>` from `close` to `end`.
#[derive(Clone, Copy, Debug)]
struct Block {
    start: usize,
    /// Where its inside starts ([`Tags::inner`]).
    inner: usize,
    close: usize,
    end: usize,
}

/// The `count`th tag block around the caret at `offset`, for an operator
/// or a visual selection of one position. From a row's indent the caret
/// starts at its first non-blank. Inside a `<…` on its row (on the `<` or
/// past it, before any `>`) it goes on to the next `>`, or with a `</…`,
/// back to before it. The tags starting at or before the caret count: the
/// `count`th opening tag back that no later one closes (a closing tag of
/// any name closes the opening one before it), or past it, the first with
/// a partner after the caret.
fn around(text: &Text, offset: usize, count: u32) -> Option<(Block, Clock)> {
    let clock = Clock::new();
    let source = gather(text, &clock).ok()?;
    let bytes = source.as_bytes();
    clock.phase("row");
    let row_start = before(bytes, offset, &clock, |byte| byte == b'\n')
        .ok()?
        .map_or(0, |at| at + 1);
    let row_end = find(bytes, offset, &clock, |byte| byte == b'\n')
        .ok()?
        .unwrap_or(bytes.len());
    let blanks = |byte: &u8| matches!(byte, b' ' | b'\t');
    let row = &bytes[row_start..row_end];
    let mut column = offset - row_start;
    if row[..(column + 1).min(row.len())].iter().all(blanks) {
        column = row.iter().position(|byte| !blanks(byte)).unwrap_or(column);
    }
    let mut offset = row_start + column;
    // The tags starting before this count, and the blocks whose closing
    // tag starts at or after `close` (past the caret, or its closing tag's
    // `<`).
    let mut bound = offset + 1;
    let mut close = offset + 1;
    let lt = row[..(column + 1).min(row.len())]
        .iter()
        .rposition(|&byte| byte == b'<')
        .filter(|&lt| {
            row.get(lt + 1..column)
                .is_none_or(|between| !between.contains(&b'>'))
        });
    match lt.map(|lt| (lt, row.get(lt + 1))) {
        Some((lt, Some(b'/'))) => {
            bound = row_start + lt;
            close = bound;
        }
        Some(_) => {
            let gt = find(bytes, offset, &clock, |byte| byte == b'>').ok()??;
            // As nvim's `in_html_tag`: a `>` right after a `/` ends no tag
            // the caret could be in (so `<</>` leaves it on its `<`).
            if bytes[gt - 1] != b'/' {
                offset = gt;
                bound = offset + 1;
            }
        }
        None => {}
    }
    let tags = Tags::scan(text, source, offset, clock).ok()?;
    let before = tags.tags.partition_point(|tag| tag.start < bound);
    // Gone on from a `<`, a block whose closing tag starts at or before the
    // caret (one the caret is in, by a `<` in its name) isn't around it.
    tags.first(before, count, |block| {
        block.end > offset && block.close >= close
    })
}

/// The `count`th tag block around a visual selection from `lo` to `hi`
/// (more than one position), as [`around`] finds it but from the position
/// before `lo` (where a tag starting counts), going on out until its
/// closing tag starts at or after `hi`.
fn around_selection(text: &Text, lo: usize, hi: usize, count: u32) -> Option<(Block, Clock)> {
    let clock = Clock::new();
    let from = lo.checked_sub(1)?;
    let source = gather(text, &clock).ok()?;
    let tags = Tags::scan(text, source, from, clock).ok()?;
    let before = tags.tags.partition_point(|tag| tag.start <= from);
    tags.first(before, count, |block| block.close >= hi)
}

/// What one selection command may do: [`crate::search::time_limit`] from
/// before it reads the text, looked at every [`WORK`] steps of gathering,
/// searching and reading names, and before each pass that isn't one of those
/// (so no pass runs on past it). Past it, the object fails.
struct Clock(Deadline);

/// Steps (bytes, graphemes or tags) between looks at a [`Clock`].
pub(crate) const WORK: usize = 1 << 12;

#[cfg(test)]
thread_local! {
    /// Set by a test to have every [`Clock`] on its thread run out.
    pub(crate) static STOPPED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Set by a test to have every [`Clock`] on its thread run out at its
    /// `n`th look, counting from 0 ([`EXPIRED`] says if one did).
    pub(crate) static LOOKS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    /// How many looks clocks on this thread have taken, and whether one ran
    /// out because of [`LOOKS`].
    pub(crate) static LOOKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static EXPIRED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The phase of the command last named ([`Clock::phase`]), and the
    /// phase of each look taken (when [`RECORD`] is set).
    pub(crate) static PHASE: std::cell::Cell<&'static str> = const { std::cell::Cell::new("") };
    pub(crate) static RECORD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(crate) static LOG: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    /// The steps passes have taken since the last look ([`Clock::did`]),
    /// and the most between any two.
    pub(crate) static SINCE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static MOST_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The capacity of what passes on this thread have allocated beyond
    /// [`Run`] entries ([`ENTRIES`]), by [`Clock::allocated`].
    pub(crate) static ALLOCATED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many entries [`Run`] tables on this thread have held.
    pub(crate) static ENTRIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many steps [`Run::from`] lookups on this thread have taken.
    pub(crate) static STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many pattern names have been compiled on this thread, and how
    /// many spans of text a compiled name has been matched against.
    pub(crate) static COMPILED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static SPANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Clock {
    fn new() -> Self {
        // Unit tests don't race the wall clock: under load a pattern-name
        // object would run out of its 2 s and fail. [`STOPPED`] still
        // stops one.
        #[cfg(test)]
        return Self(Deadline::never());
        #[cfg(not(test))]
        Self(Deadline::new())
    }

    fn expired(&self) -> bool {
        #[cfg(test)]
        {
            SINCE.with(|since| {
                MOST_STEPS.set(MOST_STEPS.get().max(since.get()));
                since.set(0);
            });
            if RECORD.get() {
                LOG.with_borrow_mut(|log| log.push(PHASE.get()));
            }
            LOOKED.set(LOOKED.get() + 1);
            let at = LOOKS.get();
            if STOPPED.get() || at.is_some_and(|at| LOOKED.get() > at) {
                EXPIRED.set(true);
                return true;
            }
            // A recording test ends only by its own looks, not by how loaded
            // the machine is.
            if RECORD.get() {
                return false;
            }
        }
        self.0.expired()
    }

    fn check(&self) -> Result<(), Interrupted> {
        match self.expired() {
            true => Err(Interrupted),
            false => Ok(()),
        }
    }

    /// Looks at the clock before a pass takes more steps, which are never
    /// many more than [`WORK`] before the next look: `Err` if it's out of
    /// time, and then the pass doesn't start.
    fn spend(&self, steps: usize) -> Result<(), Interrupted> {
        let _ = steps;
        self.check()
    }

    /// Notes (for a test) that a pass has taken `steps` more steps.
    fn did(&self, steps: usize) {
        #[cfg(test)]
        SINCE.set(SINCE.get() + steps);
        let _ = steps;
    }

    /// Notes (for a test) that a new phase of the command starts.
    fn phase(&self, name: &'static str) {
        #[cfg(test)]
        PHASE.set(name);
        let _ = name;
    }

    /// Notes (for a test) that a pass allocated `capacity` elements.
    fn allocated(&self, capacity: usize) {
        #[cfg(test)]
        ALLOCATED.set(ALLOCATED.get() + capacity);
        let _ = capacity;
    }
}

/// The text, as one string.
fn gather(text: &Text, clock: &Clock) -> Result<String, Interrupted> {
    clock.phase("gather");
    let mut source = String::with_capacity(text.len());
    clock.allocated(source.capacity());
    let mut pending = 0;
    for chunk in text.chunks() {
        if pending + chunk.len() > WORK {
            clock.spend(WORK)?;
            pending = 0;
        }
        pending += chunk.len();
        clock.did(chunk.len());
        source.push_str(chunk);
    }
    Ok(source)
}

/// The last byte before `to` that `found` takes.
fn before(
    bytes: &[u8],
    to: usize,
    clock: &Clock,
    found: impl Fn(u8) -> bool,
) -> Result<Option<usize>, Interrupted> {
    let mut to = to.min(bytes.len());
    while to > 0 {
        let from = to.saturating_sub(WORK);
        clock.spend(to - from)?;
        if let Some(ix) = bytes[from..to].iter().rposition(|&byte| found(byte)) {
            clock.did(to - from - ix);
            return Ok(Some(from + ix));
        }
        clock.did(to - from);
        to = from;
    }
    Ok(None)
}

/// The first byte at or after `from` that `found` takes.
fn find(
    bytes: &[u8],
    from: usize,
    clock: &Clock,
    found: impl Fn(u8) -> bool,
) -> Result<Option<usize>, Interrupted> {
    let mut at = from.min(bytes.len());
    while at < bytes.len() {
        let block = &bytes[at..(at + WORK).min(bytes.len())];
        clock.spend(block.len())?;
        if let Some(ix) = block.iter().position(|&byte| found(byte)) {
            clock.did(ix + 1);
            return Ok(Some(at + ix));
        }
        clock.did(block.len());
        at += block.len();
    }
    Ok(None)
}

/// The most bytes a grapheme may take to be read in a bounded step:
/// segmenting is done a grapheme at a time, and one longer than this (a base
/// with that many marks) can't be told in one, so the object fails
/// ([`Interrupted`]) rather than run on past the clock.
const SEGMENT: usize = 2 * WORK;

/// Whether `text` is all ASCII, looking at the clock a [`WORK`] bytes at a
/// time; `Err` if a grapheme of it is longer than [`SEGMENT`]. A grapheme
/// goes on into no ASCII char but a base, so only runs of more than that
/// many bytes without one are segmented here, each grapheme read from a
/// window of two segments (never past the end of its run).
fn bounded(text: &str, clock: &Clock) -> Result<bool, Interrupted> {
    let bytes = text.as_bytes();
    let (mut ascii, mut run_from) = (true, None::<usize>);
    for (chunk_ix, chunk) in bytes.chunks(WORK).enumerate() {
        clock.spend(chunk.len())?;
        clock.did(chunk.len());
        for (ix, &byte) in chunk.iter().enumerate() {
            let at = chunk_ix * WORK + ix;
            if byte < 0x80 {
                if let Some(from) = run_from.take() {
                    segmentable(&text[from..at], clock)?;
                }
            } else {
                ascii = false;
                run_from.get_or_insert(at);
            }
        }
    }
    if let Some(from) = run_from {
        segmentable(&text[from..], clock)?;
    }
    Ok(ascii)
}

/// `run`'s graphemes, none longer than [`SEGMENT`] (checked only if the run
/// is).
fn segmentable(run: &str, clock: &Clock) -> Result<(), Interrupted> {
    if run.len() <= SEGMENT {
        return Ok(());
    }
    let mut at = 0;
    let mut step = 0usize;
    while at < run.len() {
        let mut end = (at + 2 * SEGMENT).min(run.len());
        while !run.is_char_boundary(end) {
            end -= 1;
        }
        let window = &run[at..end];
        let len = window.graphemes(true).next().map_or(window.len(), str::len);
        if len > SEGMENT || (len == window.len() && end < run.len()) {
            return Err(Interrupted);
        }
        if step.is_multiple_of(WORK) {
            clock.spend(WORK)?;
        }
        step += 1;
        clock.did(1);
        at += len;
    }
    Ok(())
}

/// `source[range]`, or an empty string if that isn't whole chars (a name's
/// bounds always are).
fn slice(source: &str, range: std::ops::Range<usize>) -> &str {
    source.get(range).unwrap_or_default()
}

/// A tag: `</name>`, or `<name>` or `<name …>` (not ending `/>`). Its name
/// is kept as where it is and what [`Tags`] needs to pair it, not as its
/// units (overlapping tags, `<<<…`, would take room by the square of their
/// run, a long name by its length): [`Tag::units`] reads them as they're
/// compared.
struct Tag {
    start: usize,
    /// Past its `>`.
    end: usize,
    /// Where its name starts and ends.
    name_start: usize,
    name_end: usize,
    closing: bool,
    /// An opening tag whose `<` has composing chars: the name is what
    /// follows them, and it's no other tag's nested opening tag.
    marked: bool,
    /// An opening tag whose name holds a char magic in a pattern (`\ . *
    /// [ ~`): it pairs as a pattern ([`pattern_partner`]).
    pattern: bool,
    /// Its name's units hashed ([`Unit::key`]): equal names hash alike.
    key: u64,
    /// Its name's graphemes.
    units: usize,
    /// Whether a char of its name has composing chars.
    composed: bool,
    /// Whether its name ends in a word char, as `\>` takes it.
    word: bool,
    /// Whether `partner` is known (else, for a lone or pattern opening tag,
    /// it's found when asked for).
    paired: bool,
    /// An opening tag's partner, the `</name>` closing it (one name's tags
    /// nest): where it starts and ends.
    partner: Option<(usize, usize)>,
}

impl Tag {
    /// An opening tag whose name is all composing chars (on its `<`).
    fn nameless(&self) -> bool {
        !self.closing && self.units == 0
    }

    /// Whether its partner is found by [`partner`], not by the name's
    /// stack of opening tags: if it's marked, if a char of its name has
    /// composing chars (such a name may match another without the other
    /// matching it), or if no tag nests in it ([`Tag::nests`]).
    fn lone(&self) -> bool {
        !self.closing
            && !self.pattern
            && self.units > 0
            && (self.marked || !self.nests() || self.composed)
    }

    /// Whether opening tags of its name nest in it: if the name ends in a
    /// word char, as nvim's `<name\>` finds them.
    fn nests(&self) -> bool {
        self.word
    }

    /// Whether its name is `other`'s: the same bytes, or the same units.
    fn same_name(&self, other: &Tag, source: &str, clock: &Clock) -> Result<bool, Interrupted> {
        if self.key != other.key || self.units != other.units {
            return Ok(false);
        }
        let (one, two) = (self.written(source), other.written(source));
        // The bytes, a stretch at a time.
        let same = one.len() == two.len()
            && one
                .chunks(WORK)
                .zip(two.chunks(WORK))
                .try_fold(true, |same, (one, two)| {
                    clock.spend(one.len())?;
                    clock.did(one.len());
                    Ok::<_, Interrupted>(same && one == two)
                })?;
        if same {
            return Ok(true);
        }
        for (at, (unit, other)) in self.units(source).zip(other.units(source)).enumerate() {
            if at % WORK == 0 {
                clock.spend(WORK)?;
            }
            clock.did(1);
            if unit != other {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Its name's bytes.
    fn written<'b>(&self, source: &'b str) -> &'b [u8] {
        slice(source, self.name_start..self.name_end).as_bytes()
    }

    /// Its name's units, read from `source`.
    fn units<'b>(&self, source: &'b str) -> impl Iterator<Item = Unit> + 'b {
        slice(source, self.name_start..self.name_end)
            .graphemes(true)
            .map(Unit::new)
    }
}

/// The text's tags in order, from its start through `offset`, and on until
/// every opening tag at or before it has its partner (or the text ends, as
/// it must for a [`Tag::lone`] one), with what finding the partners of the
/// lone and pattern ones needs: those are found as they're asked for
/// ([`Tags::first`]), all on the command's [`Clock`].
struct Tags<'a> {
    tags: Vec<Tag>,
    text: &'a Text,
    source: String,
    clock: Clock,
    /// The private-use chars of `source` (see [`private_use`]), read when a
    /// pattern name first needs them.
    private: RefCell<Option<Rc<Vec<u64>>>>,
    /// Each pattern name's [`Compiled`] pairing, by its text: `None` if it
    /// pairs with nothing.
    compiled: RefCell<HashMap<String, Option<Rc<Compiled>>>>,
}

impl<'a> Tags<'a> {
    fn scan(
        text: &'a Text,
        source: String,
        offset: usize,
        clock: Clock,
    ) -> Result<Self, Interrupted> {
        let bytes = source.as_bytes();
        let mut tags: Vec<Tag> = Vec::new();
        // Each name's opening tags without a partner yet, by key.
        let mut open: HashMap<u64, Vec<usize>> = HashMap::new();
        // Those at or before `offset`.
        let mut pending = 0usize;
        // Whether one at or before `offset` is [`Tag::lone`]: its partner is
        // found once every tag is in.
        let mut lone = false;
        let mut names = Names::default();
        clock.phase("scan");
        let mut at = 0;
        let mut seen = 0usize;
        while let Some(found) = find(bytes, at, &clock, |byte| byte == b'<')? {
            if found > offset && pending == 0 && !lone {
                break;
            }
            seen += 1;
            clock.did(1);
            if seen.is_multiple_of(256) {
                clock.spend(256)?;
            }
            at = found + 1;
            let Some(tag) = tag(&source, found, &mut names, &clock)? else {
                continue;
            };
            if tag.nameless() || tag.lone() || tag.pattern {
                lone |= tag.lone() && found <= offset;
                tags.push(tag);
                continue;
            }
            let stack = open.entry(tag.key).or_default();
            if !tag.closing {
                stack.push(tags.len());
                pending += usize::from(found <= offset);
            } else {
                // Of the same key, the innermost opening tag of the same name.
                let mut same = None;
                for (at, &opening) in stack.iter().enumerate().rev() {
                    if tags[opening].same_name(&tag, &source, &clock)? {
                        same = Some(at);
                        break;
                    }
                }
                if let Some(opening) = same.map(|at| stack.remove(at)) {
                    tags[opening].partner = Some((tag.start, tag.end));
                    pending -= usize::from(tags[opening].start <= offset);
                }
            }
            tags.push(tag);
        }
        // A lone tag that nothing nests in and whose name has no composing
        // chars pairs with the first closing tag of the same name: one pass
        // back finds them all.
        let mut next: HashMap<u64, usize> = HashMap::new();
        clock.phase("pair");
        clock.spend(1)?;
        for ix in (0..tags.len()).rev() {
            if ix % WORK == 0 {
                clock.spend(WORK)?;
            }
            clock.did(1);
            let tag = &tags[ix];
            if tag.closing {
                next.insert(tag.key, ix);
            } else if tag.lone() && !tag.nests() && !tag.composed && tag.start <= offset {
                let close = next.get(&tag.key).map(|&close| &tags[close]);
                match close {
                    Some(close) if !close.same_name(tag, &source, &clock)? => continue,
                    Some(close) => tags[ix].partner = Some((close.start, close.end)),
                    None => {}
                }
                tags[ix].paired = true;
            }
        }
        Ok(Self {
            tags,
            text,
            source,
            clock,
            private: RefCell::new(None),
            compiled: RefCell::new(HashMap::new()),
        })
    }

    /// The partner of the opening tag at index `ix`, finding it if it's a
    /// lone or pattern one not found yet: `Err` if that ran out of time.
    fn partner(&self, ix: usize) -> Result<Option<(usize, usize)>, Interrupted> {
        self.clock.check()?;
        let tag = &self.tags[ix];
        if tag.paired || !(tag.lone() || tag.pattern) {
            return Ok(tag.partner);
        }
        if tag.lone() {
            let at = partner(&self.tags, &self.source, ix, &self.clock)?;
            return Ok(at.map(|at| (self.tags[at].start, self.tags[at].end)));
        }
        let name = slice(&self.source, tag.name_start..tag.name_end);
        let Some(compiled) = self.compiled(name)? else {
            return Ok(None);
        };
        pattern_partner(self.text, tag.start, &compiled, &self.clock)
    }

    /// `name` as a pattern, compiled the first time it's asked for.
    fn compiled(&self, name: &str) -> Result<Option<Rc<Compiled>>, Interrupted> {
        let known = self.compiled.borrow().get(name).cloned();
        if let Some(found) = known {
            return Ok(found);
        }
        let known = self.private.borrow().clone();
        let private = match known {
            Some(private) => private,
            None => {
                let private = Rc::new(private_use(&self.source, &self.clock)?);
                *self.private.borrow_mut() = Some(private.clone());
                private
            }
        };
        #[cfg(test)]
        COMPILED.set(COMPILED.get() + 1);
        let found = Compiled::new(name, &private, &self.clock)?.map(Rc::new);
        self.clock.allocated(name.len());
        self.compiled
            .borrow_mut()
            .insert(name.to_string(), found.clone());
        Ok(found)
    }

    /// Where the inside of the opening `tag`'s block, closed at `close`,
    /// starts: past the tag's first `>` outside a quoted attribute value,
    /// or with none, at the tag itself.
    fn inner(&self, tag: &Tag, close: usize) -> Result<usize, Interrupted> {
        self.clock.phase("inner");
        let mut quote = None;
        let skipped = self.source.as_bytes().iter().enumerate().skip(tag.name_end);
        for (step, (at, &byte)) in skipped.enumerate() {
            if step % WORK == 0 {
                self.clock.spend(WORK)?;
            }
            self.clock.did(1);
            match (quote, byte) {
                (Some(open), _) if byte == open => quote = None,
                (Some(_), _) => {}
                (None, b'"' | b'\'') => quote = Some(byte),
                (None, b'>') => return Ok((at + 1).min(close)),
                _ => {}
            }
        }
        Ok(tag.start)
    }

    /// Back from the tag before index `before`, the opening tags no later
    /// one closes, from the `count`th on (passed over unpaired), the first
    /// whose block `fits`; none if one fails the object first: a nameless
    /// one, or one whose pairing ran out of time. Only those from the
    /// `count`th on are paired.
    fn first(
        self,
        before: usize,
        count: u32,
        fits: impl Fn(&Block) -> bool,
    ) -> Option<(Block, Clock)> {
        let mut depth = 0usize;
        let mut skip = count as usize - 1;
        self.clock.phase("count");
        self.clock.spend(1).ok()?;
        for (step, ix) in (0..before).rev().enumerate() {
            if step % WORK == 0 {
                self.clock.spend(WORK).ok()?;
            }
            self.clock.did(1);
            let tag = &self.tags[ix];
            match (tag.closing, depth) {
                (true, _) => {
                    depth += 1;
                    continue;
                }
                (false, 0) => {}
                (false, _) => {
                    depth -= 1;
                    continue;
                }
            }
            if skip > 0 {
                skip -= 1;
                continue;
            }
            match self.partner(ix) {
                Ok(Some((close, end))) => {
                    let mut block = Block {
                        start: tag.start,
                        inner: tag.start,
                        close,
                        end,
                    };
                    if fits(&block) {
                        block.inner = self.inner(tag, close).ok()?;
                        return Some((block, self.clock));
                    }
                }
                Ok(None) if !tag.nameless() => {}
                _ => return None,
            }
        }
        None
    }
}

/// The partner of the [`Tag::lone`] opening tag at index `ix`: the first
/// closing tag after it that its name [`matches`] and that the unmarked
/// opening tags it matches in between (if it [`Tag::nests`]) leave
/// unmatched.
fn partner(
    tags: &[Tag],
    source: &str,
    ix: usize,
    clock: &Clock,
) -> Result<Option<usize>, Interrupted> {
    let seeker = &tags[ix];
    let nests = seeker.nests();
    let mut depth = 0usize;
    clock.phase("partner");
    clock.spend(1)?;
    for (at, tag) in tags.iter().enumerate().skip(ix + 1) {
        if at % WORK == 0 {
            clock.spend(WORK)?;
        }
        clock.did(1);
        let opening_nests = !tag.closing && (tag.marked || !nests);
        if opening_nests
            || tag.units != seeker.units
            || !matches(seeker.units(source), tag.units(source), clock)?
        {
            continue;
        }
        match (tag.closing, depth) {
            (false, _) => depth += 1,
            (true, 0) => return Ok(Some(at)),
            (true, _) => depth -= 1,
        }
    }
    Ok(None)
}

/// The partner of the opening tag at `start` whose name is a pattern
/// ([`Tag::pattern`]), as nvim pairs it, case-blind: the first `</name>`
/// after it (ending at its first `>`) that the `<name`s in between leave
/// unmatched, where a `<name` ends in a word char and then a blank, `>` or
/// the row's end. Matches may overlap, so each `<` that a tag's name could
/// start at is tried, against each span it could take: one search of a row
/// finds where `<name` or `</name` match, and only those `<` are tried.
/// `Err` if pairing ran out of time.
fn pattern_partner(
    text: &Text,
    start: usize,
    compiled: &Compiled,
    clock: &Clock,
) -> Result<Option<(usize, usize)>, Interrupted> {
    let deadline = &clock.0;
    clock.phase("pairing");
    let whole = |pattern: &Pattern, span: &str| -> Result<bool, Interrupted> {
        #[cfg(test)]
        SPANS.set(SPANS.get() + 1);
        clock.spend(1)?;
        let span = compiled.stand_in(span, clock)?;
        clock.did(span.text.len());
        clock.allocated(span.text.len());
        let span = Text::new(&span.text);
        Ok(first_in_row(&span, 0, pattern, deadline)?.is_some())
    };
    let mut depth = 0usize;
    let mut buf = String::new();
    for row in text.offset_to_point(start).row..text.row_count() {
        clock.check()?;
        let (row_start, hay) = row_text(text, row, &mut buf);
        if find(hay.as_bytes(), 0, clock, |byte| byte == b'<')?.is_none() {
            continue;
        }
        bounded(hay, clock)?;
        let heads = compiled.heads(text, row, row_start, hay, clock)?;
        let heads = heads.as_ref();
        let hit = |ranges: Option<&Vec<Range<usize>>>, column: usize| {
            ranges.is_none_or(|ranges| {
                let ix = ranges.partition_point(|range| range.end <= column);
                ranges.get(ix).is_some_and(|range| range.start <= column)
            })
        };
        for (column, _) in hay.match_indices('<') {
            if row_start + column <= start {
                continue;
            }
            clock.check()?;
            clock.did(1);
            let rest = &hay[column..];
            let mut closes = false;
            let mut first = 0;
            if rest.starts_with("</") && hit(heads.map(|heads| &heads.1), column) {
                let gts = rest.match_indices('>').map(|(at, _)| at + 1);
                for (step, gt) in gts.enumerate() {
                    if step == 0 {
                        first = gt;
                    }
                    if whole(&compiled.close, &rest[..gt])? {
                        closes = true;
                        break;
                    }
                }
            }
            if closes {
                match depth {
                    0 => return Ok(Some((row_start + column, row_start + column + first))),
                    _ => depth -= 1,
                }
                continue;
            }
            if !hit(heads.map(|heads| &heads.0), column) {
                continue;
            }
            let ends = rest
                .match_indices([' ', '\t', '>'])
                .map(|(at, _)| at)
                .chain([rest.len()]);
            for end in ends {
                let word = rest[..end]
                    .graphemes(true)
                    .next_back()
                    .is_some_and(|last| end > 1 && matches!(class(last, false), Class::Of(2..)));
                if word && whole(&compiled.open, &rest[..end])? {
                    depth += 1;
                    break;
                }
            }
        }
    }
    Ok(None)
}

/// Most distinct joined graphemes of one base char a pattern name may hold
/// (each takes a stand-in for every combination of them a text grapheme
/// could satisfy).
const CLUSTERS: usize = 16;

/// The most a pattern name may spell out to, once its joined graphemes and
/// character-class atoms are put as collections.
const SPELLED: usize = 1 << 16;

/// Where the private-use chars [`Compiled`] stands in with start, and how
/// many there are (planes 15 and 16).
const PRIVATE_START: u32 = 0xf_0000;
const PRIVATE_CHARS: usize = 0x2_0000;

/// The private-use chars (planes 15 and 16) of `source`, a bit each.
fn private_use(source: &str, clock: &Clock) -> Result<Vec<u64>, Interrupted> {
    let mut used = vec![0u64; PRIVATE_CHARS / 64];
    clock.allocated(used.capacity());
    let mut at = 0;
    // Their UTF-8 starts with 0xf3 or 0xf4, which no other byte does.
    while let Some(found) = find(source.as_bytes(), at, clock, |byte| {
        matches!(byte, 0xf3 | 0xf4)
    })? {
        clock.did(1);
        at = found + 1;
        let ch = source.get(found..).and_then(|rest| rest.chars().next());
        if let Some(bit) = ch.and_then(|ch| (ch as u32).checked_sub(PRIVATE_START))
            && (bit as usize) < PRIVATE_CHARS
        {
            used[bit as usize / 64] |= 1 << (bit % 64);
        }
    }
    Ok(used)
}

/// Marks in `used` the private-use chars `name` names by an escape
/// (`\U000f0000`, `\%u..`, `\x..`, `\d..`, `\o..`).
fn named_private_use(name: &str, used: &mut [u64], clock: &Clock) -> Result<(), Interrupted> {
    let bytes = name.as_bytes();
    let mut ix = 0;
    while ix < bytes.len() {
        if ix % WORK == 0 {
            clock.spend(WORK)?;
        }
        clock.did(1);
        ix += 1;
        if bytes[ix - 1] != b'\\' {
            continue;
        }
        let mut at = ix;
        if bytes.get(at) == Some(&b'%') {
            at += 1;
        }
        let (radix, most) = match bytes.get(at) {
            Some(b'U') => (16, 8),
            Some(b'u') => (16, 4),
            Some(b'x') => (16, 2),
            Some(b'd') => (10, 10),
            Some(b'o') => (8, 4),
            _ => continue,
        };
        let digits = bytes[at + 1..]
            .iter()
            .take(most)
            .take_while(|&&byte| char::from(byte).is_digit(radix))
            .count();
        let value = u32::from_str_radix(&name[at + 1..at + 1 + digits], radix).unwrap_or(0);
        if let Some(bit) = value.checked_sub(PRIVATE_START)
            && (bit as usize) < PRIVATE_CHARS
        {
            used[bit as usize / 64] |= 1 << (bit % 64);
        }
    }
    Ok(())
}

/// A pattern name read as a pattern, to pair its tags ([`pattern_partner`]):
/// compiled once for all the tags of the name.
struct Compiled {
    /// `<name` and `</name`, each whole: the spans a tag can take.
    open: Pattern,
    close: Pattern,
    /// `<name` and `</name`, wherever they start: where a tag could start.
    /// None if what comes after the name could change what it matches.
    heads: Option<(Pattern, Pattern)>,
    /// The graphemes of the name that join chars other than composing ones
    /// (a ZWJ, an emoji modifier), as their base and other chars. fe-vim's
    /// `/` would fold those as separate chars; nvim takes one as a char
    /// matching only the same base with at least its other chars.
    clusters: Vec<(char, Vec<char>)>,
    /// The `clusters` of each base char, by their index.
    groups: HashMap<char, Vec<usize>>,
    /// What stands in, in the name and the text, for a grapheme of a base
    /// by which of that base's clusters it satisfies (a bit each, in the
    /// order of its group): a grapheme satisfying none stands for itself.
    stand_ins: HashMap<(char, usize), char>,
}

/// The text with its joined graphemes put as [`Compiled`]'s stand-ins, and
/// where its offsets were.
struct Standin<'a> {
    text: std::borrow::Cow<'a, str>,
    /// Where each stand-in ends, and where what it stands for ended.
    shifts: Vec<(usize, usize)>,
}

impl Standin<'_> {
    /// The offset in the original of `at` in the text.
    fn original(&self, at: usize) -> usize {
        let ix = self.shifts.partition_point(|&(end, _)| end <= at);
        match ix.checked_sub(1).map(|ix| self.shifts[ix]) {
            Some((end, was)) => was + (at - end),
            None => at,
        }
    }
}

impl Compiled {
    /// `name` as nvim reads a pattern, case-blind; `None` if it pairs with
    /// nothing: if it isn't a pattern on its own (one fe-vim can't read, or
    /// see [`literals`]), if a char magic in a pattern is joined to others
    /// (`.` takes its whole grapheme, so nothing is left for them), or if
    /// there are too many joined graphemes of one base or too few
    /// private-use chars left to stand for them. `Err` if that ran out of
    /// time, or the name spells out to more than [`SPELLED`].
    fn new(name: &str, private: &[u64], clock: &Clock) -> Result<Option<Self>, Interrupted> {
        clock.phase("prepare");
        bounded(name, clock)?;
        let Some(scan) = literals(name, clock)? else {
            return Ok(None);
        };
        for (step, &dot) in scan.dots.iter().enumerate() {
            if step % WORK == 0 {
                clock.spend(WORK)?;
            }
            clock.did(1);
            let dot = name[dot..].graphemes(true).next().map_or(0, str::len);
            if dot > 1 {
                return Ok(None);
            }
        }
        // The joined graphemes written literally, each once, by base.
        let mut written: Vec<(usize, usize, usize)> = Vec::new();
        let mut found: Vec<&str> = Vec::new();
        let mut clusters: Vec<(char, Vec<char>)> = Vec::new();
        let mut groups: HashMap<char, Vec<usize>> = HashMap::new();
        for (step, (at, grapheme)) in name.grapheme_indices(true).enumerate() {
            if step % WORK == 0 {
                clock.spend(WORK)?;
            }
            clock.did(grapheme.len().div_ceil(4));
            if !(scan.literal[at] && joined(grapheme)) {
                continue;
            }
            let ix = match found.iter().position(|&seen| seen == grapheme) {
                Some(ix) => ix,
                None => {
                    let (base, rest) = units(grapheme);
                    let group = groups.entry(base).or_default();
                    if group.len() == CLUSTERS {
                        return Ok(None);
                    }
                    group.push(found.len());
                    found.push(grapheme);
                    clusters.push((base, rest));
                    found.len() - 1
                }
            };
            written.push((at, grapheme.len(), ix));
        }
        // The private-use chars the name itself names by an escape are taken
        // too, or a collection naming one would match its stand-in.
        let mut private = private.to_vec();
        named_private_use(name, &mut private, clock)?;
        let mut next_bit = 0usize;
        let mut stand_ins = HashMap::new();
        for (&base, group) in &groups {
            for mask in 1..1usize << group.len() {
                let free = loop {
                    if next_bit.is_multiple_of(WORK) {
                        clock.spend(WORK)?;
                    }
                    clock.did(1);
                    let bit = next_bit;
                    next_bit += 1;
                    if bit >= PRIVATE_CHARS {
                        return Ok(None);
                    }
                    let ch = char::from_u32(PRIVATE_START + bit as u32);
                    match ch {
                        Some(ch)
                            if private[bit / 64] >> (bit % 64) & 1 == 0
                                && ch as u32 & 0xfffe != 0xfffe =>
                        {
                            break ch;
                        }
                        _ => {}
                    }
                };
                stand_ins.insert((base, mask), free);
            }
        }
        clock.allocated(stand_ins.len());
        // The members of the collection of what satisfies each cluster.
        let mut members: Vec<String> = Vec::with_capacity(clusters.len());
        for (ix, (base, _)) in clusters.iter().enumerate() {
            clock.spend(1)?;
            let bit = groups[base]
                .iter()
                .position(|&other| other == ix)
                .unwrap_or(0);
            let mut chars: Vec<char> = stand_ins
                .iter()
                .filter(|&(&(other, mask), _)| other == *base && mask >> bit & 1 == 1)
                .map(|(_, &ch)| ch)
                .collect();
            chars.sort_unstable();
            members.push(chars.into_iter().collect());
        }
        // What each atom of the name that needs a collection of its own
        // is spelled as: a joined grapheme, as what satisfies it, and an
        // atom `\k`, `\i`, `\f`, `\p` or its capital, as its members.
        let mut atom_members: HashMap<char, String> = HashMap::new();
        let mut spelled = String::with_capacity(name.len());
        let mut done = 0;
        let (mut cluster, mut atom, mut collection) = (0, 0, 0);
        // The stand-ins of each base char (for what's in a collection).
        let mut of_base: HashMap<char, String> = HashMap::new();
        for (&(base, _), &ch) in &stand_ins {
            of_base.entry(base).or_default().push(ch);
        }
        loop {
            clock.spend(1)?;
            clock.did(1);
            // A collection with a base char of a joined grapheme in its
            // set takes that grapheme's stand-ins in.
            if let Some(&(open, length, end)) = scan.collections.get(collection)
                && scan.atoms.get(atom).is_none_or(|&(at, _)| open < at)
                && written.get(cluster).is_none_or(|&(at, ..)| open < at)
            {
                collection += 1;
                let set = &name[open..end];
                let negated = set[length..].starts_with('^');
                let mut extra = String::new();
                for (&base, group) in &of_base {
                    clock.spend(1)?;
                    let Ok(pattern) =
                        Pattern::parse(&format!("\\c^\\{}{set}\\m$", scan.mode[open] as char))
                    else {
                        break;
                    };
                    let mut buf = [0; 4];
                    let one = Text::new(base.encode_utf8(&mut buf));
                    let found = first_in_row(&one, 0, &pattern, &clock.0)?.is_some();
                    if found != negated {
                        extra.push_str(group);
                    }
                }
                if !extra.is_empty() {
                    if spelled.len() + extra.len() + open - done > SPELLED {
                        clock.allocated(spelled.capacity());
                        return Err(Interrupted);
                    }
                    spelled.push_str(&name[done..end - 1]);
                    spelled.push_str(&extra);
                    done = end - 1;
                }
                continue;
            }
            let next_cluster = written.get(cluster).map(|&(at, ..)| at);
            let next_atom = scan.atoms.get(atom).map(|&(at, _)| at);
            let (at, len, members): (usize, usize, &str) = match (next_cluster, next_atom) {
                (Some(at), Some(other)) if at < other => {
                    cluster += 1;
                    (at, written[cluster - 1].1, &members[written[cluster - 1].2])
                }
                (Some(at), None) => {
                    cluster += 1;
                    (at, written[cluster - 1].1, &members[written[cluster - 1].2])
                }
                (_, Some(_)) => {
                    let (at, kind) = scan.atoms[atom];
                    atom += 1;
                    // The stand-ins of the graphemes whose base is in the
                    // class are in it too.
                    let with = atom_members.entry(kind).or_insert_with(|| {
                        let mut members = class_members(kind).to_string();
                        for (&(base, _), &ch) in &stand_ins {
                            if class_has(kind, u32::from(base)) {
                                members.push(ch);
                            }
                        }
                        members
                    });
                    (at, 2, with.as_str())
                }
                (None, None) => break,
            };
            if at < done {
                continue;
            }
            if spelled.len() + members.len() + at - done > SPELLED {
                clock.allocated(spelled.capacity());
                return Err(Interrupted);
            }
            clock.spend(members.len().min(WORK))?;
            spelled.push_str(&name[done..at]);
            // In the nomagic modes, a collection is `\[`.
            match scan.mode[at] {
                b'v' | b'm' => spelled.extend(["[", members, "]"]),
                _ => spelled.extend(["\\[", members, "]"]),
            }
            done = at + len;
        }
        if spelled.len() + name.len() - done > SPELLED {
            clock.allocated(spelled.capacity());
            return Err(Interrupted);
        }
        spelled.push_str(&name[done..]);
        clock.allocated(spelled.capacity());
        // Compiling can't be interrupted: it is the work of what's compiled,
        // looked at before it starts.
        for _ in 0..4 * spelled.len().div_ceil(WORK) {
            clock.spend(WORK)?;
            clock.did(WORK);
        }
        // `\c` wins over a `\C` in the name, and `\m` undoes its `\v` or `\V`.
        clock.spend(1)?;
        let Ok(open) = Pattern::parse(&format!("\\c^<{spelled}\\m$")) else {
            return Ok(None);
        };
        clock.spend(1)?;
        let Ok(close) = Pattern::parse(&format!("\\c^</{spelled}\\m>$")) else {
            return Ok(None);
        };
        // A `$` in the name could end the span, not the text, and a `\z`
        // (`\zs`, `\ze`) moves where a match starts or ends from its `<`.
        let mut heads = None;
        if !name.contains('$') && !name.contains("\\z") {
            clock.spend(1)?;
            let found = Pattern::parse(&format!("\\c<{spelled}"));
            clock.spend(1)?;
            if let (Ok(found_open), Ok(found_close)) =
                (found, Pattern::parse(&format!("\\c</{spelled}")))
            {
                heads = Some((found_open, found_close));
            }
        }
        Ok(Some(Self {
            open,
            close,
            heads,
            clusters,
            groups,
            stand_ins,
        }))
    }

    /// `text` with each of its graphemes that satisfies some of the
    /// clusters (the same base char, with at least their other chars) put as
    /// the stand-in for those.
    fn stand_in<'t>(&self, text: &'t str, clock: &Clock) -> Result<Standin<'t>, Interrupted> {
        if self.clusters.is_empty() {
            return Ok(Standin {
                text: text.into(),
                shifts: Vec::new(),
            });
        }
        bounded(text, clock)?;
        let mut put = String::with_capacity(text.len());
        let mut shifts = Vec::new();
        for (step, (at, grapheme)) in text.grapheme_indices(true).enumerate() {
            if step % WORK == 0 {
                clock.spend(WORK)?;
            }
            clock.did(grapheme.len().div_ceil(4));
            let stand_in = match Unit::new(grapheme) {
                Unit::Char(_) => None,
                Unit::Composed(base, rest) => self.groups.get(&base).and_then(|group| {
                    let mut mask = 0;
                    for (bit, &ix) in group.iter().enumerate() {
                        if self.clusters[ix].1.iter().all(|ch| rest.contains(ch)) {
                            mask |= 1 << bit;
                        }
                    }
                    self.stand_ins.get(&(base, mask))
                }),
            };
            match stand_in {
                None => put.push_str(grapheme),
                Some(&ch) => {
                    put.push(ch);
                    shifts.push((put.len(), at + grapheme.len()));
                }
            }
        }
        clock.allocated(put.capacity() + shifts.capacity());
        Ok(Standin {
            text: put.into(),
            shifts,
        })
    }

    /// Where on `row` (`hay`, from `row_start`) a `<name` and a `</name`
    /// match, as ranges in `hay`, found by one search of the row each: the
    /// rest of the tags' `<` can't start a tag with the name. None if the
    /// name doesn't allow that.
    #[allow(clippy::type_complexity)]
    fn heads(
        &self,
        text: &Text,
        row: u32,
        row_start: usize,
        hay: &str,
        clock: &Clock,
    ) -> Result<Option<(Vec<Range<usize>>, Vec<Range<usize>>)>, Interrupted> {
        let Some((open, close)) = &self.heads else {
            return Ok(None);
        };
        clock.phase("heads");
        let standin = self.stand_in(hay, clock)?;
        let own = (!self.clusters.is_empty()).then(|| Text::new(&standin.text));
        let (searched, at, base) = match &own {
            Some(own) => (own, 0, 0),
            None => (text, row, row_start),
        };
        let list = |pattern: &Pattern| -> Result<Vec<Range<usize>>, Interrupted> {
            clock.spend(1)?;
            let mut list = RowList::new(searched, at);
            let found = list.list_until(pattern, &clock.0, |_, _| false)?;
            let mut ranges = Vec::with_capacity(found.len());
            for (step, found) in found.iter().enumerate() {
                if step % WORK == 0 {
                    clock.spend(WORK)?;
                }
                clock.did(1);
                ranges
                    .push(standin.original(found.start - base)..standin.original(found.end - base));
            }
            clock.allocated(ranges.capacity());
            Ok(ranges)
        };
        Ok(Some((list(open)?, list(close)?)))
    }
}

/// The base and other chars of a grapheme.
fn units(grapheme: &str) -> (char, Vec<char>) {
    match Unit::new(grapheme) {
        Unit::Composed(base, rest) => (base, rest),
        Unit::Char(base) => (base, Vec::new()),
    }
}

/// What a pattern name holds that [`Compiled`] reads.
struct Scan {
    /// Whether each byte is a char written literally (outside a collection,
    /// not escaped, not magic).
    literal: Vec<bool>,
    /// The magic in effect at each char's first byte: `v`, `m`, `M` or `V`.
    mode: Vec<u8>,
    /// Where the `.`s that are magic are.
    dots: Vec<usize>,
    /// Where the character-class atoms (`\k \i \f \p` and their capitals)
    /// are, and which.
    atoms: Vec<(usize, char)>,
    /// The collections: where each starts (at its `[`, or its `\`), how
    /// long its opening is, and where it ends (past its `]`).
    collections: Vec<(usize, usize, usize)>,
}

/// Where `name`, read as nvim reads a pattern, has literal chars (outside
/// a collection, not escaped): a flag per byte. None if it isn't a pattern
/// on its own, as nvim takes a tag's name: with a `~` that is the last
/// substitute string (there's none), a branch outside a group, a
/// collection without its `]` (it would take in what nvim puts after the
/// name), or ending very magic (`\v`).
fn literals(name: &str, clock: &Clock) -> Result<Option<Scan>, Interrupted> {
    #[derive(Clone, Copy, PartialEq)]
    enum Magic {
        Very,
        On,
        Off,
        VeryOff,
    }
    let mut scan = Scan {
        literal: vec![false; name.len()],
        mode: vec![b'm'; name.len()],
        dots: Vec::new(),
        atoms: Vec::new(),
        collections: Vec::new(),
    };
    clock.allocated(scan.literal.capacity() + scan.mode.capacity());
    let mut magic = Magic::On;
    let mut depth = 0usize;
    let mut chars = name.char_indices().peekable();
    let mut step = 0usize;
    // One more char read: looks at the clock every [`WORK`].
    let mut read = |clock: &Clock| -> Result<(), Interrupted> {
        if step.is_multiple_of(WORK) {
            clock.spend(WORK)?;
        }
        step += 1;
        clock.did(1);
        Ok(())
    };
    while let Some((at, ch)) = chars.next() {
        read(clock)?;
        scan.mode[at] = match magic {
            Magic::Very => b'v',
            Magic::On => b'm',
            Magic::Off => b'M',
            Magic::VeryOff => b'V',
        };
        let escaped = ch == '\\';
        let ch = match escaped {
            true => match chars.next() {
                Some((_, next)) => next,
                None => break,
            },
            false => ch,
        };
        // Whether `ch` is special here: escaped in the nomagic modes, bare
        // in the magic ones (`(` `)` `|` only very magic).
        let special = |very_only: bool| match magic {
            Magic::Very => !escaped,
            Magic::On if very_only => escaped,
            Magic::On => !escaped,
            Magic::Off | Magic::VeryOff => escaped,
        };
        match ch {
            'v' | 'm' | 'M' | 'V' if escaped => {
                magic = match ch {
                    'v' => Magic::Very,
                    'm' => Magic::On,
                    'M' => Magic::Off,
                    _ => Magic::VeryOff,
                }
            }
            'k' | 'K' | 'i' | 'I' | 'f' | 'F' | 'p' | 'P' if escaped => scan.atoms.push((at, ch)),
            '~' if special(false) => return Ok(None),
            '(' if special(true) => depth += 1,
            '%' if special(true) && chars.peek().is_some_and(|&(_, next)| next == '(') => {
                chars.next();
                depth += 1;
            }
            ')' if special(true) => depth = depth.saturating_sub(1),
            '|' if special(true) && depth == 0 => return Ok(None),
            '[' if special(false) => {
                // A `]` first, after any `^`, is itself; `\]` too, and a
                // `[:name:]` holds its own.
                if chars.peek().is_some_and(|&(_, next)| next == '^') {
                    chars.next();
                }
                if chars.peek().is_some_and(|&(_, next)| next == ']') {
                    chars.next();
                }
                let open = at;
                loop {
                    read(clock)?;
                    let Some((place, next)) = chars.next() else {
                        return Ok(None);
                    };
                    match next {
                        ']' => {
                            let length = 1 + usize::from(escaped);
                            scan.collections.push((open, length, place + 1));
                            break;
                        }
                        '\\' => {
                            if chars.next().is_none() {
                                return Ok(None);
                            }
                        }
                        '[' if chars.peek().is_some_and(|&(_, next)| next == ':') => loop {
                            read(clock)?;
                            match chars.next() {
                                None => return Ok(None),
                                Some((_, ']')) => break,
                                Some(_) => {}
                            }
                        },
                        _ => {}
                    }
                }
            }
            '.' if special(false) => scan.dots.push(at),
            '*' if special(false) => {}
            '+' | '?' | '=' | '{' | '@' | '<' | '>' | '&' if special(true) => {}
            _ if !escaped => scan.literal[at..at + ch.len_utf8()].fill(true),
            _ => {}
        }
    }
    Ok((magic != Magic::Very).then_some(scan))
}

/// The ranges (`first-last` in hex) of the code points `\i`, `\f` and `\p`
/// take in nvim 0.12.4 with its default 'isident', 'isfname' and 'isprint'
/// (what `/\i` and the others match, as the recorder `tests/support` reads
/// it), for `\k` see [`class_members`].
const IDENT: &str = "30-39,41-5a,5f-5f,61-7a,b5-b5,c0-ff";
const FILENAME: &str = "23-25,2b-39,3d-3d,41-5a,5f-5f,61-7a,7e-7e,a0-d7ff,e000-10ffff";
const PRINTABLE: &str = "20-7e,a0-70e,710-180a,180f-200a,2010-2029,202f-205f,2070-d7ff,e000-fefe,ff00-fff8,fffc-fffd,10000-10ffff";

/// The members of a collection for the character-class atom `\k`, `\i`,
/// `\f` or `\p` (or the capital, without the digits), as `\u` escapes in
/// ranges: `\k` is the chars nvim's 'iskeyword' counts as word chars
/// (class 2 or more).
fn class_members(atom: char) -> &'static str {
    &class_of(atom).0
}

/// Whether the char `cp` is one `atom` takes.
fn class_has(atom: char, cp: u32) -> bool {
    let ranges = &class_of(atom).1;
    let ix = ranges.partition_point(|&(_, last)| last < cp);
    ranges.get(ix).is_some_and(|&(first, _)| first <= cp)
}

/// One class atom's members spelled, and as ranges.
type ClassSet = (String, Vec<(u32, u32)>);

/// What `atom` (`k i f p`, or a capital) takes, worked out the first time
/// it's asked for.
fn class_of(atom: char) -> &'static ClassSet {
    static CLASSES: [OnceLock<ClassSet>; 8] = [const { OnceLock::new() }; 8];
    let slot = "kifpKIFP".find(atom).unwrap_or(0);
    CLASSES[slot].get_or_init(|| {
        let escape = |cp: u32| match cp {
            0..=0xffff => format!("\\u{cp:04x}"),
            _ => format!("\\U{cp:08x}"),
        };
        let parse = |ranges: &str| -> Vec<(u32, u32)> {
            ranges
                .split(',')
                .filter_map(|range| {
                    let (first, last) = range.split_once('-')?;
                    Some((
                        u32::from_str_radix(first, 16).ok()?,
                        u32::from_str_radix(last, 16).ok()?,
                    ))
                })
                .collect()
        };
        let mut ranges = match atom.to_ascii_lowercase() {
            'i' => parse(IDENT),
            'f' => parse(FILENAME),
            'p' => parse(PRINTABLE),
            _ => {
                let mut keyword: Vec<(u32, u32)> = Vec::new();
                for cp in (0..=0x10_ffffu32).filter(|cp| !(0xd800..0xe000).contains(cp)) {
                    let Some(ch) = char::from_u32(cp) else {
                        continue;
                    };
                    if !matches!(class(ch.encode_utf8(&mut [0; 4]), false), Class::Of(2..)) {
                        continue;
                    }
                    match keyword.last_mut() {
                        Some((_, last)) if *last + 1 == cp => *last = cp,
                        _ => keyword.push((cp, cp)),
                    }
                }
                keyword
            }
        };
        if atom.is_ascii_uppercase() {
            // Without the digits.
            let (digits_from, digits_to) = (u32::from(b'0'), u32::from(b'9'));
            let mut without: Vec<(u32, u32)> = Vec::new();
            for &(first, last) in &ranges {
                if last < digits_from || first > digits_to {
                    without.push((first, last));
                    continue;
                }
                if first < digits_from {
                    without.push((first, digits_from - 1));
                }
                if last > digits_to {
                    without.push((digits_to + 1, last));
                }
            }
            ranges = without;
        }
        let spelled = ranges
            .iter()
            .map(|&(first, last)| match first == last {
                true => escape(first),
                false => format!("{}-{}", escape(first), escape(last)),
            })
            .collect();
        (spelled, ranges)
    })
}

/// The graphemes of `name` written literally that join chars other than
/// composing ones (a ZWJ, an emoji modifier): see [`Compiled::clusters`].
fn joined(grapheme: &str) -> bool {
    static MARK: OnceLock<Regex> = OnceLock::new();
    let mark = MARK.get_or_init(|| Regex::new(r"^\p{M}$").expect("a valid regex"));
    grapheme
        .chars()
        .skip(1)
        .any(|ch| !mark.is_match(ch.encode_utf8(&mut [0; 4])))
}

/// A grapheme of a tag name: one char, case folded, or a char with
/// composing chars after it, as written, with the set of those.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Unit {
    Char(char),
    Composed(char, Vec<char>),
}

impl Unit {
    /// `rest`, the key of the units after it, with it before them: equal
    /// units with equal rests give equal keys.
    fn key(&self, rest: u64) -> u64 {
        let mix = |key: u64, ch: char| (key ^ u64::from(ch)).wrapping_mul(0x0100_0000_01b3);
        let key = match self {
            Self::Char(ch) => mix(rest, *ch),
            Self::Composed(base, rest_chars) => rest_chars
                .iter()
                .fold(mix(rest ^ 0x9e37, *base), |key, &ch| mix(key, ch)),
        };
        key.rotate_left(17)
    }

    /// Whether it's a word char, as `\>` takes it: its base char's class
    /// is 2 or more.
    fn word(&self) -> bool {
        let (Self::Char(base) | Self::Composed(base, _)) = self;
        matches!(class(base.encode_utf8(&mut [0; 4]), false), Class::Of(2..))
    }

    fn new(grapheme: &str) -> Self {
        let mut chars = grapheme.chars();
        let base = chars.next().unwrap_or_default();
        let mut composing: Vec<char> = chars.collect();
        if composing.is_empty() {
            return Self::Char(fold(base));
        }
        composing.sort_unstable();
        composing.dedup();
        Self::Composed(base, composing)
    }
}

/// Whether the opening tag name `name` matches `other`'s, as long:
/// grapheme by grapheme, a char matches the same char case-blind, and a
/// composed one the same char with at least its composing chars.
fn matches(
    name: impl Iterator<Item = Unit>,
    other: impl Iterator<Item = Unit>,
    clock: &Clock,
) -> Result<bool, Interrupted> {
    for (at, pair) in name.zip(other).enumerate() {
        if at % WORK == 0 {
            clock.spend(WORK)?;
        }
        clock.did(1);
        let same = match &pair {
            (Unit::Composed(base, composing), Unit::Composed(other_base, other_composing)) => {
                base == other_base && composing.iter().all(|ch| other_composing.contains(ch))
            }
            (unit, other) => unit == other,
        };
        if !same {
            return Ok(false);
        }
    }
    Ok(true)
}

/// What [`tag`] reads of names, kept between tags: past each `<` in a run
/// (`<<<…`) a name runs to the same end, so the end, and the key, unit
/// count and flags of the name from each place a tag's name can start, are
/// worked out once for the run.
#[derive(Default)]
struct Names {
    /// The first `>` at or after some earlier point.
    gt: usize,
    /// Opening and closing names: the run last read.
    opening: Run,
    closing: Run,
}

/// A run of name bytes from `start` to `end` (the first byte that ends a
/// name), and the name from each place in it a tag's name can start (the
/// run's start, past a `<` in it, and its end), as [`Run::from`] gives it:
/// a few entries per `<`, not one per byte.
#[derive(Default)]
struct Run {
    start: usize,
    end: usize,
    word: bool,
    /// By place, ascending.
    from: Vec<(usize, (u64, usize, bool, bool))>,
    /// Where in `from` the last lookup ended: tags ask in ascending order.
    next: usize,
}

impl Run {
    /// The run holding `at`, read anew if `at` is past the last one.
    fn at(
        &mut self,
        source: &str,
        at: usize,
        closing: bool,
        clock: &Clock,
    ) -> Result<&Self, Interrupted> {
        if at >= self.start && at <= self.end && !self.from.is_empty() {
            return Ok(self);
        }
        let stops = |byte: u8| match closing {
            true => matches!(byte, b'\n' | b'>'),
            false => matches!(byte, b' ' | b'\t' | b'\n' | b'>' | b'/' | b'!'),
        };
        let end = find(source.as_bytes(), at, clock, stops)?.unwrap_or(source.len());
        let text = slice(source, at..end);
        let mut from = std::mem::take(&mut self.from);
        from.clear();
        let mut state = (0u64, 0usize, false, false);
        from.push((end, state));
        let mut word = None;
        // ASCII graphemes are its bytes (a name holds no line break).
        let ascii = bounded(text, clock)?;
        let graphemes: Box<dyn Iterator<Item = (usize, &str)>> = match ascii {
            true => Box::new((0..text.len()).rev().map(|ix| (ix, &text[ix..ix + 1]))),
            false => Box::new(text.grapheme_indices(true).rev()),
        };
        // The one and two graphemes to the right of the one being read:
        // where they start, the name from there, and whether the first is a
        // lone `/`.
        let mut right: Option<(usize, (u64, usize, bool, bool))> = None;
        let mut right2 = None;
        let mut slash = false;
        for (step, (ix, grapheme)) in graphemes.enumerate() {
            if step % WORK == 0 {
                clock.spend(WORK)?;
            }
            clock.did(grapheme.len().div_ceil(4));
            // A tag at this grapheme's `<` names what's to its right: past
            // the `<` and its composing chars, or past `</`.
            let named = match closing {
                false => grapheme.starts_with('<').then_some(right).flatten(),
                true => (grapheme == "<" && slash).then_some(right2).flatten(),
            };
            from.extend(named);
            let unit = Unit::new(grapheme);
            word.get_or_insert_with(|| unit.word());
            let (key, units, composed, magic) = state;
            state = (
                unit.key(key),
                units + 1,
                composed || matches!(unit, Unit::Composed(..)),
                magic || grapheme.contains(['\\', '.', '*', '[', '~']),
            );
            right2 = right;
            right = Some((at + ix, state));
            slash = grapheme == "/";
        }
        // The run's start, and past its first grapheme (a marked `<`'s
        // composing chars).
        if !closing {
            from.extend(right2);
        }
        from.extend(right);
        // In place, a stretch at a time.
        clock.spend(1)?;
        let (mut low, mut high) = (0, from.len());
        while low + 1 < high {
            if low % WORK == 0 {
                clock.spend(WORK)?;
            }
            clock.did(1);
            high -= 1;
            from.swap(low, high);
            low += 1;
        }
        let mut kept = 0;
        for ix in 0..from.len() {
            if ix % WORK == 0 {
                clock.spend(WORK)?;
            }
            clock.did(1);
            if kept == 0 || from[kept - 1].0 != from[ix].0 {
                from[kept] = from[ix];
                kept += 1;
            }
        }
        from.truncate(kept);
        #[cfg(test)]
        ENTRIES.set(ENTRIES.get() + from.len());
        *self = Self {
            start: at,
            end,
            word: word.unwrap_or(false),
            from,
            next: 0,
        };
        Ok(self)
    }

    /// The name from `at` on, if a tag's name can start there; `at` never
    /// goes back within a run, so the lookups step through it once.
    fn from(&mut self, at: usize) -> Option<(u64, usize, bool, bool)> {
        while self
            .from
            .get(self.next)
            .is_some_and(|&(place, _)| place < at)
        {
            self.next += 1;
            #[cfg(test)]
            STEPS.set(STEPS.get() + 1);
        }
        let &(place, name) = self.from.get(self.next)?;
        (place == at).then_some(name)
    }
}

/// The tag whose `<` is at `at`: `</name>`, or `<name>` or `<name …>`
/// (whose attributes run to the first `>`), but not one ending `/>`.
/// Composing chars starting an opening tag's name are the `<`'s, not the
/// name's.
fn tag(
    source: &str,
    at: usize,
    names: &mut Names,
    clock: &Clock,
) -> Result<Option<Tag>, Interrupted> {
    let bytes = source.as_bytes();
    let closing = bytes.get(at + 1) == Some(&b'/');
    let name_start = at + 1 + usize::from(closing);
    let run = match closing {
        true => names.closing.at(source, name_start, true, clock)?,
        false => names.opening.at(source, name_start, false, clock)?,
    };
    let name_end = run.end;
    let word = run.word;
    if name_end == name_start && !closing {
        return Ok(None);
    }
    let end = match (closing, bytes.get(name_end)) {
        (_, Some(b'>')) => name_end + 1,
        (false, Some(b' ' | b'\t' | b'\n')) => {
            if names.gt <= name_end {
                names.gt =
                    find(bytes, name_end, clock, |byte| byte == b'>')?.unwrap_or(bytes.len());
            }
            if names.gt == bytes.len() || bytes[names.gt - 1] == b'/' {
                return Ok(None);
            }
            names.gt + 1
        }
        _ => return Ok(None),
    };
    let name_start = match (closing, lt_len(slice(source, at..name_end))) {
        (true, _) => name_start,
        (false, Some(lt)) => at + lt,
        (false, None) => return Ok(None),
    };
    let run = match closing {
        true => &mut names.closing,
        false => &mut names.opening,
    };
    let Some((key, units, composed, magic)) = run.from(name_start) else {
        return Ok(None);
    };
    Ok(Some(Tag {
        start: at,
        end,
        name_start,
        name_end,
        closing,
        marked: !closing && name_start > at + 1,
        pattern: !closing && magic,
        key,
        units,
        composed,
        word: word && units > 0,
        paired: false,
        partner: None,
    }))
}

/// The length of the grapheme a tag's `<` starts in `written` (the tag
/// from its `<` through its name): the `<` and any composing chars after it.
/// A window of it is read, not all of it, unless the grapheme fills that.
fn lt_len(written: &str) -> Option<usize> {
    let mut window = 64;
    loop {
        let mut end = window.min(written.len());
        while !written.is_char_boundary(end) {
            end -= 1;
        }
        let text = &written[..end];
        let lt = text.graphemes(true).next()?.len();
        if lt < text.len() || end == written.len() {
            return Some(lt);
        }
        window *= 4;
    }
}

/// The char `ch` matches case-blind, as nvim's `\c` folds case: its
/// uppercase's lowercase, if each is one char, but for the dotless `ı` and
/// capital sharp `ẞ`, which match only themselves.
fn fold(ch: char) -> char {
    fn single(mut chars: impl Iterator<Item = char>) -> Option<char> {
        match (chars.next(), chars.next()) {
            (Some(ch), None) => Some(ch),
            _ => None,
        }
    }
    if matches!(ch, '\u{131}' | '\u{1e9e}') {
        return ch;
    }
    single(ch.to_uppercase()).map_or(ch, |upper| single(upper.to_lowercase()).unwrap_or(upper))
}

#[cfg(test)]
#[path = "tests/delimiters_work.rs"]
mod work_tests;
