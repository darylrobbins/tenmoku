//! Whitespace marks (`editor.render_whitespace`) laid out on a frame's
//! display rows and painted, as VS Code's `editor.renderWhitespace`: a
//! centred dot on a space and an arrow across a tab's advance, in the
//! theme's `whitespace` colour.

use std::ops::Range;

use fe_display::layout::{MAX_SHAPED_ROW_BYTES, Rect};
use gpui::{Bounds, LineLayout, PathBuilder, Pixels, Window, fill, point, px, size};

use super::FrameLayout;
use crate::{EditorView, RenderWhitespace};

/// A buffer row, its text and the byte indexes of its first and last
/// non-blank chars, if it has any.
type RowText = (u32, String, Option<(usize, usize)>);

/// What a mark is drawn on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Space,
    Tab,
}

/// A space or tab to mark, laid out.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Mark {
    /// The buffer offset of the space or tab.
    pub(crate) offset: usize,
    pub(crate) kind: Kind,
    /// The char's advance on its display row, a row tall, in window
    /// coordinates.
    pub(crate) bounds: Bounds<Pixels>,
}

/// The marks on the frame's display rows that the setting shows and the
/// view's width reaches, in row and then offset order. Rows longer than
/// [`MAX_SHAPED_ROW_BYTES`] get none.
pub(super) fn layout(layout: &FrameLayout, view: &EditorView) -> Vec<Mark> {
    let mode = view.theme.render_whitespace;
    let frame = &layout.frame;
    if mode == RenderWhitespace::None || frame.rows.is_empty() {
        return Vec::new();
    }
    let model = &view.model;
    let mut selected: Vec<Range<usize>> = Vec::new();
    if mode == RenderWhitespace::Selection {
        selected = (model.selections().into_iter())
            .map(|selection| selection.range)
            .filter(|range| !range.is_empty())
            .collect();
        if selected.is_empty() {
            return Vec::new();
        }
        selected.sort_by_key(|range| range.start);
    }
    let is_selected = |offset: usize| {
        let after = selected.partition_point(|range| range.start <= offset);
        selected[..after].iter().any(|range| offset < range.end)
    };
    let text = model.text();
    let text_bounds = frame.text_bounds();
    let (left, right) = (text_bounds.x, text_bounds.x + text_bounds.width);
    let mut marks = Vec::new();
    // The buffer row of the last display row, its text and the char
    // indexes of its first and last non-blank chars: a wrapped row's pieces
    // share them.
    let mut current: Option<RowText> = None;
    let mut found = Vec::new();
    for row in &frame.rows {
        let row_len = text.row_len(row.buffer_row) as usize;
        if row_len > MAX_SHAPED_ROW_BYTES {
            continue;
        }
        if current
            .as_ref()
            .is_none_or(|(at, ..)| *at != row.buffer_row)
        {
            let line = text.row_to_string(row.buffer_row);
            let blank = |byte: &u8| matches!(byte, b' ' | b'\t');
            let first = line.bytes().position(|byte| !blank(&byte));
            let last = line.bytes().rposition(|byte| !blank(&byte));
            current = Some((row.buffer_row, line, first.zip(last)));
        }
        let Some((_, line, content)) = &current else {
            continue;
        };
        let row_start = text.row_start(row.buffer_row);
        let shown = row.shown();
        let bytes = line.as_bytes();
        let start = row.start.max(shown.start);
        let end = (row.end.min(shown.end).min(row_start + row_len)).max(start);
        let piece = start - row_start..end - row_start;
        found.clear();
        for at in piece {
            let kind = match bytes[at] {
                b' ' => Kind::Space,
                b'\t' => Kind::Tab,
                _ => continue,
            };
            let shows = match mode {
                RenderWhitespace::None => false,
                RenderWhitespace::All => true,
                RenderWhitespace::Selection => is_selected(row_start + at),
                RenderWhitespace::Trailing => content.is_none_or(|(_, last)| at > last),
                // A single space between two non-blank chars has none.
                RenderWhitespace::Boundary => {
                    let blank = |at: usize| matches!(bytes.get(at), Some(b' ' | b'\t'));
                    let inside = content.is_some_and(|(first, last)| first < at && at < last);
                    !(kind == Kind::Space && inside && !blank(at - 1) && !blank(at + 1))
                }
            };
            if shows {
                found.push((row_start + at, kind));
            }
        }
        if found.is_empty() {
            continue;
        }
        // Where each char starts, past any hint before it, and ends.
        let indexes = (found.iter()).flat_map(|&(offset, _)| {
            let col = offset - shown.start;
            [
                row.expanded.to_display_after_hints(col),
                row.expanded.to_display(col + 1),
            ]
        });
        let xs = xs_for_indexes(&row.line.layout, indexes);
        let top = frame.row_top(row.row);
        for (&(offset, kind), x) in found.iter().zip(xs.as_chunks::<2>().0) {
            let start = frame.view_x(row.start_x() + f64::from(x[0]));
            let end = frame.view_x(row.start_x() + f64::from(x[1]));
            if end <= left || start >= right {
                continue;
            }
            let rect = Rect {
                x: start,
                y: top,
                width: end - start,
                height: frame.line_height,
            };
            marks.push(Mark {
                offset,
                kind,
                bounds: layout.bounds(rect),
            });
        }
    }
    marks
}

/// The x of the boundary before each of `indexes`, which never decrease,
/// from the line's start, as [`LineLayout::x_for_index`] gives it, in one
/// walk over the glyphs.
fn xs_for_indexes(line: &LineLayout, indexes: impl Iterator<Item = usize>) -> Vec<f32> {
    let mut glyphs = (line.runs.iter())
        .flat_map(|run| run.glyphs.iter())
        .peekable();
    indexes
        .map(|index| {
            while glyphs.next_if(|glyph| glyph.index < index).is_some() {}
            glyphs
                .peek()
                .map_or(line.width, |glyph| glyph.position.x)
                .into()
        })
        .collect()
}

/// Paints the marks in `color`, `font_size` setting their size.
pub(super) fn paint(marks: &[Mark], color: gpui::Hsla, font_size: Pixels, window: &mut Window) {
    let scale = window.scale_factor();
    let pixel = 1. / scale;
    // A dot about the size of a `·` glyph's, and the arrow's stroke.
    let dot = px(((f32::from(font_size) * 0.15 * scale).round().max(1.)) / scale);
    let stroke = px(pixel);
    for mark in marks {
        let bounds = mark.bounds;
        let center = bounds.center();
        match mark.kind {
            Kind::Space => {
                let origin = point(center.x - dot / 2., center.y - dot / 2.);
                window.paint_quad(fill(Bounds::new(origin, size(dot, dot)), color));
            }
            Kind::Tab => {
                let width = bounds.size.width;
                let inset = (width * 0.15).min(px(2.));
                let head = (font_size * 0.25).min(width / 2.);
                let (from, tip) = (bounds.left() + inset, bounds.right() - inset);
                let mut path = PathBuilder::stroke(stroke);
                path.move_to(point(from, center.y));
                path.line_to(point(tip, center.y));
                path.move_to(point(tip - head, center.y - head));
                path.line_to(point(tip, center.y));
                path.line_to(point(tip - head, center.y + head));
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            }
        }
    }
    #[cfg(test)]
    PAINTED.with(|painted| *painted.borrow_mut() = marks.to_vec());
}

#[cfg(test)]
thread_local! {
    /// The marks this thread's last frame painted.
    pub(crate) static PAINTED: std::cell::RefCell<Vec<Mark>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
#[path = "whitespace_tests.rs"]
mod tests;
