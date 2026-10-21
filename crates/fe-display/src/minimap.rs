//! The minimap's model: VS Code's minimap laid out as on a 2x display, in
//! logical pixels, and a row's coloured runs in minimap columns. Pure
//! functions over plain numbers; painting it is the UI's.
//!
//! A minimap column is `scale` pixels wide and a row `2 * scale` tall
//! (`3 * scale` drawing blocks, as VS Code's are). The editor scrolls until
//! its last row is at the top, so the rows the minimap lays out are the
//! file's and then the space that scrolling past the end reaches, one
//! screen less a row, as VS Code's do with `scrollBeyondLastLine`. That keeps
//! the slider over exactly the rows the editor shows.

use std::ops::Range;

/// Which side of the text the minimap is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MinimapSide {
    #[default]
    Right,
    Left,
}

/// How tall the minimap's rows are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MinimapSize {
    /// The scale's row height; the minimap scrolls with the editor when the
    /// rows don't fit.
    #[default]
    Proportional,
    /// The row height that makes the rows fill the minimap's height,
    /// stretched or shrunk; it never scrolls.
    Fill,
    /// As `Proportional` when the rows fit, else shrunk as `Fill`.
    Fit,
}

/// When the slider shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MinimapShowSlider {
    /// While the pointer is over the minimap.
    #[default]
    Mouseover,
    Always,
}

/// The minimap's settings, VS Code's `editor.minimap.*` but `enabled`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MinimapOptions {
    pub side: MinimapSide,
    /// 1 to 3; others are clamped to that.
    pub scale: u32,
    /// The most columns of a row drawn, at least 1.
    pub max_column: u32,
    pub size: MinimapSize,
    pub show_slider: MinimapShowSlider,
    /// Characters' shapes, or blocks of colour.
    pub render_characters: bool,
}

impl Default for MinimapOptions {
    /// VS Code's defaults.
    fn default() -> Self {
        Self {
            side: MinimapSide::Right,
            scale: 1,
            max_column: 120,
            size: MinimapSize::Proportional,
            show_slider: MinimapShowSlider::Mouseover,
            render_characters: true,
        }
    }
}

impl MinimapOptions {
    fn scale(&self) -> f32 {
        self.scale.clamp(1, 3) as f32
    }

    /// A column's width: `scale` pixels.
    pub fn column_width(&self) -> f32 {
        self.scale()
    }

    /// A row's height at the scale, as `Proportional` draws it: `2 * scale`
    /// pixels, or `3 * scale` drawing blocks.
    pub fn row_height(&self) -> f32 {
        let base = if self.render_characters { 2. } else { 3. };
        base * self.scale()
    }

    /// The minimap's width, from the `width` it shares with the text, whose
    /// columns are `em` wide: `max_column` columns, but no more than
    /// `floor(width * column / (em + column))`, which gives the text and
    /// the minimap the same number of columns, as VS Code shrinks it for a
    /// narrow editor. Under 2 columns, 0: hidden.
    pub fn width(&self, width: f32, em: f32) -> f32 {
        let column = self.column_width();
        let full = self.max_column.max(1) as f32 * column;
        let narrow = (width.max(0.) * column / (em.max(0.) + column)).floor();
        let width = full.min(narrow);
        if width < 2. * column { 0. } else { width }
    }
}

/// The editor the minimap is beside, in logical pixels and rows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MinimapViewport {
    /// The width the minimap and the text share.
    pub width: f32,
    /// The editor's height, and the minimap's.
    pub height: f32,
    /// A text column's width.
    pub em: f32,
    /// How many display rows the editor has.
    pub rows: u32,
    /// The display row at the editor's top, fractional when scrolled part
    /// way into it.
    pub first_visible_row: f32,
    /// How many rows the editor's height holds, fractional.
    pub visible_rows: f32,
}

/// Where the minimap is, its rows and its slider, for one frame.
#[derive(Clone, Debug, PartialEq)]
pub struct MinimapLayout {
    /// The minimap's left and right edges; empty when it's hidden.
    pub x: Range<f32>,
    /// What's left of the shared width for the text.
    pub text_x: Range<f32>,
    /// A row's height.
    pub row_height: f32,
    /// How far the minimap has scrolled, in pixels: row `r` is drawn at
    /// `r * row_height - top`.
    pub top: f32,
    /// The first display row drawn, at `first_row_y`.
    pub first_row: u32,
    pub first_row_y: f32,
    /// Past the last display row drawn.
    pub end_row: u32,
    /// The slider over the visible rows, from the minimap's top.
    pub slider_top: f32,
    pub slider_height: f32,
    rows: u32,
    /// How far the slider moves per row the editor scrolls.
    slider_per_row: f32,
}

impl MinimapLayout {
    pub fn new(options: &MinimapOptions, viewport: &MinimapViewport) -> Self {
        let width = options.width(viewport.width, viewport.em);
        let shared = viewport.width.max(0.);
        let (x, text_x) = match options.side {
            MinimapSide::Right => (shared - width..shared, 0.0..shared - width),
            MinimapSide::Left => (0.0..width, width..shared),
        };
        let height = viewport.height.max(0.);
        let rows = viewport.rows;
        let last_row = rows.saturating_sub(1) as f32;
        let visible = viewport.visible_rows.max(0.);
        // The file's rows and the space scrolling past its end reaches.
        let content_rows = last_row + visible.max(1.);
        let base = options.row_height();
        let fits = content_rows * base <= height;
        let row_height = match options.size {
            MinimapSize::Proportional => base,
            MinimapSize::Fit if fits => base,
            MinimapSize::Fill | MinimapSize::Fit => height / content_rows,
        };
        let scrolls = options.size == MinimapSize::Proportional && !fits;
        let slider_height = (visible.max(1.) * row_height).min(height);
        let travel = (height - slider_height).max(0.);
        let slider_per_row = if last_row == 0. {
            0.
        } else if scrolls {
            travel / last_row
        } else {
            row_height.min(travel / last_row)
        };
        let first = viewport.first_visible_row.clamp(0., last_row);
        let slider_top = first * slider_per_row;
        let top = if scrolls {
            first * (row_height - slider_per_row)
        } else {
            0.
        };
        let (first_row, end_row) = if row_height > 0. {
            let first_row = ((top / row_height).floor() as u32).min(rows);
            let end_row = (((top + height) / row_height).ceil() as u32).min(rows);
            (first_row, end_row.max(first_row))
        } else {
            (0, 0)
        };
        Self {
            x,
            text_x,
            row_height,
            top,
            first_row,
            first_row_y: first_row as f32 * row_height - top,
            end_row,
            slider_top,
            slider_height,
            rows,
            slider_per_row,
        }
    }

    /// Whether the minimap shows: wide enough for 2 columns.
    pub fn visible(&self) -> bool {
        self.x.end > self.x.start
    }

    /// Display row `row`'s y, from the minimap's top.
    pub fn row_y(&self, row: u32) -> f32 {
        row as f32 * self.row_height - self.top
    }

    /// The display row drawn at `y`, from the minimap's top, which a click
    /// there reveals in the editor's centre, as VS Code's does: the last row
    /// below the rows.
    pub fn scroll_for_click(&self, y: f32) -> u32 {
        let last = self.rows.saturating_sub(1);
        if self.row_height <= 0. {
            return 0;
        }
        let row = ((self.top + y.max(0.)) / self.row_height).floor();
        (row as u32).min(last)
    }

    /// The editor's first visible row once the slider, dragged from where it
    /// was with `start_first_row` at the top, has moved `dy` pixels: the
    /// slider follows the pointer exactly, from the top to the last row.
    pub fn scroll_for_drag(&self, start_first_row: f32, dy: f32) -> f32 {
        let last = self.rows.saturating_sub(1) as f32;
        if self.slider_per_row <= 0. {
            return start_first_row.clamp(0., last);
        }
        (start_first_row + dy / self.slider_per_row).clamp(0., last)
    }
}

/// A run of a row's characters of one colour, in minimap columns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MinimapRun<C> {
    pub columns: Range<u32>,
    pub color: C,
}

/// `row`'s non-whitespace runs of one colour, in display columns: tabs
/// expanded to the next multiple of `tab_size` as [`crate::ExpandedRow`]
/// does, a wide char two columns (as VS Code's minimap draws CJK and
/// fullwidth forms, and the two UTF-16 halves of an astral char), split at
/// whitespace and where the colour changes, clipped at `max_column`, and
/// adjacent runs of one colour merged. `spans` are byte ranges within
/// `row`, ordered by start, as `styled_highlights` hands them out; where
/// they overlap the first wins, and bytes in none take `default`.
pub fn row_runs<C: Copy + PartialEq>(
    row: &str,
    tab_size: usize,
    spans: &[(Range<usize>, C)],
    default: C,
    max_column: u32,
) -> Vec<MinimapRun<C>> {
    let tab_size = tab_size.max(1);
    let mut runs: Vec<MinimapRun<C>> = Vec::new();
    let mut span = 0;
    let mut column: u32 = 0;
    // The column as `ExpandedRow` counts it, a char one, for tab stops.
    let mut tab_column = 0;
    for (byte, ch) in row.char_indices() {
        if column >= max_column {
            break;
        }
        if ch == '\t' {
            let width = tab_size - tab_column % tab_size;
            tab_column += width;
            column = column.saturating_add(width as u32);
            continue;
        }
        tab_column += 1;
        let width = if is_wide(ch) { 2 } else { 1 };
        let start = column;
        column = column.saturating_add(width);
        if ch.is_whitespace() {
            continue;
        }
        while spans.get(span).is_some_and(|(range, _)| range.end <= byte) {
            span += 1;
        }
        let color = match spans.get(span) {
            Some((range, color)) if range.start <= byte => *color,
            _ => default,
        };
        let end = column.min(max_column);
        match runs.last_mut() {
            Some(last) if last.color == color && last.columns.end == start => {
                last.columns.end = end;
            }
            _ => runs.push(MinimapRun {
                columns: start..end,
                color,
            }),
        }
    }
    runs
}

/// Whether VS Code's minimap draws `ch` two columns wide: its
/// `isFullWidthCharacter` ranges (CJK, Hangul, fullwidth ASCII and
/// symbols), or an astral char, two UTF-16 units.
fn is_wide(ch: char) -> bool {
    matches!(
        ch as u32,
        0x2E80..=0xD7AF | 0xF900..=0xFAFF | 0xFF01..=0xFF5E | 0xFFE0..=0xFFE6 | 0x10000..
    )
}

#[cfg(test)]
#[path = "minimap_tests.rs"]
mod tests;
