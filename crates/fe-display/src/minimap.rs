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

use std::{collections::BTreeMap, ops::Range};

use fe_buffer::BufferSnapshot;

use crate::{DisplayMap, Fold, SourceKey};

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

/// A display row's runs as [`RowRunsCache`] holds them.
#[derive(Clone, Debug, PartialEq)]
pub struct CachedRow<C> {
    pub runs: Vec<MinimapRun<C>>,
    /// What the caller built the runs from besides the row's text, such as
    /// decorations over it, which it compares to tell whether they're stale.
    pub stamp: u64,
    /// The runs may no longer be the row's ([`RowRunsCache::invalidate`]):
    /// they're kept, to draw until rebuilt and to compare with then.
    pub stale: bool,
    /// Changes only when the runs do: rows drawn with the same ids look
    /// the same.
    pub id: u64,
    /// Where the row's text starts, and its buffer row's bytes, in the text
    /// as of the last sync: an edit touching the buffer row drops it, as
    /// soft wrap lays the whole row out again.
    start: usize,
    source: Range<usize>,
}

/// How [`RowRunsCache::sync`] followed the display rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Synced {
    /// Nothing changed.
    Unchanged,
    /// Rows were edited, folded or unfolded: their runs dropped, and the
    /// rows after them moved.
    Spliced,
    /// The rows changed in ways that can't be followed, such as a soft wrap
    /// catching up: every row's runs are stale.
    Invalidated,
    /// Another text, or history that can't be followed: every row's runs
    /// dropped.
    Cleared,
}

/// What a [`DisplayMap`]'s rows were, to tell how they changed since.
struct Shape {
    source: SourceKey,
    snapshot: Option<BufferSnapshot>,
    rows: u32,
    wrap_width: Option<f32>,
    wrap_pending: bool,
    folds: Vec<Fold>,
}

impl Shape {
    fn of(display: &DisplayMap) -> Self {
        Self {
            source: display.source_key(),
            snapshot: display.snapshot().cloned(),
            rows: display.row_count(),
            wrap_width: display.wrap_width(),
            wrap_pending: display.wrap_is_pending(),
            folds: display.folds().to_vec(),
        }
    }
}

/// The runs of display rows near those the minimap last showed, kept across
/// frames and edits: the rows an edit touched lose their runs, and the rest
/// move to wherever their text is now. Holds at most its capacity of rows,
/// around those last shown ([`RowRunsCache::retain_around`]); they needn't
/// be next to one another, as when the minimap shows every so many rows.
pub struct RowRunsCache<C> {
    shape: Option<Shape>,
    rows: BTreeMap<u32, CachedRow<C>>,
    capacity: usize,
    next_id: u64,
}

impl<C: Clone + PartialEq> RowRunsCache<C> {
    /// A cache of at most `capacity` rows.
    pub fn new(capacity: usize) -> Self {
        Self {
            shape: None,
            rows: BTreeMap::new(),
            capacity: capacity.max(1),
            next_id: 0,
        }
    }

    /// Holds at most `capacity` rows from now on, dropping the last rows
    /// beyond it.
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity.max(1);
        while self.rows.len() > self.capacity {
            self.rows.pop_last();
        }
    }

    /// Whether the cache was last synced with `display` as it is.
    pub fn is_synced(&self, display: &DisplayMap) -> bool {
        self.shape.as_ref().is_some_and(|shape| {
            shape.source == display.source_key()
                && shape.rows == display.row_count()
                && shape.folds == display.folds()
                && shape.wrap_width == display.wrap_width()
        })
    }

    /// How many rows have runs.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Display row `row`'s runs, if they're cached.
    pub fn row(&self, row: u32) -> Option<&CachedRow<C>> {
        self.rows.get(&row)
    }

    /// Caches `runs` for display row `row` of `display`, which the cache
    /// was last synced with, built with `stamp`, and returns their id: the
    /// id the row had if its runs are the same. Past the capacity, the row
    /// furthest from it goes.
    pub fn insert(
        &mut self,
        display: &DisplayMap,
        row: u32,
        runs: Vec<MinimapRun<C>>,
        stamp: u64,
    ) -> u64 {
        let id = match self.row(row) {
            Some(cached) if cached.runs == runs => cached.id,
            _ => {
                self.next_id += 1;
                self.next_id
            }
        };
        let shown = display.row(row);
        let text = display.text();
        let start = text.row_start(shown.buffer_row);
        let source = start..start + text.row_len(shown.buffer_row) as usize;
        self.rows.insert(
            row,
            CachedRow {
                runs,
                stamp,
                stale: false,
                id,
                start: shown.range.start,
                source,
            },
        );
        while self.rows.len() > self.capacity {
            let (first, last) = (self.rows.keys().next(), self.rows.keys().next_back());
            let (Some(&first), Some(&last)) = (first, last) else {
                break;
            };
            match row - first >= last - row {
                true => self.rows.pop_first(),
                false => self.rows.pop_last(),
            };
        }
        id
    }

    /// Rows `rows` were replaced by `new_rows` rows: their runs are dropped,
    /// and the rows after them move by the difference.
    pub fn splice(&mut self, rows: Range<u32>, new_rows: u32) {
        let mut after = self.rows.split_off(&rows.start);
        let moved = after.split_off(&rows.end);
        let old_rows = rows.end.saturating_sub(rows.start);
        self.rows.extend(
            (moved.into_iter())
                .map(|(row, cached)| ((row + new_rows).saturating_sub(old_rows), cached)),
        );
    }

    /// Marks rows `rows`' runs stale: kept, but to be rebuilt before
    /// they're trusted.
    pub fn invalidate(&mut self, rows: Range<u32>) {
        for cached in self.rows.range_mut(rows).map(|(_, cached)| cached) {
            cached.stale = true;
        }
    }

    /// Drops every row's runs.
    pub fn clear(&mut self) {
        self.rows.clear();
    }

    /// Keeps only rows within the capacity's reach around `shown`, the rows
    /// the minimap shows, and at most the capacity of them.
    pub fn retain_around(&mut self, shown: Range<u32>) {
        let reach =
            (self.capacity as u32).saturating_sub(shown.end.saturating_sub(shown.start)) / 2;
        let first = shown.start.saturating_sub(reach);
        let end = shown.end.saturating_add(reach);
        self.rows.retain(|&row, _| (first..end).contains(&row));
        let middle = shown.start / 2 + shown.end / 2;
        while self.rows.len() > self.capacity {
            let (Some(&first), Some(&last)) =
                (self.rows.keys().next(), self.rows.keys().next_back())
            else {
                break;
            };
            match middle.abs_diff(first) >= middle.abs_diff(last) {
                true => self.rows.pop_first(),
                false => self.rows.pop_last(),
            };
        }
    }

    /// Follows `display` since the last sync: rows whose buffer rows were
    /// edited since lose their runs, and the rest move to the rows showing
    /// their text now; a fold made or undone moves the rows after it. When
    /// the rows changed in a way that can't be followed row by row, every
    /// row's runs go stale, or for another text, are dropped.
    pub fn sync(&mut self, display: &DisplayMap) -> Synced {
        let new = Shape::of(display);
        let Some(old) = self.shape.replace(new) else {
            self.clear();
            return Synced::Cleared;
        };
        let new = self.shape.as_ref().expect("just set");
        let (followed, snapshot) = (follow(&old, new, display), new.snapshot.clone());
        match followed {
            Followed::Unchanged => Synced::Unchanged,
            Followed::Folded(splice) => {
                self.splice(
                    splice.start..splice.start + splice.old_rows,
                    splice.new_rows,
                );
                Synced::Spliced
            }
            Followed::Edited => {
                let (Some(old), Some(new)) = (&old.snapshot, &snapshot) else {
                    unreachable!("edits are of a buffer");
                };
                match self.follow_edits(old, new, display) {
                    true => Synced::Spliced,
                    false => {
                        self.invalidate(0..u32::MAX);
                        Synced::Invalidated
                    }
                }
            }
            Followed::Lost => {
                self.invalidate(0..u32::MAX);
                Synced::Invalidated
            }
            Followed::Other => {
                self.clear();
                Synced::Cleared
            }
        }
    }

    /// Drops the rows whose buffer rows the edits from `old` to `new`
    /// touched, and moves each run of the rest that were next to one
    /// another to the rows `display` shows their text at. Returns false if
    /// a run doesn't land on rows showing its text, as after a fold the
    /// edits didn't make.
    fn follow_edits(
        &mut self,
        old: &BufferSnapshot,
        new: &BufferSnapshot,
        display: &DisplayMap,
    ) -> bool {
        let mut rows: Vec<(u32, CachedRow<C>)> =
            std::mem::take(&mut self.rows).into_iter().collect();
        for batch in new.edits_since(old.version()) {
            let mut edits = batch.iter().peekable();
            let mut moved: isize = 0;
            rows.retain_mut(|(_, cached)| {
                // The edits wholly before the row move it.
                while let Some(edit) = edits.next_if(|edit| edit.old.end < cached.source.start) {
                    moved += edit.new.len() as isize - edit.old.len() as isize;
                }
                if edits
                    .peek()
                    .is_some_and(|edit| edit.old.start <= cached.source.end)
                {
                    return false;
                }
                let shift = |offset: usize| offset.saturating_add_signed(moved);
                cached.start = shift(cached.start);
                cached.source = shift(cached.source.start)..shift(cached.source.end);
                true
            });
        }
        for run in rows.chunk_by(|(above, _), (below, _)| *below == above + 1) {
            let first = display.row_for_offset(run[0].1.start);
            let last = first + run.len() as u32 - 1;
            let lands = |row: u32, cached: &CachedRow<C>| {
                row < display.row_count() && display.row(row).range.start == cached.start
            };
            if !lands(first, &run[0].1) || !lands(last, &run[run.len() - 1].1) {
                return false;
            }
            self.rows.extend(
                (run.iter().cloned())
                    .enumerate()
                    .map(|(n, (_, cached))| (first + n as u32, cached)),
            );
        }
        true
    }
}

/// How the display rows changed between two shapes.
enum Followed {
    Unchanged,
    /// The text was edited, and folds the edits didn't touch moved with it.
    Edited,
    /// One fold was made or undone.
    Folded(RowSplice),
    /// The same text, but the rows can't be followed one by one.
    Lost,
    /// Another text, or an earlier or forgotten version of it.
    Other,
}

/// Rows `start..start + old_rows` replaced by `new_rows` rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RowSplice {
    start: u32,
    old_rows: u32,
    new_rows: u32,
}

fn follow(old: &Shape, new: &Shape, display: &DisplayMap) -> Followed {
    let same_wrap = old.wrap_width == new.wrap_width && !old.wrap_pending && !new.wrap_pending;
    if old.source == new.source {
        if !same_wrap {
            return Followed::Lost;
        }
        if old.folds == new.folds {
            return match old.rows == new.rows {
                true => Followed::Unchanged,
                false => Followed::Lost,
            };
        }
        return fold_splice(old, new, display).map_or(Followed::Lost, Followed::Folded);
    }
    let (Some(old_snapshot), Some(new_snapshot)) = (&old.snapshot, &new.snapshot) else {
        return Followed::Other;
    };
    let from = old_snapshot.version();
    if old_snapshot.id() != new_snapshot.id()
        || new_snapshot.version() < from
        || new_snapshot.oldest_version() > from
    {
        return Followed::Other;
    }
    // Folds an edit touched are dropped, and the rest move with their
    // header rows: any other change of folds isn't the edits'.
    let moved_folds = (old.folds.iter()).map(|fold| {
        let header = old_snapshot.text().row_start(fold.start);
        let offset = new_snapshot.map_offset(header, fe_buffer::Bias::Right, from);
        let start = new_snapshot.text().offset_to_point(offset).row;
        Fold {
            start,
            end: start + (fold.end - fold.start),
        }
    });
    match same_wrap && moved_folds.eq(new.folds.iter().copied()) {
        true => Followed::Edited,
        false => Followed::Lost,
    }
}

/// The rows one fold made or undone hid or showed, the text the same: those
/// after its header's last display row. `None` for any other change of
/// folds.
fn fold_splice(old: &Shape, new: &Shape, display: &DisplayMap) -> Option<RowSplice> {
    let (fewer, more) = match old.folds.len() + 1 {
        n if n == new.folds.len() => (&old.folds, &new.folds),
        n if n == new.folds.len() + 2 => (&new.folds, &old.folds),
        _ => return None,
    };
    // The one fold in `more` that `fewer` lacks.
    let at = (more.iter().zip(fewer))
        .position(|(a, b)| a != b)
        .unwrap_or(fewer.len());
    if more[..at] != fewer[..at] || more[at + 1..] != fewer[at..] {
        return None;
    }
    let header = more[at].start;
    let text = display.text();
    let header_end = text.row_start(header) + text.row_len(header) as usize;
    let start = display.row_for_offset(header_end) + 1;
    let shown = new.rows as i64 - old.rows as i64;
    let rows = u32::try_from(shown.unsigned_abs()).ok()?;
    Some(match shown > 0 {
        true => RowSplice {
            start,
            old_rows: 0,
            new_rows: rows,
        },
        false => RowSplice {
            start,
            old_rows: rows,
            new_rows: 0,
        },
    })
}

#[cfg(test)]
#[path = "minimap_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "minimap_cache_tests.rs"]
mod cache_tests;
