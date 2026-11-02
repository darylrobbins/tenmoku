//! A language server's workspace edit (a rename's or a code action's) as a
//! preview to review before it applies: the files it edits as an AI
//! changeset's are, each hunk kept or left out, and the edit left to apply.
//!
//! - **When.** [`RefactorPreview::needs_preview`]: an edit that changes
//!   only the buffer it was asked in applies at once, as before. One that
//!   edits another file, open or closed, or creates, renames or deletes
//!   one, is previewed.
//! - **Scratch copies.** Each text-edited file is listed once, in the
//!   edit's order, over a scratch [`Buffer`] of its text now, owned by the
//!   preview (the host gives the text: an open buffer's, or the file's as
//!   [`loader::read`](crate::loader::read) reads it). A closed file's edits
//!   are decoded against it ([`decode_closed`]). Its hunks are the server's
//!   edits in text order, one each, kept even if it changes nothing, but
//!   inserts at one offset are one hunk, decided together, as applying
//!   joins them. Nothing outside the preview changes: accepting a hunk
//!   applies it to the scratch copy only.
//! - **Decisions.** The files go into an [`fe_editor::Changeset`] of
//!   [`Origin::Lsp`], so listing, counts and accepting or rejecting hunks,
//!   files or everything are the changeset's, as the AI Changes panel
//!   shows them.
//! - **The edit left.** [`RefactorPreview::selected`]: each entry, in
//!   order, with only the edits whose hunk isn't rejected (pending ones
//!   apply, as accepted ones do); an entry with every hunk rejected is
//!   left out.
//! - **Whole only.** An edit that creates, renames or deletes a file, or
//!   has edits that depend on whether one is made
//!   ([`EditTarget::Depends`]), is shown but decided whole: nothing in it
//!   can be accepted or rejected ([`PreviewError::NotDecidable`]) and
//!   [`RefactorPreview::selected`] is the edit as it came. Its files are
//!   shown as the edit would leave them, its operations made in order: a
//!   created (or overwritten) file's text is empty, a deleted one's gone,
//!   and a renamed one's moves to its new path, replacing what was there;
//!   an open buffer's later runs move past its earlier ones, as applying
//!   moves them, whatever path it has by then. Dependent edits are shown
//!   the way their operation goes if it's made (or skipped, if they can't
//!   apply made). A path edited in several runs, around operations, is
//!   shown as a line diff of its text before the first and after the last.

use std::{
    cell::RefCell,
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::{Buffer, BufferId, HunkId, HunkStatus, HunkView, Origin, ProposedChange, Text};
use fe_editor::{BufferSet, Changeset, ChangesetError, FileEntry, Summary};
use fe_lsp::{EditTarget, FileEdits, decode_closed};

use super::rebase;

/// Why a preview's hunk or file can't be decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PreviewError {
    /// The edit makes file operations, so it's decided whole.
    NotDecidable,
    /// The changeset refused it.
    Changeset(ChangesetError),
}

impl std::fmt::Display for PreviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotDecidable => f.write_str("this edit applies whole or not at all"),
            Self::Changeset(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PreviewError {}

impl From<ChangesetError> for PreviewError {
    fn from(error: ChangesetError) -> Self {
        Self::Changeset(error)
    }
}

/// The scratch buffers of a preview, which its changeset reaches.
#[derive(Default)]
struct Scratch {
    buffers: Vec<Buffer>,
}

impl BufferSet for Scratch {
    fn buffer_mut(&mut self, id: BufferId) -> Option<&mut Buffer> {
        self.buffers.iter_mut().find(|buffer| buffer.id() == id)
    }
}

/// One listed file of a decidable preview: its entry in the edit, and each
/// hunk with the indices of the entry's edits it holds, in text order.
struct Listed {
    entry: usize,
    buffer: BufferId,
    proposal: Rc<RefCell<ProposedChange>>,
    hunks: Vec<(HunkId, Vec<usize>)>,
}

/// A workspace edit to review before applying: see the [module](self).
pub(crate) struct RefactorPreview {
    edits: Vec<FileEdits>,
    changeset: Changeset,
    scratch: Scratch,
    /// The files listed, for [`RefactorPreview::selected`]; empty for a
    /// whole-only edit.
    listed: Vec<Listed>,
    decidable: bool,
}

/// Edits at one place of a file, decided together: their range, the text
/// replacing it, and their indices in the file's entry.
type Group = (Range<usize>, String, Vec<usize>);

/// The edits one run made to a file, as made: each its range and the
/// length of what replaced it.
type Made = Vec<(Range<usize>, usize)>;

/// One result per file, in order.
type FileResults = Vec<(PathBuf, Result<(), ChangesetError>)>;

/// A listed file: what the edit's text edits did to it.
struct Record {
    /// Its text before the edit's first text edit to it.
    base: String,
    /// Its text after the last.
    text: String,
    /// How many entries edited it.
    entries: usize,
    /// The first entry's edits, merged, each with the indices of the
    /// entry's edits it holds, and that entry.
    first: (usize, Vec<Group>),
}

impl RefactorPreview {
    /// Whether `edits`, asked for in `source`, are to be previewed: they
    /// change any file but `source`, or create, rename or delete one.
    pub(crate) fn needs_preview(edits: &[FileEdits], source: BufferId) -> bool {
        edits.iter().any(|file| match &file.target {
            EditTarget::Open { buffer, .. } => *buffer != source,
            _ => true,
        })
    }

    /// `edits` previewed under `title`, each text-edited file's text now
    /// given by `text_of`, called once per file with its path and the
    /// entry's target ([`EditTarget::Open`] or [`EditTarget::Closed`]); a
    /// file the edit creates, or renames to, isn't asked for (a renamed one
    /// is asked for by its old path). Why it can't be: a file's text can't
    /// be had, or the edits don't fit it (out of it, between the bytes of a
    /// character, overlapping).
    pub(crate) fn new(
        title: impl Into<String>,
        edits: Vec<FileEdits>,
        mut text_of: impl FnMut(&Path, &EditTarget) -> Result<String, String>,
    ) -> Result<Self, String> {
        let mut decidable = edits.iter().all(|file| {
            matches!(
                file.target,
                EditTarget::Open { .. } | EditTarget::Closed { .. }
            )
        });
        // What each path holds as the edit goes, once an entry has touched
        // it: operations change it as they'd change the file.
        let mut live: HashMap<PathBuf, String> = HashMap::new();
        // Each open buffer's text as the edit leaves it, and the runs made
        // to it, which a later run is moved past, as applying does.
        let mut buffers: HashMap<BufferId, (String, Vec<Made>)> = HashMap::new();
        let mut renames: Vec<(PathBuf, PathBuf)> = Vec::new();
        let mut order: Vec<PathBuf> = Vec::new();
        let mut records: HashMap<PathBuf, Record> = HashMap::new();
        for (entry, file) in edits.iter().enumerate() {
            let path = &file.path;
            let target = match &file.target {
                EditTarget::Create { .. } => {
                    // Made, or emptied if it's overwritten.
                    live.insert(path.clone(), String::new());
                    continue;
                }
                // To the same path: nothing to do, as applying has it.
                EditTarget::Rename { to, .. } if to == path => continue,
                EditTarget::Rename { to, .. } => {
                    // What was there is replaced (an overwrite), and what
                    // was under the old path moves.
                    live.retain(|known, _| !known.starts_with(to));
                    let moved: Vec<_> = (live.keys())
                        .filter(|known| known.starts_with(path))
                        .cloned()
                        .collect();
                    for known in moved {
                        let text = live.remove(&known).unwrap_or_default();
                        let rest = known.strip_prefix(path).unwrap_or(Path::new(""));
                        live.insert(joined(to, rest), text);
                    }
                    renames.push((path.clone(), to.clone()));
                    continue;
                }
                EditTarget::Delete { .. } => {
                    live.retain(|known, _| !known.starts_with(path));
                    continue;
                }
                EditTarget::Depends { .. } => match shown(&file.target) {
                    Some(target) => target,
                    None => continue,
                },
                target => target.clone(),
            };
            let display = path.display();
            let (current, decoded) = match &target {
                EditTarget::Open { buffer, edits, .. } => {
                    let (current, runs) = match buffers.get(buffer) {
                        Some((text, runs)) => (text.clone(), &runs[..]),
                        None => (normalized(&text_of(path, &target)?), &[][..]),
                    };
                    let edits = rebase(edits.clone(), runs)
                        .ok_or_else(|| format!("{display}: its edits overlap"))?;
                    (current, edits)
                }
                EditTarget::Closed { edits, encoding } => {
                    let current = match live.get(path) {
                        Some(text) => text.clone(),
                        None => normalized(&text_of(&origin(path, &renames), &target)?),
                    };
                    let decoded = decode_closed(&Text::new(&current), edits, *encoding)
                        .map_err(|why| format!("{display}: {why}"))?;
                    // Applying a closed file's edits normalizes each one's
                    // line endings before it joins inserts at one place.
                    let decoded = (decoded.into_iter())
                        .map(|(range, new)| (range, normalized(&new)))
                        .collect();
                    (current, decoded)
                }
                _ => unreachable!("a text edit"),
            };
            // Each decoded edit's index in the entry.
            let indices = match &target {
                EditTarget::Closed { edits, .. } => closed_order(edits),
                _ => (0..decoded.len()).collect(),
            };
            let grouped = grouped(&Text::new(&current), decoded, indices)
                .map_err(|why| format!("{display}: {why}"))?;
            let (after, made) = applied(&current, &grouped);
            if let EditTarget::Open { buffer, .. } = &target {
                let (text, runs) = buffers.entry(*buffer).or_default();
                text.clone_from(&after);
                runs.push(made);
            }
            live.insert(path.clone(), after.clone());
            match records.get_mut(path) {
                Some(record) => {
                    // Edited again, around an operation: decided whole.
                    decidable = false;
                    record.text = after;
                    record.entries += 1;
                }
                None => {
                    order.push(path.clone());
                    records.insert(
                        path.clone(),
                        Record {
                            base: current,
                            text: after,
                            entries: 1,
                            first: (entry, grouped),
                        },
                    );
                }
            }
        }

        let mut changeset = Changeset::new(title, Origin::Lsp);
        let mut scratch = Scratch::default();
        let mut listed = Vec::new();
        for path in order {
            let Record {
                base,
                text,
                entries,
                first: (entry, grouped),
            } = records.remove(&path).expect("listed");
            let buffer = Buffer::new(&base);
            let (proposal, hunks) = if entries == 1 {
                let proposal = ProposedChange::from_edits_keeping_noops(
                    buffer.current(),
                    (grouped.iter()).map(|(range, new, _)| (range.clone(), new.as_str())),
                    Origin::Lsp,
                );
                // Checked to fit and be apart, the groups are its hunks.
                debug_assert_eq!(proposal.hunk_count(), grouped.len());
                let ids = proposal
                    .hunks(buffer.current())
                    .into_iter()
                    .map(|hunk| hunk.id);
                let hunks: Vec<_> = ids
                    .zip(grouped.into_iter().map(|(_, _, held)| held))
                    .collect();
                (proposal, hunks)
            } else {
                let all = 0..buffer.len();
                let proposal = ProposedChange::from_text(buffer.current(), all, &text, Origin::Lsp);
                (proposal, Vec::new())
            };
            let proposal = Rc::new(RefCell::new(proposal));
            let id = buffer.id();
            scratch.buffers.push(buffer);
            changeset.add_file_seen(&mut scratch, path, id, proposal.clone());
            listed.push(Listed {
                entry,
                buffer: id,
                proposal,
                hunks,
            });
        }
        if !decidable {
            listed.clear();
        }
        Ok(Self {
            edits,
            changeset,
            scratch,
            listed,
            decidable,
        })
    }

    /// The preview's title, the changeset's turn id.
    pub(crate) fn title(&self) -> &str {
        self.changeset.turn_id()
    }

    /// Whether its hunks and files can be accepted or rejected one by one:
    /// not if the edit makes file operations.
    pub(crate) fn is_decidable(&self) -> bool {
        self.decidable
    }

    /// The edit as it came, in order.
    pub(crate) fn edits(&self) -> &[FileEdits] {
        &self.edits
    }

    /// The changeset the preview's files are in.
    pub(crate) fn changeset(&self) -> &Changeset {
        &self.changeset
    }

    /// The files in the edit's order, with their counts.
    pub(crate) fn files(&mut self) -> Vec<FileEntry> {
        self.changeset.files(&mut self.scratch)
    }

    pub(crate) fn summary(&mut self) -> Summary {
        self.changeset.summary(&mut self.scratch)
    }

    /// `path`'s scratch buffer, if it's listed.
    pub(crate) fn buffer(&self, path: &Path) -> Option<&Buffer> {
        let (id, _) = self.changeset.proposal(path)?;
        self.scratch.buffers.iter().find(|buffer| buffer.id() == id)
    }

    /// `path`'s hunks as they stand, in text order; none if it isn't listed.
    pub(crate) fn hunks(&self, path: &Path) -> Vec<HunkView> {
        let Some((id, proposal)) = self.changeset.proposal(path) else {
            return Vec::new();
        };
        let buffer = self.scratch.buffers.iter().find(|buffer| buffer.id() == id);
        buffer.map_or_else(Vec::new, |buffer| proposal.borrow().hunks(buffer.current()))
    }

    /// Keeps one pending hunk of `path`.
    pub(crate) fn accept_hunk(&mut self, path: &Path, hunk: HunkId) -> Result<(), PreviewError> {
        self.decidable()?;
        let set = &mut self.scratch;
        self.changeset.accept_hunk(set, path, hunk)?;
        Ok(())
    }

    /// Leaves one hunk of `path` out.
    pub(crate) fn reject_hunk(&mut self, path: &Path, hunk: HunkId) -> Result<(), PreviewError> {
        self.decidable()?;
        Ok(self.changeset.reject_hunk(&mut self.scratch, path, hunk)?)
    }

    /// Keeps every pending hunk of `path`.
    pub(crate) fn accept_file(&mut self, path: &Path) -> Result<(), PreviewError> {
        self.decidable()?;
        self.changeset.accept_file(&mut self.scratch, path)?;
        Ok(())
    }

    /// Leaves every pending hunk of `path` out.
    pub(crate) fn reject_file(&mut self, path: &Path) -> Result<(), PreviewError> {
        self.decidable()?;
        Ok(self.changeset.reject_file(&mut self.scratch, path)?)
    }

    /// Keeps every pending hunk; one result per file, in order.
    pub(crate) fn accept_all(&mut self) -> Result<FileResults, PreviewError> {
        self.decidable()?;
        let results = self.changeset.accept_all(&mut self.scratch);
        Ok((results.into_iter())
            .map(|(path, result)| (path, result.map(drop)))
            .collect())
    }

    /// Leaves every pending hunk out; one result per file, in order.
    pub(crate) fn reject_all(&mut self) -> Result<FileResults, PreviewError> {
        self.decidable()?;
        Ok(self.changeset.reject_all(&mut self.scratch))
    }

    /// The edit to apply: each entry in order, with the edits whose hunk
    /// isn't rejected, an entry with none left out. The edit as it came if
    /// it's whole only, or nothing was rejected.
    pub(crate) fn selected(&mut self) -> Vec<FileEdits> {
        if !self.decidable {
            return self.edits.clone();
        }
        let mut selected = Vec::with_capacity(self.edits.len());
        let mut listed = self.listed.iter().peekable();
        for (entry, file) in self.edits.iter().enumerate() {
            let Some(shown) = listed.next_if(|shown| shown.entry == entry) else {
                selected.push(file.clone());
                continue;
            };
            let Some(buffer) = self.scratch.buffers.iter().find(|b| b.id() == shown.buffer) else {
                selected.push(file.clone());
                continue;
            };
            let statuses: HashMap<HunkId, HunkStatus> = (shown.proposal.borrow())
                .hunks(buffer.current())
                .into_iter()
                .map(|hunk| (hunk.id, hunk.status))
                .collect();
            let mut kept: Vec<usize> = (shown.hunks.iter())
                .filter(|(id, _)| statuses.get(id) != Some(&HunkStatus::Rejected))
                .flat_map(|(_, held)| held.iter().copied())
                .collect();
            if kept.is_empty() && !shown.hunks.is_empty() {
                continue;
            }
            kept.sort_unstable();
            let mut file = file.clone();
            match &mut file.target {
                EditTarget::Open { edits, .. } => *edits = keep(edits, &kept),
                EditTarget::Closed { edits, .. } => *edits = keep(edits, &kept),
                _ => {}
            }
            selected.push(file);
        }
        selected
    }

    fn decidable(&self) -> Result<(), PreviewError> {
        if self.decidable {
            Ok(())
        } else {
            Err(PreviewError::NotDecidable)
        }
    }
}

/// The edits of `edits` at `kept`, in order.
fn keep<T: Clone>(edits: &[T], kept: &[usize]) -> Vec<T> {
    kept.iter().map(|&at| edits[at].clone()).collect()
}

/// `target`, a [`EditTarget::Depends`], as shown: the way its operation
/// goes if it's made, or skipped if the edits can't apply made. `None` if
/// they can't apply either way.
fn shown(target: &EditTarget) -> Option<EditTarget> {
    let mut target = target.clone();
    loop {
        let EditTarget::Depends { made, skipped, .. } = target else {
            return Some(target);
        };
        target = match (&*made, &*skipped) {
            (Ok(made), _) => made.clone(),
            (_, Ok(skipped)) => skipped.clone(),
            _ => return None,
        };
    }
}

/// `path` where it was before the edit's `renames`, in order, moved it.
pub(crate) fn origin(path: &Path, renames: &[(PathBuf, PathBuf)]) -> PathBuf {
    let mut path = path.to_path_buf();
    for (from, to) in renames.iter().rev() {
        if let Ok(rest) = path.strip_prefix(to) {
            path = joined(from, rest);
        }
    }
    path
}

fn joined(dir: &Path, rest: &Path) -> PathBuf {
    if rest.as_os_str().is_empty() {
        dir.to_path_buf()
    } else {
        dir.join(rest)
    }
}

/// `text` with `\n` line endings only, as a buffer holds it.
fn normalized(text: &str) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_owned()
    }
}

/// The indices of a closed file's `edits` in the order [`decode_closed`]
/// returns them.
fn closed_order(edits: &[lsp_types::TextEdit]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..edits.len()).collect();
    order.sort_by_key(|&at| (edits[at].range.start, edits[at].range.end));
    order
}

/// `edits` of `text`, `indices` their indices in the entry, in text order
/// (inserts at one place in the server's order), checked to fit it and be
/// apart, with inserts at one place joined into one, each with the indices
/// it holds.
fn grouped(
    text: &Text,
    edits: Vec<(Range<usize>, String)>,
    indices: Vec<usize>,
) -> Result<Vec<Group>, String> {
    let mut indexed: Vec<_> = indices.into_iter().zip(edits).collect();
    indexed.sort_by_key(|(_, (range, _))| (range.start, range.end));
    let fits = |at: usize| at <= text.len() && text.is_char_boundary(at);
    if let Some((_, (range, _))) = (indexed.iter())
        .find(|(_, (range, _))| range.start > range.end || !fits(range.start) || !fits(range.end))
    {
        return Err(format!("an edit at {range:?} doesn't fit its text"));
    }
    if (indexed.windows(2)).any(|w| w[1].1.0.start < w[0].1.0.end) {
        return Err("its edits overlap".into());
    }
    // Joined as `merge_inserts` joins them, which applying does.
    let mut grouped: Vec<Group> = Vec::new();
    for (at, (range, new)) in indexed {
        match grouped.last_mut() {
            Some((last, text, held)) if last.start == last.end && *last == range => {
                text.push_str(&new);
                held.push(at);
            }
            _ => grouped.push((range, new, vec![at])),
        }
    }
    Ok(grouped)
}

/// `text` with `grouped` (sorted, apart) applied, and the edits made, as
/// applying makes them: each its range and the length of what replaced
/// it, no-ops left out.
fn applied(
    text: &str,
    grouped: &[(Range<usize>, String, Vec<usize>)],
) -> (String, Vec<(Range<usize>, usize)>) {
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut made = Vec::new();
    for (range, new, _) in grouped {
        out.push_str(&text[copied..range.start]);
        let new = normalized(new);
        if text[range.clone()] != new {
            made.push((range.clone(), new.len()));
        }
        out.push_str(&new);
        copied = range.end;
    }
    out.push_str(&text[copied..]);
    (out, made)
}

#[cfg(test)]
#[path = "refactor_preview_tests.rs"]
mod tests;
