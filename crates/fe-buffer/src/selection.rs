use std::ops::Range;

use fe_text::Bias;

use crate::{Anchor, BufferSnapshot};

/// A selection or caret, anchored so it follows edits made elsewhere.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: Anchor,
    pub end: Anchor,
    /// Whether the head (the end that moves) is at `start`.
    pub reversed: bool,
}

/// A selection resolved to offsets in one snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSelection {
    pub range: Range<usize>,
    pub reversed: bool,
}

impl ResolvedSelection {
    pub fn head(&self) -> usize {
        if self.reversed {
            self.range.start
        } else {
            self.range.end
        }
    }

    pub fn tail(&self) -> usize {
        if self.reversed {
            self.range.end
        } else {
            self.range.start
        }
    }
}

/// One or more selections, kept sorted and non-overlapping, with the
/// primary one: a tracked one, or the last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionSet {
    selections: Vec<Selection>,
    /// The tracked primary's index in `selections`; `None` makes the last
    /// one primary.
    primary: Option<usize>,
}

impl SelectionSet {
    /// A single caret at `offset`.
    pub fn caret(snapshot: &BufferSnapshot, offset: usize) -> Self {
        Self::from_resolved(
            snapshot,
            [ResolvedSelection {
                range: offset..offset,
                reversed: false,
            }],
        )
    }

    /// Clips `selections` to the text, then sorts and merges them, so carets
    /// that clip together are one and a range clipped to empty is a caret. Non-empty selections don't grow when
    /// text is inserted at their edges; carets stay before text inserted at them.
    pub fn from_resolved(
        snapshot: &BufferSnapshot,
        selections: impl IntoIterator<Item = ResolvedSelection>,
    ) -> Self {
        Self::build(snapshot, selections, None)
    }

    /// [`SelectionSet::from_resolved`], tracking the `primary`th of
    /// `selections` as the primary one, or the one it merges into.
    pub fn from_resolved_with_primary(
        snapshot: &BufferSnapshot,
        selections: impl IntoIterator<Item = ResolvedSelection>,
        primary: usize,
    ) -> Self {
        Self::build(snapshot, selections, Some(primary))
    }

    fn build(
        snapshot: &BufferSnapshot,
        selections: impl IntoIterator<Item = ResolvedSelection>,
        primary: Option<usize>,
    ) -> Self {
        let text = snapshot.text();
        let clipped = selections
            .into_iter()
            .map(|selection| {
                let range = selection.range;
                let range = if range.is_empty() {
                    let caret = text.clip_offset(range.start, Bias::Left);
                    caret..caret
                } else {
                    let start = text.clip_offset(range.start, Bias::Right);
                    let end = text.clip_offset(range.end, Bias::Left);
                    start..end.max(start)
                };
                ResolvedSelection { range, ..selection }
            })
            .collect();
        let (merged, primary) = merge_tracking(clipped, primary);
        let selections = merged
            .into_iter()
            .map(|selection| {
                let range = selection.range;
                let (start, end) = if range.is_empty() {
                    let caret = snapshot.anchor_at(range.start, Bias::Left);
                    (caret, caret)
                } else {
                    (
                        snapshot.anchor_at(range.start, Bias::Right),
                        snapshot.anchor_at(range.end, Bias::Left),
                    )
                };
                Selection {
                    start,
                    end,
                    reversed: selection.reversed,
                }
            })
            .collect();
        Self {
            selections,
            primary,
        }
    }

    pub fn len(&self) -> usize {
        self.selections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.selections.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Selection> {
        self.selections.iter()
    }

    /// The selections in `snapshot`, sorted, with any that edits made overlap merged.
    pub fn resolve(&self, snapshot: &BufferSnapshot) -> Vec<ResolvedSelection> {
        self.resolve_tracking(snapshot).0
    }

    /// The primary selection in `snapshot`: the tracked one (or the one it
    /// merged into), else the last. `None` with no selections.
    pub fn primary_in(&self, snapshot: &BufferSnapshot) -> Option<ResolvedSelection> {
        let (mut selections, primary) = self.resolve_tracking(snapshot);
        match primary {
            Some(ix) => Some(selections.swap_remove(ix)),
            None => selections.pop(),
        }
    }

    /// The tracked primary's index in [`SelectionSet::resolve`]'s
    /// selections; `None` if the last is primary for want of one.
    pub fn primary_index_in(&self, snapshot: &BufferSnapshot) -> Option<usize> {
        self.primary?;
        self.resolve_tracking(snapshot).1
    }

    fn resolve_tracking(
        &self,
        snapshot: &BufferSnapshot,
    ) -> (Vec<ResolvedSelection>, Option<usize>) {
        merge_tracking(
            self.selections
                .iter()
                .map(|selection| ResolvedSelection {
                    range: snapshot.resolve_range(&(selection.start..selection.end)),
                    reversed: selection.reversed,
                })
                .collect(),
            self.primary,
        )
    }

    /// Rebases the anchors onto `snapshot` so later resolves are cheap.
    pub fn refresh(&mut self, snapshot: &BufferSnapshot) {
        let (selections, primary) = self.resolve_tracking(snapshot);
        *self = Self::build(snapshot, selections, primary);
    }
}

/// Sorts `selections` and merges overlapping ones, with where the
/// `primary`th of them ends up.
fn merge_tracking(
    selections: Vec<ResolvedSelection>,
    primary: Option<usize>,
) -> (Vec<ResolvedSelection>, Option<usize>) {
    let mut indexed: Vec<_> = selections.into_iter().enumerate().collect();
    indexed.sort_by_key(|(_, selection)| (selection.range.start, selection.range.end));
    let mut merged: Vec<ResolvedSelection> = Vec::with_capacity(indexed.len());
    let mut merged_primary = None;
    for (ix, selection) in indexed {
        let is_primary = primary == Some(ix);
        if let Some(last) = merged.last_mut() {
            let overlaps = selection.range.start < last.range.end;
            let same_caret = selection.range == last.range;
            if overlaps || same_caret {
                last.range.end = last.range.end.max(selection.range.end);
                if is_primary {
                    merged_primary = Some(merged.len() - 1);
                }
                continue;
            }
        }
        merged.push(selection);
        if is_primary {
            merged_primary = Some(merged.len() - 1);
        }
    }
    (merged, merged_primary)
}
