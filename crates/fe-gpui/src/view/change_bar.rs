//! Clicks on the git change bar: the change lane's marks, which tell the
//! host which change was clicked.

use std::ops::Range;

use fe_display::ScreenRow;
use fe_editor::decorations::{DecorationKind, GutterMark};
use fe_editor::row_changes::RowChangeKind;
use gpui::{Bounds, Context, Pixels, Point, point, px};

use super::{EditorView, EditorViewEvent};

/// How far past a quarter line either side of the edge a removed line's
/// notch is drawn on a click still hits it, in pixels: the notch itself is
/// only a few pixels tall.
const MARK_REACH: f32 = 3.;

impl EditorView {
    /// The rows of the change whose mark a click at `position` hits, if the
    /// last frame's change lane is there: a bar covers its rows, a deletion
    /// mark the strip it is drawn in, across the edge between two rows.
    fn change_at(&self, position: Point<Pixels>) -> Option<Range<u32>> {
        let layout = self.layout.as_ref()?;
        let frame = &layout.frame;
        let at = layout.to_frame(position);
        let lane = frame.change_lane;
        if at.x < lane.x || at.x >= lane.x + lane.width {
            return None;
        }
        let changes = self.model.row_changes();
        if let Some(rows) = self.deletion_at(position) {
            return Some(rows);
        }
        if at.y + frame.scroll.y < 0. {
            return None;
        }
        let ScreenRow::Text(row) = frame.screen_row_at(at.y) else {
            return None;
        };
        let clicked = frame.row(row)?.buffer_row;
        let bar = changes
            .iter()
            .find(|change| change.kind != RowChangeKind::Deleted && change.rows.contains(&clicked));
        bar.map(|change| change.rows.clone())
    }

    /// The deletion whose mark, as the last frame drew it, a click at
    /// `position` is within a quarter line and [`MARK_REACH`] of: the marks the renderer
    /// placed are the only targets, so a row's continuation under soft wrap
    /// or a row scrolled out of view has none.
    fn deletion_at(&self, position: Point<Pixels>) -> Option<Range<u32>> {
        let layout = self.layout.as_ref()?;
        let frame = &layout.frame;
        let mark = layout.decorations.placed.iter().find(|placed| {
            placed.kind == DecorationKind::Gutter(GutterMark::Deletion) && {
                let reach = px(frame.line_height / 4. + MARK_REACH);
                // The notch starts at the lane's left, but a click anywhere
                // across the lane hits it.
                let lane = layout.bounds(frame.change_lane);
                let edge = placed.bounds.center().y;
                let hit = Bounds::from_corners(
                    point(lane.left(), edge - reach),
                    point(lane.right(), edge + reach),
                );
                hit.contains(&position)
            }
        })?;
        let y = layout.to_frame(mark.bounds.center()).y;
        let near = |edge: f32| (edge - y).abs() <= MARK_REACH;
        let deletions = self.model.row_changes();
        let deletion = |row: Option<u32>| {
            deletions.iter().find(|change| {
                change.kind == RowChangeKind::Deleted && Some(change.rows.start) == row
            })
        };
        // On the top of a row, or at the end of the text below the last.
        let above = frame.rows.iter().find(|laid| near(frame.row_top(laid.row)));
        if let Some(change) = deletion(above.map(|laid| laid.buffer_row)) {
            return Some(change.rows.clone());
        }
        let below = frame
            .rows
            .iter()
            .find(|laid| laid.last && near(frame.row_top(laid.row) + frame.line_height))?;
        deletions
            .iter()
            .find(|change| {
                change.kind == RowChangeKind::Deleted && change.rows.start > below.buffer_row
            })
            .map(|change| change.rows.clone())
    }

    /// Emits [`EditorViewEvent::ChangeBarClicked`] for a click on a change
    /// mark and puts the caret at the change's first row. Returns whether
    /// the click hit one.
    pub(super) fn click_change_bar(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(rows) = self.change_at(position) else {
            return false;
        };
        // As any click does, it drops a pending vim command and leaves
        // vim's visual mode.
        self.cancel_vim(cx);
        let text = self.model.text();
        let row = rows.start.min(text.row_count().saturating_sub(1));
        let offset = text.row_start(row);
        self.split_session(fe_vim::Splits::Moves, |model| model.set_cursor(offset), cx);
        cx.emit(EditorViewEvent::ChangeBarClicked { rows });
        true
    }
}

#[cfg(test)]
#[path = "deletion_wedge_click_tests.rs"]
mod deletion_wedge_click_tests;
