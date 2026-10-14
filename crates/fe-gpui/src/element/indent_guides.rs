//! Indent guides ([`fe_editor::indent_guides`]) laid out on a frame's
//! display rows and painted: a thin vertical line at each indent level,
//! the one of the caret's block in its own colour.

use fe_display::{CharWidth as _, layout::Rect};
use fe_editor::{
    IndentStyle,
    indent_guides::{active_indent_guide, indent_guides},
};
use gpui::{Bounds, Hsla, Pixels, Window, fill};

use super::FrameLayout;
use crate::EditorView;

/// A guide's run of display rows, laid out.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Placed {
    /// In window coordinates.
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) color: Hsla,
}

/// The guides over the frame's shown buffer rows, one rect per run of each
/// guide's display rows shown one under the next: a soft-wrapped row's
/// pieces join the run, a block or fold between two rows ends it. Nothing
/// when the theme turns guides off.
pub(super) fn layout(layout: &mut FrameLayout, view: &EditorView, window: &Window) -> Vec<Placed> {
    let theme = &view.theme;
    let frame = &layout.frame;
    if !theme.indent_guides {
        return Vec::new();
    }
    if frame.rows.is_empty() {
        return Vec::new();
    }
    let model = &view.model;
    let text = model.text();
    let tab = model.tab_size() as u32;
    let width = match model.indent_style() {
        Some(IndentStyle::Spaces(n)) => u32::from(n),
        _ => tab,
    };
    let mut guides = Vec::new();
    for rows in visible_ranges(frame.rows.iter().map(|row| row.buffer_row)) {
        #[cfg(test)]
        VISITED.with(|visited| visited.borrow_mut().push(rows.clone()));
        guides.extend(indent_guides(text, rows, width, tab));
    }
    // Each range's guides are sorted; a guide crossing a fold is one per side.
    guides.sort_by_key(|guide| (guide.column, guide.rows.start));
    if guides.is_empty() {
        return Vec::new();
    }
    let caret_row = text.offset_to_point(model.primary_selection().head()).row;
    let active = active_indent_guide(text, caret_row, width, tab);

    let space = f64::from(layout.glyph_widths.char_width(' '));
    let scale = window.scale_factor();
    let line = (1. * scale).round().max(1.) / scale;
    let screen_row = |row: u32| frame.block_map.screen_row(row);
    let mut rects = Vec::new();
    for guide in guides {
        let is_active = active.as_ref().is_some_and(|active| {
            active.column == guide.column
                && active.rows.start < guide.rows.end
                && guide.rows.start < active.rows.end
        });
        let color = match is_active {
            true => theme.indent_guide_active,
            false => theme.indent_guide,
        };
        let from = (frame.rows).partition_point(|row| row.buffer_row < guide.rows.start);
        let to = (frame.rows).partition_point(|row| row.buffer_row < guide.rows.end);
        let rows = &frame.rows[from..to];
        let x = frame.view_x(f64::from(guide.column) * space);
        let mut at = 0;
        while at < rows.len() {
            let first = &rows[at];
            let mut run = 1;
            while let Some(row) = rows.get(at + run) {
                let previous = &rows[at + run - 1];
                let next_on_screen = screen_row(row.row) == screen_row(previous.row) + 1;
                let next_in_text = row.buffer_row - previous.buffer_row <= 1;
                if !(next_on_screen && next_in_text) {
                    break;
                }
                run += 1;
            }
            at += run;
            let rect = Rect {
                x,
                y: frame.row_top(first.row),
                width: line,
                height: run as f32 * frame.line_height,
            };
            rects.push((rect, color));
        }
    }
    (rects.into_iter())
        .map(|(rect, color)| Placed {
            bounds: layout.bounds(rect),
            color,
        })
        .collect()
}

/// The contiguous runs of buffer rows the frame shows, a soft-wrapped row's
/// pieces counting once; rows folded or blocked out between two shown rows
/// split the run.
fn visible_ranges(rows: impl Iterator<Item = u32>) -> Vec<std::ops::Range<u32>> {
    let mut ranges: Vec<std::ops::Range<u32>> = Vec::new();
    for row in rows {
        match ranges.last_mut() {
            Some(range) if row < range.end => {}
            Some(range) if row == range.end => range.end += 1,
            _ => ranges.push(row..row + 1),
        }
    }
    ranges
}

/// Paints the guides.
pub(super) fn paint(guides: &[Placed], window: &mut Window) {
    for guide in guides {
        window.paint_quad(fill(guide.bounds, guide.color));
    }
    #[cfg(test)]
    PAINTED.with(|painted| *painted.borrow_mut() = guides.to_vec());
}

#[cfg(test)]
thread_local! {
    /// The buffer-row ranges this thread's last frames computed guides over.
    pub(crate) static VISITED: std::cell::RefCell<Vec<std::ops::Range<u32>>> =
        const { std::cell::RefCell::new(Vec::new()) };

    /// The guides this thread's last frame painted.
    pub(crate) static PAINTED: std::cell::RefCell<Vec<Placed>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
#[path = "indent_guides_fold_tests.rs"]
mod fold_tests;
#[cfg(test)]
#[path = "indent_guides_tests.rs"]
mod tests;
