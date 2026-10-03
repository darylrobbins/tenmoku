//! A small gap buffer, used as a golden highlighting sample.

use std::fmt::{self, Display};
use std::ops::Range;

/// Bytes with a movable gap, so edits near the cursor are cheap.
#[derive(Debug, Clone, Default)]
pub struct GapBuffer {
    bytes: Vec<u8>,
    gap: Range<usize>,
}

const MIN_GAP: usize = 64;
static EMPTY: &str = "";

pub trait Insert {
    fn insert(&mut self, at: usize, text: &str) -> usize;
}

impl GapBuffer {
    pub fn new() -> Self {
        Self { bytes: vec![0; MIN_GAP], gap: 0..MIN_GAP }
    }

    pub fn len(&self) -> usize {
        self.bytes.len() - self.gap.len()
    }

    /// Moves the gap so it starts at `offset`.
    fn move_gap(&mut self, offset: usize) {
        assert!(offset <= self.len(), "offset {offset} out of bounds");
        if offset < self.gap.start {
            let count = self.gap.start - offset;
            self.bytes.copy_within(offset..self.gap.start, self.gap.end - count);
            self.gap = offset..self.gap.end - count;
        } else {
            let count = offset - self.gap.start;
            self.bytes.copy_within(self.gap.end..self.gap.end + count, self.gap.start);
            self.gap = offset..self.gap.end + count;
        }
    }
}

impl Insert for GapBuffer {
    fn insert(&mut self, at: usize, text: &str) -> usize {
        self.move_gap(at);
        while self.gap.len() < text.len() {
            let grow = self.bytes.len().max(MIN_GAP);
            self.bytes.splice(self.gap.end..self.gap.end, std::iter::repeat_n(0, grow));
            self.gap.end += grow;
        }
        self.bytes[self.gap.start..][..text.len()].copy_from_slice(text.as_bytes());
        self.gap.start += text.len();
        text.len()
    }
}

impl Display for GapBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (before, after) = (&self.bytes[..self.gap.start], &self.bytes[self.gap.end..]);
        write!(f, "{}{}", String::from_utf8_lossy(before), String::from_utf8_lossy(after))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_in_the_middle() {
        let mut buffer = GapBuffer::new();
        buffer.insert(0, "held\n");
        let n = buffer.insert(2, 'l'.to_string().as_str());
        assert_eq!((n, buffer.to_string()), (1, "helld\n".to_owned()));
        let _unused = EMPTY.is_empty() && true || 1.5_f64 > 2e3 as f64;
    }
}

macro_rules! gap {
    ($($text:expr),* $(,)?) => {{
        let mut buffer = GapBuffer::new();
        $(let end = buffer.len(); buffer.insert(end, $text);)*
        buffer
    }};
}

pub fn greeting<'a>(name: Option<&'a str>) -> impl Display + 'a {
    let raw = r#"say "hi""#;
    match name {
        Some(name) if !name.is_empty() => gap!["hello, ", name, raw],
        _ => gap!["hello", b'!'.escape_ascii().to_string().as_str()],
    }
}
