//! `in( an( il( al(`, and the same for `[ { <` and the quotes `" ' ``:
//! the object of the next pair after the caret (`n`) or the last one before
//! it (`l`), never the one holding it, as targets.vim's and mini.ai's `n`
//! and `l` forms give it. Charwise, as the plain object is.
//!
//! The next pair is the first that opens after the caret, the last the
//! nearest that closes before it; a count takes the `count`th that way.
//! Brackets pair as the plain object matches them: escaped ones don't count,
//! and those in a double-quoted string pair only within it. Quotes pair up
//! from each row's start, as `i"` does on a quote: any quote opens a string,
//! and only an unescaped one closes it. The object is the plain
//! one's for that pair (`i(`'s inside, `a"`'s blanks), and over a selection
//! of more than one position, the object grows as the plain one does.
//!
//! The search reads only the rows within [`ROWS`] of the caret's, both ends
//! of a pair among them, and gives up at the command's deadline, looking at
//! it every [`CHUNK`] bytes or pairs it reads: a pair found after it passed
//! is none.

use std::ops::{ControlFlow, Range, RangeInclusive};

use fe_text::{Bias, Text};

use crate::{
    Pending, SeekObject, Step, TextObject, VimKey,
    feature::{Feature, FeaturePrefix},
    operator::Span,
    search::Deadline,
    text_object::{CHUNK, each_byte, pair_span},
};

pub(crate) const FEATURE: Feature = Feature {
    object_prefix: Some(object_prefix),
    prefix: Some(prefix),
    ..Feature::NONE
};

/// Its prefix's name, for `vim.pending_kind`.
const NAME: &str = "seek";

/// How many rows each way of the caret's the search reads: mini.ai's
/// `n_lines` default.
pub(crate) const ROWS: u32 = 50;

/// `n` or `l` after `i` (`inner`) or `a`: waits for the pair's char.
fn object_prefix(pending: &Pending, ch: char, inner: bool) -> Option<Step> {
    if !matches!(ch, 'n' | 'l') {
        return None;
    }
    let prefix = FeaturePrefix::new(NAME)
        .then(if inner { 'i' } else { 'a' })?
        .then(ch)?;
    Some(pending.clone().with_feature_prefix(prefix))
}

/// The pair's char after `in`, `an`, `il` or `al`: the object.
fn prefix(pending: &Pending, prefix: &FeaturePrefix, key: VimKey) -> Option<Step> {
    if prefix.feature() != NAME {
        return None;
    }
    let &[kind, way] = prefix.keys() else {
        return None;
    };
    let VimKey::Char(ch) = key else {
        return None;
    };
    let object = SeekObject {
        last: way == 'l',
        inner: kind == 'i',
        pair: opening(ch)?,
    };
    Some(pending.clone().object(TextObject::Seek(object)))
}

/// The opening char of the pair `ch` names after `in` and the like, as
/// after `i`: `( ) b`, `[ ]`, `{ } B`, `< >`, and the quotes.
fn opening(ch: char) -> Option<char> {
    Some(match ch {
        '(' | ')' | 'b' => '(',
        '[' | ']' => '[',
        '{' | '}' | 'B' => '{',
        '<' | '>' => '<',
        '"' | '\'' | '`' => ch,
        _ => return None,
    })
}

/// The plain object for `object`'s pair: `i(` for `in(`.
pub(crate) fn plain(object: SeekObject) -> TextObject {
    use TextObject::*;
    let (i, a) = match object.pair {
        '(' => (InnerParen, AParen),
        '[' => (InnerBracket, ABracket),
        '{' => (InnerBrace, ABrace),
        '<' => (InnerAngle, AAngle),
        '"' => (InnerDoubleQuote, ADoubleQuote),
        '\'' => (InnerSingleQuote, ASingleQuote),
        _ => (InnerBacktick, ABacktick),
    };
    if object.inner { i } else { a }
}

/// What `object` covers from the caret at `offset`: the plain object of the
/// `count`th pair after it or before it.
pub(crate) fn select(text: &Text, offset: usize, object: SeekObject, count: u32) -> Span {
    within(text, offset, object, count, &Deadline::new())
}

/// [`select`], giving up at `deadline`.
fn within(text: &Text, offset: usize, object: SeekObject, count: u32, deadline: &Deadline) -> Span {
    let row = text.offset_to_point(offset).row;
    let last_row = text.row_count().saturating_sub(1);
    let rows = row.saturating_sub(ROWS)..=row.saturating_add(ROWS).min(last_row);
    let nth = count.max(1) as usize - 1;
    let span = match object.pair {
        '"' | '\'' | '`' => {
            let quote = object.pair as u8;
            quotes(text, offset, rows, quote, object.last, nth, deadline)
                .and_then(|(open, close)| quote_span(text, open, close, object.inner, deadline))
        }
        '(' | '[' | '{' | '<' => {
            let pair = match object.pair {
                '(' => (b'(', b')'),
                '[' => (b'[', b']'),
                '{' => (b'{', b'}'),
                _ => (b'<', b'>'),
            };
            brackets(text, offset, rows, pair, object.last, nth, deadline)
                .and_then(|(open, close)| pair_span(text, open, close, object.inner, deadline))
        }
        // Not a pair `opening` gives (from args a host wrote).
        _ => None,
    };
    match span {
        Some(span) if !deadline.expired() => span,
        _ => Span::Failed,
    }
}

/// `row`'s bytes, without its line break.
fn row_range(text: &Text, row: u32) -> Range<usize> {
    let start = text.row_start(row);
    start..start + text.row_len(row) as usize
}

/// The `nth` pair of `pair` (from 0) after `offset` by its opening bracket,
/// or the `last` ones before it by its closing one, both brackets on
/// `rows`: their offsets.
fn brackets(
    text: &Text,
    offset: usize,
    rows: RangeInclusive<u32>,
    pair: (u8, u8),
    last: bool,
    nth: usize,
    deadline: &Deadline,
) -> Option<(usize, usize)> {
    // Each opening bracket as read, with its match once found, and the
    // pairs (by their opening bracket's index) as their matches are found.
    let mut opens: Vec<(usize, Option<usize>)> = Vec::new();
    let mut closed = Vec::new();
    // The opening brackets awaiting their match outside strings, across
    // rows, and within the string being read.
    let mut outside = Vec::new();
    for row in rows {
        if deadline.expired() {
            return None;
        }
        let range = row_range(text, row);
        // A row's strings count only if it has an even number of unescaped
        // `"`, as `motion::Scan` reads them.
        let (mut quotes, mut slashes) = (0usize, 0usize);
        each_byte(text, range.clone(), deadline, |_, byte| {
            if byte == b'"' && slashes % 2 == 0 {
                quotes += 1;
            }
            slashes = if byte == b'\\' { slashes + 1 } else { 0 };
            ControlFlow::Continue(())
        })?;
        let strings = quotes % 2 == 0;
        let (mut slashes, mut in_string) = (0usize, false);
        let mut inside = Vec::new();
        each_byte(text, range, deadline, |at, byte| {
            let escaped = slashes % 2 == 1;
            let quoted = strings && in_string;
            slashes = if byte == b'\\' { slashes + 1 } else { 0 };
            if byte == b'"' && !escaped {
                in_string = !in_string;
            }
            if escaped {
                return ControlFlow::Continue(());
            }
            // A string's closing quote ends its brackets.
            if quoted && byte == b'"' {
                inside.clear();
                return ControlFlow::Continue(());
            }
            let stack = match quoted {
                true => &mut inside,
                false => &mut outside,
            };
            if byte == pair.0 {
                stack.push(opens.len());
                opens.push((at, None));
            } else if byte == pair.1
                && let Some(ix) = stack.pop()
            {
                opens[ix].1 = Some(at);
                closed.push(ix);
            }
            ControlFlow::Continue(())
        })?;
    }
    if last {
        // Matches are found in the order of their closing brackets.
        let before = closed.partition_point(|&ix| opens[ix].1 < Some(offset));
        let &ix = closed[..before].iter().rev().nth(nth)?;
        return Some((opens[ix].0, opens[ix].1?));
    }
    let after = opens.partition_point(|&(open, _)| open <= offset);
    let mut left = nth;
    for (read, &(open, close)) in opens[after..].iter().enumerate() {
        if read % CHUNK == CHUNK - 1 && deadline.expired() {
            return None;
        }
        if let Some(close) = close {
            if left == 0 {
                return Some((open, close));
            }
            left -= 1;
        }
    }
    None
}

/// The `nth` string (from 0) of `quote` after `offset`, or the `last` ones
/// before it, on `rows`: its quotes' offsets. A row's quotes pair up from
/// its start: any quote opens a string, and only an unescaped one closes
/// it.
fn quotes(
    text: &Text,
    offset: usize,
    rows: RangeInclusive<u32>,
    quote: u8,
    last: bool,
    nth: usize,
    deadline: &Deadline,
) -> Option<(usize, usize)> {
    let caret_row = text.offset_to_point(offset).row;
    let order: Box<dyn Iterator<Item = u32>> = match last {
        true => Box::new((*rows.start()..=caret_row).rev()),
        false => Box::new(caret_row..=*rows.end()),
    };
    let mut left = nth;
    for row in order {
        if deadline.expired() {
            return None;
        }
        let mut strings = Vec::new();
        let (mut open, mut slashes) = (None, 0usize);
        each_byte(text, row_range(text, row), deadline, |at, byte| {
            let escaped = slashes % 2 == 1;
            slashes = if byte == b'\\' { slashes + 1 } else { 0 };
            if byte == quote {
                match open {
                    None => open = Some(at),
                    Some(start) if !escaped => {
                        strings.push((start, at));
                        open = None;
                    }
                    Some(_) => {}
                }
            }
            ControlFlow::Continue(())
        })?;
        // The strings that way from the caret, nearest first.
        let (found, there) = match last {
            true => {
                let before = strings.partition_point(|&(_, close)| close < offset);
                (strings[..before].iter().rev().nth(left), before)
            }
            false => {
                let after = strings.partition_point(|&(open, _)| open <= offset);
                (strings.get(after + left), strings.len() - after)
            }
        };
        match found {
            Some(&pair) => return Some(pair),
            None => left -= there,
        }
    }
    None
}

/// The string from the quote at `open` to the one at `close`: its inside if
/// `inner`, else with its quotes and the blanks after them on its row, or
/// with none, those before, as `i"` and `a"` take it; `None` if `deadline`
/// passes while the blanks are read.
fn quote_span(
    text: &Text,
    open: usize,
    close: usize,
    inner: bool,
    deadline: &Deadline,
) -> Option<Span> {
    if inner {
        return Some(Span::Chars(open + 1..close));
    }
    let blank = |byte: u8| byte == b' ' || byte == b'\t';
    let row = row_range(text, text.offset_to_point(close).row);
    let mut end = close + 1;
    each_byte(text, end..row.end, deadline, |at, byte| match blank(byte) {
        true => {
            end = at + 1;
            ControlFlow::Continue(())
        }
        false => ControlFlow::Break(()),
    })?;
    let mut start = open;
    // None after: back over those before, a chunk at a time.
    let mut hi = open;
    while end == close + 1 && hi > row.start {
        if deadline.expired() {
            return None;
        }
        let lo = text.clip_offset(hi.saturating_sub(CHUNK).max(row.start), Bias::Left);
        let chunk = text.slice_to_string(lo..hi);
        let run = chunk.bytes().rev().take_while(|&byte| blank(byte)).count();
        start = hi - run;
        if run < chunk.len() {
            break;
        }
        hi = lo;
    }
    Some(Span::Chars(start..end))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn a_pair_found_past_the_deadline_is_none() {
        let row = format!("x{} (a) \"b\"", " ".repeat(3 * CHUNK));
        let text = Text::new(&row);
        let passed = Deadline::after(Duration::ZERO);
        for pair in ['(', '"'] {
            let object = SeekObject {
                last: false,
                inner: true,
                pair,
            };
            assert!(matches!(
                within(&text, 0, object, 1, &Deadline::never()),
                Span::Chars(_)
            ));
            assert!(matches!(within(&text, 0, object, 1, &passed), Span::Failed));
        }
    }

    /// A deadline passing part way through a long row stops the search
    /// there: it fails in a fraction of the time reading the row takes.
    #[test]
    fn a_long_row_stops_at_the_deadline() {
        let blanks = " ".repeat(16 << 20);
        let text = Text::new(&format!("(a) \"b\"{blanks}(c) \"d\""));
        let end = text.len();
        for (pair, last, offset) in [
            ('(', false, 0),
            ('(', true, end),
            ('"', false, 0),
            ('"', true, end),
        ] {
            let object = SeekObject {
                last,
                inner: false,
                pair,
            };
            let started = Instant::now();
            let found = within(&text, offset, object, 1, &Deadline::never());
            let whole = started.elapsed();
            assert!(matches!(found, Span::Chars(_)), "{pair} {last}");
            let limit = whole / 20;
            let started = Instant::now();
            let found = within(&text, offset, object, 1, &Deadline::after(limit));
            let cut = started.elapsed();
            assert!(matches!(found, Span::Failed), "{pair} {last}");
            assert!(cut < whole / 2, "{pair} {last}: {cut:?} of {whole:?}");
        }
    }
}
