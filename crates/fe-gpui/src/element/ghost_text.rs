//! The view's ghost text ([`crate::view`]'s `ghost_text`) in a frame: what
//! remains of the suggestion, its first line the frame's inlay at the caret
//! ([`inlay`]) and its other lines a block of rows placed by fe-display's
//! block layer under the caret's row ([`block`]), all in the gutter's
//! colour. An input method composing takes the inlay, and hides it.

use fe_buffer::Text;
use fe_display::{
    Block, CharWidth, DisplayMap, ExpandedRow,
    layout::{self, Frame, MAX_SHAPED_ROW_BYTES, MeasuredLine as _, StyleRun, TextMeasure, Vec2},
};
use gpui::{App, Hsla, Window, px};
#[cfg(test)]
use gpui::{Bounds, Pixels};

use super::{FrameLayout, Line, Shaper, host_blocks};
use crate::EditorView;

/// The id of the suggestion's block: past the host's and the proposals'.
const ID: u64 = u64::MAX;

/// What remains of the view's suggestion, as a frame shows it.
pub(super) struct Ghost {
    /// The caret it's at.
    offset: usize,
    /// Its first line, tabs expanded from the caret's column.
    inline: String,
    /// Its other lines, tabs expanded.
    rows: Vec<String>,
}

/// What remains of the view's suggestion at its caret, unless it's over, or
/// an input method is composing there or vim's mode hides it.
pub(super) fn shown(view: &EditorView) -> Option<Ghost> {
    if !view.accepts_text() || view.composition().is_some() {
        return None;
    }
    let model = &view.model;
    let remaining = view.ghost_text()?.remaining(model)?;
    let offset = model.primary_selection().head();
    let tab_size = model.tab_size();
    let mut lines = remaining
        .split('\n')
        .map(|line| line.trim_end_matches('\r'));
    let first = lines.next().unwrap_or_default();
    let inline = match first.contains('\t') {
        true => {
            let column = caret_column(model.text(), offset, tab_size);
            ExpandedRow::with_start_column(first, column, tab_size)
                .text()
                .to_string()
        }
        false => first.to_string(),
    };
    let rows = lines
        .map(|line| ExpandedRow::new(line, tab_size).text().to_string())
        .collect();
    Some(Ghost {
        offset,
        inline,
        rows,
    })
}

/// The display column, in chars, of `offset` in its row.
fn caret_column(text: &Text, offset: usize, tab_size: usize) -> usize {
    let row_start = text.row_start(text.offset_to_point(offset).row);
    let before = ExpandedRow::new(&text.slice_to_string(row_start..offset), tab_size);
    before.char_column(before.text().len())
}

/// The suggestion's first line, drawn inside the caret's row after it.
pub(super) fn inlay(ghost: &Ghost, view: &EditorView) -> layout::Inlay<Hsla> {
    layout::Inlay {
        offset: ghost.offset,
        text: ghost.inline.clone(),
        style: view.theme.gutter_foreground,
    }
}

/// The block of the suggestion's other lines, a row each, above the display
/// row after the caret's buffer row, or after the last row at the end.
pub(super) fn block(ghost: &Ghost, view: &EditorView) -> Option<Block> {
    let rows = u32::try_from(ghost.rows.len())
        .ok()
        .filter(|&rows| rows > 0)?;
    let (text, display) = (view.model.text(), view.model.display());
    Some(Block {
        above: below_row(ghost.offset, text, display),
        rows,
        id: ID,
    })
}

/// The display row after the last of `offset`'s buffer row.
fn below_row(offset: usize, text: &Text, display: &DisplayMap) -> u32 {
    let row = text.offset_to_point(offset).row;
    let row_end = text.row_start(row) + text.row_len(row) as usize;
    display.row_for_offset(row_end) + 1
}

/// The suggestion's block rows in a frame, shaped, to paint.
#[derive(Default)]
pub(super) struct Placed {
    /// The caret the inlay is at, if there's a suggestion.
    offset: Option<usize>,
    rows: Vec<Shown>,
    /// The inline part, which tests see.
    #[cfg(test)]
    inline: Option<String>,
}

impl Placed {
    /// The caret the suggestion's inlay is at, if there's one.
    pub(super) fn inlay_offset(&self) -> Option<usize> {
        self.offset
    }
}

/// A block row, and where it's painted.
struct Shown {
    at: Vec2,
    line: Line,
    /// As shaped, which tests see.
    #[cfg(test)]
    text: String,
}

/// The rows of the suggestion's block `frame` shows that are in view, and
/// the y each is at.
fn in_view<'a>(
    frame: &'a Frame<Line>,
    ghost: &'a Ghost,
) -> impl Iterator<Item = (f32, &'a str)> + 'a {
    let laid = frame.blocks.iter().find(|laid| laid.id == ID);
    let line_height = frame.line_height;
    laid.into_iter().flat_map(move |laid| {
        (ghost.rows.iter().enumerate())
            .map(move |(ix, text)| (laid.rect.y + line_height * ix as f32, text.as_str()))
            .filter(move |&(y, _)| y + line_height > 0. && y < frame.height)
    })
}

/// How wide the widest of the suggestion's block rows in view is, from
/// column 0: the view scrolls as far as it reaches too, as it does for the
/// host's blocks ([`host_blocks::widest`]).
pub(super) fn widest(
    frame: &Frame<Line>,
    ghost: Option<&Ghost>,
    view: &EditorView,
    shaper: &mut Shaper,
    widths: &mut dyn CharWidth,
) -> f64 {
    let Some(ghost) = ghost else {
        return 0.;
    };
    in_view(frame, ghost)
        .map(|(_, text)| match text.len() > MAX_SHAPED_ROW_BYTES {
            true => host_blocks::clusters(text, widths)
                .last()
                .map_or(0., |(_, _, end)| end),
            false => f64::from(shaper.shape(text, &runs(text, view)).width()),
        })
        .fold(0., f64::max)
}

/// The style run of a row of `text`: the gutter's colour.
fn runs(text: &str, view: &EditorView) -> [StyleRun<Hsla>; 1] {
    [StyleRun {
        len: text.len(),
        style: view.theme.gutter_foreground,
    }]
}

/// Shapes the rows of the suggestion's block `frame` shows that are in view,
/// each from the text area's left edge scrolled to `scroll_x`: a long one
/// around the view ([`host_blocks::slice`]).
pub(super) fn layout(
    frame: &Frame<Line>,
    ghost: Option<&Ghost>,
    view: &EditorView,
    shaper: &mut Shaper,
    widths: &mut dyn CharWidth,
    scroll_x: f32,
) -> Placed {
    let Some(ghost) = ghost else {
        return Placed::default();
    };
    let rows = in_view(frame, ghost)
        .map(|(y, text)| {
            let (range, x) = match text.len() > MAX_SHAPED_ROW_BYTES {
                true => host_blocks::slice(text, frame, scroll_x, widths),
                false => (0..text.len(), 0.),
            };
            let text = &text[range];
            Shown {
                at: Vec2 {
                    x: frame.text_left + (x - f64::from(scroll_x)) as f32,
                    y,
                },
                line: shaper.shape(text, &runs(text, view)),
                #[cfg(test)]
                text: text.to_string(),
            }
        })
        .collect();
    Placed {
        offset: Some(ghost.offset),
        rows,
        #[cfg(test)]
        inline: Some(ghost.inline.clone()),
    }
}

/// What a frame painted of the suggestion, for tests: its inline part, the
/// block rows painted, and the inline part's box in window coordinates,
/// empty if its row isn't in the frame.
#[cfg(test)]
pub(crate) type Painted = (String, Vec<String>, Bounds<Pixels>);

/// Paints the block rows of `placed`, the inline part being in its row,
/// noting in tests what it painted in `painted`.
pub(super) fn paint(
    placed: &Placed,
    layout: &FrameLayout,
    window: &mut Window,
    cx: &mut App,
    #[cfg(test)] painted: &mut Option<Painted>,
) {
    let line_height = px(layout.frame.line_height);
    for shown in &placed.rows {
        let origin = layout.to_window(shown.at);
        shown.line.paint(origin, line_height, window, cx);
    }
    #[cfg(test)]
    {
        *painted = placed.inline.clone().map(|inline| {
            let rows = placed.rows.iter().map(|shown| shown.text.clone()).collect();
            let inlay = layout.frame.inlay;
            let bounds = inlay.map_or_else(Bounds::default, |rect| layout.bounds(rect));
            (inline, rows, bounds)
        });
    }
}
