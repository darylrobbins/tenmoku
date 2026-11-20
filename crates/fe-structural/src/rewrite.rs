//! Rewriting matches: a template with the pattern's metavariables in it,
//! turned into one edit per match, and a check that the edits keep the file
//! parsing.
//!
//! A template is text with `$NAME` and `$$$NAME` standing for what the
//! pattern's metavariable of that name covered, and `$$` for a literal `$`
//! (unless it starts `$$$NAME`, so `$$$$` is `$$`).
//! Names follow the pattern's rules (`[A-Z_][A-Z0-9_]*`); a `$` that starts
//! none, as in `$x` or `$Abc`, is literal text. A capture is substituted
//! with its source text: a multi one's from the start of its first sibling
//! to the end of its last, separators included, or nothing for an empty
//! run.
//!
//! The template's literal text is kept exactly as written, except that its
//! line endings become `\n` (`\r\n` and a lone `\r` alike), as a buffer's
//! are.
//!
//! Indentation: a capture spanning several lines has its own base indent
//! (the leading whitespace of the line it starts on in the file) taken off
//! each of its later lines, and the indent of the line it lands on in the
//! output put on instead: on the template's first line that is the match's
//! start line's indent, on a later one the whitespace that template line
//! starts with. Lines left empty get no indent.

use std::ops::Range;

use tree_sitter::{Node, Tree};

use crate::{
    matcher::{MatchOptions, Stopped},
    pattern::Pattern,
};

/// What went wrong compiling a [`Rewrite`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RewriteErrorKind {
    /// The template uses a name the pattern has no metavariable for.
    UnknownMetavar,
    /// The template uses an anonymous metavariable (`$_`, `$_NAME` or
    /// `$$$_NAME`), which captures nothing. (`$$$` alone is `$$`, a `$`,
    /// and then a `$`.)
    AnonymousMetavar,
    /// The template writes `$NAME` where the pattern has `$$$NAME`, or the
    /// other way around.
    MixedMetavar,
}

/// Why [`Pattern::edits`] gave no edits.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EditsError {
    /// The search or the reparse check stopped.
    #[error(transparent)]
    Stopped(#[from] Stopped),
    /// The rewrite was compiled for another pattern, and this one doesn't
    /// bind one of its metavariables the same way: the error is the one
    /// [`Rewrite::compile`] would give for this pattern.
    #[error("rewrite does not fit the pattern: {0}")]
    WrongPattern(RewriteError),
}

/// A template that didn't compile: what went wrong, and the byte offset of
/// the metavariable's `$` in the template.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{} at byte {offset}", describe(*kind))]
pub struct RewriteError {
    pub kind: RewriteErrorKind,
    pub offset: usize,
}

fn describe(kind: RewriteErrorKind) -> &'static str {
    match kind {
        RewriteErrorKind::UnknownMetavar => "metavariable the pattern does not bind",
        RewriteErrorKind::AnonymousMetavar => "anonymous metavariable captures nothing",
        RewriteErrorKind::MixedMetavar => {
            "metavariable used as $NAME and $$$NAME in the pattern and the template"
        }
    }
}

/// A piece of a compiled template.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    /// Text as it is, `$$` already a `$`.
    Literal(String),
    /// What the named metavariable covered.
    Capture {
        name: String,
        multi: bool,
        /// Its `$` in the template.
        offset: usize,
    },
}

/// A rewrite template compiled against a [`Pattern`]: literal text and the
/// captures to put between it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rewrite {
    template: Box<str>,
    parts: Vec<Part>,
}

impl Rewrite {
    /// Compiles `template` for matches of `pattern`. Every `$NAME` and
    /// `$$$NAME` in it must be a named metavariable of the pattern, written
    /// the same way (single or multi).
    pub fn compile(pattern: &Pattern, template: &str) -> Result<Rewrite, RewriteError> {
        let bytes = template.as_bytes();
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut copied = 0;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'$' {
                i += 1;
                continue;
            }
            let start = i;
            // `$$$` with no name after it is `$$` and then a `$`: an
            // anonymous multi captures nothing, so `$$$$` is two dollars.
            let multi = bytes[i..].starts_with(b"$$$")
                && bytes.get(i + 3).is_some_and(|&b| is_name_start(b));
            if !multi && bytes[i..].starts_with(b"$$") {
                push_literal(&mut literal, &template[copied..start]);
                literal.push('$');
                i += 2;
                copied = i;
                continue;
            }
            let name_start = start + if multi { 3 } else { 1 };
            let mut end = name_start;
            if end < bytes.len() && is_name_start(bytes[end]) {
                end += 1;
                while end < bytes.len() && is_name_byte(bytes[end]) {
                    end += 1;
                }
            }
            let runs_on =
                end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] >= 0x80);
            if (end == name_start && !multi) || (end > name_start && runs_on) {
                // A lone `$`, or `$Abc`: literal text, as in a pattern.
                i = if multi { start + 3 } else { start + 1 };
                continue;
            }
            let name = &template[name_start..end];
            if name.is_empty() || name.starts_with('_') {
                return Err(RewriteError {
                    kind: RewriteErrorKind::AnonymousMetavar,
                    offset: start,
                });
            }
            fits(pattern, name, multi, start)?;
            push_literal(&mut literal, &template[copied..start]);
            if !literal.is_empty() {
                parts.push(Part::Literal(std::mem::take(&mut literal)));
            }
            parts.push(Part::Capture {
                name: name.to_owned(),
                multi,
                offset: start,
            });
            i = end;
            copied = end;
        }
        push_literal(&mut literal, &template[copied..]);
        if !literal.is_empty() {
            parts.push(Part::Literal(literal));
        }
        Ok(Rewrite {
            template: template.into(),
            parts,
        })
    }

    /// The template as the user wrote it.
    pub fn template(&self) -> &str {
        &self.template
    }

    /// Whether every capture is a metavariable `pattern` binds, the same
    /// way: the first that isn't, as [`Rewrite::compile`] would report it.
    fn fits(&self, pattern: &Pattern) -> Result<(), RewriteError> {
        for part in &self.parts {
            if let Part::Capture {
                name,
                multi,
                offset,
            } = part
            {
                fits(pattern, name, *multi, *offset)?;
            }
        }
        Ok(())
    }

    /// The text that replaces `matched` (a match's range) in `source`, with
    /// `captures` substituted and reindented.
    fn expand(&self, source: &str, matched: &Range<usize>, captures: &crate::Captures) -> String {
        let match_line = line_start(source, matched.start);
        // The output's current line as it will stand in the file, for the
        // indent a capture's later lines take.
        let mut out = String::new();
        let current_indent = |out: &str| -> String {
            match out.rfind('\n') {
                Some(at) => leading_whitespace(&out[at + 1..]).to_owned(),
                None => {
                    let line = format!("{}{out}", &source[match_line..matched.start]);
                    leading_whitespace(&line).to_owned()
                }
            }
        };
        for part in &self.parts {
            match part {
                Part::Literal(text) => out.push_str(text),
                Part::Capture { name, .. } => {
                    // `fits` checked the name is bound; every bound name of
                    // a match has a range.
                    let Some(range) = captures.get(name).and_then(<[_]>::first) else {
                        continue;
                    };
                    let text = &source[range.clone()];
                    if !text.contains('\n') {
                        out.push_str(text);
                        continue;
                    }
                    let base = leading_whitespace(&source[line_start(source, range.start)..]);
                    let target = current_indent(&out);
                    if base == target {
                        out.push_str(text);
                    } else {
                        push_indented(&mut out, text, &target, base);
                    }
                }
            }
        }
        out
    }
}

/// Pushes `text`, each line after its first with `strip` (or as much of it
/// as the line starts with) taken off and `indent` put on, unless nothing
/// is left of it.
fn push_indented(out: &mut String, text: &str, indent: &str, strip: &str) {
    let mut lines = text.split('\n');
    if let Some(first) = lines.next() {
        out.push_str(first);
    }
    for line in lines {
        out.push('\n');
        let common = line
            .bytes()
            .zip(strip.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let rest = &line[common..];
        if !rest.is_empty() {
            out.push_str(indent);
            out.push_str(rest);
        }
    }
}

/// Whether `pattern` has a named metavariable `name`, single or `multi` as
/// the template writes it at `offset`.
fn fits(pattern: &Pattern, name: &str, multi: bool, offset: usize) -> Result<(), RewriteError> {
    let error = |kind| RewriteError { kind, offset };
    let Some(metavar) = pattern
        .metavariables()
        .iter()
        .find(|m| !m.anonymous && m.name == name)
    else {
        return Err(error(RewriteErrorKind::UnknownMetavar));
    };
    if metavar.multi != multi {
        return Err(error(RewriteErrorKind::MixedMetavar));
    }
    Ok(())
}

/// Pushes template text onto `literal` with its line endings made `\n`.
fn push_literal(literal: &mut String, text: &str) {
    if text.contains('\r') {
        literal.push_str(&text.replace("\r\n", "\n").replace('\r', "\n"));
    } else {
        literal.push_str(text);
    }
}

fn line_start(source: &str, at: usize) -> usize {
    source[..at].rfind('\n').map_or(0, |nl| nl + 1)
}

/// The spaces and tabs `line` starts with.
fn leading_whitespace(line: &str) -> &str {
    let end = line
        .bytes()
        .position(|b| b != b' ' && b != b'\t')
        .unwrap_or(line.len());
    &line[..end]
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_uppercase() || b == b'_'
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'
}

/// One replacement: the match's bytes in the file and the text for them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    /// The matched bytes in the source.
    pub range: Range<usize>,
    /// What replaces them.
    pub text: String,
    /// The reparse check found that this edit alone adds syntax errors
    /// (ERROR or MISSING nodes) to the file. Returned all the same: the
    /// caller decides whether to skip it.
    pub flagged: bool,
}

impl Edit {
    /// The edit as `fe_project::Replacer::edits` gives one: the range and
    /// its replacement.
    pub fn into_pair(self) -> (Range<usize>, String) {
        (self.range, self.text)
    }
}

/// A file's edits from [`Pattern::edits`], in file order and never
/// overlapping, and whether the reparse check ran.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Edits {
    pub edits: Vec<Edit>,
    /// The reparse check ran: `false` for a file over [`CHECK_LIMIT`] bytes
    /// or one whose rewritten text couldn't be parsed, where no edit is
    /// flagged.
    pub checked: bool,
}

impl Edits {
    /// The edits as `fe_project::Replacer::edits` gives them.
    pub fn into_pairs(self) -> Vec<(Range<usize>, String)> {
        self.edits.into_iter().map(Edit::into_pair).collect()
    }
}

/// Files larger than this (1 MiB) skip the reparse check.
pub const CHECK_LIMIT: usize = 1 << 20;

/// `source` with `edits` (in order, not overlapping) applied.
pub fn apply_edits(source: &str, edits: &[Edit]) -> String {
    let mut out = String::with_capacity(source.len());
    let mut copied = 0;
    for edit in edits {
        out.push_str(&source[copied..edit.range.start]);
        out.push_str(&edit.text);
        copied = edit.range.end;
    }
    out.push_str(&source[copied..]);
    out
}

impl Pattern {
    /// The edits that rewrite each match in `tree`, a parse of `source` in
    /// the pattern's language, with `rewrite`. Matches are taken outermost
    /// first: one inside a match already taken is dropped, even with
    /// [`MatchOptions::nested`], so the edits never overlap. Captures are
    /// the file's text as it is: a match inside a capture isn't rewritten.
    ///
    /// Then the reparse check, for a file of up to [`CHECK_LIMIT`] bytes:
    /// the edits are applied to a copy, which is parsed. If it holds more
    /// ERROR and MISSING nodes than `tree`, each edit is applied alone and
    /// parsed, and those that add any are [flagged](Edit::flagged). `stop`
    /// is asked by the search, before every parse and during it; once it
    /// says `true` the call ends with [`Stopped::Cancelled`].
    ///
    /// A `rewrite` compiled for another pattern is checked against this
    /// one first: a metavariable it uses that this pattern doesn't bind the
    /// same way is [`EditsError::WrongPattern`], not an empty substitution.
    pub fn edits(
        &self,
        rewrite: &Rewrite,
        source: &str,
        tree: &Tree,
        opts: &MatchOptions,
        stop: &mut dyn FnMut() -> bool,
    ) -> Result<Edits, EditsError> {
        rewrite.fits(self).map_err(EditsError::WrongPattern)?;
        let found = self.find_all(source, tree, opts, stop)?;
        let mut edits: Vec<Edit> = Vec::with_capacity(found.len());
        for matched in &found {
            if let Some(last) = edits.last() {
                let inside = matched.range.start < last.range.end
                    || (matched.range.start == last.range.start && matched.range.is_empty());
                if inside {
                    continue;
                }
            }
            edits.push(Edit {
                range: matched.range.clone(),
                text: rewrite.expand(source, &matched.range, &matched.captures),
                flagged: false,
            });
        }
        let checked = self.check(source, tree, &mut edits, stop)?;
        Ok(Edits { edits, checked })
    }

    /// The reparse check: whether it ran.
    fn check(
        &self,
        source: &str,
        tree: &Tree,
        edits: &mut [Edit],
        stop: &mut dyn FnMut() -> bool,
    ) -> Result<bool, Stopped> {
        if source.len() > CHECK_LIMIT {
            return Ok(false);
        }
        if edits.is_empty() {
            return Ok(true);
        }
        let before = error_count(tree.root_node());
        let Some(after) = self.errors_in(&apply_edits(source, edits), stop)? else {
            return Ok(false);
        };
        if after <= before {
            return Ok(true);
        }
        for edit in edits.iter_mut() {
            let alone = apply_edits(source, std::slice::from_ref(edit));
            let Some(count) = self.errors_in(&alone, stop)? else {
                return Ok(false);
            };
            edit.flagged = count > before;
        }
        Ok(true)
    }

    /// How many ERROR and MISSING nodes a parse of `text` holds: `None` if
    /// it couldn't be parsed.
    fn errors_in(
        &self,
        text: &str,
        stop: &mut dyn FnMut() -> bool,
    ) -> Result<Option<usize>, Stopped> {
        // A short parse may never ask, so ask before each one.
        if stop() {
            return Err(Stopped::Cancelled);
        }
        let mut stopped = false;
        let tree = fe_syntax::parse_str(self.language(), text, || {
            stopped = stop();
            stopped
        });
        if stopped {
            return Err(Stopped::Cancelled);
        }
        Ok(tree.map(|tree| error_count(tree.root_node())))
    }
}

/// The ERROR and MISSING nodes under `root`, itself included.
fn error_count(root: Node<'_>) -> usize {
    if !root.has_error() {
        return 0;
    }
    let mut count = 0;
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if node.is_error() || node.is_missing() {
            count += 1;
        }
        // Only go down into a subtree that holds an error.
        if node.has_error() && cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return count;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_lines_lose_the_base_indent_and_gain_the_target() {
        let mut out = String::new();
        push_indented(&mut out, "{\n\t\ta();\n\n\t}", "    ", "\t");
        assert_eq!(out, "{\n    \ta();\n\n    }");
    }

    #[test]
    fn a_line_with_less_than_the_base_indent_loses_what_it_has() {
        let mut out = String::new();
        push_indented(&mut out, "x\n  y\nz", "\t", "    ");
        assert_eq!(out, "x\n\ty\n\tz");
    }
}
