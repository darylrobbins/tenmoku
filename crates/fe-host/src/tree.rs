//! What the project tree and its undo send and get back through
//! [`crate::FsHost`]'s tree operations: plain data, no file handles, so a
//! remote host can carry it (docs/decisions/5zajsn5anw-fs-host-tree.md).
//!
//! The types were fe-project's (`fs_ops` and `listing`), which re-exports
//! them where they were; the operations' local impl is still fe-project's.

use std::{
    any::Any,
    ffi::OsString,
    fmt, io,
    path::{Component, PathBuf},
    sync::Arc,
};

/// What an entry is: its device and inode (on Windows, its volume's serial
/// number and its file index) and, on Linux where the file system reports
/// one, its birth time. The fields compare as they are, never folded: where
/// `born` is `None` on both sides two entries are the same when their device
/// and inode are, as before.
///
/// ext4 and xfs hand a freed inode straight to the next file made, so a file
/// deleted and re-created at a path can have the same device and inode; its
/// birth time differs. Two entries made within one kernel clock tick (a few
/// milliseconds) share a birth time, and a file system that reports none
/// can't tell them apart at all (docs/decisions/1f6gnnmvk4-entry-identity-reuse.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntryId {
    pub dev: u64,
    pub ino: u64,
    /// The birth time as seconds and nanoseconds since the Unix epoch.
    pub born: Option<(i64, u32)>,
}

impl EntryId {
    /// An entry whose birth time isn't known.
    pub fn without_born((dev, ino): (u64, u64)) -> Self {
        EntryId {
            dev,
            ino,
            born: None,
        }
    }
}

/// An entry an operation made: its path, absolute under the root, and what
/// it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    pub path: PathBuf,
    pub id: EntryId,
}

/// What a recorded create made: the entry's path, and the first entry it
/// made, the top of what undoing it removes: the first folder its name
/// names that it made, or the entry. A folder that was there, or that
/// another process made meanwhile, isn't counted as made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created {
    pub path: PathBuf,
    pub first: Placed,
}

/// An entry a recorded trash moved to the Trash, for
/// [`crate::FsHost::restore`] to put back: where it was, where in the
/// Trash it went if the host's Trash said, and what it is, so another entry
/// found there isn't taken for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashRecord {
    /// Absolute, under the root, as the root was given.
    pub path: PathBuf,
    /// Where it is in the host's Trash, if the system said.
    pub at: Option<PathBuf>,
    /// What it is, as it is in the Trash and once it's put back.
    pub id: EntryId,
}

/// What a tree entry is. Symlinks are never followed, so a link to a directory
/// is a [`EntryKind::Symlink`] and can't be expanded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
}

/// One entry of a listed directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: OsString,
    pub kind: EntryKind,
    /// Matched by an ignore file. Only ever set when ignored entries are shown.
    pub ignored: bool,
    /// Directories only: its device and inode, as `stat` reports them.
    pub id: Option<(u64, u64)>,
}

/// A listing of one directory of the tree, for [`crate::FsHost::list_tree`]:
/// what fe-project's `Listing` holds, less the tree's clock, which stays
/// with the client.
#[derive(Clone, Debug)]
pub struct ListRequest {
    /// The project root.
    pub root: PathBuf,
    /// The directory, relative to `root`: plain names only, or a host
    /// refuses it with `InvalidInput`.
    pub rel: PathBuf,
    pub show_ignored: bool,
    /// Everything under an ignored directory is ignored, whatever its own name.
    pub parent_ignored: bool,
    /// The device and inode the tree has for the root and each directory
    /// down to this one, `None` where it doesn't know: one more than `rel`
    /// has names, or a host refuses it with `InvalidInput`.
    pub ids: Vec<Option<(u64, u64)>>,
    /// git's global excludes (`core.excludesFile`) as the tree read them
    /// when it opened, so every listing of one tree classifies entries
    /// alike. A local host lists with them. They're this machine's, so they
    /// never cross the wire: a remote host lists with its own, and one
    /// handed none reads its own once.
    pub global_excludes: Option<Excludes>,
}

/// A tree's snapshot of git's global excludes, opaque here: fe-project's
/// `Gitignore`, which only the local host reads. (Naming its type would
/// make fe-host depend on the `ignore` crate, and with it every crate
/// above fe-host on `aho-corasick` and `regex-automata`, whose
/// `PartialEq<Span> for Range<usize>` impls break type inference in
/// their tests: docs/decisions/5zajsn5anw-fs-host-tree.md.)
#[derive(Clone)]
pub struct Excludes(Arc<dyn Any + Send + Sync>);

impl Excludes {
    /// The snapshot `excludes` is.
    pub fn new<T: Any + Send + Sync>(excludes: Arc<T>) -> Self {
        Excludes(excludes)
    }

    /// The snapshot, if it is a `T`.
    pub fn get<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.0.clone().downcast().ok()
    }

    /// Whether `self` and `other` are one snapshot.
    pub fn same(&self, other: &Excludes) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for Excludes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Excludes").finish_non_exhaustive()
    }
}

impl ListRequest {
    /// The directory this lists.
    pub fn dir(&self) -> PathBuf {
        self.root.join(&self.rel)
    }

    /// Refuses, with `InvalidInput`, a request no tree makes: a `rel` with
    /// anything but plain names (a `..`, a `.`, a root or a prefix, which
    /// could list outside the root), or `ids` that aren't one for the root
    /// and one for each of `rel`'s names. A host checks this before it
    /// reads anything.
    pub fn check(&self) -> io::Result<()> {
        let mut names = 0;
        for component in self.rel.components() {
            match component {
                Component::Normal(_) => names += 1,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        RequestError::LeavesTheRoot,
                    ));
                }
            }
        }
        if self.ids.len() != names + 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                RequestError::IdsMismatch,
            ));
        }
        Ok(())
    }
}

/// Why [`ListRequest::check`] refused a request: the inner error of the
/// `InvalidInput` it fails with, for the caller to word.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RequestError {
    /// `rel` holds something other than plain names.
    LeavesTheRoot,
    /// `ids` isn't one for the root and one for each name of `rel`.
    IdsMismatch,
    /// `global_excludes` isn't a snapshot the host can read.
    NotExcludes,
}

impl RequestError {
    /// The [`RequestError`] `error` carries, if it does.
    pub fn of(error: &io::Error) -> Option<RequestError> {
        error.get_ref()?.downcast_ref().copied()
    }
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RequestError::LeavesTheRoot => "the listed folder isn't a path of names under the root",
            RequestError::IdsMismatch => "the ids don't match the listed folder's depth",
            RequestError::NotExcludes => "the global excludes aren't a snapshot this host reads",
        })
    }
}

impl std::error::Error for RequestError {}

/// Why a path no longer leads to the directory a listing was for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListingLost {
    /// Its own name holds nothing, another directory, or a symlink.
    Itself,
    /// A directory above it, the root included, is missing, another
    /// directory, or a symlink, perhaps only for a moment.
    Above,
    /// The root (for a listing of the root itself, a directory above it) is
    /// missing, another directory, or a symlink, perhaps only for a moment.
    /// No events come for directories above the root.
    Root,
}

/// What [`crate::FsHost::list_tree`] read.
#[derive(Debug)]
pub struct ListReply {
    /// The entries in display order, or why the directory couldn't be read.
    pub result: io::Result<Vec<Entry>>,
    /// The directory's device and inode, on success.
    pub id: Option<(u64, u64)>,
    /// Set, on failure, if the path no longer led to the directory the
    /// listing was for.
    pub lost: Option<ListingLost>,
}

/// The error of a tree operation a host doesn't have.
pub(crate) fn unsupported() -> io::Error {
    io::ErrorKind::Unsupported.into()
}
