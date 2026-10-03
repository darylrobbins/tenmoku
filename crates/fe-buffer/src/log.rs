use std::sync::Arc;

use fe_text::Bias;
use sum_tree::{ContextLessSummary, Item, SumTree};

use crate::{Edit, Version, edit::map_offset};

/// Every batch of edits ever applied to a buffer, in order. Entry `n` takes
/// the buffer from version `n` to `n + 1`. Backed by a persistent tree, so
/// cloning it into a snapshot is O(1).
#[derive(Clone)]
pub(crate) struct EditLog {
    entries: SumTree<LogEntry>,
}

#[derive(Clone)]
struct LogEntry {
    edits: Arc<[Edit]>,
    /// For undo and redo batches, the entry this one exactly reverses.
    inverts: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Count(usize);

impl Item for LogEntry {
    type Summary = Count;

    fn summary(&self, (): ()) -> Count {
        Count(1)
    }
}

impl ContextLessSummary for Count {
    fn zero() -> Self {
        Count(0)
    }

    fn add_summary(&mut self, summary: &Self) {
        self.0 += summary.0;
    }
}

impl EditLog {
    pub(crate) fn new() -> Self {
        Self {
            entries: SumTree::new(()),
        }
    }

    pub(crate) fn version(&self) -> Version {
        Version(self.entries.summary().0)
    }

    /// Appends a batch. `inverts` names the version that produced the batch
    /// this one reverses, for undo and redo.
    pub(crate) fn push(&mut self, edits: Arc<[Edit]>, inverts: Option<Version>) {
        let inverts = inverts.map(|version| version.0 - 1);
        self.entries.push(LogEntry { edits, inverts }, ());
    }

    fn entries(&self, from: Version, to: Version) -> impl Iterator<Item = (usize, &LogEntry)> {
        debug_assert!(from <= to && to <= self.version());
        let mut cursor = self.entries.cursor::<Count>(());
        cursor.seek(&Count(from.0), sum_tree::Bias::Right);
        std::iter::from_fn(move || {
            let index = cursor.start().0;
            if index >= to.0 {
                return None;
            }
            let entry = cursor.item()?;
            cursor.next();
            Some((index, entry))
        })
    }

    /// The edit batches that take the buffer from version `from` to `to`.
    pub(crate) fn batches(&self, from: Version, to: Version) -> impl Iterator<Item = &Arc<[Edit]>> {
        self.entries(from, to).map(|(_, entry)| &entry.edits)
    }

    /// The batches to map positions through from version `from` to `to`.
    ///
    /// A batch immediately followed by its own undo (once nested pairs are
    /// removed) leaves the text exactly as it was, so the pair is skipped.
    /// Mapping through it instead would lose which side of the batch's
    /// boundaries a position was on.
    pub(crate) fn mapping_batches(&self, from: Version, to: Version) -> Vec<&Arc<[Edit]>> {
        let mut stack: Vec<(usize, &LogEntry)> = Vec::new();
        for (index, entry) in self.entries(from, to) {
            if entry.inverts.is_some() && stack.last().map(|(top, _)| *top) == entry.inverts {
                stack.pop();
            } else {
                stack.push((index, entry));
            }
        }
        stack.into_iter().map(|(_, entry)| &entry.edits).collect()
    }

    /// Maps an offset at version `from` to the equivalent offset at version `to`.
    pub(crate) fn map(&self, offset: usize, bias: Bias, from: Version, to: Version) -> usize {
        self.mapping_batches(from, to)
            .into_iter()
            .fold(offset, |offset, edits| map_offset(edits, offset, bias))
    }
}
