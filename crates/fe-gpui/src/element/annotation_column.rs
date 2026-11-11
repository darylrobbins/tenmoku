//! The host's annotation column left of the line numbers
//! ([`crate::gutter_annotations`]): each display row's text and tint, laid
//! out once the frame is, and painted.

use fe_display::layout::{Frame, Rect, StyleRun, TextMeasure as _, Vec2};
use gpui::{
    App, Bounds, ContentMask, Hitbox, HitboxBehavior, Hsla, Pixels, Point, Window, fill, px,
};

use super::{FrameLayout, Line, Shaper, host_blocks};
use crate::EditorView;

/// The column's display rows in the frame.
#[derive(Default)]
pub(super) struct Placed {
    rows: Vec<PlacedRow>,
    /// How far in from the column's left the texts start: half an em.
    inset: f32,
}

struct PlacedRow {
    buffer_row: u32,
    /// The display row's slot across the column, in frame coordinates.
    rect: Rect,
    /// An entry's text, on the first display row of its first row.
    text: Option<(gpui::SharedString, Line)>,
    tint: Option<Hsla>,
}

impl FrameLayout {
    /// The buffer row of the display row whose slot in the annotation
    /// column holds a window position, and the slot's window bounds.
    pub(crate) fn annotation_row_at(
        &self,
        position: Point<Pixels>,
    ) -> Option<(u32, Bounds<Pixels>)> {
        let at = self.to_frame(position);
        let frame = &self.frame;
        let column = frame.annotation_column;
        if column.width <= 0. || !column.contains(at) {
            return None;
        }
        let row = host_blocks::row_at(frame, at.y)?;
        Some((row.buffer_row, self.bounds(slot(frame, row.row))))
    }
}

/// Display row `row`'s slot across the column.
fn slot(frame: &Frame<Line>, row: u32) -> Rect {
    let column = frame.annotation_column;
    Rect {
        x: column.x,
        y: frame.row_top(row),
        width: column.width,
        height: frame.line_height,
    }
}

/// The text and tint of each of the frame's display rows in the column:
/// an entry's text on the first display row of its first buffer row, and
/// its tint on all of them; nothing on rows edited since.
pub(super) fn layout(
    frame: &Frame<Line>,
    view: &EditorView,
    shaper: &mut Shaper,
    em: f32,
) -> Placed {
    let Some((annotations, resolved)) = view.annotation_rows() else {
        return Placed::default();
    };
    if frame.annotation_column.width <= 0. {
        return Placed::default();
    }
    let text = view.model.text();
    let theme = &view.theme;
    let rows = (frame.rows.iter())
        .map(|row| {
            let ix = resolved.entry_at(row.buffer_row);
            let entry = ix.map(|ix| &annotations.entries[ix]);
            let first = ix.and_then(|ix| resolved.first_row(ix)) == Some(row.buffer_row)
                && row.start == text.row_start(row.buffer_row);
            let text = entry.filter(|_| first).map(|entry| {
                let runs = [StyleRun {
                    len: entry.text.len(),
                    style: theme.gutter_foreground,
                }];
                (entry.text.clone(), shaper.shape(&entry.text, &runs))
            });
            let tint = entry.and_then(|entry| entry.heat).map(|heat| {
                (theme.gutter_active_foreground).opacity(0.04 + 0.16 * heat.clamp(0., 1.))
            });
            PlacedRow {
                buffer_row: row.buffer_row,
                rect: slot(frame, row.row),
                text,
                tint,
            }
        })
        .collect();
    Placed {
        rows,
        inset: em / 2.,
    }
}

/// A hitbox over the column, if there is one, and one over each run of
/// display rows an entry shows on, which show a pointing hand.
pub(super) fn insert_hitboxes(
    placed: &Placed,
    layout: &FrameLayout,
    view: &EditorView,
    window: &mut Window,
) -> (Option<Hitbox>, Vec<Hitbox>) {
    let column = layout.frame.annotation_column;
    if column.width <= 0. {
        return (None, Vec::new());
    }
    let hitbox = window.insert_hitbox(layout.bounds(column), HitboxBehavior::Normal);
    let resolved = view.annotation_rows().map(|(_, resolved)| resolved);
    let shown = |row: &PlacedRow| {
        resolved
            .and_then(|resolved| resolved.entry_at(row.buffer_row))
            .is_some()
    };
    let mut runs: Vec<Rect> = Vec::new();
    for row in placed.rows.iter().filter(|row| shown(row)) {
        match runs.last_mut() {
            Some(run) if run.y + run.height == row.rect.y => run.height += row.rect.height,
            _ => runs.push(row.rect),
        }
    }
    let entries = (runs.into_iter())
        .map(|run| window.insert_hitbox(layout.bounds(run), HitboxBehavior::Normal))
        .collect();
    (Some(hitbox), entries)
}

/// Paints the rows' tints, then their texts, clipped to the column.
pub(super) fn paint(
    placed: &Placed,
    layout: &FrameLayout,
    window: &mut Window,
    cx: &mut App,
    #[cfg(test)] painted: &mut Vec<(u32, Option<gpui::SharedString>, Option<Hsla>)>,
) {
    let frame = &layout.frame;
    if frame.annotation_column.width <= 0. {
        return;
    }
    let column = layout.bounds(frame.annotation_column);
    let line_height = px(frame.line_height);
    window.with_content_mask(Some(ContentMask { bounds: column }), |window| {
        for row in &placed.rows {
            if let Some(tint) = row.tint {
                window.paint_quad(fill(layout.bounds(row.rect), tint));
            }
            if let Some((_, line)) = &row.text {
                let origin = layout.to_window(Vec2 {
                    x: row.rect.x + placed.inset,
                    y: row.rect.y,
                });
                line.paint(origin, line_height, window, cx);
            }
            #[cfg(test)]
            painted.push((
                row.buffer_row,
                row.text.as_ref().map(|(text, _)| text.clone()),
                row.tint,
            ));
        }
    });
}

/// The agent lane's runs over the frame's display rows, in frame
/// coordinates: the top and bottom of each run of rows shown one under the
/// next whose buffer rows are in the lane ([`crate::gutter_annotations`]),
/// a block or a fold between two rows ending it.
pub(super) fn agent_runs(frame: &Frame<Line>, view: &EditorView) -> Vec<(f32, f32)> {
    let Some((_, resolved)) = view.annotation_rows() else {
        return Vec::new();
    };
    if !view.gutter {
        return Vec::new();
    }
    let screen_row = |row: u32| frame.block_map.screen_row(row);
    let mut runs: Vec<(f32, f32)> = Vec::new();
    let mut last: Option<(u32, u32)> = None;
    for row in &frame.rows {
        if !resolved.agent_at(row.buffer_row) {
            last = None;
            continue;
        }
        let top = frame.row_top(row.row);
        let joins = last.is_some_and(|(screen, buffer)| {
            screen_row(row.row) == screen + 1 && row.buffer_row - buffer <= 1
        });
        match runs.last_mut() {
            Some(run) if joins => run.1 = top + frame.line_height,
            _ => runs.push((top, top + frame.line_height)),
        }
        last = Some((screen_row(row.row), row.buffer_row));
    }
    runs
}
