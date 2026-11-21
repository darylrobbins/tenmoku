//! Where a build's daemon listens (D § 3): `$XDG_RUNTIME_DIR/tenmoku/`, else
//! `~/.cache/tenmoku-server/<build>/run/`, in a folder only this user can
//! enter. The socket, lock and log are named for the build, so two builds'
//! daemons sit side by side and never meet (no cross-build attach).
//!
//! The lock is the liveness test, as for the app's own socket
//! (`crates/fastedit/src/ipc.rs`): the daemon holds it for its whole life,
//! so a socket file is stale exactly when the lock is free, however the old
//! daemon ended and however many attaches race to replace it.

use std::fs::{self, DirBuilder, File, OpenOptions, TryLockError};
use std::io;
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt,
};
use std::path::{Path, PathBuf};

/// What a Unix socket's path may be, with room for the terminating NUL, on
/// the platform with the shorter limit (macOS's `sun_path`).
const SOCKET_PATH_MAX: usize = 104;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    /// The mode-700 folder that holds the rest.
    pub dir: PathBuf,
    pub socket: PathBuf,
    /// Held, with `flock`, by the live daemon, which writes its pid in it.
    pub lock: PathBuf,
    /// The detached daemon's stderr.
    pub log: PathBuf,
}

impl Paths {
    /// `build`'s files under `runtime_dir/tenmoku/` when there is a runtime
    /// dir, else under `home/.cache/tenmoku-server/<build>/run/`. A folder
    /// too long a path to hold a socket gives way to
    /// `/tmp/tenmoku-server-<uid>/`.
    pub fn new(build: &str, runtime_dir: Option<&Path>, home: Option<&Path>) -> io::Result<Paths> {
        if build.is_empty() || build.contains('/') || build.starts_with('.') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{build:?} can't name the daemon's files"),
            ));
        }
        let dir = match (runtime_dir, home) {
            (Some(run), _) => run.join("tenmoku"),
            (None, Some(home)) => home
                .join(".cache")
                .join("tenmoku-server")
                .join(build)
                .join("run"),
            (None, None) => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "neither XDG_RUNTIME_DIR nor HOME is set, so the daemon has nowhere to listen",
                ));
            }
        };
        let paths = Paths::in_dir(dir, build);
        if paths.socket.as_os_str().len() < SOCKET_PATH_MAX {
            return Ok(paths);
        }
        // SAFETY: getuid has no preconditions and can't fail.
        let uid = unsafe { libc::getuid() };
        Ok(Paths::in_dir(
            Path::new("/tmp").join(format!("tenmoku-server-{uid}")),
            build,
        ))
    }

    /// [`Paths::new`] from this process's `XDG_RUNTIME_DIR` and `HOME`. A
    /// relative or empty one counts as unset, as the XDG spec has it.
    pub fn from_env(build: &str) -> io::Result<Paths> {
        let var = |name| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        };
        Paths::new(
            build,
            var("XDG_RUNTIME_DIR").as_deref(),
            var("HOME").as_deref(),
        )
    }

    fn in_dir(dir: PathBuf, build: &str) -> Paths {
        let file = |ext: &str| dir.join(format!("{build}.{ext}"));
        Paths {
            socket: file("sock"),
            lock: file("lock"),
            log: file("log"),
            dir,
        }
    }

    /// Makes [`Paths::dir`] this user's alone, with its parents. One that's
    /// someone else's, or a symlink, is refused: `/tmp` is anyone's to
    /// create names in, and a daemon found through such a folder could be
    /// anyone's too.
    pub fn secure(&self) -> io::Result<()> {
        let dir = &self.dir;
        if let Some(parent) = dir.parent() {
            fs::create_dir_all(parent)?;
        }
        match DirBuilder::new().mode(0o700).create(dir) {
            Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
            _ => {}
        }
        let meta = fs::symlink_metadata(dir)?;
        // SAFETY: getuid has no preconditions and can't fail.
        if !meta.is_dir() || meta.uid() != unsafe { libc::getuid() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{} isn't this user's folder", dir.display()),
            ));
        }
        if meta.mode() & 0o777 != 0o700 {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    /// Takes the lock: `None` if another process holds it.
    pub fn take_lock(&self) -> io::Result<Option<File>> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .read(true)
            .mode(0o600)
            .open(&self.lock)?;
        match lock.try_lock() {
            Ok(()) => Ok(Some(lock)),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(e),
        }
    }

    /// The pid the live daemon wrote in the lock, if there is one.
    pub fn daemon_pid(&self) -> Option<u32> {
        let text = fs::read_to_string(&self.lock).ok()?;
        text.trim().parse().ok()
    }
}

/// The inode at `path`, to tell the file this process made from one put in
/// its place.
pub(crate) fn inode(path: &Path) -> Option<u64> {
    fs::symlink_metadata(path).ok().map(|m| m.ino())
}
