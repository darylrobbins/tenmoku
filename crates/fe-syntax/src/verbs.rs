//! Go's format verbs. The printf grammar is a C one: it reads `w` and `t` as
//! length modifiers, so `%w happened` is one long directive. Go's own syntax
//! is `%`, flags, width, precision, one verb letter, so injected `printf`
//! layers are coloured by this scanner instead of a highlights query. The C
//! dialect (`printf-c` layers) follows C's directive syntax.

use crate::highlight::{Highlight, Painted};

/// The verbs and `%%` escapes in `text`, which starts at byte `base`.
#[cfg(test)]
fn scan(text: &[u8], base: usize) -> Vec<Painted> {
    let mut scanner = Scanner::new(base, Dialect::Go);
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
    // The C dialect's phases.
    /// Digits right after `%`: a `n$` position or the width.
    CNum,
    CFlags,
    CWidth,
    CWidthDigits,
    /// After a width `*`: maybe `n$`.
    CWidthStar,
    CWidthStarDigits,
    CDot,
    CPrecision,
    CPrecisionDigits,
    CPrecisionStar,
    CPrecisionStarDigits,
    /// Length modifiers, then the conversion.
    CLength,
}

/// Which language's directive syntax a [`Scanner`] reads.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Dialect {
    Go,
    C,
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
    dialect: Dialect,
    /// Inside a `[n]`: whether it has digits yet.
    bracket: Option<bool>,
    placeholder: Option<Highlight>,
    escape: Option<Highlight>,
}

impl Scanner {
    /// A scanner for text starting at byte `base`.
    pub(crate) fn new(base: usize, dialect: Dialect) -> Self {
        Self {
            dialect,
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
        if self.dialect == Dialect::C {
            self.feed_c(chunk, placeholder, escape, out);
            return;
        }
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
                    // The C dialect's phases are not reached from Go.
                    Phase::CNum
                    | Phase::CFlags
                    | Phase::CWidth
                    | Phase::CWidthDigits
                    | Phase::CWidthStar
                    | Phase::CWidthStarDigits
                    | Phase::CDot
                    | Phase::CPrecision
                    | Phase::CPrecisionDigits
                    | Phase::CPrecisionStar
                    | Phase::CPrecisionStarDigits
                    | Phase::CLength => {
                        self.phase = Phase::Text;
                        continue;
                    }
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

impl Scanner {
    /// [`Scanner::feed`] for C: `%`, `n$`, flags, width, `.precision`,
    /// length modifiers, one conversion.
    fn feed_c(
        &mut self,
        chunk: &[u8],
        placeholder: Highlight,
        escape: Highlight,
        out: &mut Vec<Painted>,
    ) {
        for &b in chunk {
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
                    Phase::Opened if digit && b != b'0' => {
                        self.phase = Phase::CNum;
                        break;
                    }
                    Phase::Opened => Phase::CFlags,
                    Phase::CNum if digit => break,
                    Phase::CNum if b == b'$' => {
                        self.phase = Phase::CFlags;
                        break;
                    }
                    Phase::CNum => Phase::CDot,
                    Phase::CFlags if matches!(b, b'-' | b'+' | b' ' | b'#' | b'0' | b'\'') => {
                        break;
                    }
                    Phase::CFlags => Phase::CWidth,
                    Phase::CWidth if digit => {
                        self.phase = Phase::CWidthDigits;
                        break;
                    }
                    Phase::CWidth if b == b'*' => {
                        self.phase = Phase::CWidthStar;
                        break;
                    }
                    Phase::CWidth => Phase::CDot,
                    Phase::CWidthDigits if digit => break,
                    Phase::CWidthDigits => Phase::CDot,
                    Phase::CWidthStar if digit => {
                        self.phase = Phase::CWidthStarDigits;
                        break;
                    }
                    Phase::CWidthStar => Phase::CDot,
                    Phase::CWidthStarDigits if digit => break,
                    Phase::CWidthStarDigits if b == b'$' => {
                        self.phase = Phase::CDot;
                        break;
                    }
                    Phase::CWidthStarDigits => {
                        self.phase = Phase::Text;
                        continue;
                    }
                    Phase::CDot if b == b'.' => {
                        self.phase = Phase::CPrecision;
                        break;
                    }
                    Phase::CDot => Phase::CLength,
                    Phase::CPrecision if digit => {
                        self.phase = Phase::CPrecisionDigits;
                        break;
                    }
                    Phase::CPrecision if b == b'*' => {
                        self.phase = Phase::CPrecisionStar;
                        break;
                    }
                    Phase::CPrecision => Phase::CLength,
                    Phase::CPrecisionDigits if digit => break,
                    Phase::CPrecisionDigits => Phase::CLength,
                    Phase::CPrecisionStar if digit => {
                        self.phase = Phase::CPrecisionStarDigits;
                        break;
                    }
                    Phase::CPrecisionStar => Phase::CLength,
                    Phase::CPrecisionStarDigits if digit => break,
                    Phase::CPrecisionStarDigits if b == b'$' => {
                        self.phase = Phase::CLength;
                        break;
                    }
                    Phase::CPrecisionStarDigits => {
                        self.phase = Phase::Text;
                        continue;
                    }
                    Phase::CLength
                        if matches!(b, b'h' | b'l' | b'j' | b'z' | b't' | b'L' | b'q') =>
                    {
                        break;
                    }
                    Phase::CLength => {
                        self.phase = Phase::Text;
                        if b"diouxXeEfFgGaAcCsSpnmb".contains(&b) {
                            out.push(span(self.start, self.at + 1, placeholder));
                            break;
                        }
                        // Not a conversion: this byte may open the next directive.
                        continue;
                    }
                    // Go's phases are not reached from the C dialect.
                    _ => {
                        self.phase = Phase::Text;
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
        let mut scanner = Scanner::new(0, Dialect::Go);
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
        let mut scanner = Scanner::new(0, Dialect::Go);
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

#[cfg(test)]
#[path = "verbs_c_tests.rs"]
mod c_dialect_tests;
