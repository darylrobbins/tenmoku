//! A log file for a launch whose stderr would go nowhere. A detached server
//! and a LaunchServices launch both have fd 2 on `/dev/null`, so the logger's
//! lines, panics and a library's own complaints (gpui's `could not start
//! display link`) were all thrown away. [`install`] points fd 2 at an
//! append-mode file in that case, and leaves a terminal, a pipe or a file the
//! user redirected to as it is.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    os::unix::{fs::MetadataExt as _, io::AsRawFd as _},
    path::{Path, PathBuf},
};

/// The size past which the log is moved aside at the next launch.
const LIMIT: u64 = 5 * 1024 * 1024;

/// How a launch came about, for its first log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaunchKind {
    /// The respawned server of a terminal launch.
    Detached,
    /// Finder, the Dock or `open`.
    LaunchServices,
    /// Anything else.
    Foreground,
}

impl LaunchKind {
    pub(crate) fn of(launch_services: bool, foreground: bool, terminal: bool) -> Self {
        if launch_services {
            Self::LaunchServices
        } else if foreground && !terminal {
            Self::Detached
        } else {
            Self::Foreground
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Detached => "detached",
            Self::LaunchServices => "LaunchServices",
            Self::Foreground => "foreground",
        }
    }
}

/// What fd 2 is, as far as the decision goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stderr {
    /// `/dev/null`: same device and inode.
    Null,
    /// A terminal, a pipe, a socket or a file.
    Elsewhere,
}

fn redirects(stderr: Stderr) -> bool {
    stderr == Stderr::Null
}

fn stderr_kind(fd: i32) -> Stderr {
    let (Ok(null), Ok(err)) = (fs::metadata("/dev/null"), fstat(fd)) else {
        return Stderr::Elsewhere;
    };
    if null.dev() == err.0 && null.ino() == err.1 {
        Stderr::Null
    } else {
        Stderr::Elsewhere
    }
}

fn fstat(fd: i32) -> io::Result<(u64, u64)> {
    // SAFETY: `stat` is plain data; fstat only writes it.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: the descriptor is only inspected.
    if unsafe { libc::fstat(fd, &mut stat) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((stat.st_dev as u64, stat.st_ino as u64))
}

/// Where the log goes, from the environment.
fn log_path(var: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(var("HOME")?.join("Library/Logs/Tenmoku/tenmoku.log"))
    } else {
        let state = var("XDG_STATE_HOME")
            .filter(|path| path.is_absolute())
            .or_else(|| Some(var("HOME")?.join(".local/state")))?;
        Some(state.join("tenmoku/tenmoku.log"))
    }
}

/// Moves `path` to `path.1` if it is over `limit`, replacing the old one.
fn rotate(path: &Path, limit: u64) -> io::Result<()> {
    match fs::metadata(path) {
        Ok(meta) if meta.len() > limit => {
            let mut old = path.as_os_str().to_owned();
            old.push(".1");
            fs::rename(path, old)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// The first line of a launch.
fn launch_line(kind: LaunchKind, pid: u32, displays: Option<usize>) -> String {
    let displays = displays.map_or_else(|| "?".to_owned(), |n| n.to_string());
    format!(
        "[INFO tenmoku] launch: version {} pid {pid} kind {} displays {displays}\n",
        env!("CARGO_PKG_VERSION"),
        kind.name(),
    )
}

/// Rotates, then opens `path` for append and writes the launch line to it
/// before anything else can.
fn open(path: &Path, limit: u64, line: &str) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    rotate(path, limit)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())?;
    Ok(file)
}

/// Points `fd` at the log file `path` if it is `/dev/null`; whether it did.
fn redirect(fd: i32, path: &Path, limit: u64, line: &str) -> bool {
    if !redirects(stderr_kind(fd)) {
        return false;
    }
    let Ok(file) = open(path, limit, line) else {
        return false;
    };
    // SAFETY: both descriptors are open; dup2 replaces `fd` atomically.
    let done = unsafe { libc::dup2(file.as_raw_fd(), fd) };
    done >= 0
}

/// Points fd 2 at the log file if it is `/dev/null`. A failure leaves it as
/// it was: there is nowhere to say so.
pub(crate) fn install(kind: LaunchKind, displays: Option<usize>) {
    if !redirects(stderr_kind(2)) {
        return;
    }
    let Some(path) = log_path(|name| std::env::var_os(name).map(PathBuf::from)) else {
        return;
    };
    let line = launch_line(kind, std::process::id(), displays);
    redirect(2, &path, LIMIT, &line);
}

/// The number of active displays, where it can be had before the app runs.
pub(crate) fn display_count() -> Option<usize> {
    #[cfg(target_os = "macos")]
    {
        #[link(name = "CoreGraphics", kind = "framework")]
        unsafe extern "C" {
            fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
        }
        let mut count = 0u32;
        // SAFETY: a null list with a zero maximum only asks for the count.
        let status = unsafe { CGGetActiveDisplayList(0, std::ptr::null_mut(), &mut count) };
        (status == 0).then_some(count as usize)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(test)]
mod tests;
