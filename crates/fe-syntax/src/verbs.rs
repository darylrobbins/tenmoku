//! Go's format verbs. The printf grammar is a C one: it reads `w` and `t` as
//! length modifiers, so `%w happened` is one long directive. Go's own syntax
//! is `%`, flags, width, precision, one verb letter, so injected `printf`
//! layers are coloured by this scanner instead of a highlights query.

use std::ops::Range;

use crate::highlight::{Highlight, Painted};

/// The verbs and `%%` escapes in `text`, which starts at byte `base`.
pub(crate) fn scan(text: &[u8], base: usize) -> Vec<Painted> {
    let mut spans = Vec::new();
    let (Some(placeholder), Some(escape)) = (
        Highlight::from_capture("string.placeholder"),
        Highlight::from_capture("escape"),
    ) else {
        return spans;
    };
    let mut at = 0;
    while let Some(found) = text[at..].iter().position(|&b| b == b'%') {
        let start = at + found;
        if text.get(start + 1) == Some(&b'%') {
            spans.push(span(base, start..start + 2, escape));
            at = start + 2;
        } else if let Some(end) = verb_end(text, start + 1) {
            spans.push(span(base, start..end, placeholder));
            at = end;
        } else {
            at = start + 1;
        }
    }
    spans
}

fn span(base: usize, range: Range<usize>, highlight: Highlight) -> Painted {
    Painted {
        range: base + range.start..base + range.end,
        highlight: Some(highlight),
    }
}

/// The end of the verb whose flags start at `at`, if there is one.
fn verb_end(text: &[u8], mut at: usize) -> Option<usize> {
    while matches!(text.get(at), Some(b'+' | b'-' | b'#' | b'0' | b' ')) {
        at += 1;
    }
    at = index(text, at);
    at = count(text, at);
    if text.get(at) == Some(&b'.') {
        at = count(text, index(text, at + 1));
    }
    at = index(text, at);
    text.get(at)
        .is_some_and(u8::is_ascii_alphabetic)
        .then_some(at + 1)
}

/// Past a `[n]` argument index at `at`, or `at` itself.
fn index(text: &[u8], at: usize) -> usize {
    if text.get(at) != Some(&b'[') {
        return at;
    }
    let digits = text[at + 1..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if digits > 0 && text.get(at + 1 + digits) == Some(&b']') {
        at + digits + 2
    } else {
        at
    }
}

/// Past a width or precision: digits, or `*` with its optional index.
fn count(text: &[u8], at: usize) -> usize {
    if text.get(at) == Some(&b'*') {
        return at + 1;
    }
    at + text[at.min(text.len())..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count()
}
