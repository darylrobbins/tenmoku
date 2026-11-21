//! [`LocalFs`]: this machine's files, through the code fe-editor's
//! `SaveJob` and fe-workbench's loader ran before there was a seam.

use std::{
    fs::File,
    io::{self, Read as _, Write},
    path::{Path, PathBuf},
};

use crate::{
    Conflict, DirEntry, DiskStamp, FileKind, FsHost, PathError, Stat, WriteError, WriteExpect,
};

/// This machine's files.
#[derive(Copy, Clone, Debug, Default)]
pub struct LocalFs;

/// How [`LocalFs::commit`] runs, beyond its [`WriteExpect`].
///
/// Its hooks are calls a commit makes at its boundaries, so a test can race
/// it there rather than by timing: `before_commit` between writing the new
/// file and checking the target again, `before_persist` after the last
/// check, just before the rename.
#[derive(Default)]
pub struct CommitOptions<'a> {
    before_commit: Option<Box<dyn FnOnce() + 'a>>,
    before_persist: Option<Box<dyn FnOnce() + 'a>>,
    at_the_path: bool,
    deleted_is_written: bool,
}

impl<'a> CommitOptions<'a> {
    pub fn before_commit(mut self, hook: impl FnOnce() + 'a) -> Self {
        self.before_commit = Some(Box::new(hook));
        self
    }

    pub fn before_persist(mut self, hook: impl FnOnce() + 'a) -> Self {
        self.before_persist = Some(Box::new(hook));
        self
    }

    /// Unless overwriting, commits at the path itself, never through a
    /// symlink there, even one to nothing: a Save As destination, where not
    /// even a symlink may be. With [`WriteExpect::Absent`] a link at the path,
    /// there when the write starts or made before the rename, is then a
    /// conflict, as the no-clobber rename refuses it
    /// (docs/decisions/h6ngbbm2jd-fe-host.md).
    pub fn at_the_path(mut self) -> Self {
        self.at_the_path = true;
        self
    }

    /// With [`WriteExpect::Matches`], writes a file that is missing when the
    /// write starts (deleted since the stamp was taken), without clobbering,
    /// rather than refusing it: an editor's save of a file deleted under it,
    /// as fe-editor's `SaveJob` saves one
    /// (docs/decisions/h6ngbbm2jd-fe-host.md). A file made at the path
    /// before the commit is still a conflict.
    pub fn deleted_is_written(mut self) -> Self {
        self.deleted_is_written = true;
        self
    }
}

impl FsHost for LocalFs {
    fn read(&self, path: &Path) -> io::Result<(Vec<u8>, DiskStamp)> {
        let mut file = File::open(path)?;
        let metadata = file.metadata()?;
        let stamp = DiskStamp::from_metadata(&metadata)?;
        let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
        file.read_to_end(&mut bytes)?;
        Ok((bytes, stamp))
    }

    fn write_atomic(
        &self,
        path: &Path,
        bytes: &[u8],
        expect: WriteExpect,
    ) -> Result<DiskStamp, WriteError> {
        self.write_atomic_with(path, bytes, expect, CommitOptions::default())
    }

    fn stat(&self, path: &Path) -> io::Result<Option<Stat>> {
        match std::fs::metadata(path) {
            Ok(metadata) => Ok(Some(Stat {
                kind: FileKind::of(metadata.file_type()),
                stamp: DiskStamp::from_metadata(&metadata)?,
            })),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        std::fs::read_dir(path)?
            .map(|entry| {
                let entry = entry?;
                Ok(DirEntry {
                    name: entry.file_name(),
                    kind: FileKind::of(entry.file_type()?),
                })
            })
            .collect()
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        dunce::canonicalize(path)
    }
}

impl LocalFs {
    /// [`FsHost::write_atomic`], run as `options` say.
    pub fn write_atomic_with(
        &self,
        path: &Path,
        bytes: &[u8],
        expect: WriteExpect,
        options: CommitOptions<'_>,
    ) -> Result<DiskStamp, WriteError> {
        self.commit(path, expect, &|out| out.write_all(bytes), options)
    }

    /// [`FsHost::write_atomic`] of what `content` writes, streamed into the
    /// new file rather than held in memory first. `content` runs again, into
    /// the file itself, on a volume with no exclusive rename.
    ///
    /// Writes a new file beside the target and renames it over the
    /// original, so a failed write never truncates the file. A path that
    /// exists but isn't a regular file fails first with an I/O error, never
    /// as a conflict.
    ///
    /// Unless `expect` is [`WriteExpect::Overwrite`], it then fails with a
    /// conflict, writing nothing, if the file isn't as `expect` says. Then,
    /// once the new file is written and synced, it fails the same way,
    /// discarding it, if the file or the one a symlink points to changed
    /// since the write started.
    ///
    /// The new file gets a random name and is created exclusively, so it
    /// never follows a planted symlink. Over an existing file it is
    /// owner-only mid-write, so never readable by others, and the
    /// original's permissions are then applied through the open handle,
    /// and on macOS its extended attributes (Finder tags, quarantine) and
    /// creation date are copied too; a file made where nothing was gets
    /// 0666 less the umask. If the path is a symlink, the file it points to
    /// is updated and the link is left in place. Where nothing was found
    /// (and not overwriting), the rename refuses an existing name
    /// (`persist_noclobber`), and on a volume with no exclusive rename the
    /// file is instead created in place, still never replacing anything.
    pub fn commit(
        &self,
        path: &Path,
        expect: WriteExpect,
        content: &dyn Fn(&mut dyn Write) -> io::Result<()>,
        options: CommitOptions<'_>,
    ) -> Result<DiskStamp, WriteError> {
        let overwrite = expect == WriteExpect::Overwrite;
        let conflict = || {
            WriteError::Conflict(Conflict {
                current: DiskStamp::of(path).ok().flatten(),
            })
        };
        ensure_regular(path)?;
        let found = DiskStamp::of(path)?;
        // Nothing, not even a symlink, was at `path`: a non-forced write
        // commits at `path` itself, so whatever appears there is kept. One
        // made at the path is the same, even if a symlink has since appeared.
        let fresh = !overwrite
            && (options.at_the_path
                || matches!(std::fs::symlink_metadata(path),
                    Err(error) if error.kind() == io::ErrorKind::NotFound));
        let refused = match expect {
            WriteExpect::Absent => found.is_some(),
            WriteExpect::Matches(stamp) => match found {
                Some(found) => found != stamp,
                None => !options.deleted_is_written,
            },
            WriteExpect::AsFound | WriteExpect::Overwrite => false,
        };
        if refused {
            return Err(WriteError::Conflict(Conflict { current: found }));
        }
        let target = if fresh {
            path.to_path_buf()
        } else {
            resolve(path)?
        };
        let permissions = match std::fs::metadata(&target) {
            Ok(metadata) => Some(metadata.permissions()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let dir = match target.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => Path::new("."),
        };
        let file_name = target.file_name().unwrap_or_default().to_string_lossy();
        let prefix = format!(".{file_name}.");
        let mut builder = tempfile::Builder::new();
        builder.prefix(&prefix).suffix(".fastedit-save");
        // Nothing at the target: `open(2)` applies the umask to 0666.
        #[cfg(unix)]
        if permissions.is_none() {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o666));
        }
        let temp = builder.tempfile_in(dir)?;
        if let Some(permissions) = permissions {
            copy_xattrs(&target, temp.as_file());
            keep_creation_date(&target, temp.as_file());
            temp.as_file().set_permissions(permissions)?;
        }
        let mut out = io::BufWriter::new(temp);
        content(&mut out)?;
        let temp = out.into_inner().map_err(|error| error.into_error())?;
        temp.as_file().sync_all()?;
        // Renaming keeps the file's length, time and inode.
        let stamp = DiskStamp::from_metadata(&temp.as_file().metadata()?)?;
        if let Some(hook) = options.before_commit {
            hook();
        }
        // Narrows the window another writer has to the rename itself.
        ensure_regular(path)?;
        let now = resolve(path)?;
        ensure_regular(&now)?;
        // The rename lands on `target`, which a retargeted link no longer
        // names.
        ensure_regular(&target)?;
        if !overwrite && (DiskStamp::of(path)? != found || (!fresh && now != target)) {
            return Err(conflict());
        }
        // Dropping `temp` on any error above deletes it. A target that was
        // missing is committed with a rename that refuses an existing one,
        // so a file created since the check isn't replaced.
        if let Some(hook) = options.before_persist {
            hook();
        }
        let no_clobber = !overwrite && found.is_none();
        let persisted = if no_clobber {
            temp.persist_noclobber(&target)
        } else {
            temp.persist(&target)
        };
        match persisted {
            Ok(_) => Ok(stamp),
            Err(error) if no_clobber && no_exclusive_rename(&error.error) => {
                // The temp is deleted when `error` drops.
                create_in_place(&target, content).map_err(|error| match error {
                    WriteError::Conflict(_) => conflict(),
                    error => error,
                })
            }
            Err(error) => Err(match error.error.kind() {
                io::ErrorKind::AlreadyExists if no_clobber => conflict(),
                _ => WriteError::from(error.error),
            }),
        }
    }

    /// The file a write to `path` writes, as [`LocalFs::commit`] finds it:
    /// `path` canonical if it exists, else with its symlinks followed, even
    /// when the last one's target is missing. Two paths that give the same
    /// target write one file.
    pub fn save_target(path: &Path) -> io::Result<PathBuf> {
        resolve(path)
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn fcopyfile(
        from: std::ffi::c_int,
        to: std::ffi::c_int,
        state: *mut std::ffi::c_void,
        flags: u32,
    ) -> std::ffi::c_int;
    fn fsetattrlist(
        fd: std::ffi::c_int,
        attr_list: *mut AttrList,
        attr_buf: *mut std::ffi::c_void,
        attr_buf_size: usize,
        options: u32,
    ) -> std::ffi::c_int;
}

/// `struct attrlist` of `sys/attr.h`.
#[cfg(target_os = "macos")]
#[repr(C)]
struct AttrList {
    bitmapcount: u16,
    reserved: u16,
    commonattr: u32,
    volattr: u32,
    dirattr: u32,
    fileattr: u32,
    forkattr: u32,
}

/// `ATTR_BIT_MAP_COUNT` of `sys/attr.h`.
#[cfg(target_os = "macos")]
const ATTR_BIT_MAP_COUNT: u16 = 5;

/// `ATTR_CMN_CRTIME` of `sys/attr.h`: the creation date.
#[cfg(target_os = "macos")]
const ATTR_CMN_CRTIME: u32 = 0x200;

/// `COPYFILE_XATTR` of `copyfile.h`: extended attributes only.
#[cfg(target_os = "macos")]
const COPYFILE_XATTR: u32 = 1 << 2;

/// Copies the extended attributes of the file at `original` onto `new` (macOS).
/// A failure is logged and the write goes on.
#[cfg(target_os = "macos")]
fn copy_xattrs(original: &Path, new: &File) {
    use std::os::fd::AsRawFd;
    let copied = File::open(original).and_then(|from| {
        // SAFETY: both descriptors are open for the call; no state is passed.
        match unsafe {
            fcopyfile(
                from.as_raw_fd(),
                new.as_raw_fd(),
                std::ptr::null_mut(),
                COPYFILE_XATTR,
            )
        } {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        }
    });
    if let Err(error) = copied {
        log::warn!(
            "{}: its extended attributes weren't kept: {error}",
            original.display()
        );
    }
}

#[cfg(not(target_os = "macos"))]
fn copy_xattrs(_: &Path, _: &File) {}

/// Gives `new` the creation date of the file at `original` (macOS), as
/// `FileManager.replaceItemAt` does. A failure is logged and the write goes on.
#[cfg(target_os = "macos")]
fn keep_creation_date(original: &Path, new: &File) {
    let set = std::fs::metadata(original).and_then(|metadata| {
        // The signed fields, so a date before 1970 is kept too.
        use std::os::macos::fs::MetadataExt;
        let mut time = TimeSpec {
            tv_sec: metadata.st_birthtime(),
            tv_nsec: metadata.st_birthtime_nsec(),
        };
        set_creation_date(new, &mut time)
    });
    if let Err(error) = set {
        log::warn!(
            "{}: its creation date wasn't kept: {error}",
            original.display()
        );
    }
}

/// Sets the creation date of `file` to `time`.
#[cfg(target_os = "macos")]
fn set_creation_date(file: &File, time: &mut TimeSpec) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let mut list = AttrList {
        bitmapcount: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: ATTR_CMN_CRTIME,
        volattr: 0,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    // SAFETY: the descriptor is open, and the buffer holds the one
    // `timespec` that `commonattr` names.
    match unsafe {
        fsetattrlist(
            file.as_raw_fd(),
            &mut list,
            (time as *mut TimeSpec).cast(),
            std::mem::size_of::<TimeSpec>(),
            0,
        )
    } {
        0 => Ok(()),
        _ => Err(io::Error::last_os_error()),
    }
}

/// `struct timespec` on 64-bit macOS.
#[cfg(target_os = "macos")]
#[repr(C)]
struct TimeSpec {
    tv_sec: i64,
    tv_nsec: i64,
}

#[cfg(not(target_os = "macos"))]
fn keep_creation_date(_: &Path, _: &File) {}

/// Whether `error` says the file system has no exclusive rename (exFAT, FAT
/// and some network shares), so a new file can't be committed by one.
fn no_exclusive_rename(error: &io::Error) -> bool {
    let Some(code) = error.raw_os_error() else {
        return false;
    };
    #[cfg(target_os = "macos")]
    return matches!(
        code,
        45 // ENOTSUP
        | 102 // EOPNOTSUPP
        | 22 // EINVAL
        | 78 // ENOSYS
    );
    #[cfg(target_os = "linux")]
    return matches!(
        code,
        95 // EOPNOTSUPP
        | 22 // EINVAL
        | 38 // ENOSYS
    );
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = code;
        false
    }
}

/// Writes what `content` writes to a file created at `target` that must not
/// exist yet, for a volume with no exclusive rename. An existing file there,
/// or a symlink, is never replaced: a conflict. A write that fails removes
/// the file, unless something else has since taken its place.
fn create_in_place(
    target: &Path,
    content: &dyn Fn(&mut dyn Write) -> io::Result<()>,
) -> Result<DiskStamp, WriteError> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|error| match error.kind() {
            io::ErrorKind::AlreadyExists => WriteError::Conflict(Conflict { current: None }),
            _ => WriteError::from(error),
        })?;
    let written = (|| -> io::Result<DiskStamp> {
        let mut out = io::BufWriter::new(&file);
        content(&mut out)?;
        out.flush()?;
        drop(out);
        file.sync_all()?;
        DiskStamp::from_metadata(&file.metadata()?)
    })();
    if written.is_err() && is_same_file(target, &file) {
        let _ = std::fs::remove_file(target);
    }
    Ok(written?)
}

/// Whether `path` is still the file `file` has open.
fn is_same_file(path: &Path, file: &File) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::symlink_metadata(path), file.metadata()) {
            (Ok(at), Ok(open)) => at.dev() == open.dev() && at.ino() == open.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, file);
        false
    }
}

/// Fails with the I/O error a write would, if `path` (symlinks followed)
/// exists and isn't a regular file: `EISDIR` for a directory, so a write
/// never reports that as a conflict.
fn ensure_regular(path: &Path) -> io::Result<()> {
    let file_type = match std::fs::metadata(path) {
        Ok(metadata) => metadata.file_type(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if file_type.is_file() {
        Ok(())
    } else if file_type.is_dir() {
        #[cfg(unix)]
        return Err(io::Error::from_raw_os_error(21)); // EISDIR
        #[cfg(not(unix))]
        return Err(PathError::IsADirectory.into_io());
    } else {
        Err(PathError::NotARegularFile.into_io())
    }
}

/// The file a write to `path` writes: `path` with its symlinks resolved,
/// even when the last one's target is missing, so a write recreates the
/// target rather than replacing the link.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    match dunce::canonicalize(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        resolved => return resolved,
    }
    let mut path = path.to_path_buf();
    // Linux's limit on links followed in one lookup.
    for _ in 0..40 {
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                // A relative link is relative to its directory; joining an
                // absolute one replaces the base.
                let link = std::fs::read_link(&path)?;
                path = match path.parent() {
                    Some(dir) => dir.join(link),
                    None => link,
                };
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => return Ok(path),
        }
    }
    Err(PathError::SymlinkLoop.into_io())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn a_creation_date_before_1970_is_kept() {
        use std::os::macos::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let (old, new) = (dir.path().join("old"), dir.path().join("new"));
        let old_file = File::create(&old).unwrap();
        let new_file = File::create(&new).unwrap();
        let mut time = TimeSpec {
            tv_sec: -100_000_000,
            tv_nsec: 5,
        };
        set_creation_date(&old_file, &mut time).unwrap();
        assert_eq!(std::fs::metadata(&old).unwrap().st_birthtime(), time.tv_sec);
        keep_creation_date(&old, &new_file);
        let metadata = std::fs::metadata(&new).unwrap();
        assert_eq!(metadata.st_birthtime(), -100_000_000);
        assert_eq!(metadata.st_birthtime_nsec(), 5);
    }
}
