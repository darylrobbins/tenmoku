use std::sync::Arc;

use sum_tree::{ContextLessSummary, Item, SumTree};

use crate::{
    Edit, Version,
    edit::{Mapping, collapses, map_offset},
};

/// Every batch of edits applied to a buffer since its floor, in order. Entry
/// `n` takes the buffer from version `n` to `n + 1`. Backed by a persistent
/// tree, so cloning it into a snapshot is O(1).
///
/// Entries below the floor have been compacted away (see
/// [`EditLog::compact`]); positions can only be mapped from versions at or
/// above it.
#[derive(Clone)]
pub(crate) struct EditLog {
    entries: SumTree<LogEntry>,
    /// The index of the first entry kept.
    floor: usize,
}

#[derive(Clone)]
struct LogEntry {
    edits: Arc<[Edit]>,
    /// For undo and redo batches, the entry this one exactly reverses.
    inverts: Option<usize>,
    /// The original edit this entry derives from through undo and redo, and
    /// whether it re-applies (`true`) or reverses (`false`) it. An original
    /// edit is its own root, applied forward.
    root: Collapse,
}

/// An edit that collapsed a position, identified by its root edit and
/// direction so that reverting it can be recognised through any chain of
/// undos and redos.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Collapse {
    root: usize,
    forward: bool,
}

impl Collapse {
    fn reverted_by(self, other: Collapse) -> bool {
        self.root == other.root && self.forward != other.forward
    }
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
            floor: 0,
        }
    }

    pub(crate) fn version(&self) -> Version {
        Version(self.floor + self.entries.summary().0)
    }

    /// The oldest version positions can still be mapped from.
    pub(crate) fn floor(&self) -> Version {
        Version(self.floor)
    }

    /// Drops the entries below `floor` (clamped to the current version).
    /// Versions keep their numbers. Snapshots cloned earlier keep their own
    /// entries.
    pub(crate) fn compact(&mut self, floor: Version) {
        let floor = floor.min(self.version()).0;
        if floor <= self.floor {
            return;
        }
        let kept = {
            let mut cursor = self.entries.cursor::<Count>(());
            cursor.seek(&Count(floor - self.floor), sum_tree::Bias::Right);
            cursor.suffix()
        };
        self.entries = kept;
        self.floor = floor;
    }

    /// Appends a batch. `inverts` names the version that produced the batch
    /// this one reverses, for undo and redo.
    pub(crate) fn push(&mut self, edits: Arc<[Edit]>, inverts: Option<Version>) {
        let inverts = inverts.map(|version| version.0 - 1);
        let root = match inverts {
            Some(inverted) => {
                let inverted = self.entry(inverted).root;
                Collapse {
                    root: inverted.root,
                    forward: !inverted.forward,
                }
            }
            None => Collapse {
                root: self.version().0,
                forward: true,
            },
        };
        self.entries.push(
            LogEntry {
                edits,
                inverts,
                root,
            },
            (),
        );
    }

    /// The entry at `index`, which must be at or above the floor.
    fn entry(&self, index: usize) -> &LogEntry {
        let (_, _, entry) =
            self.entries
                .find::<Count, _>((), &Count(index - self.floor), sum_tree::Bias::Right);
        entry.expect("log entry exists")
    }

    fn entries(&self, from: Version, to: Version) -> impl Iterator<Item = (usize, &LogEntry)> {
        assert!(
            from.0 >= self.floor,
            "{from:?} is below the edit log's floor {:?}",
            self.floor()
        );
        debug_assert!(from <= to && to <= self.version());
        let floor = self.floor;
        let mut cursor = self.entries.cursor::<Count>(());
        cursor.seek(&Count(from.0 - floor), sum_tree::Bias::Right);
        std::iter::from_fn(move || {
            let index = floor + cursor.start().0;
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
        self.mapping_entries(from, to)
            .into_iter()
            .map(|(_, entry)| &entry.edits)
            .collect()
    }

    fn mapping_entries(&self, from: Version, to: Version) -> Vec<(usize, &LogEntry)> {
        let mut stack: Vec<(usize, &LogEntry)> = Vec::new();
        for (index, entry) in self.entries(from, to) {
            if entry.inverts.is_some() && stack.last().map(|(top, _)| *top) == entry.inverts {
                stack.pop();
            } else {
                stack.push((index, entry));
            }
        }
        stack
    }

    /// Maps a boundary of text that undo or redo is about to change, like
    /// [`EditLog::map`].
    ///
    /// Returns `None` when the answer is unknowable:
    /// - another undo or redo has since put text back exactly at the
    ///   boundary: the two pieces of text may once have sat side by side, and
    ///   offsets alone can't say in which order (text typed fresh at the spot
    ///   never sat beside them, so either side is fine for it); or
    /// - an edit swallowed the boundary into text it replaced, collapsing it
    ///   to one end, and that edit has since been reverted: the replaced text
    ///   is back, but where in it the boundary was is lost; or
    /// - the batch that produced `from` is below the floor, so the batch
    ///   reverting it couldn't be linked to it; or
    /// - a collapse root the boundary depends on points below the floor: the
    ///   root of the batch that produced `from`, of an entry mapped through,
    ///   or of a collapse in `collapsed_by`. What that root's chain of undos
    ///   and redos did below the floor is gone. (An entry's `inverts` is
    ///   never below its root, so this covers inversions too.)
    ///
    /// `collapsed_by` starts with collapses the boundary inherited (it may
    /// come from an undo or redo computed through one) and gains any found
    /// here, to pass on to the batch being produced.
    pub(crate) fn map_for_history(
        &self,
        offset: usize,
        mapping: Mapping,
        from: Version,
        to: Version,
        collapsed_by: &mut Vec<Collapse>,
    ) -> Option<usize> {
        let below_floor = |collapse: &Collapse| collapse.root < self.floor;
        if from.0 <= self.floor
            || below_floor(&self.entry(from.0 - 1).root)
            || collapsed_by.iter().any(below_floor)
        {
            return None;
        }
        let mut offset = offset;
        for (_, entry) in self.mapping_entries(from, to) {
            if below_floor(&entry.root) {
                return None;
            }
            if collapsed_by
                .iter()
                .any(|collapse| collapse.reverted_by(entry.root))
            {
                return None;
            }
            if collapses(&entry.edits, offset) {
                collapsed_by.push(entry.root);
            }
            let reinserted_here = entry.inverts.is_some()
                && entry.edits.iter().any(|edit| {
                    edit.old.is_empty() && edit.old.start == offset && !edit.new.is_empty()
                });
            if reinserted_here {
                return None;
            }
            offset = map_offset(&entry.edits, offset, mapping);
        }
        Some(offset)
    }

    /// Maps an offset at version `from` to the equivalent offset at version `to`.
    pub(crate) fn map(&self, offset: usize, mapping: Mapping, from: Version, to: Version) -> usize {
        self.mapping_batches(from, to)
            .into_iter()
            .fold(offset, |offset, edits| map_offset(edits, offset, mapping))
    }
}
