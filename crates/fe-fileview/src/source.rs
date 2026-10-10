//! [`FileText`]: a [`FileView`] and its [`LineIndex`] as a [`TextSource`].
//!
//! Offsets are file byte offsets. `Text` turns `\r\n` into `\n` and invalid
//! UTF-8 into U+FFFD (3 bytes), and either would shift every later offset by
//! an amount only a decode of everything before it could know. So rows keep
//! their bytes where the file has them, and decoding preserves length: valid
//! UTF-8 reads as itself and each byte of an invalid sequence reads as one
//! `?`. Only a line break read across rows changes length: `\r\n` reads as
//! `\n`, as it does in the rope.
//!
//! Whether a byte is part of a valid char depends only on the 3 bytes either
//! side of it, because UTF-8 is self-synchronising: a decode that starts 3
//! bytes early classifies every byte from the 4th on as a decode from the
//! file's start would. So every read here decodes a window with 3 bytes of
//! context on each side and keeps the middle.

use std::borrow::Cow;
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use fe_text::{Bias, Point, TextSource};

use crate::{
    CHUNK_BYTES, FileView, IndexState, LineIndex, MAX_SNAP_BACK, ReadError, ReadErrorKind,
};

/// The context a window keeps each side of what it decodes: a char is at most
/// 4 bytes.
const CONTEXT: u64 = MAX_SNAP_BACK;

/// A snapshot of a [`FileView`] and its [`LineIndex`] as a [`TextSource`].
/// `Clone` is cheap: it copies Arcs.
///
/// It reads as the file's prefix through the last `\n` the index had found
/// when it was taken, so while the index is building its last row is empty.
/// Once the index is `Complete`, it reads as the whole file. Take a new one to
/// see the rows found since.
///
/// Offsets are file byte offsets, and a row excludes its `\n` and a `\r` just
/// before it. Each byte of invalid UTF-8 reads as `?`, so within a row the
/// text is as long as its bytes. A `\r` not before a `\n` reads as itself.
///
/// A read that fails is recorded (see [`FileText::error`]) and answered with
/// empty text for what couldn't be read. No method panics.
#[derive(Clone, Debug)]
pub struct FileText {
    view: Arc<FileView>,
    index: Arc<LineIndex>,
    /// The last row: the line breaks the snapshot holds.
    last_row: u32,
    /// The start of the last row.
    last_start: u64,
    /// The whole file once the index is complete, else `last_start`.
    len: u64,
    /// The first read error, shared by clones.
    error: Arc<OnceLock<ReadErrorKind>>,
}

impl FileText {
    /// A snapshot of how far `index`, which indexes `view`, has got. An index
    /// that stopped because the file changed or a read failed starts the
    /// snapshot's [`error`](Self::error) off with that.
    pub fn new(view: Arc<FileView>, index: Arc<LineIndex>) -> FileText {
        let progress = index.progress();
        let error = Arc::new(OnceLock::new());
        match progress.state {
            IndexState::Changed => _ = error.set(ReadErrorKind::Changed),
            IndexState::Failed => _ = error.set(ReadErrorKind::Io),
            IndexState::Building | IndexState::Complete | IndexState::TooManyRows => {}
        }
        let mut text = FileText {
            view,
            index,
            last_row: 0,
            last_start: 0,
            len: 0,
            error,
        };
        // Only fails if the index has moved on and the file changed since.
        if let Some(start) = text.answer(text.index.row_start(progress.rows)) {
            text.last_row = progress.rows;
            text.last_start = start;
            text.len = if progress.state == IndexState::Complete {
                text.view.len()
            } else {
                start
            };
        }
        text
    }

    /// The first read that failed, if any has: on this snapshot, a clone of
    /// it, or the index's scan before it was taken.
    pub fn error(&self) -> Option<ReadErrorKind> {
        self.error.get().copied()
    }

    fn record(&self, error: &ReadError) {
        _ = self.error.set(error.kind());
    }

    /// The value of an index query, recording an error.
    fn answer<T>(&self, result: Result<Option<T>, ReadError>) -> Option<T> {
        result.unwrap_or_else(|error| {
            self.record(&error);
            None
        })
    }

    /// The bytes in `range`, clamped to the snapshot, or `None` after
    /// recording why they couldn't be read.
    fn bytes(&self, range: Range<u64>) -> Option<Vec<u8>> {
        let end = range.end.min(self.len);
        let start = range.start.min(end);
        self.view
            .read_bytes(start..end)
            .map_err(|error| self.record(&error))
            .ok()
    }

    /// The bytes in `range`, clamped to the snapshot, with up to [`CONTEXT`]
    /// bytes either side for decoding, and the window's start. `None` if
    /// `range` itself can't be read. Context that can't be read is left out,
    /// so the text next to it decodes as if the file ended there.
    fn window(&self, range: Range<u64>) -> Option<(u64, Vec<u8>)> {
        let end = range.end.min(self.len);
        let start = range.start.min(end);
        let middle = self.bytes(start..end)?;
        let before = self.context(start, false);
        let lo = start - before.len() as u64;
        let mut window = before;
        window.extend(middle);
        window.extend(self.context(end, true));
        Some((lo, window))
    }

    /// Up to [`CONTEXT`] bytes after `at` (or before it, if `after` is false),
    /// as many as can be read: the contiguous run nearest `at`, stopping at
    /// the first unavailable byte or chunk. Failures are recorded.
    fn context(&self, at: u64, after: bool) -> Vec<u8> {
        for n in (1..=CONTEXT).rev() {
            let range = if after {
                at..at + n
            } else {
                at.saturating_sub(n)..at
            };
            if range.is_empty() || range.start >= self.len {
                continue;
            }
            if let Some(bytes) = self.bytes(range) {
                return bytes;
            }
        }
        Vec::new()
    }

    /// Where `offset` sits, for the char boundary questions.
    fn place(&self, offset: u64) -> Place {
        if offset == 0 || offset >= self.len {
            return Place::Boundary;
        }
        let Some((lo, window)) = self.window(offset..offset + 1) else {
            return Place::Boundary;
        };
        let at = (offset - lo) as usize;
        if at > 0 && window[at - 1] == b'\r' && window.get(at) == Some(&b'\n') {
            return Place::InBreak;
        }
        match inside_char(&window, at) {
            Some(span) => Place::InChar(lo + span.start as u64..lo + span.end as u64),
            None => Place::Boundary,
        }
    }

    /// `row`'s bytes, without its line break, clamped to the last row.
    fn row_range(&self, row: u32) -> Range<u64> {
        let start = self.row_start(row) as u64;
        if row >= self.last_row {
            return start..self.len;
        }
        // The row's `\n` is just before the next row's start.
        let mut end = (self.row_start(row + 1) as u64)
            .saturating_sub(1)
            .max(start);
        if end > start && self.bytes(end - 1..end).as_deref() == Some(b"\r") {
            end -= 1;
        }
        start..end
    }

    /// `offset` clamped to the snapshot.
    fn clamped(&self, offset: usize) -> u64 {
        (offset as u64).min(self.len)
    }
}

/// Where an offset sits.
enum Place {
    Boundary,
    /// Between the `\r` and `\n` of a line break.
    InBreak,
    /// Inside the valid char spanning this range.
    InChar(Range<u64>),
}

impl TextSource for FileText {
    fn len(&self) -> usize {
        self.len as usize
    }

    fn row_count(&self) -> u32 {
        self.last_row + 1
    }

    /// Clamped to the last row, like `Text`.
    fn row_start(&self, row: u32) -> usize {
        if row >= self.last_row {
            return self.last_start as usize;
        }
        // Rows before the last are always found; a failed walk reads as an
        // empty row at the end.
        self.answer(self.index.row_start(row))
            .map_or(self.last_start, |start| start.min(self.last_start)) as usize
    }

    fn row_len(&self, row: u32) -> u32 {
        let range = self.row_range(row);
        u32::try_from(range.end - range.start).unwrap_or(u32::MAX)
    }

    fn offset_to_point(&self, offset: usize) -> Point {
        let offset = self.clamped(offset);
        let column = |start: u64| u32::try_from(offset.saturating_sub(start)).unwrap_or(u32::MAX);
        if offset >= self.last_start {
            return Point::new(self.last_row, column(self.last_start));
        }
        let row = self
            .answer(self.index.row_for_offset(offset))
            .map_or(0, |row| row.min(self.last_row));
        Point::new(row, column(self.row_start(row) as u64))
    }

    /// Offsets inside a valid char or between a `\r` and its `\n` move to
    /// that char's or break's start or end.
    fn clip_offset(&self, offset: usize, bias: Bias) -> usize {
        let offset = self.clamped(offset);
        let clipped = match (self.place(offset), bias) {
            (Place::Boundary, _) => offset,
            (Place::InBreak, Bias::Left) => offset - 1,
            (Place::InBreak, Bias::Right) => offset + 1,
            (Place::InChar(span), Bias::Left) => span.start,
            (Place::InChar(span), Bias::Right) => span.end,
        };
        clipped as usize
    }

    /// False inside a valid char, between a `\r` and its `\n`, and past the
    /// end. Each byte of an invalid sequence is a char of its own.
    fn is_char_boundary(&self, offset: usize) -> bool {
        let offset = offset as u64;
        offset <= self.len && matches!(self.place(offset), Place::Boundary)
    }

    /// The char starting at `offset`: `\n` at a line break, either byte of
    /// which may start it, and `?` for each byte of an invalid sequence.
    /// `None` at or past the end and inside a valid char.
    fn char_at(&self, offset: usize) -> Option<char> {
        let offset = offset as u64;
        if offset >= self.len {
            return None;
        }
        let (lo, window) = self.window(offset..offset + 1)?;
        let at = (offset - lo) as usize;
        if inside_char(&window, at).is_some() {
            return None;
        }
        let rest = &window[at..];
        Some(match rest[0] {
            b'\r' if rest.get(1) == Some(&b'\n') => '\n',
            lead => rest
                .get(..utf8_width(lead))
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .and_then(|text| text.chars().next())
                .unwrap_or('?'),
        })
    }

    /// One owned chunk per [`CHUNK_BYTES`] of the file, read through the
    /// view's cache as the iterator is advanced, so a range as long as the
    /// file is never read whole. Each line break reads as `\n`, and part of a
    /// valid char cut by the range's ends reads as a `?` per byte.
    fn chunks_in_range(&self, range: Range<usize>) -> Box<dyn Iterator<Item = Cow<'_, str>> + '_> {
        let end = self.clamped(range.end);
        let mut at = self.clamped(range.start).min(end);
        Box::new(std::iter::from_fn(move || {
            while at < end {
                let mut to = end.min((at / CHUNK_BYTES + 1) * CHUNK_BYTES);
                let Some((lo, window)) = self.window(at..to) else {
                    at = end;
                    return None;
                };
                if to < end {
                    // Keep a char that straddles the chunk edge whole, unless
                    // the range ends inside it too.
                    if let Some(span) = inside_char(&window, (to - lo) as usize) {
                        to = (lo + span.end as u64).min(end);
                    }
                }
                let text = decode(&window, (at - lo) as usize..(to - lo) as usize);
                at = to;
                if !text.is_empty() {
                    return Some(Cow::Owned(text));
                }
            }
            None
        }))
    }

    /// Allocates for the range clamped to the snapshot, not as asked.
    fn slice_to_string(&self, range: Range<usize>) -> String {
        let end = self.clamped(range.end);
        let start = self.clamped(range.start).min(end);
        let mut text = String::with_capacity((end - start) as usize);
        for chunk in self.chunks_in_range(start as usize..end as usize) {
            text.push_str(&chunk);
        }
        text
    }

    /// Walks the index once per row end, where the default walks to the
    /// row's start twice.
    fn row_to_string(&self, row: u32) -> String {
        let range = self.row_range(row);
        self.slice_to_string(range.start as usize..range.end as usize)
    }

    fn share(&self) -> Arc<dyn TextSource> {
        Arc::new(self.clone())
    }
}

/// Decodes `window[range]`, where `window` holds up to [`CONTEXT`] bytes
/// either side of `range` (fewer only at the ends of the text). Every byte
/// reads as one byte, except a `\r` before a `\n`, which reads as nothing.
fn decode(window: &[u8], range: Range<usize>) -> String {
    let mut text = String::with_capacity(range.len());
    let mut at = 0;
    for chunk in window.utf8_chunks() {
        let valid = chunk.valid();
        push_valid(&mut text, valid, at, &range);
        at += valid.len();
        let invalid = chunk.invalid().len();
        let overlap = overlap(at..at + invalid, &range);
        text.extend(std::iter::repeat_n('?', overlap.len()));
        at += invalid;
    }
    // The text is byte for byte the window's range, and `\r` and `\n` read
    // as themselves, so the text has a `\r\n` where the range does.
    if text.contains('\r') {
        if text.ends_with('\r') && window.get(range.end) == Some(&b'\n') {
            text.pop();
        }
        text = text.replace("\r\n", "\n");
    }
    text
}

/// Pushes the part of `valid`, which starts at `at`, inside `range`. A char
/// cut by `range` reads as a `?` per byte inside it.
fn push_valid(text: &mut String, valid: &str, at: usize, range: &Range<usize>) {
    let overlap = overlap(at..at + valid.len(), range);
    if overlap.is_empty() {
        return;
    }
    let (start, end) = (overlap.start - at, overlap.end - at);
    let mut inner_start = start;
    while !valid.is_char_boundary(inner_start) {
        inner_start += 1;
    }
    let mut inner_end = end;
    while !valid.is_char_boundary(inner_end) {
        inner_end -= 1;
    }
    if inner_start >= inner_end {
        text.extend(std::iter::repeat_n('?', end - start));
        return;
    }
    text.extend(std::iter::repeat_n('?', inner_start - start));
    text.push_str(&valid[inner_start..inner_end]);
    text.extend(std::iter::repeat_n('?', end - inner_end));
}

fn overlap(a: Range<usize>, b: &Range<usize>) -> Range<usize> {
    a.start.max(b.start)..a.end.min(b.end).max(a.start.max(b.start))
}

/// The span of the valid char that `bytes[at]` is inside, not at the start
/// of. `bytes` holds [`CONTEXT`] bytes either side of `at` where the text has
/// them.
fn inside_char(bytes: &[u8], at: usize) -> Option<Range<usize>> {
    if !bytes.get(at).copied().is_some_and(crate::is_continuation) {
        return None;
    }
    let start = (at.saturating_sub(CONTEXT as usize)..at)
        .rev()
        .find(|&ix| !crate::is_continuation(bytes[ix]))?;
    let end = start + utf8_width(bytes[start]);
    let valid = end > at
        && bytes
            .get(start..end)
            .is_some_and(|c| std::str::from_utf8(c).is_ok());
    valid.then_some(start..end)
}

/// The length of the char that `lead` starts, if it starts a valid one.
fn utf8_width(lead: u8) -> usize {
    match lead {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding_keeps_lengths_and_drops_the_cr_of_a_crlf() {
        let window = "aé€\r\nb\rc😀".as_bytes();
        assert_eq!(decode(window, 0..window.len()), "aé€\nb\rc😀");
        // The ranges cut é, € and 😀: their bytes in range read as `?`.
        assert_eq!(decode(window, 2..5), "???");
        assert_eq!(decode(window, 2..6), "?€");
        assert_eq!(decode(window, 6..8), "\n");
        assert_eq!(decode(window, 6..7), "");
        assert_eq!(decode(window, 7..window.len() - 1), "\nb\rc???");
        let invalid = b"a\xe2\x82b\xff\xf0\x9f\x98";
        assert_eq!(decode(invalid, 0..invalid.len()), "a??b????");
    }

    #[test]
    fn inside_char_needs_a_whole_valid_char() {
        let bytes = "aé€😀".as_bytes();
        assert_eq!(inside_char(bytes, 2), Some(1..3));
        assert_eq!(inside_char(bytes, 5), Some(3..6));
        assert_eq!(inside_char(bytes, 9), Some(6..10));
        assert_eq!(inside_char(bytes, 6), None);
        assert_eq!(inside_char(&bytes[..9], 8), None);
        assert_eq!(inside_char(b"\x80\x80\x80\x80", 3), None);
    }
}
