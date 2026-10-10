// SPDX-License-Identifier: LicenseRef-Proprietary

//! File operations whose safe form differs by operating system: opening an
//! untrusted file without blocking on a FIFO, and creating private
//! directories and files (owner-only on Unix; Windows keeps its defaults,
//! which a user's profile directory already restricts). And a file's
//! [`FileStamp`], as finely as the OS reports what changed.

use std::{
    fs::{self, File, OpenOptions},
    io,
    path::Path,
};

/// Opens `path` for reading without blocking: a FIFO put in place of a
/// config file would otherwise hang the open.
pub(crate) fn open_for_reading(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    options.open(path)
}

/// Creates `dir` and its missing ancestors, readable by the owner alone.
pub(crate) fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// `options` with new files readable and writable by the owner alone.
pub(crate) fn private(options: &mut OpenOptions) -> &mut OpenOptions {
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(options, 0o600);
    options
}

/// What a path's own metadata (`symlink_metadata`) says of its identity and
/// last change, as finely as the OS reports it: on Unix its device, inode,
/// size, and modification and status-change times to the nanosecond, so a
/// file replaced by a rename, or rewritten within a second, still differs;
/// elsewhere its size and modification time. Two stamps of an unchanged
/// path are equal.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FileStamp {
    len: u64,
    #[cfg(unix)]
    unix: UnixStamp,
    #[cfg(not(unix))]
    modified: Option<std::time::SystemTime>,
}

#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
struct UnixStamp {
    dev: u64,
    ino: u64,
    /// Seconds and nanoseconds.
    mtime: (i64, i64),
    ctime: (i64, i64),
}

impl FileStamp {
    pub(crate) fn of(meta: &fs::Metadata) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Self {
                len: meta.len(),
                unix: UnixStamp {
                    dev: meta.dev(),
                    ino: meta.ino(),
                    mtime: (meta.mtime(), meta.mtime_nsec()),
                    ctime: (meta.ctime(), meta.ctime_nsec()),
                },
            }
        }
        #[cfg(not(unix))]
        {
            Self {
                len: meta.len(),
                modified: meta.modified().ok(),
            }
        }
    }
}
