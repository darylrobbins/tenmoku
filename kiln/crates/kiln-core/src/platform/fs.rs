// SPDX-License-Identifier: Apache-2.0

//! File operations whose safe form differs by operating system: opening an
//! untrusted file without blocking on a FIFO, creating private directories
//! and files (owner-only modes on Unix, a protected access list on Windows),
//! and telling whether what is in one is still ours alone. And a file's
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

/// Creates `dir` and its missing ancestors, readable by the owner alone: mode 0700 on Unix; on
/// Windows owned by the current user, with a protected access list (none of the parent's
/// inherited) that gives full control to that user and `SYSTEM`, which what is created inside
/// inherits. A directory that already exists is left as it is.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(dir)
    }
    #[cfg(windows)]
    {
        // As `DirBuilder::recursive` does it: the directory, else its parent first.
        if dir.as_os_str().is_empty() {
            return Ok(());
        }
        match super::acl::create_private_dir(dir) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) if dir.is_dir() => return Ok(()),
            Err(error) => return Err(error),
        }
        match dir.parent() {
            Some(parent) => create_private_dir(parent)?,
            None => return Err(io::ErrorKind::NotFound.into()),
        }
        match super::acl::create_private_dir(dir) {
            Ok(()) => Ok(()),
            Err(_) if dir.is_dir() => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// Opens directory `dir` to check it with [`owned_and_unshared`], following a symlink as
/// [`fs::metadata`] does. Not a directory is an error.
pub fn open_dir(dir: &Path) -> io::Result<File> {
    #[cfg(unix)]
    let file = open_for_reading(dir)?;
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, READ_CONTROL,
        };
        // Its security descriptor and attributes alone; a folder opens only with backup
        // semantics.
        OpenOptions::new()
            .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(dir)?
    };
    if !file.metadata()?.is_dir() {
        return Err(io::ErrorKind::NotADirectory.into());
    }
    Ok(file)
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

/// Whose a file this process creates is, learnt from one it just created ([`Owner::of`]), to
/// check others against with [`owned_and_unshared`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owner {
    #[cfg(unix)]
    uid: u32,
    /// The current user, then the token's default owner if that differs (`Administrators` in
    /// some elevated processes): either may own what this process made.
    #[cfg(windows)]
    sids: Vec<super::acl::Sid>,
}

impl Owner {
    /// The owner of `file`, one this process just created: on Unix its user id; on Windows the
    /// current user from the process token, and an error if `file` is owned by anyone else.
    pub fn of(file: &File) -> io::Result<Owner> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Owner {
                uid: file.metadata()?.uid(),
            })
        }
        #[cfg(windows)]
        {
            use super::acl;
            let mut sids = vec![acl::current_user()?];
            let default = acl::default_owner()?;
            if default != sids[0] {
                sids.push(default);
            }
            let owner = acl::security(file)?.owner;
            if !owner.is_some_and(|owner| sids.contains(&owner)) {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            Ok(Owner { sids })
        }
    }
}

/// Whether open file or directory `file` is `owner`'s, and no one else can write it. On Unix:
/// its user id is the owner's and it has no group or other write bit. On Windows: its owner SID
/// is the current user (or the default owner `owner` holds), and its access list grants a
/// write right to no one but that user, `SYSTEM` and `Administrators`; a null list, or an entry
/// of a kind not read, fails. A directory is opened with [`open_dir`].
pub fn owned_and_unshared(file: &File, owner: &Owner) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        file.metadata()
            .is_ok_and(|metadata| metadata.uid() == owner.uid && metadata.mode() & 0o022 == 0)
    }
    #[cfg(windows)]
    {
        use super::acl::{ADMINISTRATORS, SYSTEM, security};
        let Ok(security) = security(file) else {
            return false;
        };
        let ours = |sid: &super::acl::Sid| owner.sids.contains(sid);
        security.owner.as_ref().is_some_and(ours)
            && security.writers.is_some_and(|writers| {
                writers
                    .iter()
                    .all(|sid| ours(sid) || sid.is(SYSTEM) || sid.is(ADMINISTRATORS))
            })
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
        super::acl::file_identity(file)
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
