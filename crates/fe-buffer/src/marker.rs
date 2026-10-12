use std::{cmp::Ordering, ops::Range};

use fe_text::Bias;
use sum_tree::{ContextLessSummary, Cursor, Dimension, Item, SeekTarget, SumTree};

use crate::{
    BufferId, BufferSnapshot, Edit, Version,
    edit::{Mapping, map_offset},
};

/// Identifies a marker within its [`MarkerSet`]. Never reused by the set.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MarkerId(u64);

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MarkerError {
    /// The snapshot's edit log was compacted past the set's version, so the
    /// edits since can't be replayed; rebuild the set from the snapshot.
    #[error("the edit log was compacted past the marker set's version")]
    Compacted,
    /// The snapshot is of another buffer.
    #[error("the snapshot is of another buffer")]
    OtherBuffer,
    /// The snapshot is older than the set's version.
    #[error("the snapshot is older than the marker set")]
    Older,
}

/// Ranges of one buffer's text, each with a value, that follow its edits.
///
/// Where [`Anchor`](crate::Anchor)s remap every position on every resolve, a
/// marker set keeps its ranges in a [`SumTree`] sorted by start, each start
/// stored as the gap from the previous marker's and each end as a length.
/// [`MarkerSet::sync`] catches up with an edit by changing the one gap after
/// it and remapping only the markers that touch it, and
/// [`MarkerSet::iter_in`] skips every subtree that ends before the range
/// asked for: both stay cheap with tens of thousands of markers.
///
/// A marker's ends move as [`BufferSnapshot::map_offset`] moves offsets,
/// under the set's start and end [`Bias`]; an end that would fall before its
/// start stays at the start. Markers are ordered by start, then by insertion.
#[derive(Clone)]
pub struct MarkerSet<T: Clone> {
    tree: SumTree<Entry<T>>,
    buffer: BufferId,
    version: Version,
    /// The text's length at `version`, to clip inserted ranges to.
    text_len: usize,
    start_bias: Bias,
    end_bias: Bias,
    next_id: u64,
}

impl<T: Clone> MarkerSet<T> {
    /// An empty set for `snapshot`'s buffer, at its version. Text inserted
    /// right at a marker's end stays outside it.
    pub fn new(snapshot: &BufferSnapshot) -> Self {
        Self::with_biases(snapshot, Bias::Right, Bias::Left)
    }

    /// An empty set whose markers' starts and ends take these biases.
    pub fn with_biases(snapshot: &BufferSnapshot, start: Bias, end: Bias) -> Self {
        Self {
            tree: SumTree::new(()),
            buffer: snapshot.id(),
            version: snapshot.version(),
            text_len: snapshot.len(),
            start_bias: start,
            end_bias: end,
            next_id: 0,
        }
    }

    /// The buffer version the markers' ranges are bytes of.
    pub fn version(&self) -> Version {
        self.version
    }

    pub fn len(&self) -> usize {
        self.tree.summary().count
    }

    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    /// Removes every marker. Ids aren't reused after.
    pub fn clear(&mut self) {
        self.tree = SumTree::new(());
    }

    /// Adds a marker over `range`, bytes of the text at
    /// [`version`](Self::version), clipped to it. An inverted range is stored
    /// empty at its start. O(log n).
    pub fn insert(&mut self, range: Range<usize>, value: T) -> MarkerId {
        let marker = self.new_marker(range, value);
        let id = marker.id;
        self.tree = merge(&self.tree, vec![marker]);
        id
    }

    /// Removes the markers starting in `range` (half-open, so an empty range
    /// removes none) and adds `items` as [`insert`](Self::insert) would,
    /// returning their ids in order. O((r + k + 1) log n) for r items and k
    /// removed markers.
    pub fn splice(
        &mut self,
        range: Range<usize>,
        items: impl IntoIterator<Item = (Range<usize>, T)>,
    ) -> Vec<MarkerId> {
        let mut kept = SumTree::new(());
        {
            let mut cursor = self.tree.cursor::<Position>(());
            let prefix = cursor.slice(&StartAtLeast(range.start), sum_tree::Bias::Left);
            kept.append(prefix, ());
            cursor.seek_forward(
                &StartAtLeast(range.end.max(range.start)),
                sum_tree::Bias::Left,
            );
            if let Some(entry) = cursor.item() {
                let first_start = cursor.start().start + entry.gap;
                append_from(&mut kept, cursor.suffix(), first_start);
            }
        }
        let mut markers: Vec<Marker<T>> = (items.into_iter())
            .map(|(range, value)| self.new_marker(range, value))
            .collect();
        let ids = markers.iter().map(|marker| marker.id).collect();
        markers.sort_by_key(|marker| (marker.range.start, marker.id));
        self.tree = merge(&kept, markers);
        ids
    }

    /// Keeps only the markers `keep` returns true for. O(n).
    pub fn retain(&mut self, mut keep: impl FnMut(MarkerId, Range<usize>, &T) -> bool) {
        let mut kept = SumTree::new(());
        let mut last_start = 0;
        let mut start = 0;
        for entry in self.tree.iter() {
            start += entry.gap;
            if keep(entry.id, start..start + entry.len, &entry.value) {
                kept.push(
                    Entry {
                        gap: start - last_start,
                        ..entry.clone()
                    },
                    (),
                );
                last_start = start;
            }
        }
        self.tree = kept;
    }

    /// Removes the marker `id` and returns its value, if it's in the set.
    /// O(n): it's found by scanning.
    pub fn remove(&mut self, id: MarkerId) -> Option<T> {
        let ordinal = self.tree.iter().position(|entry| entry.id == id)?;
        let (kept, removed) = {
            let mut cursor = self.tree.cursor::<Position>(());
            let mut kept = cursor.slice(&Ordinal(ordinal), sum_tree::Bias::Right);
            let removed = cursor.item()?.value.clone();
            cursor.next();
            if let Some(entry) = cursor.item() {
                let first_start = cursor.start().start + entry.gap;
                append_from(&mut kept, cursor.suffix(), first_start);
            }
            (kept, removed)
        };
        self.tree = kept;
        Some(removed)
    }

    /// Every marker overlapping `range`, by start then insertion, an empty
    /// marker at either end of it included, and an empty `range` taking the
    /// markers touching it. O(log n) per marker returned.
    pub fn iter_in(
        &self,
        range: Range<usize>,
    ) -> impl Iterator<Item = (MarkerId, Range<usize>, &T)> + '_ {
        let range = range.start..range.end.max(range.start);
        Touching::new(&self.tree, range.clone())
            .filter(move |hit| overlaps(&hit.range, &range))
            .map(|hit| (hit.entry.id, hit.range, &hit.entry.value))
    }

    /// Moves the markers through the edits from [`version`](Self::version)
    /// to `snapshot`'s, then takes its version. Each edit costs
    /// O((k + 1) log n) for the k markers touching it; the markers after it
    /// shift with one gap change.
    ///
    /// The edits are those [`BufferSnapshot::map_offset`] maps through, so a
    /// batch undone before the sync is skipped as it skips it; a set synced
    /// in between moved its markers through both.
    ///
    /// Leaves the set unchanged on an error.
    pub fn sync(&mut self, snapshot: &BufferSnapshot) -> Result<(), MarkerError> {
        if snapshot.id() != self.buffer {
            return Err(MarkerError::OtherBuffer);
        }
        if snapshot.version() < self.version {
            return Err(MarkerError::Older);
        }
        if self.version < snapshot.oldest_version() {
            return Err(MarkerError::Compacted);
        }
        if !self.tree.is_empty() {
            for edits in snapshot
                .log
                .mapping_batches(self.version, snapshot.version())
            {
                self.apply(edits);
            }
        }
        self.version = snapshot.version();
        self.text_len = snapshot.len();
        Ok(())
    }

    /// Moves the markers through one batch of edits.
    fn apply(&mut self, edits: &[Edit]) {
        // The markers touching an edit, by position in the tree, mapped.
        let mut touched: Vec<(usize, Marker<T>)> = Vec::new();
        // Per edit, the position of the first marker starting after it.
        let mut cuts = Vec::with_capacity(edits.len());
        for edit in edits {
            for hit in Touching::new(&self.tree, edit.old.clone()) {
                if touched
                    .last()
                    .is_some_and(|(ordinal, _)| *ordinal >= hit.ordinal)
                {
                    // Touches the previous edit too.
                    continue;
                }
                let start = map_offset(edits, hit.range.start, Mapping::from(self.start_bias));
                let end = map_offset(edits, hit.range.end, Mapping::from(self.end_bias));
                touched.push((
                    hit.ordinal,
                    Marker {
                        range: start..end.max(start),
                        id: hit.entry.id,
                        value: hit.entry.value.clone(),
                    },
                ));
            }
            let mut cursor = self.tree.cursor::<Position>(());
            cursor.seek(&StartAtLeast(edit.old.end + 1), sum_tree::Bias::Left);
            cuts.push(cursor.start().count);
        }
        if touched.is_empty() && cuts.iter().all(|&cut| cut == self.len()) {
            return;
        }

        // Every marker but the touched ones, shifted past the edits before
        // them: one gap change per run.
        let mut kept = SumTree::new(());
        {
            let mut cursor = self.tree.cursor::<Position>(());
            cursor.seek(&Ordinal(0), sum_tree::Bias::Right);
            let mut breaks: Vec<usize> = (cuts.iter().copied())
                .chain(touched.iter().map(|(ordinal, _)| *ordinal))
                .chain([self.len()])
                .collect();
            breaks.sort_unstable();
            breaks.dedup();
            let mut touched_ordinals = touched.iter().map(|(ordinal, _)| *ordinal).peekable();
            let mut ordinal = 0;
            let mut passed = 0;
            for brk in breaks {
                if brk > ordinal {
                    while passed < cuts.len() && cuts[passed] <= ordinal {
                        passed += 1;
                    }
                    let base = cursor.start().start;
                    let run = cursor.slice(&Ordinal(brk), sum_tree::Bias::Right);
                    let old_start = base + run.first().map_or(0, |entry| entry.gap);
                    let new_start = match passed.checked_sub(1) {
                        Some(ix) => old_start - edits[ix].old.end + edits[ix].new.end,
                        None => old_start,
                    };
                    append_from(&mut kept, run, new_start);
                    ordinal = brk;
                }
                if touched_ordinals.next_if_eq(&brk).is_some() {
                    cursor.next();
                    ordinal += 1;
                }
            }
        }

        let mut markers: Vec<Marker<T>> = touched.into_iter().map(|(_, marker)| marker).collect();
        markers.sort_by_key(|marker| (marker.range.start, marker.id));
        self.tree = merge(&kept, markers);
    }

    fn new_marker(&mut self, range: Range<usize>, value: T) -> Marker<T> {
        let start = range.start.min(self.text_len);
        let end = range.end.min(self.text_len).max(start);
        let id = MarkerId(self.next_id);
        self.next_id += 1;
        Marker {
            range: start..end,
            id,
            value,
        }
    }
}

impl<T: Clone + std::fmt::Debug> std::fmt::Debug for MarkerSet<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MarkerSet")
            .field("version", &self.version)
            .field(
                "markers",
                &self.iter_in(0..self.text_len).collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Whether `item` overlaps `range`, counting an empty one touching the other.
fn overlaps(item: &Range<usize>, range: &Range<usize>) -> bool {
    if item.is_empty() || range.is_empty() {
        item.start <= range.end && range.start <= item.end
    } else {
        item.start < range.end && range.start < item.end
    }
}

/// A marker with its absolute range, out of the tree.
struct Marker<T> {
    range: Range<usize>,
    id: MarkerId,
    value: T,
}

#[derive(Clone)]
struct Entry<T> {
    /// From the previous marker's start, or from 0 for the first.
    gap: usize,
    len: usize,
    id: MarkerId,
    value: T,
}

#[derive(Clone, Debug, Default)]
struct Summary {
    count: usize,
    /// The last marker's start, from the subtree's base: the start of the
    /// marker before it.
    gaps: usize,
    /// The greatest end, from the subtree's base.
    max_end: usize,
    last_id: Option<MarkerId>,
}

impl<T: Clone> Item for Entry<T> {
    type Summary = Summary;

    fn summary(&self, (): ()) -> Summary {
        Summary {
            count: 1,
            gaps: self.gap,
            max_end: self.gap + self.len,
            last_id: Some(self.id),
        }
    }
}

impl ContextLessSummary for Summary {
    fn zero() -> Self {
        Self::default()
    }

    fn add_summary(&mut self, summary: &Self) {
        if summary.count == 0 {
            return;
        }
        self.max_end = self.max_end.max(self.gaps + summary.max_end);
        self.gaps += summary.gaps;
        self.count += summary.count;
        self.last_id = summary.last_id;
    }
}

/// How far into the tree a cursor is: past `count` markers, the last of
/// which starts at `start` and is `last_id`.
#[derive(Clone, Debug, Default)]
struct Position {
    count: usize,
    start: usize,
    last_id: Option<MarkerId>,
}

impl Dimension<'_, Summary> for Position {
    fn zero((): ()) -> Self {
        Self::default()
    }

    fn add_summary(&mut self, summary: &Summary, (): ()) {
        if summary.count == 0 {
            return;
        }
        self.count += summary.count;
        self.start += summary.gaps;
        self.last_id = summary.last_id;
    }
}

/// Seeks past the first `n` markers.
struct Ordinal(usize);

impl SeekTarget<'_, Summary, Position> for Ordinal {
    fn cmp(&self, cursor_location: &Position, (): ()) -> Ordering {
        self.0.cmp(&cursor_location.count)
    }
}

/// Seeks (with [`sum_tree::Bias::Left`]) to the first marker starting at or
/// after an offset.
struct StartAtLeast(usize);

impl SeekTarget<'_, Summary, Position> for StartAtLeast {
    fn cmp(&self, cursor_location: &Position, (): ()) -> Ordering {
        if cursor_location.count == 0 || cursor_location.start < self.0 {
            Ordering::Greater
        } else {
            Ordering::Less
        }
    }
}

/// Seeks (with [`sum_tree::Bias::Left`]) to where a marker with this start
/// and id goes.
struct Key(usize, MarkerId);

impl SeekTarget<'_, Summary, Position> for Key {
    fn cmp(&self, cursor_location: &Position, (): ()) -> Ordering {
        match cursor_location.last_id {
            Some(id) => (self.0, self.1).cmp(&(cursor_location.start, id)),
            None => Ordering::Greater,
        }
    }
}

/// Appends `run`, a slice of some tree, to `tree`, its first marker starting
/// at `first_start`.
fn append_from<T: Clone>(
    tree: &mut SumTree<Entry<T>>,
    mut run: SumTree<Entry<T>>,
    first_start: usize,
) {
    let last_start = tree.summary().gaps;
    run.update_first(|entry| entry.gap = first_start - last_start, ());
    tree.append(run, ());
}

/// `tree` with `markers`, sorted by start then id, merged in.
fn merge<T: Clone>(tree: &SumTree<Entry<T>>, markers: Vec<Marker<T>>) -> SumTree<Entry<T>> {
    let mut merged = SumTree::new(());
    let mut cursor = tree.cursor::<Position>(());
    for marker in markers {
        let base = cursor.start().start;
        let run = cursor.slice(&Key(marker.range.start, marker.id), sum_tree::Bias::Left);
        if let Some(first) = run.first() {
            let first_start = base + first.gap;
            append_from(&mut merged, run, first_start);
        }
        let gap = marker.range.start - merged.summary().gaps;
        merged.push(
            Entry {
                gap,
                len: marker.range.len(),
                id: marker.id,
                value: marker.value,
            },
            (),
        );
    }
    if !cursor.did_seek() {
        return tree.clone();
    }
    if let Some(entry) = cursor.item() {
        let first_start = cursor.start().start + entry.gap;
        append_from(&mut merged, cursor.suffix(), first_start);
    }
    merged
}

/// A marker found by [`Touching`].
struct Hit<'a, T> {
    /// Its position in the tree.
    ordinal: usize,
    range: Range<usize>,
    entry: &'a Entry<T>,
}

/// The markers that start at or before `range.end` and end at or after
/// `range.start`, in order. Skips every subtree ending before the range.
struct Touching<'a, T: Clone> {
    cursor: Cursor<'a, 'a, Entry<T>, Position>,
    range: Range<usize>,
    started: bool,
    done: bool,
}

impl<'a, T: Clone> Touching<'a, T> {
    fn new(tree: &'a SumTree<Entry<T>>, range: Range<usize>) -> Self {
        Self {
            cursor: tree.cursor(()),
            range,
            started: false,
            done: false,
        }
    }
}

impl<'a, T: Clone> Iterator for Touching<'a, T> {
    type Item = Hit<'a, T>;

    fn next(&mut self) -> Option<Hit<'a, T>> {
        // The cursor's position as it searches: the filter only sees each
        // subtree's summary, whose ends are relative to that position. It
        // starts past the current marker, which the search steps over first.
        if self.done {
            return None;
        }
        let mut base = if self.started {
            self.cursor.item()?;
            self.cursor.end().start
        } else {
            0
        };
        self.started = true;
        let from = self.range.start;
        self.cursor.search_forward(|summary| {
            let reaches = base + summary.max_end >= from;
            if !reaches {
                base += summary.gaps;
            }
            reaches
        });
        let entry = self.cursor.item()?;
        debug_assert_eq!(base, self.cursor.start().start);
        let start = self.cursor.start().start + entry.gap;
        if start > self.range.end {
            self.done = true;
            return None;
        }
        Some(Hit {
            ordinal: self.cursor.start().count,
            range: start..start + entry.len,
            entry,
        })
    }
}
