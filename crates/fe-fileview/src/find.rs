//! Finding text in a [`FileView`] by raw bytes, past the chunk cache.
//!
//! The viewer has no rope to run the editor's find over, and reading the file
//! through [`FileView::read`] would push all of it through the 16 MiB chunk
//! cache and evict what the screen shows. [`find`] reads [`FIND_BLOCK`] bytes
//! at a time with `read_uncached` instead.

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{FileView, ReadError};

/// How many bytes [`find`] reads at a time.
pub const FIND_BLOCK: u64 = 1 << 20;

/// What to look for. Case-insensitive matching folds ASCII letters only; other
/// bytes match exactly. A needle that is empty or holds `\n` or `\r` never
/// matches: a find bar is one line, and the viewer shows `\r\n` as a row break.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindQuery {
    pub needle: String,
    pub case_sensitive: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindOutcome {
    /// The match's byte range, and whether the search wrapped round to find it.
    Found {
        range: Range<u64>,
        wrapped: bool,
    },
    NotFound,
    /// `stop` was set before the search finished.
    Stopped,
}

/// Finds the next match from `from` in `direction`, wrapping once.
///
/// `within` is clamped to the view's length, and only matches lying wholly
/// inside it count. Forward finds the first match starting at or after `from`,
/// else wraps to the first one from `within.start`. Backward finds the last
/// match ending at or before `from`, else wraps to the last one before
/// `within.end`. A match without wrapping wins over a wrapped one, and the one
/// match covering `from` is found on the wrap if nothing else is.
///
/// `stop` is checked before each block read.
pub fn find(
    view: &FileView,
    query: &FindQuery,
    within: Range<u64>,
    from: u64,
    direction: Direction,
    stop: &AtomicBool,
) -> Result<FindOutcome, ReadError> {
    let needle = query.needle.as_bytes();
    if needle.is_empty() || needle.iter().any(|&b| b == b'\n' || b == b'\r') {
        return Ok(FindOutcome::NotFound);
    }
    let end = within.end.min(view.len());
    let start = within.start.min(end);
    let n = needle.len() as u64;
    if end - start < n {
        return Ok(FindOutcome::NotFound);
    }
    let matcher = Matcher::new(needle, query.case_sensitive);
    let from = from.clamp(start, end);
    let scan = |lo, hi| match direction {
        Direction::Forward => scan_forward(view, &matcher, lo..hi, stop),
        Direction::Backward => scan_backward(view, &matcher, lo..hi, stop),
    };
    let (first, wrap) = match direction {
        Direction::Forward => (from..end, start..end.min(from + n - 1)),
        Direction::Backward => (start..from, start.max(from.saturating_sub(n - 1))..end),
    };
    let wraps = match direction {
        Direction::Forward => from > start,
        Direction::Backward => from < end,
    };
    match scan(first.start, first.end)? {
        Scan::Stopped => return Ok(FindOutcome::Stopped),
        Scan::Match(range) => {
            return Ok(FindOutcome::Found {
                range,
                wrapped: false,
            });
        }
        Scan::None => {}
    }
    if !wraps {
        return Ok(FindOutcome::NotFound);
    }
    Ok(match scan(wrap.start, wrap.end)? {
        Scan::Stopped => FindOutcome::Stopped,
        Scan::Match(range) => FindOutcome::Found {
            range,
            wrapped: true,
        },
        Scan::None => FindOutcome::NotFound,
    })
}

enum Scan {
    Match(Range<u64>),
    None,
    Stopped,
}

/// A needle, lowercased when matching ignores case.
struct Matcher {
    needle: Vec<u8>,
    case_sensitive: bool,
}

impl Matcher {
    fn new(needle: &[u8], case_sensitive: bool) -> Matcher {
        let needle = if case_sensitive {
            needle.to_vec()
        } else {
            needle.to_ascii_lowercase()
        };
        Matcher {
            needle,
            case_sensitive,
        }
    }

    fn len(&self) -> u64 {
        self.needle.len() as u64
    }

    fn at(&self, hay: &[u8], i: usize) -> bool {
        let window = &hay[i..i + self.needle.len()];
        if self.case_sensitive {
            window == self.needle
        } else {
            window.eq_ignore_ascii_case(&self.needle)
        }
    }

    fn first(&self, hay: &[u8]) -> Option<usize> {
        let last = hay.len().checked_sub(self.needle.len())?;
        (0..=last).find(|&i| self.at(hay, i))
    }

    fn last(&self, hay: &[u8]) -> Option<usize> {
        let last = hay.len().checked_sub(self.needle.len())?;
        (0..=last).rev().find(|&i| self.at(hay, i))
    }
}

/// The bytes read per block: [`FIND_BLOCK`], or more for a huge needle so each
/// block still moves the scan on.
fn block_size(matcher: &Matcher) -> u64 {
    FIND_BLOCK.max(2 * matcher.len())
}

fn read_block(view: &FileView, range: Range<u64>) -> Result<Vec<u8>, ReadError> {
    let mut buf = vec![0; (range.end - range.start) as usize];
    view.read_uncached(&mut buf, range.start)?;
    Ok(buf)
}

fn scan_forward(
    view: &FileView,
    matcher: &Matcher,
    region: Range<u64>,
    stop: &AtomicBool,
) -> Result<Scan, ReadError> {
    let n = matcher.len();
    let block = block_size(matcher);
    let mut at = region.start;
    while region.end.saturating_sub(at) >= n {
        if stop.load(Ordering::Relaxed) {
            return Ok(Scan::Stopped);
        }
        let to = region.end.min(at + block);
        let bytes = read_block(view, at..to)?;
        if let Some(i) = matcher.first(&bytes) {
            let start = at + i as u64;
            return Ok(Scan::Match(start..start + n));
        }
        if to == region.end {
            break;
        }
        // Overlap, so a match across the block edge is seen whole, once.
        at = to - (n - 1);
    }
    Ok(Scan::None)
}

fn scan_backward(
    view: &FileView,
    matcher: &Matcher,
    region: Range<u64>,
    stop: &AtomicBool,
) -> Result<Scan, ReadError> {
    let n = matcher.len();
    let block = block_size(matcher);
    let mut to = region.end;
    while to.saturating_sub(region.start) >= n {
        if stop.load(Ordering::Relaxed) {
            return Ok(Scan::Stopped);
        }
        let at = region.start.max(to.saturating_sub(block));
        let bytes = read_block(view, at..to)?;
        if let Some(i) = matcher.last(&bytes) {
            let start = at + i as u64;
            return Ok(Scan::Match(start..start + n));
        }
        if at == region.start {
            break;
        }
        to = at + (n - 1);
    }
    Ok(Scan::None)
}
