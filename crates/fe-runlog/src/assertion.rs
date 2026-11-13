//! The `assert_eq!` or `assert_ne!` a test's output says failed.
//!
//! Rust 1.73 and later write:
//!
//! ```text
//! thread 'a::b' panicked at src/lib.rs:7:5:
//! assertion `left == right` failed: the message, when there is one
//!   left: [1, 2]
//!  right: [1, 3]
//! ```
//!
//! Earlier versions wrote the values in backticks, inside the quoted panic
//! message:
//!
//! ```text
//! thread 'a::b' panicked at 'assertion failed: `(left == right)`
//!   left: `[1, 2]`,
//!  right: `[1, 3]`: the message, when there is one', src/lib.rs:7:5
//! ```
//!
//! A value's `Debug` can span lines, and so can the message: each part runs
//! to the next part's label, and the right value to the end of the panic
//! (the backtrace note, or the end of the output).

/// The comparison a failed assertion made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssertOp {
    /// `assert_eq!`: the values should have been equal.
    Eq,
    /// `assert_ne!`: the values should have differed.
    Ne,
}

impl AssertOp {
    /// The operator as the panic writes it: `==` or `!=`.
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::Ne => "!=",
        }
    }

    fn from_symbol(symbol: &str) -> Option<Self> {
        match symbol {
            "==" => Some(Self::Eq),
            "!=" => Some(Self::Ne),
            _ => None,
        }
    }
}

/// A failed `assert_eq!` or `assert_ne!` (or their `debug_` forms), read from
/// a failed test's output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assertion {
    /// Which assertion it was.
    pub op: AssertOp,
    /// The left value's `Debug`, as written (possibly several lines).
    pub left: String,
    /// The right value's `Debug`, as written.
    pub right: String,
    /// The assertion's own message, when it had one.
    pub message: Option<String>,
}

impl Assertion {
    /// The first failed `assert_eq!` or `assert_ne!` in `output`, in either
    /// form the module docs show; `None` when the test failed some other way
    /// (another panic, an `Err`, a crash).
    pub fn parse(output: &str) -> Option<Self> {
        line_starts(output).find_map(|at| {
            let line = &output[at..];
            if let Some(rest) = line.strip_prefix(CURRENT) {
                parse_current(rest)
            } else {
                // Legacy: `thread 'x' panicked at 'assertion failed: …`.
                let line_end = line.find('\n').unwrap_or(line.len());
                let start = line[..line_end].find(LEGACY)?;
                parse_legacy(&line[start + LEGACY.len()..])
            }
        })
    }
}

const CURRENT: &str = "assertion `left ";
const LEGACY: &str = "assertion failed: `(left ";
const LEFT: &str = "\n  left: ";
const RIGHT: &str = "\n right: ";

/// The byte offsets at which `text`'s lines start.
fn line_starts(text: &str) -> impl Iterator<Item = usize> + '_ {
    std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1))
}

/// `rest` follows `assertion \`left `.
fn parse_current(rest: &str) -> Option<Assertion> {
    let (symbol, rest) = rest.split_once(" right` failed")?;
    let op = AssertOp::from_symbol(symbol)?;
    let (message, rest) = if let Some(rest) = rest.strip_prefix(": ") {
        let end = rest.find(LEFT)?;
        (Some(rest[..end].to_owned()), &rest[end..])
    } else {
        (None, rest)
    };
    let rest = rest.strip_prefix(LEFT)?;
    let (left, rest) = rest.split_once(RIGHT)?;
    let right = &rest[..panic_end(rest)];
    Some(Assertion {
        op,
        left: left.to_owned(),
        right: right.to_owned(),
        message,
    })
}

/// `rest` follows `assertion failed: \`(left `.
fn parse_legacy(rest: &str) -> Option<Assertion> {
    let (symbol, rest) = rest.split_once(" right)`")?;
    let op = AssertOp::from_symbol(symbol)?;
    let rest = rest.strip_prefix("\n  left: `")?;
    let (left, rest) = rest.split_once("`,\n right: `")?;
    // The panic message was quoted: it ends at `', src/lib.rs:7:5`.
    let end = legacy_location(rest).unwrap_or_else(|| panic_end(rest));
    let tail = &rest[..end];
    let (right, message) = match tail.split_once("`: ") {
        Some((right, message)) => (right, Some(message.to_owned())),
        None => (tail.strip_suffix('`').unwrap_or(tail), None),
    };
    Some(Assertion {
        op,
        left: left.to_owned(),
        right: right.to_owned(),
        message,
    })
}

/// Where the legacy quoted panic message in `text` ends: at the `', ` before
/// the location that ends its last line.
fn legacy_location(text: &str) -> Option<usize> {
    line_starts(text).find_map(|start| {
        let line = &text[start..];
        let line = &line[..line.find('\n').unwrap_or(line.len())];
        let quote = line.rfind("', ")?;
        is_location(&line[quote + 3..]).then_some(start + quote)
    })
}

/// Whether `text` is a panic location, `path:line:column`.
fn is_location(text: &str) -> bool {
    let mut parts = text.rsplitn(3, ':');
    let column = parts.next().unwrap_or_default();
    let line = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    !path.is_empty() && digits(line) && digits(column)
}

/// The length of the panic message that starts `text`: up to the backtrace
/// note or backtrace that follows it, or the end, less the line endings
/// before that.
fn panic_end(text: &str) -> usize {
    let end = line_starts(text)
        .skip(1)
        .find(|&at| {
            let line = &text[at..];
            line.starts_with("note: run with `RUST_BACKTRACE=")
                || line.starts_with("stack backtrace:")
                || line.starts_with("note: Some details are omitted")
        })
        .unwrap_or(text.len());
    text[..end].trim_end_matches(['\n', '\r']).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_are_recognised() {
        assert!(is_location("src/lib.rs:7:5"));
        assert!(is_location(r"C:\x\lib.rs:7:5"));
        assert!(!is_location("src/lib.rs:7"));
        assert!(!is_location("a message"));
    }
}
