//! Paints [`EditorModel::row_changes`] through the decoration seam: the
//! provider every [`EditorModel`](crate::EditorModel) with a diff base
//! queries.
//!
//! [`EditorModel::row_changes`]: crate::EditorModel::row_changes

use std::ops::Range;
use std::rc::Weak;

use fe_buffer::BufferSnapshot;

use crate::decorations::{
    Decoration, DecorationKind, DecorationProvider, GutterMark, Tint, overlaps,
};
use crate::row_changes::{RowChangeKind, RowChangeState};

/// Below every other provider's decorations.
const PRIORITY: i32 = -100;

pub(crate) struct RowChangeDecorations {
    state: Weak<RowChangeState>,
}

impl RowChangeDecorations {
    pub(crate) fn new(state: Weak<RowChangeState>) -> Self {
        Self { state }
    }
}

impl DecorationProvider for RowChangeDecorations {
    fn decorations(
        &self,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let changes = state.changes(snapshot);
        if changes.is_empty() {
            return;
        }
        let text = snapshot.text();
        let row_count = text.row_count();
        let offset = |row: u32| {
            if row >= row_count {
                text.len()
            } else {
                text.row_start(row)
            }
        };
        let first_row = text.offset_to_point(range.start).row;
        // Past the last row, a change at the row count is in reach too.
        let last_row = if range.end >= text.len() {
            row_count
        } else {
            text.offset_to_point(range.end).row
        };
        // Changes are sorted and disjoint, so their ends are sorted too.
        let first = changes.partition_point(|change| change.rows.end < first_row);
        for change in &changes[first..] {
            if change.rows.start > last_row {
                break;
            }
            let span = offset(change.rows.start)..offset(change.rows.end);
            if !overlaps(&span, &range) {
                continue;
            }
            match change.kind {
                RowChangeKind::Deleted => out.push(
                    Decoration::new(
                        span.start..span.start,
                        DecorationKind::Gutter(GutterMark::Deletion),
                        Tint::Removed,
                    )
                    .with_priority(PRIORITY),
                ),
                kind => {
                    let tint = match kind {
                        RowChangeKind::Added => Tint::Added,
                        _ => Tint::Modified,
                    };
                    out.push(
                        Decoration::new(span, DecorationKind::Gutter(GutterMark::Bar), tint)
                            .with_priority(PRIORITY),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "row_change_decorations_tests.rs"]
mod tests;
