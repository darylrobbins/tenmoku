//! What a match's metavariables stood for: byte ranges in the file, by name.

use std::ops::Range;

/// The byte ranges each named metavariable of a [`Match`](crate::Match)
/// covered in the file, by name, in order of first appearance in the
/// pattern. Each has one range: a single metavariable (`$A`) the node's, a
/// multi one (`$$$A`) from the start of the first sibling it took to the end
/// of the last, separators between them included, or an empty range where
/// the run would stand when it took none. Anonymous metavariables (`$_`,
/// `$$$`) capture nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Captures {
    entries: Vec<(Box<str>, Vec<Range<usize>>)>,
}

impl Captures {
    /// The ranges `name` (without its `$`s) covered, if it is a named
    /// metavariable of the pattern.
    pub fn get(&self, name: &str) -> Option<&[Range<usize>]> {
        self.entries
            .iter()
            .find(|(n, _)| &**n == name)
            .map(|(_, ranges)| ranges.as_slice())
    }

    /// The one range a metavariable covered: `None` if `name` didn't
    /// capture exactly one.
    pub fn single(&self, name: &str) -> Option<Range<usize>> {
        match self.get(name)? {
            [range] => Some(range.clone()),
            _ => None,
        }
    }

    /// Each name and its ranges, in order of first appearance in the pattern.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[Range<usize>])> {
        self.entries
            .iter()
            .map(|(name, ranges)| (&**name, ranges.as_slice()))
    }

    /// How many names captured something.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing was captured: a pattern with no named metavariable.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn push(&mut self, name: &str, ranges: Vec<Range<usize>>) {
        self.entries.push((name.into(), ranges));
    }
}
