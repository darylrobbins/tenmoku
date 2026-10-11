//! Paints a [`TerminalView`]'s grid: shapes each [`layout`] run with the
//! window's text system and paints it, with the backgrounds, selection and
//! cursor, as quads, and an input method's marked text over the cursor. Takes
//! text input while the view is focused, and the mouse over it.

use fe_terminal::GridSize;
use gpui::{
    App, BorderStyle, Bounds, ContentMask, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, Font, FontStyle, FontWeight, GlobalElementId, Hitbox, HitboxBehavior, Hsla,
    InspectorElementId, IntoElement, LayoutId, Pixels, Point, ShapedLine, SharedString, Size,
    StrikethroughStyle, Style, TextAlign, TextRun, UnderlineStyle, Window, fill, outline, point,
    px, relative, size,
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
pub(crate) fn cell_size(style: &TerminalStyle, window: &Window) -> Size<Pixels> {
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

enum Cursor {
    Block {
        bounds: Bounds<Pixels>,
        color: Hsla,
        glyph: Option<Box<Text>>,
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

pub(crate) struct Prepainted {
    cell: Size<Pixels>,
    hitbox: Hitbox,
    /// What the mouse listeners map positions and encode reports by.
    frame: Frame,
    background: Hsla,
    quads: Vec<(Bounds<Pixels>, Hsla)>,
    text: Vec<Text>,
    cursor: Option<Cursor>,
    /// Marked text over its background, at the cursor.
    marked: Option<(Bounds<Pixels>, Text)>,
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
        let focused = self.view.read(cx).focus.is_focused(window);
        let marked = self.view.read(cx).marked.clone();
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
        let shape = |row: usize, run: &Run, window: &mut Window| Text {
            origin: at(row, run.col),
            line: shape_run(run, &font, &style, cell.width, window),
        };

        let rows = layout::rows(&snapshot, &style, focused);
        for (row, layout) in rows.iter().enumerate() {
            for (range, colour) in &layout.backgrounds {
                prepainted
                    .quads
                    .push((cells(row, range.start, range.len()), *colour));
            }
        }
        for span in &snapshot.selection {
            let width = span.end_col.saturating_sub(span.start_col);
            prepainted
                .quads
                .push((cells(span.row, span.start_col, width), style.selection));
        }
        for (row, layout) in rows.iter().enumerate() {
            for run in &layout.runs {
                prepainted.text.push(shape(row, run, window));
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
                glyph: glyph.map(|glyph| Box::new(shape(row, &glyph, window))),
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
                    if let Some(glyph) = glyph {
                        paint_text(glyph, line_height, window, cx);
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

/// Shapes `run` with every glyph a cell wide, so they stay on the grid
/// whatever font they fall back to.
fn shape_run(
    run: &Run,
    font: &Font,
    style: &TerminalStyle,
    cell_width: Pixels,
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
        len: run.text.len(),
        font,
        color: run.fg,
        background_color: None,
        underline: run.underline.then_some(UnderlineStyle {
            thickness: px(1.),
            color: Some(run.fg),
            wavy: false,
        }),
        strikethrough: run.strikeout.then_some(StrikethroughStyle {
            thickness: px(1.),
            color: Some(run.fg),
        }),
    };
    window.text_system().shape_line(
        SharedString::from(run.text.clone()),
        style.font_size,
        &[text_run],
        Some(cell_width),
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
