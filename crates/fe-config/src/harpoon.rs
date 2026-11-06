//! Harpoon-style file marks: a short, hand-kept list of a project's files,
//! each in a numbered slot to jump to, kept per project in
//! `<state_dir>/harpoon/<key>.toml`, `<key>` a stable hash of the project's
//! canonical root ([`project_key`]).
//!
//! A [`MarkList`] holds project-relative paths, at most [`MAX_MARKS`],
//! each once, in slot order. Files that are gone stay listed, so a branch
//! switch doesn't lose them; the host shows them as missing.
//!
//! [`update`] is the cross-window entry point: it reads, changes and
//! writes back under an advisory lock, so windows and processes changing
//! one list at once all land. A file that can't be parsed reads as an
//! empty list and is left as it is until the next change replaces it.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// The most files a list holds: slots 1 to 9.
pub const MAX_MARKS: usize = 9;

/// What [`MarkList::add`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    /// Added at this index, the last.
    New(usize),
    /// Already listed, at this index; nothing changed.
    Already(usize),
    /// The list holds [`MAX_MARKS`]; nothing changed.
    Full,
    /// Not a project-relative path ([`is_relative_mark`]); nothing changed.
    Invalid,
}

/// A project's marked files, relative to its root, in slot order: index 0
/// is slot 1.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MarkList {
    files: Vec<PathBuf>,
}

/// Whether `path` can be a mark: relative, and only names (no `..`, `.`,
/// root or prefix), so it stays under the project's root.
pub fn is_relative_mark(path: &Path) -> bool {
    let mut components = path.components().peekable();
    components.peek().is_some() && components.all(|c| matches!(c, Component::Normal(_)))
}

impl MarkList {
    /// A list of `files`, in order, keeping the first of any repeated and
    /// at most [`MAX_MARKS`], and dropping any that isn't a
    /// [relative mark](is_relative_mark).
    pub fn from_files(files: impl IntoIterator<Item = PathBuf>) -> MarkList {
        let mut list = MarkList::default();
        for file in files {
            if list.files.len() == MAX_MARKS {
                break;
            }
            list.add(&file);
        }
        list
    }

    /// The marked files, in slot order.
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// The file at index `ix` (slot `ix + 1`).
    pub fn get(&self, ix: usize) -> Option<&Path> {
        self.files.get(ix).map(PathBuf::as_path)
    }

    /// The index of `path`, if it's listed.
    pub fn position(&self, path: &Path) -> Option<usize> {
        self.files.iter().position(|file| file == path)
    }

    /// Adds `path`, project-relative, in the next slot, unless it's
    /// listed already, the list is full or it isn't a relative mark.
    pub fn add(&mut self, path: &Path) -> Added {
        if !is_relative_mark(path) {
            return Added::Invalid;
        }
        if let Some(ix) = self.position(path) {
            return Added::Already(ix);
        }
        if self.files.len() >= MAX_MARKS {
            return Added::Full;
        }
        self.files.push(path.to_owned());
        Added::New(self.files.len() - 1)
    }

    /// Removes the file at index `ix`, the ones after moving up a slot.
    pub fn remove(&mut self, ix: usize) -> Option<PathBuf> {
        (ix < self.files.len()).then(|| self.files.remove(ix))
    }

    /// Moves the file at index `from` to index `to`, the ones between
    /// shifting a slot to make room. False, changing nothing, if either is
    /// out of range.
    pub fn move_to(&mut self, from: usize, to: usize) -> bool {
        let len = self.files.len();
        if from >= len || to >= len {
            return false;
        }
        let file = self.files.remove(from);
        self.files.insert(to, file);
        true
    }

    /// Reads the list at `path`. A missing file is an empty list; an
    /// unreadable or invalid one is an empty list with the error beside it,
    /// for the caller to log, the file left as it is.
    pub fn load(path: &Path) -> (MarkList, Option<io::Error>) {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return (MarkList::default(), None),
            Err(e) => return (MarkList::default(), Some(e)),
        };
        match toml::from_str::<MarkList>(&text) {
            Ok(list) => (MarkList::from_files(list.files), None),
            Err(e) => (
                MarkList::default(),
                Some(io::Error::new(io::ErrorKind::InvalidData, e)),
            ),
        }
    }

    /// Writes the list to `path` through a temp file and a rename, so a
    /// reader never sees half of it.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let text = toml::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
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
            fs::rename(&tmp, path)
        })();
        if written.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        written
    }
}

/// A stable name for the project at `root`: the 64-bit FNV-1a hash of its
/// canonical path's bytes, in hex. The same folder reached through a
/// symlink or a `..` gets the same key; a root that can't be canonicalized
/// (gone, say) hashes as given.
pub fn project_key(root: &Path) -> String {
    let canonical = fs::canonicalize(root).unwrap_or_else(|_| root.to_owned());
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in canonical.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// `<state_dir>/harpoon/<key>.toml`: the list of the project at `root`.
pub fn marks_file(state_dir: &Path, root: &Path) -> PathBuf {
    (state_dir.join("harpoon")).join(format!("{}.toml", project_key(root)))
}

/// [`marks_file`] in the user's state directory, if there is one.
pub fn marks_path(root: &Path) -> Option<PathBuf> {
    crate::paths::state_dir().map(|dir| marks_file(&dir, root))
}

/// Runs `change` on the list at `path` under the advisory lock on
/// `<path>.lock`, writing the result back if `change` says it changed it.
/// An invalid file counts as an empty list, replaced only if `change`
/// changes that; an unreadable one is left alone and its error returned.
/// Returns the list, as written or as read, and what `change` gave.
pub fn update<R>(
    path: &Path,
    change: impl FnOnce(&mut MarkList) -> (bool, R),
) -> io::Result<(MarkList, R)> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        // Before the first write there: a launch's own isn't earlier use.
        crate::first_run::note_writing(dir);
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
    let mut list = match MarkList::load(path) {
        (list, None) => list,
        (_, Some(e)) if e.kind() == io::ErrorKind::InvalidData => {
            log::warn!("invalid harpoon list {}: {e}", path.display());
            MarkList::default()
        }
        (_, Some(e)) => return Err(e),
    };
    let (changed, result) = change(&mut list);
    if changed {
        list.save(path)?;
    }
    Ok((list, result))
}
