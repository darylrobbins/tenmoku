//! A pattern's `where` section: what its metavariables may match, and where
//! a match may stand.
//!
//! The section starts after a line holding only `where` (spaces around it
//! allowed); the text before that line is the pattern's code. Each line
//! after it that isn't blank is one constraint, and a match must meet all
//! of them:
//!
//! - `$NAME: kind=<kind>`: the node is of that kind (by its name, as
//!   [`Node::kind`] gives it). For a multi metavariable (`$$$NAME`) each
//!   named node of the run is.
//! - `$NAME: regex=<re>`: the regular expression (the `regex` crate's
//!   syntax, unanchored) matches somewhere in the text it covers. The
//!   value runs to the end of the line, trailing spaces included.
//! - `$NAME: text=<text>`: the text it covers is exactly `<text>`, which
//!   runs to the end of the line, trailing spaces included.
//! - `not` before the key, `$NAME: not kind=<kind>`, inverts any of them.
//! - `inside: kind=<kind>`: some ancestor of the matched node is of that
//!   kind; with ` name=<text>` after it, one whose `name` field's text is
//!   `<text>` too (`inside: kind=function_item name=test`).
//! - `not inside: …`: no ancestor is.
//!
//! The metavariable must be a named one of the pattern: anonymous ones
//! (`$_`, `$$$`) can't be constrained. Metavariable constraints are
//! checked when the metavariable first binds, so they steer the
//! backtracking of a multi metavariable; `inside` once a node has matched.

use std::ops::Range;

use regex::Regex;
use tree_sitter::Node;

use crate::{
    error::{PatternError, PatternErrorKind},
    pattern::Metavar,
};

/// A pattern's compiled `where` section.
#[derive(Clone, Debug, Default)]
pub(crate) struct Constraints {
    vars: Vec<VarConstraint>,
    inside: Vec<Inside>,
}

#[derive(Clone, Debug)]
struct VarConstraint {
    name: Box<str>,
    test: Test,
    negated: bool,
}

#[derive(Clone, Debug)]
enum Test {
    Kind(Box<str>),
    Regex(Regex),
    Text(Box<str>),
}

#[derive(Clone, Debug)]
struct Inside {
    kind: Box<str>,
    name: Option<Box<str>>,
    negated: bool,
}

/// Splits a pattern's source at its `where` line: the code before it, and
/// the section after it with that section's offset in `source`.
pub(crate) fn split(source: &str) -> (&str, Option<(usize, &str)>) {
    let mut at = 0;
    for line in source.split_inclusive('\n') {
        if line.trim() == "where" {
            let after = at + line.len();
            return (&source[..at], Some((after, &source[after..])));
        }
        at += line.len();
    }
    (source, None)
}

/// Whether `s` is a metavariable name: `[A-Z_][A-Z0-9_]*`.
fn is_name(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_uppercase() || b == b'_')
        && bytes.all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

fn error(kind: PatternErrorKind, offset: usize) -> PatternError {
    PatternError { kind, offset }
}

/// Splits `not ` off the front of `text` (at `at`), if it is there.
fn negation(text: &str, at: usize) -> (bool, &str, usize) {
    match text.strip_prefix("not") {
        Some(rest) if rest.starts_with([' ', '\t']) => {
            let trimmed = rest.trim_start();
            (true, trimmed, at + text.len() - trimmed.len())
        }
        _ => (false, text, at),
    }
}

/// `key=value` at `at`: the key, the value and the value's offset.
fn key_value(text: &str, at: usize) -> Result<(&str, &str, usize), PatternError> {
    let Some(eq) = text.find('=') else {
        return Err(error(PatternErrorKind::UnknownConstraint, at));
    };
    let value = &text[eq + 1..];
    if value.is_empty() {
        return Err(error(PatternErrorKind::UnknownConstraint, at + eq + 1));
    }
    Ok((&text[..eq], value, at + eq + 1))
}

impl Constraints {
    /// Compiles the `where` section `section`, found at `offset` in the
    /// pattern source, for a pattern with `metavars`.
    pub(crate) fn parse(
        section: &str,
        offset: usize,
        metavars: &[Metavar],
    ) -> Result<Constraints, PatternError> {
        let mut out = Constraints::default();
        let mut line_start = offset;
        for raw in section.split_inclusive('\n') {
            let ended = raw.strip_suffix('\n').unwrap_or(raw);
            let ended = ended.strip_suffix('\r').unwrap_or(ended);
            // Indentation goes, but not trailing space: a `text=` or
            // `regex=` value runs to the line's end.
            let line = ended.trim_start();
            let at = line_start + (raw.len() - raw.trim_start().len());
            line_start += raw.len();
            if line.trim_end().is_empty() {
                continue;
            }
            let (negated, rest, rest_at) = negation(line, at);
            if let Some(body) = rest.strip_prefix("inside:") {
                let trimmed = body.trim_start();
                let body_at = rest_at + rest.len() - trimmed.len();
                out.inside.push(parse_inside(trimmed, body_at, negated)?);
                continue;
            }
            if negated {
                // `not` goes before the key, not before `$NAME`.
                return Err(error(PatternErrorKind::UnknownConstraint, at));
            }
            out.vars.push(parse_var(line, at, metavars)?);
        }
        Ok(out)
    }

    /// Whether binding `name` to `nodes` (one node, or a multi
    /// metavariable's run) covering `range` meets its constraints.
    pub(crate) fn allows(
        &self,
        name: &str,
        multi: bool,
        nodes: &[Node<'_>],
        range: &Range<usize>,
        source: &str,
    ) -> bool {
        self.vars
            .iter()
            .filter(|c| *c.name == *name)
            .all(|c| c.test.holds(multi, nodes, range, source) != c.negated)
    }

    /// Whether a match at `node` meets the `inside` constraints.
    pub(crate) fn allows_place(&self, node: Node<'_>, source: &str) -> bool {
        self.inside.iter().all(|inside| {
            let mut ancestor = node.parent();
            let mut found = false;
            while let Some(a) = ancestor {
                if inside.holds(a, source) {
                    found = true;
                    break;
                }
                ancestor = a.parent();
            }
            found != inside.negated
        })
    }

    /// Whether there are `inside` constraints to check.
    pub(crate) fn has_place(&self) -> bool {
        !self.inside.is_empty()
    }
}

fn parse_inside(body: &str, at: usize, negated: bool) -> Result<Inside, PatternError> {
    let mut kind = None;
    let mut name = None;
    let mut offset = at;
    for word in body.split_whitespace() {
        let word_at = at + (word.as_ptr() as usize - body.as_ptr() as usize);
        offset = word_at;
        let (key, value, _) = key_value(word, word_at)?;
        let slot = match key {
            "kind" => &mut kind,
            "name" => &mut name,
            _ => return Err(error(PatternErrorKind::UnknownConstraint, word_at)),
        };
        if slot.is_some() {
            return Err(error(PatternErrorKind::UnknownConstraint, word_at));
        }
        *slot = Some(Box::<str>::from(value));
    }
    let Some(kind) = kind else {
        // `inside:` needs a kind.
        return Err(error(PatternErrorKind::UnknownConstraint, offset));
    };
    Ok(Inside {
        kind,
        name,
        negated,
    })
}

fn parse_var(line: &str, at: usize, metavars: &[Metavar]) -> Result<VarConstraint, PatternError> {
    let Some(colon) = line.find(':') else {
        return Err(error(PatternErrorKind::UnknownConstraint, at));
    };
    let subject = line[..colon].trim_end();
    let name = subject
        .strip_prefix("$$$")
        .or_else(|| subject.strip_prefix('$'))
        .filter(|name| is_name(name));
    let Some(name) = name else {
        return Err(error(PatternErrorKind::UnknownConstraint, at));
    };
    if !metavars.iter().any(|m| !m.anonymous && m.name == name) {
        return Err(error(PatternErrorKind::UnboundMetavar, at));
    }
    let rest = &line[colon + 1..];
    let trimmed = rest.trim_start();
    let rest_at = at + colon + 1 + (rest.len() - trimmed.len());
    let (negated, clause, clause_at) = negation(trimmed, rest_at);
    let (key, value, value_at) = key_value(clause, clause_at)?;
    let test = match key {
        "kind" => match value.trim_end() {
            "" => return Err(error(PatternErrorKind::UnknownConstraint, value_at)),
            kind => Test::Kind(kind.into()),
        },
        "text" => Test::Text(value.into()),
        "regex" => Test::Regex(
            Regex::new(value).map_err(|_| error(PatternErrorKind::InvalidRegex, value_at))?,
        ),
        _ => return Err(error(PatternErrorKind::UnknownConstraint, clause_at)),
    };
    Ok(VarConstraint {
        name: name.into(),
        test,
        negated,
    })
}

impl Test {
    fn holds(&self, multi: bool, nodes: &[Node<'_>], range: &Range<usize>, source: &str) -> bool {
        match self {
            Test::Kind(kind) => {
                let mut shown = nodes.iter().filter(|n| !multi || n.is_named());
                shown.all(|n| n.kind() == &**kind)
            }
            Test::Regex(re) => source.get(range.clone()).is_some_and(|t| re.is_match(t)),
            Test::Text(text) => source.get(range.clone()) == Some(&**text),
        }
    }
}

impl Inside {
    fn holds(&self, node: Node<'_>, source: &str) -> bool {
        if node.kind() != &*self.kind {
            return false;
        }
        match &self.name {
            None => true,
            Some(name) => node
                .child_by_field_name("name")
                .is_some_and(|n| source.get(n.byte_range()) == Some(&**name)),
        }
    }
}
