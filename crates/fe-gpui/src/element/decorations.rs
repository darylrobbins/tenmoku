//! The decorations an editor's providers hand out
//! ([`fe_editor::decorations`]), laid out on a frame's display rows and
//! painted: row backgrounds, gutter marks, highlights, underlines and text
//! after a line. Each [`Tint`] resolves through the theme
//! ([`tint_color`]).

use std::collections::HashMap;

use fe_display::{CharWidth as _, layout::Rect};
use fe_editor::decorations::{DecorationKind, GutterMark, Tint};
use gpui::{App, Bounds, DecorationRun, Hsla, Pixels, TextRun, UnderlineStyle, Window, fill, px};

use super::{FrameLayout, Line};
use crate::{EditorTheme, EditorView};

/// How opaque each kind is over its tint: a row background faint, a
/// highlight as a search match ([`super::search_match_color`]).
const ROW_BACKGROUND_ALPHA: f32 = 0.08;
const HIGHLIGHT_ALPHA: f32 = 0.22;
const GUTTER_ALPHA: f32 = 0.8;
const AFTER_LINE_ALPHA: f32 = 0.6;

/// A gutter bar's width, in pixels, rounded to device pixels.
const BAR_WIDTH: f32 = 2.;
/// A deletion mark's width and height, in pixels.
const DELETION: (f32, f32) = (6., 2.);
/// An underline's thickness, in pixels.
const UNDERLINE: f32 = 1.;

/// What each [`DecorationKind`] resolves its tint to, until the theme has
/// slots for them.
fn tint_color(theme: &EditorTheme, tint: Tint) -> Hsla {
    match tint {
        Tint::Error | Tint::Removed => theme.error,
        Tint::Warning => theme.warning,
        Tint::Information | Tint::Modified => theme.information,
        Tint::Hint => theme.hint,
        Tint::Accent => theme.cursor,
        Tint::Muted => theme.foreground,
        Tint::Added => Hsla {
            h: 130. / 360.,
            ..theme.information
        },
    }
}

/// A decoration laid out on one display row.
#[derive(Clone)]
pub(crate) struct Placed {
    pub(crate) kind: DecorationKind,
    /// In window coordinates. An underline's height is its thickness.
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) color: Hsla,
    /// An after-line decoration's text, shaped in `color`.
    text: Option<Line>,
}

/// When a frame's decorations are painted, each kind in the order the
/// model hands them out.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum Layer {
    /// Row backgrounds, then highlights: under search matches and the
    /// selection.
    Backgrounds,
    /// With the diagnostics' squiggles, over the text.
    Underlines,
    /// Over the text, clipped to the text area.
    AfterLine,
    /// With the diagnostics' gutter marks.
    Gutter,
}

/// What a frame laid out of the decorations its rows show.
#[derive(Clone, Default)]
pub(crate) struct Decorations {
    /// In paint order within each kind.
    pub(crate) placed: Vec<Placed>,
    line_height: Pixels,
}

impl Decorations {
    /// The decorations `view`'s model has for the frame's shown range, from
    /// one query, on the frame's display rows: folded and scrolled-off rows
    /// get nothing, every soft-wrapped piece of a covered row does. `em` is
    /// the editor font's, which after-line text is spaced by.
    pub(super) fn layout(
        layout: &mut FrameLayout,
        view: &EditorView,
        em: f32,
        window: &Window,
    ) -> Self {
        let FrameLayout {
            frame,
            long_rows,
            glyph_widths,
            ..
        } = layout;
        let line_height = frame.line_height;
        let mut out = Self {
            placed: Vec::new(),
            line_height: px(line_height),
        };
        let (Some(first), Some(last)) = (frame.rows.first(), frame.rows.last()) else {
            return out;
        };
        let decorations = view.model.decorations(first.start..last.end);
        if decorations.is_empty() {
            return out;
        }
        let text = view.model.text();
        let theme = &view.theme;
        let font = gpui::font(theme.font_family.clone());
        let text_bounds = frame.text_bounds();
        let gutter_x = frame.fold_column.x;
        let scale = window.scale_factor();
        let bar = (BAR_WIDTH * scale).round().max(1.) / scale;
        let space = glyph_widths.char_width(' ');
        // Where the next after-line text goes on each display row.
        let mut after: HashMap<u32, f32> = HashMap::new();
        let mut placed = Vec::new();
        for decoration in decorations {
            let covered = decoration.covered_rows(text);
            // The frame's display rows of the covered rows, by buffer row.
            let from = (frame.rows).partition_point(|row| row.buffer_row < *covered.start());
            let to = (frame.rows).partition_point(|row| row.buffer_row <= *covered.end());
            let rows = &frame.rows[from..to];
            let tint = tint_color(theme, decoration.tint);
            let range = decoration.range;
            let mut place = |rect: Rect, color: Hsla, text: Option<Line>| {
                placed.push((decoration.kind.clone(), rect, color, text));
            };
            let whole_row = |row: u32, x: f32, width: f32| Rect {
                x,
                y: frame.row_top(row),
                width,
                height: line_height,
            };
            match &decoration.kind {
                DecorationKind::RowBackground => {
                    for row in rows {
                        let rect = whole_row(row.row, text_bounds.x, text_bounds.width);
                        place(rect, tint.opacity(ROW_BACKGROUND_ALPHA), None);
                    }
                }
                DecorationKind::Gutter(GutterMark::Bar) => {
                    for row in rows {
                        let rect = whole_row(row.row, gutter_x, bar);
                        place(rect, tint.opacity(GUTTER_ALPHA), None);
                    }
                }
                DecorationKind::Gutter(GutterMark::Deletion) => {
                    // On the top edge of the first covered row, or, past the
                    // end of a text whose last row isn't empty, the bottom
                    // edge of that row.
                    let row_start = text.row_start(*covered.start());
                    let edge = match range.start == text.len() && range.start > row_start {
                        true => (rows.iter().rfind(|row| row.last))
                            .map(|row| frame.row_top(row.row) + line_height),
                        false => (rows.iter().find(|row| row.start == row_start))
                            .map(|row| frame.row_top(row.row)),
                    };
                    let Some(edge) = edge else {
                        continue;
                    };
                    let (width, height) = DELETION;
                    let rect = Rect {
                        x: gutter_x,
                        y: edge - height / 2.,
                        width,
                        height,
                    };
                    place(rect, tint.opacity(GUTTER_ALPHA), None);
                }
                DecorationKind::Highlight | DecorationKind::Underline { .. } => {
                    for row in rows {
                        let (start, end) = (range.start.max(row.start), range.end.min(row.end));
                        // An empty one only on the row its caret would be
                        // on.
                        let on_row = match range.is_empty() {
                            true => row.contains(range.start),
                            false => start < end,
                        };
                        if !on_row {
                            continue;
                        }
                        let left = frame.view_x(row.x_for_offset(start, long_rows, glyph_widths));
                        let width = match range.is_empty() {
                            true => space,
                            false => {
                                frame.view_x(row.x_for_offset(end, long_rows, glyph_widths)) - left
                            }
                        };
                        if decoration.kind == DecorationKind::Highlight {
                            place(
                                whole_row(row.row, left, width),
                                tint.opacity(HIGHLIGHT_ALPHA),
                                None,
                            );
                            continue;
                        }
                        // Just below the baseline, as diagnostics' squiggles.
                        let line = &row.line.layout;
                        let (ascent, descent): (f32, f32) =
                            (line.ascent.into(), line.descent.into());
                        let baseline =
                            frame.row_top(row.row) + (line_height - ascent - descent) / 2. + ascent;
                        let rect = Rect {
                            x: left,
                            y: baseline.round() + 1.,
                            width,
                            height: UNDERLINE,
                        };
                        place(rect, tint, None);
                    }
                }
                DecorationKind::AfterLine(label) => {
                    let Some(row) = rows
                        .iter()
                        .rfind(|row| row.buffer_row == *covered.end() && row.last)
                    else {
                        continue;
                    };
                    let x = after.entry(row.row).or_insert_with(|| {
                        frame.view_x(row.x_for_offset(row.end, long_rows, glyph_widths)) + em
                    });
                    let label = label.replace(['\n', '\r'], " ");
                    let color = tint.opacity(AFTER_LINE_ALPHA);
                    let run = TextRun {
                        len: label.len(),
                        font: font.clone(),
                        color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let shaped =
                        window
                            .text_system()
                            .layout_line(&label, theme.font_size, &[run], None);
                    let width: f32 = shaped.width.into();
                    let line = Line {
                        layout: shaped,
                        decorations: vec![DecorationRun {
                            len: label.len() as u32,
                            color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                    };
                    place(whole_row(row.row, *x, width), color, Some(line));
                    *x += width + em;
                }
            }
        }
        out.placed = (placed.into_iter())
            .map(|(kind, rect, color, text)| Placed {
                kind,
                bounds: layout.bounds(rect),
                color,
                text,
            })
            .collect();
        out
    }

    /// Paints the decorations of `layer`.
    pub(super) fn paint(&self, layer: Layer, window: &mut Window, cx: &mut App) {
        let of = |pick: fn(&DecorationKind) -> bool| {
            (self.placed.iter()).filter(move |placed| pick(&placed.kind))
        };
        let mut painted = false;
        match layer {
            Layer::Backgrounds => {
                let backgrounds = of(|kind| *kind == DecorationKind::RowBackground);
                let highlights = of(|kind| *kind == DecorationKind::Highlight);
                for placed in backgrounds.chain(highlights) {
                    window.paint_quad(fill(placed.bounds, placed.color));
                    painted = true;
                }
            }
            Layer::Underlines => {
                for placed in of(|kind| matches!(kind, DecorationKind::Underline { .. })) {
                    let DecorationKind::Underline { wavy } = placed.kind else {
                        continue;
                    };
                    let style = UnderlineStyle {
                        thickness: placed.bounds.size.height,
                        color: Some(placed.color),
                        wavy,
                    };
                    window.paint_underline(placed.bounds.origin, placed.bounds.size.width, &style);
                    painted = true;
                }
            }
            Layer::AfterLine => {
                for placed in &self.placed {
                    if let Some(line) = &placed.text {
                        line.paint(placed.bounds.origin, self.line_height, window, cx);
                        painted = true;
                    }
                }
            }
            Layer::Gutter => {
                for placed in of(|kind| matches!(kind, DecorationKind::Gutter(_))) {
                    window.paint_quad(fill(placed.bounds, placed.color));
                    painted = true;
                }
            }
        }
        #[cfg(test)]
        if painted {
            note(match layer {
                Layer::Backgrounds => "backgrounds",
                Layer::Underlines => "underlines",
                Layer::AfterLine => "after line",
                Layer::Gutter => "gutter",
            });
        }
        #[cfg(not(test))]
        let _ = painted;
    }
}

#[cfg(test)]
thread_local! {
    /// What this thread's frames painted, in order: each decoration layer
    /// that painted something, and the stages of the frame noted around
    /// them.
    pub(crate) static PAINTED: std::cell::RefCell<Vec<&'static str>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Notes a stage of the frame in [`PAINTED`].
#[cfg(test)]
pub(super) fn note(stage: &'static str) {
    PAINTED.with(|painted| painted.borrow_mut().push(stage));
}

#[cfg(test)]
#[path = "decorations_tests.rs"]
mod tests;
