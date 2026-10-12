//! File operations on the project tree: create a file or a folder, rename,
//! duplicate and move to the Trash, all kept under the project root.
//!
//! Every path is resolved under the root through directory handles opened a
//! component at a time without following a symlink, as
//! [`crate::open_file_under`] opens, and every change is made through those
//! handles with the `*at` calls. A path that would leave the root, through a
//! `..` or through a symlinked folder, is refused with
//! [`io::ErrorKind::InvalidInput`], whenever the link appeared.
//!
//! A path is absolute under the root or relative to it, as
//! [`crate::FileTree::find`] takes one. The root itself must be reachable
//! without a symlink: canonicalize one reached through a link first (the
//! tree's [`crate::FileTree::root_path`] is canonical). The paths returned
//! are absolute: the root joined with the names below it.

use std::{
    ffi::{CStr, CString, OsStr, OsString, c_int},
    fmt,
    fs::{File, Permissions},
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{
            ffi::{OsStrExt, OsStringExt},
            fs::{MetadataExt, PermissionsExt},
        },
    },
    path::{Component, Path, PathBuf},
};

use crate::listing::{self, Access, EntryKind};

/// Why a name typed for a new or renamed entry is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    /// No name at all.
    Empty,
    /// A component that is `.`.
    Dot,
    /// A component that is `..`.
    DotDot,
    /// A NUL anywhere in the name.
    Nul,
    /// An empty component: a leading `/`, or two in a row.
    EmptyComponent,
    /// A `/` in a name that must be a single component, as a rename's.
    Separator,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "a name is required",
            Self::Dot => "`.` is not a name",
            Self::DotDot => "`..` is not a name",
            Self::Nul => "a name can't hold a NUL",
            Self::EmptyComponent => "a name can't have an empty part between `/`s",
            Self::Separator => "a name can't hold a `/` here",
        })
    }
}

impl std::error::Error for NameError {}

impl From<NameError> for io::Error {
    fn from(err: NameError) -> Self {
        io::Error::new(io::ErrorKind::InvalidInput, err)
    }
}

/// Checks a name typed for a new entry. It may hold `/`s to name the folders
/// to create above it, nvim-tree style (`a/b/c.rs`), and end with one `/` to
/// name a folder. It is refused if it is empty or holds a NUL, or if a
/// component between the `/`s is empty, `.` or `..`.
pub fn validate_name(name: &str) -> Result<(), NameError> {
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.contains('\0') {
        return Err(NameError::Nul);
    }
    let body = name.strip_suffix('/').unwrap_or(name);
    for component in body.split('/') {
        match component {
            "" => return Err(NameError::EmptyComponent),
            "." => return Err(NameError::Dot),
            ".." => return Err(NameError::DotDot),
            _ => {}
        }
    }
    Ok(())
}

/// [`validate_name`] for a name that must be a single component.
fn validate_component(name: &str) -> Result<(), NameError> {
    validate_name(name)?;
    if name.contains('/') {
        return Err(NameError::Separator);
    }
    Ok(())
}

/// Creates file `name` in folder `dir`, with any folders `name` names above
/// it (`a/b/c.rs`); a `name` that ends in `/` creates a folder instead, as
/// [`create_dir`] does. Returns the new entry's path.
///
/// An existing target fails with [`io::ErrorKind::AlreadyExists`], and a file
/// or symlink where a folder should be fails too, both before anything is
/// created. Should creating fail midway, the folders made are removed again.
pub fn create_file(root: &Path, dir: &Path, name: &str) -> io::Result<PathBuf> {
    create(root, dir, name, name.ends_with('/'))
}

/// Creates folder `name` in folder `dir`, with any folders `name` names above
/// it (`a/b/c`); a trailing `/` is allowed. Otherwise as [`create_file`].
pub fn create_dir(root: &Path, dir: &Path, name: &str) -> io::Result<PathBuf> {
    create(root, dir, name, true)
}

fn create(root: &Path, dir: &Path, name: &str, folder: bool) -> io::Result<PathBuf> {
    validate_name(name)?;
    let root = Root::open(root)?;
    let dir_names = names_under(&root.path, dir)?;
    let names: Vec<&OsStr> = name
        .strip_suffix('/')
        .unwrap_or(name)
        .split('/')
        .map(OsStr::new)
        .collect();
    let (last, above) = names.split_last().expect("a valid name has a component");
    let mut path = root.path.clone();
    path.extend(dir_names.iter().chain(&names));

    // Open the folders that exist without creating anything, so an existing
    // target, or a file where a folder should be, changes nothing.
    let mut parent = root.open_dir(&dir_names)?;
    let mut existing = 0;
    for name in above {
        match open_subdir(&parent, name) {
            Ok(dir) => parent = dir,
            Err(err) if err.kind() == io::ErrorKind::NotFound => break,
            Err(err) => return Err(err),
        }
        existing += 1;
    }
    if existing == above.len() && listing::stat_at(&parent, last).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "something by that name already exists",
        ));
    }

    // The folders made, each with its parent, to remove should a later step
    // fail.
    let mut made: Vec<(File, &OsStr)> = Vec::new();
    let result = (|| {
        for name in &above[existing..] {
            match mkdir_at(&parent, name, 0o777) {
                Ok(()) => made.push((parent.try_clone()?, name)),
                // Made since it was looked for: use it, but it isn't ours.
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                Err(err) => return Err(err),
            }
            parent = open_subdir(&parent, name)?;
        }
        if folder {
            mkdir_at(&parent, last, 0o777)
        } else {
            let flags = libc::O_WRONLY
                | libc::O_CREAT
                | libc::O_EXCL
                | libc::O_NOFOLLOW
                | libc::O_NONBLOCK
                | libc::O_CLOEXEC;
            open_in(&parent, last, flags, 0o666).map(drop)
        }
    })();
    if result.is_err() {
        for (parent, name) in made.iter().rev() {
            // Only removes a folder still empty: never anything else's.
            let _ = unlink_at(parent, name, libc::AT_REMOVEDIR);
        }
    }
    result.map(|()| path)
}

/// Renames the entry at `from` to `new_name` in the same folder, and returns
/// its new path. `new_name` is a single component.
///
/// An existing target fails with [`io::ErrorKind::AlreadyExists`], checked
/// atomically with the rename, except the entry itself under another case on
/// a case-insensitive volume (`readme.md` to `README.md`): that rename is
/// made through a temporary name. Never replaces anything: where the file
/// system can't rename without replacing, the rename fails with
/// [`io::ErrorKind::Unsupported`].
pub fn rename(root: &Path, from: &Path, new_name: &str) -> io::Result<PathBuf> {
    validate_component(new_name)?;
    let root = Root::open(root)?;
    let target = root.resolve(from)?;
    let new = OsStr::new(new_name);
    let to = target.path.with_file_name(new);
    let source = listing::stat_at(&target.parent, target.name)?;
    if target.name == new {
        return Ok(to);
    }
    match rename_noreplace(&target.parent, target.name, &target.parent, new) {
        Ok(()) => Ok(to),
        Err(err)
            if err.kind() == io::ErrorKind::AlreadyExists
                && listing::stat_at(&target.parent, new)
                    .is_ok_and(|found| found.id == source.id) =>
        {
            // `new` finds the entry itself: the volume ignores case (or it is
            // a hard link, which the second rename below then finds).
            rename_through_temp(&target.parent, target.name, new)?;
            Ok(to)
        }
        Err(err) => Err(err),
    }
}

/// Renames `old` to `new` in `dir` through a temporary name, for a rename
/// that only changes case. Puts `old` back if `new` turns out to be taken.
fn rename_through_temp(dir: &File, old: &OsStr, new: &OsStr) -> io::Result<()> {
    let mut temp = None;
    for attempt in 0..100 {
        // Not built from `old`, which may be as long as a name can be.
        let name = OsString::from(format!(".fastedit-rename-{}-{attempt}", std::process::id()));
        match rename_noreplace(dir, old, dir, &name) {
            Ok(()) => {
                temp = Some(name);
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    let Some(temp) = temp else {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "no free temporary name to rename through",
        ));
    };
    rename_noreplace(dir, &temp, dir, new).inspect_err(|_| {
        let _ = rename_noreplace(dir, &temp, dir, old);
    })
}

/// Moves the entries at `paths` to the system Trash: the Finder's on macOS,
/// the freedesktop.org trash on Linux. Nothing is ever deleted outright. An
/// entry inside another one given goes with it. Should one fail, those before
/// it stay in the Trash.
///
/// Every entry is found through the root's handles first, so one outside the
/// root, or reached through a symlinked folder, is refused before anything
/// moves. Each is then found again as it is moved, and moved by what was
/// found, not by its path: on Linux renamed into the trash from its folder's
/// handle, and on macOS trashed through a file reference URL checked to name
/// the entry the handles found. A folder swapped for a symlink in between
/// can't lead the move out of the root.
pub fn trash(root: &Path, paths: &[impl AsRef<Path>]) -> io::Result<()> {
    let root = Root::open(root)?;
    // Each folder's handle is closed again, so a long selection doesn't hold
    // one open per entry.
    let mut full = Vec::with_capacity(paths.len());
    for path in paths {
        let target = root.resolve(path.as_ref())?;
        listing::stat_at(&target.parent, target.name)?;
        full.push(target.path);
    }
    full.sort();
    full.dedup_by(|inner, outer| inner.starts_with(outer));
    for path in &full {
        trash_target(&root.resolve(path)?)?;
    }
    Ok(())
}

/// In tests, runs what a test set to happen between the last check of an
/// entry to trash and its move.
#[cfg(test)]
fn after_check() {
    if let Some(hook) = tests::AFTER_CHECK.with(std::cell::Cell::take) {
        hook();
    }
}

#[cfg(not(test))]
fn after_check() {}

/// Trashes `target` with `NSFileManager`, which needs no Automation
/// permission (the Finder would), through a file reference URL: it names the
/// entry by volume and file id rather than by path, and is checked to name
/// the entry found through the handles before it is used.
#[cfg(target_os = "macos")]
fn trash_target(target: &Target<'_>) -> io::Result<()> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    let path = target
        .path
        .to_str()
        .ok_or_else(|| invalid("a path that isn't UTF-8"))?;
    let (dev, ino) = listing::stat_at(&target.parent, target.name)?.id;
    let reference = NSURL::fileURLWithPath(&NSString::from_str(path))
        .fileReferenceURL()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no file reference"))?;
    let ids = reference
        .absoluteString()
        .and_then(|url| reference_ids(&url.to_string()));
    let volumes = [dev as i32, fs_id(&target.parent)?];
    if !ids.is_some_and(|(volume, file)| file == ino && volumes.contains(&volume)) {
        return Err(invalid(
            "the path no longer leads to the entry found under the project root",
        ));
    }
    after_check();
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&reference, None)
        .map_err(|err| io::Error::other(err.localizedDescription().to_string()))
}

/// The first word of the file system id of the volume `dir` is on, which
/// names the volume in a file reference URL.
#[cfg(target_os = "macos")]
fn fs_id(dir: &File) -> io::Result<i32> {
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `dir` is an open descriptor and `stat` is valid for writes of
    // a `libc::statfs`.
    check(unsafe { libc::fstatfs(dir.as_raw_fd(), stat.as_mut_ptr()) })?;
    // SAFETY: `fstatfs` succeeded, so it filled `stat`.
    let stat = unsafe { stat.assume_init() };
    // SAFETY: `fsid_t` is two `int32_t`s, `val[0]` first.
    Ok(unsafe { (&raw const stat.f_fsid).cast::<i32>().read() })
}

/// The volume and file ids a file reference URL names:
/// `file:///.file/id=<volume>.<file>`, with a `/` after a folder's.
#[cfg(any(target_os = "macos", test))]
fn reference_ids(url: &str) -> Option<(i32, u64)> {
    let ids = url.strip_prefix("file:///.file/id=")?;
    let (volume, file) = ids.strip_suffix('/').unwrap_or(ids).split_once('.')?;
    // Written signed or unsigned, it is 32 bits.
    let volume = volume.parse::<i64>().ok()? as i32;
    Some((volume, file.parse().ok()?))
}

/// Moves `target` into the freedesktop.org trash of its volume: the home
/// trash (`$XDG_DATA_HOME/Trash`) where it is on the home trash's mount,
/// else `$topdir/.Trash/$uid` (a shared, sticky `.Trash`) or
/// `$topdir/.Trash-$uid` at the top of its mount. The `.trashinfo` file is
/// made first, exclusively, reserving the name in `files`; the entry is then
/// renamed into `files` from its folder's handle.
#[cfg(target_os = "linux")]
fn trash_target(target: &Target<'_>) -> io::Result<()> {
    let volume = freedesktop::Volume::of(&target.parent)?;
    // The home trash records the entry's absolute path; a volume's, its path
    // below the volume's top, so it still restores with the volume mounted
    // elsewhere.
    let (trash, recorded) = match freedesktop::home_trash() {
        Ok(home) if home.volume.same(&volume) => (home.open()?, target.path.clone()),
        _ => {
            let (trash, climbed) = freedesktop::volume_trash(&target.parent, &volume)?;
            (trash, freedesktop::below_top(&target.path, climbed))
        }
    };
    after_check();
    freedesktop::move_into(&trash, target, &recorded)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn trash_target(_: &Target<'_>) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no Trash on this system",
    ))
}

#[cfg(target_os = "linux")]
mod freedesktop {
    use std::{
        ffi::OsString,
        fs::{self, File},
        io::{self, Write},
        os::{
            fd::AsRawFd,
            unix::{
                ffi::{OsStrExt, OsStringExt},
                fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
            },
        },
        path::{Path, PathBuf},
    };

    use super::{
        NAME_MAX, Target, check, cut, mkdir_at, open_dir_at, open_in, rename_noreplace, unlink_at,
    };
    use crate::listing::{self, Access};

    /// Where an entry can be renamed to: the same device, and the same mount
    /// (a rename between two mounts fails, even of one file system).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct Volume {
        dev: u64,
        /// The mount's id, where the kernel tells it (`statx`, Linux 5.8).
        mount: Option<u64>,
    }

    impl Volume {
        /// The volume of open file `file`.
        pub(super) fn of(file: &File) -> io::Result<Self> {
            let dev = file.metadata()?.dev();
            let mut stat = std::mem::MaybeUninit::<libc::statx>::uninit();
            // SAFETY: `file` is an open descriptor, the empty path is
            // NUL-terminated and `stat` is valid for writes of a `statx`.
            let result = check(unsafe {
                libc::statx(
                    file.as_raw_fd(),
                    c"".as_ptr(),
                    libc::AT_EMPTY_PATH,
                    libc::STATX_MNT_ID,
                    stat.as_mut_ptr(),
                )
            });
            let mount = result.ok().and_then(|()| {
                // SAFETY: `statx` succeeded, so it filled `stat`.
                let stat = unsafe { stat.assume_init() };
                (stat.stx_mask & libc::STATX_MNT_ID != 0).then_some(stat.stx_mnt_id)
            });
            Ok(Self { dev, mount })
        }

        /// Whether an entry can be renamed from one to the other: the same
        /// device, and the same mount where both are known.
        pub(super) fn same(&self, other: &Self) -> bool {
            self.dev == other.dev
                && match (self.mount, other.mount) {
                    (Some(a), Some(b)) => a == b,
                    // Where statx can't tell a mount (it fails, or leaves
                    // `STATX_MNT_ID` out: Linux before 5.8, or a file system
                    // that omits it), only the devices are compared, so a bind
                    // mount of one file system passes for the same mount and
                    // its rename fails with `EXDEV`. Telling mounts apart
                    // there is follow-up T64.
                    _ => true,
                }
        }
    }

    /// The home trash: where it is, and the volume it is (or would be) on.
    pub(super) struct HomeTrash {
        path: PathBuf,
        pub(super) volume: Volume,
    }

    impl HomeTrash {
        /// Opens the home trash, making it if need be.
        pub(super) fn open(&self) -> io::Result<File> {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&self.path)?;
            open_path(&self.path)
        }
    }

    fn open_path(path: &Path) -> io::Result<File> {
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(path)
    }

    pub(super) fn home_trash() -> io::Result<HomeTrash> {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|home| !home.is_empty())
                    .map(|home| Path::new(&home).join(".local/share"))
            })
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home folder"))?;
        let path = data.join("Trash");
        // The volume of the nearest folder there is, should it not exist yet.
        let nearest = path
            .ancestors()
            .find_map(|dir| open_path(dir).ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home folder"))?;
        let volume = Volume::of(&nearest)?;
        Ok(HomeTrash { path, volume })
    }

    /// The trash at the top of the mount `dir` is on, made if need be, and
    /// how many folders above `dir` that top is.
    pub(super) fn volume_trash(dir: &File, volume: &Volume) -> io::Result<(File, usize)> {
        let mut top = dir.try_clone()?;
        let mut climbed = 0;
        loop {
            let up = listing::open_at(&top, "..".as_ref(), Access::Search)?;
            if !Volume::of(&up)?.same(volume) || listing::identity(&up)? == listing::identity(&top)?
            {
                break;
            }
            top = up;
            climbed += 1;
        }
        // SAFETY: always safe to call.
        let uid = unsafe { libc::getuid() };
        let owned = |dir: &File| dir.metadata().is_ok_and(|meta| meta.uid() == uid);
        let uid_name = OsString::from(uid.to_string());
        // A shared `.Trash`, made by an administrator: a sticky folder.
        if let Ok(shared) = open_dir_at(&top, ".Trash".as_ref(), Access::Search)
            && shared
                .metadata()
                .is_ok_and(|meta| meta.mode() & libc::S_ISVTX != 0)
        {
            let _ = mkdir_at(&shared, &uid_name, 0o700);
            if let Ok(own) = open_dir_at(&shared, &uid_name, Access::Search)
                && owned(&own)
            {
                return Ok((own, climbed));
            }
        }
        let name = OsString::from(format!(".Trash-{uid}"));
        match mkdir_at(&top, &name, 0o700) {
            Err(err) if err.kind() != io::ErrorKind::AlreadyExists => return Err(err),
            _ => {}
        }
        let own = open_dir_at(&top, &name, Access::Search)?;
        if !owned(&own) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the volume's trash belongs to someone else",
            ));
        }
        Ok((own, climbed))
    }

    /// Entry `path`, whose folder is `climbed` folders below its volume's
    /// top, relative to that top, as a volume trash records it.
    pub(super) fn below_top(path: &Path, climbed: usize) -> PathBuf {
        let names: Vec<_> = path.iter().filter(|name| *name != "/").collect();
        let below = names.len().saturating_sub(climbed + 1);
        names[below..].iter().collect()
    }

    /// Moves `target` into `trash`, recording that it was at `recorded`.
    pub(super) fn move_into(trash: &File, target: &Target<'_>, recorded: &Path) -> io::Result<()> {
        let files = make_and_open(trash, "files")?;
        let info = make_and_open(trash, "info")?;
        // Room for a number and `.trashinfo` in a name.
        let base = cut(
            target.name.as_bytes(),
            NAME_MAX - ".trashinfo".len() - ".9999".len(),
        )
        .to_vec();
        let contents = format!(
            "[Trash Info]\nPath={}\nDeletionDate={}\n",
            percent_encode(recorded.as_os_str().as_bytes()),
            deletion_date(),
        );
        for n in 1..10_000u32 {
            let mut name = base.clone();
            if n > 1 {
                name.extend_from_slice(format!(".{n}").as_bytes());
            }
            let name = OsString::from_vec(name);
            let mut info_name = name.clone();
            info_name.push(".trashinfo");
            let flags =
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
            let mut file = match open_in(&info, &info_name, flags, 0o600) {
                Ok(file) => file,
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err),
            };
            let moved = file
                .write_all(contents.as_bytes())
                .and_then(|()| rename_noreplace(&target.parent, target.name, &files, &name));
            match moved {
                Ok(()) => return Ok(()),
                Err(err) => {
                    let _ = unlink_at(&info, &info_name, 0);
                    if err.kind() != io::ErrorKind::AlreadyExists {
                        return Err(err);
                    }
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "no free name in the trash",
        ))
    }

    fn make_and_open(dir: &File, name: &str) -> io::Result<File> {
        match mkdir_at(dir, name.as_ref(), 0o700) {
            Err(err) if err.kind() != io::ErrorKind::AlreadyExists => return Err(err),
            _ => {}
        }
        open_dir_at(dir, name.as_ref(), Access::Search)
    }

    /// `path` escaped as the trash spec's `Path` key wants: as a URI path.
    fn percent_encode(path: &[u8]) -> String {
        let mut out = String::with_capacity(path.len());
        for &byte in path {
            if byte.is_ascii_alphanumeric() || b"-_.~/".contains(&byte) {
                out.push(char::from(byte));
            } else {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
        out
    }

    /// Now, local time, as `YYYY-MM-DDThh:mm:ss`.
    fn deletion_date() -> String {
        // SAFETY: a null argument is allowed.
        let now = unsafe { libc::time(std::ptr::null_mut()) };
        let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
        // SAFETY: `now` and `tm` are valid for the call.
        if unsafe { libc::localtime_r(&now, tm.as_mut_ptr()) }.is_null() {
            return String::from("1970-01-01T00:00:00");
        }
        // SAFETY: `localtime_r` succeeded, so it filled `tm`.
        let tm = unsafe { tm.assume_init() };
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec,
        )
    }
}

/// Copies the entry at `path` next to itself, named as the Finder names a
/// copy (`name copy.ext`, then `name copy 2.ext`, …), and returns the copy's
/// path. A folder is copied with everything in it; a symlink is copied as a
/// link, never followed. Duplicating a copy counts on from its number.
///
/// Contents and permission bits are copied, and a file's modification time.
/// Inside a folder, anything but a file, a folder or a symlink (a FIFO or a
/// socket) is left out. Should copying fail, the partial copy is removed.
pub fn duplicate(root: &Path, path: &Path) -> io::Result<PathBuf> {
    let root = Root::open(root)?;
    let target = root.resolve(path)?;
    let kind = listing::stat_at(&target.parent, target.name)?.kind;
    let dir = &target.parent;
    for n in 1..10_000 {
        let name = copy_name(target.name, kind == EntryKind::Dir, n);
        let mut modes = Modes::default();
        match copy_new(dir, target.name, kind, dir, &name, true, &mut modes)? {
            Copied::Done | Copied::Skipped => {}
            Copied::Taken => continue,
        }
        // The folders' own permissions, which may not let their owner write
        // or even search them, only once everything is in them: each folder
        // opened afresh from the copy's (so as few are open at once as the
        // copy is deep), and inner ones first.
        let result = modes.list.iter().try_for_each(|(path, mode)| {
            let mut folder = open_dir_at(dir, &name, Access::Read)?;
            for component in path {
                folder = open_dir_at(&folder, component, Access::Read)?;
            }
            folder.set_permissions(Permissions::from_mode(*mode))
        });
        if let Err(err) = result {
            remove_copy(dir, &name);
            return Err(err);
        }
        return Ok(target.path.with_file_name(name));
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free name for a copy",
    ))
}

/// The longest name, in bytes, on the file systems served (`NAME_MAX`).
const NAME_MAX: usize = 255;

/// The Finder's name for copy `n` of `name`: `name copy.ext`, then
/// `name copy 2.ext`, …, with a folder's name not split at a dot. A name
/// that is already a copy's loses its ` copy` or ` copy N` first. The stem is
/// cut short (at a character boundary, where it is UTF-8) to keep the name
/// within [`NAME_MAX`] bytes. The extension is kept whenever a non-empty
/// stem, the suffix, the dot and the extension fit; only when they can't is
/// it kept as part of the stem instead.
fn copy_name(name: &OsStr, folder: bool, n: u32) -> OsString {
    let path = Path::new(name);
    let mut suffix = b" copy".to_vec();
    if n > 1 {
        suffix.extend_from_slice(format!(" {n}").as_bytes());
    }
    let split = match (folder, path.file_stem(), path.extension()) {
        (false, Some(stem), Some(ext)) => {
            let stem = strip_copy_suffix(stem.as_bytes());
            // What is left for the stem beside the suffix, the dot and the
            // extension, if anything.
            NAME_MAX
                .checked_sub(suffix.len() + 1 + ext.len())
                .map(|room| (cut(stem, room), ext.as_bytes()))
                .filter(|(stem, _)| !stem.is_empty())
        }
        _ => None,
    };
    let (stem, ext) = match split {
        Some((stem, ext)) => (stem, Some(ext)),
        None => (
            cut(strip_copy_suffix(name.as_bytes()), NAME_MAX - suffix.len()),
            None,
        ),
    };
    let mut copy = stem.to_vec();
    copy.extend_from_slice(&suffix);
    if let Some(ext) = ext {
        copy.push(b'.');
        copy.extend_from_slice(ext);
    }
    OsString::from_vec(copy)
}

/// `bytes` cut to at most `max`, at a character boundary where they are
/// UTF-8.
fn cut(bytes: &[u8], max: usize) -> &[u8] {
    if bytes.len() <= max {
        return bytes;
    }
    let mut end = max;
    if let Ok(text) = std::str::from_utf8(bytes) {
        while !text.is_char_boundary(end) {
            end -= 1;
        }
    }
    &bytes[..end]
}

/// `stem` without a trailing ` copy` or ` copy N`, unless that is all of it.
fn strip_copy_suffix(stem: &[u8]) -> &[u8] {
    let digits = stem.iter().rev().take_while(|b| b.is_ascii_digit()).count();
    let base = if digits > 0 {
        match stem[..stem.len() - digits].strip_suffix(b" copy ") {
            Some(base) => base,
            None => return stem,
        }
    } else {
        match stem.strip_suffix(b" copy") {
            Some(base) => base,
            None => return stem,
        }
    };
    if base.is_empty() { stem } else { base }
}

/// What [`copy_new`] did.
enum Copied {
    Done,
    /// The destination name was taken; nothing was made.
    Taken,
    /// Neither a file, a folder nor a symlink, so left out.
    Skipped,
}

/// The permissions to give the folders of a copy once it is complete.
#[derive(Default)]
struct Modes {
    /// Where the copy has got to, below its top.
    path: Vec<OsString>,
    /// Each folder below the copy's top (the top itself is the empty path),
    /// after the folders in it, with its permissions.
    list: Vec<(Vec<OsString>, u32)>,
}

/// Copies entry `name` of `kind` in `from` to new entry `to_name` in `to`.
/// At the `top`, something not to be copied is an error rather than
/// [`Copied::Skipped`]. Each folder made is left writable by its owner, and
/// added to `modes` with the permissions it is to have.
fn copy_new(
    from: &File,
    name: &OsStr,
    kind: EntryKind,
    to: &File,
    to_name: &OsStr,
    top: bool,
    modes: &mut Modes,
) -> io::Result<Copied> {
    let skip = || {
        if top {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "only files, folders and symlinks can be duplicated",
            ));
        }
        log::warn!("duplicate: left out {name:?}, neither a file, folder nor symlink");
        Ok(Copied::Skipped)
    };
    let made = |result: io::Result<()>| match result {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(err),
    };
    match kind {
        EntryKind::Symlink => {
            let link = read_link_at(from, name)?;
            if !made(symlink_at(&link, to, to_name))? {
                return Ok(Copied::Taken);
            }
        }
        EntryKind::File => {
            // A FIFO, socket or device counts as a file in a listing. Look
            // before opening: a socket can't be opened at all.
            if mode_at(from, name)? & libc::S_IFMT != libc::S_IFREG {
                return skip();
            }
            let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
            let mut source = open_in(from, name, flags, 0)?;
            let metadata = source.metadata()?;
            // Swapped since it was looked at.
            if !metadata.is_file() {
                return skip();
            }
            let flags = libc::O_WRONLY
                | libc::O_CREAT
                | libc::O_EXCL
                | libc::O_NOFOLLOW
                | libc::O_NONBLOCK
                | libc::O_CLOEXEC;
            let mut copy = match open_in(to, to_name, flags, 0o600) {
                Ok(copy) => copy,
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                    return Ok(Copied::Taken);
                }
                Err(err) => return Err(err),
            };
            let result = (|| {
                io::copy(&mut source, &mut copy)?;
                copy.set_permissions(Permissions::from_mode(metadata.mode() & 0o777))?;
                copy.set_modified(metadata.modified()?)
            })();
            if result.is_err() {
                let _ = unlink_at(to, to_name, 0);
            }
            result?;
        }
        EntryKind::Dir => {
            if !made(mkdir_at(to, to_name, 0o700))? {
                return Ok(Copied::Taken);
            }
            let result = (|| {
                // Open to its owner until it is filled, whatever the source's
                // mode, and whatever the umask took from `mkdirat`'s.
                chmod_at(to, to_name, 0o700)?;
                let source = open_dir_at(from, name, Access::Read)?;
                let mode = source.metadata()?.mode() & 0o777;
                let copy = open_dir_at(to, to_name, Access::Read)?;
                let mut children = listing::read_dir(&source)?;
                children.sort_by(|(a, _), (b, _)| a.cmp(b));
                for (child, kind) in children {
                    let kind = match kind {
                        Some(kind) => kind,
                        None => listing::stat_at(&source, &child)?.kind,
                    };
                    modes.path.push(child.clone());
                    let copied = copy_new(&source, &child, kind, &copy, &child, false, modes);
                    modes.path.pop();
                    if let Copied::Taken = copied? {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "an entry appeared in a new copy",
                        ));
                    }
                }
                modes.list.push((modes.path.clone(), mode));
                Ok(())
            })();
            if result.is_err() {
                remove_copy(to, to_name);
            }
            result?;
        }
    }
    Ok(Copied::Done)
}

/// Removes a copy made by [`copy_new`], as far as it can: its folders are
/// made writable by their owner first, whatever permissions they were given.
fn remove_copy(dir: &File, name: &OsStr) {
    if !listing::stat_at(dir, name).is_ok_and(|stat| stat.kind == EntryKind::Dir) {
        let _ = unlink_at(dir, name, 0);
        return;
    }
    // Opened to its owner first: it may have been made with no access at
    // all, or already given a mode without it.
    let _ = chmod_at(dir, name, 0o700);
    if let Ok(sub) = open_dir_at(dir, name, Access::Read) {
        for (child, _) in listing::read_dir(&sub).unwrap_or_default() {
            remove_copy(&sub, &child);
        }
    }
    let _ = unlink_at(dir, name, libc::AT_REMOVEDIR);
}

/// `path` relative to `root`, its components joined with `/`. A relative
/// `path` is taken as relative to the root already; an absolute one outside
/// the root is returned whole. The root itself is the empty string.
pub fn relative_path(root: &Path, path: &Path) -> String {
    let rel = match path.strip_prefix(root) {
        Ok(rel) => rel,
        Err(_) if path.is_relative() => path,
        Err(_) => return path.to_string_lossy().into_owned(),
    };
    let names: Vec<_> = rel
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy()),
            Component::ParentDir => Some("..".into()),
            Component::CurDir | Component::RootDir | Component::Prefix(_) => None,
        })
        .collect();
    names.join("/")
}

/// The project root, opened.
struct Root {
    /// Absolute, as given.
    path: PathBuf,
    dir: File,
}

/// An entry under the root: its folder, opened, and its name.
struct Target<'p> {
    parent: File,
    name: &'p OsStr,
    /// Absolute.
    path: PathBuf,
}

impl Root {
    fn open(root: &Path) -> io::Result<Self> {
        let path = std::path::absolute(root)?;
        let dir = match listing::open_dirs(&path, Access::Search) {
            Ok(mut dirs) => dirs.pop().expect("`/` is always opened"),
            Err(unreached) => return Err(unreached.err),
        };
        Ok(Self { path, dir })
    }

    /// Opens the folder `names` lead to from the root.
    fn open_dir(&self, names: &[&OsStr]) -> io::Result<File> {
        let mut dir = self.dir.try_clone()?;
        for name in names {
            dir = open_subdir(&dir, name)?;
        }
        Ok(dir)
    }

    /// The entry at `path`, which must be below the root, not the root.
    fn resolve<'p>(&self, path: &'p Path) -> io::Result<Target<'p>> {
        let names = names_under(&self.path, path)?;
        let Some((name, above)) = names.split_last() else {
            return Err(invalid("the project root itself"));
        };
        let mut full = self.path.clone();
        full.extend(&names);
        Ok(Target {
            parent: self.open_dir(above)?,
            name,
            path: full,
        })
    }
}

/// The names of `path` below `root`: `path` is absolute under `root` or
/// relative to it, and has no `..`.
fn names_under<'p>(root: &Path, path: &'p Path) -> io::Result<Vec<&'p OsStr>> {
    let rel = if path.is_absolute() {
        path.strip_prefix(root)
            .map_err(|_| invalid("a path outside the project root"))?
    } else {
        path
    };
    rel.components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(Ok(name)),
            Component::CurDir => None,
            Component::ParentDir => Some(Err(invalid("`..` in a path under the project root"))),
            Component::RootDir | Component::Prefix(_) => {
                Some(Err(invalid("a path outside the project root")))
            }
        })
        .collect()
}

fn invalid(what: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, what)
}

/// Opens folder `name` in `dir` for search, refusing a symlink.
fn open_subdir(dir: &File, name: &OsStr) -> io::Result<File> {
    open_dir_at(dir, name, Access::Search)
}

/// Opens folder `name` in `dir` without following a symlink. A symlink there
/// fails with [`io::ErrorKind::InvalidInput`], as it could lead out of the
/// root, and a file with [`io::ErrorKind::NotADirectory`].
fn open_dir_at(dir: &File, name: &OsStr, access: Access) -> io::Result<File> {
    listing::open_at(dir, name, access).map_err(|err| {
        // A symlink opened as a folder without following it fails as not a
        // folder (or, on some systems, as a loop), like a file does.
        let not_dir = matches!(err.raw_os_error(), Some(libc::ENOTDIR | libc::ELOOP));
        match listing::stat_at(dir, name) {
            Ok(stat) if not_dir && stat.kind == EntryKind::Symlink => {
                invalid("a symlinked folder, which could lead outside the project root")
            }
            Ok(stat) if not_dir && stat.kind == EntryKind::File => io::Error::new(
                io::ErrorKind::NotADirectory,
                "a file where a folder should be",
            ),
            _ => err,
        }
    })
}

fn c_name(name: &OsStr) -> io::Result<CString> {
    Ok(CString::new(name.as_bytes())?)
}

/// The mode of entry `name` in `dir`, not following a symlink.
fn mode_at(dir: &File, name: &OsStr) -> io::Result<libc::mode_t> {
    let name = c_name(name)?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `dir` is an open descriptor, `name` is NUL-terminated and
    // `stat` is valid for writes of a `libc::stat`.
    check(unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    })?;
    // SAFETY: `fstatat` succeeded, so it filled `stat`.
    Ok(unsafe { stat.assume_init() }.st_mode)
}

fn check(result: c_int) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Opens `name` in `dir` with `flags`, and `mode` should it create it.
fn open_in(dir: &File, name: &OsStr, flags: c_int, mode: libc::mode_t) -> io::Result<File> {
    let name = c_name(name)?;
    // SAFETY: `dir` is an open descriptor and `name` is NUL-terminated.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            flags,
            libc::c_uint::from(mode),
        )
    };
    check(fd)?;
    // SAFETY: `openat` returned a new descriptor that nothing else owns.
    Ok(File::from(unsafe { OwnedFd::from_raw_fd(fd) }))
}

fn mkdir_at(dir: &File, name: &OsStr, mode: libc::mode_t) -> io::Result<()> {
    let name = c_name(name)?;
    // SAFETY: `dir` is an open descriptor and `name` is NUL-terminated.
    check(unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), mode) })
}

/// Sets the mode of folder `name` in `dir`, one just made by this module.
fn chmod_at(dir: &File, name: &OsStr, mode: libc::mode_t) -> io::Result<()> {
    let name = c_name(name)?;
    // SAFETY: `dir` is an open descriptor and `name` is NUL-terminated.
    check(unsafe { libc::fchmodat(dir.as_raw_fd(), name.as_ptr(), mode, 0) })
}

fn unlink_at(dir: &File, name: &OsStr, flags: c_int) -> io::Result<()> {
    let name = c_name(name)?;
    // SAFETY: `dir` is an open descriptor and `name` is NUL-terminated.
    check(unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), flags) })
}

fn symlink_at(target: &OsStr, dir: &File, name: &OsStr) -> io::Result<()> {
    let target = c_name(target)?;
    let name = c_name(name)?;
    // SAFETY: `dir` is an open descriptor and both names are NUL-terminated.
    check(unsafe { libc::symlinkat(target.as_ptr(), dir.as_raw_fd(), name.as_ptr()) })
}

/// What symlink `name` in `dir` points to.
fn read_link_at(dir: &File, name: &OsStr) -> io::Result<OsString> {
    let name = c_name(name)?;
    let mut buffer = vec![0u8; 1024];
    loop {
        // SAFETY: `dir` is an open descriptor, `name` is NUL-terminated and
        // `buffer` is valid for writes of its length.
        let len = unsafe {
            libc::readlinkat(
                dir.as_raw_fd(),
                name.as_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        let len = usize::try_from(len).map_err(|_| io::Error::last_os_error())?;
        // A full buffer may have cut the target short.
        if len < buffer.len() {
            buffer.truncate(len);
            return Ok(OsString::from_vec(buffer));
        }
        buffer.resize(buffer.len() * 2, 0);
    }
}

/// Renames `from` in `from_dir` to `to` in `to_dir`, failing with
/// [`io::ErrorKind::AlreadyExists`] if `to` exists, atomically. A file system
/// with no such rename fails it with [`io::ErrorKind::Unsupported`]: there is
/// no safe way round it, as a check then a rename could replace what appeared
/// between, and a link then an unlink could remove what replaced the source.
fn rename_noreplace(from_dir: &File, from: &OsStr, to_dir: &File, to: &OsStr) -> io::Result<()> {
    no_replace_result(rename_exclusive(
        from_dir,
        &c_name(from)?,
        to_dir,
        &c_name(to)?,
    ))
}

/// What a rename that refuses to replace returned, its file system's want of
/// support made [`io::ErrorKind::Unsupported`].
fn no_replace_result(result: io::Result<()>) -> io::Result<()> {
    result.map_err(|err| {
        let errno = err.raw_os_error();
        let unsupported = matches!(errno, Some(libc::EINVAL | libc::ENOSYS | libc::ENOTSUP))
            || errno == Some(libc::EOPNOTSUPP);
        if unsupported {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "this file system can't rename without the risk of replacing",
            )
        } else {
            err
        }
    })
}

/// The system's rename that refuses to replace.
#[cfg(target_os = "linux")]
fn rename_exclusive(from_dir: &File, from: &CStr, to_dir: &File, to: &CStr) -> io::Result<()> {
    // SAFETY: both descriptors are open and both names are NUL-terminated.
    check(unsafe {
        libc::renameat2(
            from_dir.as_raw_fd(),
            from.as_ptr(),
            to_dir.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    })
}

/// The system's rename that refuses to replace.
#[cfg(target_vendor = "apple")]
fn rename_exclusive(from_dir: &File, from: &CStr, to_dir: &File, to: &CStr) -> io::Result<()> {
    // SAFETY: both descriptors are open and both names are NUL-terminated.
    check(unsafe {
        libc::renameatx_np(
            from_dir.as_raw_fd(),
            from.as_ptr(),
            to_dir.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    })
}

/// No rename that refuses to replace here.
#[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
fn rename_exclusive(_: &File, _: &CStr, _: &File, _: &CStr) -> io::Result<()> {
    Err(io::Error::from_raw_os_error(libc::ENOSYS))
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::symlink};

    use tempfile::TempDir;

    use super::*;

    fn temp() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = fs::canonicalize(dir.path()).unwrap();
        (dir, path)
    }

    fn open(dir: &Path) -> File {
        Root::open(dir).unwrap().dir
    }

    thread_local! {
        /// What [`after_check`] runs next, once.
        pub(super) static AFTER_CHECK: std::cell::Cell<Option<Box<dyn FnOnce()>>> =
            std::cell::Cell::new(None);
    }

    #[test]
    fn a_file_system_without_an_exclusive_rename_is_unsupported() {
        // What `renameat2` gives where the file system lacks
        // `RENAME_NOREPLACE`: refused, never a rename that could replace.
        for errno in [libc::EINVAL, libc::ENOSYS, libc::ENOTSUP, libc::EOPNOTSUPP] {
            let err = no_replace_result(Err(io::Error::from_raw_os_error(errno))).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::Unsupported, "errno {errno}");
        }
        let err = no_replace_result(Err(io::Error::from_raw_os_error(libc::EEXIST))).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        no_replace_result(Ok(())).unwrap();
    }

    #[test]
    fn a_temporary_rename_name_is_bounded() {
        // A name as long as a name can be still renames through a temporary
        // one: here onto a hard link of itself, which finds it taken, and
        // then onto a free name.
        let (_dir, root) = temp();
        let long = "x".repeat(255);
        let other = "y".repeat(255);
        fs::write(root.join(&long), "x").unwrap();
        let dir = open(&root);
        fs::hard_link(root.join(&long), root.join(&other)).unwrap();
        let err = rename_through_temp(&dir, OsStr::new(&long), OsStr::new(&other)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(root.join(&long).is_file());
        fs::remove_file(root.join(&other)).unwrap();
        rename_through_temp(&dir, OsStr::new(&long), OsStr::new(&other)).unwrap();
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        assert_eq!(fs::read_to_string(root.join(&other)).unwrap(), "x");
    }

    #[test]
    fn copy_names_stay_within_the_longest_name() {
        let name = |name: &str, folder, n| copy_name(OsStr::new(name), folder, n);
        let file = format!("{}.txt", "a".repeat(251));
        let copy = name(&file, false, 1);
        assert_eq!(copy.len(), NAME_MAX);
        assert!(copy.to_str().unwrap().ends_with("a copy.txt"));
        let copy = name(&file, false, 12);
        assert_eq!(copy.len(), NAME_MAX);
        assert!(copy.to_str().unwrap().ends_with("a copy 12.txt"));
        let folder = "b".repeat(255);
        assert!(name(&folder, true, 1).to_str().unwrap().ends_with("b copy"));
        assert_eq!(name(&folder, true, 1).len(), NAME_MAX);
        // Cut at a character boundary.
        let wide = format!("{}.md", "é".repeat(126));
        let copy = name(&wide, false, 1);
        assert!(copy.len() <= NAME_MAX);
        assert!(copy.to_str().unwrap().ends_with("é copy.md"));
        assert_eq!(name("name.ext", false, 2), "name copy 2.ext");
    }

    #[test]
    fn a_copy_keeps_any_extension_that_fits_beside_a_stem() {
        let name = |name: &str, n| copy_name(OsStr::new(name), false, n).into_string().unwrap();
        let ext = |len| "e".repeat(len);
        // Around the old `NAME_MAX / 2` threshold, which dropped extensions
        // that fit.
        for len in [120, 121, 122, 127, 128] {
            let copy = name(&format!("x.{}", ext(len)), 1);
            assert_eq!(copy, format!("x copy.{}", ext(len)), "{len}");
        }
        // At the real limit: a one-byte stem, ` copy`, the dot and the
        // extension make exactly `NAME_MAX`.
        let fits = NAME_MAX - 1 - " copy".len() - 1;
        let copy = name(&format!("x.{}", ext(fits)), 1);
        assert_eq!(copy, format!("x copy.{}", ext(fits)));
        assert_eq!(copy.len(), NAME_MAX);
        // A longer stem is cut to leave the extension room.
        let copy = name(&format!("{}.{}", "s".repeat(50), ext(fits)), 1);
        assert_eq!(copy, format!("s copy.{}", ext(fits)));
        // ` copy 12` takes three bytes more.
        let fits_12 = fits - " 12".len();
        let copy = name(&format!("x.{}", ext(fits_12)), 12);
        assert_eq!(copy, format!("x copy 12.{}", ext(fits_12)));
        assert_eq!(copy.len(), NAME_MAX);
        // One byte more and no stem fits: the extension is part of the stem,
        // cut to fit.
        let copy = name(&format!("x.{}", ext(fits + 1)), 1);
        assert_eq!(
            copy,
            format!("x.{} copy", ext(NAME_MAX - 2 - " copy".len()))
        );
        assert_eq!(copy.len(), NAME_MAX);
        let copy = name(&format!("x.{}", ext(fits_12 + 1)), 12);
        assert!(copy.ends_with("e copy 12"));
        assert_eq!(copy.len(), NAME_MAX);
        // The stem must keep a whole character: a two-byte one fits beside
        // an extension a byte shorter, not beside the longest.
        let copy = name(&format!("é.{}", ext(fits - 1)), 1);
        assert_eq!(copy, format!("é copy.{}", ext(fits - 1)));
        let copy = name(&format!("é.{}", ext(fits)), 1);
        assert!(copy.ends_with("e copy"));
        assert!(copy.len() <= NAME_MAX);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_volume_trash_records_the_path_below_the_volume_top() {
        let path = Path::new("/media/disk/a/b/file");
        assert_eq!(freedesktop::below_top(path, 0), Path::new("file"));
        assert_eq!(freedesktop::below_top(path, 2), Path::new("a/b/file"));
        assert_eq!(
            freedesktop::below_top(path, 4),
            Path::new("media/disk/a/b/file")
        );
    }

    #[test]
    fn reference_ids_read_a_file_reference_url() {
        assert_eq!(
            reference_ids("file:///.file/id=6571367.2773272"),
            Some((6_571_367, 2_773_272))
        );
        assert_eq!(
            reference_ids("file:///.file/id=16777230.9/"),
            Some((16_777_230, 9))
        );
        assert_eq!(
            reference_ids("file:///.file/id=4294967295.9"),
            Some((-1, 9))
        );
        assert_eq!(reference_ids("file:///Users/x/a.txt"), None);
        assert_eq!(reference_ids("file:///.file/id=1"), None);
    }

    /// A temporary folder the trash moves into the home trash, as
    /// `tests/fs_ops.rs` makes one; `None` (saying why) where there is none.
    fn trashable_temp() -> Option<(TempDir, PathBuf)> {
        #[cfg(target_os = "macos")]
        let base = std::env::temp_dir();
        #[cfg(target_os = "linux")]
        let base = match freedesktop::home_trash() {
            Ok(_) => match std::env::var_os("XDG_DATA_HOME").filter(|dir| !dir.is_empty()) {
                Some(dir) => PathBuf::from(dir),
                None => Path::new(&std::env::var_os("HOME").unwrap()).join(".local/share"),
            },
            Err(err) => {
                eprintln!("skipped: no home trash: {err}");
                return None;
            }
        };
        match tempfile::Builder::new()
            .prefix("fe-project-trash-")
            .tempdir_in(&base)
        {
            Ok(dir) => {
                let path = fs::canonicalize(dir.path()).unwrap();
                Some((dir, path))
            }
            Err(err) => {
                eprintln!(
                    "skipped: no temporary folder under {}: {err}",
                    base.display()
                );
                None
            }
        }
    }

    /// Empties `name`, trashed from `parent`, from the trash: checking it is
    /// there off macOS, where the trash can be listed.
    #[cfg(target_os = "linux")]
    fn purge(parent: &Path, name: &str) {
        let items: Vec<_> = trash::os_limited::list()
            .unwrap()
            .into_iter()
            .filter(|item| item.original_parent == parent)
            .collect();
        let names: Vec<_> = items
            .iter()
            .map(|item| item.name.to_string_lossy())
            .collect();
        assert_eq!(names, [name]);
        trash::os_limited::purge_all(items).unwrap();
    }

    /// Empties `name` from `~/.Trash` where it can (reading it takes Full
    /// Disk Access); the name is the test's own.
    #[cfg(target_os = "macos")]
    fn purge(_parent: &Path, name: &str) {
        if let Some(home) = std::env::var_os("HOME") {
            let _ = fs::remove_file(Path::new(&home).join(".Trash").join(name));
        }
    }

    /// When a folder on an entry's path is swapped for a symlink.
    #[derive(Clone, Copy, PartialEq)]
    enum Swap {
        /// After the entry was found, before the move's last check.
        BeforeCheck,
        /// Between the move's last check and the move.
        AfterCheck,
    }

    /// Trashes `rel` after `swapped`, a folder on its way, was replaced with a
    /// symlink to `outside`, where the same names lead to a file to keep.
    fn trash_after_a_swap(base: &Path, swapped: &str, rel: &str, when: Swap) {
        let root = base.join("project");
        let outside = base.join("outside");
        let name = base.file_name().unwrap().to_str().unwrap();
        let file = format!("{rel}/{name}.txt");
        fs::create_dir_all(root.join(rel)).unwrap();
        fs::create_dir_all(outside.join(rel)).unwrap();
        fs::write(root.join(&file), "inside").unwrap();
        fs::write(outside.join(&file), "outside").unwrap();

        let opened = Root::open(&root).unwrap();
        let target = opened.resolve(Path::new(&file)).unwrap();
        // Swapped after the target was found: the folder moves away and a
        // symlink out of the root takes its place.
        let real = base.join(swapped);
        let moved = real.with_file_name("moved-away");
        let link_to = if swapped == "project" {
            outside.clone()
        } else {
            outside.join(rel)
        };
        let swap = {
            let (real, moved) = (real.clone(), moved.clone());
            move || {
                fs::rename(&real, &moved).unwrap();
                symlink(&link_to, &real).unwrap();
            }
        };
        match when {
            Swap::BeforeCheck => swap(),
            Swap::AfterCheck => AFTER_CHECK.with(|hook| hook.set(Some(Box::new(swap)))),
        }

        let result = trash_target(&target);
        assert!(
            AFTER_CHECK.with(|hook| hook.take()).is_none(),
            "the swap was made"
        );
        assert_eq!(
            fs::read_to_string(outside.join(&file)).unwrap(),
            "outside",
            "the file outside the root is never trashed"
        );
        let moved_file = if swapped == "project" {
            moved.join(&file)
        } else {
            moved.join(format!("{name}.txt"))
        };
        // On macOS, a swap before the reference URL is made and checked is
        // seen: the path no longer leads to the entry found, so it is refused.
        if cfg!(target_os = "macos") && when == Swap::BeforeCheck {
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
            assert!(moved_file.exists());
            return;
        }
        // Otherwise the entry found goes, by its folder's handle or by its
        // file id, wherever its folder went.
        result.unwrap();
        assert!(!moved_file.exists());
        purge(target.path.parent().unwrap(), &format!("{name}.txt"));
    }

    #[test]
    fn trash_never_follows_a_parent_swapped_for_a_symlink() {
        for when in [Swap::BeforeCheck, Swap::AfterCheck] {
            let Some((_dir, base)) = trashable_temp() else {
                return;
            };
            trash_after_a_swap(&base, "project/a/b", "a/b", when);
        }
    }

    #[test]
    fn trash_never_follows_a_root_swapped_for_a_symlink() {
        for when in [Swap::BeforeCheck, Swap::AfterCheck] {
            let Some((_dir, base)) = trashable_temp() else {
                return;
            };
            trash_after_a_swap(&base, "project", "a", when);
        }
    }
}
