//! Problems found in a config file, with where they are.

use std::{
    fmt::{self, Write as _},
    path::PathBuf,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// The file's layer was rejected and its last good version kept.
    Error,
    /// The rest of the file still applies.
    Warning,
}

/// A problem in a config file, such as a syntax error, a value of the wrong
/// type or out of range, or an unknown key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub file: PathBuf,
    /// The dotted key path, such as `editor.font_size`, when the problem is
    /// with one key.
    pub key: Option<String>,
    /// 1-based line of the offending token.
    pub line: usize,
    /// 1-based column of the offending token, in chars.
    pub column: usize,
    pub message: String,
}

impl Diagnostic {
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// `file:line:column: error: key: message`, as compilers print them.
/// Control characters are escaped: keys and messages come from files that
/// may be untrusted, and are printed to terminals.
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let file = self.file.display().to_string();
        write!(
            f,
            "{}:{}:{}: {severity}: ",
            Escaped(&file),
            self.line,
            self.column
        )?;
        if let Some(key) = &self.key {
            write!(f, "{}: ", Escaped(key))?;
        }
        write!(f, "{}", Escaped(&self.message))
    }
}

/// Text with its control characters escaped, as `\u{1b}` and so on.
struct Escaped<'a>(&'a str);

impl fmt::Display for Escaped<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for ch in self.0.chars() {
            if ch.is_control() {
                write!(f, "{}", ch.escape_default())?;
            } else {
                f.write_char(ch)?;
            }
        }
        Ok(())
    }
}

/// The 1-based line and column (in chars) of byte `offset` in `source`.
pub(crate) fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &source[..offset];
    let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    let line = before.matches('\n').count() + 1;
    let column = before[line_start..].chars().count() + 1;
    (line, column)
}
