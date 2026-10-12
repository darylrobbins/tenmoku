//! Project-search replacements: the edits that replace every match `search`
//! would report in a file's text, with the find bar's template rules.

use std::ops::Range;

use grep_matcher::{Captures, Matcher};

use super::{LineSelector, SearchError, SearchQuery};

/// A piece of a parsed replacement template.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Literal(String),
    Group(usize),
    Named(String),
}

/// Turns a file's text into the edits that replace what [`super::search`]
/// finds in it.
pub struct Replacer {
    matcher: LineSelector,
    template: Vec<Part>,
}

impl Replacer {
    /// A replacer for `query` with `template`: literal text unless
    /// `query.regex`, where `$0` or `$&` (the match), `$1`..`$99`, `${name}`,
    /// `$$`, `\n`, `\t` and `\\` are expanded. A group that doesn't exist
    /// or didn't take part expands to nothing.
    pub fn new(query: &SearchQuery, template: &str) -> Result<Replacer, SearchError> {
        let matcher = query.matcher()?;
        let template = if query.regex {
            parse_template(template)
        } else {
            vec![Part::Literal(template.to_owned())]
        };
        Ok(Replacer { matcher, template })
    }

    /// Every match in `text`, as a byte range in it, in order, with its
    /// expansion. Matching is as in the search: line by line, never across
    /// `\n`, with a `\r` before `\n` not part of the line.
    pub fn edits(&self, text: &str) -> Vec<(Range<usize>, String)> {
        let mut edits = Vec::new();
        self.each(text, |range, line, caps| {
            edits.push((range, self.expand(line, caps)));
        });
        edits
    }

    /// How many matches [`Replacer::edits`] gives for `text`.
    pub fn count(&self, text: &str) -> usize {
        let mut count = 0;
        self.each(text, |_, _, _| count += 1);
        count
    }

    fn each(&self, text: &str, mut found: impl FnMut(Range<usize>, &[u8], &Caps)) {
        let mut caps = match self.matcher.line.new_captures() {
            Ok(caps) => caps,
            Err(_) => return,
        };
        let mut start = 0;
        while start < text.len() {
            let end = text[start..].find('\n').map_or(text.len(), |at| start + at);
            let next = (end + 1).min(text.len() + 1);
            let line = &text[start..end];
            let line = if end < text.len() {
                line.strip_suffix('\r').unwrap_or(line)
            } else {
                line
            };
            let base = start;
            let _ = self
                .matcher
                .line
                .captures_iter(line.as_bytes(), &mut caps, |caps| {
                    if let Some(whole) = caps.get(0) {
                        found(
                            base + whole.start()..base + whole.end(),
                            line.as_bytes(),
                            caps,
                        );
                    }
                    true
                });
            start = next;
        }
    }

    /// The expansion of the match `caps` found in `line`.
    fn expand(&self, line: &[u8], caps: &Caps) -> String {
        let mut out = String::new();
        for part in &self.template {
            let index = match part {
                Part::Literal(lit) => {
                    out.push_str(lit);
                    continue;
                }
                Part::Group(index) => Some(*index),
                Part::Named(name) => self.matcher.line.capture_index(name),
            };
            if let Some(range) = index.and_then(|i| caps.get(i)) {
                out.push_str(&String::from_utf8_lossy(&line[range.start()..range.end()]));
            }
        }
        out
    }
}

type Caps = grep_regex::RegexCaptures;

/// Parses `template` into literal runs and group references.
fn parse_template(template: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut chars = template.char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
        match ch {
            '\\' => match chars.peek().map(|&(_, c)| c) {
                Some('n') => {
                    lit.push('\n');
                    chars.next();
                }
                Some('t') => {
                    lit.push('\t');
                    chars.next();
                }
                Some('\\') => {
                    lit.push('\\');
                    chars.next();
                }
                _ => lit.push('\\'),
            },
            '$' => {
                let rest = &template[at + 1..];
                let group = if rest.starts_with('$') {
                    lit.push('$');
                    chars.next();
                    continue;
                } else if rest.starts_with('&') {
                    chars.next();
                    Part::Group(0)
                } else if let Some(body) = rest.strip_prefix('{') {
                    match body.find('}') {
                        Some(close) if close > 0 => {
                            let name = &body[..close];
                            for _ in 0..name.chars().count() + 2 {
                                chars.next();
                            }
                            match name.parse::<usize>() {
                                Ok(n) => Part::Group(n),
                                Err(_) => Part::Named(name.to_owned()),
                            }
                        }
                        _ => {
                            lit.push('$');
                            continue;
                        }
                    }
                } else {
                    let digits = rest.bytes().take(2).take_while(u8::is_ascii_digit).count();
                    if digits == 0 {
                        lit.push('$');
                        continue;
                    }
                    let n = rest[..digits].parse().unwrap_or(0);
                    for _ in 0..digits {
                        chars.next();
                    }
                    Part::Group(n)
                };
                if !lit.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut lit)));
                }
                parts.push(group);
            }
            _ => lit.push(ch),
        }
    }
    if !lit.is_empty() {
        parts.push(Part::Literal(lit));
    }
    parts
}
