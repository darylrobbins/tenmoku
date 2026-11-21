//! The machine a window's files live on (docs/decisions/j8wvgszah0-remote-development.md
//! § 5): one trait per kind of access, held together in a [`HostSet`], each
//! with a local impl that is the code the editor ran before there was a
//! seam, moved rather than rewritten.
//!
//! So far there is [`FsHost`], whose local impl is [`LocalFs`]. The other
//! traits (watching, search, processes, ptys, git) and `ProjectLocation`
//! (phase 1f) join it here.
//!
//! This crate is GPUI-free and depends on none of the fe-* crates that use
//! it (fe-project, fe-buffer, fe-editor, fe-terminal, fe-git, fe-workbench),
//! and it holds no UI text: errors it can't name with an
//! [`io::ErrorKind`] carry a [`PathError`] that the caller words.

mod local;

use std::{
    ffi::OsString,
    fmt,
    fs::Metadata,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

pub use local::{CommitOptions, LocalFs};

/// What a file on disk looked like when it was read or saved: compared
/// before a save to tell whether something else wrote it since. A client
/// treats a remote host's stamps as opaque, comparing them only for
/// equality.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DiskStamp {
    pub len: u64,
    pub modified: SystemTime,
    #[cfg(unix)]
    pub dev: u64,
    #[cfg(unix)]
    pub ino: u64,
}

impl DiskStamp {
    pub fn from_metadata(metadata: &Metadata) -> io::Result<Self> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified()?,
            #[cfg(unix)]
            dev: metadata.dev(),
            #[cfg(unix)]
            ino: metadata.ino(),
        })
    }

    /// The stamp of the file at `path`, following symlinks; `None` when
    /// there's no file there.
    pub fn of(path: impl AsRef<Path>) -> io::Result<Option<Self>> {
        match std::fs::metadata(path) {
            Ok(metadata) => Self::from_metadata(&metadata).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// What a write expects to replace (D § 7, "Write preconditions").
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WriteExpect {
    /// Overwrite was chosen: replace whatever is there.
    Overwrite,
    /// The writer doesn't know the file's state. What the host finds when
    /// the write starts must still be there at the commit, and a path
    /// found empty is committed without clobbering.
    AsFound,
    /// No file may be at the path: a new file, or a Save As destination.
    /// A symlink at the path when the write starts is followed, and the
    /// file it names must be missing; anything that appears at the path,
    /// or at that file, before the commit is a conflict. (D § 7 has
    /// "not even a symlink"; the local impl keeps today's save through a
    /// dangling link, and a Save As refuses one with
    /// [`CommitOptions::at_the_path`]: docs/decisions/h6ngbbm2jd-fe-host.md.)
    Absent,
    /// The file must still be the one with this stamp: one deleted since is
    /// a conflict too, unless the local commit is told otherwise
    /// ([`CommitOptions::deleted_is_written`], as an editor's save of a
    /// deleted file is: docs/decisions/h6ngbbm2jd-fe-host.md).
    Matches(DiskStamp),
}

/// A write refused because the file isn't what [`WriteExpect`] said.
/// Nothing was written.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// The file at the path as the host found it on refusing, following
    /// symlinks; `None` when there's none (a symlink to nothing, say).
    pub current: Option<DiskStamp>,
}

/// Why [`FsHost::write_atomic`] didn't write.
#[derive(Debug)]
pub enum WriteError {
    Conflict(Conflict),
    Io(io::Error),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Conflict(_) => f.write_str("the file changed since it was read"),
            WriteError::Io(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for WriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WriteError::Conflict(_) => None,
            WriteError::Io(error) => Some(error),
        }
    }
}

impl From<io::Error> for WriteError {
    fn from(error: io::Error) -> Self {
        WriteError::Io(error)
    }
}

/// A path the host can't write, for a reason with no [`io::ErrorKind`] of
/// its own: the inner error of the [`io::Error`] it fails with, for the
/// caller to word ([`PathError::of`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PathError {
    /// A directory, where the platform has no `EISDIR` to say so.
    IsADirectory,
    /// Something that is neither a file nor a directory: a FIFO, a socket,
    /// a device.
    NotARegularFile,
    /// More symlinks than a lookup follows.
    SymlinkLoop,
}

impl PathError {
    /// The [`PathError`] `error` carries, if it does.
    pub fn of(error: &io::Error) -> Option<PathError> {
        error.get_ref()?.downcast_ref().copied()
    }

    pub(crate) fn into_io(self) -> io::Error {
        let kind = match self {
            PathError::IsADirectory => io::ErrorKind::IsADirectory,
            PathError::NotARegularFile => io::ErrorKind::InvalidInput,
            PathError::SymlinkLoop => io::ErrorKind::Other,
        };
        io::Error::new(kind, self)
    }
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PathError::IsADirectory => "is a directory",
            PathError::NotARegularFile => "not a regular file",
            PathError::SymlinkLoop => "too many levels of symbolic links",
        })
    }
}

impl std::error::Error for PathError {}

/// What is at a path.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FileKind {
    File,
    Dir,
    Symlink,
    /// A FIFO, socket, device and the like.
    Other,
}

impl FileKind {
    pub fn of(file_type: std::fs::FileType) -> Self {
        if file_type.is_symlink() {
            FileKind::Symlink
        } else if file_type.is_file() {
            FileKind::File
        } else if file_type.is_dir() {
            FileKind::Dir
        } else {
            FileKind::Other
        }
    }
}

/// What [`FsHost::stat`] found at a path, symlinks followed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Stat {
    pub kind: FileKind,
    pub stamp: DiskStamp,
}

/// One entry of [`FsHost::list_dir`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: OsString,
    /// The entry itself, a symlink not followed.
    pub kind: FileKind,
}

/// A host's files. Every call blocks, so run it off the UI thread.
pub trait FsHost: Send + Sync {
    /// The file at `path` and its stamp, taken from the open file before
    /// reading.
    fn read(&self, path: &Path) -> io::Result<(Vec<u8>, DiskStamp)>;

    /// Replaces the file at `path` with `bytes`, atomically, if it is what
    /// `expect` says; else a [`WriteError::Conflict`], writing nothing.
    /// Returns the new file's stamp. A symlink at `path` is kept, and the
    /// file it names is written.
    fn write_atomic(
        &self,
        path: &Path,
        bytes: &[u8],
        expect: WriteExpect,
    ) -> Result<DiskStamp, WriteError>;

    /// What is at `path`, symlinks followed; `None` when nothing is.
    fn stat(&self, path: &Path) -> io::Result<Option<Stat>>;

    /// The entries of the directory at `path`, in no order.
    fn list_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>>;

    /// `path` absolute, with its symlinks, `.` and `..` resolved.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
}

/// The hosts a window works through, one per trait (D § 5): today's code
/// for a local window. Cheap to clone.
#[derive(Clone)]
pub struct HostSet {
    pub fs: Arc<dyn FsHost>,
}

impl HostSet {
    /// This machine's hosts.
    pub fn local() -> Self {
        HostSet {
            fs: Arc::new(LocalFs),
        }
    }
}

impl fmt::Debug for HostSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostSet").finish_non_exhaustive()
    }
}
