use std::{ops::Range, sync::Arc};

use fe_text::{Bias, Text};

use crate::{
    Edit,
    edit::{Mapping, map_sorted},
    log::EditLog,
};

/// Identifies a buffer, so snapshots and anchors can't be mixed up between buffers.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct BufferId(pub(crate) u64);

/// Counts applied edit batches. Every batch, including undo and redo, bumps it.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(pub(crate) usize);

/// A position that follows the text as it's edited. Resolve it with
/// [`BufferSnapshot::resolve`]; its [`Bias`] decides which side of text
/// inserted exactly at it the anchor ends up on.
///
/// Resolving costs time proportional to the edits made since the anchor's
/// version. Long-lived holders should [`BufferSnapshot::refresh`] their
/// anchors now and then to keep that cost small, and so the buffer's owner
/// can [compact](crate::Buffer::compact_log) its log below them.
/// Anchors below a snapshot's [`BufferSnapshot::oldest_version`] can't be
/// resolved in it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Anchor {
    pub(crate) offset: usize,
    pub(crate) bias: Bias,
    pub(crate) version: Version,
}

impl Anchor {
    pub fn bias(&self) -> Bias {
        self.bias
    }

    pub fn version(&self) -> Version {
        self.version
    }
}

/// An immutable view of a buffer at one version. Cheap to clone and safe to
/// send to other threads.
#[derive(Clone)]
pub struct BufferSnapshot {
    pub(crate) id: BufferId,
    pub(crate) text: Text,
    pub(crate) log: EditLog,
}

impl BufferSnapshot {
    pub fn id(&self) -> BufferId {
        self.id
    }

    pub fn version(&self) -> Version {
        self.log.version()
    }

    /// The oldest version anchors, offsets and [`BufferSnapshot::edits_since`]
    /// can be mapped from. Older ones were compacted away by
    /// [`Buffer::compact_log`](crate::Buffer::compact_log); their holders
    /// have to rebuild from this snapshot.
    pub fn oldest_version(&self) -> Version {
        self.log.floor()
    }

    pub fn text(&self) -> &Text {
        &self.text
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// An anchor at `offset`, clipped to the text and to a char boundary.
    pub fn anchor_at(&self, offset: usize, bias: Bias) -> Anchor {
        Anchor {
            offset: self.text.clip_offset(offset, bias),
            bias,
            version: self.version(),
        }
    }

    /// An anchor that stays before text inserted at `offset`.
    pub fn anchor_before(&self, offset: usize) -> Anchor {
        self.anchor_at(offset, Bias::Left)
    }

    /// An anchor that moves after text inserted at `offset`.
    pub fn anchor_after(&self, offset: usize) -> Anchor {
        self.anchor_at(offset, Bias::Right)
    }

    /// The anchor's current offset in this snapshot.
    ///
    /// # Panics
    /// If the anchor comes from a newer version than this snapshot, or one
    /// below its [`BufferSnapshot::oldest_version`].
    pub fn resolve(&self, anchor: &Anchor) -> usize {
        assert!(
            anchor.version <= self.version(),
            "anchor from {:?} is newer than snapshot {:?}",
            anchor.version,
            self.version()
        );
        self.log.map(
            anchor.offset,
            anchor.bias.into(),
            anchor.version,
            self.version(),
        )
    }

    /// Resolves many anchors at once, in the order given. Much faster than
    /// resolving one at a time when many anchors share a version, as
    /// diagnostics, highlights and decorations usually do.
    pub fn resolve_many(&self, anchors: &[Anchor]) -> Vec<usize> {
        let key = |anchor: &Anchor| (anchor.version, anchor.bias);
        let mut order: Vec<usize> = (0..anchors.len()).collect();
        order.sort_unstable_by_key(|&ix| (key(&anchors[ix]), anchors[ix].offset));

        let mut resolved = vec![0; anchors.len()];
        for group in order.chunk_by(|&a, &b| key(&anchors[a]) == key(&anchors[b])) {
            let first = &anchors[group[0]];
            assert!(
                first.version <= self.version(),
                "anchor is newer than snapshot"
            );
            let mut offsets: Vec<usize> = group.iter().map(|&ix| anchors[ix].offset).collect();
            for edits in self.log.mapping_batches(first.version, self.version()) {
                map_sorted(edits, &mut offsets, first.bias.into());
            }
            for (&ix, offset) in group.iter().zip(offsets) {
                resolved[ix] = offset;
            }
        }
        resolved
    }

    pub fn resolve_range(&self, range: &Range<Anchor>) -> Range<usize> {
        let start = self.resolve(&range.start);
        let end = self.resolve(&range.end).max(start);
        start..end
    }

    /// The same anchor, rebased onto this snapshot's version so resolving it
    /// later is cheap.
    pub fn refresh(&self, anchor: &Anchor) -> Anchor {
        Anchor {
            offset: self.resolve(anchor),
            bias: anchor.bias,
            version: self.version(),
        }
    }

    /// Maps an offset from an older version of this buffer to this one.
    pub fn map_offset(&self, offset: usize, bias: Bias, from: Version) -> usize {
        self.log.map(offset, bias.into(), from, self.version())
    }

    /// Whether any byte of `range`, in the text at `from`, is still in this snapshot's text:
    /// edits (counting undo and redo as `resolve` does) haven't replaced or deleted all of it.
    /// Text inserted into the range doesn't count.
    ///
    /// # Panics
    /// If `from` is below [`BufferSnapshot::oldest_version`] or after this snapshot.
    pub fn survives(&self, range: Range<usize>, from: Version) -> bool {
        self.log.survives(range, from, self.version())
    }

    /// Whether edits since `from` replaced or deleted text on both sides of `offset`, a position
    /// at that version which moves with `bias` as an anchor does, so that no neighbour is left.
    ///
    /// # Panics
    /// If `from` is below [`BufferSnapshot::oldest_version`] or after this snapshot.
    pub fn swallowed(&self, offset: usize, bias: Bias, from: Version) -> bool {
        self.log
            .swallowed(offset, bias.into(), from, self.version())
    }

    /// The parts of `spans`, bytes of the text at `from`, that edits since haven't replaced or
    /// deleted, as bytes of this snapshot's text. Text inserted into a span splits it.
    ///
    /// # Panics
    /// If `from` is below [`BufferSnapshot::oldest_version`] or after this snapshot.
    pub fn carried(&self, spans: Vec<Range<usize>>, from: Version) -> Vec<Range<usize>> {
        self.log.carried(spans, from, self.version())
    }

    /// Follows an empty entry through the edits since `from`: `at` is its offset and the text's
    /// length at `from`, and it moves with `bias` as an anchor does. Returns its offset, the
    /// length, and its span, as of this snapshot, or None if edits took it: they replaced text
    /// strictly around it, or all of its `span` (the text around it at `from`, none at either
    /// end of the text). An entry with no span gets one when it has text on both sides.
    ///
    /// # Panics
    /// If `from` is below [`BufferSnapshot::oldest_version`] or after this snapshot.
    #[allow(clippy::type_complexity)]
    pub fn followed(
        &self,
        at: (usize, usize),
        bias: Bias,
        span: Option<Vec<Range<usize>>>,
        from: Version,
    ) -> Option<(usize, usize, Option<Vec<Range<usize>>>)> {
        self.log
            .followed(at, bias.into(), span, from, self.version())
    }

    /// See [`EditLog::map_for_history`].
    pub(crate) fn map_for_history(
        &self,
        offset: usize,
        mapping: Mapping,
        from: Version,
        collapsed_by: &mut Vec<crate::log::Collapse>,
    ) -> Option<usize> {
        self.log
            .map_for_history(offset, mapping, from, self.version(), collapsed_by)
    }

    /// The edit batches applied since `version`, oldest first. Each batch's
    /// ranges are in the coordinates just before and just after that batch.
    ///
    /// # Panics
    /// If `version` is below [`BufferSnapshot::oldest_version`].
    pub fn edits_since(&self, version: Version) -> impl Iterator<Item = &Arc<[Edit]>> {
        self.log.batches(version, self.version())
    }
}

impl std::fmt::Debug for BufferSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BufferSnapshot")
            .field("id", &self.id)
            .field("version", &self.version())
            .field("len", &self.len())
            .finish()
    }
}
