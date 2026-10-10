//! Positions in rows too long to shape whole, such as minified files and data
//! dumps. [`RowIndex`] finds the x and display column of any offset in a row,
//! and the offset at any x, without shaping the row, so the layout can shape
//! only the part that's on screen.
//!
//! Offsets are buffer offsets; x is from the row's start, in the unit of the
//! [`CharWidth`] (the UI uses pixels). X is an `f64`: 10 MB along a row is
//! tens of millions of pixels, where an `f32` is off by several and summing
//! `f32` widths drifts much further. Sums and multiples of `f32` widths are
//! exact in an `f64` up to about 2^31, so x matches measuring the prefix
//! exactly, whichever way it's found.

use std::{
    collections::HashMap,
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use fe_buffer::{BufferId, BufferSnapshot, Version};
use fe_text::{Bias, TextSource};
use unicode_segmentation::UnicodeSegmentation;

use crate::{CharWidth, layout::MAX_SHAPED_ROW_BYTES};

/// Bytes between checkpoints.
pub(crate) const CHUNK: usize = 4 * 1024;

/// Longest grapheme cluster that [`RowIndex`] measures as a unit, with
/// [`CharWidth::cluster_width`]. A longer one, such as thousands of stacked
/// combining marks or a long ZWJ chain, is pathological: the index cuts it
/// into pieces of at most this many bytes, at char boundaries, and measures
/// each as the sum of its chars' widths, so neither measuring nor slicing it
/// grows with its length. A shaper draws such a cluster its own way, so
/// positions in and after it may be off.
pub const MAX_CLUSTER_BYTES: usize = 256;

/// Rows [`LongRows`] holds before it drops those the last two frames
/// didn't use.
const MAX_CACHED_ROWS: usize = 128;

/// A unit boundary in a row (see [`RowIndex`]).
#[derive(Copy, Clone, Debug, PartialEq)]
struct Position {
    offset: usize,
    /// The display column, counting chars and expanding tabs as
    /// [`crate::ExpandedRow`] does.
    column: usize,
    x: f64,
    /// On a checkpoint, that it starts a unit which ends at the next
    /// checkpoint, so queries inside the unit needn't measure it again.
    cluster: bool,
}

/// Where a walk along a row stops.
#[derive(Copy, Clone)]
enum Target {
    /// At the last unit boundary at or before this offset.
    Offset(usize),
    /// At the first unit boundary at or after this offset.
    AtOrAfter(usize),
    /// At the last unit boundary at or before this x.
    X(f64),
}

/// Checkpoints along one buffer row, each on the first unit boundary at or
/// after a 4 KB mark and holding its display column and x. A unit that
/// crosses a mark gets a checkpoint at its start too, so it's measured only
/// once. They're measured lazily, only as far along the row as a query
/// needs, so a query costs at most a chunk of measuring once the row is
/// indexed up to it.
///
/// The units are grapheme clusters, measured whole: a multi-char cluster
/// with [`CharWidth::cluster_width`], any other char with
/// [`CharWidth::char_width`]. A cluster longer than [`MAX_CLUSTER_BYTES`]
/// is cut into pieces instead, each a unit measured as the sum of its
/// chars' widths. Offsets inside a unit clamp to its start, and offsets
/// found are unit boundaries.
///
/// Queries take the measurer the index was made with. Printable ASCII is
/// measured once, and never segmented: each of its chars is a cluster of its
/// own. When it's all one width (a monospace font), runs of it advance
/// without per-char work.
pub struct RowIndex {
    text: Arc<dyn TextSource>,
    end: usize,
    tab_size: usize,
    ascii: [f64; 128],
    /// The width of every printable ASCII char, when they're all equal.
    advance: Option<f64>,
    /// The start of each chunk measured so far, then where measuring got to:
    /// the row's end once it's all measured.
    checkpoints: Vec<Position>,
    /// Bytes looked at, so tests can check queries stay local.
    #[cfg(test)]
    scanned: std::cell::Cell<usize>,
}

/// The part of a row to shape for a view, from [`RowIndex::slice`].
#[derive(Clone, Debug, PartialEq)]
pub struct RowSlice {
    /// Buffer offsets, on unit boundaries (see [`RowIndex`]), at most
    /// [`MAX_SHAPED_ROW_BYTES`] apart.
    pub range: Range<usize>,
    /// The x of `range.start`.
    pub start_x: f64,
    /// The display column of `range.start`, for
    /// [`crate::ExpandedRow::with_start_column`].
    pub start_column: usize,
}

impl RowIndex {
    /// Indexes the buffer row of `text` at `range`, which excludes its line
    /// break. Nothing past the row's start is measured yet. Keeps a
    /// [`TextSource::share`] of `text`.
    pub fn new(
        text: &dyn TextSource,
        range: Range<usize>,
        tab_size: usize,
        measure: &mut dyn CharWidth,
    ) -> Self {
        let ascii: [f64; 128] =
            std::array::from_fn(|ch| f64::from(measure.char_width(char::from(ch as u8))));
        let printable = &ascii[usize::from(b' ')..=usize::from(b'~')];
        let advance = printable
            .iter()
            .all(|&width| width == printable[0])
            .then_some(printable[0]);
        Self {
            text: text.share(),
            end: range.end,
            tab_size: tab_size.max(1),
            ascii,
            advance,
            checkpoints: vec![Position {
                offset: range.start,
                column: 0,
                x: 0.,
                cluster: false,
            }],
            #[cfg(test)]
            scanned: Default::default(),
        }
    }

    pub fn range(&self) -> Range<usize> {
        self.checkpoints[0].offset..self.end
    }

    /// Bytes looked at so far.
    #[cfg(test)]
    pub(crate) fn scanned(&self) -> usize {
        self.scanned.get()
    }

    /// The x of `offset`, which is clamped to the row and onto a unit
    /// boundary towards the start.
    pub fn x_for_offset(&mut self, offset: usize, measure: &mut dyn CharWidth) -> f64 {
        self.locate(offset, measure).x
    }

    /// The display column of `offset`, clamped as for
    /// [`RowIndex::x_for_offset`].
    pub fn column_for_offset(&mut self, offset: usize, measure: &mut dyn CharWidth) -> usize {
        self.locate(offset, measure).column
    }

    /// The last unit boundary at or before `x`: the start of the unit under
    /// it, or the row's end past it.
    pub fn offset_for_x(&mut self, x: f64, measure: &mut dyn CharWidth) -> usize {
        self.locate_x(x, measure).offset
    }

    /// The whole row's width, which measures all of it.
    pub fn width(&mut self, measure: &mut dyn CharWidth) -> f64 {
        self.locate(self.end, measure).x
    }

    /// The part of the row to shape to show `x_range`: from a unit boundary
    /// at or left of `x_range.start - margin` to one at or right of
    /// `x_range.end + margin`, clamped to the row and to
    /// [`RowIndex::slice_limit`]. Where the limit cuts it short, which takes
    /// kilobytes of text measured into a few hundred pixels, such as a
    /// pathological cluster, the slice starts at most half the limit before
    /// the view, so the view gets the rest.
    pub fn slice(
        &mut self,
        x_range: Range<f64>,
        margin: f64,
        measure: &mut dyn CharWidth,
    ) -> RowSlice {
        let mut start = self.locate_x(x_range.start - margin, measure);
        let mut end = self.boundary_at_or_after_x(x_range.end + margin, measure);
        if end.saturating_sub(start.offset) > MAX_SHAPED_ROW_BYTES {
            let view = self.locate_x(x_range.start, measure).offset;
            if view - start.offset > MAX_SHAPED_ROW_BYTES / 2 {
                start = self.locate(view - MAX_SHAPED_ROW_BYTES / 2, measure);
            }
            end = end.min(self.slice_limit(start.offset, measure));
        }
        RowSlice {
            range: start.offset..end.max(start.offset),
            start_x: start.x,
            start_column: start.column,
        }
    }

    /// `slice` with its end moved right by at least `by`, to the first unit
    /// boundary that far along, clamped to the row and to
    /// [`RowIndex::slice_limit`]. For a slice shaped narrower than the index
    /// measures it.
    pub fn widen(&mut self, slice: &RowSlice, by: f64, measure: &mut dyn CharWidth) -> RowSlice {
        let limit = self.slice_limit(slice.range.start, measure);
        let end_x = self.x_for_offset(slice.range.end, measure) + by;
        // At or past the limit's x, there's no need to look further. Text
        // measured as nothing after the limit has its x too, so looking for
        // `end_x` there would run on past the limit.
        let end = if self.x_for_offset(limit, measure) <= end_x {
            limit
        } else {
            self.boundary_at_or_after_x(end_x, measure).min(limit)
        };
        RowSlice {
            range: slice.range.start..end.max(slice.range.end),
            ..slice.clone()
        }
    }

    /// The furthest a slice from the unit boundary `start` may reach, so
    /// that shaping it is bounded however the row's text is drawn: the last
    /// unit boundary at most [`MAX_SHAPED_ROW_BYTES`] past `start`, or the
    /// row's end. Units are at most [`MAX_CLUSTER_BYTES`] long, so that's
    /// past `start` unless `start` is the row's end.
    pub fn slice_limit(&mut self, start: usize, measure: &mut dyn CharWidth) -> usize {
        if self.end.saturating_sub(start) <= MAX_SHAPED_ROW_BYTES {
            return self.end;
        }
        self.locate(start + MAX_SHAPED_ROW_BYTES, measure).offset
    }

    /// The unit boundary after the one `offset` clamps to, or the row's
    /// end.
    pub fn next_boundary(&mut self, offset: usize, measure: &mut dyn CharWidth) -> usize {
        let at = self.locate(offset, measure);
        self.boundary_after(at, measure)
    }

    /// The first unit boundary at or after `x`, or the row's end.
    fn boundary_at_or_after_x(&mut self, x: f64, measure: &mut dyn CharWidth) -> usize {
        let at = self.locate_x(x, measure);
        if at.x >= x {
            return at.offset;
        }
        // Include the unit that crosses `x`.
        self.boundary_after(at, measure)
    }

    /// The unit boundary after `at`, or the row's end.
    fn boundary_after(&mut self, at: Position, measure: &mut dyn CharWidth) -> usize {
        if at.offset == self.end {
            return at.offset;
        }
        let ix = self.checkpoints.partition_point(|c| c.offset <= at.offset);
        if self.checkpoints[ix - 1].offset == at.offset && self.checkpoints[ix - 1].cluster {
            return self.checkpoints[ix].offset;
        }
        let bound = self.bound_after(at.offset);
        let next_char = self.text.clip_offset(at.offset + 1, Bias::Right);
        self.walk(at, bound, Target::AtOrAfter(next_char), measure)
            .offset
    }

    fn locate(&mut self, offset: usize, measure: &mut dyn CharWidth) -> Position {
        let offset = self
            .text
            .clip_offset(offset.clamp(self.range().start, self.end), Bias::Left);
        while let Some(mark) = self.next_mark()
            && mark <= offset
        {
            self.measure_chunk(mark, measure);
        }
        let ix = self.checkpoints.partition_point(|at| at.offset <= offset) - 1;
        if self.checkpoints[ix].cluster {
            return self.checkpoints[ix];
        }
        let bound = self.bound_after(self.checkpoints[ix].offset);
        self.walk(self.checkpoints[ix], bound, Target::Offset(offset), measure)
    }

    fn locate_x(&mut self, x: f64, measure: &mut dyn CharWidth) -> Position {
        if x < 0. {
            return self.checkpoints[0];
        }
        while let Some(mark) = self.next_mark()
            && self.checkpoints.last().is_some_and(|last| last.x <= x)
        {
            self.measure_chunk(mark, measure);
        }
        let ix = self.checkpoints.partition_point(|at| at.x <= x) - 1;
        if self.checkpoints[ix].cluster {
            return self.checkpoints[ix];
        }
        let bound = self.bound_after(self.checkpoints[ix].offset);
        self.walk(self.checkpoints[ix], bound, Target::X(x), measure)
    }

    /// The checkpoint after `offset`, or the row's end: a unit boundary that
    /// walks from `offset` can stop at.
    fn bound_after(&self, offset: usize) -> usize {
        let ix = self.checkpoints.partition_point(|at| at.offset <= offset);
        self.checkpoints.get(ix).map_or(self.end, |at| at.offset)
    }

    /// The 4 KB mark the next checkpoint goes at or after, unless the row is
    /// measured to its end.
    fn next_mark(&self) -> Option<usize> {
        let last = self.checkpoints.last()?.offset;
        (last < self.end).then(|| {
            self.text
                .clip_offset((last + CHUNK).min(self.end), Bias::Left)
        })
    }

    fn measure_chunk(&mut self, mark: usize, measure: &mut dyn CharWidth) {
        let last = *self.checkpoints.last().expect("the row's start");
        let before = self.walk(last, self.end, Target::Offset(mark), measure);
        if before.offset < mark {
            // A unit crosses the mark: mark its start.
            if before.offset == last.offset {
                self.checkpoints
                    .last_mut()
                    .expect("the row's start")
                    .cluster = true;
            } else {
                self.checkpoints.push(Position {
                    cluster: true,
                    ..before
                });
            }
        }
        let next = self.walk(before, self.end, Target::AtOrAfter(mark), measure);
        self.checkpoints.push(next);
    }

    /// Measures from `from` towards `bound` until `target`. Both are unit
    /// boundaries.
    fn walk(
        &self,
        from: Position,
        bound: usize,
        target: Target,
        measure: &mut dyn CharWidth,
    ) -> Position {
        let stop = match target {
            Target::Offset(offset) | Target::AtOrAfter(offset) => bound.min(offset),
            Target::X(_) => bound,
        };
        let from = match self.walk_ascii(from, bound, stop, target) {
            Ok(at) => return at,
            Err(at) => at,
        };

        // Segment the rest a window at a time, measuring the units that
        // start in it. Each window copies a little more than the longest
        // unit past its end, so a unit that starts in it is segmented
        // whole, and is cut into pieces however far the window reaches.
        let reach = |at: usize| match target {
            Target::Offset(offset) | Target::AtOrAfter(offset) => offset.saturating_sub(at) + 64,
            Target::X(_) => 1024,
        };
        let mut at = from;
        loop {
            let end = self
                .text
                .clip_offset((at.offset + reach(at.offset)).min(bound), Bias::Right);
            let ahead = self
                .text
                .clip_offset((end + MAX_CLUSTER_BYTES + 4).min(self.end), Bias::Right);
            let text = self.text.slice_to_string(at.offset..ahead);
            self.count(text.len());
            for (cluster, piece) in Clusters::new(&text) {
                if at.offset >= end {
                    break;
                }
                match target {
                    Target::Offset(offset) if at.offset + cluster.len() > offset => return at,
                    Target::AtOrAfter(offset) if at.offset >= offset => return at,
                    _ => {}
                }
                let (columns, next_x) = self.measure_cluster(cluster, piece, at, measure);
                if let Target::X(x) = target
                    && next_x > x
                {
                    return at;
                }
                at = Position {
                    offset: at.offset + cluster.len(),
                    column: at.column + columns,
                    x: next_x,
                    cluster: false,
                };
            }
            if end == bound {
                return at;
            }
        }
    }

    /// Walks printable ASCII from `from` towards `stop` until `target`. Its
    /// chars are each a cluster of their own, so it needn't be segmented, and
    /// when they're all one width, it's walked in one step. Short of the
    /// target, it's `Err` with the last cluster boundary it's sure of: before
    /// the first char that isn't printable ASCII, or, if the char at `stop`
    /// (short of `bound`) extends the last one, as a combining mark does,
    /// before that one. So too before a char that crosses an x target but
    /// is extended, since its cluster may be narrower than it.
    fn walk_ascii(
        &self,
        from: Position,
        bound: usize,
        stop: usize,
        target: Target,
    ) -> Result<Position, Position> {
        let position = |chars: usize, x: f64| Position {
            offset: from.offset + chars,
            column: from.column + chars,
            x,
            cluster: false,
        };
        // The chars walked, the x after them, and the x before the last.
        let (chars, x, last_x) = if let Some(advance) = self.advance {
            if !self.is_printable_ascii(from.offset..stop) {
                return Err(from);
            }
            let len = stop - from.offset;
            let x_at = |chars: usize| from.x + chars as f64 * advance;
            let chars = match target {
                Target::Offset(_) | Target::AtOrAfter(_) => len,
                Target::X(x) => {
                    // The quotient is exact while x is (see the module docs),
                    // but correct it against `x_at` in case it isn't.
                    let mut chars = ((x - from.x) / advance).floor().clamp(0., len as f64) as usize;
                    while chars < len && x_at(chars + 1) <= x {
                        chars += 1;
                    }
                    while chars > 0 && x_at(chars) > x {
                        chars -= 1;
                    }
                    chars
                }
            };
            (chars, x_at(chars), x_at(chars.saturating_sub(1)))
        } else {
            let (mut chars, mut x, mut last_x) = (0usize, from.x, from.x);
            for chunk in self.text.chunks_in_range(from.offset..stop) {
                for &byte in chunk.as_bytes() {
                    if !matches!(byte, b' '..=b'~') {
                        // It may extend the char before, which is measured
                        // again with it.
                        let sure = chars.saturating_sub(1);
                        self.count(sure);
                        return Err(position(sure, last_x));
                    }
                    let next_x = x + self.ascii[usize::from(byte)];
                    if let Target::X(target_x) = target
                        && next_x > target_x
                    {
                        self.count(chars + 1);
                        // A combining mark after it may make its cluster
                        // narrower than it, so segment from its start.
                        let after = from.offset + chars + 1;
                        if after < stop && self.extends_cluster(after) {
                            return Err(position(chars, x));
                        }
                        return Ok(position(chars, x));
                    }
                    (chars, x, last_x) = (chars + 1, next_x, x);
                }
            }
            self.count(chars);
            (chars, x, last_x)
        };
        if chars > 0 && from.offset + chars == stop && stop < bound && self.extends_cluster(stop) {
            return Err(position(chars - 1, last_x));
        }
        Ok(position(chars, x))
    }

    /// The display columns of `cluster`, a unit which starts at `at`, and
    /// the x after it. A `piece` of a longer cluster is measured char by
    /// char.
    fn measure_cluster(
        &self,
        cluster: &str,
        piece: bool,
        at: Position,
        measure: &mut dyn CharWidth,
    ) -> (usize, f64) {
        let mut chars = cluster.chars();
        let ch = chars.next().expect("clusters aren't empty");
        if chars.next().is_some() {
            let width = if piece {
                cluster
                    .chars()
                    .map(|ch| f64::from(measure.char_width(ch)))
                    .sum()
            } else {
                f64::from(measure.cluster_width(cluster))
            };
            return (cluster.chars().count(), at.x + width);
        }
        if ch == '\t' {
            // A tab is drawn as spaces, so add their widths one by one, as
            // measuring the expanded row does.
            let columns = self.tab_size - at.column % self.tab_size;
            let space = self.ascii[usize::from(b' ')];
            (columns, (0..columns).fold(at.x, |x, _| x + space))
        } else if ch.is_ascii() {
            (1, at.x + self.ascii[ch as usize])
        } else {
            (1, at.x + f64::from(measure.char_width(ch)))
        }
    }

    /// Whether the char at `offset`, after a printable ASCII char, continues
    /// that char's cluster, as a combining mark does.
    fn extends_cluster(&self, offset: usize) -> bool {
        let (Some(before), Some(after)) = (
            self.text.char_at(offset - 1),
            self.text.char_at(offset).filter(|_| offset < self.end),
        ) else {
            return false;
        };
        let pair: String = [before, after].into_iter().collect();
        pair.graphemes(true).nth(1).is_none()
    }

    fn is_printable_ascii(&self, range: Range<usize>) -> bool {
        self.count(range.len());
        self.text
            .chunks_in_range(range)
            .all(|chunk| chunk.bytes().all(|byte| matches!(byte, b' '..=b'~')))
    }

    /// Counts bytes looked at, in tests.
    fn count(&self, bytes: usize) {
        #[cfg(test)]
        self.scanned.set(self.scanned.get() + bytes);
        #[cfg(not(test))]
        let _ = bytes;
    }
}

/// The units of [`RowIndex`] in text that starts on a unit boundary:
/// grapheme clusters, except that one longer than [`MAX_CLUSTER_BYTES`] is
/// cut at a char boundary that long, and the text after the cut segmented
/// afresh. Only the stretches around chars that may join others are
/// segmented, and only a little more than that limit at a time.
struct Clusters<'a> {
    rest: &'a str,
    /// What's left to segment of the stretch the last unit came from.
    stretch: &'a str,
}

impl<'a> Clusters<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            rest: text,
            stretch: "",
        }
    }
}

impl<'a> Iterator for Clusters<'a> {
    /// A unit, and whether it's a piece of a longer cluster.
    type Item = (&'a str, bool);

    fn next(&mut self) -> Option<(&'a str, bool)> {
        if self.stretch.is_empty() {
            // Up to the next pair of chars that each are a cluster of their
            // own.
            let mut chars = self.rest.char_indices();
            let (_, first) = chars.next()?;
            let mut end = self.rest.len();
            let mut own = is_own_cluster(first);
            for (ix, ch) in chars {
                let next_own = is_own_cluster(ch);
                if own && next_own {
                    end = ix;
                    break;
                }
                own = next_own;
            }
            let (stretch, rest) = self.rest.split_at(end);
            self.rest = rest;
            if stretch.len() == first.len_utf8() {
                return Some((stretch, false));
            }
            self.stretch = stretch;
        }
        // Whether there's a boundary between two chars depends on them and
        // the chars before, so a cluster that ends before the text segmented
        // does ends there in the whole stretch too. One that doesn't is
        // longer than the limit.
        let segmented = &self.stretch[..self.stretch.ceil_char_boundary(MAX_CLUSTER_BYTES + 1)];
        let cluster = segmented.graphemes(true).next().expect("not empty");
        let piece = cluster.len() > MAX_CLUSTER_BYTES;
        let len = if piece {
            cluster.floor_char_boundary(MAX_CLUSTER_BYTES)
        } else {
            cluster.len()
        };
        let (unit, stretch) = self.stretch.split_at(len);
        self.stretch = stretch;
        Some((unit, piece))
    }
}

/// Whether `ch` is in a range of chars that never join a neighbour into a
/// grapheme cluster: no combining marks, joiners, prepended or Hangul jamo
/// or regional indicators. Between two such chars there's always a cluster
/// boundary, so [`Clusters`] needn't segment there. Conservative: it's
/// false for some chars that never join too.
fn is_own_cluster(ch: char) -> bool {
    matches!(ch,
        '\0'..='\u{2ff}'
        | '\u{370}'..='\u{482}'
        | '\u{3040}'..='\u{3098}'
        | '\u{30a0}'..='\u{30ff}'
        | '\u{3400}'..='\u{4dbf}'
        | '\u{4e00}'..='\u{9fff}'
        | '\u{f900}'..='\u{faff}'
        | '\u{ff01}'..='\u{ff9d}'
        | '\u{1f300}'..='\u{1f3fa}'
        | '\u{1f400}'..='\u{1faff}'
        | '\u{20000}'..='\u{3ffff}')
    // A CR joins a following LF; rows have no LF, but be exact anyway.
    && ch != '\r'
}

/// Which text a [`LongRows`] holds indexes of: a version of a buffer, or a
/// snapshot of a read-only source (see [`crate::DisplayMap::read_only`]),
/// told apart by its row count and length. Get one from
/// [`crate::DisplayMap::source_key`], or from a [`BufferSnapshot`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SourceKey(Key);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Key {
    Buffer(BufferId, Version),
    ReadOnly { id: SourceId, rows: u32, len: usize },
}

/// A read-only source, unique in the process.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceId(u64);

impl SourceId {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl SourceKey {
    /// `text`, a snapshot of the read-only source `id`, which grows from one
    /// snapshot to the next: a later one has more rows, or the same rows and
    /// a longer last row.
    pub(crate) fn read_only(id: SourceId, text: &dyn TextSource) -> Self {
        Self(Key::ReadOnly {
            id,
            rows: text.row_count(),
            len: text.len(),
        })
    }
}

impl From<&BufferSnapshot> for SourceKey {
    fn from(snapshot: &BufferSnapshot) -> Self {
        Self(Key::Buffer(snapshot.id(), snapshot.version()))
    }
}

/// The [`RowIndex`]es of long rows of one text, kept from frame to frame.
/// Another text (another [`SourceKey`]) drops them all; so does
/// [`LongRows::clear`], which a font or tab size change must call. Rows the
/// current or last frame used are never dropped, however many there are.
#[derive(Default)]
pub struct LongRows {
    key: Option<SourceKey>,
    /// Each index, with the frame that last used it.
    rows: HashMap<u32, (RowIndex, u64)>,
    frame: u64,
}

impl LongRows {
    /// Starts a frame, which [`LongRows::index`] counts its rows as used by.
    pub fn start_frame(&mut self) {
        self.frame += 1;
    }

    /// The index of buffer row `row` of `text`, which `key` names.
    pub fn index(
        &mut self,
        key: SourceKey,
        text: &dyn TextSource,
        row: u32,
        tab_size: usize,
        measure: &mut dyn CharWidth,
    ) -> &mut RowIndex {
        let key = Some(key);
        if self.key != key {
            self.rows.clear();
            self.key = key;
        }
        if self.rows.len() >= MAX_CACHED_ROWS && !self.rows.contains_key(&row) {
            let frame = self.frame;
            self.rows.retain(|_, (_, used)| *used + 1 >= frame);
        }
        let (index, used) = self.rows.entry(row).or_insert_with(|| {
            let start = text.row_start(row);
            let index = RowIndex::new(
                text,
                start..start + text.row_len(row) as usize,
                tab_size,
                measure,
            );
            (index, 0)
        });
        *used = self.frame;
        index
    }

    /// The index of buffer row `row` of the version last indexed, if it's
    /// still held.
    pub fn get(&mut self, row: u32) -> Option<&mut RowIndex> {
        self.rows.get_mut(&row).map(|(index, _)| index)
    }

    pub fn clear(&mut self) {
        self.key = None;
        self.rows.clear();
    }
}

#[cfg(test)]
mod tests {
    use fe_text::Text;
    use proptest::prelude::*;

    use super::*;
    use crate::{ExpandedRow, Monospace};

    /// Every char `0.` wide, like a monospace font at a fractional size.
    struct Fixed(f32);

    impl CharWidth for Fixed {
        fn char_width(&mut self, _: char) -> f32 {
            self.0
        }
    }

    /// ASCII 7.2 wide except a narrow `i`, CJK and emoji twice that and
    /// combining marks nothing. Counts its calls.
    #[derive(Default)]
    struct Wide {
        calls: usize,
    }

    impl CharWidth for Wide {
        fn char_width(&mut self, ch: char) -> f32 {
            self.calls += 1;
            match ch {
                'i' => 3.3,
                '\u{300}'..='\u{36f}' => 0.,
                '\u{2e80}'.. => 14.4,
                _ => 7.2,
            }
        }
    }

    /// Counts calls to [`Monospace`].
    #[derive(Default)]
    struct Counting {
        calls: usize,
    }

    impl CharWidth for Counting {
        fn char_width(&mut self, ch: char) -> f32 {
            self.calls += 1;
            Monospace.char_width(ch)
        }
    }

    /// `row` as the middle row of a text, and its range there.
    fn middle_row(row: &str) -> (Text, Range<usize>) {
        let text = Text::new(&format!("before\n{row}\nafter"));
        (text, 7..7 + row.len())
    }

    /// Rows of runs of pieces, crossing a few checkpoints.
    fn long_row() -> impl Strategy<Value = String> {
        let piece =
            prop::sample::select(vec!["a", "i", "\t", "é", "中", "😀", "e\u{301}", "ab\tc"]);
        prop::collection::vec((piece, 1usize..1500), 0..10).prop_map(|runs| {
            runs.into_iter()
                .map(|(piece, count)| piece.repeat(count))
                .collect()
        })
    }

    fn check_matches_prefix(
        row: &str,
        tab_size: usize,
        picks: &[prop::sample::Index],
        measure: &mut dyn CharWidth,
    ) -> Result<(), TestCaseError> {
        let (text, range) = middle_row(row);
        let expanded = ExpandedRow::new(row, tab_size);
        // (offset, column, x) of every cluster boundary, summing widths over
        // the expanded prefix, with each multi-char cluster measured whole.
        let mut boundaries = vec![(range.start, 0, 0.)];
        let (mut display, mut column, mut x) = (0, 0, 0.);
        for (ix, cluster) in row.grapheme_indices(true) {
            let end = if cluster == "\t" {
                expanded.to_display(ix + 1)
            } else {
                display + cluster.len()
            };
            let shown = &expanded.text()[display..end];
            if cluster.chars().nth(1).is_some() {
                column += cluster.chars().count();
                x += f64::from(measure.cluster_width(shown));
            } else {
                for ch in shown.chars() {
                    column += 1;
                    x += f64::from(measure.char_width(ch));
                }
            }
            display = end;
            boundaries.push((range.start + ix + cluster.len(), column, x));
        }

        let mut index = RowIndex::new(&text, range.clone(), tab_size, measure);
        for pick in picks {
            let ix = pick.index(boundaries.len());
            let (offset, column, want_x) = boundaries[ix];
            let x = index.x_for_offset(offset, measure);
            prop_assert_eq!(x, want_x, "x of {}", offset);
            prop_assert_eq!(index.column_for_offset(offset, measure), column);
            if let Some(&(next, ..)) = boundaries.get(ix + 1) {
                prop_assert!(index.x_for_offset(next, measure) >= x);
                // Offsets inside a cluster clamp to its start.
                for inside in (offset + 1..next).filter(|&at| text.is_char_boundary(at)) {
                    prop_assert_eq!(index.x_for_offset(inside, measure), x);
                }
            }
            // The last boundary at or before x, at and around boundaries.
            for x in [x.next_down(), x, x + 0.25] {
                let found = index.offset_for_x(x, measure);
                let at = boundaries.partition_point(|b| b.0 < found);
                prop_assert_eq!(boundaries[at].0, found, "{} is a cluster boundary", found);
                prop_assert!(at == 0 || index.x_for_offset(found, measure) <= x);
                if let Some(&(next, ..)) = boundaries.get(at + 1) {
                    prop_assert!(
                        index.x_for_offset(next, measure) > x,
                        "{found} is last at {x}"
                    );
                }
            }
        }
        let width = index.width(measure);
        prop_assert_eq!(width, boundaries.last().unwrap().2);
        prop_assert!(
            index
                .checkpoints
                .is_sorted_by(|a, b| a.offset < b.offset && a.x <= b.x)
        );
        for checkpoint in &index.checkpoints {
            let at = boundaries.partition_point(|b| b.0 < checkpoint.offset);
            prop_assert_eq!(checkpoint.column, boundaries[at].1);
            prop_assert_eq!(checkpoint.x, boundaries[at].2);
        }
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        #[test]
        fn index_matches_measuring_the_prefix(
            row in long_row(),
            tab_size in 1usize..9,
            picks in prop::collection::vec(any::<prop::sample::Index>(), 1..48),
        ) {
            check_matches_prefix(&row, tab_size, &picks, &mut Monospace)?;
            check_matches_prefix(&row, tab_size, &picks, &mut Fixed(7.2))?;
            check_matches_prefix(&row, tab_size, &picks, &mut Wide::default())?;
        }

        #[test]
        fn expanded_slice_matches_whole_row(
            row in "[a\t é中]{0,40}",
            split in any::<prop::sample::Index>(),
            tab_size in 1usize..9,
        ) {
            let whole = ExpandedRow::new(&row, tab_size);
            let boundaries: Vec<usize> = row.char_indices().map(|(ix, _)| ix).chain([row.len()]).collect();
            let split = boundaries[split.index(boundaries.len())];
            let display_split = whole.to_display(split);
            let column = whole.char_column(display_split);
            let part = ExpandedRow::with_start_column(&row[split..], column, tab_size);
            prop_assert_eq!(part.text(), &whole.text()[display_split..]);
            for &offset in boundaries.iter().filter(|&&offset| offset >= split) {
                prop_assert_eq!(
                    part.to_display(offset - split),
                    whole.to_display(offset) - display_split
                );
            }
        }
    }

    #[test]
    fn slice_is_bounded_and_covers_the_view() {
        const LEN: usize = 10 << 20;
        let (view, margin) = (800., 200.);
        let rows: [(&str, &mut dyn CharWidth); 2] = [
            ("abcdefgh", &mut Fixed(7.2)),
            ("ab\tcé中 😀e\u{301}xyz", &mut Wide::default()),
        ];
        for (pattern, measure) in rows {
            let row = pattern.repeat(LEN / pattern.len());
            let (text, range) = middle_row(&row);
            let mut index = RowIndex::new(&text, range.clone(), 4, measure);
            let width = index.width(measure);
            for start in [
                -50.,
                0.,
                1234.5,
                width / 2.,
                width - view,
                width - 10.,
                width + 50.,
            ] {
                let slice = index.slice(start..start + view, margin, measure);
                let RowSlice {
                    range: bytes,
                    start_x,
                    start_column,
                } = slice.clone();
                assert!(text.is_char_boundary(bytes.start) && text.is_char_boundary(bytes.end));
                assert!(range.start <= bytes.start && bytes.end <= range.end);
                assert_eq!(start_x, index.x_for_offset(bytes.start, measure));
                assert_eq!(start_column, index.column_for_offset(bytes.start, measure));
                let context = format!("{pattern:?} at {start}: {slice:?}");
                assert!(start_x <= (start - margin).max(0.), "{context}");
                let end_x = index.x_for_offset(bytes.end, measure);
                assert!(
                    end_x >= start + view + margin || bytes.end == range.end,
                    "{context}"
                );
                // At most 3 bytes per char 7.2 wide, plus a char at each end.
                assert!(
                    bytes.len() <= ((view + margin * 2.) / 2.4) as usize + 8,
                    "{context}"
                );
            }
        }
    }

    #[test]
    fn far_along_a_row_x_stays_exact() {
        let row = "abcdefgh".repeat(10 << 17);
        let (text, range) = middle_row(&row);
        let measure = &mut Fixed(7.2);
        let mut index = RowIndex::new(&text, range.clone(), 4, measure);
        for chars in [3, 100, 6_148_089, row.len()] {
            let x = index.x_for_offset(range.start + chars, measure);
            assert_eq!(x, chars as f64 * f64::from(7.2f32), "{chars} chars");
            assert_eq!(index.offset_for_x(x, measure), range.start + chars);
            assert_eq!(
                index.offset_for_x(x.next_down(), measure),
                range.start + chars - 1
            );
        }
    }

    #[test]
    fn index_builds_lazily() {
        let row = "é中".repeat(2 << 20);
        let (text, range) = middle_row(&row);
        let measure = &mut Wide::default();
        let mut index = RowIndex::new(&text, range.clone(), 4, measure);
        index.x_for_offset(range.start + 100, measure);
        index.offset_for_x(15000., measure);
        index.slice(5000.0..13000., 2000., measure);
        let checkpoints = index.checkpoints.len();
        assert!(checkpoints <= 3, "{checkpoints} checkpoints");
        assert!(measure.calls <= 128 + 3 * CHUNK, "{} calls", measure.calls);

        index.width(measure);
        assert_eq!(index.checkpoints.len(), row.len().div_ceil(CHUNK - 1) + 1);
        assert!(measure.calls > row.chars().count());

        // A fresh index looks at only the start of the row for the first view.
        let rows: [(&str, &mut dyn CharWidth); 2] = [
            ("abcdefgh", &mut Counting::default()),
            ("é中", &mut Wide::default()),
        ];
        for (pattern, measure) in rows {
            let row = pattern.repeat((10 << 20) / pattern.len());
            let (text, range) = middle_row(&row);
            let mut index = RowIndex::new(&text, range, 4, measure);
            index.slice(0.0..800., 200., measure);
            let scanned = index.scanned.get();
            assert!(scanned <= 4 * CHUNK, "{pattern:?}: {scanned} bytes scanned");
        }
    }

    #[test]
    fn monospace_ascii_is_not_measured_per_char() {
        let row = "x = [1, 2, 3];".repeat(1 << 20);
        let (text, range) = middle_row(&row);
        let measure = &mut Counting::default();
        let mut index = RowIndex::new(&text, range.clone(), 4, measure);
        assert_eq!(index.width(measure), row.len() as f64);
        assert_eq!(
            index.offset_for_x(1e6 + 0.5, measure),
            range.start + 1_000_000
        );
        assert_eq!(measure.calls, 128);
    }

    #[test]
    fn long_rows_follow_the_snapshot() {
        let mut buffer = fe_buffer::Buffer::new("ab\n\tcd");
        let mut rows = LongRows::default();
        let measure = &mut Monospace;
        assert_eq!(
            rows.index(
                (&buffer.snapshot()).into(),
                buffer.snapshot().text(),
                1,
                4,
                measure
            )
            .range(),
            3..6
        );
        assert_eq!(
            rows.index(
                (&buffer.snapshot()).into(),
                buffer.snapshot().text(),
                1,
                4,
                measure
            )
            .width(measure),
            6.
        );
        assert_eq!(rows.rows.len(), 1);

        buffer.edit([(0..0, "xyz")], fe_buffer::Origin::User);
        let index = rows.index(
            (&buffer.snapshot()).into(),
            buffer.snapshot().text(),
            1,
            4,
            measure,
        );
        assert_eq!((index.range(), index.width(measure)), (6..9, 6.));
        rows.index(
            (&buffer.snapshot()).into(),
            buffer.snapshot().text(),
            0,
            4,
            measure,
        );
        assert_eq!(rows.rows.len(), 2);
    }
}
