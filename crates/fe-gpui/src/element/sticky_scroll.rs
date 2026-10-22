//! Sticky scroll's band ([`fe_display::sticky`]): the header rows of the
//! scopes enclosing the top of the view, pinned there over the text, each
//! with its line number in the gutter and its text in the colours the text
//! area gives the row, with a line and a shadow under the band. The scopes
//! come from fe-syntax's outline, found once per top row and text version
//! ([`scopes`]); the band is laid out once the frame has settled and painted
//! over the text, selections and carets. It spans the gutter and the text,
//! not the minimap beside them.

#[cfg(test)]
use std::cell::Cell;
use std::ops::{Range, RangeInclusive};

use fe_buffer::Version;
use fe_display::{
    Bias, ExpandedRow,
    layout::{
        Frame, LINE_NUMBER_GAP_EMS, MAX_SHAPED_ROW_BYTES, Rect, StyleRun, TextMeasure, Vec2,
        row_runs,
    },
};
use fe_syntax::Syntax;
use gpui::{App, ContentMask, Window, fill, linear_color_stop, linear_gradient, px};
#[cfg(test)]
use gpui::{Bounds, Hsla, Pixels};

use super::{FrameLayout, Line, Shaper, foreground_spans, styled_highlights};
use crate::{EditorTheme, EditorView};

/// The most rows the band pins, VS Code's `editor.stickyScroll.maxLineCount`
/// default.
pub(crate) const MAX_ROWS: usize = 5;

/// How far the shadow under the band fades down, as VS Code's
/// `scrollbar.shadow` does.
const SHADOW_HEIGHT: f32 = 3.;

/// How far either side of the view a long header's shaped slice reaches, in
/// ems, as a long row's in the text area.
const SLICE_MARGIN_EMS: f32 = 32.;

/// The scopes the band was last found from, cached on the view.
pub(crate) struct Scopes {
    /// The language, the buffer rows the scopes overlap and the text version
    /// they were found for.
    key: (&'static str, Range<u32>, Version),
    scopes: Vec<RangeInclusive<u32>>,
}

#[cfg(test)]
thread_local! {
    /// How many times [`scopes`] has asked the outline, for tests.
    static QUERIES: Cell<usize> = const { Cell::new(0) };
}

/// How many times the outline has been asked for scopes on this thread.
#[cfg(test)]
pub(crate) fn queries() -> usize {
    QUERIES.with(Cell::get)
}

/// The scopes overlapping the rows at the top of `frame` that the band can
/// pin, outermost first, from the view's cache when the top row and the
/// text are those it was found for. None while the band is off or there's
/// no syntax tree for the text.
pub(crate) fn scopes<L>(view: &mut EditorView, frame: &Frame<L>) -> Vec<RangeInclusive<u32>> {
    let syntax = (view.syntax.as_ref().map(fe_syntax::SyntaxDriver::syntax))
        .filter(|syntax| syntax.version() == view.model.snapshot().version());
    let (Some(syntax), true) = (syntax, view.theme.sticky_scroll) else {
        return Vec::new();
    };
    let display = view.model.display();
    let row_count = display.row_count();
    let line_height = frame.line_height.max(1.);
    // From the display row at the top to the one the band's last slot can
    // reach, by the whole screen row at the top, so scrolling within it
    // keeps the rows.
    let top = (frame.scroll.y / line_height).floor();
    let blocks = &frame.block_map;
    let display_row =
        |screen_top: f32| (blocks.to_display_top(screen_top).floor() as u32).min(row_count - 1);
    let first = display.row(display_row(top)).buffer_row;
    let last = display
        .row(display_row(top + MAX_ROWS as f32 + 1.))
        .buffer_row;
    let key = (syntax.language().name(), first..last + 1, syntax.version());
    if let Some(cached) = view
        .sticky_scopes
        .as_ref()
        .filter(|cached| cached.key == key)
    {
        return cached.scopes.clone();
    }
    #[cfg(test)]
    QUERIES.with(|queries| queries.set(queries.get() + 1));
    // Not parsed yet, it's asked again next frame; a language without an
    // outline, or a file too large for one, has no scopes.
    let scopes = match fe_syntax::outline_scopes(syntax, key.1.clone()) {
        Ok(scopes) => scopes,
        Err(fe_syntax::NoOutline::NotParsed) => return Vec::new(),
        Err(_) => Vec::new(),
    };
    view.sticky_scopes = Some(Scopes {
        key,
        scopes: scopes.clone(),
    });
    scopes
}

/// One pinned row, in the frame's coordinates.
pub(crate) struct Row {
    #[cfg(test)]
    pub(crate) buffer_row: u32,
    /// The row's top, from the editor's.
    pub(crate) y: f32,
    /// The shaped text and the x it's painted at.
    pub(crate) line: Line,
    pub(crate) x: f32,
    /// What's shaped: the row's text from where it starts, tabs expanded.
    #[cfg(test)]
    pub(crate) text: String,
    /// The line number and its x, with a gutter.
    pub(crate) number: Option<(f32, Line)>,
}

/// The band one frame laid out.
pub(crate) struct Placed {
    /// Top first.
    pub(crate) rows: Vec<Row>,
    /// Across the gutter and the text, from the editor's top to the bottom
    /// of the last row.
    pub(crate) rect: Rect,
    /// Right of the gutter, where the rows' text is clipped to.
    text: Rect,
}

/// Lays out the band over `layout`'s frame from `scopes`, its rows shaped
/// with `shaper`: `None` while it's off or pins nothing.
pub(crate) fn layout(
    layout: &mut FrameLayout,
    view: &EditorView,
    scopes: &[RangeInclusive<u32>],
    syntax: Option<&Syntax>,
    unnecessary: &[Range<usize>],
    shaper: &mut Shaper,
    em: f32,
) -> Option<Placed> {
    if !view.theme.sticky_scroll || scopes.is_empty() {
        return None;
    }
    let theme = &view.theme;
    let model = &view.model;
    let display = model.display();
    let frame = &layout.frame;
    let line_height = frame.line_height;
    let pinned = fe_display::sticky_rows(
        scopes,
        display,
        &frame.block_map,
        frame.scroll.y,
        line_height,
        MAX_ROWS,
        None,
    );
    if pinned.is_empty() {
        return None;
    }
    let text = model.text();
    // The rows with a caret, whose numbers are the active colour.
    let caret_rows: Vec<u32> = (model.selections().iter())
        .filter(|selection| selection.range.is_empty())
        .map(|selection| text.offset_to_point(selection.head()).row)
        .collect();
    let tab_size = model.tab_size();
    let wrapped = display.wrap_width().is_some();
    let text_bounds = frame.text_bounds();
    let (scroll_x, view_width) = (f64::from(frame.scroll.x), f64::from(text_bounds.width));
    let number_right = frame.fold_column.x - em * LINE_NUMBER_GAP_EMS;
    let mut rows = Vec::with_capacity(pinned.len());
    for row in pinned {
        // A wrapped header shows its first display row.
        let display_row = display.row(display.row_for_buffer_row(row.buffer_row));
        let range = display_row.range;
        // A long row, as in the text area, is shaped from a slice around the
        // view, and the rest of the row cut off.
        let (shown, start_column, start_x) = if !wrapped && range.len() > MAX_SHAPED_ROW_BYTES {
            let FrameLayout {
                long_rows,
                glyph_widths,
                ..
            } = layout;
            let index = long_rows.index(
                display.source_key(),
                display.text(),
                row.buffer_row,
                tab_size,
                glyph_widths,
            );
            let margin = f64::from(em * SLICE_MARGIN_EMS);
            let slice = index.slice(scroll_x..scroll_x + view_width, margin, glyph_widths);
            (slice.range, slice.start_column, slice.start_x)
        } else {
            let len = range.len().min(MAX_SHAPED_ROW_BYTES);
            let end = text.clip_offset(range.start + len, Bias::Left);
            (range.start..end, 0, 0.)
        };
        let expanded = ExpandedRow::with_start_column(
            &text.slice_to_string(shown.clone()),
            start_column,
            tab_size,
        );
        let foreground = foreground_spans(&model.decorations(shown.clone()), theme);
        let spans = styled_highlights(syntax, theme, unnecessary, &foreground, shown.clone());
        let first = spans.partition_point(|span| span.range.end <= shown.start);
        let runs = row_runs(&expanded, shown.start, &spans[first..], &theme.foreground);
        let line = shaper.shape(expanded.text(), &runs);
        let number = view.gutter.then(|| {
            let color = match caret_rows.contains(&row.buffer_row) {
                true => theme.gutter_active_foreground,
                false => theme.gutter_foreground,
            };
            let number = (row.buffer_row + 1).to_string();
            let run = StyleRun {
                len: number.len(),
                style: color,
            };
            let line = shaper.shape(&number, &[run]);
            let x = number_right - f32::from(line.layout.width);
            (x, line)
        });
        rows.push(Row {
            #[cfg(test)]
            buffer_row: row.buffer_row,
            y: row.y,
            line,
            x: layout.frame.view_x(start_x),
            #[cfg(test)]
            text: expanded.text().to_owned(),
            number,
        });
    }
    let frame = &layout.frame;
    let bottom = rows
        .iter()
        .map(|row| row.y + line_height)
        .fold(0., f32::max);
    Some(Placed {
        rows,
        rect: Rect {
            x: 0.,
            y: 0.,
            width: frame.width,
            height: bottom,
        },
        text: text_bounds,
    })
}

/// The band's row rect at `y`.
fn row_rect(placed: &Placed, y: f32, line_height: f32) -> Rect {
    Rect {
        y,
        height: line_height,
        ..placed.rect
    }
}

/// The line under the band, if the theme's is visible, and the shadow below
/// it, if that is.
fn edges(placed: &Placed, theme: &EditorTheme) -> (Option<Rect>, Option<Rect>) {
    let bottom = placed.rect.y + placed.rect.height;
    let border = (theme.sticky_scroll_border.a > 0.).then_some(Rect {
        y: bottom,
        height: 1.,
        ..placed.rect
    });
    let shadow = (theme.sticky_scroll_shadow.a > 0.).then_some(Rect {
        y: bottom + border.map_or(0., |border| border.height),
        height: SHADOW_HEIGHT,
        ..placed.rect
    });
    (border, shadow)
}

/// Paints the band `layout` placed: the rows bottom first, so a row pushed
/// up slides under the one above it, each over the band's background, then
/// the line and the shadow under them.
pub(crate) fn paint(layout: &FrameLayout, theme: &EditorTheme, window: &mut Window, cx: &mut App) {
    let Some(placed) = &layout.sticky_scroll else {
        return;
    };
    let line_height = layout.frame.line_height;
    let text = layout.bounds(placed.text);
    for row in placed.rows.iter().rev() {
        let rect = layout.bounds(row_rect(placed, row.y, line_height));
        window.paint_quad(fill(rect, theme.sticky_scroll_background));
        if let Some((x, number)) = &row.number {
            let origin = layout.to_window(Vec2 { x: *x, y: row.y });
            number.paint(origin, px(line_height), window, cx);
        }
        let mask = ContentMask {
            bounds: text.intersect(&rect),
        };
        window.with_content_mask(Some(mask), |window| {
            let origin = layout.to_window(Vec2 { x: row.x, y: row.y });
            row.line.paint(origin, px(line_height), window, cx);
        });
    }
    let (border, shadow) = edges(placed, theme);
    if let Some(border) = border {
        window.paint_quad(fill(layout.bounds(border), theme.sticky_scroll_border));
    }
    if let Some(shadow) = shadow {
        let fade = linear_gradient(
            180.,
            linear_color_stop(theme.sticky_scroll_shadow, 0.),
            linear_color_stop(theme.sticky_scroll_shadow.opacity(0.), 1.),
        );
        window.paint_quad(fill(layout.bounds(shadow), fade));
    }
}

/// A row [`paint`] painted, in window coordinates.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PaintedRow {
    pub(crate) buffer_row: u32,
    pub(crate) bounds: Bounds<Pixels>,
    /// The line number and its colour.
    pub(crate) number: Option<(Bounds<Pixels>, Hsla)>,
    pub(crate) text: String,
    /// Where the text is painted from.
    pub(crate) text_origin: gpui::Point<Pixels>,
    /// The text's colour runs: bytes and colour.
    pub(crate) runs: Vec<(u32, Hsla)>,
}

/// What [`paint`] painted, in window coordinates.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Painted {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) background: Hsla,
    /// Top first.
    pub(crate) rows: Vec<PaintedRow>,
    pub(crate) border: Option<Bounds<Pixels>>,
    pub(crate) shadow: Option<Bounds<Pixels>>,
}

/// What [`paint`] paints for `layout`, for tests.
#[cfg(test)]
pub(crate) fn painted(layout: &FrameLayout, theme: &EditorTheme) -> Option<Painted> {
    let placed = layout.sticky_scroll.as_ref()?;
    let line_height = layout.frame.line_height;
    let rows = (placed.rows.iter())
        .map(|row| PaintedRow {
            buffer_row: row.buffer_row,
            bounds: layout.bounds(row_rect(placed, row.y, line_height)),
            number: row.number.as_ref().map(|(x, number)| {
                let rect = Rect {
                    x: *x,
                    y: row.y,
                    width: number.layout.width.into(),
                    height: line_height,
                };
                (layout.bounds(rect), number.color_at(0).unwrap_or_default())
            }),
            text: row.text.clone(),
            text_origin: layout.to_window(Vec2 { x: row.x, y: row.y }),
            runs: runs(&row.line),
        })
        .collect();
    let (border, shadow) = edges(placed, theme);
    Some(Painted {
        bounds: layout.bounds(placed.rect),
        background: theme.sticky_scroll_background,
        rows,
        border: border.map(|border| layout.bounds(border)),
        shadow: shadow.map(|shadow| layout.bounds(shadow)),
    })
}

/// A shaped line's colour runs: bytes and colour.
#[cfg(test)]
pub(crate) fn runs(line: &Line) -> Vec<(u32, Hsla)> {
    (line.decorations.iter())
        .map(|run| (run.len, run.color))
        .collect()
}

#[cfg(test)]
#[path = "sticky_scroll_tests.rs"]
mod tests;
