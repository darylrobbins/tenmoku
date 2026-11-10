//! The view's proposals ([`crate::view`]'s `proposal`) in a frame: each
//! pending or stale hunk as a block of rows placed by fe-display's block
//! layer under the text it replaces ([`blocks`]), a header row then its new
//! text a line a row on an "added" band, and its old rows under a full-width
//! "removed" band. A hunk whose old rows a fold hides shows neither.
//!
//! The header row says what the hunk is, then has its buttons: Accept and
//! Reject for a pending hunk, Reject alone for a stale one.
//!
//! The tints are the theme's diff tokens, as a diff view's ([`Tints`]): the
//! block on `diff_added_row`, the old rows on `diff_removed_row`, and each
//! button on the word tint of what it does to the text, `diff_added_word`
//! for Accept and `diff_removed_word` for Reject.
//! A frame reads only the new text's rows in view, through [`Texts`].

use std::{collections::HashMap, ops::Range, sync::Arc};

use fe_buffer::{HunkId, HunkStatus, Outcome, Text};
use fe_display::{
    Block, BlockMap, CharWidth, DisplayMap, ExpandedRow, RowIndex,
    layout::{Frame, MAX_SHAPED_ROW_BYTES, MeasuredLine as _, Rect, StyleRun, TextMeasure, Vec2},
};
use gpui::{
    App, Bounds, ContentMask, CursorStyle, FontId, Hitbox, HitboxBehavior, Hsla, Pixels,
    SharedString, Window, fill, px,
};
use kiln_l10n::tr;

use super::{FrameLayout, GlyphWidths, Line, Shaper};
use crate::{EditorTheme, EditorView, view::ProposalButton};

/// Block ids from here on are the proposals' hunks, [`IDS`] plus the
/// hunk's index in the frame's [`Hunk`]s; below are the host's.
const IDS: u64 = 1 << 32;

/// How opaque a stale hunk's header is over the text colour.
const DIM_ALPHA: f32 = 0.6;
/// Either side of a button's label, in line heights.
const BUTTON_PAD: f32 = 0.4;
/// Before each button, in line heights.
const BUTTON_GAP: f32 = 0.5;
/// Above and below a button in its row, in line heights.
const BUTTON_INSET: f32 = 0.1;
/// Long rows' indexes a new text keeps before it drops those the last two
/// frames didn't use.
const MAX_LONG_ROWS: usize = 128;

/// A pending or stale hunk of a proposal the view shows, as of its
/// snapshot.
pub(super) struct Hunk {
    id: HunkId,
    status: HunkStatus,
    /// Where its old text is.
    range: Range<usize>,
    /// What its header row says.
    header: String,
    /// Its new text, a line a row, as [`Texts`] indexes it.
    new_text: Arc<str>,
    /// The header's and the new text's.
    rows: u32,
}

/// The new texts of the hunks the frames show, kept from frame to frame,
/// each held once however many hunks share it: its rows, so a frame reads
/// only those in view, and the indexes of its rows too long to shape whole,
/// measured with the font and tab size in `measured`.
#[derive(Default)]
pub(crate) struct Texts {
    /// By the address of each new text, which its entry holds on to.
    texts: HashMap<usize, Indexed>,
    /// What the long rows' indexes are measured with: the font, its size
    /// and the tab size.
    measured: Option<(FontId, Pixels, usize)>,
    frame: u64,
    /// Bytes of new text read since the frame began, which tests see.
    #[cfg(test)]
    pub(crate) read: usize,
    /// New texts indexed since the view was made, which tests see.
    #[cfg(test)]
    pub(crate) indexed: usize,
}

/// A hunk's new text, indexed.
struct Indexed {
    /// Keeps the address it's found by from being reused.
    _new_text: Arc<str>,
    text: Text,
    /// How many lines it has ([`lines`]).
    lines: u32,
    /// The last frame that showed it.
    used: u64,
    /// The indexes of its long rows, by row, with the frame that last used
    /// each.
    long: HashMap<u32, (RowIndex, u64)>,
}

/// Where [`Texts`] finds `new_text`.
fn key(new_text: &Arc<str>) -> usize {
    Arc::as_ptr(new_text).cast::<u8>() as usize
}

/// How many lines `text` has: none if it's empty, and none after a final
/// line break.
fn lines(text: &Text, ends_in_break: bool) -> u32 {
    match text.len() {
        0 => 0,
        _ => text.row_count() - u32::from(ends_in_break),
    }
}

impl Texts {
    /// Starts a frame, which the texts [`Texts::lines`] finds are used by.
    fn start_frame(&mut self) {
        self.frame += 1;
        #[cfg(test)]
        {
            self.read = 0;
        }
    }

    /// Drops the texts this frame didn't show.
    fn end_hunks(&mut self) {
        let frame = self.frame;
        self.texts.retain(|_, indexed| indexed.used == frame);
    }

    /// How many lines `new_text` has, indexing it the first time it's seen.
    fn lines(&mut self, new_text: &Arc<str>) -> u32 {
        let frame = self.frame;
        let indexed = self.texts.entry(key(new_text)).or_insert_with(|| {
            #[cfg(test)]
            {
                self.indexed += 1;
            }
            // Line breaks are `\n` in a buffer; anything else is shown as
            // one too.
            let (text, _) = Text::from_file_contents(new_text);
            let ends_in_break = new_text.ends_with(['\n', '\r']);
            Indexed {
                _new_text: new_text.clone(),
                lines: lines(&text, ends_in_break),
                text,
                used: frame,
                long: HashMap::new(),
            }
        });
        indexed.used = frame;
        indexed.lines
    }

    /// Drops the long rows' indexes unless they were measured with `widths`
    /// at `tab_size`.
    fn measure_with(&mut self, widths: &GlyphWidths, tab_size: usize) {
        let measured = Some((widths.font_id, widths.font_size, tab_size));
        if self.measured != measured {
            self.measured = measured;
            for indexed in self.texts.values_mut() {
                indexed.long.clear();
            }
        }
    }

    /// What line `line` of `new_text` shows, tabs expanded, and the x it
    /// starts at from column 0: all of it, or, for one too long to shape
    /// whole, the part around `view`, the scroll and the text area's width,
    /// a view's width either side, as much as is shaped at once. Without a
    /// `view`, which only measures, a long one is `Err` with its width.
    fn line(
        &mut self,
        new_text: &Arc<str>,
        line: u32,
        view: Option<(f32, f32)>,
        tab_size: usize,
        widths: &mut dyn CharWidth,
    ) -> Result<(String, f64), f64> {
        let frame = self.frame;
        let Some(indexed) = self.texts.get_mut(&key(new_text)) else {
            debug_assert!(false, "a hunk's new text is indexed first");
            return Ok((String::new(), 0.));
        };
        let text = &indexed.text;
        let start = text.row_start(line);
        let row = start..start + text.row_len(line) as usize;
        if row.len() <= MAX_SHAPED_ROW_BYTES {
            #[cfg(test)]
            {
                self.read += row.len();
            }
            let expanded = ExpandedRow::new(&text.slice_to_string(row.clone()), tab_size);
            if expanded.text().len() <= MAX_SHAPED_ROW_BYTES {
                return Ok((expanded.text().to_string(), 0.));
            }
        }
        if indexed.long.len() >= MAX_LONG_ROWS && !indexed.long.contains_key(&line) {
            indexed.long.retain(|_, (_, used)| *used + 1 >= frame);
        }
        let (index, used) = (indexed.long.entry(line))
            .or_insert_with(|| (RowIndex::new(text, row, tab_size, widths), frame));
        *used = frame;
        let Some((scroll_x, width)) = view else {
            return Err(index.width(widths));
        };
        let (scroll_x, width) = (f64::from(scroll_x), f64::from(width));
        let slice = index.slice(scroll_x..scroll_x + width, width, widths);
        #[cfg(test)]
        {
            self.read += slice.range.len();
        }
        let shown = text.slice_to_string(slice.range);
        let expanded = ExpandedRow::with_start_column(&shown, slice.start_column, tab_size);
        Ok((expanded.text().to_string(), slice.start_x))
    }
}

/// The pending and stale hunks of the view's proposals, in its snapshot,
/// their new texts indexed in `texts`. A stale one's header gives the dry
/// run's reason. A proposal anchored in a later snapshot than the view's
/// isn't shown until the view catches up.
pub(super) fn hunks(view: &EditorView, texts: &mut Texts) -> Vec<Hunk> {
    let snapshot = view.model.snapshot();
    texts.start_frame();
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
                // A pending hunk's header.
                HunkStatus::Pending => tr!("gpui-proposal-label").to_string(),
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
                id: hunk.id,
                status: hunk.status,
                range: hunk.range,
                header,
                rows: 1 + texts.lines(&hunk.new_text),
                new_text: hunk.new_text,
            });
        }
    }
    texts.end_hunks();
    out
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

/// Where to scroll, as a top in screen rows, to show the block of hunk `id`
/// in a view `visible_rows` tall now at `scroll_top`: nowhere if it's in
/// view, else as little as shows it whole, or, for a block taller than the
/// view, to its header at the top. A hunk without a block has none.
pub(super) fn reveal(
    hunks: &[Hunk],
    id: HunkId,
    blocks: &BlockMap,
    scroll_top: f32,
    visible_rows: f32,
) -> Option<f32> {
    let ix = hunks.iter().position(|hunk| hunk.id == id)?;
    let rows = blocks.screen_rows(IDS + ix as u64)?;
    let (start, end) = (rows.start as f32, rows.end as f32);
    if start >= scroll_top && end <= scroll_top + visible_rows {
        return None;
    }
    match start < scroll_top || end - start > visible_rows {
        true => Some(start),
        false => Some(end - visible_rows),
    }
}

/// The hunk a frame's block of `id` shows, if it's a proposal's.
fn hunk(hunks: &[Hunk], id: u64) -> Option<&Hunk> {
    hunks.get(usize::try_from(id.checked_sub(IDS)?).ok()?)
}

/// The hunks' blocks and bands in a frame, to paint.
#[derive(Default)]
pub(crate) struct Layout {
    /// The tints it's painted in, from the theme it was laid out in.
    tints: Tints,
    blocks: Vec<Placed>,
    /// Full-width bands behind the old rows of the hunks shown.
    pub(crate) removed: Vec<Rect>,
    /// Over the part of each button in view, which shows a pointing hand.
    hitboxes: Vec<Hitbox>,
}

/// The colours a frame paints the proposals in: the theme's diff tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Tints {
    /// Behind a hunk's block, its new text: `diff_added_row`.
    block: Hsla,
    /// Behind a hunk's old rows: `diff_removed_row`.
    band: Hsla,
    /// Behind Accept: `diff_added_word`, stronger over the block's tint.
    accept: Hsla,
    /// Behind Reject: `diff_removed_word`.
    reject: Hsla,
}

impl Tints {
    /// `theme`'s.
    fn of(theme: &EditorTheme) -> Self {
        Self {
            block: theme.diff_added_row,
            band: theme.diff_removed_row,
            accept: theme.diff_added_word,
            reject: theme.diff_removed_word,
        }
    }

    /// The colour of a button, as of what it does to the text.
    fn button(&self, button: ProposalButton) -> Hsla {
        match button {
            ProposalButton::Accept => self.accept,
            ProposalButton::Reject => self.reject,
        }
    }
}

impl EditorView {
    /// The tints the last frame laid the proposals out in, if it showed a
    /// block or a band of one, as `[block, band, accept, reject]` (see
    /// [`Tints`]): for tests outside the crate.
    #[doc(hidden)]
    pub fn drawn_proposal_tints(&self) -> Option<[Hsla; 4]> {
        let proposals = &self.layout.as_ref()?.proposals;
        if proposals.blocks.is_empty() && proposals.removed.is_empty() {
            return None;
        }
        let Tints {
            block,
            band,
            accept,
            reject,
        } = proposals.tints;
        Some([block, band, accept, reject])
    }
}

/// A hunk's block laid out in a frame.
struct Placed {
    id: HunkId,
    status: HunkStatus,
    /// Which tests see.
    #[cfg(test)]
    header: String,
    /// In the frame.
    rect: Rect,
    /// The rows in view: the header's, then the new text's.
    rows: Vec<Row>,
    /// The header's buttons, if it's in view.
    buttons: Vec<Button>,
}

/// A block's row, shaped, and where it's painted.
struct Row {
    at: Vec2,
    line: Line,
    /// As shaped, which tests see.
    #[cfg(test)]
    text: String,
}

/// A button in a header row, laid out.
struct Button {
    button: ProposalButton,
    /// Its box in the frame.
    rect: Rect,
    /// The part of it in view, inside its block and the text area, which a
    /// click presses.
    shown: Option<Rect>,
    label: Row,
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

    /// The hunk of each block whose header is in view, and where the
    /// header's text starts and ends, from the frame's left.
    pub(crate) fn headers(&self) -> Vec<(HunkId, f32, f32)> {
        (self.blocks.iter())
            .filter(|block| !block.buttons.is_empty())
            .filter_map(|block| Some((block.id, block.rows.first()?)))
            .map(|(id, row)| (id, row.at.x, row.at.x + row.line.width()))
            .collect()
    }
}

/// The buttons a header row of a hunk with `status` has: Accept and Reject
/// for a pending one, though a read-only view takes no edit (unless it
/// takes Accepts all the same), and Reject alone for a stale one.
fn buttons(status: HunkStatus, read_only: bool) -> &'static [ProposalButton] {
    match status {
        HunkStatus::Pending if !read_only => &[ProposalButton::Accept, ProposalButton::Reject],
        _ => &[ProposalButton::Reject],
    }
}

fn label(button: ProposalButton) -> SharedString {
    match button {
        ProposalButton::Accept => tr!("gpui-proposal-accept"),
        ProposalButton::Reject => tr!("gpui-proposal-reject"),
    }
}

/// The buttons of a hunk's header row with `status`, after its text
/// `text_width` wide, shaped: each at x from the row's start and as wide as
/// its label and padding. Then the row's width.
fn header_buttons(
    status: HunkStatus,
    read_only: bool,
    text_width: f32,
    line_height: f32,
    shaper: &mut Shaper,
    theme: &EditorTheme,
) -> (Vec<(ProposalButton, f32, f32, Line)>, f32) {
    let (pad, gap) = (line_height * BUTTON_PAD, line_height * BUTTON_GAP);
    let mut x = text_width;
    let shaped = (buttons(status, read_only).iter())
        .map(|&button| {
            let text = label(button);
            let line = shaper.shape(&text, &runs(&text, false, status, theme));
            let width = line.width() + pad * 2.;
            let at = x + gap;
            x = at + width;
            (button, at, width, line)
        })
        .collect();
    (shaped, x)
}

/// The rows of the hunks' blocks `frame` shows that are in view: each
/// block's index in `frame.blocks`, its hunk, the row's index in the block
/// (the header's 0, then a row a line of new text) and its y.
fn shown_rows<'a>(
    frame: &'a Frame<Line>,
    hunks: &'a [Hunk],
) -> impl Iterator<Item = (usize, &'a Hunk, u32, f32)> + 'a {
    let line_height = frame.line_height;
    frame.blocks.iter().enumerate().flat_map(move |(ix, laid)| {
        // Those from the first below the frame's top to the last above its
        // bottom, a long block's others left as they are.
        let first = (-laid.rect.y / line_height).floor().max(0.) as u32;
        let end = ((frame.height - laid.rect.y) / line_height).ceil().max(0.) as u32;
        hunk(hunks, laid.id).into_iter().flat_map(move |hunk| {
            (first..end.min(hunk.rows))
                .map(move |row| (ix, hunk, row, laid.rect.y + line_height * row as f32))
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

/// How wide the widest row of the blocks in view is, from column 0, as
/// [`super::host_blocks::widest`] measures the host's: a header with its
/// buttons, or a line of new text, one too long to shape whole as its index
/// measures it.
pub(super) fn widest(
    frame: &Frame<Line>,
    hunks: &[Hunk],
    texts: &mut Texts,
    view: &EditorView,
    shaper: &mut Shaper,
    widths: &mut GlyphWidths,
) -> f64 {
    let (theme, tab_size) = (&view.theme, view.model.tab_size());
    texts.measure_with(widths, tab_size);
    let mut widest = 0f64;
    for (_, hunk, row, _) in shown_rows(frame, hunks) {
        let width = match row {
            0 => {
                let runs = runs(&hunk.header, true, hunk.status, theme);
                let text_width = shaper.shape(&hunk.header, &runs).width();
                let (_, width) = header_buttons(
                    hunk.status,
                    view.read_only && !view.accepts_when_read_only,
                    text_width,
                    frame.line_height,
                    shaper,
                    theme,
                );
                f64::from(width)
            }
            _ => match texts.line(&hunk.new_text, row - 1, None, tab_size, widths) {
                Ok((text, _)) => {
                    let runs = runs(&text, false, hunk.status, theme);
                    f64::from(shaper.shape(&text, &runs).width())
                }
                Err(width) => width,
            },
        };
        widest = widest.max(width);
    }
    widest
}

/// Lays out the hunks' blocks `frame` shows, their rows in view scrolled
/// to `scroll_x` as the host's blocks' lines are, and the bands behind the
/// old rows of the hunks shown.
pub(super) fn layout(
    frame: &Frame<Line>,
    hunks: &[Hunk],
    texts: &mut Texts,
    view: &EditorView,
    shaper: &mut Shaper,
    widths: &mut GlyphWidths,
    scroll_x: f32,
) -> Layout {
    let (buffer, theme, tab_size) = (view.model.text(), &view.theme, view.model.tab_size());
    texts.measure_with(widths, tab_size);
    let line_height = frame.line_height;
    let text_area = frame.text_bounds();
    let mut placed: Vec<Option<Placed>> = (frame.blocks.iter())
        .map(|laid| {
            let hunk = hunk(hunks, laid.id)?;
            Some(Placed {
                id: hunk.id,
                status: hunk.status,
                #[cfg(test)]
                header: hunk.header.clone(),
                rect: laid.rect,
                rows: Vec::new(),
                buttons: Vec::new(),
            })
        })
        .collect();
    let left = frame.text_left - scroll_x;
    let view_width = frame.width - frame.text_left;
    for (ix, hunk, row, y) in shown_rows(frame, hunks) {
        let Some(block) = placed[ix].as_mut() else {
            continue;
        };
        let (text, x) = match row {
            0 => {
                let runs = runs(&hunk.header, true, hunk.status, theme);
                let text_width = shaper.shape(&hunk.header, &runs).width();
                let (shaped, _) = header_buttons(
                    hunk.status,
                    view.read_only && !view.accepts_when_read_only,
                    text_width,
                    line_height,
                    shaper,
                    theme,
                );
                let inset = line_height * BUTTON_INSET;
                let pad = line_height * BUTTON_PAD;
                for (button, at, width, line) in shaped {
                    let rect = Rect {
                        x: left + at,
                        y: y + inset,
                        width,
                        height: line_height - inset * 2.,
                    };
                    let shown =
                        (rect.intersect(block.rect)).and_then(|rect| rect.intersect(text_area));
                    block.buttons.push(Button {
                        button,
                        rect,
                        shown,
                        label: Row {
                            at: Vec2 {
                                x: left + at + pad,
                                y,
                            },
                            line,
                            #[cfg(test)]
                            text: label(button).to_string(),
                        },
                    });
                }
                (hunk.header.clone(), 0.)
            }
            _ => {
                let view = Some((scroll_x, view_width));
                let line = texts.line(&hunk.new_text, row - 1, view, tab_size, widths);
                line.unwrap_or_default()
            }
        };
        let runs = runs(&text, row == 0, block.status, theme);
        block.rows.push(Row {
            at: Vec2 {
                x: left + x as f32,
                y,
            },
            line: shaper.shape(&text, &runs),
            #[cfg(test)]
            text,
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
        tints: Tints::of(theme),
        blocks: placed.into_iter().flatten().collect(),
        removed,
        hitboxes: Vec::new(),
    }
}

/// Puts a hitbox over the part of each button in view, for its pointing
/// hand.
pub(super) fn insert_hitboxes(
    proposals: &mut Layout,
    to_window: impl Fn(Rect) -> Bounds<Pixels>,
    window: &mut Window,
) {
    let shown = (proposals.blocks.iter())
        .flat_map(|block| &block.buttons)
        .filter_map(|button| button.shown);
    proposals.hitboxes = shown
        .map(|rect| window.insert_hitbox(to_window(rect), HitboxBehavior::Normal))
        .collect();
}

/// Paints the bands behind the hunks' old rows, under the text.
pub(super) fn paint_bands(proposals: &Layout, layout: &FrameLayout, window: &mut Window) {
    for &band in &proposals.removed {
        window.paint_quad(fill(layout.bounds(band), proposals.tints.band));
    }
}

/// Paints the hunks' blocks, each clipped to its rect inside the text
/// area, with their buttons, noting in `buttons` the part of each in view,
/// which a click presses, and in tests what it painted in `painted`.
pub(super) fn paint(
    proposals: &Layout,
    layout: &FrameLayout,
    window: &mut Window,
    cx: &mut App,
    buttons: &mut Vec<(HunkId, ProposalButton, Bounds<Pixels>)>,
    #[cfg(test)] painted: &mut Vec<(HunkId, HunkStatus, String, Bounds<Pixels>)>,
) {
    let line_height = px(layout.frame.line_height);
    for block in &proposals.blocks {
        let bounds: Bounds<Pixels> = layout.bounds(block.rect);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, proposals.tints.block));
            for button in &block.buttons {
                let rect = layout.bounds(button.rect);
                let corner = rect.size.height / 4.;
                window.paint_quad(
                    fill(rect, proposals.tints.button(button.button)).corner_radii(corner),
                );
                let label = &button.label;
                (label.line).paint(layout.to_window(label.at), line_height, window, cx);
            }
            for row in &block.rows {
                (row.line).paint(layout.to_window(row.at), line_height, window, cx);
            }
        });
        let shown = (block.buttons.iter())
            .filter_map(|button| Some((block.id, button.button, layout.bounds(button.shown?))));
        buttons.extend(shown);
        #[cfg(test)]
        painted.push((block.id, block.status, block.header.clone(), bounds));
    }
    for hitbox in &proposals.hitboxes {
        window.set_cursor_style(CursorStyle::PointingHand, hitbox);
    }
}
