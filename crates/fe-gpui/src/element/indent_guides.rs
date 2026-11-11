//! Indent guides ([`fe_editor::indent_guides`]) laid out on a frame's
//! display rows and painted: a 1 px vertical line every [`GUIDE_COLUMNS`]
//! columns inside the rows' leading whitespace, whatever the file's indent
//! (`docs/design/design-system.md#editor`), the one of the caret's block,
//! and of a hovered fold ([`super::HoveredFold`]), a step brighter in its
//! own colour. A display row whose text starts left of a guide, such as a
//! soft-wrapped row's continuation, breaks it.

use fe_display::{CharWidth as _, layout::Rect};
use fe_editor::indent_guides::{active_indent_guide, indent_guides};
use gpui::{Bounds, Hsla, Pixels, Window, fill};

use super::FrameLayout;
use crate::EditorView;

/// How many columns apart the guides are.
const GUIDE_COLUMNS: u32 = 4;

/// The most of a row looked through for its text's first column: indents
/// past it are no guide's.
const SCAN_BYTES: usize = 4096;

/// A guide's run of display rows, laid out.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Placed {
    /// In window coordinates.
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) color: Hsla,
}

/// The guides over the frame's shown buffer rows, one rect per run of each
/// guide's display rows shown one under the next: a soft-wrapped row's
/// pieces join the run, a block or fold between two rows ends it, and so
/// does a display row with text at or left of the guide. Nothing when the
/// theme turns guides off.
pub(super) fn layout(layout: &mut FrameLayout, view: &EditorView, window: &Window) -> Vec<Placed> {
    let theme = &view.theme;
    let FrameLayout {
        frame,
        long_rows,
        glyph_widths,
        hovered_fold,
        ..
    } = layout;
    if !theme.indent_guides {
        return Vec::new();
    }
    if frame.rows.is_empty() {
        return Vec::new();
    }
    let model = &view.model;
    let text = model.text();
    let tab = model.tab_size() as u32;
    let width = GUIDE_COLUMNS;
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
    // The hovered fold's: at its header's indent, down the rows it folds.
    let hovered = (hovered_fold.as_ref())
        .and_then(|fold| fold.open.clone())
        .map(|(candidate, _)| {
            let indent = leading_columns(text, *candidate.start(), tab);
            (
                indent / width * width,
                candidate.start() + 1..candidate.end() + 1,
            )
        });

    let space = f64::from(glyph_widths.char_width(' '));
    // Where each display row's text starts, in frame coordinates: none on a
    // row of only whitespace.
    let text_x: Vec<Option<f32>> = (frame.rows.iter())
        .map(|row| {
            let end = text.clip_offset(row.end.min(row.start + SCAN_BYTES), fe_buffer::Bias::Left);
            let head = text.slice_to_string(row.start..end);
            let ws = head.len() - head.trim_start_matches([' ', '\t']).len();
            (row.start + ws < row.end)
                .then(|| frame.view_x(row.x_for_offset(row.start + ws, long_rows, glyph_widths)))
        })
        .collect();
    let scale = window.scale_factor();
    let line = (1. * scale).round().max(1.) / scale;
    let screen_row = |row: u32| frame.block_map.screen_row(row);
    let mut rects = Vec::new();
    for guide in guides {
        let crosses = |column: u32, rows: &std::ops::Range<u32>| {
            column == guide.column && rows.start < guide.rows.end && guide.rows.start < rows.end
        };
        let is_active = active
            .as_ref()
            .is_some_and(|active| crosses(active.column, &active.rows))
            || (hovered.as_ref()).is_some_and(|(column, rows)| crosses(*column, rows));
        let color = match is_active {
            true => theme.indent_guide_active,
            false => theme.indent_guide,
        };
        let from = (frame.rows).partition_point(|row| row.buffer_row < guide.rows.start);
        let to = (frame.rows).partition_point(|row| row.buffer_row < guide.rows.end);
        let rows = &frame.rows[from..to];
        let x = frame.view_x(f64::from(guide.column) * space);
        let clear = |ix: usize| text_x[from + ix].is_none_or(|text| x < text);
        let mut at = 0;
        while at < rows.len() {
            if !clear(at) {
                at += 1;
                continue;
            }
            let first = &rows[at];
            let mut run = 1;
            while let Some(row) = rows.get(at + run) {
                let previous = &rows[at + run - 1];
                let next_on_screen = screen_row(row.row) == screen_row(previous.row) + 1;
                let next_in_text = row.buffer_row - previous.buffer_row <= 1;
                if !(next_on_screen && next_in_text && clear(at + run)) {
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

/// The display columns of buffer row `row`'s leading whitespace, tabs
/// `tab` columns apart.
fn leading_columns(text: &fe_buffer::Text, row: u32, tab: u32) -> u32 {
    let start = text.row_start(row);
    // Indents past this are no guide's.
    let end = start + (text.row_len(row) as usize).min(4096);
    let end = text.clip_offset(end, fe_buffer::Bias::Left);
    let mut columns = 0;
    for ch in text.slice_to_string(start..end).chars() {
        match ch {
            ' ' => columns += 1,
            '\t' => columns += tab.max(1) - columns % tab.max(1),
            _ => break,
        }
    }
    columns
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
