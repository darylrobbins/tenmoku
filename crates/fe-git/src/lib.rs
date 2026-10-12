//! Git status and HEAD contents for a project, through gix (gitoxide).
//!
//! [`Repo`] answers the two questions the Changes panel and the editor's
//! changed-row highlighting ask of a repository: which files differ from HEAD
//! ([`Repo::status`]) and what a file's text is at HEAD ([`Repo::head_text`]).
//! Every path in and out is absolute. `Repo` is `Send + Sync`, so the workbench
//! can run it on a background thread. Nothing here writes to the repository.

mod index;
mod refs;

pub use index::SplitStatus;
pub use refs::{Branch, Head};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gix::bstr::{BString, ByteSlice};
use gix::status::index_worktree::Item as WorktreeItem;
use gix::status::plumbing::index_as_worktree::{Change, EntryStatus};

/// How far into a blob git looks for a NUL byte to decide it's binary.
const BINARY_PROBE_LEN: usize = 8000;

/// A failure reading the repository. gix's errors are kept as text.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git: {0}")]
    Gix(String),
}

impl GitError {
    fn from_gix(err: impl std::fmt::Display) -> Self {
        GitError::Gix(err.to_string())
    }
}

/// How a file differs from HEAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    /// In HEAD and in the working tree, with different content or mode (a type
    /// change included).
    Modified,
    /// In the index but not in HEAD.
    Added,
    /// In HEAD, but missing from the working tree or removed from the index.
    Deleted,
    /// In neither the index nor HEAD, and not ignored.
    Untracked,
    /// An unmerged index entry.
    Conflicted,
}

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStatus {
    /// Absolute path of the file.
    pub path: PathBuf,
    pub kind: ChangeKind,
}

/// A repository with a working tree.
pub struct Repo {
    repo: gix::ThreadSafeRepository,
    workdir: PathBuf,
}

impl Repo {
    /// The repository containing `path`, searching upward. `None` outside any
    /// repository, and for a bare one.
    pub fn discover(path: &Path) -> Result<Option<Repo>, GitError> {
        use gix::discover::upwards::Error as Upwards;
        let repo = match gix::ThreadSafeRepository::discover(path) {
            Ok(repo) => repo,
            Err(err) => {
                return match err.downcast_any_ref::<Upwards>() {
                    Some(
                        Upwards::NoGitRepository { .. }
                        | Upwards::NoGitRepositoryWithinCeiling { .. }
                        | Upwards::NoGitRepositoryWithinFs { .. },
                    ) => Ok(None),
                    _ => Err(GitError::from_gix(err)),
                };
            }
        };
        let Some(workdir) = repo.work_dir() else {
            return Ok(None);
        };
        let workdir = std::path::absolute(workdir).map_err(GitError::from_gix)?;
        Ok(Some(Repo { repo, workdir }))
    }

    /// The root of the working tree.
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// Every file whose working-tree content differs from HEAD, staged and
    /// unstaged changes combined, sorted by path. Renames aren't tracked and
    /// submodules are skipped. The index's refreshed stat cache isn't written
    /// back.
    pub fn status(&self) -> Result<Vec<FileStatus>, GitError> {
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

        let mut paths: BTreeMap<BString, Flags> = BTreeMap::new();
        for item in iter {
            match item.map_err(GitError::from_gix)? {
                gix::status::Item::TreeIndex(change) => {
                    use gix::diff::index::ChangeRef;
                    if change.entry_mode().is_submodule() {
                        continue;
                    }
                    let flags = paths.entry(change.location().to_owned()).or_default();
                    match change {
                        ChangeRef::Addition { .. } => flags.staged_add = true,
                        ChangeRef::Deletion { .. } => flags.staged_delete = true,
                        ChangeRef::Modification { .. } | ChangeRef::Rewrite { .. } => {
                            flags.modified = true
                        }
                    }
                }
                gix::status::Item::IndexWorktree(item) => match item {
                    WorktreeItem::Modification {
                        entry,
                        rela_path,
                        status,
                        ..
                    } => {
                        if entry.mode.is_submodule() {
                            continue;
                        }
                        let flags = match status {
                            EntryStatus::Conflict { .. } => Flags {
                                conflicted: true,
                                ..Flags::default()
                            },
                            EntryStatus::Change(Change::Removed) => Flags {
                                removed: true,
                                ..Flags::default()
                            },
                            EntryStatus::Change(Change::Type { .. }) => Flags {
                                modified: true,
                                ..Flags::default()
                            },
                            EntryStatus::Change(Change::Modification {
                                executable_bit_changed,
                                content_change,
                                ..
                            }) => Flags {
                                modified: executable_bit_changed || content_change.is_some(),
                                ..Flags::default()
                            },
                            EntryStatus::IntentToAdd => Flags {
                                staged_add: true,
                                ..Flags::default()
                            },
                            EntryStatus::Change(Change::SubmoduleModification(_))
                            | EntryStatus::NeedsUpdate(_) => continue,
                        };
                        paths.entry(rela_path).or_default().merge(flags);
                    }
                    WorktreeItem::DirectoryContents { entry, .. } => {
                        let is_file = matches!(
                            entry.disk_kind,
                            Some(gix::dir::entry::Kind::File | gix::dir::entry::Kind::Symlink)
                        );
                        if is_file && entry.status == gix::dir::entry::Status::Untracked {
                            paths.entry(entry.rela_path).or_default().untracked = true;
                        }
                    }
                    // Rename tracking is off, so there are no rewrites.
                    WorktreeItem::Rewrite { .. } => {}
                },
            }
        }

        Ok(paths
            .into_iter()
            .filter_map(|(rela_path, flags)| {
                let kind = flags.kind()?;
                let path = self
                    .workdir
                    .join(gix::path::from_bstr(rela_path.as_bstr()).ok()?);
                Some(FileStatus { path, kind })
            })
            .collect())
    }

    /// The text of `path`'s blob in HEAD's tree, decoded as UTF-8 (lossily).
    /// `None` when HEAD is unborn, `path` isn't in HEAD's tree or is outside the
    /// working tree, or the blob is binary.
    pub fn head_text(&self, path: &Path) -> Result<Option<String>, GitError> {
        let Ok(rela_path) = path.strip_prefix(&self.workdir) else {
            return Ok(None);
        };
        if rela_path.as_os_str().is_empty() {
            return Ok(None);
        }
        let repo = self.repo.to_thread_local();
        let mut head = repo.head().map_err(GitError::from_gix)?;
        if head.is_unborn() {
            return Ok(None);
        }
        let tree = head
            .peel_to_commit()
            .map_err(GitError::from_gix)?
            .tree()
            .map_err(GitError::from_gix)?;
        let Some(entry) = tree
            .lookup_entry_by_path(rela_path)
            .map_err(GitError::from_gix)?
        else {
            return Ok(None);
        };
        if !entry.mode().is_blob_or_symlink() {
            return Ok(None);
        }
        let object = entry.object().map_err(GitError::from_gix)?;
        let data = &object.data;
        if data[..data.len().min(BINARY_PROBE_LEN)].contains(&0) {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(data).into_owned()))
    }

    /// HEAD's commit id in hex, `None` when HEAD is unborn (or can't be read),
    /// so callers can tell when HEAD moved.
    pub fn head_id(&self) -> Option<String> {
        let repo = self.repo.to_thread_local();
        let mut head = repo.head().ok()?;
        if head.is_unborn() {
            return None;
        }
        head.peel_to_commit()
            .ok()
            .map(|commit| commit.id.to_string())
    }
}

/// What the status passes found for one path.
#[derive(Debug, Default, Clone, Copy)]
struct Flags {
    /// In the index, not in HEAD (or marked intent-to-add).
    staged_add: bool,
    /// In HEAD, not in the index.
    staged_delete: bool,
    /// In the index, missing from the working tree.
    removed: bool,
    /// Content or mode differs between HEAD and the index, or the index and the working tree.
    modified: bool,
    /// On disk, in neither the index nor HEAD, not ignored.
    untracked: bool,
    /// An unmerged index entry.
    conflicted: bool,
}

impl Flags {
    fn merge(&mut self, other: Flags) {
        self.staged_add |= other.staged_add;
        self.staged_delete |= other.staged_delete;
        self.removed |= other.removed;
        self.modified |= other.modified;
        self.untracked |= other.untracked;
        self.conflicted |= other.conflicted;
    }

    /// The combined kind, or `None` when the working tree matches HEAD after all
    /// (a file staged as new and then deleted from disk).
    fn kind(self) -> Option<ChangeKind> {
        if self.conflicted {
            Some(ChangeKind::Conflicted)
        } else if self.staged_add {
            (!self.removed).then_some(ChangeKind::Added)
        } else if self.staged_delete || self.removed {
            Some(ChangeKind::Deleted)
        } else if self.modified {
            Some(ChangeKind::Modified)
        } else if self.untracked {
            Some(ChangeKind::Untracked)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<Repo>();
    }
}
