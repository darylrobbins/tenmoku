//! The host's blocks ([`crate::view::HostBlock`]) in a frame: placed by
//! fe-display's block layer between the display rows, as screen rows
//! ([`block_map`]), then painted across the text area: a filler's faint
//! diagonal hatch, or a band of text on its tint. Positions down the frame
//! go by screen row, so a block's rows push the text's down ([`row_at`]).
//!
//! The view's lens rows ([`crate::lens`]) are blocks too, a row each above
//! their rows ([`lens_blocks`]), with their items from the row's first
//! non-blank column ([`lens_layout`]).

use std::ops::Range;

use fe_display::{
    Block, CharWidth, DisplayMap, ExpandedRow, MAX_CLUSTER_BYTES, ScreenRow,
    layout::{
        Frame, MAX_SHAPED_ROW_BYTES, MeasuredLine as _, Rect, RowLayout, StyleRun, TextMeasure,
        Vec2,
    },
};
use fe_editor::decorations::Tint;
use gpui::{
    App, Bounds, ContentMask, CursorStyle, DispatchPhase, Hitbox, HitboxBehavior, Hsla,
    MouseMoveEvent, PathBuilder, Pixels, Window, fill, point, px, size,
};

use super::{FrameLayout, Line, Shaper};
use crate::{
    EditorTheme, EditorView,
    lens::{self, Lens, LensItem, PaintedRow},
    view::HostBlockKind,
};

/// Lens rows' block ids: this plus the lens's index in the frame's
/// [`lenses`]. Below are the host's and the proposals'.
const LENS_IDS: u64 = 1 << 40;
/// A lens row's text size, of the editor font's.
pub(super) const LENS_SCALE: f32 = 0.9;
/// Between a lens row's items.
const LENS_SEPARATOR: &str = " | ";

/// How opaque a text block's band is over its tint.
const BAND_ALPHA: f32 = 0.15;
/// How opaque a filler's hatch is over the text colour.
const HATCH_ALPHA: f32 = 0.12;
/// How far apart a filler's hatch lines are, across, in line heights.
const HATCH_SPACING: f32 = 0.5;

/// The view's blocks, each above the first display row of its buffer row,
/// or after the last row for the row count, by their index in the view's
/// list. One above a row a fold hides is left out. The frame's block map
/// holds these and others ([`super::block_map`]).
pub(super) fn blocks(view: &EditorView) -> impl Iterator<Item = Block> + '_ {
    let display = view.model.display();
    let rows = view.model.text().row_count();
    view.blocks
        .iter()
        .enumerate()
        .filter_map(move |(ix, block)| {
            let above = match block.above >= rows {
                true => display.row_count(),
                false => {
                    let row = display.row_for_buffer_row(block.above);
                    let shown = display.row(row);
                    (shown.buffer_row == block.above && shown.first).then_some(row)?
                }
            };
            Some(Block {
                above,
                rows: block.rows,
                id: ix as u64,
            })
        })
}

/// The display row a buffer row starts, unless a fold hides it.
fn first_display_row(display: &DisplayMap, row: u32) -> Option<u32> {
    let at = display.row_for_buffer_row(row);
    let shown = display.row(at);
    (shown.buffer_row == row && shown.first).then_some(at)
}

/// The lenses the view's providers have for its snapshot, joined by row in
/// provider order ([`lens::join`]), but those past the last row or inside a
/// fold.
pub(super) fn lenses(view: &EditorView) -> Vec<Lens> {
    let snapshot = view.model.snapshot();
    let (rows, display) = (view.model.text().row_count(), view.model.display());
    let all =
        (view.lens_providers().iter()).flat_map(|provider| provider.borrow_mut().lenses(snapshot));
    (lens::join(all).into_iter())
        .filter(|lens| lens.row < rows && first_display_row(display, lens.row).is_some())
        .collect()
}

/// A row's block for each of `lenses`, above its row's first display row.
pub(super) fn lens_blocks<'a>(
    lenses: &'a [Lens],
    view: &'a EditorView,
) -> impl Iterator<Item = Block> + 'a {
    let display = view.model.display();
    lenses.iter().enumerate().filter_map(move |(ix, lens)| {
        Some(Block {
            above: first_display_row(display, lens.row)?,
            rows: 1,
            id: LENS_IDS + ix as u64,
        })
    })
}

/// A lens row laid out in a frame, to paint.
pub(super) struct PlacedLens {
    /// In the frame.
    rect: Rect,
    items: Vec<PlacedItem>,
    /// The separators between them, and where each is painted.
    separators: Vec<(Vec2, Line)>,
}

/// A lens row's item, shaped.
struct PlacedItem {
    item: LensItem,
    /// Where its text is painted.
    at: Vec2,
    line: Line,
    /// The part of its box in view, inside its row and the text area, which
    /// a click runs.
    shown: Option<Rect>,
    /// Over `shown`, for its underline and pointing hand.
    hitbox: Option<Hitbox>,
}

/// The lens rows in view, each item shaped by `small` in the muted text
/// colour from its row's first non-blank column, as `shaper` measures the
/// row's indentation, then ` | ` between them.
pub(super) fn lens_layout(
    frame: &Frame<Line>,
    lenses: &[Lens],
    view: &EditorView,
    shaper: &mut Shaper,
    small: &mut Shaper,
) -> Vec<PlacedLens> {
    let (text, tab_size) = (view.model.text(), view.model.tab_size());
    let color = view.theme.gutter_foreground;
    let run = |text: &str| {
        [StyleRun {
            len: text.len(),
            style: color,
        }]
    };
    let text_area = frame.text_bounds();
    let mut out = Vec::new();
    for laid in &frame.blocks {
        let ix = laid
            .id
            .checked_sub(LENS_IDS)
            .and_then(|ix| usize::try_from(ix).ok());
        let Some(lens) = ix.and_then(|ix| lenses.get(ix)) else {
            continue;
        };
        if laid.rect.y + laid.rect.height <= 0. || laid.rect.y >= frame.height {
            continue;
        }
        let start = text.row_start(lens.row);
        let row = start..start + text.row_len(lens.row) as usize;
        let indent: String = (text.chunks_in_range(row).flat_map(str::chars))
            .take_while(|&ch| ch == ' ' || ch == '\t')
            .collect();
        let indent = ExpandedRow::new(&indent, tab_size);
        let mut x = frame.view_x(f64::from(shaper.shape(indent.text(), &run("")).width()));
        let y = laid.rect.y;
        let mut placed = PlacedLens {
            rect: laid.rect,
            items: Vec::new(),
            separators: Vec::new(),
        };
        for (n, item) in lens.items.iter().enumerate() {
            if n > 0 {
                let line = small.shape(LENS_SEPARATOR, &run(LENS_SEPARATOR));
                let width = line.width();
                placed.separators.push((Vec2 { x, y }, line));
                x += width;
            }
            let line = small.shape(&item.title, &run(&item.title));
            let rect = Rect {
                x,
                y,
                width: line.width(),
                height: frame.line_height,
            };
            x += rect.width;
            placed.items.push(PlacedItem {
                item: item.clone(),
                at: Vec2 { x: rect.x, y },
                line,
                shown: (rect.intersect(laid.rect)).and_then(|rect| rect.intersect(text_area)),
                hitbox: None,
            });
        }
        out.push(placed);
    }
    out
}

/// Puts a hitbox over the part of each lens item in view.
pub(super) fn insert_lens_hitboxes(
    placed: &mut [PlacedLens],
    to_window: impl Fn(Rect) -> Bounds<Pixels>,
    window: &mut Window,
) {
    for item in placed.iter_mut().flat_map(|lens| &mut lens.items) {
        item.hitbox =
            (item.shown).map(|rect| window.insert_hitbox(to_window(rect), HitboxBehavior::Normal));
    }
}

/// Paints the lens rows, each clipped to its row inside the text area, the
/// item under the pointer underlined with a pointing hand, noting in
/// `painted` the part of each row and item in view, which a click goes by.
/// The pointer moving onto or off an item redraws.
pub(super) fn paint_lenses(
    placed: &[PlacedLens],
    layout: &FrameLayout,
    window: &mut Window,
    cx: &mut App,
    painted: &mut Vec<PaintedRow>,
) {
    let line_height = px(layout.frame.line_height);
    let text_area = layout.frame.text_bounds();
    let mut hovered = Vec::new();
    for lens in placed {
        let bounds = layout.bounds(lens.rect);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (at, line) in &lens.separators {
                line.paint(layout.to_window(*at), line_height, window, cx);
            }
            for item in &lens.items {
                let origin = layout.to_window(item.at);
                item.line.paint(origin, line_height, window, cx);
                let Some(hitbox) = &item.hitbox else {
                    continue;
                };
                let over = hitbox.is_hovered(window);
                hovered.push((hitbox.clone(), over));
                if over {
                    let line = &item.line.layout;
                    let (ascent, descent) = (line.ascent, line.descent);
                    let baseline = origin.y + (line_height - ascent - descent) / 2. + ascent;
                    let underline =
                        Bounds::new(point(origin.x, baseline + px(1.)), size(line.width, px(1.)));
                    let color = item.line.color_at(0).unwrap_or_default();
                    window.paint_quad(fill(underline, color));
                    window.set_cursor_style(CursorStyle::PointingHand, hitbox);
                }
            }
        });
        if let Some(shown) = lens.rect.intersect(text_area) {
            painted.push(PaintedRow {
                bounds: layout.bounds(shown),
                items: (lens.items.iter())
                    .filter_map(|item| Some((item.item.clone(), layout.bounds(item.shown?))))
                    .collect(),
            });
        }
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

/// The view's block a frame's block of `id` is, if it's one of the host's.
fn host_block(view: &EditorView, id: u64) -> Option<&crate::view::HostBlock> {
    view.blocks.get(usize::try_from(id).ok()?)
}

/// The display row laid out at `y` in the frame, if a row this frame shows
/// is there: none on a block's rows, nor above the frame.
pub(super) fn row_at(frame: &Frame<Line>, y: f32) -> Option<&RowLayout<Line>> {
    if y + frame.scroll.y < 0. {
        return None;
    }
    match frame.screen_row_at(y) {
        ScreenRow::Text(row) => frame.row(row),
        ScreenRow::Block { .. } => None,
    }
}

/// A block laid out in a frame, to paint.
pub(super) struct Placed {
    /// The host's, which tests see.
    #[cfg(test)]
    id: u64,
    /// In the frame.
    rect: Rect,
    kind: PlacedKind,
}

enum PlacedKind {
    Filler,
    /// The band's colour, and each line shown.
    Lines {
        band: Hsla,
        lines: Vec<Shown>,
    },
}

/// A text block's line, shaped, and where it's painted.
struct Shown {
    at: Vec2,
    line: Line,
    /// As shaped, which tests see.
    #[cfg(test)]
    text: String,
}

/// What a frame painted of a block, for tests.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PaintedBlock {
    /// The host's.
    pub(crate) id: u64,
    pub(crate) bounds: Bounds<Pixels>,
    /// A text block's band colour; `None` for a filler.
    pub(crate) band: Option<Hsla>,
    /// A filler's hatch lines.
    pub(crate) hatch: usize,
    /// A text block's lines shown, and the window position each starts at.
    pub(crate) lines: Vec<(String, gpui::Point<Pixels>)>,
}

/// The lines of the text blocks `frame` shows that are in view, each with
/// its block's index in `frame.blocks`, its y and its text, tabs expanded.
fn shown_lines(frame: &Frame<Line>, view: &EditorView) -> Vec<(usize, f32, String)> {
    let tab_size = view.model.tab_size();
    let line_height = frame.line_height;
    let mut out = Vec::new();
    for (ix, laid) in frame.blocks.iter().enumerate() {
        let Some(HostBlockKind::Lines { lines, .. }) =
            host_block(view, laid.id).map(|block| &block.kind)
        else {
            continue;
        };
        let shown = lines.iter().take(laid.rows as usize).enumerate();
        for (row, line) in shown {
            let y = laid.rect.y + line_height * row as f32;
            if y + line_height > 0. && y < frame.height {
                out.push((ix, y, ExpandedRow::new(line, tab_size).text().to_string()));
            }
        }
    }
    out
}

/// How wide the widest line of the text blocks in view is, from column 0:
/// the view scrolls as far as it reaches too. A line shaped whole is as
/// wide as `shaper` shapes it, which the frame's own shaping of it reuses;
/// a longer one, as its grapheme clusters measure ([`clusters`]).
pub(super) fn widest(
    frame: &Frame<Line>,
    view: &EditorView,
    shaper: &mut Shaper,
    widths: &mut dyn CharWidth,
) -> f64 {
    (shown_lines(frame, view).iter())
        .map(|(_, _, text)| match text.len() > MAX_SHAPED_ROW_BYTES {
            true => clusters(text, widths).last().map_or(0., |(_, _, end)| end),
            false => f64::from(shaper.shape(text, &runs(text, view)).width()),
        })
        .fold(0., f64::max)
}

/// The style run of a line of `text`: the text colour.
fn runs(text: &str, view: &EditorView) -> [StyleRun<Hsla>; 1] {
    [StyleRun {
        len: text.len(),
        style: view.theme.foreground,
    }]
}

/// Each grapheme cluster of `text`, as fe-display's long rows measure
/// them: its bytes, and the x it starts and ends at from column 0. A
/// cluster longer than [`MAX_CLUSTER_BYTES`] is cut into pieces of at most
/// that many bytes at char boundaries, each as wide as its chars measured
/// alone; the others measure as [`CharWidth::cluster_width`] has them.
pub(super) fn clusters<'a>(
    text: &'a str,
    widths: &'a mut dyn CharWidth,
) -> impl Iterator<Item = (Range<usize>, f64, f64)> + 'a {
    use unicode_segmentation::UnicodeSegmentation as _;
    let mut x = 0.;
    let units = text.grapheme_indices(true).flat_map(|(at, cluster)| {
        let mut rest = cluster;
        let mut cut = at;
        std::iter::from_fn(move || {
            if rest.is_empty() {
                return None;
            }
            let whole = rest.len() <= MAX_CLUSTER_BYTES;
            let len = match whole {
                true => rest.len(),
                false => rest.floor_char_boundary(MAX_CLUSTER_BYTES),
            };
            let (piece, tail) = rest.split_at(len);
            let range = cut..cut + len;
            rest = tail;
            cut += len;
            Some((range, piece, whole && piece.len() == cluster.len()))
        })
    });
    units.map(move |(range, piece, is_cluster)| {
        let start = x;
        x += f64::from(match is_cluster {
            true => widths.cluster_width(piece),
            false => piece.chars().map(|ch| widths.char_width(ch)).sum::<f32>(),
        });
        (range, start, x)
    })
}

/// Of a line longer than [`MAX_SHAPED_ROW_BYTES`], the part around the
/// view scrolled to `scroll_x`, a view's width either side, as much of it
/// as is shaped at once, cut between [`clusters`]: its bytes, at most
/// [`MAX_SHAPED_ROW_BYTES`] of them, on char boundaries, and the x it
/// starts at, from column 0.
pub(super) fn slice(
    text: &str,
    frame: &Frame<Line>,
    scroll_x: f32,
    widths: &mut dyn CharWidth,
) -> (Range<usize>, f64) {
    let width = f64::from(frame.width - frame.text_left);
    let left = f64::from(scroll_x) - width;
    let right = f64::from(scroll_x) + width * 2.;
    let mut start = None;
    for (cluster, x, end_x) in clusters(text, widths) {
        match start {
            None if end_x > left => start = Some((cluster.start, x)),
            Some((from, from_x)) if x >= right || cluster.end - from > MAX_SHAPED_ROW_BYTES => {
                return (from..cluster.start, from_x);
            }
            _ => {}
        }
    }
    let (from, from_x) = start.unwrap_or((text.len(), 0.));
    (from..text.len(), from_x)
}

/// Lays out the blocks `frame` shows, shaping the lines of each in view,
/// scrolled to `scroll_x`: a long one around the view ([`slice`]). The
/// lines scroll with the rows, but for soft-wrapped rows, which don't.
pub(super) fn layout(
    frame: &Frame<Line>,
    view: &EditorView,
    shaper: &mut Shaper,
    widths: &mut dyn CharWidth,
    scroll_x: f32,
) -> Vec<Placed> {
    // Indexed as `frame.blocks`; others' blocks are left out.
    let mut placed: Vec<Option<Placed>> = (frame.blocks.iter())
        .map(|laid| {
            let block = host_block(view, laid.id)?;
            let kind = match &block.kind {
                HostBlockKind::Lines { tint, .. } => PlacedKind::Lines {
                    band: tint_color(&view.theme, *tint).opacity(BAND_ALPHA),
                    lines: Vec::new(),
                },
                HostBlockKind::Filler => PlacedKind::Filler,
            };
            Some(Placed {
                #[cfg(test)]
                id: block.id,
                rect: laid.rect,
                kind,
            })
        })
        .collect();
    for (ix, y, text) in shown_lines(frame, view) {
        let (range, x) = match text.len() > MAX_SHAPED_ROW_BYTES {
            true => slice(&text, frame, scroll_x, widths),
            false => (0..text.len(), 0.),
        };
        let text = &text[range];
        let shown = Shown {
            at: Vec2 {
                x: frame.text_left + (x - f64::from(scroll_x)) as f32,
                y,
            },
            line: shaper.shape(text, &runs(text, view)),
            #[cfg(test)]
            text: text.to_string(),
        };
        if let Some(PlacedKind::Lines { lines, .. }) =
            placed[ix].as_mut().map(|placed| &mut placed.kind)
        {
            lines.push(shown);
        }
    }
    placed.into_iter().flatten().collect()
}

/// A tint's colour, as the decorations resolve it.
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

/// Paints `placed`, each clipped to its rect inside the text area, noting
/// in tests what it painted in `painted`.
pub(super) fn paint(
    placed: &[Placed],
    layout: &FrameLayout,
    theme: &EditorTheme,
    window: &mut Window,
    cx: &mut App,
    #[cfg(test)] painted: &mut Vec<PaintedBlock>,
) {
    let line_height = px(layout.frame.line_height);
    for block in placed {
        let bounds = layout.bounds(block.rect);
        window.with_content_mask(Some(ContentMask { bounds }), |window| match &block.kind {
            PlacedKind::Filler => {
                let shown = layout.bounds(layout.frame.text_bounds());
                let _hatch = paint_hatch(bounds, shown, line_height, theme, window);
                #[cfg(test)]
                painted.push(PaintedBlock {
                    id: block.id,
                    bounds,
                    band: None,
                    hatch: _hatch,
                    lines: Vec::new(),
                });
            }
            PlacedKind::Lines { band, lines } => {
                window.paint_quad(fill(bounds, *band));
                for shown in lines {
                    let origin = layout.to_window(shown.at);
                    shown.line.paint(origin, line_height, window, cx);
                }
                #[cfg(test)]
                painted.push(PaintedBlock {
                    id: block.id,
                    bounds,
                    band: Some(*band),
                    hatch: 0,
                    lines: (lines.iter())
                        .map(|shown| (shown.text.clone(), layout.to_window(shown.at)))
                        .collect(),
                });
            }
        });
    }
}

/// Strokes a filler's hatch over what's `shown` of `bounds`: lines rising
/// to the right, [`HATCH_SPACING`] line heights apart, placed from the
/// block's corner so they scroll with it. Returns how many.
fn paint_hatch(
    bounds: Bounds<Pixels>,
    shown: Bounds<Pixels>,
    line_height: Pixels,
    theme: &EditorTheme,
    window: &mut Window,
) -> usize {
    let shown = bounds.intersect(&shown);
    if shown.is_empty() {
        return 0;
    }
    let spacing = f32::from((line_height * HATCH_SPACING).max(px(4.)));
    let color = theme.foreground.opacity(HATCH_ALPHA);
    // Each line is where x + y is the block's corner's plus a whole number
    // of spacings; those crossing what's shown.
    let corner = f32::from(bounds.left() + bounds.bottom());
    let (top, bottom) = (shown.top(), shown.bottom());
    let first = f32::from(shown.left() + top);
    let mut sum = corner + ((first - corner) / spacing).floor() * spacing;
    let end = f32::from(shown.right() + bottom);
    let mut count = 0;
    while sum < end {
        let mut path = PathBuilder::stroke(px(1.));
        path.move_to(point(px(sum) - bottom, bottom));
        path.line_to(point(px(sum) - top, top));
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
            count += 1;
        }
        sum += spacing;
    }
    count
}
