// SPDX-License-Identifier: Apache-2.0

//! Saved window layouts, one file per root folder: `<state>/layouts/<key>.json`
//! (see [`AppPaths::layouts_dir`](crate::AppPaths::layouts_dir)), where
//! `<key>` is [`layout_key`] of the canonical root. The file is
//! `{"version": 1, "root": "<root>", "windows": [entry, …]}`, an entry per
//! window, each a JSON object the app's shell writes, with its window's `id`
//! and when it was `saved` (milliseconds since the Unix epoch). A file of
//! another version, or one that doesn't parse, reads as empty. `root` is the
//! root folder the file is for, written with every write if it's UTF-8: what
//! [`restore_last_quit`] finds the file's root by. A file written before it was
//! there, or for a root that isn't UTF-8, has none; [`restore_last_quit`] then
//! looks for it among the ancestors of the absolute paths its entries hold
//! (an editor's file, say), by key.
//!
//! **Sessions.** Each entry is also stamped with the `session` of the
//! process that wrote it ([`session`]; an entry from before sessions has
//! none, and all such entries are one legacy session). A launch on the
//! folder restores only the last session's windows ([`LayoutStore::
//! restore_last_session`]): the windows open when the app last quit, or
//! crashed. It re-writes them under its own session, and deletes every
//! other entry no live window holds: older sessions' windows, and the
//! entries of file launches (marked `"restore": false`), which are never
//! restored. A cap bounds how many windows one launch reopens.
//!
//! **The last quit, across roots.** A launch on no folder in particular can
//! reopen every window the app had open when it last quit, whatever its
//! root ([`restore_last_quit`]): the last session is that of the newest
//! entry in the layouts directory that a folder launch may restore and no
//! live window holds, and each store whose newest such entry is of it is
//! restored as a launch on its folder restores it. Each window reopens on
//! the folder it was on, its entry's own `root` when it has one (a window
//! re-rooted since it opened), else its store's, and stays in its store.
//!
//! **Each window id has one live owner, across processes.** A window takes
//! a [`Claim`] on an id before it uses it: an exclusive lock on
//! `<key>.<id>.lock`, held for as long as the window is open, which the OS
//! drops if the process dies. A window restores from an entry only if it
//! can claim the entry's id ([`LayoutStore::restorable`]); otherwise that
//! entry's window is open somewhere, and the window takes a fresh id
//! ([`LayoutStore::claim_new`]).
//!
//! **Writes merge.** A window writes ([`LayoutStore::write`]) and deletes
//! ([`LayoutStore::delete`]) only its own entry, by its claim: under an
//! exclusive lock on `<key>.json.lock`, it reads the file as it is, puts
//! its entry in or takes it out, and puts the file back whole through a
//! temporary file. So windows on one root, in one process or several,
//! never drop each other's entries.

use std::{
    collections::hash_map::RandomState,
    fs::{self, File, OpenOptions},
    hash::{BuildHasher as _, Hash as _, Hasher as _},
    io,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use kiln_command::Value;

use crate::{
    platform::fs::private,
    trust::{create_dir, write},
};

/// The layout file's version.
pub const VERSION: u64 = 1;

/// A stable key for the canonical root folder `root`: the FNV-1a 64 hash
/// of its bytes, in hex, so roots that aren't UTF-8 have one too.
pub fn layout_key(root: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in root.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// A fresh window id: random, and never twice in a process.
fn fresh_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut hasher = RandomState::new().build_hasher();
    (NEXT.fetch_add(1, Ordering::Relaxed), std::process::id()).hash(&mut hasher);
    SystemTime::now().hash(&mut hasher);
    hasher.finish()
}

/// This process's session id: random, fixed for the life of the process.
pub fn session() -> u64 {
    static SESSION: OnceLock<u64> = OnceLock::new();
    *SESSION.get_or_init(fresh_id)
}

fn now_millis() -> u64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    since.map_or(0, |since| since.as_millis() as u64)
}

/// The id of a saved entry.
pub fn entry_id(entry: &Value) -> Option<u64> {
    entry.get("id")?.as_u64()
}

/// The session of a saved entry: `None` for one saved before sessions.
pub fn entry_session(entry: &Value) -> Option<u64> {
    entry.get("session")?.as_u64()
}

/// Whether a launch on the folder may restore `entry`: not a file
/// launch's.
fn restores(entry: &Value) -> bool {
    entry.get("restore").and_then(Value::as_bool) != Some(false)
}

/// What [`LayoutStore::restore_last_session`] found.
#[derive(Debug)]
pub struct LastSession {
    /// The last session's windows to restore, newest first.
    pub windows: Vec<(Claim, Value)>,
    /// How many saved windows it deleted.
    pub dropped: usize,
}

/// The saved layouts of one root folder's windows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutStore {
    dir: PathBuf,
    key: String,
    root: PathBuf,
}

/// A window's hold on its id: see the module docs. Dropping it releases
/// it; [`Claim::release`] can also remove its lock file.
#[derive(Debug)]
pub struct Claim {
    id: u64,
    path: PathBuf,
    _lock: File,
}

impl Claim {
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Releases the claim, and with `forget` removes its lock file, for an
    /// id whose entry has gone.
    pub fn release(self, forget: bool) {
        let path = self.path.clone();
        drop(self);
        if forget {
            fs::remove_file(path).ok();
        }
    }
}

impl LayoutStore {
    /// The layouts of the canonical root folder `root`, in `dir`.
    pub fn new(dir: impl Into<PathBuf>, root: &Path) -> Self {
        Self {
            dir: dir.into(),
            key: layout_key(root),
            root: root.to_owned(),
        }
    }

    /// The root folder the layouts are of.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The layout file.
    pub fn file(&self) -> PathBuf {
        self.dir.join(format!("{}.json", self.key))
    }

    fn claim_file(&self, id: u64) -> PathBuf {
        self.dir.join(format!("{}.{id:016x}.lock", self.key))
    }

    /// The entries, newest first, as the file is now.
    pub fn entries(&self) -> Vec<Value> {
        // Each write puts its entry last, so of two saved in the same
        // millisecond the later is newer.
        let mut entries = read(&self.file()).1;
        entries.reverse();
        entries.sort_by_key(|entry| {
            std::cmp::Reverse(entry.get("saved").and_then(Value::as_u64).unwrap_or(0))
        });
        entries
    }

    /// Claims `id`: `None` if a live window, in this process or another,
    /// holds it.
    pub fn claim(&self, id: u64) -> io::Result<Option<Claim>> {
        create_dir(&self.dir)?;
        let path = self.claim_file(id);
        let lock = private(
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false),
        )
        .open(&path)?;
        match lock.try_lock() {
            Ok(()) => Ok(Some(Claim {
                id,
                path,
                _lock: lock,
            })),
            Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(error)) => Err(error),
        }
    }

    /// Claims a fresh random id, for a window that restores no entry.
    pub fn claim_new(&self) -> io::Result<Claim> {
        loop {
            if let Some(claim) = self.claim(fresh_id())? {
                return Ok(claim);
            }
        }
    }

    /// Claims each entry no live window holds, newest first, with the
    /// entry: what a launch on the root restores, a window each.
    pub fn restorable(&self) -> Vec<(Claim, Value)> {
        self.entries()
            .into_iter()
            .filter_map(|entry| {
                let claim = self.claim(entry_id(&entry)?).ok()??;
                Some((claim, entry))
            })
            .collect()
    }

    /// What a launch on the folder restores: the windows of the last
    /// session, at most `max` (see the module docs). Entries a live window
    /// holds are never touched.
    pub fn restore_last_session(&self, max: usize) -> LastSession {
        let mut dropped = 0;
        let mut candidates = Vec::new();
        for (claim, entry) in self.restorable() {
            if restores(&entry) {
                candidates.push((claim, entry));
            } else {
                self.forget(claim);
                dropped += 1;
            }
        }
        let last = candidates.first().map(|(_, entry)| entry_session(entry));
        let mut windows = Vec::new();
        for (claim, entry) in candidates {
            if Some(entry_session(&entry)) == last && windows.len() < max {
                windows.push((claim, entry));
            } else {
                self.forget(claim);
                dropped += 1;
            }
        }
        // Oldest first, so their order stays.
        for (claim, entry) in windows.iter().rev() {
            if let Err(error) = self.write(claim, entry.clone()) {
                log::error!("can't keep a restored window's layout: {error}");
            }
        }
        LastSession { windows, dropped }
    }

    fn forget(&self, claim: Claim) {
        if let Err(error) = self.delete(&claim) {
            log::error!("can't delete a saved window's layout: {error}");
        }
        claim.release(true);
    }

    /// The newest entry no live window holds, claimed: what a new window
    /// on the root restores. A file launch's is skipped.
    pub fn newest_restorable(&self) -> Option<(Claim, Value)> {
        self.entries()
            .into_iter()
            .filter(restores)
            .find_map(|entry| {
                let claim = self.claim(entry_id(&entry)?).ok()??;
                Some((claim, entry))
            })
    }

    /// Puts `entry` in the file as `claim`'s window's, replacing its last,
    /// stamped with its id and the time: see the module docs.
    pub fn write(&self, claim: &Claim, mut entry: Value) -> io::Result<()> {
        if let Some(object) = entry.as_object_mut() {
            object.insert("id".into(), Value::from(claim.id));
            object.insert("saved".into(), Value::from(now_millis()));
            object.insert("session".into(), Value::from(session()));
        }
        self.update(claim.id, Some(entry))
    }

    /// Takes `claim`'s window's entry out of the file.
    pub fn delete(&self, claim: &Claim) -> io::Result<()> {
        self.update(claim.id, None)
    }

    /// Reads the file, swaps entry `id` for `entry`, and writes it back
    /// whole, under the file's lock.
    fn update(&self, id: u64, entry: Option<Value>) -> io::Result<()> {
        create_dir(&self.dir)?;
        let file = self.file();
        let lock = private(
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false),
        )
        .open(self.dir.join(format!("{}.json.lock", self.key)))?;
        lock.lock()?;
        let mut entries = read(&file).1;
        entries.retain(|other| entry_id(other) != Some(id));
        entries.extend(entry);
        let mut text = Value::from_iter([("version", Value::from(VERSION))]);
        if let (Some(object), Some(root)) = (text.as_object_mut(), self.root.to_str()) {
            object.insert("root".into(), Value::from(root));
        }
        if let Some(object) = text.as_object_mut() {
            object.insert("windows".into(), Value::Array(entries));
        }
        let text = serde_json::to_vec_pretty(&text).map_err(io::Error::other)?;
        // The lock is released when `lock` drops, after the write.
        write(&file, &text)
    }
}

/// The root the layout file at `path` records, if any, and its entries:
/// neither if it's missing, of another version, or bad.
fn read(path: &Path) -> (Option<PathBuf>, Vec<Value>) {
    let Ok(text) = fs::read(path) else {
        return (None, Vec::new());
    };
    let Ok(value) = serde_json::from_slice::<Value>(&text) else {
        log::warn!("{} isn't a layout file; it's ignored", path.display());
        return (None, Vec::new());
    };
    if value.get("version").and_then(Value::as_u64) != Some(VERSION) {
        return (None, Vec::new());
    }
    let root = value.get("root").and_then(Value::as_str).map(PathBuf::from);
    let entries = match value.get("windows") {
        Some(Value::Array(entries)) => (entries.iter())
            .filter(|entry| entry_id(entry).is_some())
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    (root, entries)
}

/// The root of a layout file, named for `key`, that records none (written
/// before files did): the empty root if `key` is its, or else the first of
/// the ancestors of the absolute paths in `entries`, at any depth, whose key
/// is `key`.
fn root_of(key: &str, entries: &[Value]) -> Option<PathBuf> {
    fn find(value: &Value, key: &str) -> Option<PathBuf> {
        match value {
            Value::String(path) => {
                let path = Path::new(path);
                if !path.is_absolute() {
                    return None;
                }
                let mut ancestors = path.ancestors();
                ancestors
                    .find(|root| layout_key(root) == key)
                    .map(Path::to_owned)
            }
            Value::Array(values) => values.iter().find_map(|value| find(value, key)),
            Value::Object(object) => object.values().find_map(|value| find(value, key)),
            _ => None,
        }
    }
    if layout_key(Path::new("")) == key {
        return Some(PathBuf::new());
    }
    entries.iter().find_map(|entry| find(entry, key))
}

/// A window the last quit left, claimed: its store, its claim, its entry,
/// and the folder it was on (the empty path for none).
#[derive(Debug)]
pub struct QuitWindow {
    pub store: LayoutStore,
    pub claim: Claim,
    pub entry: Value,
    pub root: PathBuf,
}

/// The folder a window saved as `entry`, in `store`, was on: the entry's
/// own `root` if it has one, else the store's.
pub fn entry_root(store: &LayoutStore, entry: &Value) -> PathBuf {
    match entry.get("root").and_then(Value::as_str) {
        Some(root) => PathBuf::from(root),
        None => store.root.clone(),
    }
}

/// The windows open when the app last quit (or crashed), in the layouts
/// directory `dir`, whatever their roots, claimed, newest first, at most
/// `max`: see the module docs. The claims are held from the moment the
/// last session is chosen, so another process can't take a window of it
/// and leave an older one in its place.
///
/// A window whose folder isn't `there` is passed over, its entry kept, for
/// a launch on its folder once it's back (until a later restore of its
/// store deletes it as an older session's). Of each store restored, the older sessions'
/// windows are deleted and the last session's re-written under this one,
/// as [`LayoutStore::restore_last_session`] does; a window past `max` is
/// left as it is, and so is every store not restored. File-launch entries
/// are left for a launch on their folder to delete.
///
/// A file that records no root is taken for the root its key is of, if
/// its entries show it (see the module docs); one whose root is still
/// unknown, or whose root's key isn't the file's (a file copied or
/// renamed), is passed over.
pub fn restore_last_quit(dir: &Path, max: usize, there: impl Fn(&Path) -> bool) -> Vec<QuitWindow> {
    let Ok(listing) = fs::read_dir(dir) else {
        return Vec::new();
    };
    // Each store's entries a folder launch may restore, claimed, newest
    // first.
    let mut stores = Vec::new();
    for file in listing.flatten() {
        let path = file.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let (root, entries) = read(&path);
        let key = path.file_stem().and_then(|stem| stem.to_str());
        let Some(root) = root.or_else(|| root_of(key?, &entries)) else {
            continue;
        };
        let store = LayoutStore::new(dir, &root);
        if store.file() != path {
            continue;
        }
        let candidates: Vec<_> = (store.restorable().into_iter())
            .filter(|(_, entry)| restores(entry))
            .collect();
        if !candidates.is_empty() {
            stores.push((store, candidates));
        }
    }
    let saved = |entry: &Value| entry.get("saved").and_then(Value::as_u64).unwrap_or(0);
    let newest = (stores.iter())
        .map(|(_, candidates)| &candidates[0].1)
        .max_by_key(|entry| saved(entry));
    let Some(last) = newest.map(entry_session) else {
        return Vec::new();
    };
    let mut windows = Vec::new();
    for (store, candidates) in stores {
        // Not one of the last session's stores: its claims go, untouched.
        if entry_session(&candidates[0].1) != last {
            continue;
        }
        for (claim, entry) in candidates {
            if entry_session(&entry) != last {
                store.forget(claim);
                continue;
            }
            let root = entry_root(&store, &entry);
            if there(&root) {
                windows.push(QuitWindow {
                    store: store.clone(),
                    claim,
                    entry,
                    root,
                });
            }
        }
    }
    // Newest first; of two saved in the same millisecond, by store, so the
    // order doesn't hang on the directory's.
    windows.sort_by(|a, b| {
        (saved(&b.entry).cmp(&saved(&a.entry))).then_with(|| a.store.key.cmp(&b.store.key))
    });
    windows.truncate(max);
    // Oldest first, so their order stays.
    for window in windows.iter().rev() {
        if let Err(error) = window.store.write(&window.claim, window.entry.clone()) {
            log::error!("can't keep a restored window's layout: {error}");
        }
    }
    windows
}

#[cfg(test)]
#[path = "layout_session_tests.rs"]
mod session_tests;
