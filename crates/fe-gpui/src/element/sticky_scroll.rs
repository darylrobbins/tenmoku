//! Sticky scroll's band ([`fe_display::sticky`]): the header rows of the
//! scopes enclosing the top of the view, pinned there over the text, each
//! with its line number in the gutter and its text in the colours the text
//! area gives the row, with a line and a shadow under the band. The scopes
//! come from fe-syntax's outline, or its fold candidates for a language
//! without one, or fe-display's indented blocks for text without a tree, as
//! `editor.sticky_scroll.default_model` starts, found once per top row and
//! text version ([`scopes`]); the band is laid out once the frame has
//! settled and painted over the text, selections and carets. It spans the
//! gutter and the text, not the minimap beside them.
//!
//! The band takes the pointer from what's under it: a row under the pointer
//! is painted in `sticky_scroll_hover`, and a click on it reveals its header
//! ([`crate::EditorView::reveal_sticky_header`]). Autoscroll keeps the
//! caret's row a row below the band ([`clear_of_caret`]); after a vim scroll
//! the band leaves the caret's row out instead ([`keep_above`]).

#[cfg(test)]
use std::cell::Cell;
use std::ops::{Range, RangeInclusive};

use fe_buffer::Version;
use fe_config::StickyScrollModel;
use fe_display::{
    Bias, BlockMap, CharWidth, ExpandedRow, RowIndex, RowSlice, StickyRow,
    layout::{
        MAX_SHAPED_ROW_BYTES, MeasuredLine as _, Rect, StyleRun, TextMeasure, Vec2, row_runs,
    },
};
use fe_syntax::Syntax;
#[cfg(test)]
use gpui::Bounds;
use gpui::{
    App, ContentMask, CursorStyle, DispatchPhase, Hitbox, HitboxBehavior, Hsla, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, Point, WeakEntity, Window, fill, linear_color_stop,
    linear_gradient, px,
};

use super::{FrameLayout, Line, Shaper, foreground_spans, styled_highlights};
use crate::{EditorTheme, EditorView};

/// The most rows the band pins, `editor.sticky_scroll.max_line_count`,
/// within its bounds.
pub(crate) fn max_rows(view: &EditorView) -> usize {
    view.theme.sticky_scroll.max_line_count.clamp(1, 20) as usize
}

/// How far the shadow under the band fades down, as VS Code's
/// `scrollbar.shadow` does.
const SHADOW_HEIGHT: f32 = 3.;

/// How far either side of the view a long header's shaped slice reaches, in
/// ems, as a long row's in the text area.
const SLICE_MARGIN_EMS: f32 = 32.;

/// Where a band's scopes come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Model {
    /// fe-syntax's outline items.
    Outline,
    /// fe-syntax's fold candidates.
    Folding,
    /// fe-display's indented blocks, at a tab size.
    Indentation(usize),
}

/// The scopes the band was last found from, cached on the view.
pub(crate) struct Scopes {
    /// The language, the model, the runs of buffer rows the scopes overlap
    /// and the text version they were found for.
    key: (Option<&'static str>, Model, Vec<Range<u32>>, Version),
    scopes: Vec<RangeInclusive<u32>>,
}

#[cfg(test)]
thread_local! {
    /// How many times [`scopes`] has asked a model, for tests.
    static QUERIES: Cell<usize> = const { Cell::new(0) };
    /// The model it last asked, and the buffer rows it asked for.
    static ASKED: Cell<Option<Model>> = const { Cell::new(None) };
    static ASKED_ROWS: std::cell::RefCell<Vec<Range<u32>>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

/// How many times a model has been asked for scopes on this thread.
#[cfg(test)]
pub(crate) fn queries() -> usize {
    QUERIES.with(Cell::get)
}

/// The model last asked for scopes on this thread.
#[cfg(test)]
pub(crate) fn asked() -> Option<Model> {
    ASKED.with(Cell::get)
}

/// The runs of buffer rows the model was last asked for on this thread.
#[cfg(test)]
pub(crate) fn asked_rows() -> Vec<Range<u32>> {
    ASKED_ROWS.with(|rows| rows.borrow().clone())
}

/// The model the band's scopes come from: the setting's, or the next that
/// can serve the text. The outline needs a language with an outline query
/// and folding a syntax tree, which plain text, an unknown language and a
/// file over its language's parse limit lack: those use indentation. An
/// outline with no scope at the top doesn't fall back. `None` while the
/// tree is behind the text, being parsed.
fn model(view: &EditorView) -> Option<(Model, Option<&Syntax>)> {
    let snapshot = view.model.snapshot();
    let indentation = Model::Indentation(view.model.tab_size());
    let setting = view.theme.sticky_scroll.default_model;
    let syntax = (view.syntax.as_ref())
        .map(fe_syntax::SyntaxDriver::syntax)
        .filter(|syntax| !syntax.failed() && snapshot.len() <= syntax.language().parse_limit());
    let Some(syntax) = syntax.filter(|_| setting != StickyScrollModel::Indentation) else {
        return Some((indentation, None));
    };
    if syntax.version() != snapshot.version() || syntax.needs_parse() {
        return None;
    }
    let outline = setting == StickyScrollModel::Outline
        && fe_syntax::outline_supported(syntax.language().name());
    let model = match outline {
        true => Model::Outline,
        false => Model::Folding,
    };
    Some((model, Some(syntax)))
}

/// The scopes overlapping the rows at the top of a view scrolled to
/// `scroll_y` over `blocks` that the band can pin, outermost first, from
/// `cache` when the top row, the model ([`model`]) and the text are those
/// it was found for, else found and kept there. None while the band is off.
/// The model is asked for each run of buffer rows shown there, so the rows
/// a closed fold hides between them aren't read.
/// `None` while they can't be known yet: the syntax tree is behind the
/// text, being parsed, and asked again on a frame after it's caught up.
pub(crate) fn scopes(
    view: &EditorView,
    cache: &mut Option<Scopes>,
    blocks: &BlockMap,
    scroll_y: f32,
    line_height: f32,
) -> Option<Vec<RangeInclusive<u32>>> {
    if !view.theme.sticky_scroll.enabled {
        return Some(Vec::new());
    }
    let (model, syntax) = model(view)?;
    let display = view.model.display();
    let row_count = display.row_count();
    let line_height = line_height.max(1.);
    // From the display row at the top to the one the band's last slot can
    // reach, by the whole screen row at the top, so scrolling within it
    // keeps the rows.
    let top = (scroll_y / line_height).floor();
    let display_row =
        |screen_top: f32| (blocks.to_display_top(screen_top).floor() as u32).min(row_count - 1);
    let first = display_row(top);
    let last = display_row(top + max_rows(view) as f32 + 1.);
    let mut runs: Vec<Range<u32>> = Vec::new();
    for row in first..=last {
        let buffer_row = display.row(row).buffer_row;
        match runs.last_mut() {
            Some(run) if buffer_row <= run.end => run.end = buffer_row + 1,
            _ => runs.push(buffer_row..buffer_row + 1),
        }
    }
    let language = syntax.map(|syntax| syntax.language().name());
    let version = view.model.snapshot().version();
    let key = (language, model, runs, version);
    if let Some(cached) = cache.as_ref().filter(|cached| cached.key == key) {
        return Some(cached.scopes.clone());
    }
    // Counted once the model has answered: a tree still being parsed gives
    // None and is asked again, when a background parse lands depends on timing.
    let mut scopes = Vec::new();
    // Indentation: the scopes still open at the end of the last run, with
    // their headers' indents, which the next run may find open past a fold.
    let mut carried: Vec<(u32, usize)> = Vec::new();
    for rows in key.2.iter().cloned() {
        if let Model::Indentation(tab_size) = model {
            let text = view.model.text();
            let found = fe_display::indent_scopes(text, rows.clone(), tab_size);
            let mut open = Vec::new();
            for (header, indent) in std::mem::take(&mut carried) {
                if let (Some(end), still_open) = continue_scope(text, indent, &rows, tab_size) {
                    scopes.push(header..=end);
                    if still_open {
                        open.push((header, indent));
                    }
                }
            }
            for scope in &found {
                if *scope.end() == rows.end && !open.iter().any(|(h, _)| h == scope.start()) {
                    let indent = row_indent(text, *scope.start(), tab_size);
                    open.push((*scope.start(), indent));
                }
            }
            scopes.extend(found);
            carried = open;
            continue;
        }
        scopes.extend(match (model, syntax) {
            (Model::Outline, Some(syntax)) => match fe_syntax::outline_scopes(syntax, rows) {
                Ok(scopes) => scopes,
                Err(fe_syntax::NoOutline::NotParsed) => return None,
                Err(_) => Vec::new(),
            },
            (Model::Folding, Some(syntax)) => fe_syntax::fold_scopes(syntax, rows),
            (Model::Indentation(tab_size), _) => {
                fe_display::indent_scopes(view.model.text(), rows, tab_size)
            }
            (_, None) => Vec::new(),
        });
    }
    #[cfg(test)]
    {
        QUERIES.with(|queries| queries.set(queries.get() + 1));
        ASKED.with(|asked| asked.set(Some(model)));
        ASKED_ROWS.with(|rows| *rows.borrow_mut() = key.2.clone());
    }
    // One per header, as each run gives them. Indentation ends a block
    // still open at the row past a run there, a row the fold after it
    // hides, so that it ends with the fold's header unless the next run
    // finds it open past the fold too, further down.
    scopes.sort_by_key(|scope| (*scope.start(), std::cmp::Reverse(*scope.end())));
    scopes.dedup_by_key(|scope| *scope.start());
    *cache = Some(Scopes {
        key,
        scopes: scopes.clone(),
    });
    Some(scopes)
}

/// The column `row`'s text starts at, tabs advancing to the next multiple
/// of `tab_size`; a blank row's is `usize::MAX`.
fn row_indent(text: &fe_text::Text, row: u32, tab_size: usize) -> usize {
    let tab_size = tab_size.max(1);
    let end = match row + 1 < text.row_count() {
        true => text.row_start(row + 1),
        false => text.len(),
    };
    let mut column = 0;
    for chunk in text.chunks_in_range(text.row_start(row)..end) {
        for ch in chunk.chars() {
            match ch {
                '\n' => return usize::MAX,
                ' ' => column += 1,
                '\t' => column += tab_size - column % tab_size,
                ch if ch.is_whitespace() => {}
                _ => return column,
            }
        }
    }
    usize::MAX
}

/// How far a block headed at `indent`, open at the start of `rows`, goes
/// through them, and whether it's still open at their end: it ends with the
/// last text row before one indented no deeper, else is open to `rows.end`.
/// A block closed before its first text row there gets no end (`None`):
/// it ended where the last run did.
fn continue_scope(
    text: &fe_text::Text,
    indent: usize,
    rows: &Range<u32>,
    tab_size: usize,
) -> (Option<u32>, bool) {
    let mut last = None;
    for row in rows.clone() {
        match row_indent(text, row, tab_size) {
            usize::MAX => {}
            level if level <= indent => return (last, false),
            _ => last = Some(row),
        }
    }
    (Some(rows.end), true)
}

/// The rows the band pins from `scopes` over a view scrolled to `scroll_y`,
/// none whose bottom passes `keep_above`: none while it's off.
pub(crate) fn pinned(
    view: &EditorView,
    scopes: &[RangeInclusive<u32>],
    blocks: &BlockMap,
    scroll_y: f32,
    line_height: f32,
    keep_above: Option<f32>,
) -> Vec<StickyRow> {
    if !view.theme.sticky_scroll.enabled || scopes.is_empty() {
        return Vec::new();
    }
    let display = view.model.display();
    let (lines, rows) = (line_height, max_rows(view));
    fe_display::sticky_rows(scopes, display, blocks, scroll_y, lines, rows, keep_above)
}

/// The bottom of the band of `rows`, from the view's top: 0 with none.
pub(crate) fn bottom(rows: &[StickyRow], line_height: f32) -> f32 {
    (rows.iter())
        .map(|row| row.y + line_height)
        .fold(0., f32::max)
}

/// The band the view pins at `scroll_y`, found through `cache`: `None`
/// while its scopes can't be known yet ([`scopes`]).
pub(crate) fn band_at(
    view: &EditorView,
    cache: &mut Option<Scopes>,
    blocks: &BlockMap,
    scroll_y: f32,
    line_height: f32,
) -> Option<Vec<StickyRow>> {
    let scopes = scopes(view, cache, blocks, scroll_y, line_height)?;
    Some(pinned(view, &scopes, blocks, scroll_y, line_height, None))
}

/// The top of the caret at `head`'s row, in pixels from the text's top,
/// block rows counted.
pub(crate) fn caret_top(
    view: &EditorView,
    blocks: &BlockMap,
    head: usize,
    line_height: f32,
) -> f32 {
    let display_row = view.model.display().row_for_offset(head);
    blocks.screen_row(display_row) as f32 * line_height
}

/// Where a frame autoscrolled to `scroll_y` puts its top instead, so the
/// caret's row, at `caret_top`, has a row between it and the band: the
/// band's height for the scroll it ends at plus one row is its top margin,
/// as fe-display's three rows are. Not so far that the caret's row passes
/// `bottom_margin` above the bottom of a view `height` tall: a view too
/// short for both keeps the caret there, and the band then spares its row
/// ([`keep_above`]), which the second value says. `Ok(None)` where the
/// caret already clears the band, or the band is empty; `Err` while the
/// band can't be known yet ([`scopes`]), to ask again on a later frame.
#[expect(clippy::too_many_arguments)]
pub(crate) fn clear_of_caret(
    view: &EditorView,
    cache: &mut Option<Scopes>,
    blocks: &BlockMap,
    scroll_y: f32,
    line_height: f32,
    caret_top: f32,
    height: f32,
    bottom_margin: f32,
) -> Result<Option<(f32, bool)>, NotKnown> {
    let mut band = |scroll_y: f32| {
        let rows = band_at(view, cache, blocks, scroll_y, line_height).ok_or(NotKnown)?;
        Ok(bottom(&rows, line_height))
    };
    let mut pinned = band(scroll_y)?;
    if pinned == 0. || caret_top - scroll_y >= pinned + line_height {
        return Ok(None);
    }
    // Scrolled up, the band may pin more: up again by what it adds, until
    // it adds nothing, which it does within its most rows.
    let lowest = (caret_top + line_height + bottom_margin - height).max(0.);
    let mut top = scroll_y;
    for _ in 0..=max_rows(view) {
        let wanted = (caret_top - pinned - line_height).max(lowest);
        if wanted >= top {
            break;
        }
        top = wanted;
        let more = band(top)?;
        if more <= pinned {
            pinned = more;
            break;
        }
        pinned = more;
    }
    let spare = caret_top - top < pinned;
    Ok((top < scroll_y || spare).then_some((top, spare)))
}

/// The band's scopes aren't known yet: the syntax tree is being parsed.
#[derive(Debug)]
pub(crate) struct NotKnown;

/// Where a click on the pinned header `buffer_row` scrolls: its row
/// directly under the band its enclosing scopes then form, so it shows as
/// itself, not pinned (VS Code's `top = header − ancestors pinned`).
/// `None` while the band can't be known yet ([`scopes`]).
pub(crate) fn reveal_top(
    view: &EditorView,
    cache: &mut Option<Scopes>,
    blocks: &BlockMap,
    buffer_row: u32,
    line_height: f32,
) -> Option<f32> {
    let display_row = view.model.display().row_for_buffer_row(buffer_row);
    let header = blocks.screen_row(display_row) as f32 * line_height;
    let mut top = header;
    for above in 0..=max_rows(view) {
        top = (header - above as f32 * line_height).max(0.);
        let rows = band_at(view, cache, blocks, top, line_height)?;
        if top == 0. || bottom(&rows, line_height) <= header - top {
            break;
        }
    }
    Some(top)
}

/// With the view placed by a vim scroll, or too short to clear the band
/// (`spares`, [`EditorView::band_spares_caret`]), the caret's row's top in the view at `scroll_y`, which the band pins
/// nothing at or below; `None` otherwise, or with the caret above the view.
pub(crate) fn keep_above(spares: bool, scroll_y: f32, caret_top: f32) -> Option<f32> {
    let y = caret_top - scroll_y;
    (spares && y >= 0.).then_some(y)
}

/// One pinned row, in the frame's coordinates.
pub(crate) struct Row {
    /// The header's, which a click on the row reveals.
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
    /// Where the pointer is over the row, once inserted
    /// ([`insert_hitboxes`]).
    hitbox: Option<Hitbox>,
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

/// Lays out the band of `pinned` rows ([`pinned`]) over `layout`'s frame,
/// its rows shaped with `shaper`: `None` while it pins nothing.
pub(crate) fn layout(
    layout: &mut FrameLayout,
    view: &EditorView,
    pinned: Vec<StickyRow>,
    syntax: Option<&Syntax>,
    unnecessary: &[Range<usize>],
    shaper: &mut Shaper,
    em: f32,
) -> Option<Placed> {
    if pinned.is_empty() {
        return None;
    }
    let theme = &view.theme;
    let model = &view.model;
    let display = model.display();
    let frame = &layout.frame;
    let line_height = frame.line_height;
    let text = model.text();
    // The rows with a caret, whose numbers are the active colour.
    let caret_rows: Vec<u32> = (model.selections().iter())
        .filter(|selection| selection.range.is_empty())
        .map(|selection| text.offset_to_point(selection.head()).row)
        .collect();
    let numbers = display.line_numbers();
    let primary_line =
        display.visible_line(text.offset_to_point(model.primary_selection().head()).row);
    let tab_size = model.tab_size();
    let wrapped = display.wrap_width().is_some();
    let text_bounds = frame.text_bounds();
    // The text scrolls with the editor's, or with
    // `scroll_with_editor = false` stays at the first column.
    let scroll_x = match theme.sticky_scroll.scroll_with_editor {
        true => f64::from(frame.scroll.x),
        false => 0.,
    };
    let view_width = f64::from(text_bounds.width);
    let number_right = frame.problem_column.x;
    // The text from `slice`, tabs expanded, shaped in the colours the text
    // area gives it.
    let shape = |shaper: &mut Shaper, slice: &RowSlice| {
        let shown = slice.range.clone();
        let expanded = ExpandedRow::with_start_column(
            &text.slice_to_string(shown.clone()),
            slice.start_column,
            tab_size,
        );
        let foreground = foreground_spans(&model.decorations(shown.clone()), theme);
        let spans = styled_highlights(syntax, theme, unnecessary, &foreground, shown.clone());
        let first = spans.partition_point(|span| span.range.end <= shown.start);
        let runs = row_runs(&expanded, shown.start, &spans[first..], &theme.foreground);
        (shaper.shape(expanded.text(), &runs), expanded)
    };
    let mut rows = Vec::with_capacity(pinned.len());
    for row in pinned {
        // A wrapped header shows its first display row.
        let display_row = display.row(display.row_for_buffer_row(row.buffer_row));
        let range = display_row.range;
        // A long row, as in the text area, is shaped from a slice around the
        // view, and the rest of the row cut off.
        #[cfg_attr(not(test), allow(unused_variables))]
        let (slice, (line, expanded)) = if !wrapped && range.len() > MAX_SHAPED_ROW_BYTES {
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
            let view = scroll_x..scroll_x + view_width;
            let shape = |slice: &RowSlice| {
                let shaped = shape(shaper, slice);
                let width = f64::from(shaped.0.width());
                (shaped, width)
            };
            let (slice, shaped) = widened_slice(index, glyph_widths, view, margin, shape);
            (slice, shaped)
        } else {
            let len = range.len().min(MAX_SHAPED_ROW_BYTES);
            let end = text.clip_offset(range.start + len, Bias::Left);
            let slice = RowSlice {
                range: range.start..end,
                start_x: 0.,
                start_column: 0,
            };
            let shaped = shape(shaper, &slice);
            (slice, shaped)
        };
        let number = view.gutter.then(|| {
            let number = numbers.shown(
                row.buffer_row,
                display.visible_line(row.buffer_row),
                primary_line,
            )?;
            let color = match caret_rows.contains(&row.buffer_row) {
                true => theme.gutter_active_foreground,
                false => theme.gutter_foreground,
            };
            let number = number.to_string();
            let run = StyleRun {
                len: number.len(),
                style: color,
            };
            let line = shaper.shape(&number, &[run]);
            let x = number_right - f32::from(line.layout.width);
            Some((x, line))
        });
        rows.push(Row {
            buffer_row: row.buffer_row,
            y: row.y,
            line,
            x: layout.frame.text_left + (slice.start_x - scroll_x) as f32,
            #[cfg(test)]
            text: expanded.text().to_owned(),
            number: number.flatten(),
            hitbox: None,
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

/// The slice of a long row, indexed by `index`, to shape for the view
/// `view` (x in the row) with `margin` either side, and what `shape`, which
/// gives it and the width it's drawn, made of it. As in the text area
/// (fe-display's `layout_frame`): where the shaper draws the slice narrower
/// than the index measures it (contextual forms the index doesn't know),
/// its text ends short of the view. It's widened by twice the shortfall, in
/// index units per shaped unit, then, if it still ends short of the view,
/// to as much as a slice may hold, each time shaped with what it then
/// holds: three shapes at most.
pub(crate) fn widened_slice<S>(
    index: &mut RowIndex,
    widths: &mut dyn CharWidth,
    view: Range<f64>,
    margin: f64,
    mut shape: impl FnMut(&RowSlice) -> (S, f64),
) -> (RowSlice, S) {
    let mut slice = index.slice(view.clone(), margin, widths);
    let (mut shaped, mut drawn) = shape(&slice);
    let short = view.end + margin - (slice.start_x + drawn);
    let limit = match short > 0. {
        true => index.slice_limit(slice.range.start, widths),
        false => slice.range.end,
    };
    if slice.range.end < limit {
        slice = if drawn > 0. {
            let indexed = index.x_for_offset(slice.range.end, widths) - slice.start_x;
            index.widen(&slice, 2. * short * indexed / drawn, widths)
        } else {
            // Text shaped to nothing gives no ratio.
            RowSlice {
                range: slice.range.start..limit,
                ..slice
            }
        };
        (shaped, drawn) = shape(&slice);
        if slice.start_x + drawn < view.end && slice.range.end < limit {
            slice.range.end = limit;
            (shaped, _) = shape(&slice);
        }
    }
    (slice, shaped)
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

/// Inserts a hit area over each row of the band `layout` placed, which takes
/// the pointer from what's under it: the text, the gutter and the blocks.
/// The wheel goes on to the view, which scrolls.
/// Inserted after every other of the frame's, and the rows bottom first, so
/// a row pushed up under the one above it is under it here too.
pub(crate) fn insert_hitboxes(layout: &mut FrameLayout, window: &mut Window) {
    let Some(mut placed) = layout.sticky_scroll.take() else {
        return;
    };
    let line_height = layout.frame.line_height;
    for ix in (0..placed.rows.len()).rev() {
        let rect = layout.bounds(row_rect(&placed, placed.rows[ix].y, line_height));
        let behavior = HitboxBehavior::BlockMouseExceptScroll;
        placed.rows[ix].hitbox = Some(window.insert_hitbox(rect, behavior));
    }
    layout.sticky_scroll = Some(placed);
}

/// Listens for button-1 presses on the band `layout` placed, which it takes
/// from the view under it, and passes them to the view, where a press on a
/// row reveals the row's header ([`EditorView::reveal_sticky_header`]),
/// with any modifiers held.
/// The header of the band's row under `position` in the rendered frame,
/// if one is: the top row where rows overlap.
pub(crate) fn row_at(
    layout: &FrameLayout,
    position: Point<Pixels>,
    window: &Window,
) -> Option<u32> {
    let placed = layout.sticky_scroll.as_ref()?;
    let over = |row: &&Row| {
        (row.hitbox.as_ref()).is_some_and(|hitbox| hitbox.is_hovered_at(position, window))
    };
    placed.rows.iter().find(over).map(|row| row.buffer_row)
}

pub(crate) fn listen(view: &WeakEntity<EditorView>, layout: &FrameLayout, window: &mut Window) {
    let Some(placed) = &layout.sticky_scroll else {
        return;
    };
    let rows: Vec<Hitbox> = (placed.rows.iter())
        .filter_map(|row| row.hitbox.clone())
        .collect();
    let view = view.clone();
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
            return;
        }
        if !rows.iter().any(|hitbox| hitbox.is_hovered(window)) {
            return;
        }
        // As a press on the view: held keys resolve first, and the press
        // finds the row under it again in the frame laid out after them.
        view.update(cx, |view, cx| view.on_mouse_down(event, window, cx))
            .ok();
    });
}

/// The background of `row`: `sticky_scroll_hover` under the pointer.
fn background(row: &Row, theme: &EditorTheme, window: &Window) -> Hsla {
    match (row.hitbox.as_ref()).is_some_and(|hitbox| hitbox.is_hovered(window)) {
        true => theme.sticky_scroll_hover,
        false => theme.sticky_scroll_background,
    }
}

/// Paints the band `layout` placed: the rows bottom first, so a row pushed
/// up slides under the one above it, each over the band's background (or
/// the hover's, with a pointing hand, under the pointer), then the line and
/// the shadow under them. The pointer moving onto or off a row redraws.
pub(crate) fn paint(layout: &FrameLayout, theme: &EditorTheme, window: &mut Window, cx: &mut App) {
    let Some(placed) = &layout.sticky_scroll else {
        return;
    };
    let line_height = layout.frame.line_height;
    let text = layout.bounds(placed.text);
    let mut hovered = Vec::new();
    for row in placed.rows.iter().rev() {
        let rect = layout.bounds(row_rect(placed, row.y, line_height));
        window.paint_quad(fill(rect, background(row, theme, window)));
        if let Some(hitbox) = &row.hitbox {
            window.set_cursor_style(CursorStyle::PointingHand, hitbox);
            hovered.push((hitbox.clone(), hitbox.is_hovered(window)));
        }
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
    if !hovered.is_empty() {
        window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, _| {
            let moved = (hovered.iter()).any(|(hitbox, over)| hitbox.is_hovered(window) != *over);
            if phase == DispatchPhase::Bubble && moved {
                window.refresh();
            }
        });
    }
}

/// A row [`paint`] painted, in window coordinates.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PaintedRow {
    pub(crate) buffer_row: u32,
    pub(crate) bounds: Bounds<Pixels>,
    /// Its background: the hover's under the pointer.
    pub(crate) background: Hsla,
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
pub(crate) fn painted(
    layout: &FrameLayout,
    theme: &EditorTheme,
    window: &Window,
) -> Option<Painted> {
    let placed = layout.sticky_scroll.as_ref()?;
    let line_height = layout.frame.line_height;
    let rows = (placed.rows.iter())
        .map(|row| PaintedRow {
            buffer_row: row.buffer_row,
            bounds: layout.bounds(row_rect(placed, row.y, line_height)),
            background: background(row, theme, window),
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

#[cfg(test)]
#[path = "sticky_scroll_settings_tests.rs"]
mod settings_tests;
