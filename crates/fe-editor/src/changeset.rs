//! A headless AI changeset: the proposals one chat turn made for several
//! files, reviewed together. Per-file and per-hunk accept and reject, accept
//! all, reject all, a summary, and a checkpoint that reverts everything the
//! turn accepted.
//!
//! A [`Changeset`] owns no buffers and no GPUI. It holds the same
//! `Rc<RefCell<ProposedChange>>` the editor view and `ScriptedAi` hold, and
//! borrows buffers per call through a [`BufferSet`] the workbench implements.
//!
//! Accepting goes through [`ProposedChange::accept_as`] with [`Origin::User`],
//! so each accept is a user-origin transaction, as the proposal API defines.
//!
//! # Counts
//!
//! [`Changeset::add_file`] records the proposal's hunk counts as it stands
//! (every hunk pending unless it was already decided). They are read again
//! from the buffer whenever it is reachable (a listing, summary or any
//! decision) and kept for when it is not.
//!
//! # Checkpoint
//!
//! The first accept in a file that applies something records its pre-accept
//! text and version, then the version after every accept and the
//! [`TransactionId`]s. `fe-buffer` has no undo-by-id primitive:
//! [`Buffer::undo`] takes the newest transaction matching an origin filter,
//! which cannot tell this changeset's accepts from the user's other
//! transactions. So revert uses the version check and a text restore only:
//! [`Changeset::revert`] replaces the file's text with the pre-accept text as
//! one new transaction (`reapplied: true`) when the buffer's version is the
//! one after the last accept and nothing edited between accepts. A file
//! edited since is never overwritten by it; it reports
//! [`RevertOutcome::NeedsConfirm`], and [`Changeset::revert_forced`]
//! restores the text anyway. The recorded transaction ids stay in the
//! checkpoint for a host that gains undo-by-id.
//!
//! After a revert the shared proposal is rebuilt over the restored text,
//! from the hunks as they stood at the first accept, keeping no-op ones
//! (insertions that user edits had merged into one point become one
//! insertion). Its hunk ids are new, and every hunk, stale or rejected
//! earlier, is pending again.

use std::{
    cell::RefCell,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use fe_buffer::{
    Buffer, BufferId, HunkId, HunkStatus, Origin, ProposalError, ProposedChange, TransactionId,
    Version,
};

/// The buffers a changeset reaches, by id. The workbench implements this;
/// tests implement it over a map.
pub trait BufferSet {
    fn buffer_mut(&mut self, id: BufferId) -> Option<&mut Buffer>;

    /// Creates a buffer holding `text` for a new file. `None` if it can't.
    fn create(&mut self, _path: &Path, _text: &str) -> Option<BufferId> {
        None
    }

    /// Removes a buffer made by [`BufferSet::create`]. Whether it was there.
    fn remove(&mut self, _id: BufferId) -> bool {
        false
    }
}

/// Why a hunk or file could not be decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangesetError {
    /// The file's buffer is gone, or its text moved so a hunk no longer
    /// applies. Nothing was applied to the file.
    Conflict { path: PathBuf },
    /// The changeset has no such file.
    NoSuchFile { path: PathBuf },
    /// The hunk is not pending, or is not in that file (or the file is a new
    /// file, which is decided whole).
    NotPending { path: PathBuf },
}

impl std::fmt::Display for ChangesetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict { path } => {
                write!(f, "{} changed since it was proposed", path.display())
            }
            Self::NoSuchFile { path } => write!(f, "{} is not in the changeset", path.display()),
            Self::NotPending { path } => {
                write!(f, "that hunk of {} is not pending", path.display())
            }
        }
    }
}

impl std::error::Error for ChangesetError {}

/// Hunk counts of one file, or the totals of a changeset.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub hunks: usize,
    pub pending: usize,
    pub accepted: usize,
    pub rejected: usize,
    /// Hunks whose text changed under them; they can only be rejected.
    pub stale: usize,
}

impl Counts {
    fn add(&mut self, other: Counts) {
        self.hunks += other.hunks;
        self.pending += other.pending;
        self.accepted += other.accepted;
        self.rejected += other.rejected;
        self.stale += other.stale;
    }
}

/// One file of a changeset, as listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub path: PathBuf,
    /// A file the turn creates; it counts as one hunk.
    pub is_new: bool,
    /// The buffer is gone, so `counts` is as last read.
    pub missing: bool,
    pub counts: Counts,
    /// Whether `counts` was ever read; true for every file added.
    pub counts_known: bool,
}

/// The totals over every file.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub files: usize,
    pub counts: Counts,
}

/// What reverting did to one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevertOutcome {
    /// Back to its text before the turn, by one new transaction restoring it
    /// (`reapplied` is always true until the buffer can undo by id), or, for a
    /// new file, by removing it.
    Reverted { path: PathBuf, reapplied: bool },
    /// Edited since the last accept; [`Changeset::revert_forced`] overwrites.
    NeedsConfirm { path: PathBuf },
    /// The file's buffer is gone; nothing to revert.
    Conflict { path: PathBuf },
}

enum Kind {
    Existing {
        buffer: BufferId,
        proposal: Rc<RefCell<ProposedChange>>,
        checkpoint: Option<Checkpoint>,
        /// The counts last read, for when the buffer is missing.
        last: Option<Counts>,
    },
    New {
        text: String,
        state: NewState,
    },
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum NewState {
    Pending,
    Accepted { buffer: BufferId, version: Version },
    Rejected,
}

/// What the first accept in a file recorded.
struct Checkpoint {
    text: String,
    /// The hunks at the first accept, in the coordinates of `text`.
    edits: Vec<(Range<usize>, Arc<str>)>,
    transactions: Vec<TransactionId>,
    /// The buffer's version after the last accept.
    after: Version,
    /// Whether anything else edited between accepts.
    interleaved: bool,
    /// Whether the buffer held changes beyond this changeset's hunks when
    /// they were accepted (another proposal's, say): a revert then undoes
    /// the hunks one by one and leaves the rest of the text be.
    foreign: bool,
}

struct File {
    path: PathBuf,
    kind: Kind,
}

pub struct Changeset {
    turn_id: String,
    origin: Origin,
    files: Vec<File>,
    /// What a first accept made by the host's own editors would checkpoint,
    /// per file, taken by [`Changeset::arm`].
    armed: Vec<(PathBuf, Checkpoint)>,
    /// The hunks' statuses per file as last observed, for following a file
    /// whose tab was closed and reopened ([`Changeset::rebind`]).
    memo: Vec<(PathBuf, Vec<fe_buffer::HunkView>)>,
}

impl Changeset {
    pub fn new(turn_id: impl Into<String>, origin: Origin) -> Self {
        Self {
            turn_id: turn_id.into(),
            origin,
            files: Vec::new(),
            armed: Vec::new(),
            memo: Vec::new(),
        }
    }

    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Adds a file the turn proposes edits to, after the files added so far.
    pub fn add_file(
        &mut self,
        path: impl Into<PathBuf>,
        buffer: BufferId,
        proposal: Rc<RefCell<ProposedChange>>,
    ) {
        let last = Some(stored_counts(&proposal.borrow()));
        self.files.push(File {
            path: path.into(),
            kind: Kind::Existing {
                buffer,
                proposal,
                checkpoint: None,
                last,
            },
        });
    }

    /// As [`Changeset::add_file`], also reading the hunk counts from
    /// the buffer in `set`, so they are known even if it later vanishes.
    pub fn add_file_seen(
        &mut self,
        set: &mut impl BufferSet,
        path: impl Into<PathBuf>,
        buffer: BufferId,
        proposal: Rc<RefCell<ProposedChange>>,
    ) {
        let counts = set
            .buffer_mut(buffer)
            .map(|buf| count(&proposal.borrow().hunks(buf.current())));
        self.add_file(path, buffer, proposal);
        if let Some(File {
            kind: Kind::Existing { last, .. },
            ..
        }) = self.files.last_mut()
            && counts.is_some()
        {
            *last = counts;
        }
    }

    /// Adds a file the turn creates, with its whole `text`.
    pub fn add_new_file(&mut self, path: impl Into<PathBuf>, text: impl Into<String>) {
        self.files.push(File {
            path: path.into(),
            kind: Kind::New {
                text: text.into(),
                state: NewState::Pending,
            },
        });
    }

    /// The buffer and the shared proposal of the existing file `path`, for a
    /// host that shows the proposal in the file's editor. `None` for a new
    /// file or one not in the changeset.
    pub fn proposal(&self, path: &Path) -> Option<(BufferId, Rc<RefCell<ProposedChange>>)> {
        self.files.iter().find_map(|file| match &file.kind {
            Kind::Existing {
                buffer, proposal, ..
            } if file.path == path => Some((*buffer, proposal.clone())),
            _ => None,
        })
    }

    /// For a host whose editors accept hunks by themselves: records what the
    /// first accept in `path` would checkpoint, from the buffer as it is now.
    /// Call it before editors can accept, and again after the buffer changed;
    /// then [`Changeset::adopt_accept`] files the accept under the
    /// checkpoint, so [`Changeset::revert`] covers it.
    pub fn arm(&mut self, set: &mut impl BufferSet, path: &Path) {
        let Some(File {
            kind:
                Kind::Existing {
                    buffer,
                    proposal,
                    checkpoint: None,
                    ..
                },
            ..
        }) = self.files.iter().find(|file| file.path == path)
        else {
            return;
        };
        let Some(buf) = set.buffer_mut(*buffer) else {
            return;
        };
        if (self.armed.iter()).any(|(armed, cp)| armed == path && cp.after == buf.version()) {
            return;
        }
        let Some(fresh) = prepare(&None, &proposal.borrow(), buf).fresh else {
            return;
        };
        self.armed.retain(|(armed, _)| armed != path);
        self.armed.push((path.to_path_buf(), fresh));
    }

    /// Files an accept of `transaction` that a host's editor made in `path`
    /// under the checkpoint. Returns whether it could: not if the file was
    /// not [armed](Changeset::arm) before its first accept.
    pub fn adopt_accept(
        &mut self,
        set: &mut impl BufferSet,
        path: &Path,
        hunk: HunkId,
        transaction: Option<TransactionId>,
    ) -> bool {
        let Some(File {
            kind:
                Kind::Existing {
                    buffer,
                    proposal,
                    checkpoint,
                    last,
                },
            ..
        }) = self.files.iter_mut().find(|file| file.path == path)
        else {
            return false;
        };
        let Some(buf) = set.buffer_mut(*buffer) else {
            return false;
        };
        let hunks = proposal.borrow().hunks(buf.current());
        // Only the hunks of this changeset's own proposal: an earlier turn's
        // proposal may still be shown by the editor, and what it changed is
        // not this turn's to revert.
        if !hunks.iter().any(|shown| shown.id == hunk) {
            if let Some(cp) = checkpoint {
                cp.foreign = true;
            }
            return false;
        }
        *last = Some(count(&hunks));
        let fresh = match self.armed.iter().position(|(armed, _)| armed == path) {
            Some(at) => Some(self.armed.remove(at).1),
            None => None,
        };
        if checkpoint.is_none() && fresh.is_none() {
            return false;
        }
        commit(
            checkpoint,
            Prep {
                fresh,
                interleaved: false,
            },
            buf,
            transaction.as_slice(),
            true,
        );
        if let Some(cp) = checkpoint
            && without_accepted(buf, &hunks).is_none_or(|before| before != cp.text)
        {
            cp.foreign = true;
        }
        true
    }

    /// Notes edits made since the last accept, which a revert must ask
    /// about. Call it whenever a buffer may have changed.
    pub fn observe(&mut self, set: &mut impl BufferSet) {
        let mut memo = Vec::new();
        for file in &mut self.files {
            let Kind::Existing {
                buffer,
                proposal,
                checkpoint,
                ..
            } = &mut file.kind
            else {
                continue;
            };
            let Some(buf) = set.buffer_mut(*buffer) else {
                continue;
            };
            if let Some(cp) = checkpoint
                && buf.version() != cp.after
            {
                cp.interleaved = true;
            }
            let hunks = proposal.borrow().hunks(buf.current());
            memo.push((file.path.clone(), hunks));
        }
        for (path, hunks) in memo {
            match self.memo.iter_mut().find(|(known, _)| *known == path) {
                Some(known) => known.1 = hunks,
                None => self.memo.push((path, hunks)),
            }
        }
    }

    /// Follows `path` to `buffer`, a reopened buffer of the file whose
    /// previous one is gone: if it holds the text the file had before any
    /// accept, the proposal is made again on it, every hunk pending.
    ///
    /// # Errors
    /// [`ChangesetError::Conflict`] if `buffer` is missing, or holds other
    /// text. Then nothing changes.
    pub fn rebind(
        &mut self,
        set: &mut impl BufferSet,
        path: &Path,
        buffer: BufferId,
    ) -> Result<(), ChangesetError> {
        let conflict = || ChangesetError::Conflict {
            path: path.to_path_buf(),
        };
        let file = (self.files.iter_mut())
            .find(|file| file.path == path)
            .ok_or_else(|| ChangesetError::NoSuchFile {
                path: path.to_path_buf(),
            })?;
        let Kind::Existing {
            buffer: held,
            proposal,
            checkpoint,
            last,
        } = &mut file.kind
        else {
            return Err(ChangesetError::NotPending {
                path: path.to_path_buf(),
            });
        };
        if *held == buffer {
            return Ok(());
        }
        let buf = set.buffer_mut(buffer).ok_or_else(conflict)?;
        let armed = self.armed.iter().position(|(armed, _)| armed == path);
        let hunks = (self.memo.iter())
            .find(|(known, _)| known == path)
            .map(|(_, hunks)| hunks.clone())
            .ok_or_else(conflict)?;
        // The hunks as they stood when the tab closed must be where they
        // were: the accepted ones with their new text in place, the others
        // with their old text (or one typed by hand that equals the new).
        let now = buf.text().to_string();
        let at = |range: &Range<usize>| now.get(range.clone());
        let in_place = hunks.iter().all(|hunk| match hunk.status {
            HunkStatus::Accepted => at(&hunk.range) == Some(&*hunk.new_text),
            HunkStatus::Stale => true,
            // Whatever the user left in a rejected hunk's place stays.
            HunkStatus::Rejected => at(&hunk.range).is_some(),
            _ => {
                let shown = at(&hunk.range);
                shown == Some(&*hunk.old_text) || shown == Some(&*hunk.new_text)
            }
        });
        // Stale hunks, and hunks that overlap or share an insertion point,
        // can't be given back their identities: refuse before touching
        // the buffer.
        let ordered = hunks.windows(2).all(|pair| {
            let (a, b) = (&pair[0].range, &pair[1].range);
            a.end <= b.start && !(a.is_empty() && b.is_empty() && a.start == b.start)
        });
        let stale = hunks.iter().any(|hunk| hunk.status == HunkStatus::Stale);
        if !in_place || !ordered || stale {
            return Err(conflict());
        }
        // The text before the accepts, and each hunk's range in it.
        let mut before = String::new();
        let mut edits = Vec::new();
        let (mut copied, mut shift) = (0usize, 0isize);
        for hunk in &hunks {
            let range = hunk.range.clone();
            before.push_str(&now[copied..range.start]);
            let start = (range.start as isize + shift) as usize;
            if hunk.status == HunkStatus::Accepted {
                before.push_str(&hunk.old_text);
                edits.push((start..start + hunk.old_text.len(), hunk.new_text.clone()));
                shift += hunk.old_text.len() as isize - range.len() as isize;
            } else {
                before.push_str(&now[range.clone()]);
                edits.push((start..start + range.len(), hunk.new_text.clone()));
            }
            copied = range.end;
        }
        before.push_str(&now[copied..]);
        let accepted = hunks.iter().any(|hunk| hunk.status == HunkStatus::Accepted);
        if accepted {
            // Back to the text before the accepts, to accept them again.
            let len = buf.len();
            buf.with_transaction_aside(|buf| buf.edit([(0..len, before.as_str())], Origin::User));
        }
        let mut fresh = ProposedChange::from_edits_keeping_noops(
            buf.current(),
            edits,
            proposal.borrow().origin().clone(),
        );
        let views = fresh.hunks(buf.current());
        let mut transactions = Vec::new();
        for (view, old) in views.iter().zip(&hunks) {
            match old.status {
                HunkStatus::Rejected => drop(fresh.reject(view.id)),
                HunkStatus::Accepted => {
                    // A hunk with nothing to change is accepted all the same.
                    transactions.extend(fresh.accept_as(buf, view.id, Origin::User).ok());
                }
                _ => {}
            }
        }
        *last = Some(count(&fresh.hunks(buf.current())));
        *proposal.borrow_mut() = fresh;
        *held = buffer;
        let version = buf.version();
        match checkpoint {
            Some(cp) => {
                // Text outside the hunks, saved, is still to be asked about.
                cp.interleaved |= before != cp.text;
                cp.transactions = transactions;
                cp.after = version;
            }
            None => {
                if let Some(at) = armed {
                    let mut kept = self.armed.remove(at).1;
                    // Armed on other text: armed again at the next sync.
                    if before == kept.text {
                        kept.after = version;
                        self.armed.push((path.to_path_buf(), kept));
                    }
                }
            }
        }
        Ok(())
    }

    /// The whole text of the new file `path`, if it is one.
    pub fn new_file_text(&self, path: &Path) -> Option<&str> {
        self.files.iter().find_map(|file| match &file.kind {
            Kind::New { text, .. } if file.path == path => Some(text.as_str()),
            _ => None,
        })
    }

    /// The files in insertion order with their counts.
    pub fn files(&mut self, set: &mut impl BufferSet) -> Vec<FileEntry> {
        self.files
            .iter_mut()
            .map(|file| FileEntry {
                path: file.path.clone(),
                is_new: matches!(file.kind, Kind::New { .. }),
                missing: file.is_missing(set),
                counts: file.counts(set),
                counts_known: file.counts_known(),
            })
            .collect()
    }

    pub fn summary(&mut self, set: &mut impl BufferSet) -> Summary {
        let mut summary = Summary::default();
        for file in &mut self.files {
            summary.files += 1;
            summary.counts.add(file.counts(set));
        }
        summary
    }

    /// Whether no hunk of any file is pending any more.
    pub fn is_resolved(&self) -> bool {
        self.files.iter().all(|file| match &file.kind {
            Kind::Existing { proposal, .. } => proposal.borrow().is_resolved(),
            Kind::New { state, .. } => *state != NewState::Pending,
        })
    }

    /// Accepts one pending hunk of `path`. The transaction is `None` for a
    /// hunk that changes nothing (a restored deletion already done); it is
    /// accepted all the same.
    ///
    /// # Errors
    /// [`ChangesetError::Conflict`] if the buffer is missing or the hunk's
    /// text moved, [`ChangesetError::NotPending`] if it is decided or unknown.
    pub fn accept_hunk(
        &mut self,
        set: &mut impl BufferSet,
        path: &Path,
        hunk: HunkId,
    ) -> Result<Option<TransactionId>, ChangesetError> {
        let file = self.file_mut(path)?;
        let Kind::Existing {
            buffer,
            proposal,
            checkpoint,
            last,
        } = &mut file.kind
        else {
            return Err(ChangesetError::NotPending {
                path: file.path.clone(),
            });
        };
        let conflict = || ChangesetError::Conflict {
            path: path.to_path_buf(),
        };
        let buf = set.buffer_mut(*buffer).ok_or_else(conflict)?;
        let mut proposal = proposal.borrow_mut();
        let prep = prepare(checkpoint, &proposal, buf);
        let result = proposal.accept_as(buf, hunk, Origin::User);
        *last = Some(count(&proposal.hunks(buf.current())));
        match result {
            Ok(id) => {
                commit(checkpoint, prep, buf, &[id], true);
                Ok(Some(id))
            }
            // Accepted, but it changed nothing: no transaction, yet the
            // checkpoint keeps the hunk so a revert rebuilds it pending.
            Err(ProposalError::NoChange) => {
                commit(checkpoint, prep, buf, &[], true);
                Ok(None)
            }
            Err(ProposalError::Stale) => Err(conflict()),
            Err(ProposalError::NotPending) => Err(ChangesetError::NotPending {
                path: path.to_path_buf(),
            }),
        }
    }

    /// Rejects one hunk of `path`; the buffer is untouched.
    ///
    /// # Errors
    /// [`ChangesetError::Conflict`] if the buffer is missing (nothing changes),
    /// [`ChangesetError::NotPending`] if it is decided or unknown.
    pub fn reject_hunk(
        &mut self,
        set: &mut impl BufferSet,
        path: &Path,
        hunk: HunkId,
    ) -> Result<(), ChangesetError> {
        let file = self.file_mut(path)?;
        match &mut file.kind {
            Kind::Existing {
                buffer,
                proposal,
                last,
                ..
            } => {
                let buf = set.buffer_mut(*buffer).ok_or_else(|| conflict_for(path))?;
                let mut proposal = proposal.borrow_mut();
                let result = proposal
                    .reject(hunk)
                    .map_err(|_| ChangesetError::NotPending {
                        path: path.to_path_buf(),
                    });
                *last = Some(count(&proposal.hunks(buf.current())));
                result
            }
            Kind::New { .. } => Err(ChangesetError::NotPending {
                path: path.to_path_buf(),
            }),
        }
    }

    /// Accepts every pending hunk of `path`, or none: a file with a stale
    /// hunk or a missing buffer is a conflict and is left as it was.
    ///
    /// # Errors
    /// [`ChangesetError::Conflict`], [`ChangesetError::NoSuchFile`].
    pub fn accept_file(
        &mut self,
        set: &mut impl BufferSet,
        path: &Path,
    ) -> Result<Vec<TransactionId>, ChangesetError> {
        let file = self.file_mut(path)?;
        file.accept(set)
    }

    /// Rejects every pending hunk of `path`.
    ///
    /// # Errors
    /// [`ChangesetError::NoSuchFile`]; [`ChangesetError::Conflict`] if the
    /// buffer is missing, since the hunks can't be listed without it.
    pub fn reject_file(
        &mut self,
        set: &mut impl BufferSet,
        path: &Path,
    ) -> Result<(), ChangesetError> {
        self.file_mut(path)?.reject(set)
    }

    /// Accepts every file, carrying on past a conflicting one. One result per
    /// file, in order.
    pub fn accept_all(
        &mut self,
        set: &mut impl BufferSet,
    ) -> Vec<(PathBuf, Result<Vec<TransactionId>, ChangesetError>)> {
        self.files
            .iter_mut()
            .map(|file| (file.path.clone(), file.accept(set)))
            .collect()
    }

    /// Rejects every pending hunk of every file; no text is touched. A file
    /// whose buffer is missing reports a conflict and the rest carry on.
    pub fn reject_all(
        &mut self,
        set: &mut impl BufferSet,
    ) -> Vec<(PathBuf, Result<(), ChangesetError>)> {
        self.files
            .iter_mut()
            .map(|file| (file.path.clone(), file.reject(set)))
            .collect()
    }

    /// Undoes what this changeset accepted, per file; see the module docs.
    /// Files with nothing accepted are left out.
    pub fn revert(&mut self, set: &mut impl BufferSet) -> Vec<RevertOutcome> {
        self.revert_with(set, false)
    }

    /// As [`Changeset::revert`], but a file edited since is restored too.
    pub fn revert_forced(&mut self, set: &mut impl BufferSet) -> Vec<RevertOutcome> {
        self.revert_with(set, true)
    }

    fn revert_with(&mut self, set: &mut impl BufferSet, force: bool) -> Vec<RevertOutcome> {
        let mut out = Vec::new();
        for file in &mut self.files {
            let path = file.path.clone();
            match &mut file.kind {
                Kind::Existing {
                    buffer,
                    proposal,
                    checkpoint,
                    last,
                } => {
                    let Some(cp) = checkpoint.as_ref() else {
                        continue;
                    };
                    let Some(buf) = set.buffer_mut(*buffer) else {
                        out.push(RevertOutcome::Conflict { path });
                        continue;
                    };
                    if cp.foreign {
                        // Other changes are in the text: undo this turn's
                        // hunks and leave the rest be.
                        let hunks = proposal.borrow().hunks(buf.current());
                        let accepted: Vec<_> = (hunks.iter())
                            .filter(|hunk| hunk.status == HunkStatus::Accepted)
                            .collect();
                        let undo: Vec<_> = (accepted.iter().copied())
                            .filter(|hunk| {
                                buf.text().slice_to_string(hunk.range.clone()) == *hunk.new_text
                            })
                            .collect();
                        // A hunk edited since can't be undone by itself.
                        if undo.len() != accepted.len() && !force {
                            out.push(RevertOutcome::NeedsConfirm { path });
                            continue;
                        }
                        if !undo.is_empty() {
                            let edits: Vec<_> = (undo.iter())
                                .map(|hunk| (hunk.range.clone(), hunk.old_text.clone()))
                                .collect();
                            buf.with_transaction_aside(|buf| buf.edit(edits, Origin::User));
                        }
                        for hunk in &undo {
                            proposal.borrow_mut().reset_to_pending(hunk.id).ok();
                        }
                        *last = Some(count(&proposal.borrow().hunks(buf.current())));
                        if undo.len() == accepted.len() {
                            *checkpoint = None;
                            out.push(RevertOutcome::Reverted {
                                path,
                                reapplied: true,
                            });
                        } else {
                            // What can't be undone keeps the checkpoint.
                            out.push(RevertOutcome::Conflict { path });
                        }
                        continue;
                    }
                    let clean = !cp.interleaved && buf.version() == cp.after;
                    if !clean && !force {
                        out.push(RevertOutcome::NeedsConfirm { path });
                        continue;
                    }
                    if buf.text().to_string() != cp.text {
                        let len = buf.len();
                        buf.with_transaction_aside(|buf| {
                            buf.edit([(0..len, cp.text.as_str())], Origin::User)
                        });
                    }
                    let cp = checkpoint.take().expect("checked above");
                    let fresh = ProposedChange::from_edits_keeping_noops(
                        buf.current(),
                        cp.edits,
                        proposal.borrow().origin().clone(),
                    );
                    *last = Some(count(&fresh.hunks(buf.current())));
                    *proposal.borrow_mut() = fresh;
                    out.push(RevertOutcome::Reverted {
                        path,
                        reapplied: true,
                    });
                }
                Kind::New { state, .. } => {
                    let NewState::Accepted { buffer, version } = *state else {
                        continue;
                    };
                    let edited = set
                        .buffer_mut(buffer)
                        .is_some_and(|buf| buf.version() != version);
                    if edited && !force {
                        out.push(RevertOutcome::NeedsConfirm { path });
                        continue;
                    }
                    if !set.remove(buffer) && set.buffer_mut(buffer).is_some() {
                        out.push(RevertOutcome::Conflict { path });
                        continue;
                    }
                    *state = NewState::Pending;
                    out.push(RevertOutcome::Reverted {
                        path,
                        reapplied: false,
                    });
                }
            }
        }
        out
    }

    fn file_mut(&mut self, path: &Path) -> Result<&mut File, ChangesetError> {
        self.files
            .iter_mut()
            .find(|file| file.path == path)
            .ok_or_else(|| ChangesetError::NoSuchFile {
                path: path.to_path_buf(),
            })
    }
}

impl File {
    fn is_missing(&self, set: &mut impl BufferSet) -> bool {
        match &self.kind {
            Kind::Existing { buffer, .. } => set.buffer_mut(*buffer).is_none(),
            Kind::New { state, .. } => match *state {
                NewState::Accepted { buffer, .. } => set.buffer_mut(buffer).is_none(),
                _ => false,
            },
        }
    }

    fn counts_known(&self) -> bool {
        match &self.kind {
            Kind::Existing { last, .. } => last.is_some(),
            Kind::New { .. } => true,
        }
    }

    fn counts(&mut self, set: &mut impl BufferSet) -> Counts {
        match &mut self.kind {
            Kind::New { state, .. } => {
                let mut counts = Counts {
                    hunks: 1,
                    ..Counts::default()
                };
                match state {
                    NewState::Pending => counts.pending = 1,
                    NewState::Accepted { .. } => counts.accepted = 1,
                    NewState::Rejected => counts.rejected = 1,
                }
                counts
            }
            Kind::Existing {
                buffer,
                proposal,
                last,
                ..
            } => {
                if let Some(buf) = set.buffer_mut(*buffer) {
                    *last = Some(count(&proposal.borrow().hunks(buf.current())));
                }
                last.unwrap_or_default()
            }
        }
    }

    fn accept(&mut self, set: &mut impl BufferSet) -> Result<Vec<TransactionId>, ChangesetError> {
        let conflict = ChangesetError::Conflict {
            path: self.path.clone(),
        };
        match &mut self.kind {
            Kind::New { text, state } => match *state {
                NewState::Pending => {
                    let id = set.create(&self.path, text).ok_or(conflict)?;
                    let buf = set.buffer_mut(id).ok_or(conflict_for(&self.path))?;
                    *state = NewState::Accepted {
                        buffer: id,
                        version: buf.version(),
                    };
                    Ok(Vec::new())
                }
                NewState::Accepted { buffer, .. } => {
                    if set.buffer_mut(buffer).is_none() {
                        return Err(conflict_for(&self.path));
                    }
                    Ok(Vec::new())
                }
                NewState::Rejected => Ok(Vec::new()),
            },
            Kind::Existing {
                buffer,
                proposal,
                checkpoint,
                last,
            } => {
                let buf = set.buffer_mut(*buffer).ok_or(conflict)?;
                let mut proposal = proposal.borrow_mut();
                if proposal
                    .hunks(buf.current())
                    .iter()
                    .any(|hunk| hunk.status == HunkStatus::Stale)
                {
                    *last = Some(count(&proposal.hunks(buf.current())));
                    return Err(conflict_for(&self.path));
                }
                let prep = prepare(checkpoint, &proposal, buf);
                let before = proposal.status_count(HunkStatus::Accepted);
                let (id, _) = proposal.accept_all_as(buf, Origin::User);
                let ids: Vec<_> = id.into_iter().collect();
                let accepted = proposal.status_count(HunkStatus::Accepted) > before;
                *last = Some(count(&proposal.hunks(buf.current())));
                commit(checkpoint, prep, buf, &ids, accepted);
                Ok(ids)
            }
        }
    }

    fn reject(&mut self, set: &mut impl BufferSet) -> Result<(), ChangesetError> {
        match &mut self.kind {
            Kind::New { state, .. } => match *state {
                NewState::Pending => {
                    *state = NewState::Rejected;
                    Ok(())
                }
                NewState::Accepted { buffer, .. } if set.buffer_mut(buffer).is_none() => {
                    Err(conflict_for(&self.path))
                }
                _ => Ok(()),
            },
            Kind::Existing {
                buffer,
                proposal,
                last,
                ..
            } => {
                let buf = set
                    .buffer_mut(*buffer)
                    .ok_or_else(|| conflict_for(&self.path))?;
                let mut proposal = proposal.borrow_mut();
                for hunk in proposal.hunks(buf.current()) {
                    if matches!(hunk.status, HunkStatus::Pending | HunkStatus::Stale) {
                        let _ = proposal.reject(hunk.id);
                    }
                }
                *last = Some(count(&proposal.hunks(buf.current())));
                Ok(())
            }
        }
    }
}

fn conflict_for(path: &Path) -> ChangesetError {
    ChangesetError::Conflict {
        path: path.to_path_buf(),
    }
}

/// The counts as the proposal last recorded them, without a snapshot.
fn stored_counts(proposal: &ProposedChange) -> Counts {
    Counts {
        hunks: proposal.hunk_count(),
        pending: proposal.status_count(HunkStatus::Pending),
        accepted: proposal.status_count(HunkStatus::Accepted),
        rejected: proposal.status_count(HunkStatus::Rejected),
        stale: proposal.status_count(HunkStatus::Stale),
    }
}

fn count(hunks: &[fe_buffer::HunkView]) -> Counts {
    let mut counts = Counts {
        hunks: hunks.len(),
        ..Counts::default()
    };
    for hunk in hunks {
        match hunk.status {
            HunkStatus::Pending => counts.pending += 1,
            HunkStatus::Accepted => counts.accepted += 1,
            HunkStatus::Rejected => counts.rejected += 1,
            HunkStatus::Stale => counts.stale += 1,
        }
    }
    counts
}

/// What an accept would add to the checkpoint, made before it and kept only
/// if the accept applied something.
struct Prep {
    fresh: Option<Checkpoint>,
    /// Whether anything edited since the last accept.
    interleaved: bool,
}

fn prepare(checkpoint: &Option<Checkpoint>, proposal: &ProposedChange, buf: &Buffer) -> Prep {
    match checkpoint {
        Some(cp) => Prep {
            fresh: None,
            interleaved: buf.version() != cp.after,
        },
        None => {
            let mut edits: Vec<(Range<usize>, Arc<str>)> = Vec::new();
            for hunk in proposal.hunks(buf.current()) {
                // Stale and rejected hunks are kept: after the restore they
                // are pending again.
                if hunk.status == HunkStatus::Accepted {
                    continue;
                }
                // User edits can merge two insertion points into one offset;
                // `from_edits` takes those as one insertion, in text order.
                match edits.last_mut() {
                    Some((range, text)) if range.start == range.end && *range == hunk.range => {
                        *text = format!("{text}{}", hunk.new_text).into();
                    }
                    _ => edits.push((hunk.range, hunk.new_text)),
                }
            }
            Prep {
                fresh: Some(Checkpoint {
                    text: buf.text().to_string(),
                    edits,
                    transactions: Vec::new(),
                    after: buf.version(),
                    interleaved: false,
                    foreign: false,
                }),
                interleaved: false,
            }
        }
    }
}

/// Records an accept that applied `ids`, or that accepted hunks changing
/// nothing (`accepted`); nothing if it accepted nothing.
fn commit(
    checkpoint: &mut Option<Checkpoint>,
    prep: Prep,
    buf: &Buffer,
    ids: &[TransactionId],
    accepted: bool,
) {
    if ids.is_empty() && !accepted {
        return;
    }
    if checkpoint.is_none() {
        *checkpoint = prep.fresh;
    }
    if let Some(cp) = checkpoint {
        cp.interleaved |= prep.interleaved;
        cp.transactions.extend_from_slice(ids);
        cp.after = buf.version();
    }
}

/// The text of `buf` with the accepted `hunks` put back as they were, or
/// `None` if one's new text is not where it landed.
fn without_accepted(buf: &Buffer, hunks: &[fe_buffer::HunkView]) -> Option<String> {
    let text = buf.text().to_string();
    let mut out = String::new();
    let mut at = 0;
    for hunk in hunks
        .iter()
        .filter(|hunk| hunk.status == HunkStatus::Accepted)
    {
        let range = hunk.range.clone();
        if range.start < at || text.get(range.clone()) != Some(&*hunk.new_text) {
            return None;
        }
        out.push_str(&text[at..range.start]);
        out.push_str(&hunk.old_text);
        at = range.end;
    }
    out.push_str(&text[at..]);
    Some(out)
}
