// SPDX-License-Identifier: Apache-2.0

//! The host's data store for plugins: files under `<state dir>/plugin-data/<plugin id>/` that the
//! plugin reaches only through the `data` interface, never through WASI, so every byte can be
//! charged before it is written.
//!
//! One [`PluginData`] stands for a plugin's directory. Its counter (bytes used, files held) sits
//! behind one lock that every store of the plugin shares, the interactive one and each job's
//! worker store, and that Clear Data and uninstall take too. A call that grows anything takes
//! the lock, checks used + growth against the quota, charges the growth, does the write, and
//! gives back what a failed write didn't use: the check never follows the write. The lock is
//! held across the write, so two writers can't both pass the check, and nothing races a clear.
//!
//! [`PluginData::open`] hands out the one instance a directory has while any holder keeps it, so
//! a Clear Data on a plugin that is running shares the counter of its stores, and counts the
//! directory from disk each time: a file a crash left is counted, and so is one a previous
//! instance didn't know. A plugin over a lowered quota keeps reading and deleting; only growth
//! is refused.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

use kiln_core::platform;
use kiln_l10n::verbatim;

use crate::bindings::kiln::extension::data::Host;
use crate::bindings::{Error, HostState};
use crate::limits::DATA_QUOTA_DEFAULT;

/// The longest file name.
pub const MAX_NAME_LEN: usize = 128;

/// The most bytes one `read`, `write` or `replace` moves.
pub const MAX_IO_BYTES: usize = 16 << 20;

/// The most files a plugin holds.
pub const MAX_FILES: usize = 10_000;

/// The directory under the state directory that holds every plugin's data directory.
pub const DATA_DIR: &str = "plugin-data";

/// Why a write that would grow the plugin's data was refused.
pub const OVER_QUOTA: &str = verbatim!("over the data quota");

/// Why a write that would add a file past [`MAX_FILES`] was refused.
pub const TOO_MANY_FILES: &str = verbatim!("too many data files");

const BAD_NAME: &str = verbatim!(
    "a data file name is 1 to 128 characters of a-z, 0-9, '.', '_' and '-', not starting with '.'"
);
const TOO_BIG: &str = verbatim!("a data read or write is at most 16 MiB");
const DATA_IO: &str = verbatim!("the data store failed to read or write a file");
const NO_STORE: &str = verbatim!("the data store is not available");

/// What the counter holds, under [`Inner::ledger`].
#[derive(Debug)]
struct Ledger {
    /// Bytes the directory's files take, temporary files included, charged before they grow.
    used: u64,
    /// Files with a valid name.
    files: usize,
    quota: u64,
}

#[derive(Debug)]
struct Inner {
    dir: PathBuf,
    ledger: Mutex<Ledger>,
}

/// One plugin's data directory and its counter. Clones share both.
#[derive(Clone, Debug)]
pub struct PluginData {
    inner: Arc<Inner>,
}

/// The instance each live directory has, so every holder of one shares its lock.
fn registry() -> &'static Mutex<HashMap<PathBuf, Weak<Inner>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<PathBuf, Weak<Inner>>>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

fn held<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl PluginData {
    /// Plugin `plugin_id`'s data under `state_dir/plugin-data/`, counted from disk. The quota is
    /// [`DATA_QUOTA_DEFAULT`] until [`Self::set_quota`] says another. The directory isn't
    /// created until a file is.
    pub fn open(state_dir: &Path, plugin_id: &str) -> Result<Self, Error> {
        let unsafe_id = plugin_id.is_empty()
            || plugin_id == "."
            || plugin_id == ".."
            || plugin_id.contains(['/', '\\', '\0']);
        if unsafe_id {
            return Err(Error::InvalidArgument(NO_STORE.to_owned()));
        }
        let dir = state_dir.join(DATA_DIR).join(plugin_id);
        let inner = {
            let mut registry = held(registry());
            registry.retain(|_, inner| inner.strong_count() > 0);
            match registry.get(&dir).and_then(Weak::upgrade) {
                Some(inner) => inner,
                None => {
                    let inner = Arc::new(Inner {
                        dir: dir.clone(),
                        ledger: Mutex::new(Ledger {
                            used: 0,
                            files: 0,
                            quota: DATA_QUOTA_DEFAULT,
                        }),
                    });
                    registry.insert(dir, Arc::downgrade(&inner));
                    inner
                }
            }
        };
        let data = Self { inner };
        data.recount()?;
        Ok(data)
    }

    /// Counts the directory from disk. A temporary file a crash left is deleted: nothing
    /// refers to it.
    fn recount(&self) -> Result<(), Error> {
        let mut ledger = held(&self.inner.ledger);
        let (used, files) = self.scan()?;
        ledger.used = used;
        ledger.files = files;
        Ok(())
    }

    fn scan(&self) -> Result<(u64, usize), Error> {
        let entries = match fs::read_dir(&self.inner.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((0, 0)),
            Err(error) => return Err(failed("read", &error)),
        };
        let (mut used, mut files) = (0u64, 0usize);
        for entry in entries {
            let entry = entry.map_err(|error| failed("read", &error))?;
            let meta = match entry.metadata() {
                Ok(meta) => meta,
                // Gone since the listing: nothing to count.
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                // An entry that can't be measured can't be left out of the count.
                Err(error) => return Err(failed("read", &error)),
            };
            if !meta.is_file() {
                continue;
            }
            if valid_name(&entry.file_name().to_string_lossy()) {
                files += 1;
                used = used.saturating_add(meta.len());
            } else {
                // Nothing but a crash leaves a name the plugin couldn't give. One that can't be
                // deleted still takes its bytes on disk, so it counts.
                match fs::remove_file(entry.path()) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(_) => used = used.saturating_add(meta.len()),
                }
            }
        }
        Ok((used, files))
    }

    /// Sets the quota, in bytes. Lowering it below what is used refuses growth, never reads or
    /// deletes.
    pub fn set_quota(&self, bytes: u64) {
        held(&self.inner.ledger).quota = bytes;
    }

    /// Bytes used and the quota.
    pub fn usage(&self) -> (u64, u64) {
        let ledger = held(&self.inner.ledger);
        (ledger.used, ledger.quota)
    }

    fn path(&self, name: &str) -> Result<PathBuf, Error> {
        if valid_name(name) {
            Ok(self.inner.dir.join(name))
        } else {
            Err(Error::InvalidArgument(BAD_NAME.to_owned()))
        }
    }

    /// Up to `len` bytes of `file` from `offset`.
    pub fn read(&self, file: &str, offset: u64, len: u32) -> Result<Vec<u8>, Error> {
        let path = self.path(file)?;
        if len as usize > MAX_IO_BYTES {
            return Err(Error::InvalidArgument(TOO_BIG.to_owned()));
        }
        let mut open = File::open(&path).map_err(|error| io_error(file, "read", &error))?;
        open.seek(SeekFrom::Start(offset))
            .map_err(|error| failed("read", &error))?;
        let mut bytes = Vec::new();
        open.take(u64::from(len))
            .read_to_end(&mut bytes)
            .map_err(|error| failed("read", &error))?;
        Ok(bytes)
    }

    /// Writes `bytes` at `offset`, charging the growth first.
    pub fn write(&self, file: &str, offset: u64, bytes: &[u8]) -> Result<(), Error> {
        let path = self.path(file)?;
        if bytes.len() > MAX_IO_BYTES {
            return Err(Error::InvalidArgument(TOO_BIG.to_owned()));
        }
        let end = offset
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| Error::InvalidArgument(TOO_BIG.to_owned()))?;
        let mut ledger = held(&self.inner.ledger);
        let before = size_of(&path)?;
        // An empty write never extends a file, whatever its offset: it only creates one.
        let growth = if bytes.is_empty() {
            0
        } else {
            end.saturating_sub(before.unwrap_or(0))
        };
        let creates = before.is_none();
        charge(&mut ledger, growth, creates)?;
        let written = (|| {
            create_dir(&self.inner.dir)?;
            let mut open = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)?;
            open.seek(SeekFrom::Start(offset))?;
            injected(Fault::Write)?;
            open.write_all(bytes)
        })();
        if let Err(error) = written {
            settle(&mut ledger, &path, before, growth, creates);
            return Err(failed("write", &error));
        }
        Ok(())
    }

    /// Replaces `file` with `bytes`: the new size is charged while the old file still exists,
    /// and the old size is given back after the rename.
    pub fn replace(&self, file: &str, bytes: &[u8]) -> Result<(), Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = self.path(file)?;
        if bytes.len() > MAX_IO_BYTES {
            return Err(Error::InvalidArgument(TOO_BIG.to_owned()));
        }
        let mut ledger = held(&self.inner.ledger);
        let before = size_of(&path)?;
        let size = bytes.len() as u64;
        let creates = before.is_none();
        charge(&mut ledger, size, creates)?;
        // A leading dot: no name a plugin can give, so a crash-left one is told apart.
        let mut temporary_name = String::from(".");
        temporary_name.push_str(file);
        temporary_name.push('.');
        temporary_name.push_str(&std::process::id().to_string());
        temporary_name.push('.');
        temporary_name.push_str(&NEXT.fetch_add(1, Ordering::Relaxed).to_string());
        temporary_name.push_str(".tmp");
        let temporary = self.inner.dir.join(temporary_name);
        let replaced = (|| {
            create_dir(&self.inner.dir)?;
            let mut open = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            open.write_all(bytes)?;
            open.sync_all()?;
            drop(open);
            injected(Fault::Rename)?;
            platform::replace_file(&temporary, &path)
        })();
        match replaced {
            Ok(()) => {
                ledger.used = ledger.used.saturating_sub(before.unwrap_or(0));
                Ok(())
            }
            Err(error) => {
                // What the temporary file still takes stays charged: a file that can't be
                // deleted (Windows may hold it) is on disk, whatever the replace did.
                // A size that can't be established is the whole charge: it stays.
                let left = match remove(&temporary) {
                    Ok(()) => 0,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
                    Err(_) => match stat(&temporary) {
                        Ok(meta) => meta.len(),
                        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
                        Err(_) => size,
                    },
                };
                ledger.used = ledger.used.saturating_sub(size.saturating_sub(left));
                if creates {
                    ledger.files = ledger.files.saturating_sub(1);
                }
                Err(failed("replace", &error))
            }
        }
    }

    /// Cuts `file` to `len` bytes, or grows it, charging the growth first.
    pub fn truncate(&self, file: &str, len: u64) -> Result<(), Error> {
        let path = self.path(file)?;
        let mut ledger = held(&self.inner.ledger);
        let Some(before) = size_of(&path)? else {
            return Err(Error::NotFound(file.to_owned()));
        };
        let growth = len.saturating_sub(before);
        charge(&mut ledger, growth, false)?;
        let cut = OpenOptions::new()
            .write(true)
            .open(&path)
            .and_then(|open| injected(Fault::Write).and_then(|()| open.set_len(len)));
        match cut {
            Ok(()) => {
                ledger.used = ledger.used.saturating_sub(before.saturating_sub(len));
                Ok(())
            }
            Err(error) => {
                settle(&mut ledger, &path, Some(before), growth, false);
                Err(failed("truncate", &error))
            }
        }
    }

    /// Deletes `file`; one that doesn't exist is fine.
    pub fn delete(&self, file: &str) -> Result<(), Error> {
        let path = self.path(file)?;
        let mut ledger = held(&self.inner.ledger);
        let before = size_of(&path)?;
        match fs::remove_file(&path) {
            Ok(()) => {
                ledger.used = ledger.used.saturating_sub(before.unwrap_or(0));
                if before.is_some() {
                    ledger.files = ledger.files.saturating_sub(1);
                }
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(failed("delete", &error)),
        }
    }

    /// Every file and its size, by name.
    pub fn list(&self) -> Result<Vec<(String, u64)>, Error> {
        let _ledger = held(&self.inner.ledger);
        let entries = match fs::read_dir(&self.inner.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(failed("list", &error)),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| failed("list", &error))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Ok(meta) = entry.metadata()
                && meta.is_file()
                && valid_name(&name)
            {
                files.push((name, meta.len()));
            }
        }
        files.sort();
        Ok(files)
    }

    /// Deletes every file, under the lock, so no write races it. Clear Data.
    pub fn clear(&self) -> Result<(), Error> {
        let mut ledger = held(&self.inner.ledger);
        let result = self.remove_files();
        // Whatever is left on a failure is what the counter holds.
        let (used, files) = self.scan()?;
        ledger.used = used;
        ledger.files = files;
        result
    }

    /// Deletes the plugin's data directory, under the lock: what an uninstall does.
    pub fn remove(&self) -> Result<(), Error> {
        let mut ledger = held(&self.inner.ledger);
        match fs::remove_dir_all(&self.inner.dir) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                let (used, files) = self.scan()?;
                ledger.used = used;
                ledger.files = files;
                return Err(failed("delete", &error));
            }
        }
        ledger.used = 0;
        ledger.files = 0;
        Ok(())
    }

    fn remove_files(&self) -> Result<(), Error> {
        let entries = match fs::read_dir(&self.inner.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(failed("clear", &error)),
        };
        for entry in entries {
            let entry = entry.map_err(|error| failed("clear", &error))?;
            match fs::remove_file(entry.path()) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(failed("clear", &error)),
            }
        }
        Ok(())
    }
}

/// Whether `name` is a flat data file name.
fn valid_name(name: &str) -> bool {
    (1..=MAX_NAME_LEN).contains(&name.len())
        && !name.starts_with('.')
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

/// Charges `growth` bytes, and a file if `creates`, against the quota: before anything is
/// written. Nothing is charged for a refusal. A write that grows nothing is never refused, so
/// a plugin over a lowered quota can still overwrite in place.
fn charge(ledger: &mut Ledger, growth: u64, creates: bool) -> Result<(), Error> {
    if growth > 0 && ledger.used.saturating_add(growth) > ledger.quota {
        return Err(Error::Failed(OVER_QUOTA.to_owned()));
    }
    if creates && ledger.files >= MAX_FILES {
        return Err(Error::Failed(TOO_MANY_FILES.to_owned()));
    }
    ledger.used += growth;
    if creates {
        ledger.files += 1;
    }
    Ok(())
}

/// After a failed write that was charged `growth`: gives back what the file didn't take, from
/// its real size. `before` is its size when the call began, or `None` if it didn't exist.
fn settle(ledger: &mut Ledger, path: &Path, before: Option<u64>, growth: u64, creates: bool) {
    let after = match stat(path) {
        Ok(meta) => Some(meta.len()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        // What the file holds now can't be established: the charge stays, name and bytes, until
        // the next load counts the directory again.
        Err(_) => return,
    };
    let grew = after
        .unwrap_or(before.unwrap_or(0))
        .saturating_sub(before.unwrap_or(0));
    ledger.used = ledger.used.saturating_sub(growth.saturating_sub(grew));
    if creates && after.is_none() {
        ledger.files = ledger.files.saturating_sub(1);
    }
}

/// The size of the file at `path`, or `None` if there is none.
fn size_of(path: &Path) -> Result<Option<u64>, Error> {
    match stat(path) {
        Ok(meta) if meta.is_file() => Ok(Some(meta.len())),
        Ok(_) => Err(Error::Failed(NO_STORE.to_owned())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(failed("stat", &error)),
    }
}

fn create_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)
}

fn io_error(file: &str, action: &'static str, error: &io::Error) -> Error {
    if error.kind() == io::ErrorKind::NotFound {
        Error::NotFound(file.to_owned())
    } else {
        failed(action, error)
    }
}

fn failed(action: &'static str, error: &io::Error) -> Error {
    log::warn!("plugin data {action} failed: {error}");
    Error::Failed(DATA_IO.to_owned())
}

/// The data slice of [`HostState`]: the plugin's [`PluginData`], shared by its interactive store
/// and every worker store. The default has none: every import is unimplemented.
#[derive(Clone, Debug, Default)]
pub struct State {
    data: Option<PluginData>,
}

impl State {
    pub fn new(data: PluginData) -> Self {
        Self { data: Some(data) }
    }

    /// Whether this slice has a store.
    pub fn is_set(&self) -> bool {
        self.data.is_some()
    }

    fn data(&self, import: &str) -> Result<&PluginData, Error> {
        self.data
            .as_ref()
            .map_or_else(|| crate::bindings::unimplemented(import), Ok)
    }
}

impl Host for HostState {
    fn read(&mut self, file: String, offset: u64, len: u32) -> Result<Vec<u8>, Error> {
        self.data
            .data("kiln:extension/data.read")?
            .read(&file, offset, len)
    }

    fn write(&mut self, file: String, offset: u64, bytes: Vec<u8>) -> Result<(), Error> {
        self.data
            .data("kiln:extension/data.write")?
            .write(&file, offset, &bytes)
    }

    fn replace(&mut self, file: String, bytes: Vec<u8>) -> Result<(), Error> {
        self.data
            .data("kiln:extension/data.replace")?
            .replace(&file, &bytes)
    }

    fn truncate(&mut self, file: String, len: u64) -> Result<(), Error> {
        self.data
            .data("kiln:extension/data.truncate")?
            .truncate(&file, len)
    }

    fn delete(&mut self, file: String) -> Result<(), Error> {
        self.data.data("kiln:extension/data.delete")?.delete(&file)
    }

    fn list(&mut self) -> Result<Vec<(String, u64)>, Error> {
        self.data.data("kiln:extension/data.list")?.list()
    }

    fn usage(&mut self) -> (u64, u64) {
        self.data.data.as_ref().map_or((0, 0), PluginData::usage)
    }
}

/// What a test makes fail, once each, in a call of this thread.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    Write,
    Rename,
    Stat,
    Remove,
}

#[cfg(test)]
thread_local! {
    /// Each armed fault, and how many calls of its kind pass first.
    static FAULTS: std::cell::RefCell<Vec<(Fault, usize)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Fails with an injected error if a test armed `fault` and its passes are used up, once.
#[cfg(test)]
fn injected(fault: Fault) -> io::Result<()> {
    let fires = FAULTS.with_borrow_mut(|faults| {
        let at = faults.iter().position(|(armed, _)| *armed == fault)?;
        if faults[at].1 > 0 {
            faults[at].1 -= 1;
            return Some(false);
        }
        faults.remove(at);
        Some(true)
    });
    if fires == Some(true) {
        Err(io::Error::other("injected failure"))
    } else {
        Ok(())
    }
}

#[cfg(not(test))]
#[derive(Clone, Copy)]
enum Fault {
    Write,
    Rename,
    Stat,
    Remove,
}

#[cfg(not(test))]
#[allow(clippy::unnecessary_wraps)]
fn injected(_: Fault) -> io::Result<()> {
    Ok(())
}

/// [`fs::metadata`], which a test can make fail.
fn stat(path: &Path) -> io::Result<fs::Metadata> {
    injected(Fault::Stat)?;
    fs::metadata(path)
}

/// [`fs::remove_file`], which a test can make fail.
fn remove(path: &Path) -> io::Result<()> {
    injected(Fault::Remove)?;
    fs::remove_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Arms `fault` to fail after `pass` calls of its kind have gone through.
    fn arm(fault: Fault, pass: usize) {
        FAULTS.with_borrow_mut(|armed| armed.push((fault, pass)));
    }

    fn open(name: &str, quota: u64) -> (PluginData, PathBuf) {
        let state =
            std::env::temp_dir().join(format!("kiln-data-unit-{}-{name}", std::process::id()));
        fs::remove_dir_all(&state).ok();
        let data = PluginData::open(&state, "p").unwrap();
        data.set_quota(quota);
        (data, state)
    }

    fn over_quota() -> Error {
        Error::Failed(OVER_QUOTA.to_owned())
    }

    /// A replace whose rename fails, whose temporary file then can't be deleted or measured,
    /// keeps its whole charge: what is on disk is unknown, so none of it is refunded.
    #[test]
    fn a_failed_replace_with_an_unknown_leftover_keeps_its_charge() {
        let (data, state) = open("replace", 100);
        arm(Fault::Rename, 0);
        arm(Fault::Remove, 0);
        // The call's own size check passes; the check of the leftover fails.
        arm(Fault::Stat, 1);
        assert!(data.replace("f", &[1; 30]).is_err());
        assert_eq!(data.usage().0, 30);
        assert_eq!(data.write("g", 0, &[2; 80]), Err(over_quota()));
        // A recount, as a load does, sees the real directory.
        let reopened = PluginData::open(&state, "p").unwrap();
        assert_eq!(reopened.usage().0, 0);
        fs::remove_dir_all(&state).ok();
    }

    /// A replace whose rename fails, whose temporary file is deleted, gives its charge back.
    #[test]
    fn a_failed_replace_that_cleans_up_refunds_its_charge() {
        let (data, state) = open("replace-clean", 100);
        arm(Fault::Rename, 0);
        assert!(data.replace("f", &[1; 30]).is_err());
        assert_eq!(data.usage().0, 0);
        assert_eq!(data.list().unwrap(), vec![]);
        fs::remove_dir_all(&state).ok();
    }

    /// A write that fails, then can't be measured, keeps its charge; measured, it gives back
    /// what the file didn't take.
    #[test]
    fn a_failed_write_that_cannot_be_measured_keeps_its_charge() {
        let (data, state) = open("write", 100);
        data.write("f", 0, &[1; 10]).unwrap();
        arm(Fault::Write, 0);
        assert!(data.write("f", 10, &[1; 20]).is_err());
        assert_eq!(data.usage().0, 10, "measured: the 20 are given back");
        arm(Fault::Write, 0);
        arm(Fault::Stat, 1);
        assert!(data.write("f", 10, &[1; 20]).is_err());
        assert_eq!(data.usage().0, 30, "unmeasured: the charge stays");
        assert_eq!(data.write("g", 0, &[2; 80]), Err(over_quota()));
        assert_eq!(PluginData::open(&state, "p").unwrap().usage().0, 10);
        fs::remove_dir_all(&state).ok();
    }

    /// The same for a truncate that grows and fails, and a new file that fails to be written
    /// and can't be measured keeps its name too.
    #[test]
    fn a_failed_truncate_or_new_file_that_cannot_be_measured_keeps_its_charge() {
        let (data, state) = open("truncate", 100);
        data.write("f", 0, &[1; 10]).unwrap();
        arm(Fault::Write, 0);
        arm(Fault::Stat, 1);
        assert!(data.truncate("f", 60).is_err());
        assert_eq!(data.usage().0, 60);
        let (data, other) = open("new-file", 100);
        arm(Fault::Write, 0);
        arm(Fault::Stat, 1);
        assert!(data.write("new", 0, &[1; 20]).is_err());
        assert_eq!(data.usage().0, 20);
        assert_eq!(data.inner.ledger.lock().unwrap().files, 1);
        fs::remove_dir_all(&state).ok();
        fs::remove_dir_all(&other).ok();
    }
}
