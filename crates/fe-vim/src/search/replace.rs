//! Replacements for a find bar's [`Pattern`]: a template parsed once
//! ([`Replacement`]) and expanded against a match ([`Pattern::replacement_at`],
//! [`replace_all`]).
//!
//! Semantics against `:s`: the find bar's pattern is the `regex` crate's
//! syntax with case from its Match Case toggle (off is case-insensitive
//! whatever the query holds, no smartcase), and its replacement uses `$n`
//! groups; `:s` keeps Vim's syntax (`\(\)`, `\1`, `&`, `\c`, 'ignorecase' off
//! by default). Both match within one row, and for equivalent patterns they
//! produce the same text.

use std::ops::Range;

use fe_text::Text;

use super::{Deadline, Engine, Interrupted, Pattern, RowMatches, row_text, time_limit};

/// A replacement template, parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replacement {
    parts: Vec<Part>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Literal(String),
    /// Group `n`; 0 is the whole match.
    Group(usize),
    Named(String),
}

impl Replacement {
    /// `template` as a replacement. With `regex` false it is literal text.
    /// With `regex` (VS Code's rules): `$0` and `$&` are the whole match,
    /// `$1`..`$99` a group (at most two digits, so `$1a` is group 1 then `a`;
    /// a group the pattern lacks expands to nothing), `${name}` a named
    /// group, `$$` a `$`, `\n` a line break, `\t` a tab, `\\` a backslash;
    /// anything else is literal.
    pub fn parse(template: &str, regex: bool) -> Self {
        if !regex {
            return Self {
                parts: vec![Part::Literal(template.to_string())],
            };
        }
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut chars = template.char_indices().peekable();
        while let Some((at, ch)) = chars.next() {
            let part = match (ch, chars.peek().map(|&(_, next)| next)) {
                ('$', Some('$')) => {
                    chars.next();
                    literal.push('$');
                    continue;
                }
                ('$', Some('&')) => {
                    chars.next();
                    Part::Group(0)
                }
                ('$', Some(digit)) if digit.is_ascii_digit() => {
                    let mut number = 0;
                    for _ in 0..2 {
                        match chars.peek() {
                            Some(&(_, digit)) if digit.is_ascii_digit() => {
                                number = number * 10 + digit as usize - '0' as usize;
                                chars.next();
                            }
                            _ => break,
                        }
                    }
                    Part::Group(number)
                }
                ('$', Some('{')) => {
                    let rest = &template[at + 2..];
                    match rest.find('}').filter(|&end| end > 0) {
                        Some(end) => {
                            let name = &rest[..end];
                            while chars.next_if(|&(i, _)| i < at + 2 + end + 1).is_some() {}
                            match name.parse::<usize>() {
                                Ok(number) => Part::Group(number),
                                Err(_) => Part::Named(name.to_string()),
                            }
                        }
                        None => {
                            literal.push('$');
                            continue;
                        }
                    }
                }
                ('\\', Some(escaped @ ('n' | 't' | '\\'))) => {
                    chars.next();
                    literal.push(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        _ => '\\',
                    });
                    continue;
                }
                _ => {
                    literal.push(ch);
                    continue;
                }
            };
            if !literal.is_empty() {
                parts.push(Part::Literal(std::mem::take(&mut literal)));
            }
            parts.push(part);
        }
        if !literal.is_empty() {
            parts.push(Part::Literal(literal));
        }
        Self { parts }
    }

    /// The text for the match `matches` has just found in `hay`.
    fn expand(&self, pattern: &Pattern, matches: &RowMatches<'_>, hay: &str) -> String {
        let mut out = String::new();
        for part in &self.parts {
            let group = match part {
                Part::Literal(text) => {
                    out.push_str(text);
                    continue;
                }
                Part::Group(number) => matches.group(*number),
                Part::Named(name) => match &pattern.engine {
                    Engine::Regex { regex, .. } => regex
                        .capture_names()
                        .position(|candidate| candidate == Some(name.as_str()))
                        .and_then(|number| matches.group(number)),
                    Engine::Backtrack(_) => None,
                },
            };
            if let Some(group) = group {
                out.push_str(&hay[group]);
            }
        }
        out
    }
}

impl Pattern {
    /// The row's matches with their groups, as [`super::matches_in`] finds them.
    fn grouped<'a>(&'a self, hay: &'a str, deadline: &'a Deadline) -> RowMatches<'a> {
        RowMatches {
            groups: true,
            ..self.matches(hay, deadline)
        }
    }

    /// The expansion of `with` if `range` is exactly a match this pattern
    /// finds ([`super::matches_in`] on its row, whole-word filter included),
    /// else `None`.
    pub fn replacement_at(
        &self,
        text: &Text,
        range: Range<usize>,
        with: &Replacement,
    ) -> Option<String> {
        if range.start > range.end || range.end > text.len() {
            return None;
        }
        let row = text.offset_to_point(range.start).row;
        let deadline = Deadline::new();
        let mut buf = String::new();
        let (start, hay) = row_text(text, row, &mut buf);
        if range.end > start + hay.len() {
            return None;
        }
        let mut matches = self.grouped(hay, &deadline);
        while let Some(found) = matches.next() {
            if start + found.start > range.start {
                break;
            }
            if start + found.start == range.start && start + found.end == range.end {
                return Some(with.expand(self, &matches, hay));
            }
        }
        None
    }
}

/// Every match in `text`, in order, with its expansion by `with`: row by row
/// exactly as [`super::matches_in`] finds them (empty matches once each).
/// Past [`time_limit`], counted from the call, `Err(Interrupted)`, never a
/// partial list.
pub fn replace_all(
    text: &Text,
    pattern: &Pattern,
    with: &Replacement,
) -> Result<Vec<(Range<usize>, String)>, Interrupted> {
    let deadline = Deadline::within(time_limit());
    let mut edits = Vec::new();
    let mut buf = String::new();
    for row in 0..text.row_count() {
        if deadline.expired() {
            return Err(Interrupted);
        }
        let (start, hay) = row_text(text, row, &mut buf);
        let mut matches = pattern.grouped(hay, &deadline);
        while let Some(found) = matches.next() {
            edits.push((
                start + found.start..start + found.end,
                with.expand(pattern, &matches, hay),
            ));
        }
        if deadline.expired() {
            return Err(Interrupted);
        }
    }
    Ok(edits)
}
