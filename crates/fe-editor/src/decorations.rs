//! Decorations a host or a feature puts on the editor's rows: bands,
//! gutter marks, highlights, underlines and text after a line.
//!
//! A [`DecorationProvider`] hands them out for the range a frame shows. It
//! either stores them, in a [`DecorationSet`], or computes them from the
//! snapshot on demand, caching by [`BufferSnapshot::version`] itself (a git
//! provider diffing against a base text, a bracket matcher). An
//! [`EditorModel`](crate::EditorModel) holds its providers in a registry
//! (see [`EditorModel::add_decoration_provider`](crate::EditorModel::add_decoration_provider)),
//! refreshes them as it catches up, and merges what they return in paint
//! order.
//!
//! Nothing here paints: a decoration names a role ([`Tint`]), never a
//! colour, and the painter resolves it from the theme.

use std::{
    ops::{Range, RangeInclusive},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use fe_buffer::{Anchor, Bias, BufferSnapshot, Text, Version};

/// What a decoration is drawn in: a role the painter resolves from the
/// theme, so decorations follow theme changes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Tint {
    Error,
    Warning,
    Information,
    Hint,
    Accent,
    Muted,
    Added,
    Modified,
    Removed,
    /// A diff view's added text: its rows' band, or, as a highlight, its
    /// changed words, each in a colour of its own rather than a fade of
    /// one.
    DiffAdded,
    /// A diff view's removed text, as [`Tint::DiffAdded`].
    DiffRemoved,
}

/// A mark in the gutter.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum GutterMark {
    /// A bar beside every display row covered.
    Bar,
    /// A short horizontal mark on the top edge of the first row covered:
    /// rows were removed just above it.
    Deletion,
}

/// How a decoration is drawn.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DecorationKind {
    /// A full-width band behind each covered row.
    RowBackground,
    Gutter(GutterMark),
    /// A background behind the range's text.
    Highlight,
    /// An underline under the range's text.
    Underline {
        wavy: bool,
    },
    /// Text shown after the end of the range's last buffer row. It never
    /// moves or wraps the buffer's text.
    AfterLine(Arc<str>),
    /// The range's text drawn in the colour the editor's theme gives this
    /// highlight name (`"function.method"`, `"variable.parameter"`), over
    /// whatever colour the syntax gives it. A name the theme doesn't know
    /// leaves the syntax colour. It paints no background or underline.
    Foreground(Arc<str>),
    /// A straight underline under the range's text in the colour the
    /// editor's theme gives this highlight name, as [`Foreground`] would
    /// draw the text. A name the theme doesn't know underlines in the
    /// editor's foreground. The tint is unused.
    ///
    /// [`Foreground`]: DecorationKind::Foreground
    SyntaxUnderline(Arc<str>),
}

/// A decoration over a byte range of the snapshot it was resolved in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Decoration {
    pub range: Range<usize>,
    pub kind: DecorationKind,
    pub tint: Tint,
    /// Paint order: higher is painted later, on top.
    pub priority: i32,
}

impl Decoration {
    pub fn new(range: Range<usize>, kind: DecorationKind, tint: Tint) -> Self {
        Self {
            range,
            kind,
            tint,
            priority: 0,
        }
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// The buffer rows it covers in `text`, the one its range is in: from
    /// its start's row through its end's row, not counting an end row it
    /// ends at the start of. An empty one covers the row it is on.
    pub fn covered_rows(&self, text: &Text) -> RangeInclusive<u32> {
        let first = text.offset_to_point(self.range.start).row;
        let mut last = text.offset_to_point(self.range.end).row;
        if last > first && self.range.end == text.row_start(last) {
            last -= 1;
        }
        first..=last
    }
}

/// Whether a decoration over `item` overlaps `range`: they share text, or
/// either is empty and touches the other, ends included.
pub fn overlaps(item: &Range<usize>, range: &Range<usize>) -> bool {
    if item.is_empty() || range.is_empty() {
        item.start <= range.end && range.start <= item.end
    } else {
        item.start < range.end && range.start < item.end
    }
}

/// A source of decorations, registered with an
/// [`EditorModel`](crate::EditorModel).
pub trait DecorationProvider {
    /// Appends the decorations overlapping `range` (bytes of `snapshot`),
    /// an empty one at either end included. Called once per painted frame
    /// with the frame's shown range only.
    fn decorations(
        &self,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    );

    /// The model caught up to `snapshot`. Default: nothing.
    fn refresh(&mut self, _snapshot: &BufferSnapshot) {}
}

/// A provider's handle in an [`EditorModel`](crate::EditorModel)'s
/// registry. Unique within the process.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProviderId(u64);

impl ProviderId {
    pub(crate) fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Where a range's ends go when text is inserted right at them: inside
/// neither, so a decoration doesn't grow over what's typed beside it.
const START: Bias = Bias::Right;
const END: Bias = Bias::Left;

#[derive(Clone, Debug)]
struct Entry {
    range: Range<Anchor>,
    /// The greatest end of this entry and every one before it.
    max_end: Anchor,
    kind: DecorationKind,
    tint: Tint,
    priority: i32,
}

/// Decorations kept as anchors, so they follow edits: the stock
/// [`DecorationProvider`].
///
/// Entries are sorted by start, each with the running maximum of the ends
/// so far, so a query binary-searches to the first entry that can reach
/// the range and stops at the first that starts past it. Anchors map
/// offsets monotonically, so both orders survive edits.
#[derive(Clone, Debug, Default)]
pub struct DecorationSet {
    entries: Vec<Entry>,
    /// The version every anchor in `entries` is of.
    version: Version,
    /// The last version applied or spliced from.
    applied: Option<Version>,
}

impl DecorationSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Removes every decoration. A result older than the last one applied
    /// is still rejected after.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Whether a result computed at `version` can land through `snapshot`:
    /// not older than the last one that did, and mappable onto `snapshot`.
    fn accepts(&self, snapshot: &BufferSnapshot, version: Version) -> bool {
        !self.applied.is_some_and(|last| version < last)
            && version >= snapshot.oldest_version()
            && version <= snapshot.version()
    }

    /// Whether the anchors resolve in `snapshot`: it's at or after their
    /// version, and they're above its edit-log floor.
    fn resolves_in(&self, snapshot: &BufferSnapshot) -> bool {
        self.version <= snapshot.version() && self.version >= snapshot.oldest_version()
    }

    /// Replaces every decoration with `items`, their ranges bytes of buffer
    /// version `version`, which `snapshot` is at or after. Returns whether
    /// it did: not if `version` is older than the last one applied or
    /// spliced, below the snapshot's edit-log floor, or after the snapshot.
    pub fn apply(
        &mut self,
        snapshot: &BufferSnapshot,
        version: Version,
        items: Vec<Decoration>,
    ) -> bool {
        if !self.accepts(snapshot, version) {
            return false;
        }
        let items = items
            .into_iter()
            .map(|item| rebase(snapshot, version, item));
        self.rebuild(snapshot, items.collect());
        self.applied = Some(version);
        true
    }

    /// Replaces the decorations lying inside `within` with `items`, all
    /// bytes of buffer version `version`, and leaves the rest: for a re-diff
    /// of one hunk, or an edit streaming in. Returns whether it did, on the
    /// terms of [`apply`](Self::apply); nor if the set's anchors don't
    /// resolve in `snapshot` (it's older than them, or its log was compacted
    /// past them), as the decorations it keeps couldn't be rebased: the
    /// host applies them all again.
    pub fn splice(
        &mut self,
        snapshot: &BufferSnapshot,
        version: Version,
        within: Range<usize>,
        items: Vec<Decoration>,
    ) -> bool {
        if !self.accepts(snapshot, version) || (!self.is_empty() && !self.resolves_in(snapshot)) {
            return false;
        }
        let start = snapshot.map_offset(within.start, START, version);
        let within = start..snapshot.map_offset(within.end, END, version).max(start);
        let mut kept: Vec<Decoration> = self.resolve_all(snapshot);
        kept.retain(|item| !(within.start <= item.range.start && item.range.end <= within.end));
        kept.extend(
            items
                .into_iter()
                .map(|item| rebase(snapshot, version, item)),
        );
        self.rebuild(snapshot, kept);
        self.applied = Some(version);
        true
    }

    /// Rebases every anchor onto `snapshot`, so resolving them stays cheap
    /// however many edits follow. Does nothing with a snapshot older than
    /// the anchors; drops every decoration if the log has been compacted
    /// past them.
    pub fn refresh(&mut self, snapshot: &BufferSnapshot) {
        if self.version >= snapshot.version() || self.entries.is_empty() {
            return;
        }
        if self.version < snapshot.oldest_version() {
            self.entries.clear();
            return;
        }
        let items = self.resolve_all(snapshot);
        self.rebuild(snapshot, items);
    }

    /// Every decoration, by start, in `snapshot`; empty unless its anchors
    /// resolve there.
    fn resolve_all(&self, snapshot: &BufferSnapshot) -> Vec<Decoration> {
        if self.entries.is_empty() || !self.resolves_in(snapshot) {
            return Vec::new();
        }
        let anchors: Vec<Anchor> = (self.entries.iter())
            .flat_map(|entry| [entry.range.start, entry.range.end])
            .collect();
        let offsets = snapshot.resolve_many(&anchors);
        (self.entries.iter().zip(offsets.chunks(2)))
            .map(|(entry, ends)| Decoration {
                range: ends[0]..ends[1].max(ends[0]),
                kind: entry.kind.clone(),
                tint: entry.tint,
                priority: entry.priority,
            })
            .collect()
    }

    /// Replaces the entries with `items`, ranges in `snapshot`.
    fn rebuild(&mut self, snapshot: &BufferSnapshot, mut items: Vec<Decoration>) {
        items.sort_by_key(|item| (item.range.start, item.range.end));
        let mut max_end: Option<Anchor> = None;
        let mut max_offset = 0;
        self.entries = items
            .into_iter()
            .map(|item| {
                let start = snapshot.anchor_at(item.range.start, START);
                let end = snapshot.anchor_at(item.range.end, END);
                if max_end.is_none() || item.range.end > max_offset {
                    max_end = Some(end);
                    max_offset = item.range.end;
                }
                Entry {
                    range: start..end,
                    max_end: max_end.unwrap_or(end),
                    kind: item.kind,
                    tint: item.tint,
                    priority: item.priority,
                }
            })
            .collect();
        self.version = snapshot.version();
    }
}

impl DecorationProvider for DecorationSet {
    fn decorations(
        &self,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        if self.entries.is_empty() || !self.resolves_in(snapshot) {
            return;
        }
        let examine = || {
            #[cfg(test)]
            tests::EXAMINED.with(|count| count.set(count.get() + 1));
        };
        let resolve = |anchor: &Anchor| snapshot.resolve(anchor);
        // How far an entry's end, or any before it, reaches: a resolved end
        // is never before its start, and starts are sorted.
        let first = self.entries.partition_point(|entry| {
            examine();
            resolve(&entry.max_end).max(resolve(&entry.range.start)) < range.start
        });
        for entry in &self.entries[first..] {
            examine();
            let start = resolve(&entry.range.start);
            if start > range.end {
                break;
            }
            let item = start..resolve(&entry.range.end).max(start);
            if overlaps(&item, &range) {
                out.push(Decoration {
                    range: item,
                    kind: entry.kind.clone(),
                    tint: entry.tint,
                    priority: entry.priority,
                });
            }
        }
    }

    fn refresh(&mut self, snapshot: &BufferSnapshot) {
        DecorationSet::refresh(self, snapshot);
    }
}

/// `item`, its range bytes of `version`, in `snapshot`'s text.
fn rebase(snapshot: &BufferSnapshot, version: Version, mut item: Decoration) -> Decoration {
    let start = snapshot.map_offset(item.range.start, START, version);
    let end = snapshot.map_offset(item.range.end, END, version).max(start);
    item.range = start..end;
    item
}

#[cfg(test)]
#[path = "decorations_tests.rs"]
mod tests;
