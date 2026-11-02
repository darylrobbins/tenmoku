//! Quick-scope targets: which chars on the caret's row `f` and `F` reach
//! most cheaply, for a host to highlight.
//!
//! Walking outward from the caret (counts are kept in walking order), a
//! word's target is its leftmost char seen once so far, a primary (`f{c}` reaches it); failing
//! that its leftmost seen twice, a secondary (`2f{c}`). Accepted chars are
//! ASCII letters and digits (quick-scope's `g:qs_accepted_chars`), counted
//! case-sensitively. The rest of the caret's own word is counted, not
//! shown. A grapheme with combining marks is a separator. At most
//! [`MAX_CHARS`] chars are read each way.

use std::collections::HashMap;

use fe_text::Text;
use unicode_segmentation::UnicodeSegmentation;

/// Chars read each way from the caret (quick-scope's `g:qs_max_chars`).
pub const MAX_CHARS: usize = 1000;

/// A char to highlight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    /// Byte offset of the char in the text.
    pub offset: usize,
    /// Whether `f{c}` reaches it, not only `2f{c}`.
    pub primary: bool,
}

/// The targets on the caret's row going `forward` (`f`) or backward (`F`),
/// in row order.
pub fn targets(text: &Text, caret: usize, forward: bool) -> Vec<Target> {
    let caret = text.clip_offset(caret, fe_text::Bias::Left);
    let row = text.offset_to_point(caret).row;
    let start = text.row_start(row);
    let end = start + text.row_len(row) as usize;
    let mut found = Vec::new();
    let caret_char = text.char_at(caret).filter(|_| caret < end);
    let after = caret + caret_char.map_or(0, char::len_utf8);
    // The caret is on a word only if its whole grapheme is one accepted
    // char: not an ASCII base that a combining mark follows.
    let on_word = caret_char.is_some_and(|c| {
        c.is_ascii_alphanumeric()
            && text
                .char_at(after)
                .filter(|_| after < end)
                .is_none_or(|next| {
                    let pair: String = [c, next].iter().collect();
                    pair.graphemes(true).next().is_some_and(|g| g.len() == 1)
                })
    });
    // The window is MAX_CHARS chars from the caret, found by stepping, so
    // no more is read. Going back the caret's own char follows as context.
    // Going forward a grapheme cut by the limit can't be told whole, so
    // when the row goes on, the last one is dropped.
    let (from, to, limit) = if forward {
        let mut to = after;
        for _ in 0..MAX_CHARS {
            match text.char_at(to).filter(|_| to < end) {
                Some(c) => to += c.len_utf8(),
                None => break,
            }
        }
        (after, to, after)
    } else {
        let mut from = caret;
        for _ in 0..MAX_CHARS {
            if from <= start {
                break;
            }
            from -= 1;
            while !text.is_char_boundary(from) {
                from -= 1;
            }
        }
        (from, after, caret)
    };
    if from >= to {
        return found;
    }
    let window = text.slice_to_string(from..to);
    // (offset, grapheme) in walking order, within the limit.
    let mut walk: Vec<(usize, &str)> = Vec::new();
    if forward {
        walk.extend(window.grapheme_indices(true).map(|(i, g)| (from + i, g)));
        if to < end {
            walk.pop();
        }
    } else {
        walk.extend(
            window
                .grapheme_indices(true)
                .rev()
                .map(|(i, g)| (from + i, g))
                .filter(|&(offset, _)| offset < limit),
        );
    }

    let accepted = |g: &str| g.len() == 1 && g.as_bytes()[0].is_ascii_alphanumeric();
    let mut counts: HashMap<u8, u32> = HashMap::new();
    let mut iter = walk.into_iter().peekable();
    // The rest of the caret's word is counted, not shown.
    while let Some(&(_, g)) = iter.peek() {
        if !on_word || !accepted(g) {
            break;
        }
        *counts.entry(g.as_bytes()[0]).or_default() += 1;
        iter.next();
    }
    let mut first = None;
    let mut second = None;
    let mut flush = |first: &mut Option<usize>, second: &mut Option<usize>| {
        if let Some(offset) = first.take() {
            found.push(Target {
                offset,
                primary: true,
            });
            second.take();
        } else if let Some(offset) = second.take() {
            found.push(Target {
                offset,
                primary: false,
            });
        }
    };
    for (offset, g) in iter {
        if !accepted(g) {
            flush(&mut first, &mut second);
            continue;
        }
        let count = counts.entry(g.as_bytes()[0]).or_default();
        *count += 1;
        match *count {
            // The word's leftmost: going back, the last one seen.
            1 if first.is_none() || !forward => first = Some(offset),
            2 if second.is_none() || !forward => second = Some(offset),
            _ => {}
        }
    }
    flush(&mut first, &mut second);
    if !forward {
        found.reverse();
    }
    found
}
