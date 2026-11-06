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
    /// Before any length modifier; then the conversion.
    CLength,
    /// After one `h`: another makes `hh`.
    CLengthH,
    /// After one `l`: another makes `ll`.
    CLengthL,
    /// After a complete length modifier: only the conversion is left.
    CLengthDone,
    // Python's `%` dialect, after `%`: an optional `(key)`, flags, width,
    // `.precision`, one length modifier that is ignored, the conversion.
    PyKey,
    PyFlags,
    PyWidth,
    PyDot,
    PyPrecision,
    PyLength,
    // Python's `str.format` fields.
    /// Just past a `{`.
    BraceOpen,
    /// In the field name: names, digits, `.attr`, `[idx]`.
    BraceArg,
    /// Just past `!`.
    BraceConv,
    /// After the conversion letter: `:` or `}`.
    BraceConvDone,
    /// In the format spec, which may nest `{...}` once.
    BraceSpec,
    /// Just past a `}` outside a field.
    BraceClose,
}

/// Which language's directive syntax a [`Scanner`] reads.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Dialect {
    Go,
    C,
    /// Python's `%` operator and logging-style calls.
    PyPercent,
    /// Python's `str.format` replacement fields.
    Brace,
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
    /// Nesting: parens in a `%(key)`, braces in a format spec, or an open
    /// `[idx]` in a field name.
    depth: u32,
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
            depth: 0,
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
        match self.dialect {
            Dialect::C => {
                self.feed_c(chunk, placeholder, escape, out);
                return;
            }
            Dialect::PyPercent => {
                self.feed_py_percent(chunk, placeholder, escape, out);
                return;
            }
            Dialect::Brace => {
                self.feed_brace(chunk, placeholder, escape, out);
                return;
            }
            Dialect::Go => {}
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
                    // The other dialects' phases are not reached from Go.
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
                    | Phase::CLength
                    | Phase::CLengthH
                    | Phase::CLengthL
                    | Phase::CLengthDone
                    | Phase::PyKey
                    | Phase::PyFlags
                    | Phase::PyWidth
                    | Phase::PyDot
                    | Phase::PyPrecision
                    | Phase::PyLength
                    | Phase::BraceOpen
                    | Phase::BraceArg
                    | Phase::BraceConv
                    | Phase::BraceConvDone
                    | Phase::BraceSpec
                    | Phase::BraceClose => {
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
                    Phase::CLength if matches!(b, b'j' | b'z' | b't' | b'L' | b'q') => {
                        self.phase = Phase::CLengthDone;
                        break;
                    }
                    Phase::CLength if b == b'h' => {
                        self.phase = Phase::CLengthH;
                        break;
                    }
                    Phase::CLength if b == b'l' => {
                        self.phase = Phase::CLengthL;
                        break;
                    }
                    Phase::CLengthH if b == b'h' => {
                        self.phase = Phase::CLengthDone;
                        break;
                    }
                    Phase::CLengthL if b == b'l' => {
                        self.phase = Phase::CLengthDone;
                        break;
                    }
                    Phase::CLength | Phase::CLengthH | Phase::CLengthL | Phase::CLengthDone => {
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

impl Scanner {
    /// [`Scanner::feed`] for Python's `%`: `%`, `(key)`, flags, width,
    /// `.precision`, an ignored length modifier, one conversion.
    fn feed_py_percent(
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
                    Phase::Opened if b == b'%' => {
                        out.push(span(self.start, self.at + 1, escape));
                        self.phase = Phase::Text;
                        break;
                    }
                    Phase::Opened if b == b'(' => {
                        self.depth = 1;
                        self.phase = Phase::PyKey;
                        break;
                    }
                    Phase::Opened => Phase::PyFlags,
                    Phase::PyKey => {
                        match b {
                            b'(' => self.depth = self.depth.saturating_add(1),
                            b')' => {
                                self.depth -= 1;
                                if self.depth == 0 {
                                    self.phase = Phase::PyFlags;
                                }
                            }
                            _ => {}
                        }
                        break;
                    }
                    Phase::PyFlags if matches!(b, b'-' | b'+' | b' ' | b'#' | b'0') => break,
                    Phase::PyFlags => Phase::PyWidth,
                    Phase::PyWidth if digit => break,
                    Phase::PyWidth if b == b'*' => {
                        self.phase = Phase::PyDot;
                        break;
                    }
                    Phase::PyWidth => Phase::PyDot,
                    Phase::PyDot if b == b'.' => {
                        self.phase = Phase::PyPrecision;
                        break;
                    }
                    Phase::PyDot => Phase::PyLength,
                    Phase::PyPrecision if digit => break,
                    Phase::PyPrecision if b == b'*' => {
                        self.phase = Phase::PyLength;
                        break;
                    }
                    Phase::PyPrecision => Phase::PyLength,
                    Phase::PyLength if matches!(b, b'h' | b'l' | b'L') => {
                        self.phase = Phase::CLengthDone;
                        break;
                    }
                    Phase::PyLength | Phase::CLengthDone => {
                        self.phase = Phase::Text;
                        if b"diouxXeEfFgGcrsa".contains(&b) {
                            out.push(span(self.start, self.at + 1, placeholder));
                            break;
                        }
                        // Not a conversion: this byte may open the next directive.
                        continue;
                    }
                    // Any other phase is `Text` (or not this dialect's).
                    _ => {
                        if b == b'%' {
                            self.start = self.at;
                            self.phase = Phase::Opened;
                        } else {
                            self.phase = Phase::Text;
                        }
                        break;
                    }
                };
                self.phase = next;
            }
            self.at += 1;
        }
    }

    /// [`Scanner::feed`] for `str.format`: `{field}` painted whole, `{{` and
    /// `}}` as escapes; a field that doesn't close or doesn't parse paints
    /// nothing.
    fn feed_brace(
        &mut self,
        chunk: &[u8],
        placeholder: Highlight,
        escape: Highlight,
        out: &mut Vec<Painted>,
    ) {
        for &b in chunk {
            loop {
                match self.phase {
                    Phase::BraceOpen if b == b'{' => {
                        out.push(span(self.start, self.at + 1, escape));
                        self.phase = Phase::Text;
                    }
                    Phase::BraceOpen => {
                        self.depth = 0;
                        self.phase = Phase::BraceArg;
                        continue;
                    }
                    Phase::BraceArg if self.depth == 1 => {
                        // Inside `[idx]`: anything but a brace.
                        match b {
                            b']' => self.depth = 0,
                            b'{' | b'}' => {
                                self.phase = Phase::Text;
                                continue;
                            }
                            _ => {}
                        }
                    }
                    Phase::BraceArg => match b {
                        b'[' => self.depth = 1,
                        b'!' => self.phase = Phase::BraceConv,
                        b':' => self.phase = Phase::BraceSpec,
                        b'}' => {
                            out.push(span(self.start, self.at + 1, placeholder));
                            self.phase = Phase::Text;
                        }
                        _ if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.') || b >= 0x80 => {
                        }
                        _ => {
                            self.phase = Phase::Text;
                            continue;
                        }
                    },
                    Phase::BraceConv if matches!(b, b'r' | b's' | b'a') => {
                        self.phase = Phase::BraceConvDone;
                    }
                    Phase::BraceConvDone if b == b':' => self.phase = Phase::BraceSpec,
                    Phase::BraceConvDone if b == b'}' => {
                        out.push(span(self.start, self.at + 1, placeholder));
                        self.phase = Phase::Text;
                    }
                    Phase::BraceConv | Phase::BraceConvDone => {
                        self.phase = Phase::Text;
                        continue;
                    }
                    Phase::BraceSpec => match (b, self.depth) {
                        (b'}', 0) => {
                            out.push(span(self.start, self.at + 1, placeholder));
                            self.phase = Phase::Text;
                        }
                        (b'}', _) => self.depth = 0,
                        (b'{', 0) => self.depth = 1,
                        (b'{', _) => {
                            self.phase = Phase::Text;
                            continue;
                        }
                        _ => {}
                    },
                    Phase::BraceClose => {
                        self.phase = Phase::Text;
                        if b == b'}' {
                            out.push(span(self.start, self.at + 1, escape));
                        } else {
                            continue;
                        }
                    }
                    // `Text`, or a phase of another dialect.
                    _ => match b {
                        b'{' => {
                            self.start = self.at;
                            self.phase = Phase::BraceOpen;
                        }
                        b'}' => {
                            self.start = self.at;
                            self.phase = Phase::BraceClose;
                        }
                        _ => self.phase = Phase::Text,
                    },
                }
                break;
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

#[cfg(test)]
#[path = "verbs_py_tests.rs"]
mod py_dialect_tests;
