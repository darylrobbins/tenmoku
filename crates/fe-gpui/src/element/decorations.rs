//! The decorations an editor's providers hand out
//! ([`fe_editor::decorations`]), laid out on a frame's display rows and
//! painted: row backgrounds, gutter marks, highlights, underlines and text
//! after a line. Each [`Tint`] resolves through the theme
//! ([`tint_color`]).

use std::{cell::RefCell, collections::HashMap, ops::Range};

use fe_display::{
    CharWidth as _,
    layout::{Rect, RowLayout},
};
use fe_editor::decorations::{Decoration, DecorationKind, GutterMark, Tint};
use gpui::{
    App, Bounds, DecorationRun, Hsla, PathBuilder, Pixels, Point, TextRun, UnderlineStyle, Window,
    fill, point, px,
};

use super::{FrameLayout, Line};
use crate::{EditorTheme, EditorView};

/// How opaque each kind is over its tint: a row background faint, a
/// highlight as a search match ([`super::search_match_color`]).
const ROW_BACKGROUND_ALPHA: f32 = 0.08;
const HIGHLIGHT_ALPHA: f32 = 0.22;
const GUTTER_ALPHA: f32 = 0.6;
/// A deletion wedge is drawn in the full colour, to stand out from the bars.
const DELETION_ALPHA: f32 = 1.0;
const AFTER_LINE_ALPHA: f32 = 0.6;

/// A gutter bar's width, in pixels, rounded to device pixels. Bars and
/// deletion wedges sit in the frame's change lane
/// ([`fe_display::layout::Frame::change_lane`]), clear of the line numbers
/// and the chevrons; a bar has rounded ends.
const BAR_WIDTH: f32 = 3.;
/// How far a gutter bar stops short of each end of its run of rows, in
/// pixels, so the bars of two adjacent changes show a gap between them.
const BAR_INSET: f32 = 1.;
/// An underline's thickness, in pixels.
const UNDERLINE: f32 = 1.;

/// The providers' decorations already asked for in a frame, by the range
/// asked for, so the highlight spans and the painted decorations share a query.
#[derive(Default)]
pub(crate) struct Queried(RefCell<Vec<(Range<usize>, Vec<Decoration>)>>);

impl Queried {
    /// The decorations in `range`, asked of the providers the first time.
    pub(crate) fn get(&self, view: &EditorView, range: Range<usize>) -> Vec<Decoration> {
        let found = (self.0.borrow().iter())
            .find(|(asked, _)| *asked == range)
            .map(|(_, decorations)| decorations.clone());
        found.unwrap_or_else(|| {
            let decorations = view.model.decorations(range.clone());
            self.0.borrow_mut().push((range, decorations.clone()));
            decorations
        })
    }
}

/// What each [`DecorationKind`] resolves its tint to.
fn tint_color(theme: &EditorTheme, tint: Tint) -> Hsla {
    match tint {
        Tint::Error => theme.error,
        Tint::Warning => theme.warning,
        Tint::Information => theme.information,
        Tint::Added => theme.change_added,
        Tint::Modified => theme.change_modified,
        Tint::Removed => theme.change_deleted,
        Tint::Hint => theme.hint,
        Tint::Accent => theme.cursor,
        Tint::Muted => theme.foreground,
    }
}

/// A decoration laid out on one display row.
#[derive(Clone)]
pub(crate) struct Placed {
    pub(crate) kind: DecorationKind,
    /// In window coordinates. An underline's height is its thickness.
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) color: Hsla,
    /// Half the smaller side of a gutter mark, rounding its ends; zero for
    /// every other kind.
    pub(crate) corner_radius: Pixels,
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
        queried: &Queried,
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
        let decorations = queried.get(view, first.start..last.end);
        if decorations.is_empty() {
            return out;
        }
        let text = view.model.text();
        let theme = &view.theme;
        let font = gpui::font(theme.font_family.clone());
        let text_bounds = frame.text_bounds();
        let lane = frame.change_lane;
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
                    // One bar per run of rows shown one under the next: a
                    // soft-wrapped row's pieces join the run, a block
                    // between two rows ends it.
                    let screen_row = |row: &RowLayout<Line>| frame.block_map.screen_row(row.row);
                    let mut at = 0;
                    while at < rows.len() {
                        let first = &rows[at];
                        let mut run = 1;
                        while (rows.get(at + run))
                            .is_some_and(|row| screen_row(row) == screen_row(first) + run as u32)
                        {
                            run += 1;
                        }
                        at += run;
                        let rect = Rect {
                            x: lane.x,
                            y: frame.row_top(first.row) + BAR_INSET,
                            width: bar,
                            height: run as f32 * line_height - 2. * BAR_INSET,
                        };
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
                    // A wedge half a line tall centred on the edge, its base
                    // at the bars' left edge, its tip pointing at the text.
                    let rect = Rect {
                        x: lane.x,
                        y: edge - line_height / 4.,
                        width: lane.width / 2.,
                        height: line_height / 2.,
                    };
                    place(rect, tint.opacity(DELETION_ALPHA), None);
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
                // Recolours text through the highlight spans; paints nothing.
                DecorationKind::Foreground(_) => {}
            }
        }
        out.placed = (placed.into_iter())
            .map(|(kind, rect, color, text)| Placed {
                corner_radius: match kind {
                    DecorationKind::Gutter(GutterMark::Bar) => px(rect.width.min(rect.height) / 2.),
                    _ => px(0.),
                },
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
                for placed in of(|kind| *kind == DecorationKind::Gutter(GutterMark::Bar)) {
                    let quad = fill(placed.bounds, placed.color).corner_radii(placed.corner_radius);
                    window.paint_quad(quad);
                    painted = true;
                }
                // After the bars, so a wedge sits over the end of a bar.
                for placed in of(|kind| *kind == DecorationKind::Gutter(GutterMark::Deletion)) {
                    let [base_top, tip, base_bottom] = wedge_points(placed.bounds);
                    let mut path = PathBuilder::fill();
                    path.move_to(base_top);
                    path.line_to(tip);
                    path.line_to(base_bottom);
                    path.close();
                    if let Ok(path) = path.build() {
                        window.paint_path(path, placed.color);
                    }
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

/// The corners of a deletion wedge in `bounds`: a triangle pointing toward
/// the text, its tip on the middle of the right edge.
fn wedge_points(bounds: Bounds<Pixels>) -> [Point<Pixels>; 3] {
    let (left, right) = (bounds.left(), bounds.right());
    [
        point(left, bounds.top()),
        point(right, bounds.center().y),
        point(left, bounds.bottom()),
    ]
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

#[cfg(test)]
#[path = "change_bar_tests.rs"]
mod change_bar_tests;

#[cfg(test)]
#[path = "change_rail_tests.rs"]
mod change_rail_tests;

#[cfg(test)]
#[path = "deletion_wedge_tests.rs"]
mod deletion_wedge_tests;
