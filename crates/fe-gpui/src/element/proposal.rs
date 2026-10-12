//! The view's proposals ([`crate::view`]'s `proposal`) in a frame: each
//! pending or stale hunk as a block of rows placed by fe-display's block
//! layer under the text it replaces ([`blocks`]), a header row then its new
//! text a line a row on an "added" band, and its old rows under a full-width
//! "removed" band. A hunk whose old rows a fold hides shows neither.

use std::{borrow::Cow, iter, ops::Range, sync::Arc};

use fe_buffer::{HunkStatus, Outcome, Text};
use fe_display::{
    Block, CharWidth, DisplayMap, ExpandedRow,
    layout::{Frame, MAX_SHAPED_ROW_BYTES, MeasuredLine as _, Rect, StyleRun, TextMeasure, Vec2},
};
use gpui::{App, Bounds, ContentMask, Hsla, Window, fill, px};

use super::{FrameLayout, Line, Shaper, host_blocks};
use crate::{EditorTheme, EditorView};

/// Block ids from here on are the proposals' hunks, [`IDS`] plus the
/// hunk's index in the frame's [`Hunk`]s; below are the host's.
const IDS: u64 = 1 << 32;

/// Behind a hunk's block: its new text. Until the themes have diff colours.
const ADDED: Hsla = Hsla {
    h: 130. / 360.,
    s: 0.45,
    l: 0.45,
    a: 0.2,
};
/// Behind a hunk's old rows.
const REMOVED: Hsla = Hsla {
    h: 0.,
    s: 0.6,
    l: 0.5,
    a: 0.2,
};
/// How opaque a stale hunk's header is over the text colour.
const DIM_ALPHA: f32 = 0.6;
/// A pending hunk's header.
const PENDING: &str = "Proposed change";

/// A pending or stale hunk of a proposal the view shows, as of its
/// snapshot.
pub(super) struct Hunk {
    /// Which tests see.
    #[cfg(test)]
    id: fe_buffer::HunkId,
    status: HunkStatus,
    /// Where its old text is.
    range: Range<usize>,
    /// What its header row says.
    header: String,
    /// Its new text, a line a row.
    new_text: Arc<str>,
    /// The header's and the new text's.
    rows: u32,
}

/// The pending and stale hunks of the view's proposals, in its snapshot. A
/// stale one's header gives the dry run's reason. A proposal anchored in a
/// later snapshot than the view's isn't shown until the view catches up.
pub(super) fn hunks(view: &EditorView) -> Vec<Hunk> {
    let snapshot = view.model.snapshot();
    let mut out = Vec::new();
    for proposal in &view.proposals {
        let proposal = proposal.borrow();
        if proposal.buffer() != snapshot.id() || proposal.version() > snapshot.version() {
            continue;
        }
        let hunks = proposal.hunks(snapshot);
        // Only for a stale hunk's reason: a dry run applies the clean ones
        // to a copy of the text.
        let stale = hunks.iter().any(|hunk| hunk.status == HunkStatus::Stale);
        let dry_run = stale.then(|| proposal.dry_run(snapshot));
        for hunk in hunks {
            let header = match hunk.status {
                HunkStatus::Pending => PENDING.to_string(),
                HunkStatus::Stale => (dry_run.iter())
                    .flat_map(|dry_run| dry_run.hunks())
                    .find(|outcome| outcome.id == hunk.id)
                    .and_then(|outcome| match &outcome.outcome {
                        Outcome::Conflict(conflict) => Some(conflict.to_string()),
                        Outcome::Clean | Outcome::Resolved(_) => None,
                    })
                    .unwrap_or_else(|| fe_buffer::ProposalError::Stale.to_string()),
                HunkStatus::Accepted | HunkStatus::Rejected => continue,
            };
            out.push(Hunk {
                #[cfg(test)]
                id: hunk.id,
                status: hunk.status,
                range: hunk.range,
                header,
                rows: 1 + lines(&hunk.new_text).count() as u32,
                new_text: hunk.new_text,
            });
        }
    }
    out
}

/// `text`'s lines: none if it's empty, and none after a final line break.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    let text = (!text.is_empty()).then(|| text.strip_suffix('\n').unwrap_or(text));
    text.into_iter().flat_map(|text| text.split('\n'))
}

/// The first and last buffer rows of `range`'s old text. A range ending at
/// the start of a row ends on the row before, unless it's empty.
fn old_rows(range: &Range<usize>, text: &Text) -> (u32, u32) {
    let first = text.offset_to_point(range.start).row;
    let end = text.offset_to_point(range.end);
    let last = match range.is_empty() || end.column != 0 {
        true => end.row,
        false => end.row - 1,
    };
    (first, last)
}

/// Whether a fold hides every one of buffer rows `first..=last`.
fn folded_away(first: u32, last: u32, display: &DisplayMap) -> bool {
    let hidden = |row: u32| display.row(display.row_for_buffer_row(row)).buffer_row != row;
    // Hidden rows behind the same visible row are in one run.
    hidden(first)
        && hidden(last)
        && display.row_for_buffer_row(first) == display.row_for_buffer_row(last)
}

/// The display row a hunk's block goes above: the first after its old
/// text, that is the row `range.end` starts if it starts one, else the
/// first of the next buffer row, or after the last row at the end. Past a
/// fold the hunk's last row heads, the first after the fold.
fn above(range: &Range<usize>, text: &Text, display: &DisplayMap) -> u32 {
    let end = text.offset_to_point(range.end);
    let before = match end.column {
        0 => end.row.checked_sub(1),
        _ => Some(end.row),
    };
    // A hidden row's offset is on the visible row before it: a fold's
    // header, whose last display row the fold follows.
    before.map_or(0, |row| {
        let row_end = text.row_start(row) + text.row_len(row) as usize;
        display.row_for_offset(row_end) + 1
    })
}

/// The blocks of `hunks` in the view's display rows, by [`IDS`] plus each
/// one's index: a header row and a row a line of new text.
pub(super) fn blocks<'a>(
    hunks: &'a [Hunk],
    view: &'a EditorView,
) -> impl Iterator<Item = Block> + 'a {
    let (text, display) = (view.model.text(), view.model.display());
    hunks.iter().enumerate().filter_map(move |(ix, hunk)| {
        let (first, last) = old_rows(&hunk.range, text);
        if folded_away(first, last, display) {
            return None;
        }
        Some(Block {
            above: above(&hunk.range, text, display),
            rows: hunk.rows,
            id: IDS + ix as u64,
        })
    })
}

/// The hunk a frame's block of `id` shows, if it's a proposal's.
fn hunk(hunks: &[Hunk], id: u64) -> Option<&Hunk> {
    hunks.get(usize::try_from(id.checked_sub(IDS)?).ok()?)
}

/// The hunks' blocks and bands in a frame, to paint.
#[derive(Default)]
pub(crate) struct Layout {
    blocks: Vec<Placed>,
    /// Full-width bands behind the old rows of the hunks shown.
    pub(crate) removed: Vec<Rect>,
}

/// A hunk's block laid out in a frame.
struct Placed {
    /// The hunk's, which tests see.
    #[cfg(test)]
    id: fe_buffer::HunkId,
    status: HunkStatus,
    /// Which tests see.
    #[cfg(test)]
    header: String,
    /// In the frame.
    rect: Rect,
    /// The rows in view: the header's, then the new text's.
    rows: Vec<Row>,
}

/// A block's row, shaped, and where it's painted.
struct Row {
    at: Vec2,
    line: Line,
    /// As shaped, which tests see.
    #[cfg(test)]
    text: String,
}

#[cfg(test)]
impl Layout {
    /// The text of each block's rows in view, in the order painted.
    pub(crate) fn texts(&self) -> Vec<Vec<String>> {
        let blocks = self.blocks.iter();
        blocks
            .map(|block| block.rows.iter().map(|row| row.text.clone()).collect())
            .collect()
    }
}

/// The rows of the hunks' blocks `frame` shows that are in view: each
/// block's index in `frame.blocks`, the row's y, its text (a line of new
/// text with its tabs expanded), and whether it's the header.
fn shown_rows<'a>(
    frame: &'a Frame<Line>,
    hunks: &'a [Hunk],
    tab_size: usize,
) -> impl Iterator<Item = (usize, f32, Cow<'a, str>, bool)> + 'a {
    let line_height = frame.line_height;
    frame.blocks.iter().enumerate().flat_map(move |(ix, laid)| {
        let rows = hunk(hunks, laid.id)
            .into_iter()
            .flat_map(|hunk| iter::once(hunk.header.as_str()).chain(lines(&hunk.new_text)));
        // Those from the first below the frame's top to the last above its
        // bottom, a long block's others left as they are.
        let first = (-laid.rect.y / line_height).floor().max(0.) as usize;
        let end = ((frame.height - laid.rect.y) / line_height).ceil().max(0.) as usize;
        let shown = rows.enumerate().skip(first);
        shown
            .take(end.saturating_sub(first))
            .map(move |(row, text)| {
                let y = laid.rect.y + line_height * row as f32;
                let text = match row {
                    0 => Cow::Borrowed(text),
                    _ => Cow::Owned(ExpandedRow::new(text, tab_size).text().to_string()),
                };
                (ix, y, text, row == 0)
            })
    })
}

/// The style run of a row of `text`: a stale header's dimmed.
fn runs(text: &str, header: bool, status: HunkStatus, theme: &EditorTheme) -> [StyleRun<Hsla>; 1] {
    let style = match header && status == HunkStatus::Stale {
        true => theme.foreground.opacity(DIM_ALPHA),
        false => theme.foreground,
    };
    [StyleRun {
        len: text.len(),
        style,
    }]
}

/// How wide the widest new text line of the blocks in view is, from column
/// 0, as [`host_blocks::widest`] measures the host's.
pub(super) fn widest(
    frame: &Frame<Line>,
    hunks: &[Hunk],
    view: &EditorView,
    shaper: &mut Shaper,
    widths: &mut dyn CharWidth,
) -> f64 {
    let theme = &view.theme;
    shown_rows(frame, hunks, view.model.tab_size())
        .filter(|&(_, _, _, header)| !header)
        .map(|(_, _, text, _)| match text.len() > MAX_SHAPED_ROW_BYTES {
            true => (host_blocks::clusters(&text, widths))
                .last()
                .map_or(0., |(_, _, end)| end),
            false => {
                let runs = runs(&text, false, HunkStatus::Pending, theme);
                f64::from(shaper.shape(&text, &runs).width())
            }
        })
        .fold(0., f64::max)
}

/// Lays out the hunks' blocks `frame` shows, their rows in view scrolled
/// to `scroll_x` as the host's blocks' lines are, and the bands behind the
/// old rows of the hunks shown.
pub(super) fn layout(
    frame: &Frame<Line>,
    hunks: &[Hunk],
    view: &EditorView,
    shaper: &mut Shaper,
    widths: &mut dyn CharWidth,
    scroll_x: f32,
) -> Layout {
    let (buffer, theme) = (view.model.text(), &view.theme);
    let mut placed: Vec<Option<Placed>> = (frame.blocks.iter())
        .map(|laid| {
            let hunk = hunk(hunks, laid.id)?;
            Some(Placed {
                #[cfg(test)]
                id: hunk.id,
                status: hunk.status,
                #[cfg(test)]
                header: hunk.header.clone(),
                rect: laid.rect,
                rows: Vec::new(),
            })
        })
        .collect();
    for (ix, y, text, header) in shown_rows(frame, hunks, view.model.tab_size()) {
        let Some(block) = placed[ix].as_mut() else {
            continue;
        };
        let (range, x) = match text.len() > MAX_SHAPED_ROW_BYTES {
            true => host_blocks::slice(&text, frame, scroll_x, widths),
            false => (0..text.len(), 0.),
        };
        let text = &text[range];
        let at = Vec2 {
            x: frame.text_left + (x - f64::from(scroll_x)) as f32,
            y,
        };
        let runs = runs(text, header, block.status, theme);
        block.rows.push(Row {
            at,
            line: shaper.shape(text, &runs),
            #[cfg(test)]
            text: text.to_string(),
        });
    }
    // The buffer rows of the hunks' old text. The frame lays out only the
    // rows a fold doesn't hide.
    let old: Vec<(u32, u32)> = (hunks.iter())
        .filter(|hunk| !hunk.range.is_empty())
        .map(|hunk| old_rows(&hunk.range, buffer))
        .collect();
    let removed = (frame.rows.iter())
        .filter(|row| (old.iter()).any(|&(first, last)| (first..=last).contains(&row.buffer_row)))
        .map(|row| Rect {
            x: 0.,
            y: frame.row_top(row.row),
            width: frame.width,
            height: frame.line_height,
        })
        .collect();
    Layout {
        blocks: placed.into_iter().flatten().collect(),
        removed,
    }
}

/// Paints the bands behind the hunks' old rows, under the text.
pub(super) fn paint_bands(proposals: &Layout, layout: &FrameLayout, window: &mut Window) {
    for &band in &proposals.removed {
        window.paint_quad(fill(layout.bounds(band), REMOVED));
    }
}

/// Paints the hunks' blocks, each clipped to its rect inside the text
/// area, noting in tests what it painted in `painted`.
pub(super) fn paint(
    proposals: &Layout,
    layout: &FrameLayout,
    window: &mut Window,
    cx: &mut App,
    #[cfg(test)] painted: &mut Vec<(fe_buffer::HunkId, HunkStatus, String, Bounds<gpui::Pixels>)>,
) {
    let line_height = px(layout.frame.line_height);
    for block in &proposals.blocks {
        let bounds: Bounds<gpui::Pixels> = layout.bounds(block.rect);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, ADDED));
            for row in &block.rows {
                (row.line).paint(layout.to_window(row.at), line_height, window, cx);
            }
        });
        #[cfg(test)]
        painted.push((block.id, block.status, block.header.clone(), bounds));
    }
}
