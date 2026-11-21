//! The literal prefilter a project scan runs before parsing a file: text
//! every match must contain, looked for in the file's bytes.

use memchr::memmem::Finder;

use crate::pattern::{Pattern, PatternNode};

/// The strings a file must contain for `pattern` to match anywhere in it,
/// longest first, without repeats: the text of every named leaf of the
/// pattern that isn't a wildcard or an extra. Empty for a pattern of only
/// metavariables (and anonymous tokens), which can't be prefiltered.
///
/// Named leaves only, though [`Pattern::literals`] holds anonymous tokens
/// too: a named leaf matches only a node with exactly its text, but an
/// anonymous token matches by kind alone, and a grammar can give one kind
/// several spellings (a case-insensitive keyword aliased to one name), so
/// requiring its text could skip a file that matches. Extras (a comment in
/// the pattern) are skipped when matching, so their text isn't required
/// either.
pub fn prefilter_literals(pattern: &Pattern) -> Vec<&str> {
    let mut out = Vec::new();
    collect(pattern.root(), &mut out);
    out.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    out.dedup();
    out
}

fn collect<'a>(node: &'a PatternNode, out: &mut Vec<&'a str>) {
    if node.wildcard.is_some() || node.extra {
        return;
    }
    if let Some(text) = node.text.as_deref() {
        if node.named && !text.is_empty() {
            out.push(text);
        }
        return;
    }
    for child in &node.children {
        collect(child, out);
    }
}

/// A pattern's literals compiled for searching many files.
pub(crate) struct Prefilter {
    finders: Vec<Finder<'static>>,
}

impl Prefilter {
    pub(crate) fn new(pattern: &Pattern) -> Prefilter {
        Prefilter {
            finders: prefilter_literals(pattern)
                .into_iter()
                .map(|literal| Finder::new(literal.as_bytes()).into_owned())
                .collect(),
        }
    }

    /// Whether there is anything to look for.
    pub(crate) fn is_empty(&self) -> bool {
        self.finders.is_empty()
    }

    /// Whether `bytes` holds every literal (always, with none).
    pub(crate) fn passes(&self, bytes: &[u8]) -> bool {
        self.finders
            .iter()
            .all(|finder| finder.find(bytes).is_some())
    }
}
