//! Local history: the past texts of each file, by path, newest first.
//!
//! A [`LocalHistory`] keeps one private directory per file under its own:
//!
//! ```text
//! <dir>/.lock
//! <dir>/<key>/path                  the file's absolute path, as bytes
//! <dir>/<key>/<unix_millis>-<n>.saved
//! <dir>/<key>/<unix_millis>-<n>.external
//! ```
//!
//! `<key>` is the FNV-1a 64 of the path's bytes as 16 hex digits, written here
//! to stay the same across Rust versions. A key whose `path` names another
//! file takes the next free suffix, `<key>-1` and so on. A file's directory
//! appears by renaming a finished temporary one, so it always has its `path`,
//! and an entry appears by renaming a finished temporary file, so it is always
//! a whole text.
//!
//! `<n>` counts up within a file's directory and orders its entries, so the
//! newest is the one recorded last even if the clock moved back between them.
//! `<unix_millis>` is when the entry was recorded, and the age limit and
//! [`LocalHistory::prune`]'s order across files go by it.
//!
//! Stores on one directory may run in any number of windows and processes:
//! [`LocalHistory::record`] and [`LocalHistory::prune`] each hold the `.lock`
//! file exclusively, which keeps `<n>` and the suffixes unique, makes "equals
//! the newest entry" exact, and means any temporary file a prune sees was left
//! by a crash. Reads take no lock, and treat a file removed meanwhile as gone.
//!
//! Entries hold whatever the file held, so the directories are mode 0700 and
//! the files 0600.

use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const LOCK_NAME: &str = ".lock";
const PATH_NAME: &str = "path";
const KEY_LEN: usize = 16;

/// What a [`LocalHistory`] keeps.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most entries a file keeps; older ones go when one is recorded.
    /// Zero records nothing.
    pub max_entries_per_file: usize,
    /// Entries older than this go, from a file when it records and from every
    /// file on a prune.
    pub max_age: Duration,
    /// A text longer than this, in bytes, isn't recorded.
    pub max_text_len: u64,
    /// The most bytes of text the whole store keeps after a prune.
    pub max_total_len: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entries_per_file: 100,
            max_age: Duration::from_secs(30 * 24 * 60 * 60),
            max_text_len: 4 * 1024 * 1024,
            max_total_len: 256 * 1024 * 1024,
        }
    }
}

/// Why an entry was recorded.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Reason {
    /// The user saved the file.
    Saved,
    /// The file changed on disk, from outside the editor.
    External,
}

impl Reason {
    fn extension(self) -> &'static str {
        match self {
            Self::Saved => "saved",
            Self::External => "external",
        }
    }

    fn from_extension(extension: &str) -> Option<Self> {
        [Self::Saved, Self::External]
            .into_iter()
            .find(|reason| reason.extension() == extension)
    }
}

/// Names an entry among those of one file.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct EntryId {
    millis: u64,
    n: u64,
    reason: Reason,
}

impl EntryId {
    fn file_name(self) -> String {
        format!("{}-{}.{}", self.millis, self.n, self.reason.extension())
    }

    /// Parses exactly what [`EntryId::file_name`] writes.
    fn parse(name: &str) -> Option<Self> {
        let (stem, extension) = name.rsplit_once('.')?;
        let (millis, n) = stem.split_once('-')?;
        Some(Self {
            millis: canonical(millis)?,
            n: canonical(n)?,
            reason: Reason::from_extension(extension)?,
        })
    }

    fn time(self) -> Option<SystemTime> {
        UNIX_EPOCH.checked_add(Duration::from_millis(self.millis))
    }
}

/// One past text of a file, as [`LocalHistory::entries`] lists it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: EntryId,
    /// When it was recorded, to the millisecond.
    pub time: SystemTime,
    pub reason: Reason,
    /// The text's length in bytes.
    pub len: u64,
}

/// What a [`LocalHistory::prune`] removed.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    /// Entries removed, by age or to fit the total limit.
    pub entries: usize,
    /// The bytes of text those held.
    pub bytes: u64,
}

/// A directory of past texts; see the [module docs](self).
#[derive(Clone, Debug)]
pub struct LocalHistory {
    dir: PathBuf,
    limits: Limits,
}

impl LocalHistory {
    /// A store in `dir` with the default [`Limits`]. The directory is created,
    /// mode 0700, on the first record.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_limits(dir, Limits::default())
    }

    pub fn with_limits(dir: impl Into<PathBuf>, limits: Limits) -> Self {
        Self {
            dir: dir.into(),
            limits,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Records `text` as the newest entry of `path`, then drops that file's
    /// entries past the count and age limits. Returns `None`, recording
    /// nothing, if the text is over the size limit or equals the newest entry
    /// that's within the age limit, whatever that entry's reason.
    ///
    /// A relative `path` means the file under the current directory.
    pub fn record(
        &self,
        path: &Path,
        text: &[u8],
        reason: Reason,
        now: SystemTime,
    ) -> io::Result<Option<EntryId>> {
        if text.len() as u64 > self.limits.max_text_len || self.limits.max_entries_per_file == 0 {
            return Ok(None);
        }
        let path = absolute_bytes(path)?;
        create_private_dir(&self.dir)?;
        let lock = self.open_lock()?;
        lock.lock()?;
        let slot = match self.find_slot(&path)? {
            Some(slot) => slot,
            None => self.claim_slot(&path)?,
        };

        let scan = scan_slot(&slot)?;
        let n = scan.entries.first().map_or(0, |newest| newest.id.n + 1);
        let (live, expired): (Vec<_>, Vec<_>) = scan
            .entries
            .into_iter()
            .partition(|entry| !self.is_expired(entry, now));
        remove_entries(&slot, &expired)?;
        if let Some(newest) = live.first()
            && newest.len == text.len() as u64
            && fs::read(slot.join(newest.id.file_name()))? == text
        {
            return Ok(None);
        }

        let millis = unix_millis(now);
        let id = EntryId { millis, n, reason };
        let name = id.file_name();
        write_atomically(&slot.join(&name), &slot.join(temp_name(&name)), text)?;
        sync_dir(&slot)?;
        // The new entry counts towards the limit.
        let over = live
            .len()
            .saturating_sub(self.limits.max_entries_per_file - 1);
        remove_entries(&slot, &live[live.len() - over..])?;
        Ok(Some(id))
    }

    /// The entries of `path`, newest first; empty for a file never recorded.
    pub fn entries(&self, path: &Path) -> io::Result<Vec<Entry>> {
        let path = absolute_bytes(path)?;
        match self.find_slot(&path)? {
            Some(slot) => Ok(scan_slot(&slot)?.entries),
            None => Ok(Vec::new()),
        }
    }

    /// The text of the entry `id` of `path`. Fails with
    /// [`io::ErrorKind::NotFound`] if it's no longer kept.
    pub fn read(&self, path: &Path, id: EntryId) -> io::Result<Vec<u8>> {
        let path = absolute_bytes(path)?;
        let Some(slot) = self.find_slot(&path)? else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                kiln_l10n::tr!(
                    "buffer-local-history-none",
                    path = format!("{:?}", String::from_utf8_lossy(&path))
                )
                .to_string(),
            ));
        };
        fs::read(slot.join(id.file_name()))
    }

    /// Applies the age limit to every file, then removes the oldest entries
    /// across all files until their texts fit the total limit. Files left
    /// with no entries lose their directories, and temporary files left by a
    /// crash go. Files the store didn't name are left alone, along with the
    /// directories that hold them.
    pub fn prune(&self, now: SystemTime) -> io::Result<Pruned> {
        let lock = match self.open_lock() {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Pruned::default());
            }
            lock => lock?,
        };
        lock.lock()?;

        let mut slots = Vec::new();
        for item in fs::read_dir(&self.dir)? {
            let item = item?;
            let name = item.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !item.file_type()?.is_dir() {
                continue;
            }
            if is_temp_slot_name(name) {
                remove_dir_all_if_present(&item.path())?;
            } else if is_slot_name(name) {
                slots.push(item.path());
            }
        }

        let mut pruned = Pruned::default();
        let mut kept = Vec::new();
        let mut leftovers = Vec::with_capacity(slots.len());
        for (index, slot) in slots.iter().enumerate() {
            let scan = scan_slot(slot)?;
            for temp in &scan.temps {
                remove_if_present(temp)?;
            }
            for entry in scan.entries {
                if self.is_expired(&entry, now) {
                    remove_if_present(&slot.join(entry.id.file_name()))?;
                    pruned.entries += 1;
                    pruned.bytes += entry.len;
                } else {
                    kept.push((index, entry));
                }
            }
            leftovers.push(scan.others);
        }

        let mut total: u64 = kept.iter().map(|(_, entry)| entry.len).sum();
        if total > self.limits.max_total_len {
            kept.sort_by_key(|(_, entry)| (entry.id.millis, entry.id.n));
            let mut fitting = 0;
            for (index, entry) in &kept {
                if total <= self.limits.max_total_len {
                    break;
                }
                remove_if_present(&slots[*index].join(entry.id.file_name()))?;
                total -= entry.len;
                pruned.entries += 1;
                pruned.bytes += entry.len;
                fitting += 1;
            }
            kept.drain(..fitting);
        }

        let occupied: BTreeSet<usize> = kept.iter().map(|(index, _)| *index).collect();
        for (index, slot) in slots.iter().enumerate() {
            if !occupied.contains(&index) && !leftovers[index] {
                remove_if_present(&slot.join(PATH_NAME))?;
                remove_dir_if_present(slot)?;
            }
        }
        Ok(pruned)
    }

    fn is_expired(&self, entry: &Entry, now: SystemTime) -> bool {
        // An entry from the future (the clock moved back) has no age yet.
        now.duration_since(entry.time)
            .is_ok_and(|age| age > self.limits.max_age)
    }

    /// The directory of the file with these path bytes.
    fn find_slot(&self, path: &[u8]) -> io::Result<Option<PathBuf>> {
        for (_, slot) in self.slots_of_key(&key_of(path))? {
            match fs::read(slot.join(PATH_NAME)) {
                Ok(bytes) if bytes == path => return Ok(Some(slot)),
                Ok(_) => {}
                // Removed by a prune, or never a directory.
                Err(error) if is_gone(&error) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    /// Everything named `<key>` or `<key>-<suffix>`, by suffix. It lists the
    /// store rather than trying the names in turn because a prune can remove
    /// `<key>` and leave `<key>-1`.
    fn slots_of_key(&self, key: &str) -> io::Result<Vec<(u64, PathBuf)>> {
        let items = match fs::read_dir(&self.dir) {
            Ok(items) => items,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut slots = Vec::new();
        for item in items {
            let item = item?;
            if let Some(suffix) = item
                .file_name()
                .to_str()
                .and_then(|name| suffix_of(name, key))
            {
                slots.push((suffix, item.path()));
            }
        }
        slots.sort();
        Ok(slots)
    }

    /// Makes the directory of a file that has none, under the first free name
    /// of its key.
    fn claim_slot(&self, path: &[u8]) -> io::Result<PathBuf> {
        let key = key_of(path);
        let used: BTreeSet<u64> = self
            .slots_of_key(&key)?
            .into_iter()
            .map(|(suffix, _)| suffix)
            .collect();
        let name = match (0..).find(|suffix| !used.contains(suffix)) {
            Some(0) | None => key,
            Some(suffix) => format!("{key}-{suffix}"),
        };
        let slot = self.dir.join(&name);
        let temp = self.dir.join(temp_name(&name));
        remove_dir_all_if_present(&temp)?;
        let result = (|| {
            create_private_dir(&temp)?;
            write_atomically(
                &temp.join(PATH_NAME),
                &temp.join(temp_name(PATH_NAME)),
                path,
            )?;
            sync_dir(&temp)?;
            fs::rename(&temp, &slot)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&temp);
        }
        result?;
        sync_dir(&self.dir)?;
        Ok(slot)
    }

    fn open_lock(&self) -> io::Result<File> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(self.dir.join(LOCK_NAME))
    }
}

/// What one file's directory holds.
struct Scan {
    /// Newest first.
    entries: Vec<Entry>,
    /// Temporary files of entries.
    temps: Vec<PathBuf>,
    /// Whether it holds anything the store didn't name.
    others: bool,
}

fn scan_slot(slot: &Path) -> io::Result<Scan> {
    let mut scan = Scan {
        entries: Vec::new(),
        temps: Vec::new(),
        others: false,
    };
    let items = match fs::read_dir(slot) {
        Ok(items) => items,
        Err(error) if is_gone(&error) => return Ok(scan),
        Err(error) => return Err(error),
    };
    for item in items {
        let item = item?;
        let name = item.file_name();
        let name = name.to_str();
        if name == Some(PATH_NAME) {
            continue;
        }
        if name.is_some_and(is_temp_entry_name) {
            scan.temps.push(item.path());
            continue;
        }
        let Some(id) = name.and_then(EntryId::parse) else {
            scan.others = true;
            continue;
        };
        let metadata = match item.metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if !metadata.is_file() {
            scan.others = true;
            continue;
        }
        let Some(time) = id.time() else {
            scan.others = true;
            continue;
        };
        scan.entries.push(Entry {
            id,
            time,
            reason: id.reason,
            len: metadata.len(),
        });
    }
    scan.entries
        .sort_by_key(|entry| std::cmp::Reverse((entry.id.n, entry.id.millis)));
    Ok(scan)
}

fn remove_entries(slot: &Path, entries: &[Entry]) -> io::Result<()> {
    for entry in entries {
        remove_if_present(&slot.join(entry.id.file_name()))?;
    }
    Ok(())
}

/// The bytes that identify a file: its absolute path's, on Unix whatever they
/// are, including non-UTF-8.
fn absolute_bytes(path: &Path) -> io::Result<Vec<u8>> {
    Ok(std::path::absolute(path)?
        .into_os_string()
        .into_encoded_bytes())
}

fn key_of(path: &[u8]) -> String {
    let hash = path.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// `name`'s suffix if it's `key` (zero) or `<key>-<suffix>`.
fn suffix_of(name: &str, key: &str) -> Option<u64> {
    let rest = name.strip_prefix(key)?;
    if rest.is_empty() {
        return Some(0);
    }
    canonical(rest.strip_prefix('-')?).filter(|&suffix| suffix > 0)
}

fn is_slot_name(name: &str) -> bool {
    name.get(..KEY_LEN).is_some_and(|key| {
        key.bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            && suffix_of(name, key).is_some()
    })
}

fn temp_name(name: &str) -> String {
    format!(".{name}.tmp")
}

fn temp_target(name: &str) -> Option<&str> {
    name.strip_prefix('.')?.strip_suffix(".tmp")
}

fn is_temp_slot_name(name: &str) -> bool {
    temp_target(name).is_some_and(is_slot_name)
}

fn is_temp_entry_name(name: &str) -> bool {
    temp_target(name).is_some_and(|target| EntryId::parse(target).is_some())
}

/// Parses a decimal number written without a sign or leading zeros.
fn canonical(digits: &str) -> Option<u64> {
    digits
        .parse()
        .ok()
        .filter(|number: &u64| number.to_string() == digits)
}

fn unix_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis().try_into().unwrap_or(u64::MAX))
}

/// Writes `path` through `temp`, synced and then renamed over it. The caller
/// holds the lock, so a file already at `temp` is a crash's.
fn write_atomically(path: &Path, temp: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let result = (|| {
        let mut file = options.open(temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        builder.mode(0o700);
        builder.create(dir)?;
        // The mode is masked by the umask, and an existing directory keeps
        // its own.
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    builder.create(dir)
}

/// Makes the renames into `dir` durable.
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// A path that doesn't lead to a file or directory: removed, or under
/// something that isn't a directory.
fn is_gone(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn remove_dir_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_dir(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn remove_dir_all_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
