//! The index side of a repository: a file's staged text, and status split into
//! its staged (HEAD to index) and unstaged (index to working tree) halves.

use std::collections::BTreeMap;
use std::path::PathBuf;

use gix::bstr::{BString, ByteSlice};
use gix::index::entry::Mode;
use gix::status::index_worktree::Item as WorktreeItem;
use gix::status::plumbing::index_as_worktree::{Change, EntryStatus};

use crate::{BINARY_PROBE_LEN, ChangeKind, GitError, Repo};

/// One changed file with its staged and unstaged halves kept apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitStatus {
    /// Absolute path of the file.
    pub path: PathBuf,
    /// How the index differs from HEAD, `None` when it doesn't. An
    /// intent-to-add file is `unstaged: Some(Added)`, as git status shows it.
    pub staged: Option<ChangeKind>,
    /// How the working tree differs from the index, `None` when it doesn't.
    pub unstaged: Option<ChangeKind>,
}

impl Repo {
    /// The text of `path`'s stage-0 blob in the index, decoded as UTF-8
    /// (lossily). `None` when `path` isn't in the index (or is only marked
    /// intent-to-add), is outside the working tree, or the blob is binary.
    pub fn index_text(&self, path: &std::path::Path) -> Result<Option<String>, GitError> {
        let Ok(rela_path) = path.strip_prefix(&self.workdir) else {
            return Ok(None);
        };
        let Ok(rela_path) = gix::path::os_str_into_bstr(rela_path.as_os_str()) else {
            return Ok(None);
        };
        if rela_path.is_empty() {
            return Ok(None);
        }
        let repo = self.repo.to_thread_local();
        let index = repo.index_or_empty().map_err(GitError::from_gix)?;
        let Some(entry) = index
            .entry_by_path_and_stage(rela_path.as_bstr(), gix::index::entry::Stage::Unconflicted)
        else {
            return Ok(None);
        };
        if entry
            .flags
            .contains(gix::index::entry::Flags::INTENT_TO_ADD)
            || ![Mode::FILE, Mode::FILE_EXECUTABLE, Mode::SYMLINK].contains(&entry.mode)
        {
            return Ok(None);
        }
        let object = repo.find_object(entry.id).map_err(GitError::from_gix)?;
        let data = &object.data;
        if data[..data.len().min(BINARY_PROBE_LEN)].contains(&0) {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(data).into_owned()))
    }

    /// Every changed file with the staged and unstaged halves apart, sorted by
    /// path. Untracked files are `unstaged: Some(Untracked)`; an unmerged path is
    /// `Conflicted` on both sides. Renames aren't tracked and submodules are
    /// skipped, as in [`Repo::status`]. An intent-to-add file is
    /// `unstaged: Some(Added)`, as git status shows it.
    pub fn status_split(&self) -> Result<Vec<SplitStatus>, GitError> {
        let repo = self.repo.to_thread_local();
        let iter = repo
            .status(gix::progress::Discard)
            .map_err(GitError::from_gix)?
            .untracked_files(gix::status::UntrackedFiles::Files)
            // Status of the whole repository, not just the process's current directory.
            .dirwalk_options(|options| options.empty_patterns_match_prefix(false))
            .index_worktree_submodules(None)
            .index_worktree_rewrites(None)
            .tree_index_track_renames(gix::status::tree_index::TrackRenames::Disabled)
            .into_iter(Vec::<BString>::new())
            .map_err(GitError::from_gix)?;

        let mut paths: BTreeMap<BString, (Option<ChangeKind>, Option<ChangeKind>)> =
            BTreeMap::new();
        for item in iter {
            match item.map_err(GitError::from_gix)? {
                gix::status::Item::TreeIndex(change) => {
                    use gix::diff::index::ChangeRef;
                    if change.entry_mode().is_submodule() {
                        continue;
                    }
                    let kind = match change {
                        ChangeRef::Addition { .. } => ChangeKind::Added,
                        ChangeRef::Deletion { .. } => ChangeKind::Deleted,
                        ChangeRef::Modification { .. } | ChangeRef::Rewrite { .. } => {
                            ChangeKind::Modified
                        }
                    };
                    paths.entry(change.location().to_owned()).or_default().0 = Some(kind);
                }
                gix::status::Item::IndexWorktree(WorktreeItem::Modification {
                    entry,
                    rela_path,
                    status,
                    ..
                }) => {
                    if entry.mode.is_submodule() {
                        continue;
                    }
                    let slot = paths.entry(rela_path).or_default();
                    match status {
                        EntryStatus::Conflict { .. } => *slot = conflicted(),
                        EntryStatus::Change(Change::Removed) => slot.1 = Some(ChangeKind::Deleted),
                        EntryStatus::Change(Change::Type { .. }) => {
                            slot.1 = Some(ChangeKind::Modified)
                        }
                        EntryStatus::Change(Change::Modification {
                            executable_bit_changed,
                            content_change,
                            ..
                        }) => {
                            if executable_bit_changed || content_change.is_some() {
                                slot.1 = Some(ChangeKind::Modified);
                            }
                        }
                        EntryStatus::IntentToAdd => slot.1 = Some(ChangeKind::Added),
                        EntryStatus::Change(Change::SubmoduleModification(_))
                        | EntryStatus::NeedsUpdate(_) => {}
                    }
                }
                gix::status::Item::IndexWorktree(WorktreeItem::DirectoryContents {
                    entry, ..
                }) => {
                    let is_file = matches!(
                        entry.disk_kind,
                        Some(gix::dir::entry::Kind::File | gix::dir::entry::Kind::Symlink)
                    );
                    if is_file && entry.status == gix::dir::entry::Status::Untracked {
                        paths.entry(entry.rela_path).or_default().1 = Some(ChangeKind::Untracked);
                    }
                }
                // Rename tracking is off, so there are no rewrites.
                gix::status::Item::IndexWorktree(WorktreeItem::Rewrite { .. }) => {}
            }
        }

        Ok(paths
            .into_iter()
            .filter(|(_, (staged, unstaged))| staged.is_some() || unstaged.is_some())
            .filter_map(|(rela_path, (staged, unstaged))| {
                let path = self
                    .workdir
                    .join(gix::path::from_bstr(rela_path.as_bstr()).ok()?);
                Some(SplitStatus {
                    path,
                    staged,
                    unstaged,
                })
            })
            .collect())
    }
}

fn conflicted() -> (Option<ChangeKind>, Option<ChangeKind>) {
    (Some(ChangeKind::Conflicted), Some(ChangeKind::Conflicted))
}
