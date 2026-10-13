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

/// One or more selections, kept sorted and non-overlapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionSet {
    selections: Vec<Selection>,
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
        let selections = merge(clipped)
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
        Self { selections }
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
        merge(
            self.selections
                .iter()
                .map(|selection| ResolvedSelection {
                    range: snapshot.resolve_range(&(selection.start..selection.end)),
                    reversed: selection.reversed,
                })
                .collect(),
        )
    }

    /// Rebases the anchors onto `snapshot` so later resolves are cheap.
    pub fn refresh(&mut self, snapshot: &BufferSnapshot) {
        *self = Self::from_resolved(snapshot, self.resolve(snapshot));
    }
}

fn merge(mut selections: Vec<ResolvedSelection>) -> Vec<ResolvedSelection> {
    selections.sort_by_key(|selection| (selection.range.start, selection.range.end));
    let mut merged: Vec<ResolvedSelection> = Vec::with_capacity(selections.len());
    for selection in selections {
        if let Some(last) = merged.last_mut() {
            let overlaps = selection.range.start < last.range.end;
            let same_caret = selection.range == last.range;
            if overlaps || same_caret {
                last.range.end = last.range.end.max(selection.range.end);
                continue;
            }
        }
        merged.push(selection);
    }
    merged
}
