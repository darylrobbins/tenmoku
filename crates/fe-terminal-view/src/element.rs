//! Paints a [`TerminalView`]'s grid: shapes each [`layout`] run with the
//! window's text system and paints it, with the backgrounds, selection and
//! cursor, as quads, and an input method's marked text over the cursor. Takes
//! text input while the view is focused, and the mouse over it.

use std::ops::Range;

use fe_terminal::GridSize;
use gpui::{
    App, BorderStyle, Bounds, ContentMask, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, Font, FontStyle, FontWeight, GlobalElementId, Hitbox, HitboxBehavior, Hsla,
    InspectorElementId, IntoElement, LayoutId, Pixels, Point, ShapedLine, SharedString, Size,
    Style, TextAlign, TextRun, UnderlineStyle, Window, fill, outline, point, px, relative, size,
};

use crate::layout::{self, CursorLayout, Run};
use crate::mouse::{self, Frame};
use crate::{TerminalStyle, TerminalView};

/// The beam's width and the underline cursor's height.
const BAR: Pixels = px(2.);

pub(crate) struct TerminalElement {
    view: Entity<TerminalView>,
}

impl TerminalElement {
    pub(crate) fn new(view: Entity<TerminalView>) -> Self {
        Self { view }
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

/// A cell's size in `style`'s font: the advance of `m` wide, and the font
/// size times the line height, in whole pixels, high.
pub fn cell_size(style: &TerminalStyle, window: &Window) -> Size<Pixels> {
    let text_system = window.text_system();
    let font_id = text_system.resolve_font(&gpui::font(style.font_family.clone()));
    let width = text_system
        .advance(font_id, style.font_size, 'm')
        .map(|advance| advance.width)
        .unwrap_or(style.font_size * 0.6);
    size(width, (style.font_size * style.line_height).round())
}

/// The grid that fits in `bounds`, at least 2 × 1.
fn grid_size(bounds: Size<Pixels>, cell: Size<Pixels>) -> GridSize {
    let fit = |length: Pixels, cell: Pixels, min: u16| {
        let cells = (f32::from(length) / f32::from(cell).max(1.)).floor();
        (cells.min(f32::from(u16::MAX)) as u16).max(min)
    };
    let pixels = |length: Pixels| f32::from(length).round().clamp(1., f32::from(u16::MAX)) as u16;
    GridSize {
        cols: fit(bounds.width, cell.width, 2),
        rows: fit(bounds.height, cell.height, 1),
        cell_width: pixels(cell.width),
        cell_height: pixels(cell.height),
    }
}

/// A shaped run, at its window position.
struct Text {
    origin: Point<Pixels>,
    line: ShapedLine,
}

/// The cursor cell's character, shaped, and its decoration quads.
struct Glyph {
    text: Vec<Text>,
    quads: Vec<(Bounds<Pixels>, Hsla)>,
}

enum Cursor {
    Block {
        bounds: Bounds<Pixels>,
        color: Hsla,
        glyph: Option<Glyph>,
    },
    Hollow {
        bounds: Bounds<Pixels>,
        color: Hsla,
    },
    Bar {
        bounds: Bounds<Pixels>,
        color: Hsla,
    },
}

/// A shaped cell's text and where it is painted.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct PaintedText {
    pub origin: Point<Pixels>,
    pub text: String,
    /// The shaped line's natural width.
    pub width: Pixels,
    /// The font size it was shaped at: smaller than the style's when a
    /// symbol was scaled to fit its cells.
    pub font_size: Pixels,
    /// Each shaped glyph's byte index in `text` and x offset from `origin`.
    pub glyphs: Vec<(usize, Pixels)>,
}

/// The cursor as last painted.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PaintedCursor {
    Block(Bounds<Pixels>),
    Hollow(Bounds<Pixels>),
    Bar(Bounds<Pixels>),
}

/// What the last prepaint laid out, for tests to check the production
/// shaping and painting path with. Only recorded after
/// [`TerminalView::record_paint`].
#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaintReport {
    /// Each shaped cell of the grid's text.
    pub text: Vec<PaintedText>,
    /// Backgrounds, selection, underlines and strikeouts.
    pub quads: Vec<Bounds<Pixels>>,
    /// The find query's matches, a quad per row they cover.
    pub find_quads: Vec<Bounds<Pixels>>,
    pub cursor: Option<PaintedCursor>,
    /// A block cursor's character, and its underline and strikeout.
    pub cursor_text: Vec<PaintedText>,
    pub cursor_quads: Vec<Bounds<Pixels>>,
}

pub(crate) struct Prepainted {
    cell: Size<Pixels>,
    hitbox: Hitbox,
    /// What the mouse listeners map positions and encode reports by.
    frame: Frame,
    background: Hsla,
    quads: Vec<(Bounds<Pixels>, Hsla)>,
    /// The part of `quads` that's find highlights.
    find: Range<usize>,
    text: Vec<Text>,
    cursor: Option<Cursor>,
    /// Marked text over its background, at the cursor.
    marked: Option<(Bounds<Pixels>, Text)>,
}

impl Prepainted {
    fn report(&self) -> PaintReport {
        let texts = |texts: &[Text]| {
            texts
                .iter()
                .map(|text| PaintedText {
                    origin: text.origin,
                    text: text.line.text.to_string(),
                    width: text.line.width,
                    font_size: text.line.font_size,
                    glyphs: text
                        .line
                        .runs
                        .iter()
                        .flat_map(|run| &run.glyphs)
                        .map(|glyph| (glyph.index, glyph.position.x))
                        .collect(),
                })
                .collect::<Vec<_>>()
        };
        let (cursor, cursor_text, cursor_quads) = match &self.cursor {
            Some(Cursor::Block { bounds, glyph, .. }) => (
                Some(PaintedCursor::Block(*bounds)),
                glyph.as_ref().map(|g| texts(&g.text)).unwrap_or_default(),
                glyph
                    .as_ref()
                    .map(|g| g.quads.iter().map(|(quad, _)| *quad).collect())
                    .unwrap_or_default(),
            ),
            Some(Cursor::Hollow { bounds, .. }) => {
                (Some(PaintedCursor::Hollow(*bounds)), Vec::new(), Vec::new())
            }
            Some(Cursor::Bar { bounds, .. }) => {
                (Some(PaintedCursor::Bar(*bounds)), Vec::new(), Vec::new())
            }
            None => (None, Vec::new(), Vec::new()),
        };
        PaintReport {
            text: texts(&self.text),
            quads: self
                .quads
                .iter()
                .enumerate()
                .filter(|(i, _)| !self.find.contains(i))
                .map(|(_, (quad, _))| *quad)
                .collect(),
            find_quads: self.quads[self.find.clone()]
                .iter()
                .map(|(quad, _)| *quad)
                .collect(),
            cursor,
            cursor_text,
            cursor_quads,
        }
    }
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepainted {
        let style = self.view.read(cx).style.clone();
        let cell = cell_size(&style, window);
        let grid = grid_size(bounds.size, cell);
        // A window that's lost focus to another app keeps its focus handle.
        let focused = self.view.read(cx).is_focused(window);
        let marked = self.view.read(cx).marked.clone();
        let record = self.view.read(cx).record_paint;
        let mut frame = Frame {
            bounds,
            cell,
            rows: usize::from(grid.rows),
            cols: usize::from(grid.cols),
            modes: Default::default(),
        };
        let (snapshot, drag_scroll) = self.view.update(cx, |view, cx| {
            view.fit(grid, cx);
            let drag_scroll = view.drag_scroll(&frame);
            (
                view.terminal().map(|terminal| terminal.snapshot()),
                drag_scroll,
            )
        });
        if let Some(snapshot) = &snapshot {
            frame.modes = snapshot.modes;
            // Keep scrolling while the drag is held past an end there's more
            // history toward.
            let more = match drag_scroll {
                -1 => snapshot.display_offset < snapshot.history_size,
                1 => snapshot.display_offset > 0,
                _ => false,
            };
            if more {
                window.request_animation_frame();
            }
        }
        let mut prepainted = Prepainted {
            cell,
            hitbox: window.insert_hitbox(bounds, HitboxBehavior::Normal),
            frame,
            background: style.background,
            quads: Vec::new(),
            find: 0..0,
            text: Vec::new(),
            cursor: None,
            marked: None,
        };
        let Some(snapshot) = snapshot else {
            return prepainted;
        };

        let at = |row: usize, col: usize| {
            bounds.origin + point(cell.width * col as f32, cell.height * row as f32)
        };
        let cells = |row: usize, col: usize, cells: usize| {
            Bounds::new(at(row, col), size(cell.width * cells as f32, cell.height))
        };
        let font = gpui::font(style.font_family.clone());
        // One shaped line per cell, at its column: no glyph's advance moves
        // the next, and no ligature spans two cells.
        let shape = |row: usize, run: &Run, window: &mut Window| -> Vec<Text> {
            run.clusters()
                .filter(|(_, text, _)| !text.trim_start_matches(' ').is_empty())
                .map(|(col, text, _)| {
                    let origin = at(row, col);
                    let line = shape_cluster(text, run, &font, style.font_size, window);
                    let Some(symbol) = text.chars().next().filter(|&ch| is_symbol(ch)) else {
                        return Text { origin, line };
                    };
                    let blank = blank_after(&snapshot, row, col);
                    let Some((span, scale)) = fit_symbol(symbol, line.width, cell.width, blank)
                    else {
                        return Text { origin, line };
                    };
                    let line = if scale < 1. {
                        shape_cluster(text, run, &font, style.font_size * scale, window)
                    } else {
                        line
                    };
                    // Powerline symbols stay at the cell's left edge; other
                    // symbols are centred in their span.
                    let left = if is_powerline(symbol) {
                        px(0.)
                    } else {
                        (cell.width * span as f32 - line.width) / 2.
                    };
                    Text {
                        origin: origin + point(left, px(0.)),
                        line,
                    }
                })
                .collect()
        };
        // Underline and strikeout span the cells the run occupies, not the
        // glyphs' natural width.
        let decorations = |row: usize, run: &Run, quads: &mut Vec<(Bounds<Pixels>, Hsla)>| {
            let line = |offset: f32| {
                let top = at(row, run.col).y + (cell.height * offset).floor();
                Bounds::new(
                    point(at(row, run.col).x, top),
                    size(cell.width * run.cells as f32, px(1.)),
                )
            };
            if run.underline {
                quads.push((line(0.9), run.fg));
            }
            if run.strikeout {
                quads.push((line(0.5), run.fg));
            }
        };

        let rows = layout::rows(&snapshot, &style, focused);
        for (row, layout) in rows.iter().enumerate() {
            for (range, colour) in &layout.backgrounds {
                prepainted
                    .quads
                    .push((cells(row, range.start, range.len()), *colour));
            }
        }
        // Under the selection, which the active match shows through on top
        // of.
        let find = self.view.read(cx).find.clone();
        if let Some(query) = find {
            let matches = snapshot_terminal_matches(&self.view, &query, cx);
            self.view
                .update(cx, |view, _| view.find_matches = matches.clone());
            let mut colour = style.selection;
            colour.a *= 0.5;
            let start = prepainted.quads.len();
            for found in &matches {
                let first = found.start.line + snapshot.display_offset as i32;
                let last = found.end.line + snapshot.display_offset as i32;
                for row in first.max(0)..=last.min(snapshot.rows.len() as i32 - 1) {
                    let from = if row == first { found.start.col } else { 0 };
                    let to = if row == last {
                        found.end.col + 1
                    } else {
                        snapshot.cols
                    };
                    let to = to.min(snapshot.cols);
                    if to > from {
                        prepainted
                            .quads
                            .push((cells(row as usize, from, to - from), colour));
                    }
                }
            }
            prepainted.find = start..prepainted.quads.len();
        } else {
            self.view.update(cx, |view, _| view.find_matches.clear());
        }
        for span in &snapshot.selection {
            let width = span.end_col.saturating_sub(span.start_col);
            prepainted
                .quads
                .push((cells(span.row, span.start_col, width), style.selection));
        }
        for (row, layout) in rows.iter().enumerate() {
            for run in &layout.runs {
                prepainted.text.extend(shape(row, run, window));
                decorations(row, run, &mut prepainted.quads);
            }
        }
        prepainted.cursor = match layout::cursor(&snapshot, &style, focused) {
            CursorLayout::Block {
                row,
                col,
                cells: width,
                color,
                glyph,
            } => Some(Cursor::Block {
                bounds: cells(row, col, width),
                color,
                glyph: glyph.map(|glyph| {
                    let mut quads = Vec::new();
                    decorations(row, &glyph, &mut quads);
                    Glyph {
                        text: shape(row, &glyph, window),
                        quads,
                    }
                }),
            }),
            CursorLayout::Hollow {
                row,
                col,
                cells: width,
                color,
            } => Some(Cursor::Hollow {
                bounds: cells(row, col, width),
                color,
            }),
            CursorLayout::Beam { row, col, color } => Some(Cursor::Bar {
                bounds: Bounds::new(at(row, col), size(BAR, cell.height)),
                color,
            }),
            CursorLayout::Underline {
                row,
                col,
                cells: width,
                color,
            } => Some(Cursor::Bar {
                bounds: Bounds::new(
                    at(row, col) + point(px(0.), cell.height - BAR),
                    size(cell.width * width as f32, BAR),
                ),
                color,
            }),
            CursorLayout::Hidden => None,
        };
        if let Some(text) = marked.filter(|_| focused) {
            let row = snapshot
                .cursor
                .row
                .min(snapshot.rows.len().saturating_sub(1));
            let col = snapshot.cursor.col.min(snapshot.cols.saturating_sub(1));
            let line = shape_marked(text, &font, &style, window);
            let width = line.width.max(cell.width);
            prepainted.marked = Some((
                Bounds::new(at(row, col), size(width, cell.height)),
                Text {
                    origin: at(row, col),
                    line,
                },
            ));
        }
        if record {
            let report = prepainted.report();
            self.view.update(cx, |view, _| view.painted = report);
        }
        prepainted
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepainted: &mut Prepainted,
        window: &mut Window,
        cx: &mut App,
    ) {
        let line_height = prepainted.cell.height;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, prepainted.background));
            for &(quad, colour) in &prepainted.quads {
                window.paint_quad(fill(quad, colour));
            }
            for text in &prepainted.text {
                paint_text(text, line_height, window, cx);
            }
            match &prepainted.cursor {
                Some(Cursor::Block {
                    bounds,
                    color,
                    glyph,
                }) => {
                    window.paint_quad(fill(*bounds, *color));
                    if let Some(Glyph { text, quads }) = glyph {
                        for text in text {
                            paint_text(text, line_height, window, cx);
                        }
                        for &(quad, colour) in quads {
                            window.paint_quad(fill(quad, colour));
                        }
                    }
                }
                Some(Cursor::Hollow { bounds, color }) => {
                    window.paint_quad(outline(*bounds, *color, BorderStyle::Solid));
                }
                Some(Cursor::Bar { bounds, color }) => {
                    window.paint_quad(fill(*bounds, *color));
                }
                None => {}
            }
            if let Some((bounds, text)) = &prepainted.marked {
                window.paint_quad(fill(*bounds, prepainted.background));
                paint_text(text, line_height, window, cx);
            }
        });
        let cursor = if mouse::reporting(&prepainted.frame.modes) {
            CursorStyle::Arrow
        } else {
            CursorStyle::IBeam
        };
        window.set_cursor_style(cursor, &prepainted.hitbox);
        mouse::listen(&self.view, &prepainted.hitbox, prepainted.frame, window);
        let focus = self.view.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
    }
}

/// `query`'s matches on screen; none when it doesn't compile.
fn snapshot_terminal_matches(
    view: &Entity<TerminalView>,
    query: &fe_terminal::TerminalQuery,
    cx: &App,
) -> Vec<fe_terminal::TerminalMatch> {
    view.read(cx)
        .terminal()
        .and_then(|terminal| terminal.find_all_visible(query).ok())
        .unwrap_or_default()
}

/// Shapes an input method's marked text, underlined, in its natural widths:
/// it isn't on the grid yet.
fn shape_marked(
    text: String,
    font: &Font,
    style: &TerminalStyle,
    window: &mut Window,
) -> ShapedLine {
    let run = TextRun {
        len: text.len(),
        font: font.clone(),
        color: style.foreground,
        background_color: None,
        underline: Some(UnderlineStyle {
            thickness: px(1.),
            color: Some(style.foreground),
            wavy: false,
        }),
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(SharedString::from(text), style.font_size, &[run], None)
}

/// Whether `ch` is in a private-use range, where Nerd Font icons and
/// Powerline symbols live.
fn is_symbol(ch: char) -> bool {
    matches!(ch, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}' | '\u{100000}'..='\u{10FFFD}')
}

/// Whether `ch` is a Powerline separator or arrow.
fn is_powerline(ch: char) -> bool {
    matches!(ch, '\u{E0A0}'..='\u{E0D7}')
}

/// Whether the cell after `col` on `row` is blank with the default
/// background, so a symbol may spill into it.
fn blank_after(snapshot: &fe_terminal::Snapshot, row: usize, col: usize) -> bool {
    snapshot
        .rows
        .get(row)
        .and_then(|cells| cells.get(col + 1))
        .is_some_and(|cell| {
            cell.ch == ' '
                && cell.extra.is_none()
                && cell.bg == fe_terminal::Color::Background
                && !cell.flags.contains(fe_terminal::CellFlags::INVERSE)
        })
}

/// How a private-use symbol `ch`, shaped `natural` wide, fits `cell`-wide
/// cells: the cells it spans and the factor to scale its font size by.
/// `None` when it already fits one cell and stays at the cell's origin.
/// Powerline symbols always take one cell; other symbols take two when
/// `next_blank`, else one. The scale is below 1 only when the glyph is wider
/// than its span, and no lower than a half.
fn fit_symbol(ch: char, natural: Pixels, cell: Pixels, next_blank: bool) -> Option<(usize, f32)> {
    if !is_symbol(ch) || natural <= cell {
        return None;
    }
    let span = if next_blank && !is_powerline(ch) {
        2
    } else {
        1
    };
    let room = cell * span as f32;
    let scale = if natural > room {
        (f32::from(room) / f32::from(natural)).max(0.5)
    } else {
        1.
    };
    Some((span, scale))
}

/// Shapes one cell's `text` in `run`'s style at `font_size`. The
/// decorations are painted by the caller across the cells.
fn shape_cluster(
    text: &str,
    run: &Run,
    font: &Font,
    font_size: Pixels,
    window: &mut Window,
) -> ShapedLine {
    let mut font = font.clone();
    if run.bold {
        font.weight = FontWeight::BOLD;
    }
    if run.italic {
        font.style = FontStyle::Italic;
    }
    let text_run = TextRun {
        len: text.len(),
        font,
        color: run.fg,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window.text_system().shape_line(
        SharedString::from(text.to_owned()),
        font_size,
        &[text_run],
        None,
    )
}

fn paint_text(text: &Text, line_height: Pixels, window: &mut Window, cx: &mut App) {
    if let Err(err) = text
        .line
        .paint(text.origin, line_height, TextAlign::Left, None, window, cx)
    {
        log::warn!("couldn't paint terminal text: {err}");
    }
}
