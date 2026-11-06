//! The trash of test builds (`cfg(test)` or the `test-support` feature):
//! what [`super::trash`] moves goes to a folder of this process's own under
//! [`std::env::temp_dir`], never the user's Trash, unless the calling thread
//! holds [`real_trash`]. Every check the real trash makes is still made
//! first; only the last step, the move, differs: the entry is renamed from
//! its folder's handle, so it follows the entry found as the real move does.
//!
//! The folder, `fe-test-trash-<pid>`, is removed when the process exits, and
//! any left by a process that died without exiting is removed when the next
//! one is made.

use std::{
    cell::Cell,
    ffi::{OsStr, OsString},
    fs::{self, File},
    io,
    marker::PhantomData,
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

use super::{Target, mkdir_at, open_subdir, remove_at, rename_noreplace};
use crate::listing::{self, Access};

const PREFIX: &str = "fe-test-trash-";

thread_local! {
    /// How many [`RealTrash`] guards this thread holds.
    static REAL: Cell<usize> = const { Cell::new(0) };
    /// How many times this thread went on to the real trash.
    static REAL_CALLS: Cell<usize> = const { Cell::new(0) };
}

/// While held, the trashes this thread makes reach the system Trash, as in a
/// normal build. Not `Send`: it is this thread's alone.
#[must_use = "the system Trash is reached only while this is held"]
pub struct RealTrash {
    _thread: PhantomData<*const ()>,
}

/// Lets this thread's trashes reach the system Trash until the guard is
/// dropped. Other threads (parallel tests) still trash into the test trash.
pub fn real_trash() -> RealTrash {
    REAL.set(REAL.get() + 1);
    RealTrash {
        _thread: PhantomData,
    }
}

impl Drop for RealTrash {
    fn drop(&mut self) {
        REAL.set(REAL.get() - 1);
    }
}

/// Whether this thread's trash goes on to the system Trash, counted when it
/// does.
pub(super) fn reaching_real() -> bool {
    let real = real_held();
    if real {
        REAL_CALLS.set(REAL_CALLS.get() + 1);
    }
    real
}

/// Whether this thread holds a [`RealTrash`].
pub(super) fn real_held() -> bool {
    REAL.get() > 0
}

/// How many times this thread has gone on to the system Trash.
#[cfg(test)]
fn real_calls() -> usize {
    REAL_CALLS.get()
}

/// This process's test trash, made if need be. Each trashed entry is in a
/// folder of its own there, under its own name: see [`trashed`].
///
/// # Panics
///
/// If the test trash can't be made.
pub fn trash_dir() -> PathBuf {
    opened()
        .unwrap_or_else(|err| panic!("no test trash: {err}"))
        .path
        .clone()
}

/// Every entry in the test trash named `name`, whichever test trashed it.
pub fn trashed(name: impl AsRef<OsStr>) -> Vec<PathBuf> {
    let Ok(slots) = fs::read_dir(trash_dir()) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = slots
        .flatten()
        .map(|slot| slot.path().join(name.as_ref()))
        .filter(|path| path.symlink_metadata().is_ok())
        .collect();
    found.sort();
    found
}

/// Moves `target` into the test trash, into a new folder there under its
/// own name. A test trash on another volume fails with
/// [`io::ErrorKind::CrossesDevices`]: nothing goes to the real trash.
pub(super) fn move_into(target: &Target<'_>) -> io::Result<()> {
    let trash = opened()?;
    let slot = OsString::from(trash.next.fetch_add(1, Ordering::Relaxed).to_string());
    mkdir_at(&trash.dir, &slot, 0o700)?;
    let moved = open_subdir(&trash.dir, &slot)
        .and_then(|dir| rename_noreplace(&target.parent, target.name, &dir, target.name));
    if let Err(err) = moved {
        let _ = remove_at(&trash.dir, &slot, true);
        if err.kind() == io::ErrorKind::CrossesDevices {
            return Err(io::Error::new(
                io::ErrorKind::CrossesDevices,
                format!(
                    "{} is on another volume than the test trash in {}; \
                     a test trashes only under the temporary folder",
                    target.path.display(),
                    trash.path.display()
                ),
            ));
        }
        return Err(err);
    }
    Ok(())
}

struct TestTrash {
    /// Canonical.
    path: PathBuf,
    dir: File,
    /// The name of the next entry's folder.
    next: AtomicU64,
}

static TRASH: OnceLock<TestTrash> = OnceLock::new();

fn opened() -> io::Result<&'static TestTrash> {
    static MAKING: Mutex<()> = Mutex::new(());
    if let Some(trash) = TRASH.get() {
        return Ok(trash);
    }
    let _making = MAKING.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(trash) = TRASH.get() {
        return Ok(trash);
    }
    let made = make()?;
    let trash = TRASH.get_or_init(|| made);
    // SAFETY: `remove_at_exit` is an `extern "C"` function taking nothing.
    unsafe { libc::atexit(remove_at_exit) };
    Ok(trash)
}

fn make() -> io::Result<TestTrash> {
    let base = dunce::canonicalize(std::env::temp_dir())?;
    remove_stale(&base);
    let pid = std::process::id();
    let path = base.join(format!("{PREFIX}{pid}"));
    // One left by an earlier process of this pid, which is gone.
    if path.symlink_metadata().is_ok() {
        remove_all(&path)?;
    }
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(&path)?;
    // A folder's handle, as the moves are made from (`File::open` can't
    // open a folder on Windows).
    let dir: File = listing::open_dirs(&path, Access::Search)
        .map_err(|unreached| unreached.err)?
        .pop()
        .expect("the path's own folder is opened last");
    Ok(TestTrash {
        path,
        dir,
        next: AtomicU64::new(0),
    })
}

/// Removes the test trashes in `base` of processes no longer running.
fn remove_stale(base: &Path) {
    let Ok(entries) = fs::read_dir(base) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = (name.to_str())
            .and_then(|name| name.strip_prefix(PREFIX))
            .and_then(|pid| pid.parse::<u32>().ok())
            .filter(|&pid| pid > 0)
        else {
            continue;
        };
        if process_gone(pid) {
            let _ = remove_all(&entry.path());
        }
    }
}

/// Whether no process `pid` is running.
#[cfg(unix)]
fn process_gone(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only checks whether the process exists.
    (unsafe { libc::kill(pid, 0) }) != 0
        && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// Whether no process `pid` is running: none can be opened by it, which a
/// running one of another user's can't be either, so that is kept too.
#[cfg(windows)]
fn process_gone(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: always safe to call.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return io::Error::last_os_error().raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32);
    }
    // SAFETY: `process` is ours and not used after this.
    unsafe { CloseHandle(process) };
    false
}

/// Removes `path` and everything in it, giving each folder its owner's
/// access first, as a test may trash one its owner can't read.
fn remove_all(path: &Path) -> io::Result<()> {
    fn open_up(path: &Path) {
        let Ok(meta) = path.symlink_metadata() else {
            return;
        };
        if !meta.is_dir() {
            return;
        }
        #[cfg(unix)]
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                open_up(&entry.path());
            }
        }
    }
    open_up(path);
    fs::remove_dir_all(path)
}

extern "C" fn remove_at_exit() {
    let _ = std::panic::catch_unwind(|| {
        if let Some(trash) = TRASH.get() {
            let _ = remove_all(&trash.path);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{super::trash, *};

    #[test]
    fn trash_goes_to_the_test_trash_not_the_system_trash() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let name = format!("{}-gone.txt", root.file_name().unwrap().to_str().unwrap());
        fs::write(root.join(&name), "gone").unwrap();
        // What the file is, by its handle: its identity.
        let id = |path: &Path| listing::identity(&File::open(path).unwrap()).unwrap();
        let ino = id(&root.join(&name));
        let calls = real_calls();

        trash(&root, &[Path::new(&name)]).unwrap();

        assert!(!root.join(&name).exists());
        let found = trashed(&name);
        assert_eq!(found.len(), 1, "{found:?}");
        let trash = trash_dir();
        assert!(found[0].starts_with(&trash), "{found:?} not in {trash:?}");
        assert!(trash.starts_with(dunce::canonicalize(std::env::temp_dir()).unwrap()));
        assert_eq!(id(&found[0]), ino);
        assert_eq!(fs::read_to_string(&found[0]).unwrap(), "gone");
        assert_eq!(real_calls(), calls, "the system Trash was not reached");
    }

    #[test]
    fn real_trash_is_this_threads_alone() {
        let real = real_trash();
        assert!(REAL.get() > 0);
        std::thread::spawn(|| assert_eq!(REAL.get(), 0))
            .join()
            .unwrap();
        drop(real);
        assert_eq!(REAL.get(), 0);
    }
}
