//! Diagnostic messages after their rows (`editor.inline_diagnostics`), as
//! VS Code's Error Lens extension shows them: per buffer row, the first line
//! of the most severe message at or above the setting's level, dimmed, one em
//! after the row's last glyph.
//!
//! - **Overlay.** It's laid out and painted over a finished frame, not as a
//!   display-map inlay: the text, wrapping, columns and carets don't know it,
//!   and it has no hitbox, so the pointer reaches the text below. The minimap
//!   doesn't draw it either.
//! - **Which message.** The most severe diagnostic whose range touches the
//!   row, by the rows the gutter's dot takes it to cover; of equals, the one
//!   starting earliest. A message whose first line is blank has nothing to
//!   show, and gives way.
//! - **Where.** A soft-wrapped row shows it on its last display row, after
//!   anything else drawn after the line. It's cut with an ellipsis at the
//!   frame's right edge, which leaves out a minimap; with under
//!   [`MIN_ROOM_CHARS`] chars of room it isn't drawn. Nor is it on a row whose
//!   end is scrolled out of view, a folded row or a long row's slice.
//! - **Cost.** One sweep down the frame's rows, with the diagnostics near them
//!   in order of start and those under way in a heap, and one shape per row, or
//!   fewer where rows show the same text in the same colour. So a cut takes no
//!   second shape, a message is shaped with an ellipsis on its end, and where
//!   it's cut comes from that shape, so kerning and ligatures count; what's
//!   painted is that shape's glyphs, those kept and the ellipsis moved up to
//!   them, not clipped, so a glyph's ink past its advance still shows.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    ops::Range,
};

use fe_buffer::Text;
use fe_display::{
    CharWidth as _,
    layout::{Frame, MAX_SHAPED_ROW_BYTES, MeasuredLine, StyleRun, TextMeasure as _},
};
use fe_editor::{
    decorations::DecorationKind,
    diagnostics::{DiagnosticEntry, Severity},
};
#[cfg(test)]
use gpui::Bounds;
use gpui::{App, Hsla, Pixels, Point, Window, point, px};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::{
    EditorTheme, InlineDiagnostics,
    element::{FrameLayout, Line, Shaper},
};

/// The opacity of a message, of its severity's colour.
const ALPHA: f32 = 0.6;

/// The fewest chars' room, each a space wide, a message is drawn in.
const MIN_ROOM_CHARS: f32 = 8.;

/// What ends a message that's cut.
const ELLIPSIS: char = '\u{2026}';

/// How much of a shaped message shows.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Kept {
    /// The bytes of its text kept.
    len: usize,
    /// The x its kept text ends at.
    end: f32,
    /// The x of the ellipsis in the line and its width, if it was cut.
    ellipsis: Option<(f32, f32)>,
}

impl Kept {
    /// How wide it's drawn.
    fn width(&self) -> f32 {
        self.end + self.ellipsis.map_or(0., |(_, width)| width)
    }
}

/// A message a frame lays out.
pub(crate) struct Shown {
    origin: Point<Pixels>,
    /// The glyphs kept of the message, then any ellipsis.
    line: Line,
    #[cfg(test)]
    painted: PaintedMessage,
}

/// The messages a frame shows.
#[derive(Default)]
pub(crate) struct Layout {
    pub(crate) shown: Vec<Shown>,
    /// How many lines were shaped for them.
    #[cfg(test)]
    shapes: usize,
    /// How many steps it took to place them.
    #[cfg(test)]
    steps: usize,
}

/// A message a frame painted, in window coordinates.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PaintedMessage {
    pub(crate) buffer_row: u32,
    pub(crate) text: String,
    pub(crate) color: Hsla,
    pub(crate) bounds: Bounds<Pixels>,
    /// The byte index and x, from its start, of each glyph it painted.
    pub(crate) glyphs: Vec<(usize, Pixels)>,
}

/// The messages a frame painted, how many lines it shaped for them and how
/// many steps it took to place them.
#[cfg(test)]
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Painted {
    pub(crate) messages: Vec<PaintedMessage>,
    pub(crate) shapes: usize,
    pub(crate) steps: usize,
}

#[cfg(test)]
impl Layout {
    pub(crate) fn painted(&self) -> Painted {
        Painted {
            messages: self
                .shown
                .iter()
                .map(|shown| shown.painted.clone())
                .collect(),
            shapes: self.shapes,
            steps: self.steps,
        }
    }
}

/// The least severe a diagnostic may be to show, if any may.
fn lowest(level: InlineDiagnostics) -> Option<Severity> {
    match level {
        InlineDiagnostics::Off => None,
        InlineDiagnostics::Error => Some(Severity::Error),
        InlineDiagnostics::Warning => Some(Severity::Warning),
        InlineDiagnostics::All => Some(Severity::Hint),
    }
}

fn is_line_break(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// Whether a message's first line has anything to show.
fn first_line_shows(message: &str) -> bool {
    message
        .chars()
        .find(|&ch| is_line_break(ch) || !(ch.is_whitespace() || ch.is_control()))
        .is_some_and(|ch| !is_line_break(ch))
}

/// A message's first line, without the space around it and with what wouldn't
/// shape as text put as spaces, up to `max_clusters` graphemes, and whether
/// that's all of it. Looks no further along it than that.
fn first_line(message: &str, max_clusters: usize) -> (String, bool) {
    let mut line = String::new();
    let mut whole = true;
    for (count, cluster) in message.graphemes(true).enumerate() {
        if cluster.contains(is_line_break) {
            break;
        }
        if count >= max_clusters || line.len() + cluster.len() > MAX_SHAPED_ROW_BYTES {
            whole = false;
            break;
        }
        match cluster.contains(|ch: char| ch.is_whitespace() || ch.is_control()) {
            true if line.is_empty() => {}
            true => line.push(' '),
            false => line.push_str(cluster),
        }
    }
    line.truncate(line.trim_end().len());
    (line, whole)
}

/// Whether `range` is on a buffer row, as the gutter's dot takes it to be:
/// through the row its end is on, unless that end is the row's start and the
/// range isn't empty.
fn touches(range: &Range<usize>, start: usize, end: usize) -> bool {
    range.start <= end && (range.end > start || (range.end == start && range.is_empty()))
}

/// The frame's rows that show a message, in order, by their index there, each
/// with the diagnostic to show. One sweep down the rows: the diagnostics near
/// the frame join a heap as the rows reach their start, the most severe and
/// then the earliest on top, and leave it for good when found to have ended.
fn chosen<'a, 'b>(
    frame: &Frame<Line>,
    text: &Text,
    diagnostics: &'a [DiagnosticEntry<'b>],
    lowest: Severity,
    #[cfg(test)] steps: &mut usize,
) -> Vec<(usize, &'a DiagnosticEntry<'b>)> {
    let (Some(first), Some(last)) = (frame.rows.first(), frame.rows.last()) else {
        return Vec::new();
    };
    let shown = text.row_start(first.buffer_row)..last.end;
    let mut near: Vec<usize> = (0..diagnostics.len())
        .filter(|&ix| {
            let entry = &diagnostics[ix];
            entry.severity >= lowest
                && entry.range.start <= shown.end
                && entry.range.end >= shown.start
                && first_line_shows(&entry.message)
        })
        .collect();
    near.sort_by_key(|&ix| diagnostics[ix].range.start);
    #[cfg(test)]
    {
        *steps += diagnostics.len();
    }
    let mut near = near.into_iter().peekable();
    let mut under_way = BinaryHeap::new();
    // In the frame's rows' order.
    let mut folded = (frame.fold_markers.iter())
        .map(|marker| marker.buffer_row)
        .peekable();
    let mut rows = Vec::new();
    for (row_ix, row) in frame.rows.iter().enumerate() {
        #[cfg(test)]
        {
            *steps += 1;
        }
        while folded.next_if(|&marker| marker < row.buffer_row).is_some() {
            #[cfg(test)]
            {
                *steps += 1;
            }
        }
        if !row.last || row.slice.is_some() || folded.peek() == Some(&row.buffer_row) {
            continue;
        }
        let start = text.row_start(row.buffer_row);
        while let Some(ix) = near.next_if(|&ix| diagnostics[ix].range.start <= row.end) {
            let entry = &diagnostics[ix];
            under_way.push((entry.severity, Reverse(entry.range.start), Reverse(ix)));
            #[cfg(test)]
            {
                *steps += 1;
            }
        }
        while let Some(&(_, _, Reverse(ix))) = under_way.peek() {
            if touches(&diagnostics[ix].range, start, row.end) {
                rows.push((row_ix, &diagnostics[ix]));
                break;
            }
            under_way.pop();
            #[cfg(test)]
            {
                *steps += 1;
            }
        }
    }
    rows
}

/// How much of `line`, `text` then an ellipsis, to show in `room`: all of
/// `text` if that's `whole` and fits, or else up to a grapheme, with the
/// ellipsis fitting after it.
fn keep(line: &impl MeasuredLine, text: &str, whole: bool, room: f32) -> Kept {
    let len = text.len();
    let message_end = line.x_for_index(len);
    if whole && message_end <= room {
        return Kept {
            len,
            end: message_end,
            ellipsis: None,
        };
    }
    let ellipsis_width = line.width() - message_end;
    let target = room - ellipsis_width;
    let near = line.index_for_x(target).min(len);
    // The last grapheme boundary at or before the x, of those near it.
    let mut boundaries: Vec<usize> = (text.grapheme_indices(true))
        .map(|(at, _)| at)
        .chain([len])
        .take_while(|&at| at <= near)
        .collect();
    while boundaries.len() > 1 && line.x_for_index(boundaries[boundaries.len() - 1]) > target {
        boundaries.pop();
    }
    let cut = boundaries.last().copied().unwrap_or(0);
    let cut = text[..cut].trim_end().len();
    Kept {
        len: cut,
        end: line.x_for_index(cut),
        ellipsis: Some((message_end, ellipsis_width)),
    }
}

/// The messages to show over a finished frame.
pub(crate) fn layout(
    frame_layout: &mut FrameLayout,
    text: &Text,
    diagnostics: &[DiagnosticEntry<'_>],
    theme: &EditorTheme,
    window: &Window,
) -> Layout {
    let mut found = Layout::default();
    let Some(lowest) = lowest(theme.inline_diagnostics) else {
        return found;
    };
    let rows = chosen(
        &frame_layout.frame,
        text,
        diagnostics,
        lowest,
        #[cfg(test)]
        &mut found.steps,
    );
    if rows.is_empty() {
        return found;
    }
    // The screen row of a y in the frame: robust where a y that went to the
    // window and back has picked up rounding.
    let (scroll_y, line_height) = (frame_layout.frame.scroll.y, frame_layout.frame.line_height);
    let screen_row = |y: f32| ((y + scroll_y) / line_height).round() as i64;
    // Where decorations' text after the frame's lines ends, by screen row.
    let mut after: HashMap<i64, f32> = HashMap::new();
    for placed in &frame_layout.decorations.placed {
        if matches!(placed.kind, DecorationKind::AfterLine(_)) {
            let at = frame_layout.to_frame(placed.bounds.top_right());
            let right = after.entry(screen_row(at.y)).or_insert(at.x);
            *right = right.max(at.x);
            #[cfg(test)]
            {
                found.steps += 1;
            }
        }
    }
    let origin = frame_layout.origin;
    let FrameLayout {
        frame,
        long_rows,
        glyph_widths,
        ..
    } = frame_layout;
    let space = glyph_widths.char_width(' ');
    let visible = frame.text_bounds();
    let right = visible.x + visible.width;
    // Room to the frame's edge for chars down to a quarter of a space wide;
    // one room's worth, so rows with the same message share a shape.
    let max_clusters = (visible.width / space.max(1.) * 4.).ceil() as usize + 1;
    let mut shaper = Shaper::editor_font(theme, window);
    let mut shaped: HashMap<(String, Hsla), Line> = HashMap::new();
    for (row_ix, entry) in rows {
        let row = &frame.rows[row_ix];
        let end = frame.view_x(row.x_for_offset(row.end, long_rows, glyph_widths));
        if end < visible.x || end > right {
            continue;
        }
        let top = frame.row_top(row.row);
        // After a hint at the row's end, and text a decoration put there.
        let last = (after.get(&screen_row(top)).copied())
            .unwrap_or(end)
            .max(end)
            .max(frame.line_origin(row).x + row.line.width());
        let x = last + space;
        let room = right - x;
        if room < space * MIN_ROOM_CHARS {
            continue;
        }
        let (message, whole) = first_line(&entry.message, max_clusters);
        if message.is_empty() {
            continue;
        }
        let color = theme.diagnostic_color(entry.severity).opacity(ALPHA);
        let shape = (shaped.entry((message.clone(), color)))
            .or_insert_with(|| {
                #[cfg(test)]
                {
                    found.shapes += 1;
                }
                let with_ellipsis = format!("{message}{ELLIPSIS}");
                let runs = [StyleRun {
                    len: with_ellipsis.len(),
                    style: color,
                }];
                shaper.shape(&with_ellipsis, &runs)
            })
            .clone();
        let kept = keep(&shape, &message, whole, room);
        let line = shape.with_glyphs_moved(kept.width(), |index| {
            if index < kept.len {
                Some(0.)
            } else if index < message.len() {
                None
            } else {
                kept.ellipsis.map(|(at, _)| kept.end - at)
            }
        });
        let at = origin + point(px(x), px(top));
        found.shown.push(Shown {
            origin: at,
            #[cfg(test)]
            painted: PaintedMessage {
                buffer_row: row.buffer_row,
                text: match kept.ellipsis {
                    Some(_) => format!("{}{ELLIPSIS}", &message[..kept.len]),
                    None => message,
                },
                color,
                bounds: Bounds::new(at, gpui::size(px(kept.width()), px(frame.line_height))),
                glyphs: line.glyphs(),
            },
            line,
        });
    }
    found
}

/// Paints the messages.
pub(crate) fn paint(shown: &[Shown], line_height: Pixels, window: &mut Window, cx: &mut App) {
    for message in shown {
        message.line.paint(message.origin, line_height, window, cx);
    }
}

#[cfg(test)]
#[path = "inline_diagnostics_tests.rs"]
mod inline_diagnostics_tests;
