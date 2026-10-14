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
//!   by a ZWJ or modifier kept whole and unfolded; one that isn't a pattern
//!   on its own (`<a[>`, `<a\|b>`) pairs with nothing, and one whose
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

use std::{collections::HashMap, sync::OnceLock};

use fe_text::Text;
use regex::Regex;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    Pending, TextObject,
    feature::Feature,
    motion::{Class, Cursor, PAIRS, class},
    operator::{Span, row_end},
    search::{Deadline, Interrupted, Pattern, first_in_row, row_text},
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
    let bytes = gather(text, &clock).ok()?;
    let row_start = before(&bytes, offset, &clock, |byte| byte == b'\n')
        .ok()?
        .map_or(0, |at| at + 1);
    let row_end = find(&bytes, offset, &clock, |byte| byte == b'\n')
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
            offset = find(&bytes, offset, &clock, |byte| byte == b'>').ok()??;
            bound = offset + 1;
        }
        None => {}
    }
    let tags = Tags::scan(text, bytes, offset, clock).ok()?;
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
    let bytes = gather(text, &clock).ok()?;
    let tags = Tags::scan(text, bytes, from, clock).ok()?;
    let before = tags.tags.partition_point(|tag| tag.start <= from);
    tags.first(before, count, |block| block.close >= hi)
}

/// What one selection command may do: [`crate::search::time_limit`] from
/// before it reads the text, looked at every [`WORK`] steps of gathering,
/// searching and reading names. Past it, the object fails.
struct Clock(Deadline);

/// Steps (bytes, graphemes or tags) between looks at a [`Clock`].
const WORK: usize = 1 << 14;

#[cfg(test)]
thread_local! {
    /// Set by a test to have every [`Clock`] on its thread run out.
    pub(crate) static STOPPED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// How many entries [`Run`] tables on this thread have held.
    pub(crate) static ENTRIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many steps [`Run::from`] lookups on this thread have taken.
    pub(crate) static STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
        if STOPPED.get() {
            return true;
        }
        self.0.expired()
    }

    fn check(&self) -> Result<(), Interrupted> {
        match self.expired() {
            true => Err(Interrupted),
            false => Ok(()),
        }
    }
}

/// The text's bytes.
fn gather(text: &Text, clock: &Clock) -> Result<Vec<u8>, Interrupted> {
    let mut bytes = Vec::with_capacity(text.len());
    for chunk in text.chunks() {
        if bytes.len() / WORK != (bytes.len() + chunk.len()) / WORK {
            clock.check()?;
        }
        bytes.extend_from_slice(chunk.as_bytes());
    }
    Ok(bytes)
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
        if let Some(ix) = bytes[from..to].iter().rposition(|&byte| found(byte)) {
            return Ok(Some(from + ix));
        }
        to = from;
        clock.check()?;
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
        if let Some(ix) = block.iter().position(|&byte| found(byte)) {
            return Ok(Some(at + ix));
        }
        at += block.len();
        clock.check()?;
    }
    Ok(None)
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
    fn same_name(&self, other: &Tag, bytes: &[u8], clock: &Clock) -> Result<bool, Interrupted> {
        let written = |tag: &Tag| &bytes[tag.name_start..tag.name_end];
        if self.key != other.key || self.units != other.units {
            return Ok(false);
        }
        if written(self) == written(other) {
            return Ok(true);
        }
        for (at, (unit, other)) in self.units(bytes).zip(other.units(bytes)).enumerate() {
            if at % WORK == WORK - 1 {
                clock.check()?;
            }
            if unit != other {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Its name's units, read from `bytes`.
    fn units<'b>(&self, bytes: &'b [u8]) -> impl Iterator<Item = Unit> + 'b {
        let name = std::str::from_utf8(&bytes[self.name_start..self.name_end]).unwrap_or_default();
        name.graphemes(true).map(Unit::new)
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
    bytes: Vec<u8>,
    clock: Clock,
}

impl<'a> Tags<'a> {
    fn scan(
        text: &'a Text,
        bytes: Vec<u8>,
        offset: usize,
        clock: Clock,
    ) -> Result<Self, Interrupted> {
        let mut tags: Vec<Tag> = Vec::new();
        // Each name's opening tags without a partner yet, by key.
        let mut open: HashMap<u64, Vec<usize>> = HashMap::new();
        // Those at or before `offset`.
        let mut pending = 0usize;
        // Whether one at or before `offset` is [`Tag::lone`]: its partner is
        // found once every tag is in.
        let mut lone = false;
        let mut names = Names::default();
        let mut at = 0;
        let mut seen = 0usize;
        while let Some(found) = find(&bytes, at, &clock, |byte| byte == b'<')? {
            if found > offset && pending == 0 && !lone {
                break;
            }
            seen += 1;
            if seen.is_multiple_of(256) {
                clock.check()?;
            }
            at = found + 1;
            let Some(tag) = tag(&bytes, found, &mut names, &clock)? else {
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
                    if tags[opening].same_name(&tag, &bytes, &clock)? {
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
        for ix in (0..tags.len()).rev() {
            if ix % WORK == 0 {
                clock.check()?;
            }
            let tag = &tags[ix];
            if tag.closing {
                next.insert(tag.key, ix);
            } else if tag.lone() && !tag.nests() && !tag.composed && tag.start <= offset {
                let close = next.get(&tag.key).map(|&close| &tags[close]);
                match close {
                    Some(close) if !close.same_name(tag, &bytes, &clock)? => continue,
                    Some(close) => tags[ix].partner = Some((close.start, close.end)),
                    None => {}
                }
                tags[ix].paired = true;
            }
        }
        Ok(Self {
            tags,
            text,
            bytes,
            clock,
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
            let at = partner(&self.tags, &self.bytes, ix, &self.clock)?;
            return Ok(at.map(|at| (self.tags[at].start, self.tags[at].end)));
        }
        match std::str::from_utf8(&self.bytes[tag.name_start..tag.name_end]) {
            Ok(name) => pattern_partner(self.text, tag.start, name, &self.clock.0),
            Err(_) => Ok(None),
        }
    }

    /// Where the inside of the opening `tag`'s block, closed at `close`,
    /// starts: past the tag's first `>` outside a quoted attribute value,
    /// or with none, at the tag itself.
    fn inner(&self, tag: &Tag, close: usize) -> Result<usize, Interrupted> {
        let mut quote = None;
        for (step, (at, &byte)) in self.bytes.iter().enumerate().skip(tag.name_end).enumerate() {
            if step % WORK == WORK - 1 {
                self.clock.check()?;
            }
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
        let unmatched = (0..before).rev().filter(|&ix| {
            let tag = &self.tags[ix];
            match (tag.closing, depth) {
                (true, _) => depth += 1,
                (false, 0) => return true,
                (false, _) => depth -= 1,
            }
            false
        });
        for ix in unmatched.skip(count as usize - 1) {
            let tag = &self.tags[ix];
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
    bytes: &[u8],
    ix: usize,
    clock: &Clock,
) -> Result<Option<usize>, Interrupted> {
    let seeker = &tags[ix];
    let nests = seeker.nests();
    let mut depth = 0usize;
    for (at, tag) in tags.iter().enumerate().skip(ix + 1) {
        if at % WORK == 0 {
            clock.check()?;
        }
        let opening_nests = !tag.closing && (tag.marked || !nests);
        if opening_nests
            || tag.units != seeker.units
            || !matches(seeker.units(bytes), tag.units(bytes), clock)?
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
/// the row's end. Matches may overlap, so each `<` is tried, against each
/// span it could take. None if the name isn't a pattern on its own (one
/// fe-vim can't read, or see [`literals`]); `Err` if pairing ran out of
/// time.
fn pattern_partner(
    text: &Text,
    start: usize,
    name: &str,
    deadline: &Deadline,
) -> Result<Option<(usize, usize)>, Interrupted> {
    if deadline.expired() {
        return Err(Interrupted);
    }
    let Some(literal) = literals(name) else {
        return Ok(None);
    };
    let clusters = clusters(name, &literal);
    let name = stand_in(name, &clusters);
    // `\c` wins over a `\C` in the name, and `\m` undoes its `\v` or `\V`.
    let (Ok(open), Ok(close)) = (
        Pattern::parse(&format!("\\c^<{name}\\m$")),
        Pattern::parse(&format!("\\c^</{name}\\m>$")),
    ) else {
        return Ok(None);
    };
    let whole = |pattern: &Pattern, span: &str| -> Result<bool, Interrupted> {
        let span = Text::new(&stand_in(span, &clusters));
        Ok(first_in_row(&span, 0, pattern, deadline)?.is_some())
    };
    let mut depth = 0usize;
    let mut buf = String::new();
    for row in text.offset_to_point(start).row..text.row_count() {
        let (row_start, hay) = row_text(text, row, &mut buf);
        for (column, _) in hay.match_indices('<') {
            if row_start + column <= start {
                continue;
            }
            if deadline.expired() {
                return Err(Interrupted);
            }
            let rest = &hay[column..];
            let gts: Vec<usize> = rest.match_indices('>').map(|(at, _)| at + 1).collect();
            let mut closes = false;
            if rest.starts_with("</") {
                for &gt in &gts {
                    if whole(&close, &rest[..gt])? {
                        closes = true;
                        break;
                    }
                }
            }
            if closes {
                match depth {
                    0 => return Ok(Some((row_start + column, row_start + column + gts[0]))),
                    _ => depth -= 1,
                }
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
                if word && whole(&open, &rest[..end])? {
                    depth += 1;
                    break;
                }
            }
        }
    }
    Ok(None)
}

/// Where `name`, read as nvim reads a pattern, has literal chars (outside
/// a collection, not escaped): a flag per byte. None if it isn't a pattern
/// on its own, as nvim takes a tag's name: with a `~` that is the last
/// substitute string (there's none), a branch outside a group, a
/// collection without its `]` (it would take in what nvim puts after the
/// name), or ending very magic (`\v`).
fn literals(name: &str) -> Option<Vec<bool>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Magic {
        Very,
        On,
        Off,
        VeryOff,
    }
    let mut literal = vec![false; name.len()];
    let mut magic = Magic::On;
    let mut depth = 0usize;
    let mut chars = name.char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
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
            '~' if special(false) => return None,
            '(' if special(true) => depth += 1,
            '%' if special(true) && chars.peek().is_some_and(|&(_, next)| next == '(') => {
                chars.next();
                depth += 1;
            }
            ')' if special(true) => depth = depth.saturating_sub(1),
            '|' if special(true) && depth == 0 => return None,
            '[' if special(false) => {
                // A `]` first, after any `^`, is itself; `\]` too, and a
                // `[:name:]` holds its own.
                if chars.peek().is_some_and(|&(_, next)| next == '^') {
                    chars.next();
                }
                if chars.peek().is_some_and(|&(_, next)| next == ']') {
                    chars.next();
                }
                loop {
                    match chars.next()?.1 {
                        ']' => break,
                        '\\' => {
                            chars.next()?;
                        }
                        '[' if chars.peek().is_some_and(|&(_, next)| next == ':') => {
                            while chars.next()?.1 != ']' {}
                        }
                        _ => {}
                    }
                }
            }
            _ if !escaped => literal[at..at + ch.len_utf8()].fill(true),
            _ => {}
        }
    }
    (magic != Magic::Very).then_some(literal)
}

/// The graphemes of `name` written literally that join chars other than
/// composing ones (a ZWJ, an emoji modifier), which fe-vim's `/` would fold
/// as separate chars, each with a private-use char to stand for it: nvim
/// takes such a grapheme as one char, matching only the same base char with
/// at least its other chars.
fn clusters(name: &str, literal: &[bool]) -> Vec<(String, char)> {
    let mut found: Vec<(String, char)> = Vec::new();
    for (at, grapheme) in name.grapheme_indices(true) {
        if literal[at] && joined(grapheme) && !found.iter().any(|(seen, _)| seen == grapheme) {
            let stand_in = char::from_u32(0xf_0000 + found.len() as u32).unwrap_or('\u{f0000}');
            found.push((grapheme.to_string(), stand_in));
        }
    }
    found
}

/// Whether `grapheme` joins chars that aren't all composing ones (marks).
fn joined(grapheme: &str) -> bool {
    static MARK: OnceLock<Regex> = OnceLock::new();
    let mark = MARK.get_or_init(|| Regex::new(r"^\p{M}$").expect("a valid regex"));
    grapheme
        .chars()
        .skip(1)
        .any(|ch| !mark.is_match(ch.encode_utf8(&mut [0; 4])))
}

/// `text` with each grapheme that one of `clusters` matches (the same base
/// char, with at least its other chars) put as that one's stand-in.
fn stand_in(text: &str, clusters: &[(String, char)]) -> String {
    let unit = |grapheme: &str| match Unit::new(grapheme) {
        Unit::Composed(base, rest) => Some((base, rest)),
        Unit::Char(_) => None,
    };
    text.graphemes(true)
        .map(|grapheme| {
            let found = unit(grapheme).and_then(|(base, rest)| {
                clusters.iter().find(|(cluster, _)| {
                    unit(cluster).is_some_and(|(other, chars)| {
                        other == base && chars.iter().all(|ch| rest.contains(ch))
                    })
                })
            });
            match found {
                Some((_, stand_in)) => stand_in.to_string(),
                None => grapheme.to_string(),
            }
        })
        .collect()
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
        if at % WORK == WORK - 1 {
            clock.check()?;
        }
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
        bytes: &[u8],
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
        let end = find(bytes, at, clock, stops)?.unwrap_or(bytes.len());
        let text = std::str::from_utf8(&bytes[at..end]).unwrap_or_default();
        let mut from = std::mem::take(&mut self.from);
        from.clear();
        let mut state = (0u64, 0usize, false, false);
        from.push((end, state));
        let mut word = None;
        // ASCII graphemes are its bytes (a name holds no line break).
        let graphemes: Box<dyn Iterator<Item = (usize, &str)>> = match text.is_ascii() {
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
            if step % WORK == WORK - 1 {
                clock.check()?;
            }
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
        from.reverse();
        from.dedup_by_key(|(place, _)| *place);
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
    bytes: &[u8],
    at: usize,
    names: &mut Names,
    clock: &Clock,
) -> Result<Option<Tag>, Interrupted> {
    let closing = bytes.get(at + 1) == Some(&b'/');
    let name_start = at + 1 + usize::from(closing);
    let run = match closing {
        true => names.closing.at(bytes, name_start, true, clock)?,
        false => names.opening.at(bytes, name_start, false, clock)?,
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
    let name_start = match (closing, lt_len(&bytes[at..name_end])) {
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
fn lt_len(written: &[u8]) -> Option<usize> {
    let mut window = 64;
    loop {
        let part = &written[..window.min(written.len())];
        let text = match std::str::from_utf8(part) {
            Ok(text) => text,
            Err(error) => std::str::from_utf8(&part[..error.valid_up_to()]).ok()?,
        };
        let lt = text.graphemes(true).next()?.len();
        if lt < text.len() || part.len() == written.len() {
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
