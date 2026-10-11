//! Which cells paint what: background ranges, text runs and the cursor, in
//! grid columns and rows, worked out from a [`Snapshot`] without a window.
//! The element multiplies them by the cell size.

use std::ops::Range;

use fe_terminal::{Cell, CellFlags, Color, CursorShape, Snapshot, indexed_rgb};
use gpui::{Hsla, Rgba};

use crate::TerminalStyle;

/// What one grid row paints.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowLayout {
    /// Columns whose background isn't the default, merged where adjacent
    /// cells share a colour. Painted over the default background.
    pub backgrounds: Vec<(Range<usize>, Hsla)>,
    /// Spans of cells drawn in one style, left to right.
    pub runs: Vec<Run>,
}

/// Text drawn in one style, from column `col`. A wide character is a run of
/// its own, so the glyphs after it start on the grid again.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub col: usize,
    /// The columns it covers: two for a wide character.
    pub cells: usize,
    pub text: String,
    pub fg: Hsla,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
}

/// The cursor, at a grid cell, `cells` wide (two on a wide character).
#[derive(Clone, Debug, PartialEq)]
pub enum CursorLayout {
    /// A filled block in the cursor colour, with the cell's character, if any,
    /// drawn over it in the cell's background colour.
    Block {
        row: usize,
        col: usize,
        cells: usize,
        color: Hsla,
        glyph: Option<Run>,
    },
    /// An outlined block: the cursor of an unfocused terminal.
    Hollow {
        row: usize,
        col: usize,
        cells: usize,
        color: Hsla,
    },
    /// A bar at the cell's left edge.
    Beam {
        row: usize,
        col: usize,
        color: Hsla,
    },
    /// A bar along the cell's bottom edge.
    Underline {
        row: usize,
        col: usize,
        cells: usize,
        color: Hsla,
    },
    Hidden,
}

/// `color` in `style`: the default colours and ANSI 0–15 from the style, the
/// rest of the 256-colour palette from [`indexed_rgb`].
pub fn colour(color: Color, style: &TerminalStyle) -> Hsla {
    match color {
        Color::Foreground => style.foreground,
        Color::Background => style.background,
        Color::Named(i) => style.ansi[usize::from(i % 16)],
        Color::Indexed(i) if i < 16 => style.ansi[usize::from(i)],
        Color::Indexed(i) => {
            let (r, g, b) = indexed_rgb(i);
            rgb(r, g, b)
        }
        Color::Rgb(r, g, b) => rgb(r, g, b),
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Hsla {
    Rgba {
        r: f32::from(r) / 255.,
        g: f32::from(g) / 255.,
        b: f32::from(b) / 255.,
        a: 1.,
    }
    .into()
}

/// How a cell draws, after inverse and dim.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Paint {
    fg: Hsla,
    /// `None` for the default background, which isn't painted per cell.
    bg: Option<Hsla>,
    bold: bool,
    italic: bool,
    underline: bool,
    strikeout: bool,
    hidden: bool,
}

impl Paint {
    fn of(cell: &Cell, style: &TerminalStyle) -> Paint {
        let flags = cell.flags;
        let (fg, bg) = if flags.contains(CellFlags::INVERSE) {
            (cell.bg, cell.fg)
        } else {
            (cell.fg, cell.bg)
        };
        let mut fg = colour(fg, style);
        if flags.contains(CellFlags::DIM) {
            fg.a *= 0.7;
        }
        Paint {
            fg,
            bg: (bg != Color::Background).then(|| colour(bg, style)),
            bold: flags.contains(CellFlags::BOLD),
            italic: flags.contains(CellFlags::ITALIC),
            underline: flags.contains(CellFlags::UNDERLINE),
            strikeout: flags.contains(CellFlags::STRIKEOUT),
            hidden: flags.contains(CellFlags::HIDDEN),
        }
    }

    /// A run of `cells` from `col`, holding `cell`'s text.
    fn run(self, col: usize, cells: usize, cell: &Cell) -> Run {
        let mut run = Run {
            col,
            cells,
            text: String::new(),
            fg: self.fg,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            strikeout: self.strikeout,
        };
        push_text(&mut run.text, cell);
        run
    }
}

impl Run {
    fn has_style(&self, paint: &Paint) -> bool {
        self.fg == paint.fg
            && self.bold == paint.bold
            && self.italic == paint.italic
            && self.underline == paint.underline
            && self.strikeout == paint.strikeout
    }
}

fn push_text(text: &mut String, cell: &Cell) {
    text.push(cell.ch);
    if let Some(extra) = &cell.extra {
        text.push_str(extra);
    }
}

/// What each row of `snapshot` paints. With `focused` and a block cursor,
/// the cursor cell's character is left out: the cursor draws it.
pub fn rows(snapshot: &Snapshot, style: &TerminalStyle, focused: bool) -> Vec<RowLayout> {
    let cursor = snapshot.cursor;
    let drawn_by_cursor = focused && cursor.shape == CursorShape::Block;
    snapshot
        .rows
        .iter()
        .enumerate()
        .map(|(row, cells)| {
            let skip = (drawn_by_cursor && cursor.row == row).then_some(cursor.col);
            row_layout(cells, style, skip)
        })
        .collect()
}

fn row_layout(cells: &[Cell], style: &TerminalStyle, skip: Option<usize>) -> RowLayout {
    let mut layout = RowLayout::default();
    let mut run: Option<Run> = None;
    for (col, cell) in cells.iter().enumerate() {
        let paint = Paint::of(cell, style);
        if let Some(bg) = paint.bg {
            push_background(&mut layout.backgrounds, col, bg);
        }
        if cell.flags.contains(CellFlags::WIDE_SPACER) {
            continue;
        }
        if paint.hidden || skip == Some(col) {
            finish(&mut run, &mut layout.runs);
            continue;
        }
        let wide = cell.flags.contains(CellFlags::WIDE);
        match &mut run {
            Some(current) if !wide && current.has_style(&paint) => {
                push_text(&mut current.text, cell);
                current.cells += 1;
            }
            _ => {
                finish(&mut run, &mut layout.runs);
                // A blank only extends a run: on its own it draws nothing.
                let blank =
                    cell.ch == ' ' && cell.extra.is_none() && !paint.underline && !paint.strikeout;
                if !blank {
                    run = Some(paint.run(col, if wide { 2 } else { 1 }, cell));
                }
            }
        }
        if wide {
            finish(&mut run, &mut layout.runs);
        }
    }
    finish(&mut run, &mut layout.runs);
    layout
}

fn push_background(backgrounds: &mut Vec<(Range<usize>, Hsla)>, col: usize, bg: Hsla) {
    match backgrounds.last_mut() {
        Some((range, colour)) if range.end == col && *colour == bg => range.end += 1,
        _ => backgrounds.push((col..col + 1, bg)),
    }
}

/// Ends `run`, keeping it unless it's only blanks, whose trailing blanks are
/// dropped unless they're underlined or struck out.
fn finish(run: &mut Option<Run>, runs: &mut Vec<Run>) {
    let Some(mut run) = run.take() else {
        return;
    };
    if !run.underline && !run.strikeout {
        while run.text.ends_with(' ') {
            run.text.pop();
            run.cells -= 1;
        }
    }
    if !run.text.is_empty() {
        runs.push(run);
    }
}

/// How `snapshot`'s cursor draws: a filled block, beam or underline per its
/// shape when `focused`, a hollow block when not, and nothing when hidden.
pub fn cursor(snapshot: &Snapshot, style: &TerminalStyle, focused: bool) -> CursorLayout {
    let fe_terminal::Cursor { row, col, shape } = snapshot.cursor;
    let Some(cell) = snapshot.rows.get(row).and_then(|cells| cells.get(col)) else {
        return CursorLayout::Hidden;
    };
    let cells = if cell.flags.contains(CellFlags::WIDE) {
        2
    } else {
        1
    };
    let color = style.cursor;
    if shape == CursorShape::Hidden {
        return CursorLayout::Hidden;
    }
    if !focused {
        return CursorLayout::Hollow {
            row,
            col,
            cells,
            color,
        };
    }
    match shape {
        CursorShape::Block => {
            let paint = Paint::of(cell, style);
            let blank = cell.ch == ' ' && cell.extra.is_none();
            let glyph = (!paint.hidden && !blank).then(|| {
                let mut glyph = paint.run(col, cells, cell);
                glyph.fg = paint.bg.unwrap_or(style.background);
                glyph
            });
            CursorLayout::Block {
                row,
                col,
                cells,
                color,
                glyph,
            }
        }
        CursorShape::Beam => CursorLayout::Beam { row, col, color },
        CursorShape::Underline => CursorLayout::Underline {
            row,
            col,
            cells,
            color,
        },
        CursorShape::Hidden => CursorLayout::Hidden,
    }
}
