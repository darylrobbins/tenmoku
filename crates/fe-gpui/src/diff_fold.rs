//! [`DiffView`]'s folding of unchanged rows (VS Code's hidden unchanged
//! regions): each run of rows the diff leaves alone, past [`CONTEXT_ROWS`]
//! rows around each hunk, is hidden on both sides, behind one row saying
//! `36 hidden lines` and, when the language has an outline, the symbols
//! enclosing the row below it. Its chevrons show [`EXPAND_ROWS`] more rows
//! at their end of the run, and a click on the label shows it all. Hunks
//! are never hidden, so moving between changes never lands in a hidden
//! row; a caret that gets into one some other way (a search, a jump, vim's
//! `G`, any of several carets) shows its run.
//!
//! The blocks ([`HiddenBlock`]) come from the diff's hunks, the rows shown
//! from them kept in the left side's rows, which a diff view never edits,
//! so they stay shown through edits to the right. Each side hides a block's
//! rows with an editor fold, the same on both, so the sides stay lined up
//! and scroll together: the fold's header is the row above the block, and
//! the label a [`HostBlockKind::Reserve`] row below it, which the view
//! draws over. A fold can't hide a text's first row, so a block starting
//! the text folds the rest behind it and its label is drawn over that row
//! instead, a caret moved onto it showing the block. Opening a fold any
//! other way, by its marker, `zo` or a jump into it, shows the block on
//! both sides; any other fold is undone, as before folding.
//!
//! `diff.toggle_folding` hides nothing, or hides again what was shown.
//! Folding starts on, and the host turns it off for views that compare two
//! files.

use std::{
    ops::{Range, RangeInclusive},
    sync::Arc,
};

use fe_buffer::Text;
use fe_editor::{
    decorations::Tint,
    text_diff::{Side, TextDiff},
};
use fe_syntax::{NoOutline, Outline};
use fe_ui::Icon;
use gpui::{
    AnyElement, App, AppContext as _, AvailableSpace, Bounds, ContentMask, Context,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Radians, SharedString,
    StatefulInteractiveElement as _, Styled as _, Transformation, WeakEntity, canvas, div, px, svg,
};

use fe_display::layout::Rect;

use super::DiffView;
use crate::{
    EditorTheme,
    commands::{HostBlock, HostBlockKind},
};

/// The unchanged rows kept shown above and below each hunk.
pub const CONTEXT_ROWS: u32 = 3;

/// The rows a chevron shows.
pub const EXPAND_ROWS: u32 = 20;

/// The ids of the label rows' blocks, from this one on, apart from the
/// fillers' and the removed rows'.
const LABEL_IDS: u64 = 1 << 32;

/// The ids of the hunk headers' blocks, from this one on: the hunk's index
/// added.
pub(super) const HEADER_IDS: u64 = 1 << 33;

/// A hunk header's text size over the editor's: code small, 12 px to the
/// code's 13.
const CODE_SMALL: f32 = 12. / 13.;

/// The start of a hunk header's element id and debug selector, never shown.
const HUNK_ID: &str = kiln_l10n::verbatim!("diff-hunk");

/// A side's folds as last set, as `(start, end)`, and the buffer version
/// they were set at.
pub(super) type Folded = (fe_buffer::Version, Vec<(u32, u32)>);

/// What an outline is of: the language's name and the syntax version.
pub(super) type OutlineKey = (&'static str, fe_buffer::Version);

/// A run of unchanged rows hidden on both sides, in each side's rows: as
/// many on each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HiddenBlock {
    pub left: Range<u32>,
    pub right: Range<u32>,
}

impl HiddenBlock {
    /// The block's rows on `side`.
    pub fn range(&self, side: Side) -> &Range<u32> {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    /// How many rows it hides on either side.
    pub fn len(&self) -> u32 {
        self.left.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.left.is_empty()
    }

    /// The left rows `expand` shows of it.
    pub fn expanded(&self, expand: Expand) -> Range<u32> {
        let Range { start, end } = self.left;
        match expand {
            Expand::Top => start..(start + EXPAND_ROWS).min(end),
            Expand::Bottom => end.saturating_sub(EXPAND_ROWS).max(start)..end,
            Expand::All => start..end,
        }
    }
}

/// What a click on a [`HiddenBlock`]'s row shows of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expand {
    /// Its first [`EXPAND_ROWS`], below the rows shown above it: its down
    /// chevron.
    Top,
    /// Its last [`EXPAND_ROWS`], above the rows shown below it: its up
    /// chevron.
    Bottom,
    /// All of it: its label.
    All,
}

/// How many rows `text` has, split as [`TextDiff`] and the editor split
/// them: `\r\n`, `\r` and `\n` each end one, and a text ending in one has
/// an empty last row.
pub(super) fn row_count(text: &str) -> u32 {
    let bytes = text.as_bytes();
    let breaks = (bytes.iter().enumerate())
        .filter(|&(ix, &byte)| {
            byte == b'\n' || (byte == b'\r' && bytes.get(ix + 1) != Some(&b'\n'))
        })
        .count();
    breaks as u32 + 1
}

/// The runs of rows `diff` leaves alone, between texts of `rows` rows
/// (left, right), less `context` rows next to each hunk and `revealed`
/// left rows, sorted. None when the texts are the same or too large to
/// compare: there are no hunks to show.
pub(super) fn hidden_blocks(
    diff: &TextDiff,
    rows: [u32; 2],
    context: u32,
    revealed: &[Range<u32>],
) -> Vec<HiddenBlock> {
    let hunks = diff.hunks();
    if hunks.is_empty() {
        return Vec::new();
    }
    // Each unchanged run, as (left start, right start, rows, whether a hunk
    // is above, whether one is below).
    let mut runs = Vec::with_capacity(hunks.len() + 1);
    let (mut left, mut right) = (0, 0);
    for (ix, hunk) in hunks.iter().enumerate() {
        runs.push((left, right, hunk.left.start - left, ix > 0, true));
        (left, right) = (hunk.left.end, hunk.right.end);
    }
    let tail = rows[0]
        .saturating_sub(left)
        .min(rows[1].saturating_sub(right));
    runs.push((left, right, tail, true, false));
    let mut blocks = Vec::new();
    for (left, right, len, above, below) in runs {
        let skip = if above { context } else { 0 };
        let keep_below = if below { context } else { 0 };
        if len <= skip + keep_below {
            continue;
        }
        let hidden = left + skip..left + len - keep_below;
        let shift = right as i64 - left as i64;
        for rows in subtract(hidden, revealed) {
            let right = (rows.start as i64 + shift) as u32..(rows.end as i64 + shift) as u32;
            blocks.push(HiddenBlock { left: rows, right });
        }
    }
    blocks
}

/// `rows` less every one of `revealed`, sorted and apart.
fn subtract(rows: Range<u32>, revealed: &[Range<u32>]) -> Vec<Range<u32>> {
    let mut pieces = Vec::new();
    let mut start = rows.start;
    let from = revealed.partition_point(|shown| shown.end <= rows.start);
    for shown in revealed[from..]
        .iter()
        .take_while(|shown| shown.start < rows.end)
    {
        if shown.start > start {
            pieces.push(start..shown.start);
        }
        start = start.max(shown.end);
    }
    if start < rows.end {
        pieces.push(start..rows.end);
    }
    pieces
}

/// Adds `rows` to `revealed`, keeping it sorted and its ranges apart.
pub(super) fn reveal(revealed: &mut Vec<Range<u32>>, rows: Range<u32>) {
    if rows.is_empty() {
        return;
    }
    let first = revealed.partition_point(|shown| shown.end < rows.start);
    let last = revealed.partition_point(|shown| shown.start <= rows.end);
    let mut merged = rows;
    for shown in &revealed[first..last] {
        merged = merged.start.min(shown.start)..merged.end.max(shown.end);
    }
    revealed.splice(first..last, [merged]);
}

/// The editor fold that hides `rows` of a side: behind the row above them,
/// or for rows starting the text, which no fold hides the first of, the
/// rest behind the first. `None` if that leaves nothing to hide.
pub(super) fn fold_of(rows: &Range<u32>) -> Option<RangeInclusive<u32>> {
    let header = rows.start.saturating_sub(1);
    (rows.end >= header + 2).then(|| header..=rows.end - 1)
}

/// The rows the fold of `rows` hides ([`fold_of`]).
pub(super) fn folded_rows(rows: &Range<u32>) -> Range<u32> {
    match fold_of(rows) {
        Some(fold) => fold.start() + 1..fold.end() + 1,
        None => rows.end..rows.end,
    }
}

/// The enclosing symbols of `row` of `text` in `outline`, outermost first,
/// as `impl Foo › fn bar`'s names: those containing the row's first
/// non-blank char. `None` if there are none.
pub fn crumb(outline: &Outline, text: &Text, row: u32) -> Option<String> {
    let row = row.min(text.row_count().checked_sub(1)?);
    let start = text.row_start(row);
    let len = (text.row_len(row) as usize).min(256);
    let line = text.slice_to_string(start..text.clip_offset(start + len, fe_buffer::Bias::Left));
    let indent = line.len() - line.trim_start().len();
    let mut ix = outline.item_at(start + indent);
    let mut names = Vec::new();
    while let Some(at) = ix {
        let item = &outline.items[at];
        names.push(item.name.as_str());
        ix = item.parent.map(|parent| parent as usize);
    }
    names.reverse();
    (!names.is_empty()).then(|| names.join(" › "))
}

/// A side's screen rows: its text rows, less those folded away, with the
/// rows of its blocks between them.
pub(super) struct ScreenRows {
    /// `(row it's above, rows)`, sorted.
    blocks: Vec<(u32, u32)>,
    /// Sorted and apart; no block is above one of their rows.
    hidden: Vec<Range<u32>>,
}

impl ScreenRows {
    pub(super) fn new(blocks: &[HostBlock], hidden: Vec<Range<u32>>) -> Self {
        let mut blocks: Vec<_> = (blocks.iter())
            .filter(|block| block.rows > 0)
            .map(|block| (block.above, block.rows))
            .collect();
        blocks.sort_by_key(|&(above, _)| above);
        Self { blocks, hidden }
    }

    /// The screen row text row `row` shows on, or for a hidden row, the
    /// row it's folded under.
    pub(super) fn screen_row(&self, row: u32) -> u32 {
        let at = self.hidden.partition_point(|run| run.end <= row);
        let row = match self.hidden.get(at) {
            Some(run) if run.start <= row => run.start.saturating_sub(1),
            _ => row,
        };
        let hidden: u32 = (self.hidden.iter())
            .take_while(|run| run.start < row)
            .map(|run| run.end.min(row) - run.start)
            .sum();
        let blocks: u32 = (self.blocks.iter())
            .take_while(|&&(above, _)| above <= row)
            .map(|&(_, rows)| rows)
            .sum();
        row - hidden + blocks
    }

    /// The text row at `screen`, and whether `screen` is a block's row; in
    /// one, the row below the block.
    pub(super) fn row_at_screen(&self, screen: u32) -> (u32, bool) {
        enum Run {
            Block(u32),
            Hidden(u32),
        }
        let mut runs: Vec<(u32, Run)> = (self.blocks.iter())
            .map(|&(above, rows)| (above, Run::Block(rows)))
            .chain((self.hidden.iter()).map(|run| (run.start, Run::Hidden(run.end))))
            .collect();
        runs.sort_by_key(|(at, _)| *at);
        let (mut row, mut top) = (0, 0);
        for (start, run) in runs {
            let text = start.saturating_sub(row);
            if screen < top + text {
                return (row + (screen - top), false);
            }
            top += text;
            row = row.max(start);
            match run {
                Run::Block(rows) if screen < top + rows => return (row, true),
                Run::Block(rows) => top += rows,
                Run::Hidden(end) => row = end,
            }
        }
        (row + (screen - top), false)
    }
}

/// What a label row shows, for drawing over its reserve block.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Label {
    pub(super) id: u64,
    /// The block's left start, which clicks name it by.
    pub(super) key: u32,
    /// The rows its fold hides.
    pub(super) hidden: u32,
    pub(super) crumb: Option<SharedString>,
    /// Whether rows are shown above and below it, which its chevrons
    /// extend.
    pub(super) above: bool,
    pub(super) below: bool,
    /// Whether it starts the text, its label drawn over the first row.
    pub(super) first: bool,
}

impl DiffView {
    /// Whether unchanged rows are hidden.
    pub fn is_folding(&self) -> bool {
        self.folding
    }

    /// Starts the view folding unchanged rows, or not.
    pub fn with_folding(mut self, folding: bool) -> Self {
        self.folding = folding;
        self.hidden = self.find_hidden();
        // Folded again from the next frame.
        self.folded = [None, None];
        self
    }

    /// The blocks hidden now.
    pub fn hidden_blocks(&self) -> &[HiddenBlock] {
        &self.hidden
    }

    /// `diff.toggle_folding`: hides unchanged rows, all of them again, or
    /// shows every row.
    pub fn toggle_folding(&mut self, cx: &mut Context<Self>) {
        self.set_folding(!self.folding, cx);
    }

    /// Hides unchanged rows, all of them again, or shows every row.
    pub fn set_folding(&mut self, folding: bool, cx: &mut Context<Self>) {
        self.folding = folding;
        self.revealed.clear();
        self.refold(cx);
    }

    /// Shows `expand`'s rows of the block starting at left row `key` on
    /// both sides.
    pub fn expand(&mut self, key: u32, expand: Expand, cx: &mut Context<Self>) {
        let Some(block) = self.hidden.iter().find(|block| block.left.start == key) else {
            return;
        };
        reveal(&mut self.revealed, block.expanded(expand));
        self.refold(cx);
    }

    /// The blocks to hide in the diff shown.
    fn find_hidden(&self) -> Vec<HiddenBlock> {
        if !self.folding {
            return Vec::new();
        }
        hidden_blocks(&self.diff, self.rows, CONTEXT_ROWS, &self.revealed)
    }

    /// Finds the blocks to hide again and hides them, showing first any
    /// that a side's carets are in: folded away or under a first block's
    /// label, but for a side's untouched caret at the start
    /// ([`DiffView::caret_rows`]).
    pub(super) fn refold(&mut self, cx: &mut Context<Self>) {
        self.hidden = self.find_hidden();
        // The carets as of the buffer the diff was of, edits through other
        // models taken in.
        for view in &self.views {
            view.update(cx, |view, _| view.model_mut().sync_display());
        }
        self.note_touched(cx);
        loop {
            let heads = [0, 1].map(|ix| self.caret_rows(ix, false, cx));
            let caret_in: Vec<_> = (self.hidden.iter())
                .filter(|block| {
                    heads[0].iter().any(|row| block.left.contains(row))
                        || heads[1].iter().any(|row| block.right.contains(row))
                })
                .map(|block| block.left.clone())
                .collect();
            if caret_in.is_empty() {
                break;
            }
            for rows in caret_in {
                reveal(&mut self.revealed, rows);
            }
            self.hidden = self.find_hidden();
        }
        self.apply_folds(cx);
        self.show_blocks(cx);
        self.heads = [0, 1].map(|ix| self.heads_of(ix, cx));
        cx.notify();
    }

    /// Notes each side whose carets have moved since last looked at: its
    /// initial caret is touched for good, wherever it is now.
    fn note_touched(&mut self, cx: &App) {
        for ix in 0..2 {
            if !self.heads[ix].is_empty() && self.heads_of(ix, cx) != self.heads[ix] {
                self.touched[ix] = true;
            }
        }
    }

    /// Side `ix`'s carets, as their heads, sorted.
    fn heads_of(&self, ix: usize, cx: &App) -> Vec<usize> {
        let model = self.views[ix].read(cx).model();
        (model.selections().iter())
            .map(|selection| selection.head())
            .collect()
    }

    /// The rows of side `ix`'s carets a hidden block's rows may not cover:
    /// none for the left inline, where it isn't shown, nor for a lone empty
    /// caret at the text's start that hasn't `moved`, as a view opens with,
    /// which a first block's label may cover.
    fn caret_rows(&self, ix: usize, moved: bool, cx: &App) -> Vec<u32> {
        if self.inline && ix == 0 {
            return Vec::new();
        }
        let model = self.views[ix].read(cx).model();
        let selections = model.selections();
        let untouched = matches!(&selections[..], [only] if only.range == (0..0));
        if untouched && !moved && !self.touched[ix] {
            return Vec::new();
        }
        let text = model.text();
        (self.heads_of(ix, cx).into_iter())
            .map(|head| text.offset_to_point(head).row)
            .collect()
    }

    /// Sets each side's folds to the blocks', noting them and the buffer's
    /// version.
    fn apply_folds(&mut self, cx: &mut Context<Self>) {
        for (ix, side) in [(0, Side::Left), (1, Side::Right)] {
            let folds: Vec<_> = (self.hidden.iter())
                .filter_map(|block| fold_of(block.range(side)))
                .collect();
            let applied = self.views[ix].update(cx, |view, cx| {
                let model = view.model_mut();
                let want = folds.iter().map(|fold| (*fold.start(), *fold.end()));
                let has = model
                    .display()
                    .folds()
                    .iter()
                    .map(|fold| (fold.start, fold.end));
                if !want.eq(has) {
                    model.unfold_all();
                    model.fold_all_exact(folds);
                    cx.notify();
                }
                let folds = model.display().folds().iter();
                let folds = folds.map(|fold| (fold.start, fold.end)).collect();
                (model.buffer().version(), folds)
            });
            self.folded[ix] = Some(applied);
        }
    }

    /// Keeps the sides lined up: unwrapped, and folded as the blocks are.
    /// A block's fold opened on either side (by its marker, `zo`, or a jump
    /// into it) shows the block on both, as does any caret in it, the
    /// untouched one under a first block's label once it moves; any other
    /// fold is undone.
    /// Until the diff of an edit lands, the folds stay as the edit left
    /// them.
    pub(super) fn keep_aligned(&mut self, cx: &mut Context<Self>) {
        // Before any early return below, since each path ends by taking the
        // heads as looked at.
        self.note_touched(cx);
        let mut opened = Vec::new();
        let mut refold = false;
        for (ix, side) in [(0, Side::Left), (1, Side::Right)] {
            let view = &self.views[ix];
            if view.read(cx).model().soft_wrap() {
                view.update(cx, |view, cx| {
                    view.model_mut().set_soft_wrap(false);
                    cx.notify();
                });
            }
            let model = view.read(cx).model();
            if (self.folded[ix].as_ref()).is_some_and(|(at, _)| *at != model.buffer().version()) {
                continue;
            }
            let has: Vec<_> = (model.display().folds().iter())
                .map(|fold| (fold.start, fold.end))
                .collect();
            let made = self.folded[ix].as_ref().map(|(_, folds)| folds);
            let moved = self.heads_of(ix, cx) != self.heads[ix];
            let heads = self.caret_rows(ix, moved, cx);
            let mut want = Vec::new();
            for block in &self.hidden {
                let rows = block.range(side);
                let caret_in = heads.iter().any(|row| rows.contains(row));
                let fold = fold_of(rows).map(|fold| (*fold.start(), *fold.end()));
                let made = (fold.zip(made)).is_some_and(|(fold, made)| made.contains(&fold));
                let opened_here = made && fold.is_some_and(|fold| !has.contains(&fold));
                if caret_in || opened_here {
                    opened.push(block.left.clone());
                }
                want.extend(fold);
            }
            // What was folded, as the map holds it, or with nothing folded
            // yet, the blocks'.
            refold |= has != *made.unwrap_or(&want);
        }
        for rows in &opened {
            reveal(&mut self.revealed, rows.clone());
        }
        if refold || !opened.is_empty() {
            self.refold(cx);
        } else {
            self.heads = [0, 1].map(|ix| self.heads_of(ix, cx));
        }
    }

    /// The label rows' blocks on side `ix`: one below each hidden block.
    pub(super) fn label_blocks(&self, ix: usize) -> Vec<HostBlock> {
        let side = [Side::Left, Side::Right][ix];
        // A first block's label is over the first row.
        (self.hidden.iter().enumerate())
            .filter(|(_, block)| block.range(side).start > 0)
            .map(|(id, block)| HostBlock {
                above: block.range(side).end,
                rows: 1,
                kind: HostBlockKind::Reserve,
                id: LABEL_IDS + id as u64,
            })
            .collect()
    }

    /// The hunk headers' blocks on side `ix`: a row above each hunk's first
    /// row, the same on both sides so they stay lined up. Before the side's
    /// other blocks, so a header is above a filler or removed rows at its
    /// row. Each is a band of no lines, which the editor lays out only in
    /// view, and the label layer draws the header over it: not a reserve
    /// block, which the editor places every frame, in view or not, at a cost
    /// that grows with their number.
    pub(super) fn header_blocks(&self, ix: usize) -> Vec<HostBlock> {
        let side = [Side::Left, Side::Right][ix];
        (self.diff.hunks().iter().enumerate())
            .map(|(n, hunk)| HostBlock {
                above: hunk.range(side).start,
                rows: 1,
                kind: HostBlockKind::Lines {
                    lines: Vec::new(),
                    tint: Tint::Muted,
                    words: Vec::new(),
                },
                id: HEADER_IDS + n as u64,
            })
            .collect()
    }

    /// The symbols enclosing hunk `n`, outermost first (`impl Foo › bar`),
    /// around its first row on a side it has rows on: the right's, else
    /// for a modified hunk the left's. A side with no rows in it isn't
    /// asked, so rows added or removed just above a symbol aren't put in
    /// it. `None` without an outline, or with no symbol around it.
    pub(super) fn hunk_symbol(&self, n: usize, cx: &App) -> Option<String> {
        let hunk = self.diff.hunks().get(n)?;
        let sides = [(1, &hunk.right), (0, &hunk.left)];
        let order = (sides.into_iter())
            .filter(|(_, rows)| !rows.is_empty())
            .map(|(ix, _)| ix);
        order.into_iter().find_map(|ix| {
            let (_, outline) = self.outlines[ix].as_ref()?;
            let text = self.views[ix].read(cx).model().text();
            crumb(
                outline,
                text,
                hunk.range([Side::Left, Side::Right][ix]).start,
            )
        })
    }

    /// What hunk `n`'s header says: the symbols around it and how many rows
    /// it removes and adds, `impl MinimapTile › render · 4 changes`, or the
    /// count alone with no symbol around it. `None` past the last hunk.
    pub fn hunk_header(&self, n: usize, cx: &App) -> Option<SharedString> {
        let hunk = self.diff.hunks().get(n)?;
        let count = hunk.left.len() + hunk.right.len();
        Some(match self.hunk_symbol(n, cx) {
            Some(symbol) => kiln_l10n::tr!("gpui-diff-hunk-header", symbol = symbol, count = count),
            None => kiln_l10n::tr!("gpui-diff-hunk-changes", count = count),
        })
    }

    /// The right side's screen rows as its blocks and folds are now.
    pub(super) fn right_screen_rows(&self) -> ScreenRows {
        let hidden = (self.hidden.iter())
            .map(|block| folded_rows(&block.right))
            .filter(|rows| !rows.is_empty())
            .collect();
        ScreenRows::new(&self.blocks_of(1), hidden)
    }

    /// Finds each side's outline again, off the main thread, when its
    /// syntax has moved on since, or is of another language, and there are
    /// hunks or blocks hidden: their headers and labels name the symbols
    /// around them. An outline found
    /// for a language or version the side no longer has isn't kept.
    pub(super) fn sync_outlines(&mut self, cx: &mut Context<Self>) {
        if self.hidden.is_empty() && self.diff.hunks().is_empty() {
            return;
        }
        for ix in 0..2 {
            let view = self.views[ix].read(cx);
            let Some(syntax) = view.syntax() else {
                (self.outlines[ix], self.outlining[ix]) = (None, None);
                continue;
            };
            let key = (syntax.language().name(), syntax.version());
            let found = self.outlines[ix].as_ref().map(|(at, _)| *at);
            let finding = self.outlining[ix].as_ref().map(|(at, _)| *at);
            if found == Some(key) || finding == Some(key) {
                continue;
            }
            // Of another language: not shown, nor kept.
            if found.is_some_and(|(language, _)| language != key.0) {
                self.outlines[ix] = None;
            }
            self.outlining[ix] = None;
            let job = match fe_syntax::outline_job(syntax) {
                Ok(job) => job,
                // Parsed later: the side redraws then.
                Err(NoOutline::NotParsed) => continue,
                Err(NoOutline::Unsupported | NoOutline::TooLarge) => {
                    self.outlines[ix] = None;
                    continue;
                }
            };
            let found = cx.background_spawn(async move { job.run() });
            let task = cx.spawn(async move |this, cx| {
                let outline = found.await;
                this.update(cx, |this, cx| {
                    let finding = this.outlining[ix].as_ref().map(|(at, _)| *at);
                    if finding == Some(key) {
                        this.outlines[ix] = Some((key, Arc::new(outline)));
                        this.outlining[ix] = None;
                        cx.notify();
                    }
                })
                .ok();
            });
            self.outlining[ix] = Some((key, task));
        }
    }

    /// The screen row right row `row` shows on as the view is now, side by
    /// side or inline, or for a hidden row, the row it's folded under.
    pub fn right_screen_row(&self, row: u32) -> u32 {
        self.right_screen_rows().screen_row(row)
    }

    /// Puts the right caret at the start of `row` and shows it, the block
    /// hiding it shown, centred among `shown` rows if given: both sides
    /// scroll there. Until the diff of the texts as they are lands, as
    /// after a side's text arrives or with large texts, each diff landing
    /// shows and scrolls to it again, unless a side is scrolled meanwhile.
    pub fn reveal_right_row(&mut self, row: u32, shown: Option<u32>, cx: &mut Context<Self>) {
        let row = self.views[1].update(cx, |view, cx| {
            let text = view.model().text();
            let row = row.min(text.row_count().saturating_sub(1));
            let at = text.row_start(row);
            view.select_and_reveal(at..at, cx);
            row
        });
        self.revealing = Some((row, shown));
        self.go_on_revealing(cx);
    }

    /// Shows and scrolls to the row being revealed, as the diff is now,
    /// done with it once that's the diff of the texts as they are.
    pub(super) fn go_on_revealing(&mut self, cx: &mut Context<Self>) {
        let Some((row, shown)) = self.revealing else {
            return;
        };
        self.show_right_row(row, cx);
        match shown {
            Some(shown) => {
                let top = self.right_screen_row(row) as f32 - (shown / 2) as f32;
                for view in &self.views {
                    view.update(cx, |view, cx| view.set_scroll_top(top, cx));
                }
            }
            // The caret's row, wherever the folds put it.
            None => self.views[1].update(cx, |view, cx| {
                view.autoscroll = true;
                cx.notify();
            }),
        }
        let versions = (self.views.each_ref()).map(|view| view.read(cx).model().buffer().version());
        if self.diffing.is_none() && self.diffed == Some(versions) {
            self.revealing = None;
        }
    }

    /// Shows the hidden block holding right row `row`, if one does, on both
    /// sides.
    pub fn show_right_row(&mut self, row: u32, cx: &mut Context<Self>) {
        let Some(block) = self.hidden.iter().find(|block| block.right.contains(&row)) else {
            return;
        };
        reveal(&mut self.revealed, block.left.clone());
        self.refold(cx);
    }

    /// What side `ix`'s label rows show.
    pub(super) fn labels(&self, ix: usize, cx: &App) -> Vec<Label> {
        let side = [Side::Left, Side::Right][ix];
        let model = self.views[ix].read(cx).model();
        let text = model.text();
        let outline = self.outlines[ix].as_ref().map(|(_, outline)| &**outline);
        let rows = self.rows[ix];
        (self.hidden.iter().enumerate())
            .map(|(id, block)| {
                let rows_of = block.range(side);
                let below = rows_of.end < rows;
                // The row below it, or at the end its last.
                let row = match below {
                    true => rows_of.end,
                    false => rows_of.end - 1,
                };
                Label {
                    id: LABEL_IDS + id as u64,
                    key: block.left.start,
                    hidden: rows_of.len() as u32,
                    crumb: outline
                        .and_then(|outline| crumb(outline, text, row))
                        .map(Into::into),
                    above: rows_of.start > 0,
                    below,
                    first: rows_of.start == 0,
                }
            })
            .collect()
    }

    /// Side `ix`'s label rows and hunk headers, drawn over their reserve
    /// blocks, or a first block's label over the first row, and the `+` or
    /// `−` of each changed row in view ([`glyphs`]), where the side's last
    /// layout put them: absolute over the side, after it. `None` with no
    /// hunks.
    pub(super) fn label_layer(&self, ix: usize, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.diff.hunks().is_empty() {
            return None;
        }
        let labels = self.labels(ix, cx);
        let editor = self.views[ix].clone();
        let this = cx.entity().downgrade();
        let layer = canvas(
            move |_, window, cx| {
                let view = editor.read(cx);
                let theme = view.theme.clone();
                let reserved = view.reserved_blocks().to_vec();
                let first_row = view.layout.as_ref().and_then(|layout| {
                    let frame = &layout.frame;
                    let row = frame.rows.iter().find(|row| row.buffer_row == 0)?;
                    let whole = |y, height| Rect {
                        x: 0.,
                        y,
                        width: frame.width,
                        height,
                    };
                    let bounds = layout.bounds(whole(frame.row_top(row.row), frame.line_height));
                    Some((bounds, layout.bounds(whole(0., frame.height))))
                });
                let gutter = (view.layout.as_ref()).map_or(0., |layout| layout.frame.text_left);
                let in_view = |bounds: &Bounds<Pixels>, clip: &Bounds<Pixels>| {
                    bounds.bottom() > clip.top() && bounds.top() < clip.bottom()
                };
                // The headers in view, and what each says.
                let headers: Vec<_> = (this.upgrade())
                    .map(|diff| {
                        let diff = diff.read(cx);
                        (header_rows(view).into_iter())
                            .filter_map(|(n, bounds, clip)| {
                                Some((n, diff.hunk_header(n, cx)?, bounds, clip))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let glyphs = (this.upgrade())
                    .map(|diff| glyphs(diff.read(cx), ix, cx))
                    .unwrap_or_default();
                let mut elements = Vec::new();
                let mut place = |mut element: AnyElement,
                                 bounds: Bounds<Pixels>,
                                 clip: Bounds<Pixels>,
                                 window: &mut gpui::Window,
                                 cx: &mut App| {
                    window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                        let space = bounds.size.map(AvailableSpace::Definite);
                        element.prepaint_as_root(bounds.origin, space, window, cx);
                    });
                    elements.push((element, clip));
                };
                for label in &labels {
                    let placed = match label.first {
                        true => first_row,
                        false => (reserved.iter())
                            .find(|block| block.id == label.id)
                            .map(|block| (block.bounds, block.clip)),
                    };
                    let Some((bounds, clip)) = placed else {
                        continue;
                    };
                    if !in_view(&bounds, &clip) {
                        continue;
                    }
                    let element = label_row(label, ix, &theme, px(gutter), &this);
                    place(element, bounds, clip, window, cx);
                }
                for (n, text, bounds, clip) in headers {
                    let element = header_row(text, ix, n, &theme, px(gutter));
                    place(element, bounds, clip, window, cx);
                }
                for (glyph, bounds, clip) in glyphs {
                    place(glyph_cell(glyph, &theme), bounds, clip, window, cx);
                }
                elements
            },
            |_, elements, window, cx| {
                for (mut element, clip) in elements {
                    window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                        element.paint(window, cx)
                    });
                }
            },
        );
        Some(
            layer
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .into_any_element(),
        )
    }
}

/// A row's mark in a diff side's gutter, so colour is never the only cue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    /// `+`: a row the right side has in a hunk, added or changed.
    Added,
    /// `−`: a row the left side has in a hunk, removed or changed, or
    /// inline a row of a removed block.
    Removed,
}

impl Glyph {
    pub fn text(self) -> &'static str {
        match self {
            Glyph::Added => "+",
            Glyph::Removed => "\u{2212}",
        }
    }
}

/// The marks side `ix` of `diff` shows in its gutter as its last frame laid
/// it out, each where it goes, at the gutter's left edge, and the side's
/// bounds, which clip it: `−` on the left's rows in a hunk, `+` on the
/// right's, and inline `−` on each row of a removed block in view. None
/// without a gutter.
pub(super) fn glyphs(
    diff: &DiffView,
    ix: usize,
    cx: &App,
) -> Vec<(Glyph, Bounds<Pixels>, Bounds<Pixels>)> {
    let side = [Side::Left, Side::Right][ix];
    let view = diff.views[ix].read(cx);
    let Some(layout) = view.layout.as_ref() else {
        return Vec::new();
    };
    // Only in the side's own column: never over a host's, such as blame.
    let owned = (view.gutter_annotations())
        .is_some_and(|annotations| std::rc::Rc::ptr_eq(&annotations.hover, &diff.glyph_column));
    let frame = &layout.frame;
    let column = frame.annotation_column;
    if !owned || column.width <= 0. {
        return Vec::new();
    }
    let cell = |y: f32| {
        layout.bounds(Rect {
            x: column.x,
            y,
            width: column.width,
            height: frame.line_height,
        })
    };
    let clip = layout.bounds(Rect {
        x: 0.,
        y: 0.,
        width: frame.width,
        height: frame.height,
    });
    let glyph = match side {
        Side::Left => Glyph::Removed,
        Side::Right => Glyph::Added,
    };
    let hunks = diff.diff.hunks();
    let mut out = Vec::new();
    let mut previous = None;
    for row in &frame.rows {
        // A row's first display row only, though the sides don't wrap.
        if previous == Some(row.buffer_row) {
            continue;
        }
        previous = Some(row.buffer_row);
        let at = hunks.partition_point(|hunk| hunk.range(side).end <= row.buffer_row);
        if hunks
            .get(at)
            .is_some_and(|hunk| hunk.range(side).contains(&row.buffer_row))
        {
            out.push((glyph, cell(frame.row_top(row.row)), clip));
        }
    }
    if diff.inline && ix == 1 {
        let blocks = view.blocks();
        let line_height = frame.line_height;
        for laid in &frame.blocks {
            let removed = (blocks.get(laid.id as usize)).is_some_and(|block| {
                block.id < HEADER_IDS && matches!(block.kind, HostBlockKind::Lines { .. })
            });
            if !removed || line_height <= 0. {
                continue;
            }
            // Only its rows in the frame: a large block is mostly out of it.
            let first = (-laid.rect.y / line_height).floor().max(0.) as u32;
            let end = ((frame.height - laid.rect.y) / line_height).ceil().max(0.) as u32;
            for line in first..end.min(laid.rows) {
                let y = laid.rect.y + line_height * line as f32;
                out.push((Glyph::Removed, cell(y), clip));
            }
        }
    }
    out
}

/// The hunk headers' blocks of `view`'s last frame in view: each one's hunk,
/// its window bounds across the frame, and the frame's, which clip it.
fn header_rows(view: &crate::EditorView) -> Vec<(usize, Bounds<Pixels>, Bounds<Pixels>)> {
    let Some(layout) = view.layout.as_ref() else {
        return Vec::new();
    };
    let frame = &layout.frame;
    let clip = layout.bounds(Rect {
        x: 0.,
        y: 0.,
        width: frame.width,
        height: frame.height,
    });
    let blocks = view.blocks();
    (frame.blocks.iter())
        .filter_map(|laid| {
            let block = blocks.get(usize::try_from(laid.id).ok()?)?;
            let n = block.id.checked_sub(HEADER_IDS)?;
            let bounds = layout.bounds(Rect {
                x: 0.,
                y: laid.rect.y,
                width: frame.width,
                height: frame.line_height * laid.rows as f32,
            });
            Some((n as usize, bounds, clip))
        })
        .collect()
}

impl DiffView {
    /// Gives each side shown, while there are hunks, a gutter annotation
    /// column for its rows' glyphs ([`glyphs`]): one blank entry over the
    /// whole text, which only makes room, left of the gutter's own marks, and
    /// shows no popover. It's made again as of each buffer version the side
    /// is drawn at, so the editor never maps edits from an old version to
    /// place it. A column the host shows instead, such as blame, is left
    /// be and no glyphs are drawn over it; they come back once it goes.
    pub(super) fn show_glyph_column(&self, cx: &mut Context<Self>) {
        let want = !self.diff.hunks().is_empty();
        for (ix, view) in self.views.iter().enumerate() {
            let shown = !(self.inline && ix == 0) && want;
            let has = view
                .read(cx)
                .gutter_annotations()
                .map(|annotations| std::rc::Rc::ptr_eq(&annotations.hover, &self.glyph_column));
            match (has, shown) {
                // Another's column, or none wanted and none there.
                (Some(false), _) | (None, false) => continue,
                // Ours, as of the buffer's version: an older one would have
                // every layout replay the edits since.
                (Some(true), true) => {
                    let view = view.read(cx);
                    let version = view.model().buffer().version();
                    let current = (view.gutter_annotations())
                        .is_some_and(|annotations| annotations.version == version);
                    if current {
                        continue;
                    }
                }
                (Some(true), false) | (None, true) => {}
            }
            let hover = self.glyph_column.clone();
            view.update(cx, |view, cx| {
                let annotations = shown.then(|| {
                    // The shared buffer's, which the `current` check above
                    // compares with: the side's model can lag it while a
                    // diff of an edit through another model is pending.
                    let snapshot = view.model().buffer().snapshot();
                    let rows = snapshot.anchor_before(0)..snapshot.anchor_after(snapshot.len());
                    crate::GutterAnnotations {
                        version: snapshot.version(),
                        entries: vec![crate::GutterAnnotation {
                            rows,
                            text: SharedString::new_static(" "),
                            heat: None,
                        }],
                        hover,
                        on_click: std::rc::Rc::new(|_, _, _, _| {}),
                    }
                });
                view.set_gutter_annotations(annotations, cx);
            });
        }
    }

    /// The row bands and word highlights side `ix` (0 left, 1 right) painted
    /// in its last frame, in paint order: each one's window bounds, colour,
    /// and whether it's a word's. For tests outside the crate.
    #[doc(hidden)]
    pub fn drawn_tints(&self, ix: usize, cx: &App) -> Vec<(Bounds<Pixels>, gpui::Hsla, bool)> {
        use fe_editor::decorations::DecorationKind;
        let view = self.views[ix].read(cx);
        let Some(layout) = view.layout.as_ref() else {
            return Vec::new();
        };
        (layout.decorations.placed.iter())
            .filter_map(|placed| match placed.kind {
                DecorationKind::RowBackground => Some((placed.bounds, placed.color, false)),
                DecorationKind::Highlight => Some((placed.bounds, placed.color, true)),
                _ => None,
            })
            .collect()
    }

    /// Where side `ix` (0 left, 1 right) painted its host's gutter glyph,
    /// a lightbulb, in its last frame. For tests outside the crate.
    #[doc(hidden)]
    pub fn drawn_lightbulb(&self, ix: usize, cx: &App) -> Option<Bounds<Pixels>> {
        self.views[ix].read(cx).glyph_hit
    }

    /// The marks side `ix` (0 left, 1 right) showed in its gutter in its
    /// last frame, each with the window bounds it was drawn in: see
    /// [`Glyph`]. For tests outside the crate.
    #[doc(hidden)]
    pub fn drawn_glyphs(&self, ix: usize, cx: &App) -> Vec<(Glyph, Bounds<Pixels>)> {
        (glyphs(self, ix, cx).into_iter())
            .map(|(glyph, bounds, _)| (glyph, bounds))
            .collect()
    }
}

/// A row's [`Glyph`], centred in its cell, in the change colour of what it
/// marks.
fn glyph_cell(glyph: Glyph, theme: &EditorTheme) -> AnyElement {
    let color = match glyph {
        Glyph::Added => theme.change_added,
        Glyph::Removed => theme.change_deleted,
    };
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(color)
        .text_size(theme.font_size)
        .font_family(theme.font_family.clone())
        .child(glyph.text())
        .into_any_element()
}

/// Hunk `n`'s header row on side `side`: `text`, code small and muted,
/// after the gutter, on the filler colour. Its debug selector is
/// `diff-hunk:<side>:<n>`.
fn header_row(
    text: SharedString,
    side: usize,
    n: usize,
    theme: &EditorTheme,
    gutter: Pixels,
) -> AnyElement {
    div()
        .id(SharedString::from(format!("{HUNK_ID}:{side}:{n}")))
        .debug_selector(move || format!("{HUNK_ID}:{side}:{n}"))
        // The wheel scrolls the side beneath.
        .block_mouse_except_scroll()
        .size_full()
        .flex()
        .flex_row()
        .items_center()
        .bg(theme.background.blend(theme.diff_filler))
        .text_color(theme.gutter_foreground)
        .text_size(theme.font_size * CODE_SMALL)
        .font_family(theme.font_family.clone())
        .whitespace_nowrap()
        .overflow_hidden()
        // Past the gutter, where the text starts.
        .child(div().flex_none().w(gutter))
        .child(div().min_w_0().px(px(6.)).overflow_hidden().child(text))
        .into_any_element()
}

/// A label row: its chevrons, `36 hidden lines` and the symbols around it,
/// on the filler colour. Its debug selectors are `diff-fold-<part>:<side>:<n>`,
/// `n` the block's index among those hidden.
fn label_row(
    label: &Label,
    side: usize,
    theme: &EditorTheme,
    gutter: Pixels,
    this: &WeakEntity<DiffView>,
) -> AnyElement {
    let n = label.id - LABEL_IDS;
    let key = label.key;
    let click = |expand: Expand| {
        let this = this.clone();
        move |_: &gpui::ClickEvent, _: &mut gpui::Window, cx: &mut App| {
            this.update(cx, |this, cx| this.expand(key, expand, cx))
                .ok();
        }
    };
    let chevron = |part: &'static str, angle: f32, expand: Expand| {
        let size = theme.font_size;
        div()
            .id(SharedString::from(format!("diff-fold-{part}:{side}:{n}")))
            .debug_selector(move || format!("diff-fold-{part}:{side}:{n}"))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .px(px(4.))
            .h_full()
            .cursor_pointer()
            .hover(|style| style.bg(theme.sticky_scroll_hover))
            .on_click(click(expand))
            .child(
                svg()
                    .path(Icon::ChevronRight.path())
                    .size(size)
                    .with_transformation(Transformation::rotate(Radians(angle)))
                    .text_color(theme.gutter_foreground),
            )
    };
    let hidden = kiln_l10n::tr!("gpui-diff-hidden-lines", count = label.hidden);
    let quarter = std::f32::consts::FRAC_PI_2;
    div()
        .id(SharedString::from(format!("diff-fold:{side}:{n}")))
        .debug_selector(move || format!("diff-fold:{side}:{n}"))
        // Clicks are the row's; the wheel scrolls the side beneath.
        .block_mouse_except_scroll()
        .size_full()
        .flex()
        .flex_row()
        .items_center()
        // Opaque: a first block's is over its first row.
        .bg(theme.background.blend(theme.diff_filler))
        .text_color(theme.gutter_foreground)
        .text_size(theme.font_size)
        .font_family(theme.font_family.clone())
        .whitespace_nowrap()
        .overflow_hidden()
        // Past the gutter, where the text starts.
        .child(div().flex_none().w(gutter))
        .children(label.below.then(|| chevron("up", -quarter, Expand::Bottom)))
        .children(label.above.then(|| chevron("down", quarter, Expand::Top)))
        .child(
            div()
                .id(SharedString::from(format!("diff-fold-label:{side}:{n}")))
                .debug_selector(move || format!("diff-fold-label:{side}:{n}"))
                .flex_none()
                .px(px(6.))
                .cursor_pointer()
                .hover(|style| style.text_color(theme.foreground))
                .on_click(click(Expand::All))
                .child(hidden),
        )
        .children(label.crumb.clone().map(|crumb| {
            div()
                .debug_selector(move || format!("diff-fold-crumb:{side}:{n}"))
                .min_w_0()
                .overflow_hidden()
                .child(crumb)
        }))
        .into_any_element()
}
