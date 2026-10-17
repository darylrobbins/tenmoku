use super::*;

use crate::motion::Scan;

/// `i"`, `a"`, `i'` and `a'` typed over a charwise selection of more than
/// one position, ported from vim's `current_quote`: the selection's new
/// anchor and cursor, or `None` for a selection on two rows or a search that
/// finds no string. Columns are bytes on the row, as vim's are.
pub(super) fn select(
    text: &Text,
    ends: &Ends,
    quote: u8,
    include: bool,
    count: u32,
) -> Option<(usize, usize)> {
    let row = text.offset_to_point(ends.anchor).row;
    if text.offset_to_point(ends.cursor).row != row {
        return None;
    }
    let scan = Scan::new(text, row);
    let line = &scan.bytes;
    let at = |ix: usize| line.get(ix).copied().unwrap_or(0);
    // A quote found by a search: past an escaped one when `escape`.
    let is_quote = |ix: usize, escape: bool| line[ix] == quote && !(escape && scan.escaped[ix]);
    let next = |from: usize, escape: bool| (from..line.len()).find(|&ix| is_quote(ix, escape));
    // `find_prev_quote`: column 0 when nothing is found.
    let prev = |from: usize, escape: bool| {
        (0..from)
            .rev()
            .find(|&ix| is_quote(ix, escape))
            .unwrap_or(0)
    };

    let (vis, cur) = (ends.anchor - scan.start, ends.cursor - scan.start);
    let forward = vis < cur;
    let (low, high) = (vis.min(cur), vis.max(cur));
    // The selection is just the inside of a quoted string, and holds a quote.
    let inside_quotes = low > 0 && at(low - 1) == quote && at(high) != 0 && at(high + 1) == quote;
    let selected_quote = (low..=high)
        .take_while(|&ix| at(ix) != 0)
        .any(|ix| at(ix) == quote);

    let (mut start, mut end);
    if at(cur) == quote {
        // On a quote already selecting something: the next string.
        if forward {
            start = next(cur + 1, false)?;
            match next(start + 1, true) {
                Some(close) => end = close,
                None => {
                    end = start;
                    start = cur;
                }
            }
        } else {
            end = prev(cur, false);
            if at(end) != quote {
                return None;
            }
            start = prev(end, true);
            if at(start) != quote {
                start = end;
                end = cur;
            }
        }
    } else {
        let first = match forward {
            true => next(cur, false)?,
            false => prev(cur, false),
        };
        // Search from the row's start for the string holding `first`.
        let mut open = 0;
        loop {
            let found = next(open, false)?;
            if found > first {
                return None;
            }
            let close = next(found + 1, true)?;
            if found <= first && first <= close {
                (start, end) = (found, close);
                break;
            }
            open = close + 1;
        }
    }

    if include {
        if matches!(at(end + 1), b' ' | b'\t') {
            while matches!(at(end + 1), b' ' | b'\t') {
                end += 1;
            }
        } else {
            while start > 0 && matches!(at(start - 1), b' ' | b'\t') {
                start -= 1;
            }
        }
    }
    if !include && count < 2 && !inside_quotes {
        start += 1;
    }
    let offset = |ix: usize| scan.start + ix;
    let before = |ix: usize| Cursor::at(text, scan.start + ix.saturating_sub(1)).offset();
    // Past the closing quote for `a`, `2i"`, or a selection just inside.
    let past = include || count > 1 || inside_quotes;
    let last = match past {
        true => offset(end),
        false => before(end),
    };
    if forward {
        let moves_anchor = !selected_quote
            && (inside_quotes || (at(vis) != quote && (vis == 0 || at(vis - 1) != quote)));
        let anchor = if moves_anchor {
            offset(start)
        } else {
            ends.anchor
        };
        Some((anchor, last))
    } else {
        let moves_anchor = inside_quotes
            || (!selected_quote && at(vis) != quote && (at(vis) == 0 || at(vis + 1) != quote));
        let anchor = if moves_anchor { last } else { ends.anchor };
        Some((anchor, offset(start)))
    }
}
