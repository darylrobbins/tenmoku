// SPDX-License-Identifier: Apache-2.0

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
pub fn open_for_reading(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    options.open(path)
}

/// Creates `dir` and its missing ancestors, readable by the owner alone.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// `options` with new files readable and writable by the owner alone.
pub fn private(options: &mut OpenOptions) -> &mut OpenOptions {
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(options, 0o600);
    options
}

/// Flushes `dir`'s entries, so a rename or a new file in it survives a crash. Nothing to do on
/// Windows, where a folder doesn't open as a `File` and NTFS journals its entries itself. An
/// empty `dir` is the working directory.
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let dir = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        };
        File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

/// Whether `metadata` is of something owned by the user `reference` is owned by (a file just
/// created) that no one else can write. Never on Windows, which has no such modes to check.
pub fn owned_and_unshared(metadata: &fs::Metadata, reference: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.uid() == reference.uid() && metadata.mode() & 0o022 == 0
    }
    #[cfg(not(unix))]
    {
        let _ = (metadata, reference);
        false
    }
}

/// [`fs::canonicalize`], in the form a user writes: on Windows, without the
/// `\\?\` that it puts before a drive path (`\\?\C:\x` is `C:\x`), so
/// canonical paths compare equal to, and read like, the ones typed or shown.
/// A verbatim path with no plainer form (a UNC share's, or one too long) is
/// kept as it is.
pub fn canonicalize(path: &Path) -> io::Result<std::path::PathBuf> {
    let canonical = fs::canonicalize(path)?;
    #[cfg(windows)]
    {
        if let Some(rest) = canonical
            .to_str()
            .and_then(|text| text.strip_prefix(r"\\?\"))
        {
            let bytes = rest.as_bytes();
            let drive = bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && bytes[2] == b'\\';
            // `MAX_PATH`: longer ones need the prefix to be opened.
            if drive && rest.len() < 260 {
                return Ok(std::path::PathBuf::from(rest));
            }
        }
    }
    Ok(canonical)
}

/// The identity of open file `file`: its device and inode on Unix, its
/// volume serial number and file index on Windows. Two handles to the same
/// file have the same identity; a file put in another's place has another.
pub fn file_identity(file: &File) -> io::Result<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata()?;
        Ok((meta.dev(), meta.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        // SAFETY: `file` is an open handle and `info` is valid for writes.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call succeeded, so it filled `info`.
        let info = unsafe { info.assume_init() };
        Ok((
            u64::from(info.dwVolumeSerialNumber),
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        ))
    }
}

/// The identity of the entry at `path`, as [`file_identity`] gives it:
/// what a symlink there leads to if `follow`, else the symlink itself.
pub fn path_identity(path: &Path, follow: bool) -> io::Result<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = if follow {
            fs::metadata(path)?
        } else {
            fs::symlink_metadata(path)?
        };
        Ok((meta.dev(), meta.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        // No access but to its attributes, which any entry gives; a folder
        // opens only with backup semantics.
        let mut flags = FILE_FLAG_BACKUP_SEMANTICS;
        if !follow {
            flags |= FILE_FLAG_OPEN_REPARSE_POINT;
        }
        let file = OpenOptions::new()
            .access_mode(0)
            .custom_flags(flags)
            .open(path)?;
        file_identity(&file)
    }
}

/// `metadata`'s modification time as seconds and nanoseconds since the Unix
/// epoch, as `MetadataExt::mtime` and `mtime_nsec` give it on Unix.
pub fn modified_parts(metadata: &fs::Metadata) -> (i64, i64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (metadata.mtime(), metadata.mtime_nsec())
    }
    #[cfg(not(unix))]
    {
        use std::time::UNIX_EPOCH;
        match metadata.modified() {
            Ok(time) => match time.duration_since(UNIX_EPOCH) {
                Ok(after) => (after.as_secs() as i64, i64::from(after.subsec_nanos())),
                Err(before) => {
                    let before = before.duration();
                    (
                        -(before.as_secs() as i64),
                        -i64::from(before.subsec_nanos()),
                    )
                }
            },
            Err(_) => (0, 0),
        }
    }
}

/// Whether a regular file with `metadata` at `path` can be run as a program:
/// on Unix, it has an execute bit; on Windows, its extension is one of
/// `PATHEXT`'s (`.exe`, `.cmd` and the like).
pub fn is_executable(path: &Path, metadata: &fs::Metadata) -> bool {
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        metadata.mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
            return false;
        };
        program_extensions().iter().any(|known| {
            known
                .trim_start_matches('.')
                .eq_ignore_ascii_case(extension)
        })
    }
}

/// The paths a program named `name` may have in directory `dir`: `name`
/// itself, and on Windows `name` with each of `PATHEXT`'s extensions, as a
/// shell searches for it.
pub fn program_candidates(dir: &Path, name: &str) -> Vec<std::path::PathBuf> {
    let bare = std::iter::once(dir.join(name));
    #[cfg(not(unix))]
    let bare = bare.chain(
        program_extensions()
            .into_iter()
            .map(|extension| dir.join(format!("{name}{extension}"))),
    );
    bare.collect()
}

/// `PATHEXT`'s extensions, each with its leading dot, or Windows' default.
#[cfg(not(unix))]
fn program_extensions() -> Vec<String> {
    let value = std::env::var("PATHEXT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_owned());
    value
        .split(';')
        .map(str::trim)
        .filter(|ext| ext.starts_with('.') && ext.len() > 1)
        .map(str::to_ascii_lowercase)
        .collect()
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
