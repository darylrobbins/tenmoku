//! Proposed changes: edits computed against a snapshot, such as an AI's or a
//! dry run's, held as hunks the user accepts or rejects one at a time while
//! they keep editing.

use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
};

use imara_diff::{Algorithm, Diff, InternedInput};

mod dry_run;
pub use dry_run::{Conflict, DryRun, HunkOutcome, Outcome};

use crate::{Anchor, Buffer, BufferId, BufferSnapshot, Origin, TransactionId, Version};

/// Identifies a hunk and the [`ProposedChange`] it belongs to.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HunkId {
    proposal: u64,
    index: u32,
}

static NEXT_PROPOSAL: AtomicU64 = AtomicU64::new(1);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum HunkStatus {
    /// Not yet accepted or rejected, and its old text was still in place
    /// when last looked at.
    Pending,
    Accepted,
    Rejected,
    /// The text the hunk replaces has changed since the proposal was made,
    /// so it can no longer be accepted.
    Stale,
}

/// A hunk as it stands in one snapshot; see [`ProposedChange::hunks`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HunkView {
    pub id: HunkId,
    /// Where the hunk is now. For a pending, rejected or stale hunk that's the
    /// text it replaces (or would have); for an accepted one, its new text,
    /// in snapshots from the accept on.
    pub range: Range<usize>,
    pub old_text: Arc<str>,
    pub new_text: Arc<str>,
    pub status: HunkStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProposalError {
    /// The text the hunk replaces has changed since the proposal was made.
    #[error("the text under the hunk has changed since it was proposed")]
    Stale,
    /// The hunk was already accepted or rejected, or isn't in this proposal.
    #[error("the hunk isn't pending")]
    NotPending,
}

/// Edits against a snapshot of one buffer, as hunks that follow the text.
///
/// Each hunk's old range is held by two [`Anchor`]s with the biases
/// [`Buffer::edit_from`] uses, so text inserted at a hunk's edges after the
/// snapshot stays where it is and the hunk lands beside it. Once the text
/// between the anchors no longer equals the hunk's old text, the hunk is
/// [`HunkStatus::Stale`] and can't be accepted. A pure insertion has no old
/// text, so it never goes stale, as with `edit_from`; like an `edit_from`
/// insertion, it can end up inside text the user deleted and restored by
/// undo after other edits, since anchors can't tell which side it was on.
///
/// An editor accepting on the user's behalf uses the `_as` accepts with
/// `Origin::User`, so the user's undo reverts the hunk.
///
/// The anchors keep the buffer's edit log alive from [`ProposedChange::version`]
/// on: [`ProposedChange::refresh`] them now and then, and keep
/// [`Buffer::compact_log`]'s floor at or below that version.
#[derive(Clone, Debug)]
pub struct ProposedChange {
    id: u64,
    buffer: BufferId,
    origin: Origin,
    hunks: Vec<Hunk>,
    /// The version of the last snapshot the anchors were made or refreshed
    /// in; [`ProposedChange::version`] when there are no hunks.
    version: Version,
}

#[derive(Clone, Debug)]
struct Hunk {
    id: HunkId,
    start: Anchor,
    end: Anchor,
    /// Once accepted, anchors around its new text, as it landed. Snapshots
    /// from before that still use `start` and `end`.
    landed: Option<(Anchor, Anchor)>,
    old_text: Arc<str>,
    new_text: Arc<str>,
    status: Status,
}

impl ProposedChange {
    /// One hunk per edit, in text order. Ranges are in `base`'s coordinates
    /// and are clipped to its text. Edits that change nothing are dropped.
    ///
    /// # Panics
    /// If two edits overlap, or two insertions share an offset:
    /// [`Buffer::edit`] would merge them into one, so they can't be taken one
    /// at a time.
    pub fn from_edits<I, S>(base: &BufferSnapshot, edits: I, origin: Origin) -> Self
    where
        I: IntoIterator<Item = (Range<usize>, S)>,
        S: Into<Arc<str>>,
    {
        let text = base.text();
        let mut edits: Vec<(Range<usize>, Arc<str>)> = edits
            .into_iter()
            .map(|(range, new_text)| (clip_range(base, range), normalize_newlines(new_text.into())))
            .collect();
        edits.sort_by_key(|(range, _)| (range.start, range.end));
        for pair in edits.windows(2) {
            let (a, b) = (&pair[0].0, &pair[1].0);
            assert!(
                a.end <= b.start && !(a.is_empty() && b.is_empty() && a.start == b.start),
                "proposed edits {a:?} and {b:?} overlap"
            );
        }
        let hunks = edits
            .into_iter()
            .filter(|(range, new_text)| !text.range_eq(range.clone(), new_text))
            .map(|(range, new_text)| {
                let old_text = text.slice_to_string(range.clone()).into();
                (range, old_text, new_text)
            })
            .collect();
        Self::new(base, hunks, origin)
    }

    /// Diffs `base`'s text in `range` against `new_text` by lines (histogram
    /// diff), with one hunk per group of changed lines. Identical text gives
    /// no hunks.
    pub fn from_text(
        base: &BufferSnapshot,
        range: Range<usize>,
        new_text: &str,
        origin: Origin,
    ) -> Self {
        let range = clip_range(base, range);
        let old = base.text().slice_to_string(range.clone());
        let new = normalize_newlines(new_text.into());
        let input = InternedInput::new(old.as_str(), &*new);
        let mut diff = Diff::compute(Algorithm::Histogram, &input);
        diff.postprocess_lines(&input);
        // Byte offset of each line's start, plus the end.
        let starts = |tokens: &[imara_diff::Token]| {
            let mut offsets = Vec::with_capacity(tokens.len() + 1);
            let mut at = 0;
            offsets.push(at);
            for &token in tokens {
                at += input.interner[token].len();
                offsets.push(at);
            }
            offsets
        };
        let (old_lines, new_lines) = (starts(&input.before), starts(&input.after));
        let hunks = diff
            .hunks()
            .map(|hunk| {
                let at = |r: &Range<u32>, lines: &[usize]| {
                    lines[r.start as usize]..lines[r.end as usize]
                };
                let old_range = at(&hunk.before, &old_lines);
                let new_range = at(&hunk.after, &new_lines);
                (
                    range.start + old_range.start..range.start + old_range.end,
                    old[old_range].into(),
                    new[new_range].into(),
                )
            })
            .collect();
        Self::new(base, hunks, origin)
    }

    /// `hunks` are sorted, don't overlap and each changes something.
    fn new(
        base: &BufferSnapshot,
        hunks: Vec<(Range<usize>, Arc<str>, Arc<str>)>,
        origin: Origin,
    ) -> Self {
        let proposal = NEXT_PROPOSAL.fetch_add(1, Ordering::Relaxed);
        let hunks = hunks
            .into_iter()
            .enumerate()
            .map(|(ix, (range, old_text, new_text))| {
                let (start, end) = anchors(base, range);
                Hunk {
                    id: HunkId {
                        proposal,
                        index: u32::try_from(ix).expect("fewer than 2^32 hunks"),
                    },
                    start,
                    end,
                    landed: None,
                    old_text,
                    new_text,
                    status: Status::new(HunkStatus::Pending),
                }
            })
            .collect();
        Self {
            id: proposal,
            buffer: base.id(),
            origin,
            hunks,
            version: base.version(),
        }
    }

    /// Who proposed the change. Accepting under another origin
    /// ([`ProposedChange::accept_as`]) doesn't change it.
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The buffer the proposal was made against.
    pub fn buffer(&self) -> BufferId {
        self.buffer
    }

    /// Every hunk as it stands in `snapshot`, in text order. A pending hunk
    /// whose old text is no longer in place is reported as, and from now on
    /// is, [`HunkStatus::Stale`].
    ///
    /// # Panics
    /// If `snapshot` belongs to another buffer, is older than the anchors
    /// (see [`ProposedChange::refresh`]), or its log was compacted above
    /// [`ProposedChange::version`].
    pub fn hunks(&self, snapshot: &BufferSnapshot) -> Vec<HunkView> {
        let ranges = self.locate(snapshot);
        self.hunks
            .iter()
            .zip(ranges)
            .map(|(hunk, range)| HunkView {
                id: hunk.id,
                status: hunk.check(snapshot, range.clone()),
                range,
                old_text: hunk.old_text.clone(),
                new_text: hunk.new_text.clone(),
            })
            .collect()
    }

    /// Applies one pending hunk at its current range, as one transaction
    /// under the proposal's own origin, separate from any transaction the
    /// caller has open. For a host that applies an agent's edits itself and
    /// wants them on the agent's undo history; an editor accepting on the
    /// user's behalf uses [`ProposedChange::accept_as`] with `Origin::User`.
    ///
    /// # Errors
    /// [`ProposalError::Stale`] if its old text is no longer in place (the
    /// hunk is then stale for good), [`ProposalError::NotPending`] if it was
    /// already accepted or rejected. Either way the buffer is unchanged.
    ///
    /// # Panics
    /// If `buffer` isn't the proposal's buffer, or its log was compacted
    /// above [`ProposedChange::version`].
    pub fn accept(
        &mut self,
        buffer: &mut Buffer,
        id: HunkId,
    ) -> Result<TransactionId, ProposalError> {
        self.accept_as(buffer, id, self.origin.clone())
    }

    /// As [`ProposedChange::accept`], but the transaction is under `origin`,
    /// so that origin's undo reverts the hunk. The proposal's own origin is
    /// unchanged, and the hunk stays accepted after an undo.
    ///
    /// # Errors
    /// As [`ProposedChange::accept`].
    ///
    /// # Panics
    /// As [`ProposedChange::accept`].
    pub fn accept_as(
        &mut self,
        buffer: &mut Buffer,
        id: HunkId,
        origin: Origin,
    ) -> Result<TransactionId, ProposalError> {
        self.check_buffer(buffer.current());
        let hunk = self.find(id).ok_or(ProposalError::NotPending)?;
        match hunk.status.get() {
            HunkStatus::Pending => {}
            HunkStatus::Stale => return Err(ProposalError::Stale),
            HunkStatus::Accepted | HunkStatus::Rejected => return Err(ProposalError::NotPending),
        }
        let range = self.locate(buffer.current())[id.index as usize].clone();
        if hunk.check(buffer.current(), range.clone()) == HunkStatus::Stale {
            return Err(ProposalError::Stale);
        }
        let new_text = hunk.new_text.clone();
        let transaction = buffer.with_transaction_aside(|buffer| {
            buffer.edit([(range.clone(), new_text.clone())], origin)
        });
        let transaction = transaction.expect("a hunk always changes the text");
        let landed = range.start..range.start + new_text.len();
        self.landed(buffer.current(), id, landed);
        Ok(transaction)
    }

    /// Applies every pending hunk whose old text is still in place, as one
    /// batch in one transaction under the proposal's own origin (see
    /// [`ProposedChange::accept`]). Returns that
    /// transaction, if anything was applied, and the hunks skipped as stale
    /// (found stale now or earlier), in text order.
    ///
    /// # Panics
    /// As [`ProposedChange::accept`].
    pub fn accept_all(&mut self, buffer: &mut Buffer) -> (Option<TransactionId>, Vec<HunkId>) {
        self.accept_all_as(buffer, self.origin.clone())
    }

    /// As [`ProposedChange::accept_all`], but the transaction is under
    /// `origin`, as in [`ProposedChange::accept_as`].
    ///
    /// # Panics
    /// As [`ProposedChange::accept`].
    pub fn accept_all_as(
        &mut self,
        buffer: &mut Buffer,
        origin: Origin,
    ) -> (Option<TransactionId>, Vec<HunkId>) {
        self.check_buffer(buffer.current());
        let ranges = self.locate(buffer.current());
        let mut edits = Vec::new();
        let mut accepted = Vec::new();
        let mut stale = Vec::new();
        for (hunk, range) in self.hunks.iter().zip(ranges) {
            match hunk.check(buffer.current(), range.clone()) {
                HunkStatus::Pending => {
                    edits.push((range.clone(), hunk.new_text.clone()));
                    accepted.push((hunk.id, range));
                }
                HunkStatus::Stale => stale.push(hunk.id),
                HunkStatus::Accepted | HunkStatus::Rejected => {}
            }
        }
        if edits.is_empty() {
            return (None, stale);
        }
        // Aside, so a caller's open transaction neither joins nor is closed.
        let transaction = buffer
            .with_transaction_aside(|buffer| buffer.edit(edits, origin))
            .expect("a hunk always changes the text");
        // Where each landed: hunks are in text order and don't overlap, so
        // this is how `Buffer::edit` placed them.
        let mut delta = 0isize;
        for (id, range) in accepted {
            let len = self.hunk(id).new_text.len();
            let start = range.start.checked_add_signed(delta).expect("sorted");
            delta += len as isize - range.len() as isize;
            self.landed(buffer.current(), id, start..start + len);
        }
        (Some(transaction), stale)
    }

    /// Marks a pending or stale hunk rejected; the buffer isn't touched.
    ///
    /// # Errors
    /// [`ProposalError::NotPending`] if it was already accepted or rejected.
    pub fn reject(&mut self, id: HunkId) -> Result<(), ProposalError> {
        let hunk = self.find(id).ok_or(ProposalError::NotPending)?;
        match hunk.status.get() {
            HunkStatus::Pending | HunkStatus::Stale => {
                hunk.status.set(HunkStatus::Rejected);
                Ok(())
            }
            HunkStatus::Accepted | HunkStatus::Rejected => Err(ProposalError::NotPending),
        }
    }

    /// Whether no hunk is pending any more. A pending hunk that has gone
    /// stale counts once [`ProposedChange::hunks`] or an accept has seen it.
    pub fn is_resolved(&self) -> bool {
        self.hunks
            .iter()
            .all(|hunk| hunk.status.get() != HunkStatus::Pending)
    }

    /// Rebases every anchor onto `snapshot`, so resolving them stays cheap and
    /// the buffer's log can be compacted up to `snapshot`'s version.
    ///
    /// # Panics
    /// As [`ProposedChange::hunks`].
    pub fn refresh(&mut self, snapshot: &BufferSnapshot) {
        self.check_buffer(snapshot);
        for hunk in &mut self.hunks {
            hunk.start = snapshot.refresh(&hunk.start);
            hunk.end = snapshot.refresh(&hunk.end);
            if let Some((start, end)) = &mut hunk.landed
                && start.version() <= snapshot.version()
            {
                *start = snapshot.refresh(start);
                *end = snapshot.refresh(end);
            }
        }
        self.version = snapshot.version();
    }

    /// The oldest version the anchors use. The buffer's owner must keep
    /// [`Buffer::compact_log`]'s floor at or below it.
    pub fn version(&self) -> Version {
        self.hunks
            .iter()
            .flat_map(|hunk| [hunk.start.version(), hunk.end.version()])
            .chain(
                self.hunks
                    .iter()
                    .filter_map(|hunk| Some(hunk.landed?.0.version())),
            )
            .min()
            .unwrap_or(self.version)
    }

    fn check_buffer(&self, snapshot: &BufferSnapshot) {
        assert_eq!(
            snapshot.id(),
            self.buffer,
            "the snapshot belongs to another buffer than the proposal"
        );
    }

    /// Every hunk's current range in `snapshot`, in hunk order.
    fn locate(&self, snapshot: &BufferSnapshot) -> Vec<Range<usize>> {
        self.check_buffer(snapshot);
        let anchors: Vec<Anchor> = self
            .hunks
            .iter()
            .flat_map(|hunk| {
                let range = hunk.anchors(snapshot);
                [range.start, range.end]
            })
            .collect();
        let mut ranges: Vec<Range<usize>> = snapshot
            .resolve_many(&anchors)
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[start, end]| start..end.max(start))
            .collect();
        // A user edit can merge two insertion points into one offset, and
        // accepting the later hunk first lands its text there, which pushes
        // the earlier hunk's anchor after it. Put the pending insertion back
        // before the text of later hunks that landed ending at its point, so
        // the text order doesn't depend on the accept order.
        for i in 0..self.hunks.len() {
            let hunk = &self.hunks[i];
            if !hunk.old_text.is_empty() || hunk.landed_in(snapshot) {
                continue;
            }
            for j in (i + 1..self.hunks.len()).rev() {
                let landed = &ranges[j];
                if self.hunks[j].landed_in(snapshot)
                    && !landed.is_empty()
                    && landed.end == ranges[i].start
                {
                    let start = landed.start;
                    ranges[i] = start..start;
                }
            }
        }
        ranges
    }

    fn find(&self, id: HunkId) -> Option<&Hunk> {
        if id.proposal != self.id {
            return None;
        }
        self.hunks.get(id.index as usize)
    }

    fn hunk(&self, id: HunkId) -> &Hunk {
        self.find(id).expect("a hunk of this proposal")
    }

    /// Records that hunk `id` was applied, its new text now at `range` in
    /// `current`, and anchors it there.
    fn landed(&mut self, current: &BufferSnapshot, id: HunkId, range: Range<usize>) {
        let hunk = &mut self.hunks[id.index as usize];
        hunk.landed = Some(anchors(current, range));
        hunk.status.set(HunkStatus::Accepted);
    }
}

impl Hunk {
    /// Whether the hunk was accepted by `snapshot`.
    fn landed_in(&self, snapshot: &BufferSnapshot) -> bool {
        matches!(self.landed, Some((start, _)) if start.version() <= snapshot.version())
    }

    /// The anchors to resolve in `snapshot`: where it landed, if it was
    /// accepted by then, else its old range.
    fn anchors(&self, snapshot: &BufferSnapshot) -> Range<Anchor> {
        match self.landed {
            Some((start, end)) if start.version() <= snapshot.version() => start..end,
            _ => self.start..self.end,
        }
    }

    /// The hunk's status, given its current `range`: a pending hunk whose
    /// old text is gone becomes stale.
    fn check(&self, snapshot: &BufferSnapshot, range: Range<usize>) -> HunkStatus {
        self.record(self.verdict(snapshot, range))
    }

    /// Records a `verdict` read earlier: only staleness is ever stored, and
    /// only over a pending status, so a verdict that raced with another
    /// thread's can't undo it.
    fn record(&self, verdict: HunkStatus) -> HunkStatus {
        if verdict == HunkStatus::Stale {
            self.status.mark_stale()
        } else {
            verdict
        }
    }

    /// What [`Hunk::check`] would make the status, without recording it.
    fn verdict(&self, snapshot: &BufferSnapshot, range: Range<usize>) -> HunkStatus {
        let status = self.status.get();
        if status == HunkStatus::Pending && !snapshot.text().range_eq(range, &self.old_text) {
            return HunkStatus::Stale;
        }
        status
    }
}

/// Anchors for a hunk at `range`, biased as [`Buffer::edit_from`] maps a
/// range: text inserted at either edge stays outside it, and an insertion
/// uses one anchor that moves after text inserted there.
fn anchors(snapshot: &BufferSnapshot, range: Range<usize>) -> (Anchor, Anchor) {
    let start = snapshot.anchor_after(range.start);
    if range.is_empty() {
        (start, start)
    } else {
        (start, snapshot.anchor_before(range.end))
    }
}

fn clip_range(snapshot: &BufferSnapshot, range: Range<usize>) -> Range<usize> {
    let text = snapshot.text();
    let (start, end) = (range.start.min(range.end), range.start.max(range.end));
    text.clip_offset(start, fe_text::Bias::Left)..text.clip_offset(end, fe_text::Bias::Right)
}

/// The buffer holds `\n` line endings only; [`Buffer::edit`] converts the
/// rest, so hunks hold the text it will actually insert.
fn normalize_newlines(text: Arc<str>) -> Arc<str> {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n").into()
    } else {
        text
    }
}

/// A hunk's status. [`ProposedChange::hunks`] records staleness through a
/// shared reference, and this keeps the proposal `Sync`.
struct Status(AtomicU8);

impl Status {
    const ALL: [HunkStatus; 4] = [
        HunkStatus::Pending,
        HunkStatus::Accepted,
        HunkStatus::Rejected,
        HunkStatus::Stale,
    ];

    fn new(status: HunkStatus) -> Self {
        Self(AtomicU8::new(status as u8))
    }

    fn get(&self) -> HunkStatus {
        Self::ALL[usize::from(self.0.load(Ordering::Relaxed))]
    }

    /// Records the transition from pending to stale, never the reverse, so a
    /// check of an older snapshot can't undo a staleness another thread
    /// recorded. Returns the status after.
    fn mark_stale(&self) -> HunkStatus {
        let seen = match self.0.compare_exchange(
            HunkStatus::Pending as u8,
            HunkStatus::Stale as u8,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => HunkStatus::Stale as u8,
            Err(seen) => seen,
        };
        Self::ALL[usize::from(seen)]
    }

    fn set(&self, status: HunkStatus) {
        self.0.store(status as u8, Ordering::Relaxed);
    }
}

impl Clone for Status {
    fn clone(&self) -> Self {
        Self::new(self.get())
    }
}

impl std::fmt::Debug for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.get().fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Buffer;

    /// A check of an unchanged snapshot that read the hunk as pending, then
    /// lost a race with a check of an edited snapshot that recorded it stale,
    /// must not bring it back. Deterministic: the first check is paused
    /// between reading its verdict and recording it.
    #[test]
    fn paused_pending_verdict_does_not_resurrect_a_stale_hunk() {
        let mut buffer = Buffer::new("a\nb\n");
        let base = buffer.snapshot();
        let proposal = ProposedChange::from_text(&base, 0..base.len(), "a\nB\n", Origin::User);
        buffer.edit([(2..3, "x")], Origin::User);
        let edited = buffer.snapshot();
        let hunk = &proposal.hunks[0];
        let range = proposal.locate(&base)[0].clone();

        let paused = hunk.verdict(&base, range);
        assert_eq!(paused, HunkStatus::Pending);

        let range = proposal.locate(&edited)[0].clone();
        assert_eq!(hunk.check(&edited, range), HunkStatus::Stale);

        hunk.record(paused);
        assert_eq!(hunk.status.get(), HunkStatus::Stale);
    }
}
