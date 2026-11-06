//! Go's format verbs. The printf grammar is a C one: it reads `w` and `t` as
//! length modifiers, so `%w happened` is one long directive. Go's own syntax
//! is `%`, flags, width, precision, one verb letter, so injected `printf`
//! layers are coloured by this scanner instead of a highlights query.

use crate::highlight::{Highlight, Painted};

/// The verbs and `%%` escapes in `text`, which starts at byte `base`.
#[cfg(test)]
fn scan(text: &[u8], base: usize) -> Vec<Painted> {
    let mut scanner = Scanner::new(base);
    let mut spans = Vec::new();
    scanner.feed(text, &mut spans);
    spans
}

/// Where a directive is, in Go's order: flags, `[n]`, width, `.`, `[n]`,
/// precision, `[n]`, verb.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    /// Outside any directive.
    Text,
    /// Just past a `%`.
    Opened,
    Flags,
    IndexWidth,
    Width,
    WidthDigits,
    Dot,
    IndexPrecision,
    Precision,
    PrecisionDigits,
    IndexVerb,
    Verb,
}

/// A resumable scan: bytes go in a chunk at a time, and only a phase and
/// two offsets are kept between them, so each byte costs the same however
/// long the directive it's in, and nothing is buffered or rescanned.
pub(crate) struct Scanner {
    /// The offset of the next byte.
    at: usize,
    /// The offset of the open directive's `%`.
    start: usize,
    phase: Phase,
    /// Inside a `[n]`: whether it has digits yet.
    bracket: Option<bool>,
    placeholder: Option<Highlight>,
    escape: Option<Highlight>,
}

impl Scanner {
    /// A scanner for text starting at byte `base`.
    pub(crate) fn new(base: usize) -> Self {
        Self {
            at: base,
            start: base,
            phase: Phase::Text,
            bracket: None,
            placeholder: Highlight::from_capture("string.placeholder"),
            escape: Highlight::from_capture("escape"),
        }
    }

    /// The offset of the next byte to feed.
    pub(crate) fn offset(&self) -> usize {
        self.at
    }

    /// Whether a directive is open: more text could still complete it.
    pub(crate) fn open(&self) -> bool {
        self.phase != Phase::Text
    }

    /// Scan the next `chunk`, adding the spans it completes to `out`. A
    /// directive the end of the text leaves open is never painted.
    pub(crate) fn feed(&mut self, chunk: &[u8], out: &mut Vec<Painted>) {
        let (Some(placeholder), Some(escape)) = (self.placeholder, self.escape) else {
            self.at += chunk.len();
            return;
        };
        for &b in chunk {
            // A byte that ends a phase without belonging to it is tried
            // again in the next one.
            loop {
                let digit = b.is_ascii_digit();
                let next = match self.phase {
                    Phase::Text => {
                        if b == b'%' {
                            self.start = self.at;
                            self.phase = Phase::Opened;
                        }
                        break;
                    }
                    Phase::Opened if b == b'%' => {
                        out.push(span(self.start, self.at + 1, escape));
                        self.phase = Phase::Text;
                        break;
                    }
                    Phase::Opened => Phase::Flags,
                    Phase::Flags if matches!(b, b'+' | b'-' | b'#' | b'0' | b' ') => break,
                    Phase::Flags => Phase::IndexWidth,
                    Phase::IndexWidth | Phase::IndexPrecision | Phase::IndexVerb => {
                        match self.bracket {
                            None if b == b'[' => {
                                self.bracket = Some(false);
                                break;
                            }
                            None => match self.phase {
                                Phase::IndexWidth => Phase::Width,
                                Phase::IndexPrecision => Phase::Precision,
                                _ => Phase::Verb,
                            },
                            Some(_) if digit => {
                                self.bracket = Some(true);
                                break;
                            }
                            Some(true) if b == b']' => {
                                self.bracket = None;
                                self.phase = match self.phase {
                                    Phase::IndexWidth => Phase::Width,
                                    Phase::IndexPrecision => Phase::Precision,
                                    _ => Phase::Verb,
                                };
                                break;
                            }
                            // A malformed index leaves no verb here.
                            Some(_) => {
                                self.bracket = None;
                                self.phase = Phase::Text;
                                continue;
                            }
                        }
                    }
                    Phase::Width if digit => {
                        self.phase = Phase::WidthDigits;
                        break;
                    }
                    Phase::Precision if digit => {
                        self.phase = Phase::PrecisionDigits;
                        break;
                    }
                    Phase::WidthDigits | Phase::PrecisionDigits if digit => break,
                    Phase::Width if b == b'*' => {
                        self.phase = Phase::Dot;
                        break;
                    }
                    Phase::Precision if b == b'*' => {
                        self.phase = Phase::IndexVerb;
                        break;
                    }
                    Phase::Width | Phase::WidthDigits => Phase::Dot,
                    Phase::Precision | Phase::PrecisionDigits => Phase::IndexVerb,
                    Phase::Dot if b == b'.' => {
                        self.phase = Phase::IndexPrecision;
                        break;
                    }
                    Phase::Dot => Phase::IndexVerb,
                    Phase::Verb => {
                        self.phase = Phase::Text;
                        if b.is_ascii_alphabetic() {
                            out.push(span(self.start, self.at + 1, placeholder));
                            break;
                        }
                        // Not a verb: this byte may open the next directive.
                        continue;
                    }
                };
                self.phase = next;
            }
            self.at += 1;
        }
    }
}

fn span(start: usize, end: usize, highlight: Highlight) -> Painted {
    Painted {
        range: start..end,
        highlight: Some(highlight),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(text: &str, spans: &[Painted]) -> Vec<String> {
        spans
            .iter()
            .map(|s| text[s.range.clone()].to_string())
            .collect()
    }

    fn chunked(text: &str, size: usize) -> Vec<Painted> {
        let mut scanner = Scanner::new(0);
        let mut out = Vec::new();
        for chunk in text.as_bytes().chunks(size) {
            scanner.feed(chunk, &mut out);
        }
        out
    }

    #[test]
    fn chunking_never_changes_the_spans() {
        let text = "a %v %5.2f %[1]d %*.*d %-+# 08.3[2]s %%x %[x]d %5*d %.[1]q %9 %%%w%";
        let whole = texts(text, &scan(text.as_bytes(), 0));
        assert!(whole.contains(&"%[1]d".to_string()), "{whole:?}");
        assert!(whole.contains(&"%*.*d".to_string()), "{whole:?}");
        assert!(!whole.iter().any(|t| t.contains("5*")), "{whole:?}");
        for size in 1..=text.len() {
            assert_eq!(texts(text, &chunked(text, size)), whole, "size {size}");
        }
    }

    #[test]
    fn a_long_open_directive_is_constant_state() {
        let mut scanner = Scanner::new(0);
        let mut out = Vec::new();
        scanner.feed(b"%", &mut out);
        let zeros = [b'0'; 4096];
        for _ in 0..256 {
            scanner.feed(&zeros, &mut out);
        }
        assert!(scanner.open() && out.is_empty());
        assert_eq!(scanner.offset(), 1 + 4096 * 256);
        scanner.feed(b"d", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].range, 0..scanner.offset());
        assert!(!scanner.open());
    }
}
