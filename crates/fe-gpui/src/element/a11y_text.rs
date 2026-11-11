//! The editor's text as a screen reader sees it: one `MultilineTextInput`
//! node whose children are `TextRun`s, one a display row, over the viewport
//! and a viewport's worth of rows above and below it, capped
//! (`docs/decisions/qenw9xxdzk-accessibility-design.md`, section 4).
//!
//! [`build`] is pure: it takes a laid-out [`Frame`], the [`DisplayMap`] it
//! was laid out from and the primary selection, and gives an [`A11yText`].
//! [`A11yText::write`] turns that into AccessKit nodes, which the editor
//! view's text-area wrapper pushes as its synthetic children; tests load
//! them in `accesskit_consumer` instead. Nothing here names the editor, so
//! the file viewer can reuse it, and the a11y-text bench compiles this file
//! on its own.
//!
//! - **Runs.** One a display row, keyed by `(buffer_row, wrap_segment)`, so
//!   soft-wrap segments are lines of their own to a screen reader, as the
//!   caret moves by display row. A buffer line's last segment ends in its
//!   line break, one character (`\n`, or a two-byte `\r\n`). Folded rows
//!   aren't display rows, so they aren't runs; a fold header's run is its
//!   own text, without the fold marker. Inlays (hints, ghost text, a
//!   composition) aren't in the runs.
//! - **Characters** are grapheme clusters, as the caret moves by them; one
//!   longer than a `character_lengths` entry holds (255 bytes) is split at
//!   char boundaries.
//! - **The window.** The visible rows always, truncated at a character
//!   boundary when one alone passes the byte cap; then rows above and below
//!   them, alternately, up to as many again on each side, clipped to the
//!   file, until [`Cap::rows`] or [`Cap::bytes`] is reached. Runs are in
//!   display-row order, which is the order AccessKit reads them in.
//! - **The selection** is projected onto the runs emitted, so each end
//!   names one ([`A11yText::anchor`]): an end hidden by a fold goes to the
//!   end of the header's text, one past a truncated row's cut to the run's
//!   end, one above the window to the first run's start and one below it to
//!   the last run's end.
//! - **Geometry.** Every run has bounds, a position and a width for every
//!   character (its line break too, zero wide at the end of the text) and a
//!   left-to-right direction, so `bounding_boxes` answers for any range. A
//!   visible row is measured as it was shaped; a row above or below the
//!   viewport is measured char by char, as soft wrap measures it, where it
//!   would be drawn.

use std::ops::Range;

use fe_display::{
    Bias, CharWidth, DisplayMap, DisplayRow, LongRows,
    layout::{Frame, MeasuredLine, RowLayout},
};
use fe_text::TextSource;
use gpui::accesskit::{self, Node, NodeId, Role, TextDirection, TextPosition, TextSelection};
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation as _};

/// How much text the node holds at most.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cap {
    /// Display rows.
    pub(crate) rows: usize,
    /// UTF-8 bytes of run text, line breaks included.
    pub(crate) bytes: usize,
}

impl Cap {
    /// The editor's: 300 display rows and 64 KiB.
    pub(crate) const EDITOR: Cap = Cap {
        rows: 300,
        bytes: 64 * 1024,
    };
}

/// A run's identity: its buffer row and which soft-wrap segment of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct RunKey {
    pub(crate) buffer_row: u32,
    pub(crate) segment: u32,
}

/// One `TextRun`: a display row's text, or the start of it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Run {
    pub(crate) key: RunKey,
    pub(crate) display_row: u32,
    /// The buffer offset of its first character.
    pub(crate) start: usize,
    /// Its text, the line break included, which is the buffer's text from
    /// `start`.
    pub(crate) text: String,
    /// Bytes of the text before the line break (all of it without one).
    pub(crate) text_len: usize,
    /// It was cut short at the byte cap.
    pub(crate) truncated: bool,
    /// In UTF-8 bytes, one a character.
    pub(crate) character_lengths: Vec<u8>,
    /// From the left of `bounds`, in the same units, one a character.
    pub(crate) positions: Vec<f32>,
    pub(crate) widths: Vec<f32>,
    /// In window coordinates, scaled to device pixels as GPUI's nodes are.
    pub(crate) bounds: accesskit::Rect,
}

impl Run {
    /// Characters before the line break: the run's end when it has none.
    pub(crate) fn text_characters(&self) -> usize {
        let has_break = self.text.len() > self.text_len;
        self.character_lengths.len() - usize::from(has_break)
    }

    /// The character holding byte `offset` of the text (its start), or the
    /// line break's index for an offset at or past the text's end.
    fn character_at(&self, offset: usize) -> usize {
        if offset >= self.text_len {
            return self.text_characters();
        }
        let mut end = 0;
        (self.character_lengths.iter())
            .take_while(|len| {
                end += usize::from(**len);
                end <= offset
            })
            .count()
    }

    /// The buffer offset of character `index`, clamped to the run's end.
    pub(crate) fn offset_of(&self, index: usize) -> usize {
        let bytes: usize = (self.character_lengths.iter().take(index))
            .map(|len| usize::from(*len))
            .sum();
        self.start + bytes
    }
}

/// The text node's content: its runs in reading order, the primary
/// selection on them, and whether it takes typing.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct A11yText {
    /// Never empty: an empty file has one empty run.
    pub(crate) runs: Vec<Run>,
    /// The selection's ends, each a run emitted and a character index in
    /// it.
    pub(crate) anchor: (RunKey, usize),
    pub(crate) focus: (RunKey, usize),
    /// Keys don't type (vim's normal, visual and operator-pending modes, or
    /// a read-only viewer).
    pub(crate) read_only: bool,
}

impl A11yText {
    pub(crate) fn run(&self, key: RunKey) -> Option<&Run> {
        self.runs.iter().find(|run| run.key == key)
    }

    /// The buffer offset of character `index` of run `key`.
    pub(crate) fn offset(&self, key: RunKey, index: usize) -> Option<usize> {
        Some(self.run(key)?.offset_of(index))
    }

    /// Sets the selection and read-only state on `parent`, the text node,
    /// and gives the run nodes in reading order, each with its id from
    /// `id`.
    pub(crate) fn write(
        &self,
        parent: &mut Node,
        mut id: impl FnMut(RunKey) -> NodeId,
    ) -> Vec<(NodeId, Node)> {
        let mut nodes = Vec::with_capacity(self.runs.len());
        let (mut anchor, mut focus) = (None, None);
        for run in &self.runs {
            let node_id = id(run.key);
            if run.key == self.anchor.0 {
                anchor = Some(node_id);
            }
            if run.key == self.focus.0 {
                focus = Some(node_id);
            }
            let mut node = Node::new(Role::TextRun);
            node.set_value(run.text.as_str());
            node.set_character_lengths(run.character_lengths.as_slice());
            node.set_character_positions(run.positions.as_slice());
            node.set_character_widths(run.widths.as_slice());
            node.set_text_direction(TextDirection::LeftToRight);
            node.set_bounds(run.bounds);
            nodes.push((node_id, node));
        }
        if let (Some(anchor), Some(focus)) = (anchor, focus) {
            parent.set_text_selection(TextSelection {
                anchor: TextPosition {
                    node: anchor,
                    character_index: self.anchor.1,
                },
                focus: TextPosition {
                    node: focus,
                    character_index: self.focus.1,
                },
            });
        }
        if self.read_only {
            parent.set_read_only();
        }
        nodes
    }
}

/// What [`build`] works from.
pub(crate) struct Input<'a, L> {
    pub(crate) display: &'a DisplayMap,
    /// The frame laid out from `display`, whose rows are the visible ones.
    pub(crate) frame: &'a Frame<L>,
    /// What the frame was laid out with.
    pub(crate) long_rows: &'a mut LongRows,
    pub(crate) widths: &'a mut dyn CharWidth,
    pub(crate) tab_size: usize,
    /// The primary selection's anchor and head, buffer offsets.
    pub(crate) anchor: usize,
    pub(crate) head: usize,
    /// The window position of the frame's top-left corner.
    pub(crate) origin: (f32, f32),
    /// Device pixels a pixel.
    pub(crate) scale: f32,
    pub(crate) cap: Cap,
    pub(crate) read_only: bool,
}

/// The text node's content for the frame in `input`.
pub(crate) fn build<L: MeasuredLine>(input: Input<'_, L>) -> A11yText {
    let Input {
        display,
        frame,
        long_rows,
        widths,
        tab_size,
        anchor,
        head,
        origin,
        scale,
        cap,
        read_only,
    } = input;
    let geometry = Geometry {
        frame,
        origin,
        scale,
        tab_size,
    };
    let mut scratch = Scratch::new();
    let last_row = display.row_count().saturating_sub(1);
    // The visible rows; with none (no height yet, or only blocks), the
    // place of the frame's first row.
    let (first, last) = match (frame.rows.first(), frame.rows.last()) {
        (Some(first), Some(last)) => (first.row, last.row),
        _ => {
            let row = frame.first_row.min(last_row);
            (row, row)
        }
    };
    let margin = last - first + 1;
    let above_limit = first.saturating_sub(margin);
    let below_limit = last.saturating_add(margin).min(last_row);
    let source = WindowText::new(display, above_limit..below_limit + 1, cap.bytes);
    let mut budget = cap.bytes;
    let mut rows_left = cap.rows;
    let mut visible = Vec::with_capacity(frame.rows.len().max(1));
    for row in &frame.rows {
        let line = RowText {
            buffer_row: row.buffer_row,
            range: row.start..row.end,
            last: row.last,
        };
        let run = make_run(
            &geometry,
            display,
            &source,
            row.row,
            line,
            Fit::Cut(budget),
            &mut scratch,
            |_, scratch| geometry.shaped(row, scratch, long_rows, widths),
        )
        .expect("a visible row is cut to fit");
        budget -= run.text.len();
        rows_left = rows_left.saturating_sub(1);
        visible.push(run);
    }
    if visible.is_empty() {
        let run = make_run(
            &geometry,
            display,
            &source,
            first,
            RowText::of(&display.row(first)),
            Fit::Cut(budget),
            &mut scratch,
            |text, scratch| geometry.measured(text, scratch, widths),
        )
        .expect("a visible row is cut to fit");
        budget -= run.text.len();
        rows_left = rows_left.saturating_sub(1);
        visible.push(run);
    }
    let (mut above, mut below) = (Vec::new(), Vec::new());
    let (mut up, mut down) = (first, last);
    let mut from_below = true;
    while rows_left > 0 && (up > above_limit || down < below_limit) {
        let row = match from_below {
            true if down < below_limit => down + 1,
            false if up > above_limit => up - 1,
            _ => {
                from_below = !from_below;
                continue;
            }
        };
        let run = make_run(
            &geometry,
            display,
            &source,
            row,
            RowText::of(&display.row(row)),
            Fit::Whole(budget),
            &mut scratch,
            |text, scratch| geometry.measured(text, scratch, widths),
        );
        let Some(run) = run else {
            break;
        };
        budget -= run.text.len();
        rows_left -= 1;
        if from_below {
            below.push(run);
            down = row;
        } else {
            above.push(run);
            up = row;
        }
        from_below = !from_below;
    }
    above.reverse();
    let mut runs = above;
    runs.append(&mut visible);
    runs.append(&mut below);
    let anchor = project(display, &runs, anchor);
    let focus = project(display, &runs, head);
    A11yText {
        runs,
        anchor,
        focus,
        read_only,
    }
}

/// Buffers reused from run to run.
struct Scratch {
    /// The byte offsets of a run's characters' starts in its text, and its
    /// end.
    starts: Vec<usize>,
    /// The x from column 0 of each of `starts`.
    xs: Vec<f64>,
    /// `starts` as buffer offsets.
    offsets: Vec<usize>,
    /// ASCII chars' widths, measured once a build (`NaN` until then).
    ascii: [f32; 128],
}

impl Scratch {
    fn new() -> Self {
        Self {
            starts: Vec::new(),
            xs: Vec::new(),
            offsets: Vec::new(),
            ascii: [f32::NAN; 128],
        }
    }
}

/// How a run must fit the bytes left.
#[derive(Clone, Copy)]
enum Fit {
    /// Cut to them if it's longer: a visible row.
    Cut(usize),
    /// Whole, or not at all: a margin row.
    Whole(usize),
}

/// What a display row shows of its buffer row.
struct RowText {
    buffer_row: u32,
    range: Range<usize>,
    /// Ends the buffer row, rather than at a soft wrap.
    last: bool,
}

impl RowText {
    fn of(row: &DisplayRow) -> Self {
        Self {
            buffer_row: row.buffer_row,
            range: row.range.clone(),
            last: row.last,
        }
    }
}

/// The text of the rows a window may take, read at once rather than row
/// by row, when it's no more than a few times the byte cap.
struct WindowText<'a> {
    source: &'a dyn TextSource,
    /// The buffer offset `text` starts at; empty when it wasn't read.
    start: usize,
    text: String,
}

impl<'a> WindowText<'a> {
    /// Of display rows `rows` of `display`.
    fn new(display: &'a DisplayMap, rows: Range<u32>, cap: usize) -> Self {
        let source = display.text();
        let start = display.row(rows.start).range.start;
        let end = match rows.end < display.row_count() {
            true => display.row(rows.end).range.start,
            false => source.len(),
        };
        let mut text = String::new();
        if end.saturating_sub(start) <= cap.saturating_mul(4) {
            text = source.slice_to_string(start..end);
            // Read as other than its bytes (never expected): row by row.
            if text.len() != end - start {
                text.clear();
            }
        }
        Self {
            source,
            start,
            text,
        }
    }

    fn slice(&self, range: Range<usize>) -> String {
        let held = (range.start.checked_sub(self.start))
            .and_then(|start| self.text.get(start..start + range.len()));
        match held {
            Some(text) => text.to_string(),
            None => {
                let mut text = String::with_capacity(range.len() + 2);
                for chunk in self.source.chunks_in_range(range) {
                    text.push_str(&chunk);
                }
                text
            }
        }
    }
}

/// The run of display row `row` (`shown`), its text cut to `budget` bytes
/// if one is given. `measure` fills in `scratch.xs` for `scratch.starts`,
/// given the text before the line break.
#[allow(clippy::too_many_arguments)]
fn make_run<L: MeasuredLine>(
    geometry: &Geometry<'_, L>,
    display: &DisplayMap,
    window: &WindowText<'_>,
    row: u32,
    shown: RowText,
    fit: Fit,
    scratch: &mut Scratch,
    measure: impl FnOnce(&str, &mut Scratch),
) -> Option<Run> {
    let source = display.text();
    // A line's last segment ends in its break: as one character, two bytes
    // for a CRLF.
    let break_len = match shown.last && shown.buffer_row + 1 < source.row_count() {
        true => source
            .row_start(shown.buffer_row + 1)
            .saturating_sub(shown.range.end),
        false => 0,
    };
    let line_break = match break_len {
        0 => "",
        2 => "\r\n",
        _ => "\n",
    };
    // Nothing past what can be emitted is read: a margin row that doesn't
    // fit is known not to from its range, and a visible row that doesn't is
    // read only to its cut, and a char or two past it to find a boundary.
    let row_len = shown.range.len();
    let fits = |budget: usize| row_len + line_break.len() <= budget;
    let cut = match fit {
        Fit::Whole(budget) if !fits(budget) => return None,
        Fit::Cut(budget) if !fits(budget) => Some(budget.min(row_len)),
        _ => None,
    };
    let read = match cut {
        Some(cut) => {
            let end = (shown.range.start + cut + 8).min(shown.range.end);
            shown.range.start..source.clip_offset(end, Bias::Right)
        }
        None => shown.range.clone(),
    };
    let mut text = window.slice(read);
    let truncated = cut.is_some();
    if let Some(mut cut) = cut {
        cut = cut.min(text.len());
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        // At a grapheme boundary of the whole row, so a cluster across the
        // cut isn't split.
        let mut cursor = GraphemeCursor::new(cut, row_len, true);
        let boundary = match cursor.is_boundary(&text, 0) {
            Ok(true) => cut,
            _ => cursor.prev_boundary(&text, 0).ok().flatten().unwrap_or(0),
        };
        text.truncate(boundary);
    }
    let text_len = text.len();
    let mut character_lengths = character_lengths(&text);
    scratch.starts.clear();
    let mut at = 0;
    for len in &character_lengths {
        scratch.starts.push(at);
        at += usize::from(*len);
    }
    scratch.starts.push(at);
    scratch.xs.clear();
    measure(&text, scratch);
    debug_assert_eq!(scratch.xs.len(), scratch.starts.len());
    let count = character_lengths.len() + 1;
    let (mut positions, mut widths) = (Vec::with_capacity(count), Vec::with_capacity(count));
    let scale = f64::from(geometry.scale);
    let mut right = 0f64;
    for pair in scratch.xs.windows(2) {
        positions.push((pair[0] * scale) as f32);
        widths.push(((pair[1] - pair[0]).max(0.) * scale) as f32);
        right = right.max(pair[1]);
    }
    let end = scratch.xs.last().copied().unwrap_or(0.);
    if !truncated && !line_break.is_empty() {
        text.push_str(line_break);
        character_lengths.push(line_break.len() as u8);
        positions.push((end * scale) as f32);
        widths.push(0.);
    }
    assert_eq!(positions.len(), character_lengths.len());
    assert_eq!(widths.len(), character_lengths.len());
    let first = display.row_for_buffer_row(shown.buffer_row);
    Some(Run {
        key: RunKey {
            buffer_row: shown.buffer_row,
            segment: row.saturating_sub(first),
        },
        display_row: row,
        start: shown.range.start,
        text,
        text_len,
        truncated,
        character_lengths,
        positions,
        widths,
        bounds: geometry.bounds(row, right as f32),
    })
}

/// `text`'s characters' lengths in UTF-8 bytes: one a grapheme cluster, a
/// cluster longer than 255 bytes split at char boundaries.
fn character_lengths(text: &str) -> Vec<u8> {
    // Rows have no line breaks, so ASCII clusters are single bytes.
    if text.is_ascii() {
        return vec![1; text.len()];
    }
    let mut lengths = Vec::with_capacity(text.len());
    for grapheme in text.graphemes(true) {
        if let Ok(len) = u8::try_from(grapheme.len()) {
            lengths.push(len);
            continue;
        }
        let mut len = 0usize;
        for ch in grapheme.chars() {
            if len + ch.len_utf8() > usize::from(u8::MAX) {
                lengths.push(len as u8);
                len = 0;
            }
            len += ch.len_utf8();
        }
        if len > 0 {
            lengths.push(len as u8);
        }
    }
    lengths
}

/// Where runs go, in window coordinates scaled to device pixels.
struct Geometry<'a, L> {
    frame: &'a Frame<L>,
    origin: (f32, f32),
    scale: f32,
    tab_size: usize,
}

impl<L: MeasuredLine> Geometry<'_, L> {
    /// The bounds of display row `row`, from column 0, `width` wide (both
    /// unscaled).
    fn bounds(&self, row: u32, width: f32) -> accesskit::Rect {
        let frame = self.frame;
        let x0 = self.origin.0 + frame.view_x(0.);
        let y0 = self.origin.1 + frame.row_top(row);
        let scale = f64::from(self.scale);
        accesskit::Rect {
            x0: f64::from(x0) * scale,
            y0: f64::from(y0) * scale,
            x1: f64::from(x0 + width.max(0.)) * scale,
            y1: f64::from(y0 + frame.line_height) * scale,
        }
    }

    /// The x from column 0 of each of `scratch.starts` (byte offsets in
    /// the row's text, from `row.start`), as the frame shaped the row.
    fn shaped(
        &self,
        row: &RowLayout<L>,
        scratch: &mut Scratch,
        long_rows: &mut LongRows,
        widths: &mut dyn CharWidth,
    ) {
        scratch.offsets.clear();
        (scratch.offsets).extend(scratch.starts.iter().map(|start| row.start + start));
        row.xs_for_offsets(&scratch.offsets, long_rows, widths, &mut scratch.xs);
    }

    /// The x from column 0 of each of `scratch.starts` (byte offsets in
    /// `text`, a display row's), measured char by char as soft wrap
    /// measures.
    fn measured(&self, text: &str, scratch: &mut Scratch, widths: &mut dyn CharWidth) {
        let Scratch {
            starts, xs, ascii, ..
        } = scratch;
        fn char_width(ascii: &mut [f32; 128], widths: &mut dyn CharWidth, ch: char) -> f32 {
            match ascii.get_mut(ch as usize) {
                Some(width) => {
                    if width.is_nan() {
                        *width = widths.char_width(ch);
                    }
                    *width
                }
                None => widths.char_width(ch),
            }
        }
        let space = char_width(ascii, widths, ' ');
        let tab_size = self.tab_size.max(1);
        let (mut x, mut column) = (0f64, 0usize);
        for pair in starts.windows(2) {
            xs.push(x);
            let character = &text[pair[0]..pair[1]];
            let mut chars = character.chars();
            let first = chars.next().unwrap_or(' ');
            let (width, columns) = match (first, chars.next()) {
                ('\t', None) => {
                    let columns = tab_size - column % tab_size;
                    (space * columns as f32, columns)
                }
                (ch, None) => (char_width(ascii, widths, ch), 1),
                _ => (widths.cluster_width(character), character.chars().count()),
            };
            x += f64::from(width);
            column += columns;
        }
        xs.push(x);
    }
}

/// `offset`, a selection's end, on the runs emitted: hidden by a fold, at
/// the end of the header's text; past a truncated row's cut, at its run's
/// end; above or below the runs, at the first's start or the last's end;
/// otherwise its character in its row's run.
fn project(display: &DisplayMap, runs: &[Run], offset: usize) -> (RunKey, usize) {
    let (first, last) = (&runs[0], &runs[runs.len() - 1]);
    let offset = offset.min(display.text().len());
    let row = display.row_for_offset(offset);
    if row < first.display_row {
        return (first.key, 0);
    }
    if row > last.display_row {
        return (last.key, last.character_lengths.len());
    }
    let run = match runs.binary_search_by_key(&row, |run| run.display_row) {
        Ok(ix) => &runs[ix],
        // Runs are consecutive rows, so it's there; the nearer edge if not.
        Err(ix) => {
            let run = &runs[ix.min(runs.len() - 1)];
            return (run.key, 0);
        }
    };
    (run.key, run.character_at(offset.saturating_sub(run.start)))
}
