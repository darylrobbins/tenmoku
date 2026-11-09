//! The recently opened files and folders, kept in `<state_dir>/recent.toml`
//! for Open Recent. Most recent first, capped, deduped by path.
//!
//! [`record`] is the cross-window entry point: it reads, records and writes
//! back under an advisory lock, so windows and processes recording at once
//! all land.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// The most folders kept.
pub const MAX_FOLDERS: usize = 20;
/// The most files kept.
pub const MAX_FILES: usize = 50;

/// What was opened, to be recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry<'a> {
    File(&'a Path),
    Folder(&'a Path),
}

/// Recently opened folders and files, most recent first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecentList {
    pub folders: Vec<PathBuf>,
    pub files: Vec<PathBuf>,
}

/// `<state_dir>/recent.toml`.
pub fn recent_path() -> Option<PathBuf> {
    crate::paths::state_dir().map(|dir| dir.join("recent.toml"))
}

/// Runs `op`, retrying it on Windows while it fails with `PermissionDenied` or
/// a sharing violation (raw error 32). Opening or renaming over a file that
/// another thread or process is replacing or reading can be refused for a
/// moment there, where Unix opens the old or the new file. Up to 10 tries,
/// 10 ms apart, then the last error is returned. Other errors return at once.
#[cfg(windows)]
fn retry_transient<T>(mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    const TRIES: u32 = 10;
    let mut tries = 1;
    loop {
        match op() {
            Err(e)
                if tries < TRIES
                    && (e.kind() == io::ErrorKind::PermissionDenied
                        || e.raw_os_error() == Some(32)) =>
            {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            result => return result,
        }
    }
}

/// Runs `op` once: only Windows refuses a file that is being replaced.
#[cfg(not(windows))]
fn retry_transient<T>(mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    op()
}

fn push_front(list: &mut Vec<PathBuf>, path: &Path, cap: usize) {
    list.retain(|p| p != path);
    list.insert(0, path.to_owned());
    list.truncate(cap);
}

impl RecentList {
    /// Reads the list at `path`. A missing file is an empty list; an
    /// unreadable or invalid one is an empty list with the error beside it,
    /// for the caller to log.
    pub fn load(path: &Path) -> (RecentList, Option<io::Error>) {
        let text = match retry_transient(|| fs::read_to_string(path)) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return (RecentList::default(), None),
            Err(e) => return (RecentList::default(), Some(e)),
        };
        match toml::from_str::<RecentList>(&text) {
            Ok(mut list) => {
                list.folders.truncate(MAX_FOLDERS);
                list.files.truncate(MAX_FILES);
                (list, None)
            }
            Err(e) => (
                RecentList::default(),
                Some(io::Error::new(io::ErrorKind::InvalidData, e)),
            ),
        }
    }

    /// Writes the list to `path` through a temp file and a rename, so a
    /// reader never sees half of it.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let text = toml::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty());
        if let Some(dir) = dir {
            fs::create_dir_all(dir)?;
        }
        // A unique name made exclusively (`create_new` does not follow a
        // planted symlink), so concurrent saves never share a temp file.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let (tmp, mut file) = loop {
            let mut name = path.as_os_str().to_owned();
            name.push(format!(
                ".{}.{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let tmp = PathBuf::from(name);
            match OpenOptions::new().write(true).create_new(true).open(&tmp) {
                Ok(file) => break (tmp, file),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        };
        let written = (|| {
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            drop(file);
            retry_transient(|| fs::rename(&tmp, path))
        })();
        if written.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        written?;
        Ok(())
    }

    /// Moves `path` to the front of the folders.
    pub fn record_folder(&mut self, path: &Path) {
        push_front(&mut self.folders, path, MAX_FOLDERS);
    }

    /// Moves `path` to the front of the files.
    pub fn record_file(&mut self, path: &Path) {
        push_front(&mut self.files, path, MAX_FILES);
    }

    /// Drops `path` from both lists.
    pub fn remove(&mut self, path: &Path) {
        self.folders.retain(|p| p != path);
        self.files.retain(|p| p != path);
    }

    pub fn clear(&mut self) {
        self.folders.clear();
        self.files.clear();
    }

    /// The entries whose paths still exist, for display. Stored entries go
    /// only by the cap, [`remove`](Self::remove) or [`clear`](Self::clear).
    pub fn existing(&self) -> RecentList {
        RecentList {
            folders: self
                .folders
                .iter()
                .filter(|p| p.exists())
                .cloned()
                .collect(),
            files: self.files.iter().filter(|p| p.exists()).cloned().collect(),
        }
    }
}

/// Runs `change` on the list at `path` under the advisory lock on
/// `<path>.lock`, writing the result back.
fn update(path: &Path, change: impl FnOnce(&mut RecentList)) -> io::Result<RecentList> {
    // Before the first write here: a launch's own isn't earlier use.
    crate::first_run::note_writing(path);
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let mut lock_name = path.as_os_str().to_owned();
    lock_name.push(".lock");
    let lock: File = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(PathBuf::from(lock_name))?;
    lock.lock()?;
    let mut list = match RecentList::load(path) {
        (list, None) => list,
        // Invalid contents are replaced rather than blocking every later
        // record; an unreadable file is left alone and the error returned.
        (_, Some(e)) if e.kind() == io::ErrorKind::InvalidData => {
            log::warn!("replacing invalid recent list {}: {e}", path.display());
            RecentList::default()
        }
        (_, Some(e)) => return Err(e),
    };
    change(&mut list);
    list.save(path)?;
    Ok(list)
}

/// Reads the list at `path`, records `entry`, and writes it back under an
/// advisory lock, so concurrent windows and processes all land. Returns the
/// list as written.
pub fn record(path: &Path, entry: Entry<'_>) -> io::Result<RecentList> {
    update(path, |list| match entry {
        Entry::File(p) => list.record_file(p),
        Entry::Folder(p) => list.record_folder(p),
    })
}

/// [`RecentList::remove`] under the lock, written back.
pub fn remove(path: &Path, entry: &Path) -> io::Result<RecentList> {
    update(path, |list| list.remove(entry))
}

/// [`RecentList::clear`] under the lock, written back.
pub fn clear(path: &Path) -> io::Result<RecentList> {
    update(path, RecentList::clear)
}
