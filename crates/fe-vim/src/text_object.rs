//! Text objects, resolved against the text when the operator runs: what
//! `iw aw iW aW i( a( i{ a{ i" a" i' a'` cover around a caret, as nvim
//! 0.12.4 selects them (the corpus records it).

use std::ops::Range;

use fe_text::Text;

use crate::{
    TextObject, delimiters,
    motion::{Class, Cursor, PAIRS, Quotes, Rules, Scan, nth_unmatched, unmatched},
    operator::Span,
    prose,
};

/// What `object` covers around the caret at `offset`, `count` times.
pub(crate) fn select(text: &Text, offset: usize, object: TextObject, count: u32) -> Span {
    use TextObject::*;
    let count = count.max(1);
    match object {
        InnerWord | AWord | InnerBigWord | ABigWord => {
            let big = matches!(object, InnerBigWord | ABigWord);
            let inner = matches!(object, InnerWord | InnerBigWord);
            words(text, offset, count, big, inner)
        }
        InnerParen | AParen => brackets(text, offset, PAIRS[0], count, object == InnerParen),
        InnerBrace | ABrace => brackets(text, offset, PAIRS[2], count, object == InnerBrace),
        InnerDoubleQuote | ADoubleQuote => {
            quotes(text, offset, b'"', count, object == InnerDoubleQuote)
        }
        InnerSingleQuote | ASingleQuote => {
            quotes(text, offset, b'\'', count, object == InnerSingleQuote)
        }
        InnerParagraph | AParagraph | InnerSentence | ASentence => {
            prose::select(text, offset, object, count)
        }
        InnerBracket | ABracket | InnerAngle | AAngle | InnerTag | ATag | InnerBacktick
        | ABacktick => delimiters::select(text, offset, object, count),
    }
}

/// A run of graphemes on one row with the same class, from grapheme `ix`
/// (on an empty row, the row itself): its byte range.
fn run(cursor: &mut Cursor, big: bool) -> Range<usize> {
    let start = cursor.offset();
    if cursor.line.len() > 0 {
        let class = cursor.class(big);
        while cursor.class_on_row(cursor.ix + 1, big) == Some(class) {
            cursor.ix += 1;
        }
    }
    start
        ..cursor.line.start
            + cursor
                .line
                .starts
                .get(cursor.ix + 1)
                .copied()
                .unwrap_or(cursor.line.text.len())
}

/// Back to the first grapheme of the run holding the cursor's.
fn run_start(cursor: &mut Cursor, big: bool) {
    if cursor.line.len() > 0 {
        let class = cursor.class(big);
        while cursor.ix > 0 && cursor.class_on_row(cursor.ix - 1, big) == Some(class) {
            cursor.ix -= 1;
        }
    }
}

/// `iw` from `cursor` (at the start of its run), `count` items as nvim
/// counts them; `None` past the end of the text. Each further item starts
/// one grapheme past the last's end: a run on its row, or from an empty row
/// up to (not including) the next row's start. An end like that, at column
/// 0, makes the rows whole from at or before the first non-blank, as
/// `exclusive-linewise` does, or else ends at the row before's end.
fn inner_words(text: &Text, mut cursor: Cursor, count: u32, big: bool) -> Option<Span> {
    let start = cursor.offset();
    let start_row = cursor.row;
    if cursor.line.len() > 0 {
        run(&mut cursor, big);
    }
    let mut inclusive = true;
    for _ in 1..count {
        // One grapheme on, across a row's end.
        if cursor.ix + 1 < cursor.line.len() {
            cursor.ix += 1;
        } else if cursor.row + 1 < text.row_count() {
            cursor.enter(cursor.row + 1);
            cursor.ix = 0;
        } else {
            return None;
        }
        inclusive = true;
        if cursor.line.len() > 0 {
            run(&mut cursor, big);
        } else {
            // Up to the next row's start, or on the last row, its own.
            if cursor.row + 1 < text.row_count() {
                cursor.enter(cursor.row + 1);
                cursor.ix = 0;
            }
            inclusive = false;
        }
    }
    let end = cursor.offset();
    if inclusive {
        let len = match cursor.line.len() {
            0 => 0,
            _ => cursor.line.grapheme(cursor.ix).len(),
        };
        return Some(Span::Chars(start..end + len));
    }
    // An exclusive end at a later row's column 0.
    let row_end = |row: u32| text.row_start(row) + text.row_len(row) as usize;
    let indent = text.slice_to_string(text.row_start(start_row)..start);
    Some(
        match indent.bytes().all(|byte| byte == b' ' || byte == b'\t') {
            true => Span::Lines {
                rows: start_row..=cursor.row - 1,
                first_non_blank: true,
                from: Some(start),
            },
            false => Span::Chars(start..row_end(cursor.row - 1)),
        },
    )
}

/// Whether the cursor's grapheme is blank (an empty row is not).
fn blank(cursor: &Cursor, big: bool) -> bool {
    cursor.line.len() > 0 && cursor.class(big) == Class::Blank
}

/// `iw` (`inner`) or `aw`, `count` items: for `iw`, runs of a word class or
/// of blanks, counted across rows as [`inner_words`] does; for `aw`, words
/// with the blanks after them, or before the first if there are none after
/// the last and they aren't the row's indent. Running out of text cancels,
/// leaving the caret on the last grapheme as nvim does.
fn words(text: &Text, offset: usize, count: u32, big: bool, inner: bool) -> Span {
    let mut cursor = Cursor::at(text, offset);
    // On an empty last row, nvim's `iw` takes the line break before it and
    // the grapheme before that; its `aw` fails.
    let last_row = text.row_count() - 1;
    if cursor.row == last_row && last_row > 0 && cursor.line.len() == 0 {
        if !inner {
            return Span::Failed;
        }
        // With a count there's nothing to take: a yank of nothing.
        if count > 1 {
            return Span::Chars(offset..offset);
        }
        let before = Cursor::at(text, text.row_start(last_row) - 1);
        return Span::Chars(before.offset()..text.len());
    }
    run_start(&mut cursor, big);
    let last = || Span::Move(Cursor::at(text, text.len()).offset());
    if inner {
        return inner_words(text, cursor, count, big).unwrap_or_else(last);
    }
    a_words(cursor, count, big).unwrap_or_else(last)
}

/// One position on: the next grapheme, or the next row's first (an empty
/// row is one position); false at the end of the text.
fn advance(cursor: &mut Cursor) -> bool {
    if cursor.ix + 1 < cursor.line.len() {
        cursor.ix += 1;
    } else if cursor.row + 1 < cursor.text.row_count() {
        cursor.enter(cursor.row + 1);
        cursor.ix = 0;
    } else {
        return false;
    }
    true
}

/// `aw` from `cursor` (at the start of its run), `count` items as nvim
/// counts them; `None` past the end of the text. An item is blanks then a
/// word, or a word then the blanks after it on its row. Blanks run across
/// rows, and an empty row starting them counts as blanks; an empty row
/// after them is the word. Without blanks after the last word (unless that's
/// an empty row), the blanks before the first are taken, unless they're the
/// row's indent.
fn a_words(mut cursor: Cursor, count: u32, big: bool) -> Option<Span> {
    let text = cursor.text;
    let start = cursor.offset();
    let started_white = blank(&cursor, big) || cursor.line.len() == 0;
    // Past blanks (from an empty row or a blank) to the word after them.
    let blanks = |cursor: &mut Cursor| -> bool {
        if cursor.line.len() > 0 {
            run(cursor, big);
        }
        if !advance(cursor) {
            return false;
        }
        while blank(cursor, big) {
            run(cursor, big);
            if !advance(cursor) {
                return false;
            }
        }
        true
    };
    // The blanks after the word, on its row.
    let after = |cursor: &mut Cursor| -> bool {
        match cursor.class_on_row(cursor.ix + 1, big) {
            Some(Class::Blank) => {
                cursor.ix += 1;
                run(cursor, big);
                true
            }
            _ => false,
        }
    };
    let mut ended_white = false;
    for item in 0..count {
        let white = match item {
            0 => started_white,
            _ => {
                if !advance(&mut cursor) {
                    return None;
                }
                blank(&cursor, big) || cursor.line.len() == 0
            }
        };
        if white && !blanks(&mut cursor) {
            return None;
        }
        run(&mut cursor, big);
        ended_white = !white && after(&mut cursor);
    }
    let len = match cursor.line.len() {
        0 => 0,
        _ => cursor.line.grapheme(cursor.ix).len(),
    };
    let end = cursor.offset() + len;
    let mut start = start;
    if !ended_white && !started_white && cursor.line.len() > 0 {
        let mut back = Cursor::at(text, start);
        while back.ix > 0 && back.class_on_row(back.ix - 1, big) == Some(Class::Blank) {
            back.ix -= 1;
        }
        if back.ix > 0 {
            start = back.offset();
        }
    }
    Some(Span::Chars(start..end))
}

/// `i(` or `a(` (`pair` holds the brackets): the innermost pair holding the
/// caret, or the `count`th out from it. With none around the caret, the
/// pair opening `count` deep in the brackets after it ([`next_bracket`]).
/// Escaped brackets don't count.
pub(crate) fn brackets(
    text: &Text,
    offset: usize,
    pair: (u8, u8),
    count: u32,
    inner: bool,
) -> Span {
    // The pair around: brackets in strings count. Its match: from outside
    // a string, brackets inside strings don't; from inside, only the
    // string's own do.
    let around = Rules {
        escaped: false,
        quotes: Quotes::Ignore,
        literals: false,
    };
    let byte = |at: usize| {
        let scan = Scan::new(text, text.offset_to_point(at).row);
        let ix = at - scan.start;
        scan.bytes.get(ix).filter(|_| !scan.escaped[ix]).copied()
    };
    let open = match byte(offset) {
        Some(byte) if byte == pair.0 => Some(offset),
        _ => unmatched(text, offset, false, pair, around),
    };
    // With none around, the pair opening `count` deep after the caret.
    let (open, count) = match open {
        Some(open) => (open, count),
        None => match next_bracket(text, offset, pair, count) {
            Some(open) => (open, 1),
            None => return Span::Failed,
        },
    };
    // The `count`th pair out, found in one pass each way.
    let open = match count {
        1 => open,
        _ => match nth_unmatched(text, open, false, pair, around, count - 1) {
            Some(outer) => outer,
            None => return Span::Failed,
        },
    };
    brackets_at(text, open, pair, inner)
}

/// The pair opening at `open`: with its brackets, or `inner` its inside.
fn brackets_at(text: &Text, open: usize, pair: (u8, u8), inner: bool) -> Span {
    let scan = Scan::new(text, text.offset_to_point(open).row);
    let rules = Rules {
        escaped: false,
        quotes: Quotes::from(scan.quoted.get(open - scan.start).copied().unwrap_or(false)),
        literals: false,
    };
    let Some(close) = unmatched(text, open, true, pair, rules) else {
        return Span::Failed;
    };
    if !inner {
        return Span::Chars(open..close + 1);
    }
    // An opening bracket ending its row starts the inside on the next row;
    // a closing one with only blanks before it ends it on the row before.
    // Both: whole rows.
    let mut start = open + 1;
    let open_row = text.offset_to_point(open).row;
    let row_end = |row: u32| text.row_start(row) + text.row_len(row) as usize;
    let starts_row = start == row_end(open_row) && open_row + 1 < text.row_count();
    if starts_row {
        start = text.row_start(open_row + 1);
    }
    let mut end = close;
    let close_row = text.offset_to_point(close).row;
    let before_close = text.slice_to_string(text.row_start(close_row)..close);
    let ends_row = close_row > open_row && before_close.bytes().all(|b| b == b' ' || b == b'\t');
    if ends_row {
        end = row_end(close_row - 1);
    }
    if starts_row && ends_row && end >= start {
        let rows = text.offset_to_point(start).row..=text.offset_to_point(end).row;
        return Span::Lines {
            rows,
            first_non_blank: true,
            from: None,
        };
    }
    // An empty inside is an empty range at its start: `di(` moves there,
    // `>i(` shifts its row.
    Span::Chars(start..end.max(start))
}

/// `i(` or `a(` typed in a visual mode over a selection of more than one
/// position, `lo` to `hi`: as nvim picks it, the pair from the selection's
/// start rather than the cursor, whether or not it holds the selection.
/// The opening bracket is the `count`th unclosed one before `lo` (the char
/// at `lo` not counted) or, with none, the one `count` deep after it
/// ([`next_bracket`]). An `a` object is the pair; an `inner` one its
/// inside, stepping out to the pair around `lo` while that inside is more
/// than one position and within the selection. `ends` gives the positions
/// a span takes in a visual mode (`None` if it selects nothing), and the
/// result is those, start first.
pub(crate) fn brackets_from(
    text: &Text,
    lo: usize,
    hi: usize,
    pair: (u8, u8),
    count: u32,
    inner: bool,
    ends: impl Fn(Span) -> Option<(usize, usize)>,
) -> Option<(usize, usize)> {
    let around = Rules {
        escaped: false,
        quotes: Quotes::Ignore,
        literals: false,
    };
    let count = count.max(1);
    let before = |from: usize, count: u32| match unmatched(text, from, false, pair, around) {
        Some(_) => nth_unmatched(text, from, false, pair, around, count).map(Some),
        None => Some(None),
    };
    let open = match before(lo, count)? {
        Some(open) => open,
        None => next_bracket(text, lo, pair, count)?,
    };
    let pick = |open: usize| ends(brackets_at(text, open, pair, inner));
    let found = pick(open)?;
    if !inner || found.0 < lo || found.1 > hi || found.1 == found.0 {
        return Some(found);
    }
    // The inside is within the selection: the pair around its start.
    let out = lo.checked_sub(1)?;
    let open = unmatched(text, out, false, pair, around)?;
    pick(open)
}

/// After `offset`, the unescaped opening bracket of `pair` that's `depth`
/// deep in the brackets after it, a closing one with none open counting as
/// one less (so with a depth of 1, one closing first leaves none).
fn next_bracket(text: &Text, offset: usize, pair: (u8, u8), depth: u32) -> Option<usize> {
    let first = text.offset_to_point(offset).row;
    let mut open = 0i64;
    for row in first..text.row_count() {
        let scan = Scan::new(text, row);
        let from = if row == first {
            offset - scan.start + 1
        } else {
            0
        };
        for ix in from..scan.bytes.len() {
            match scan.bytes[ix] {
                _ if scan.escaped[ix] => {}
                byte if byte == pair.1 => open -= 1,
                byte if byte == pair.0 => {
                    open += 1;
                    if open == i64::from(depth) {
                        return Some(scan.start + ix);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// `i"` or `a"` (`quote` is `"` or `'`), on the caret's row only. On a
/// quote, the row's quotes pair up from its start; otherwise the quotes
/// before and after the caret, or with none before, the row's first
/// string. An escaped quote doesn't count, except as the opening of that
/// first string.
/// `a"` takes the blanks after, or with none, before; `2i"` takes the
/// quotes only.
pub(crate) fn quotes(text: &Text, offset: usize, quote: u8, count: u32, inner: bool) -> Span {
    let scan = Scan::new(text, text.offset_to_point(offset).row);
    let caret = offset - scan.start;
    let bytes = &scan.bytes;
    let real: Vec<usize> = (0..bytes.len())
        .filter(|&ix| bytes[ix] == quote && !scan.escaped[ix])
        .collect();
    let before = real.partition_point(|&ix| ix < caret);
    let pair = match real.binary_search(&caret) {
        // On a quote, the row walked from its start says whether it opens: an
        // escaped quote only escapes inside a string, outside one it opens.
        Ok(ix) => {
            let mut open = None;
            for at in (0..caret).filter(|&at| bytes[at] == quote) {
                match open {
                    None => open = Some(at),
                    Some(_) if !scan.escaped[at] => open = None,
                    Some(_) => {}
                }
            }
            match open {
                Some(open) => Some((open, caret)),
                None => real.get(ix + 1).map(|&close| (caret, close)),
            }
        }
        Err(after) => match (before.checked_sub(1).map(|ix| real[ix]), real.get(after)) {
            (Some(open), Some(&close)) => Some((open, close)),
            // The row's first string, opening at its first quote, escaped or
            // not: the next string, or one an escaped quote opens.
            (None, _) => (0..bytes.len())
                .find(|&ix| bytes[ix] == quote)
                .and_then(|open| {
                    real.iter()
                        .find(|&&close| close > open)
                        .map(|&close| (open, close))
                }),
            (Some(_), None) => None,
        },
    };
    let Some((open, close)) = pair else {
        return Span::Failed;
    };
    let at = |ix: usize| scan.start + ix;
    if inner && count < 2 {
        return Span::Chars(at(open + 1)..at(close));
    }
    let (mut start, mut end) = (open, close + 1);
    if !inner {
        let is_blank = |ix: usize| matches!(bytes[ix], b' ' | b'\t');
        let after = (end..bytes.len()).take_while(|&ix| is_blank(ix)).count();
        if after > 0 {
            end += after;
        } else {
            start -= (0..start).rev().take_while(|&ix| is_blank(ix)).count();
        }
    }
    Span::Chars(at(start)..at(end))
}
