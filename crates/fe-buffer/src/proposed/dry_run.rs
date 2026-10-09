//! Dry run of a [`ProposedChange`]: what accepting every hunk would do,
//! without doing it.

use std::{fmt, ops::Range, sync::Arc};

use fe_text::Text;

use super::{HunkId, HunkStatus, ProposedChange};
use crate::{BufferSnapshot, Version, edit::normalize};

/// Why a hunk can't be applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Conflict {
    /// The text at the hunk's range differs from the text it replaces.
    Changed { expected: Arc<str>, found: Arc<str> },
    /// The hunk was recorded stale earlier, though its old text is back (say,
    /// after an undo); `accept` refuses it.
    WasStale,
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changed { .. } => f.write_str("the text under this change was edited"),
            Self::WasStale => f.write_str("this change went stale earlier"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Accepting would apply it.
    Clean,
    Conflict(Conflict),
    /// Already accepted or rejected, so a dry run doesn't apply it.
    Resolved(HunkStatus),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HunkOutcome {
    pub id: HunkId,
    /// Where the hunk is in the snapshot, as [`ProposedChange::hunks`] gives.
    pub range: Range<usize>,
    pub outcome: Outcome,
}

/// The result of [`ProposedChange::dry_run`]. Owns everything, so it can be
/// built on a background thread and sent back.
#[derive(Clone, Debug)]
pub struct DryRun {
    version: Version,
    hunks: Vec<HunkOutcome>,
    text: Text,
}

impl DryRun {
    /// The version of the snapshot the dry run was made against.
    pub fn version(&self) -> Version {
        self.version
    }

    /// Every hunk, in text order.
    pub fn hunks(&self) -> &[HunkOutcome] {
        &self.hunks
    }

    /// Whether no hunk conflicts.
    pub fn applies_cleanly(&self) -> bool {
        self.hunks
            .iter()
            .all(|hunk| !matches!(hunk.outcome, Outcome::Conflict(_)))
    }

    /// The snapshot's text with every clean hunk applied: what
    /// `accept_all` would leave.
    pub fn text(&self) -> &Text {
        &self.text
    }
}

impl ProposedChange {
    /// Reports how each hunk would fare if accepted in `snapshot`, and the
    /// text accepting all the clean ones would give. Changes nothing, not
    /// even a hunk's recorded status.
    ///
    /// # Panics
    /// As [`ProposedChange::hunks`].
    pub fn dry_run(&self, snapshot: &BufferSnapshot) -> DryRun {
        let ranges = self.locate(snapshot);
        let mut hunks = Vec::with_capacity(ranges.len());
        let mut edits = Vec::new();
        for (hunk, range) in self.hunks.iter().zip(ranges) {
            let outcome = match (hunk.status.get(), hunk.verdict(snapshot, range.clone())) {
                (status @ (HunkStatus::Accepted | HunkStatus::Rejected), _) => {
                    Outcome::Resolved(status)
                }
                (_, HunkStatus::Pending) => {
                    edits.push((range.clone(), hunk.new_text.clone()));
                    Outcome::Clean
                }
                _ if snapshot.text().range_eq(range.clone(), &hunk.old_text) => {
                    Outcome::Conflict(Conflict::WasStale)
                }
                _ => Outcome::Conflict(Conflict::Changed {
                    expected: hunk.old_text.clone(),
                    found: snapshot.text().slice_to_string(range.clone()).into(),
                }),
            };
            hunks.push(HunkOutcome {
                id: hunk.id,
                range,
                outcome,
            });
        }
        let mut text = snapshot.text().clone();
        for (range, new_text) in normalize(edits).into_iter().rev() {
            text.replace(range, &new_text);
        }
        DryRun {
            version: snapshot.version(),
            hunks,
            text,
        }
    }
}
