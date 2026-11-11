//! The decorations an editor's providers hand out
//! ([`fe_editor::decorations`]), laid out on a frame's display rows and
//! painted: row backgrounds, gutter marks, highlights, underlines and text
//! after a line. Each [`Tint`] resolves through the theme
//! ([`tint_color`]).

use std::{cell::RefCell, collections::HashMap, ops::Range};

use fe_display::{
    CharWidth as _,
    layout::{MeasuredLine as _, Rect, RowLayout},
};
use fe_editor::decorations::{Decoration, DecorationKind, GutterMark, Tint};
use gpui::{App, Bounds, DecorationRun, Hsla, Pixels, TextRun, UnderlineStyle, Window, fill, px};

use super::{FrameLayout, Line};
use crate::{EditorTheme, EditorView};

/// How opaque each kind is over its tint: a row background faint, a
/// highlight without a colour of its own ([`highlight_color`]) as a search
/// match was before the theme had a slot for one.
const ROW_BACKGROUND_ALPHA: f32 = 0.08;
const HIGHLIGHT_ALPHA: f32 = 0.22;
const GUTTER_ALPHA: f32 = 0.6;
/// A removed line's notch is drawn in the full colour, to stand out from the
/// bars.
const DELETION_ALPHA: f32 = 1.0;
const AFTER_LINE_ALPHA: f32 = 0.6;

/// Bars and removed lines' notches sit at the left of the frame's change
/// lane ([`fe_display::layout::Frame::change_lane`]), clear of the line
/// numbers and the chevrons, a bar as wide as
/// [`super::change_bar_width`] says, rounded to device pixels, with rounded
/// ends. A notch is twice a bar's width across and this much of it tall, on
/// the edge between two rows.
const NOTCH_HEIGHT: f32 = 2. / 3.;
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

/// What each [`DecorationKind`] resolves its tint to, before its kind's
/// fade; a diff tint's band and highlight have colours of their own
/// ([`diff_color`]).
pub(super) fn tint_color(theme: &EditorTheme, tint: Tint) -> Hsla {
    match tint {
        Tint::Error => theme.error,
        Tint::Warning => theme.warning,
        Tint::Information => theme.information,
        Tint::Added | Tint::DiffAdded => theme.change_added,
        Tint::Modified => theme.change_modified,
        Tint::Removed | Tint::DiffRemoved => theme.change_deleted,
        Tint::Hint => theme.hint,
        Tint::Accent => theme.cursor,
        Tint::Yank => theme.vim_yank_highlight,
        Tint::Perf => theme.change_modified,
        Tint::Muted => theme.foreground,
    }
}

/// A diff tint's own colour as a row's band or a word's highlight, painted
/// as the theme has it, unfaded: the words' over their row's are what set
/// them apart. `None` for any other tint or kind.
pub(super) fn diff_color(theme: &EditorTheme, tint: Tint, kind: &DecorationKind) -> Option<Hsla> {
    match (tint, kind) {
        (Tint::DiffAdded, DecorationKind::RowBackground) => Some(theme.diff_added_row),
        (Tint::DiffRemoved, DecorationKind::RowBackground) => Some(theme.diff_removed_row),
        (Tint::DiffAdded, DecorationKind::Highlight) => Some(theme.diff_added_word),
        (Tint::DiffRemoved, DecorationKind::Highlight) => Some(theme.diff_removed_word),
        (Tint::Yank, DecorationKind::Highlight) => Some(theme.vim_yank_highlight),
        _ => None,
    }
}

/// A highlight's colour of its own, painted as the theme has it, unfaded:
/// a muted highlight is a use of the identifier under the caret (the
/// occurrences' and a language server's document highlights, read or
/// text), drawn in `identifier_highlight`. `None` for any other tint, which
/// takes its tint faded ([`HIGHLIGHT_ALPHA`]).
pub(super) fn highlight_color(theme: &EditorTheme, tint: Tint) -> Option<Hsla> {
    (tint == Tint::Muted).then_some(theme.identifier_highlight)
}

/// The colour the theme gives the highlight `name`, if it has one.
pub(crate) fn name_color(theme: &EditorTheme, name: &str) -> Option<Hsla> {
    fe_syntax::Highlight::from_capture(name).and_then(|highlight| theme.syntax_color(highlight))
}

/// A decoration laid out on one display row.
#[derive(Clone)]
pub(crate) struct Placed {
    pub(crate) kind: DecorationKind,
    /// In window coordinates. An underline's height is its thickness.
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) color: Hsla,
    /// Half the smaller side of a gutter mark, a bar or a notch, rounding its
    /// ends; zero for
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
        let font = theme.font();
        let text_bounds = frame.text_bounds();
        let lane = frame.change_lane;
        let scale = window.scale_factor();
        let bar = (super::change_bar_width(lane.width) * scale)
            .round()
            .max(1.)
            / scale;
        let notch = (bar * NOTCH_HEIGHT * scale).round().max(1.) / scale;
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
            let tint = match &decoration.kind {
                DecorationKind::SyntaxUnderline(name) => {
                    name_color(theme, name).unwrap_or(theme.foreground)
                }
                _ => tint_color(theme, decoration.tint),
            };
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
                        let color = diff_color(theme, decoration.tint, &decoration.kind)
                            .unwrap_or(tint.opacity(ROW_BACKGROUND_ALPHA));
                        place(rect, color, None);
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
                    // A notch centred on the edge, from the bars' left edge
                    // toward the text.
                    let rect = Rect {
                        x: lane.x,
                        y: edge - notch / 2.,
                        width: bar * 2.,
                        height: notch,
                    };
                    place(rect, tint.opacity(DELETION_ALPHA), None);
                }
                DecorationKind::Highlight
                | DecorationKind::Underline { .. }
                | DecorationKind::SyntaxUnderline(_) => {
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
                        // A syntax underline without a colour of its own
                        // takes the colour of the text under it, run by run.
                        let own = match &decoration.kind {
                            DecorationKind::SyntaxUnderline(name) => name_color(theme, name),
                            _ => Some(tint),
                        };
                        // Each piece: its bytes, its colour, and whether its
                        // left edge is where its first character starts, past
                        // any hint or inlay inserted before it.
                        let mut pieces = vec![(start, end, tint, false)];
                        let shown = row.shown();
                        // Where the character at `offset` starts.
                        let display = |offset: usize| {
                            let col = offset - shown.start;
                            let column = row.expanded.to_display_after_hints(col);
                            match row.expanded.inlay_range() {
                                Some(inlay) if inlay.start == column => inlay.end,
                                _ => column,
                            }
                        };
                        if own.is_none() && !range.is_empty() {
                            pieces.clear();
                            let color_at = |offset: usize| {
                                (shown.start..shown.end)
                                    .contains(&offset)
                                    .then(|| row.line.color_at(display(offset)))
                                    .flatten()
                                    .unwrap_or(tint)
                            };
                            // Inserted text sits before the character at
                            // `offset`: the underline stops short of it.
                            let inserted = |offset: usize| {
                                (shown.start..shown.end).contains(&offset)
                                    && display(offset)
                                        != row.expanded.to_display(offset - shown.start)
                            };
                            let mut at = start;
                            while at < end {
                                let color = color_at(at);
                                let mut next = at + 1;
                                while next < end
                                    && (!text.is_char_boundary(next)
                                        || (!inserted(next) && color_at(next) == color))
                                {
                                    next += 1;
                                }
                                pieces.push((at, next, color, true));
                                at = next;
                            }
                        }
                        for (start, end, tint, past_inserted) in pieces {
                            let left = match past_inserted
                                && (shown.start..shown.end).contains(&start)
                            {
                                true => {
                                    row.start_x() + f64::from(row.line.x_for_index(display(start)))
                                }
                                false => row.x_for_offset(start, long_rows, glyph_widths),
                            };
                            let left = frame.view_x(left);
                            let width = match range.is_empty() {
                                true => space,
                                false => {
                                    frame.view_x(row.x_for_offset(end, long_rows, glyph_widths))
                                        - left
                                }
                            };
                            if decoration.kind == DecorationKind::Highlight {
                                let color = diff_color(theme, decoration.tint, &decoration.kind)
                                    .or_else(|| highlight_color(theme, decoration.tint))
                                    .unwrap_or(tint.opacity(HIGHLIGHT_ALPHA));
                                place(whole_row(row.row, left, width), color, None);
                                continue;
                            }
                            // Just below the baseline, as diagnostics' squiggles.
                            let line = &row.line.layout;
                            let (ascent, descent): (f32, f32) =
                                (line.ascent.into(), line.descent.into());
                            let baseline = frame.row_top(row.row)
                                + (line_height - ascent - descent) / 2.
                                + ascent;
                            let rect = Rect {
                                x: left,
                                y: baseline.round() + 1.,
                                width,
                                height: UNDERLINE,
                            };
                            place(rect, tint, None);
                        }
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
                    let line = Line::with_advances(
                        &label,
                        shaped,
                        vec![DecorationRun {
                            len: label.len() as u32,
                            color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                        |font_id, ch| {
                            super::char_advance(window.text_system(), font_id, theme.font_size, ch)
                        },
                    );
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
                    DecorationKind::Gutter(_) => px(rect.width.min(rect.height) / 2.),
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
                let underlined = |kind: &DecorationKind| {
                    matches!(
                        kind,
                        DecorationKind::Underline { .. } | DecorationKind::SyntaxUnderline(_)
                    )
                };
                for placed in of(underlined) {
                    let style = underline_style(placed);
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
                // After the bars, so a notch sits over the end of a bar.
                for placed in of(|kind| *kind == DecorationKind::Gutter(GutterMark::Deletion)) {
                    let quad = fill(placed.bounds, placed.color).corner_radii(placed.corner_radius);
                    window.paint_quad(quad);
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

/// How a placed underline is painted: wavy only for a wavy `Underline`.
fn underline_style(placed: &Placed) -> UnderlineStyle {
    UnderlineStyle {
        thickness: placed.bounds.size.height,
        color: Some(placed.color),
        wavy: matches!(placed.kind, DecorationKind::Underline { wavy: true }),
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

#[cfg(test)]
#[path = "syntax_underline_tests.rs"]
mod syntax_underline_tests;

#[cfg(test)]
#[path = "syntax_underline_color_tests.rs"]
mod syntax_underline_color_tests;

#[cfg(test)]
#[path = "change_bar_tests.rs"]
mod change_bar_tests;

#[cfg(test)]
#[path = "change_rail_tests.rs"]
mod change_rail_tests;

#[cfg(test)]
#[path = "deletion_wedge_tests.rs"]
mod deletion_wedge_tests;
