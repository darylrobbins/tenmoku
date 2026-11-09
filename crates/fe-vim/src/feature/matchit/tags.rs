//! matchit's tag pairs for `%` and `g%` in html, xml and markdown (whose
//! ftplugin sources html's): the groups of their `b:match_words` that pair
//! a tag's name with its closing tag, `<` with `>`, and `<!--` with `-->`,
//! scanned without Vim patterns. See [`super`] for how `%` gets here.
//!
//! Probed in nvim 0.12.4 (`nvim --clean --headless --cmd 'packadd matchit'
//! --cmd 'filetype plugin on' --cmd 'syntax off'`: with no syntax
//! `b:match_skip` skips nothing), as matchit runs:
//!
//! - It takes the match on the caret's row that starts last at or before
//!   the caret and ends after it, or else the first that starts after it;
//!   at one start the first of `b:match_words`' patterns that matches.
//!   html's order is `<!--`, `-->`, `<`, `>`, a tag's name, a closing tag,
//!   then the brackets; xml's `<`, `>`, `!--` (after a `<`), `-->`, a
//!   tag's name, a closing tag, a self-closing tag's name, `/>`, then the
//!   brackets. So on `<` the `<:>` pair (or html's `<!--`), on the name or
//!   anywhere in the tag after it the name's pair, and from text before a
//!   tag that tag's `<`. A bracket chosen is `%`'s own (nothing here).
//! - A tag's name runs from just after its `<` to a blank or `>` (html: its
//!   first char anything but `/` or `!`, so `<br/>` is named `br/`; xml: no
//!   `/` in it) and the match on to the tag's `>` or the row's end, so a tag
//!   split over rows opens from its first. In xml a name followed by blanks
//!   and attributes counts only if the char before its `>` isn't `/` (and
//!   isn't a lone blank: `<a >` doesn't). A closing tag is `/`, a name and
//!   `>` straight after (`</a >` isn't one).
//! - From a name `%` goes forward to the `/` of its closing tag, counting
//!   tags of the same name opened in between; from a closing tag back to
//!   the name of its opener; `<` and `>`, and `<!--` and `-->`, alike, each
//!   counting every one between (a `<` in text or an attribute too). `g%`
//!   is the same jumps (each group has two words). It doesn't wrap; a
//!   match with no partner fails, so does an html void element (`<br>`)
//!   and a closing tag with no opener.
//! - xml pairs a name followed by blanks with the next `/>` (`<br a="1"/>`),
//!   and `/>` with the last such name before it; `<br/>`'s name is nothing.
//! - Every match between counts, wherever it starts: a tag in another's
//!   attribute value, a `<` in quotes, each of `<a <a <a >` (`searchpair()`
//!   looks on from one char past each match, whatever `cpoptions` says).
//! - html compares names ignoring case (`b:match_ignorecase`), a char at
//!   a time as `\c` does (`<Σ>` closes at `</ς>`, `<K>` with the Kelvin
//!   sign at `</k>`), but a char with composing chars after it (an accent,
//!   a zero-width joiner) only as it is (`<á>` doesn't close at `</Á>`;
//!   here, a grapheme of more than one char); xml doesn't.
//!
//! Where it differs, on purpose: names compare as plain text, whole. nvim
//! puts the name into a pattern, so in html `<div>` also counts `<divx>`
//! and `<div/>` as openers, and `<a.b>` closes at `</axb>`; here neither.
//! A self-closing tag opens nothing: in html a tag whose attributes end
//! with `/>` (`<div class="x"/>`, which nvim counts as an opener of `div`)
//! is passed over, and `%` from its name fails. Its end is the first `>`
//! outside `"` or `'` quotes, from the end of its name, on a later row if
//! need be (each row read on from once a `%`); attributes that run to the
//! end of the text open. The match itself, which an operator takes, still
//! ends at the first `>`, as nvim's does. html's `ul`, `ol` and `dl`
//! groups (`li`, `dt`, `dd` between) are plain tags here; xml's
//! `<![CDATA[`/`]]>` and `<?name`/`?>` are names that fail. With syntax on
//! nvim skips matches in comments and strings; here none are skipped, as
//! with syntax off.
//!
//! Each `%` reads at most [`TAG_BUDGET`] steps (bytes of the rows it reads,
//! the matches it tries, the bytes of names it compares) and checks the
//! search time limit at its first step, as it goes and when it's found
//! where to go; run out, the motion fails and changes nothing.

use std::{cell::Cell, collections::HashMap, ops::Range};

use fe_text::Text;
use unicode_segmentation::UnicodeSegmentation;

use super::Keyword;
use crate::{delimiters::fold, search::Deadline};

/// Whose `b:match_words` a buffer's tags follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dialect {
    /// html's, which markdown's sources.
    Html,
    Xml,
}

/// The steps one `%` or `g%` may take through tags.
pub const TAG_BUDGET: usize = 1 << 24;

/// Steps between looks at the time limit.
const LOOK_EVERY: usize = 1 << 12;

thread_local! {
    // A test's budget in place of [`TAG_BUDGET`].
    static BUDGET: Cell<Option<usize>> = const { Cell::new(None) };
    // The steps taken on this thread since [`take_tag_work`].
    static WORK: Cell<usize> = const { Cell::new(0) };
}

/// Sets the budget of `%` through tags on this thread, for a test; `None`
/// is [`TAG_BUDGET`].
#[doc(hidden)]
pub fn set_tag_budget(budget: Option<usize>) {
    BUDGET.set(budget);
}

/// Takes the steps `%` through tags has taken on this thread since the
/// last call, for a test.
#[doc(hidden)]
pub fn take_tag_work() -> usize {
    WORK.replace(0)
}

/// The budget ran out.
struct RanOut;

/// What one `%` has spent of its steps and time.
struct Budget {
    used: usize,
    limit: usize,
    look: usize,
    deadline: Deadline,
}

impl Budget {
    fn new() -> Self {
        Self {
            used: 0,
            limit: BUDGET.get().unwrap_or(TAG_BUDGET),
            // The first step looks, so a short `%` past the limit fails too.
            look: 0,
            deadline: Deadline::new(),
        }
    }

    /// Takes `steps` more steps: `Err` past the limit or the time limit.
    fn spend(&mut self, steps: usize) -> Result<(), RanOut> {
        self.used = self.used.saturating_add(steps);
        if self.used > self.limit {
            return Err(RanOut);
        }
        if self.used >= self.look {
            self.look = self.used + LOOK_EVERY;
            if self.deadline.expired() {
                return Err(RanOut);
            }
        }
        Ok(())
    }
}

impl Drop for Budget {
    fn drop(&mut self) {
        WORK.set(WORK.get().saturating_add(self.used));
    }
}

/// Where `%` (or `g%`) goes from `offset` through the tags of `dialect`.
pub(crate) fn find(text: &Text, offset: usize, dialect: Dialect) -> Keyword {
    let mut budget = Budget::new();
    match find_within(text, offset, dialect, &mut budget) {
        // Past the time limit by the end, it fails as if it ran out.
        Ok(_) if budget.deadline.expired() => Keyword::Fails,
        Ok(keyword) => keyword,
        Err(RanOut) => Keyword::Fails,
    }
}

fn find_within(
    text: &Text,
    offset: usize,
    dialect: Dialect,
    budget: &mut Budget,
) -> Result<Keyword, RanOut> {
    let row = text.offset_to_point(offset).row;
    let col = offset - text.row_start(row);
    let line = read(text, row, budget)?;
    let here = Row::new(&line, dialect, budget)?;
    let Some(token) = here.chosen(col, budget)? else {
        return Ok(Keyword::None);
    };
    let Some((pair, opens)) = token.pair(&line) else {
        return Ok(Keyword::None);
    };
    let mut ends = Ends::new(text);
    // A self-closing tag's name opens nothing (nvim looks for its closing
    // tag).
    if let Some(ending) = here.tag_ending(&token)
        && ends.closes_itself(row, ending, budget)?
    {
        return Ok(Keyword::Fails);
    }
    // As matchit sets up `searchpair()` from the match's start: forward
    // from an opener, back from a closer, for `%` and `g%` alike.
    let start = token.span.start;
    let mut nest = 1usize;
    // Whether `found` on `row` is the partner, html's self-closing tags
    // passed over.
    let mut lands = |row: u32, found: &Found, budget: &mut Budget| {
        if let Some(ending) = found.ending
            && ends.closes_itself(row, ending, budget)?
        {
            return Ok(false);
        }
        match found.opens == opens {
            true => nest += 1,
            false => nest -= 1,
        }
        Ok(nest == 0)
    };
    let landed = |row: u32, found: &Found| {
        let at = text.row_start(row);
        Keyword::Lands(at + found.span.start..at + found.span.end)
    };
    let mine = here.matches(&pair, budget)?;
    if opens {
        for found in mine.iter().filter(|found| found.span.start > start) {
            if lands(row, found, budget)? {
                return Ok(landed(row, found));
            }
        }
        for other in row + 1..text.row_count() {
            let line = read(text, other, budget)?;
            for found in &Row::new(&line, dialect, budget)?.matches(&pair, budget)? {
                if lands(other, found, budget)? {
                    return Ok(landed(other, found));
                }
            }
        }
    } else {
        for found in mine.iter().rev().filter(|found| found.span.start < start) {
            if lands(row, found, budget)? {
                return Ok(landed(row, found));
            }
        }
        for other in (0..row).rev() {
            let line = read(text, other, budget)?;
            for found in Row::new(&line, dialect, budget)?
                .matches(&pair, budget)?
                .iter()
                .rev()
            {
                if lands(other, found, budget)? {
                    return Ok(landed(other, found));
                }
            }
        }
    }
    Ok(Keyword::Fails)
}

/// Whether html tags whose attributes run past their row close themselves,
/// read on from the rows after, each row and quote it starts in once.
struct Ends<'t> {
    text: &'t Text,
    /// Whether the attributes read from a row's start, in a quote or not,
    /// end with `/>`.
    after: HashMap<(u32, Quote), bool>,
}

impl<'t> Ends<'t> {
    fn new(text: &'t Text) -> Self {
        Self {
            text,
            after: HashMap::new(),
        }
    }

    /// Whether a tag on `row` whose attributes end as `ending` there closes
    /// itself; one whose attributes run to the end of the text doesn't.
    fn closes_itself(
        &mut self,
        row: u32,
        ending: Ending,
        budget: &mut Budget,
    ) -> Result<bool, RanOut> {
        let mut quote = match ending {
            Ending::Gt { lone } => return Ok(lone),
            Ending::Open(quote) => quote,
        };
        let mut read_through = Vec::new();
        let mut lone = false;
        for next in row + 1..self.text.row_count() {
            if let Some(&known) = self.after.get(&(next, quote)) {
                lone = known;
                break;
            }
            read_through.push((next, quote));
            let line = read(self.text, next, budget)?;
            match Row::new(&line, Dialect::Html, budget)?.ending_in(quote) {
                Ending::Gt { lone: at } => {
                    lone = at;
                    break;
                }
                Ending::Open(on) => quote = on,
            }
        }
        self.after
            .extend(read_through.into_iter().map(|key| (key, lone)));
        Ok(lone)
    }
}

/// `row`'s text, its bytes spent first.
fn read(text: &Text, row: u32, budget: &mut Budget) -> Result<String, RanOut> {
    budget.spend(text.row_len(row) as usize + 1)?;
    Ok(text.row_to_string(row))
}

/// What a match is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// `<!--` (in xml from its `!`).
    CommentOpen,
    CommentClose,
    Lt,
    Gt,
    /// A tag's name, on to its `>` or the row's end.
    Open,
    /// `/`, a name and `>`.
    Close,
    /// xml's name followed by blanks, up to a `/` or `>`.
    Bare,
    /// xml's `/>`.
    SelfClose,
    /// One of `%`'s own brackets.
    Bracket,
}

/// A match on a row: its bytes, what it is, and a tag's name.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Token {
    span: Range<usize>,
    kind: Kind,
    name: Range<usize>,
}

impl Token {
    fn new(span: Range<usize>, kind: Kind) -> Self {
        Self {
            span,
            kind,
            name: 0..0,
        }
    }

    /// The pair the match is part of, on `line`, and whether it opens it;
    /// `None` for a bracket.
    fn pair<'l>(&self, line: &'l str) -> Option<(Pair<'l>, bool)> {
        Some(match self.kind {
            Kind::CommentOpen => (Pair::Comment, true),
            Kind::CommentClose => (Pair::Comment, false),
            Kind::Lt => (Pair::Angle, true),
            Kind::Gt => (Pair::Angle, false),
            Kind::Open => (Pair::tag(&line[self.name.clone()]), true),
            Kind::Close => (Pair::tag(&line[self.name.clone()]), false),
            Kind::Bare => (Pair::Bare, true),
            Kind::SelfClose => (Pair::Bare, false),
            Kind::Bracket => return None,
        })
    }
}

/// A group `searchpair()` goes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pair<'n> {
    Comment,
    Angle,
    /// The tags of this name (of `chars` chars), and their closing tags.
    Tag {
        name: &'n str,
        chars: usize,
    },
    /// xml's names followed by blanks, and `/>`.
    Bare,
}

impl<'n> Pair<'n> {
    fn tag(name: &'n str) -> Self {
        Self::Tag {
            name,
            chars: name.chars().count(),
        }
    }
}

/// A match of a [`Pair`]'s opener or closer on a row.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Found {
    span: Range<usize>,
    opens: bool,
    /// An html tag's: where its attributes end.
    ending: Option<Ending>,
}

/// A quote that attributes are inside, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Quote {
    None,
    Double,
    Single,
}

/// Where an html tag's attributes, read on from a char outside quotes,
/// end: the first `>` outside quotes, whether a `/` is just before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ending {
    Gt {
        lone: bool,
    },
    /// Not on the row: at its end, inside this quote or none.
    Open(Quote),
}

/// A match tried at a start, in `b:match_words` order.
type Alternative = fn(&Row, usize) -> Option<Token>;

const HTML_ORDER: &[Alternative] = &[
    |row, at| row.comment_open_token(at),
    |row, at| row.comment_close_token(at),
    |row, at| row.lt_token(at),
    |row, at| row.gt_token(at),
    |row, at| row.open_token(at),
    |row, at| row.close_token(at),
    |row, at| row.bracket_token(at),
];

const XML_ORDER: &[Alternative] = &[
    |row, at| row.lt_token(at),
    |row, at| row.gt_token(at),
    |row, at| row.comment_open_token(at),
    |row, at| row.comment_close_token(at),
    |row, at| row.open_token(at),
    |row, at| row.close_token(at),
    |row, at| row.bare_token(at),
    |row, at| row.self_close_token(at),
    |row, at| row.bracket_token(at),
];

/// A row's text, with where its `>`s, the bytes that end a name, its `/`s
/// and `>`s, the bytes inside a char (past its first), and (in html) its
/// `>`s and quotes are, each in order; and where attributes read from each
/// of the last end.
struct Row<'a> {
    line: &'a str,
    dialect: Dialect,
    gts: Vec<u32>,
    stops: Vec<u32>,
    slashes: Vec<u32>,
    inside: Vec<u32>,
    marks: Vec<u32>,
    endings: Vec<Ending>,
}

impl<'a> Row<'a> {
    fn new(line: &'a str, dialect: Dialect, budget: &mut Budget) -> Result<Self, RanOut> {
        budget.spend(line.len())?;
        let mut row = Self {
            line,
            dialect,
            gts: Vec::new(),
            stops: Vec::new(),
            slashes: Vec::new(),
            inside: Vec::new(),
            marks: Vec::new(),
            endings: Vec::new(),
        };
        for (ix, &byte) in line.as_bytes().iter().enumerate() {
            let ix = ix as u32;
            if byte == b'>' {
                row.gts.push(ix);
            }
            if matches!(byte, b' ' | b'\t' | b'>') || byte == b'/' && dialect == Dialect::Xml {
                row.stops.push(ix);
            }
            if matches!(byte, b'/' | b'>') {
                row.slashes.push(ix);
            }
            if byte & 0xc0 == 0x80 {
                row.inside.push(ix);
            }
            if dialect == Dialect::Html && matches!(byte, b'>' | b'"' | b'\'') {
                row.marks.push(ix);
            }
        }
        row.endings = row.mark_endings();
        Ok(row)
    }

    /// Where attributes read from each of `marks` end, from the last back:
    /// at a `>` there; from a quote, as from past the next of its kind.
    fn mark_endings(&self) -> Vec<Ending> {
        let mut endings = vec![Ending::Open(Quote::None); self.marks.len()];
        let mut next = [None; 2];
        for (ix, &at) in self.marks.iter().enumerate().rev() {
            let at = at as usize;
            let (quote, kind) = match self.bytes()[at] {
                b'>' => {
                    let lone = at > 0 && self.bytes()[at - 1] == b'/';
                    endings[ix] = Ending::Gt { lone };
                    continue;
                }
                b'"' => (Quote::Double, 0),
                _ => (Quote::Single, 1),
            };
            endings[ix] = match next[kind] {
                Some(close) => endings
                    .get(close + 1)
                    .copied()
                    .unwrap_or(Ending::Open(Quote::None)),
                None => Ending::Open(quote),
            };
            next[kind] = Some(ix);
        }
        endings
    }

    /// Where html attributes read from `from`, outside quotes, end.
    fn ending_from(&self, from: usize) -> Ending {
        let ix = self.marks.partition_point(|&pos| (pos as usize) < from);
        self.endings
            .get(ix)
            .copied()
            .unwrap_or(Ending::Open(Quote::None))
    }

    /// Where html attributes read from the row's start, inside `quote` or
    /// not, end.
    fn ending_in(&self, quote: Quote) -> Ending {
        let byte = match quote {
            Quote::None => return self.ending_from(0),
            Quote::Double => b'"',
            Quote::Single => b'\'',
        };
        let mut marks = self.marks.iter().map(|&at| at as usize);
        match marks.find(|&at| self.bytes()[at] == byte) {
            Some(close) => self.ending_from(close + 1),
            None => Ending::Open(quote),
        }
    }

    /// Where the attributes of html's tag whose name `token` is end.
    fn tag_ending(&self, token: &Token) -> Option<Ending> {
        (self.dialect == Dialect::Html && token.kind == Kind::Open)
            .then(|| self.ending_from(token.name.end))
    }

    fn bytes(&self) -> &'a [u8] {
        self.line.as_bytes()
    }

    fn len(&self) -> usize {
        self.line.len()
    }

    /// The first of `at` (an ordered list of positions) at or after `from`.
    fn next(at: &[u32], from: usize) -> Option<usize> {
        let ix = at.partition_point(|&pos| (pos as usize) < from);
        at.get(ix).map(|&pos| pos as usize)
    }

    /// Where a name from `from` ends: the next blank or `>` (in xml, or
    /// `/`), or the row's end.
    fn name_end(&self, from: usize) -> usize {
        Self::next(&self.stops, from).unwrap_or(self.len())
    }

    /// Whether a `<` is just before `at`.
    fn after_lt(&self, at: usize) -> bool {
        at > 0 && self.bytes()[at - 1] == b'<'
    }

    fn has(&self, at: usize, literal: &str) -> bool {
        self.bytes()[at..].starts_with(literal.as_bytes())
    }

    /// How many chars `span` (on char boundaries) holds.
    fn chars(&self, span: &Range<usize>) -> usize {
        let before = |at| self.inside.partition_point(|&pos| (pos as usize) < at);
        span.len() - (before(span.end) - before(span.start))
    }

    /// The length of the char at `at`, a char boundary.
    fn char_len(&self, at: usize) -> usize {
        self.line[at..].chars().next().map_or(1, char::len_utf8)
    }

    /// `<!--` (html) or `!--` after a `<` (xml) at `at`: where it ends.
    fn comment_open(&self, at: usize) -> Option<usize> {
        match self.dialect {
            Dialect::Html => self.has(at, "<!--").then_some(at + 4),
            Dialect::Xml => (self.after_lt(at) && self.has(at, "!--")).then_some(at + 3),
        }
    }

    fn comment_close(&self, at: usize) -> Option<usize> {
        self.has(at, "-->").then_some(at + 3)
    }

    /// A tag's name at `at`: its bytes, and where the match ends (the
    /// tag's `>` or the row's end).
    fn open(&self, at: usize) -> Option<(Range<usize>, usize)> {
        if !self.after_lt(at) || at >= self.len() {
            return None;
        }
        match self.dialect {
            // `<\@<=\([^/!][^ \t>]*\)[^>]*\%(>\|$\)`
            Dialect::Html => {
                if matches!(self.bytes()[at], b'/' | b'!') {
                    return None;
                }
                let rest = at + self.char_len(at);
                let end = Self::next(&self.gts, rest).map_or(self.len(), |gt| gt + 1);
                Some((at..self.name_end(rest), end))
            }
            // `<\@<=\([^ \t>/]\+\)\%(\s\+[^>]*\%([^/]>\|$\)\|>\|$\)`
            Dialect::Xml => {
                let name = self.name_end(at);
                if name == at {
                    return None;
                }
                let end = match self.bytes().get(name) {
                    None => name,
                    Some(b'>') => name + 1,
                    Some(b' ' | b'\t') => match Self::next(&self.gts, name + 1) {
                        None => self.len(),
                        // `[^/]` takes the first `>` and the next ends it.
                        Some(gt) if self.bytes().get(gt + 1) == Some(&b'>') => gt + 2,
                        // `\s\+` takes a blank, so `[^/]` needs another.
                        Some(gt) if gt >= name + 2 && self.bytes()[gt - 1] != b'/' => gt + 1,
                        Some(_) => return None,
                    },
                    Some(_) => return None,
                };
                Some((at..name, end))
            }
        }
    }

    /// A closing tag from its `/` at `at`: its name's bytes and its end.
    fn close(&self, at: usize) -> Option<(Range<usize>, usize)> {
        if !self.after_lt(at) || self.bytes().get(at) != Some(&b'/') {
            return None;
        }
        let first = at + 1;
        let end = match self.dialect {
            // `<\@<=/\([^/!][^ \t>]*\)>`
            Dialect::Html => {
                let byte = *self.bytes().get(first)?;
                if matches!(byte, b'/' | b'!') {
                    return None;
                }
                self.name_end(first + self.char_len(first))
            }
            // `<\@<=/\([^ \t>/]\+\)>`
            Dialect::Xml => Some(self.name_end(first)).filter(|&end| end > first)?,
        };
        (self.bytes().get(end) == Some(&b'>')).then_some((first..end, end + 1))
    }

    /// xml's self-closing tag's name at `at`, followed by blanks and
    /// anything up to a `/` or `>`, or by the row's end: where it ends.
    /// `<\@<=\%([^ \t>/]\+\)\%(\s\+[^/>]*\|$\)`
    fn bare(&self, at: usize) -> Option<usize> {
        if self.dialect != Dialect::Xml || !self.after_lt(at) {
            return None;
        }
        let name = self.name_end(at);
        if name == at {
            return None;
        }
        match self.bytes().get(name) {
            None => Some(name),
            Some(b' ' | b'\t') => Some(Self::next(&self.slashes, name).unwrap_or(self.len())),
            Some(_) => None,
        }
    }

    fn self_close(&self, at: usize) -> Option<usize> {
        (self.dialect == Dialect::Xml && self.has(at, "/>")).then_some(at + 2)
    }

    fn comment_open_token(&self, at: usize) -> Option<Token> {
        let end = self.comment_open(at)?;
        Some(Token::new(at..end, Kind::CommentOpen))
    }

    fn comment_close_token(&self, at: usize) -> Option<Token> {
        let end = self.comment_close(at)?;
        Some(Token::new(at..end, Kind::CommentClose))
    }

    fn lt_token(&self, at: usize) -> Option<Token> {
        (self.bytes().get(at) == Some(&b'<')).then(|| Token::new(at..at + 1, Kind::Lt))
    }

    fn gt_token(&self, at: usize) -> Option<Token> {
        (self.bytes().get(at) == Some(&b'>')).then(|| Token::new(at..at + 1, Kind::Gt))
    }

    fn open_token(&self, at: usize) -> Option<Token> {
        let (name, end) = self.open(at)?;
        Some(Token {
            span: at..end,
            kind: Kind::Open,
            name,
        })
    }

    fn close_token(&self, at: usize) -> Option<Token> {
        let (name, end) = self.close(at)?;
        Some(Token {
            span: at..end,
            kind: Kind::Close,
            name,
        })
    }

    fn bare_token(&self, at: usize) -> Option<Token> {
        let end = self.bare(at)?;
        Some(Token::new(at..end, Kind::Bare))
    }

    fn self_close_token(&self, at: usize) -> Option<Token> {
        let end = self.self_close(at)?;
        Some(Token::new(at..end, Kind::SelfClose))
    }

    fn bracket_token(&self, at: usize) -> Option<Token> {
        let byte = *self.bytes().get(at)?;
        (b"()[]{}".contains(&byte)).then(|| Token::new(at..at + 1, Kind::Bracket))
    }

    /// The first match at `at`, in `b:match_words` order, that ends past
    /// `past`.
    fn token_at(&self, at: usize, past: usize) -> Option<Token> {
        let order = match self.dialect {
            Dialect::Html => HTML_ORDER,
            Dialect::Xml => XML_ORDER,
        };
        order
            .iter()
            .filter_map(|alternative| alternative(self, at))
            .find(|token| token.span.end > past)
    }

    /// The match `%` takes with the caret at `col`, as matchit's
    /// `s:Wholematch` finds it: the one starting last at or before `col`
    /// that ends after it, else the first starting after it.
    fn chosen(&self, col: usize, budget: &mut Budget) -> Result<Option<Token>, RanOut> {
        for at in (0..=col.min(self.len())).rev() {
            budget.spend(1)?;
            if let Some(token) = self.token_at(at, col) {
                return Ok(Some(token));
            }
        }
        for at in col + 1..self.len() {
            budget.spend(1)?;
            if let Some(token) = self.token_at(at, at) {
                return Ok(Some(token));
            }
        }
        Ok(None)
    }

    /// `pair`'s opener or closer at `at`, if one starts there: where it
    /// ends and whether it opens.
    fn pair_at(
        &self,
        pair: &Pair,
        at: usize,
        budget: &mut Budget,
    ) -> Result<Option<Found>, RanOut> {
        let found = |end: Option<usize>, opens| {
            end.map(|end| Found {
                span: at..end,
                opens,
                ending: None,
            })
        };
        let lt = |at: usize| (self.bytes()[at] == b'<').then_some(at + 1);
        let gt = |at: usize| (self.bytes()[at] == b'>').then_some(at + 1);
        Ok(match *pair {
            Pair::Comment => {
                found(self.comment_open(at), true).or_else(|| found(self.comment_close(at), false))
            }
            Pair::Angle => found(lt(at), true).or_else(|| found(gt(at), false)),
            Pair::Bare => found(self.bare(at), true).or_else(|| found(self.self_close(at), false)),
            Pair::Tag { name: want, chars } => {
                let named = |(name, end): (Range<usize>, usize), budget: &mut Budget| {
                    // A name of another length is another name, folded or
                    // not: names that start apart and are as long end
                    // apart, so the bytes compared add up to the row's.
                    if self.chars(&name) != chars {
                        return Ok(None);
                    }
                    let name = &self.line[name];
                    budget.spend(name.len())?;
                    let same = match self.dialect {
                        Dialect::Html => same_folded(name, want),
                        Dialect::Xml => name == want,
                    };
                    Ok(same.then_some(end))
                };
                match (self.open(at), self.close(at)) {
                    (Some(open), _) => {
                        let ending =
                            (self.dialect == Dialect::Html).then(|| self.ending_from(open.0.end));
                        found(named(open, budget)?, true).map(|found| Found { ending, ..found })
                    }
                    (None, Some(close)) => found(named(close, budget)?, false),
                    (None, None) => None,
                }
            }
        })
    }

    /// `pair`'s openers and closers on the row, by where they start,
    /// overlapping ones too.
    fn matches(&self, pair: &Pair, budget: &mut Budget) -> Result<Vec<Found>, RanOut> {
        let mut found = Vec::new();
        for at in 0..self.len() {
            budget.spend(1)?;
            found.extend(self.pair_at(pair, at, budget)?);
        }
        Ok(found)
    }
}

/// Whether `a` and `b` are the same name ignoring case as nvim's `\c`
/// folds it: a char at a time (`Σ` and `ς`, `k` and the Kelvin sign), but
/// one with composing chars after it (a combining accent, a zero-width
/// joiner) only as it is, with them.
fn same_folded(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(b);
    }
    let single = |grapheme: &str| {
        let mut chars = grapheme.chars();
        chars.next().filter(|_| chars.next().is_none())
    };
    let (mut a, mut b) = (a.graphemes(true), b.graphemes(true));
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(a), Some(b)) => {
                let same = match (single(a), single(b)) {
                    (Some(a), Some(b)) => fold(a) == fold(b),
                    _ => a == b,
                };
                if !same {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(line: &str, dialect: Dialect) -> Row<'_> {
        Row::new(line, dialect, &mut Budget::new()).ok().unwrap()
    }

    fn chosen(line: &str, dialect: Dialect, col: usize) -> Option<(Range<usize>, Kind)> {
        let row = row(line, dialect);
        let token = row.chosen(col, &mut Budget::new()).ok().unwrap()?;
        Some((token.span, token.kind))
    }

    #[test]
    fn names_end_as_each_dialect_ends_them() {
        let html = row("<br/><a b=\"1\"/>< x>", Dialect::Html);
        assert_eq!(html.open(1), Some((1..4, 5)));
        assert_eq!(html.open(6), Some((6..7, 15)));
        assert_eq!(html.open(16), Some((16..18, 19)));
        // The first two end with `/>`.
        assert_eq!(html.ending_from(4), Ending::Gt { lone: true });
        assert_eq!(html.ending_from(7), Ending::Gt { lone: true });
        assert_eq!(html.ending_from(18), Ending::Gt { lone: false });
        let xml = row("<br/><a b=\"1\"/><a >< x><a  >", Dialect::Xml);
        assert_eq!(xml.open(1), None);
        assert_eq!(xml.bare(1), None);
        assert_eq!(xml.open(6), None);
        assert_eq!(xml.bare(6), Some(13));
        assert_eq!(xml.open(16), None);
        assert_eq!(xml.bare(16), Some(18));
        assert_eq!(xml.open(20), None);
        assert_eq!(xml.open(24), Some((24..25, 28)));
        assert_eq!(row("<a b/>>", Dialect::Xml).open(1), Some((1..2, 7)));
        assert_eq!(row("<a b", Dialect::Xml).open(1), Some((1..2, 4)));
        assert_eq!(row("</a></a ></>", Dialect::Html).close(1), Some((2..3, 4)));
        assert_eq!(row("</a></a ></>", Dialect::Html).close(5), None);
        assert_eq!(row("</a></a ></>", Dialect::Xml).close(10), None);
    }

    #[test]
    fn attributes_end_at_the_first_gt_outside_quotes() {
        let html = row("<a title=\"<b/>\">x<c d='\"/>'/>", Dialect::Html);
        assert_eq!(html.ending_from(2), Ending::Gt { lone: false });
        assert_eq!(html.ending_from(13), Ending::Gt { lone: true });
        assert_eq!(html.ending_from(20), Ending::Gt { lone: true });
        assert_eq!(html.ending_from(29), Ending::Open(Quote::None));
        let html = row("<a title=\"x>", Dialect::Html);
        assert_eq!(html.ending_from(2), Ending::Open(Quote::Double));
        let html = row("y'\">z\"/>", Dialect::Html);
        assert_eq!(html.ending_in(Quote::Double), Ending::Gt { lone: false });
        assert_eq!(html.ending_in(Quote::Single), Ending::Gt { lone: true });
        assert_eq!(html.ending_in(Quote::None), Ending::Open(Quote::Single));
        assert_eq!(
            row("/>", Dialect::Html).ending_in(Quote::None),
            Ending::Gt { lone: true }
        );
    }

    #[test]
    fn the_match_last_at_or_first_after_the_caret() {
        let html = Dialect::Html;
        assert_eq!(
            chosen("<!-- a -->", html, 0),
            Some((0..4, Kind::CommentOpen))
        );
        assert_eq!(
            chosen("<!-- a -->", html, 1),
            Some((0..4, Kind::CommentOpen))
        );
        assert_eq!(chosen("<!-- a -->", html, 9), Some((9..10, Kind::Gt)));
        assert_eq!(
            chosen("<!-- a -->", Dialect::Xml, 0),
            Some((0..1, Kind::Lt))
        );
        assert_eq!(
            chosen("<p on=\"f(x)\">", html, 4),
            Some((1..13, Kind::Open))
        );
        assert_eq!(
            chosen("<p on=\"f(x)\">", html, 8),
            Some((8..9, Kind::Bracket))
        );
        assert_eq!(chosen("  <p>hi</p>", html, 5), Some((7..8, Kind::Lt)));
        assert_eq!(chosen("text", html, 1), None);
    }

    #[test]
    fn every_start_matches() {
        let line = "<a title=\"<a>\">x</a></a>";
        let html = row(line, Dialect::Html);
        let found = html
            .matches(&Pair::tag("a"), &mut Budget::new())
            .ok()
            .unwrap();
        let spans: Vec<_> = (found.iter())
            .map(|found| (found.span.clone(), found.opens))
            .collect();
        assert_eq!(
            spans,
            [
                (1..13, true),
                (11..13, true),
                (17..20, false),
                (21..24, false)
            ]
        );
    }

    /// Where fe-vim differs from nvim on purpose: names as plain text.
    #[test]
    fn names_are_plain_text() {
        // nvim's pattern `<\@<=div[^>]*` counts `<divx>` and `<div/>` as
        // openers of `div`, and `<a.b>` closes at `</axb>`.
        let html = row("<div><divx><div/></div><a.b></axb>", Dialect::Html);
        let found = html
            .matches(&Pair::tag("div"), &mut Budget::new())
            .ok()
            .unwrap();
        let starts: Vec<_> = found.iter().map(|found| found.span.start).collect();
        assert_eq!(starts, [1, 18]);
        let found = html
            .matches(&Pair::tag("a.b"), &mut Budget::new())
            .ok()
            .unwrap();
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn names_fold_in_html_only() {
        assert!(same_folded("DIV", "div"));
        assert!(same_folded("É", "é"));
        assert!(same_folded("Σa", "ςA"));
        assert!(same_folded("\u{212a}", "k"));
        assert!(!same_folded("ı", "I"));
        // A char with composing chars is itself only; one without folds.
        assert!(!same_folded("a\u{301}", "A\u{301}"));
        assert!(same_folded("Ka\u{301}", "ka\u{301}"));
        assert!(!same_folded("a\u{200d}b", "A\u{200d}b"));
        assert!(same_folded("a\u{200d}b", "a\u{200d}B"));
        assert!(!same_folded("a", "a\u{301}"));
        assert!(!same_folded("div", "dvi"));
        let html = row("<\u{212a}>x</k>", Dialect::Html);
        let found = html
            .matches(&Pair::tag("k"), &mut Budget::new())
            .ok()
            .unwrap();
        assert_eq!(found.len(), 2);
        let xml = row("<A></a>", Dialect::Xml);
        let found = xml
            .matches(&Pair::tag("A"), &mut Budget::new())
            .ok()
            .unwrap();
        assert_eq!(found.len(), 1);
    }
}
