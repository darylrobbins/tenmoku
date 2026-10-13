//! A copy of the visible grid, in fastedit's own types: the view renders this
//! and never names alacritty_terminal.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell as TermCell, Flags, Hyperlink};
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{
    Color as TermColor, CursorShape as TermCursorShape, NamedColor,
};

/// The visible rows at the current display offset, and the state the view
/// needs to draw them.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// `rows[row][col]`, top to bottom; every row has `cols` cells.
    pub rows: Vec<Vec<Cell>>,
    pub cols: usize,
    pub cursor: Cursor,
    /// How many lines the view is scrolled back into history (0 at the bottom).
    pub display_offset: usize,
    /// Lines of scrollback above the screen.
    pub history_size: usize,
    /// The selection's visible part, one span per row, top to bottom.
    pub selection: Vec<SelectionSpan>,
    pub modes: Modes,
    /// The URIs of the OSC 8 hyperlinks on screen, which [`Cell::link`]
    /// indexes.
    pub links: Vec<String>,
}

impl Snapshot {
    /// Row `row`'s text, with wide-character spacers skipped and trailing
    /// blanks trimmed.
    pub fn row_text(&self, row: usize) -> String {
        let mut text = String::new();
        for cell in &self.rows[row] {
            if cell.flags.contains(CellFlags::WIDE_SPACER) {
                continue;
            }
            text.push(cell.ch);
            if let Some(extra) = &cell.extra {
                text.push_str(extra);
            }
        }
        text.truncate(text.trim_end().len());
        text
    }

    /// Every row's text, joined with `\n`.
    pub fn text(&self) -> String {
        (0..self.rows.len())
            .map(|row| self.row_text(row))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// One grid cell.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub ch: char,
    /// Zero-width characters (combining marks) drawn over `ch`.
    pub extra: Option<String>,
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags,
    /// The OSC 8 hyperlink the cell belongs to, an index into
    /// [`Snapshot::links`]; the cells of one hyperlink share it.
    pub link: Option<u32>,
}

/// A cell colour, resolved against the view's theme at render time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    /// The theme's default foreground.
    Foreground,
    /// The theme's default background.
    Background,
    /// One of the theme's 16 ANSI colours, 0–15.
    Named(u8),
    /// The 256-colour palette, 16–255 (see [`indexed_rgb`]). The terminal may
    /// also send 0–15 this way.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// A cell's attributes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CellFlags(u16);

impl CellFlags {
    pub const BOLD: CellFlags = CellFlags(1);
    pub const ITALIC: CellFlags = CellFlags(1 << 1);
    pub const DIM: CellFlags = CellFlags(1 << 2);
    pub const UNDERLINE: CellFlags = CellFlags(1 << 3);
    pub const STRIKEOUT: CellFlags = CellFlags(1 << 4);
    pub const INVERSE: CellFlags = CellFlags(1 << 5);
    pub const HIDDEN: CellFlags = CellFlags(1 << 6);
    /// The first cell of a double-width character.
    pub const WIDE: CellFlags = CellFlags(1 << 7);
    /// The cell after a [`CellFlags::WIDE`] one, which draws nothing.
    pub const WIDE_SPACER: CellFlags = CellFlags(1 << 8);

    pub const fn empty() -> CellFlags {
        CellFlags(0)
    }

    pub const fn contains(self, other: CellFlags) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for CellFlags {
    type Output = CellFlags;

    fn bitor(self, rhs: CellFlags) -> CellFlags {
        CellFlags(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for CellFlags {
    fn bitor_assign(&mut self, rhs: CellFlags) {
        self.0 |= rhs.0;
    }
}

/// The cursor, in viewport coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    /// [`CursorShape::Hidden`] when the program hid it or it is scrolled out of
    /// view.
    pub shape: CursorShape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CursorShape {
    Block,
    Beam,
    Underline,
    Hidden,
}

/// Selected columns `start_col..end_col` (end exclusive) of viewport row `row`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionSpan {
    pub row: usize,
    pub start_col: usize,
    pub end_col: usize,
}

/// The terminal modes that change how input is encoded or how the view
/// behaves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modes {
    /// DECCKM: cursor keys send `SS3`.
    pub app_cursor: bool,
    /// DECKPAM: the keypad sends application sequences.
    pub app_keypad: bool,
    pub bracketed_paste: bool,
    pub alt_screen: bool,
    /// On the alternate screen, the wheel sends cursor keys.
    pub alternate_scroll: bool,
    pub focus_reporting: bool,
    pub mouse_report_click: bool,
    pub mouse_drag: bool,
    pub mouse_motion: bool,
    pub sgr_mouse: bool,
    /// Vi copy mode: [`Snapshot::cursor`] is the vi cursor.
    pub vi: bool,
}

impl Modes {
    pub(crate) fn from_term(mode: TermMode) -> Modes {
        Modes {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            app_keypad: mode.contains(TermMode::APP_KEYPAD),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            alt_screen: mode.contains(TermMode::ALT_SCREEN),
            alternate_scroll: mode.contains(TermMode::ALTERNATE_SCROLL),
            focus_reporting: mode.contains(TermMode::FOCUS_IN_OUT),
            mouse_report_click: mode.contains(TermMode::MOUSE_REPORT_CLICK),
            mouse_drag: mode.contains(TermMode::MOUSE_DRAG),
            mouse_motion: mode.contains(TermMode::MOUSE_MOTION),
            sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
            vi: mode.contains(TermMode::VI),
        }
    }
}

/// xterm's colour for palette index `i`: its default ANSI colours for 0–15,
/// the 6×6×6 cube for 16–231 and the grey ramp for 232–255.
pub fn indexed_rgb(i: u8) -> (u8, u8, u8) {
    const ANSI: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    let level = |n: u8| if n == 0 { 0 } else { 55 + 40 * n };
    match i {
        0..=15 => ANSI[usize::from(i)],
        16..=231 => {
            let n = i - 16;
            (level(n / 36), level(n / 6 % 6), level(n % 6))
        }
        232..=255 => {
            let grey = 8 + 10 * (i - 232);
            (grey, grey, grey)
        }
    }
}

/// Copies the viewport out of `term`. The caller holds the lock only for this.
pub(crate) fn take<T>(term: &Term<T>) -> Snapshot {
    let grid = term.grid();
    let display_offset = grid.display_offset();
    let screen_lines = grid.screen_lines();
    let cols = grid.columns();

    let mut hyperlinks = Vec::new();
    let rows = (0..screen_lines)
        .map(|row| {
            let line = Line(row as i32 - display_offset as i32);
            let grid_row = &grid[line];
            (0..cols)
                .map(|col| cell(&grid_row[Column(col)], &mut hyperlinks))
                .collect()
        })
        .collect();

    let mode = *term.mode();
    // In vi mode the vi cursor is the one drawn, a block wherever it is.
    let vi = mode.contains(TermMode::VI);
    let point = if vi {
        term.vi_mode_cursor.point
    } else {
        grid.cursor.point
    };
    let cursor_row = point.line.0 + display_offset as i32;
    let in_view = (0..screen_lines as i32).contains(&cursor_row) && point.column.0 < cols;
    let mut cursor_col = point.column.0.min(cols - 1);
    if in_view && cursor_col > 0 && grid[point].flags.contains(Flags::WIDE_CHAR_SPACER) {
        cursor_col -= 1;
    }
    let shape = if (!vi && !mode.contains(TermMode::SHOW_CURSOR)) || !in_view {
        CursorShape::Hidden
    } else if vi {
        CursorShape::Block
    } else {
        match term.cursor_style().shape {
            TermCursorShape::Block | TermCursorShape::HollowBlock => CursorShape::Block,
            TermCursorShape::Beam => CursorShape::Beam,
            TermCursorShape::Underline => CursorShape::Underline,
            TermCursorShape::Hidden => CursorShape::Hidden,
        }
    };
    let cursor = Cursor {
        row: cursor_row.max(0) as usize,
        col: cursor_col,
        shape,
    };

    Snapshot {
        rows,
        cols,
        cursor,
        display_offset,
        history_size: grid.history_size(),
        selection: selection_spans(term, display_offset, screen_lines, cols),
        modes: Modes::from_term(mode),
        links: hyperlinks.iter().map(|h| h.uri().to_owned()).collect(),
    }
}

fn selection_spans<T>(
    term: &Term<T>,
    display_offset: usize,
    screen_lines: usize,
    cols: usize,
) -> Vec<SelectionSpan> {
    let Some(range) = term.selection.as_ref().and_then(|s| s.to_range(term)) else {
        return Vec::new();
    };
    let to_row = |line: Line| line.0 + display_offset as i32;
    let first = to_row(range.start.line).max(0);
    let last = to_row(range.end.line).min(screen_lines as i32 - 1);
    (first..=last)
        .map(|row| {
            let line = Line(row - display_offset as i32);
            let (start_col, end_col) = if range.is_block {
                (range.start.column.0, range.end.column.0 + 1)
            } else {
                let start = if line == range.start.line {
                    range.start.column.0
                } else {
                    0
                };
                let end = if line == range.end.line {
                    range.end.column.0 + 1
                } else {
                    cols
                };
                (start, end)
            };
            // A wide character is selected whole, so the span covers both of
            // its cells when an end lands on either half.
            let grid_row = &term.grid()[line];
            let mut start_col = start_col.min(cols - 1);
            let mut end_col = end_col.min(cols);
            if start_col > 0
                && grid_row[Column(start_col)]
                    .flags
                    .contains(Flags::WIDE_CHAR_SPACER)
            {
                start_col -= 1;
            }
            if end_col > 0
                && end_col < cols
                && grid_row[Column(end_col - 1)]
                    .flags
                    .contains(Flags::WIDE_CHAR)
            {
                end_col += 1;
            }
            SelectionSpan {
                row: row as usize,
                start_col,
                end_col,
            }
        })
        .collect()
}

/// Converts `cell`, adding its hyperlink to `hyperlinks` if it is new there.
fn cell(cell: &TermCell, hyperlinks: &mut Vec<Hyperlink>) -> Cell {
    let mut flags = CellFlags::empty();
    for (theirs, ours) in [
        (Flags::BOLD, CellFlags::BOLD),
        (Flags::ITALIC, CellFlags::ITALIC),
        (Flags::DIM, CellFlags::DIM),
        (Flags::STRIKEOUT, CellFlags::STRIKEOUT),
        (Flags::INVERSE, CellFlags::INVERSE),
        (Flags::HIDDEN, CellFlags::HIDDEN),
        (Flags::WIDE_CHAR, CellFlags::WIDE),
        (Flags::WIDE_CHAR_SPACER, CellFlags::WIDE_SPACER),
    ] {
        if cell.flags.contains(theirs) {
            flags |= ours;
        }
    }
    if cell.flags.intersects(Flags::ALL_UNDERLINES) {
        flags |= CellFlags::UNDERLINE;
    }
    let extra = cell
        .zerowidth()
        .filter(|chars| !chars.is_empty())
        .map(|chars| chars.iter().collect());
    let link = cell.hyperlink().map(|hyperlink| {
        let index = match hyperlinks.iter().position(|h| *h == hyperlink) {
            Some(index) => index,
            None => {
                hyperlinks.push(hyperlink);
                hyperlinks.len() - 1
            }
        };
        index as u32
    });
    Cell {
        ch: cell.c,
        extra,
        fg: color(cell.fg),
        bg: color(cell.bg),
        flags,
        link,
    }
}

fn color(color: TermColor) -> Color {
    match color {
        TermColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        TermColor::Indexed(i) => Color::Indexed(i),
        TermColor::Named(named) => match named {
            NamedColor::Foreground
            | NamedColor::BrightForeground
            | NamedColor::DimForeground
            | NamedColor::Cursor => Color::Foreground,
            NamedColor::Background => Color::Background,
            NamedColor::DimBlack => Color::Named(0),
            NamedColor::DimRed => Color::Named(1),
            NamedColor::DimGreen => Color::Named(2),
            NamedColor::DimYellow => Color::Named(3),
            NamedColor::DimBlue => Color::Named(4),
            NamedColor::DimMagenta => Color::Named(5),
            NamedColor::DimCyan => Color::Named(6),
            NamedColor::DimWhite => Color::Named(7),
            ansi => Color::Named(ansi as u8),
        },
    }
}
