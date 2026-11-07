//! Persistent undo: a file's undo history kept on disk between opens, as
//! nvim's `undofile` keeps it.
//!
//! [`Buffer::export_undo`](crate::Buffer::export_undo) takes the history as
//! an [`UndoRecord`], a plain value: the hash ([`text_hash`]) and length of
//! the text the history ends at, and its entries, oldest first, each with
//! its batches of edits and whether it's undone.
//! [`Buffer::import_undo`](crate::Buffer::import_undo) gives it back to a
//! buffer holding that same text, and to no other.
//!
//! An [`UndoStore`] keeps one record per file:
//!
//! ```text
//! <dir>/<key>.undo
//! ```
//!
//! `<key>` is the FNV-1a 64 of the file's absolute path's bytes as 16 hex
//! digits, as [`LocalHistory`](crate::LocalHistory) keys its files. The record
//! holds the path too, so a file whose key another file's record holds reads
//! as having none, and its write replaces that record. A record appears by
//! renaming a finished temporary file over it, so it's always whole; writers
//! in other windows and processes each use their own temporary file, and the
//! last rename wins.
//!
//! `<dir>/.lock` is held exclusively, across processes, by each rename into
//! the store and by everything that removes from it ([`UndoStore::remove`],
//! a corrupt record's cleanup, [`UndoStore::prune`]), which decide what to
//! remove under it: none of them can remove a record renamed in after they
//! looked.
//!
//! The file starts with a magic and a format version: a record of a version
//! this build doesn't know is ignored and left alone, for the build that
//! wrote it. One that doesn't read (truncated, or its checksum doesn't
//! match) is ignored and removed.
//!
//! Records hold whatever the files held, so the directory is mode 0700 and
//! the files 0600. [`UndoStore::prune`] drops records not written for 30
//! days, then the oldest beyond 500.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read as _, Write},
    ops::Range,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

use fe_text::Text;

/// The most entries a record keeps; older ones drop first.
pub const MAX_ENTRIES: usize = 1000;
/// The most bytes of edits a record keeps: their old and new texts, each
/// edit's offsets and lengths counted as [`EDIT_OVERHEAD`] more. Older
/// entries drop first.
pub const MAX_BYTES: usize = 4 << 20;
/// What an edit is charged towards [`MAX_BYTES`] besides its texts: what its
/// offsets and lengths take in the file.
pub const EDIT_OVERHEAD: usize = 24;
/// Records not written for this long go on a [`UndoStore::prune`].
pub const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// The most records a [`UndoStore::prune`] keeps, the newest.
pub const MAX_FILES: usize = 500;

const MAGIC: &[u8; 8] = b"feundo\0\n";
const VERSION: u32 = 1;
const EXTENSION: &str = "undo";
const LOCK_NAME: &str = ".lock";
const KEY_LEN: usize = 16;
/// A file longer than this isn't a record [`Buffer::export_undo`](crate::Buffer::export_undo)
/// could have written.
const MAX_FILE_LEN: u64 = 4 * MAX_BYTES as u64 + (1 << 20);
/// A temporary file left this long is a crash's.
const TEMP_AGE: Duration = Duration::from_secs(60 * 60);

/// A file's undo history, as [`Buffer::export_undo`](crate::Buffer::export_undo)
/// takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoRecord {
    /// The [`text_hash`] of the text the history ends at.
    pub text_hash: u64,
    /// That text's length in bytes.
    pub text_len: u64,
    /// Oldest first. The undone ones come last, in the order redo
    /// reapplies them.
    pub entries: Vec<UndoEntry>,
}

/// One undo step: what `u` takes back, and `<C-r>` reapplies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoEntry {
    /// When it was made, in milliseconds since the Unix epoch.
    pub at_unix_ms: u64,
    /// Whether it's undone, so redo would reapply it.
    pub undone: bool,
    /// Its batches, in the order they were applied.
    pub batches: Vec<UndoBatch>,
}

/// Edits applied together, each `range` in the text as it was before the
/// batch: sorted, not overlapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoBatch {
    pub edits: Vec<UndoEdit>,
}

/// One replacement: `old_text`, at `range` before the batch, becomes
/// `new_text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoEdit {
    pub range: Range<usize>,
    pub old_text: String,
    pub new_text: String,
}

impl UndoEdit {
    /// What it's charged towards [`MAX_BYTES`].
    pub fn bytes(&self) -> usize {
        self.old_text.len() + self.new_text.len() + EDIT_OVERHEAD
    }
}

impl UndoEntry {
    /// What it's charged towards [`MAX_BYTES`].
    pub fn bytes(&self) -> usize {
        (self.batches.iter())
            .flat_map(|batch| &batch.edits)
            .map(UndoEdit::bytes)
            .sum()
    }
}

/// Why [`Buffer::import_undo`](crate::Buffer::import_undo) left a buffer as
/// it was.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UndoImportError {
    /// The buffer's text isn't the one the history ends at: the file changed
    /// since the record was written.
    #[error("the text isn't the one the undo history ends at")]
    Stale,
    /// The buffer already has undo history of its own.
    #[error("the buffer already has undo history")]
    Edited,
    /// The record's edits don't apply to its own text.
    #[error("the undo history doesn't apply to its text")]
    Invalid,
}

/// A 64-bit hash of `text`'s bytes, written here so it stays the same across
/// Rust versions and builds: FNV-1a's multiply over eight bytes at a time,
/// with a shift folding the high bits back down.
pub fn text_hash(text: &Text) -> u64 {
    hash_chunks(text.chunks().map(str::as_bytes))
}

fn hash_chunks<'a>(chunks: impl IntoIterator<Item = &'a [u8]>) -> u64 {
    fn step(hash: u64, value: u64) -> u64 {
        let hash = (hash ^ value).wrapping_mul(0x0000_0100_0000_01b3);
        hash ^ (hash >> 29)
    }
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut word = [0u8; 8];
    let mut filled = 0;
    for mut bytes in chunks {
        if filled > 0 {
            let take = (8 - filled).min(bytes.len());
            word[filled..filled + take].copy_from_slice(&bytes[..take]);
            filled += take;
            bytes = &bytes[take..];
            if filled < 8 {
                continue;
            }
            hash = step(hash, u64::from_le_bytes(word));
        }
        let (words, rest) = bytes.as_chunks::<8>();
        for chunk in words {
            hash = step(hash, u64::from_le_bytes(*chunk));
        }
        word[..rest.len()].copy_from_slice(rest);
        filled = rest.len();
    }
    for &byte in &word[..filled] {
        hash = step(hash, u64::from(byte));
    }
    hash
}

/// A directory of undo records, one per file; see the [module docs](self).
#[derive(Clone, Debug)]
pub struct UndoStore {
    dir: PathBuf,
}

impl UndoStore {
    /// A store in `dir`, created mode 0700 on the first write.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where `path`'s record is kept.
    pub fn file_of(&self, path: &Path) -> io::Result<PathBuf> {
        let key = key_of(&absolute_bytes(path)?);
        Ok(self.dir.join(format!("{key}.{EXTENSION}")))
    }

    /// `path`'s record, or `None` if it has none: none was written, it's of
    /// another file whose key is the same, or of a format version this build
    /// doesn't know. A record that doesn't read is removed, and is none:
    /// under the store's lock, and only if it's still the bytes that didn't
    /// read, so a record written meanwhile stays.
    pub fn read(&self, path: &Path) -> io::Result<Option<UndoRecord>> {
        let owner = absolute_bytes(path)?;
        let file = self.dir.join(format!("{}.{EXTENSION}", key_of(&owner)));
        let Some(bytes) = read_record_file(&file)? else {
            return Ok(None);
        };
        match decode(&bytes) {
            Decoded::Record(record_path, record) => Ok((record_path == owner).then_some(record)),
            Decoded::UnknownVersion => Ok(None),
            Decoded::Corrupt => {
                let _lock = self.lock()?;
                if read_record_file(&file)?.is_some_and(|now| now == bytes) {
                    remove_if_present(&file)?;
                }
                Ok(None)
            }
        }
    }

    /// Writes `record` as `path`'s, replacing any record its key had. The
    /// temporary file is written unlocked; the rename takes the store's
    /// lock, so no removal decided under it can take the new record.
    pub fn write(&self, path: &Path, record: &UndoRecord) -> io::Result<()> {
        let owner = absolute_bytes(path)?;
        let key = key_of(&owner);
        create_private_dir(&self.dir)?;
        let bytes = encode(&owner, record);
        let file = self.dir.join(format!("{key}.{EXTENSION}"));
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        let temp = self.dir.join(format!(
            ".{key}.{EXTENSION}.{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        write_temp(&temp, &bytes)?;
        let renamed = self.lock().and_then(|_lock| fs::rename(&temp, &file));
        if renamed.is_err() {
            let _ = fs::remove_file(&temp);
        }
        renamed?;
        sync_dir(&self.dir)
    }

    /// Removes `path`'s record, as when there's no history left to keep:
    /// one of another file whose key is the same, or of a format version
    /// this build doesn't know, stays. Under the store's lock.
    pub fn remove(&self, path: &Path) -> io::Result<()> {
        let owner = absolute_bytes(path)?;
        let file = self.dir.join(format!("{}.{EXTENSION}", key_of(&owner)));
        if !file.exists() {
            return Ok(());
        }
        let _lock = self.lock()?;
        let Some(bytes) = read_record_file(&file)? else {
            return Ok(());
        };
        match decode(&bytes) {
            Decoded::Record(record_path, _) if record_path != owner => Ok(()),
            Decoded::UnknownVersion => Ok(()),
            Decoded::Record(..) | Decoded::Corrupt => remove_if_present(&file),
        }
    }

    /// Holds the store's `.lock` file exclusively until dropped: renames
    /// into the store and removals from it take it, across processes.
    fn lock(&self) -> io::Result<File> {
        create_private_dir(&self.dir)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let lock = options.open(self.dir.join(LOCK_NAME))?;
        lock.lock()?;
        Ok(lock)
    }

    /// Removes the records last written over [`MAX_AGE`] before `now`, then
    /// the oldest beyond [`MAX_FILES`], and temporary files a crash left.
    /// Files the store didn't name are left alone. Returns how many records
    /// went.
    ///
    /// It holds the store's lock throughout, so a record written meanwhile
    /// is either seen as it is or written after.
    pub fn prune(&self, now: SystemTime) -> io::Result<usize> {
        if !self.dir.exists() {
            return Ok(0);
        }
        let _lock = self.lock()?;
        let items = match fs::read_dir(&self.dir) {
            Ok(items) => items,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        let mut records = Vec::new();
        let mut removed = 0;
        for item in items {
            let item = item?;
            let name = item.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let (record, temp) = (is_record_name(name), is_temp_name(name));
            if !record && !temp {
                continue;
            }
            let metadata = match item.metadata() {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if !metadata.is_file() {
                continue;
            }
            // A file from the future (the clock moved back) has no age yet.
            let age = metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok());
            if temp {
                if age.is_some_and(|age| age > TEMP_AGE) {
                    remove_if_present(&item.path())?;
                }
            } else if age.is_some_and(|age| age > MAX_AGE) {
                remove_if_present(&item.path())?;
                removed += 1;
            } else {
                records.push((metadata.modified().ok(), item.path()));
            }
        }
        if records.len() > MAX_FILES {
            records.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
            for (_, path) in &records[MAX_FILES..] {
                remove_if_present(path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

enum Decoded {
    Record(Vec<u8>, UndoRecord),
    UnknownVersion,
    Corrupt,
}

fn encode(path: &[u8], record: &UndoRecord) -> Vec<u8> {
    fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
        out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    put_bytes(&mut out, path);
    out.extend_from_slice(&record.text_hash.to_le_bytes());
    out.extend_from_slice(&record.text_len.to_le_bytes());
    out.extend_from_slice(&(record.entries.len() as u64).to_le_bytes());
    for entry in &record.entries {
        out.extend_from_slice(&entry.at_unix_ms.to_le_bytes());
        out.push(u8::from(entry.undone));
        out.extend_from_slice(&(entry.batches.len() as u64).to_le_bytes());
        for batch in &entry.batches {
            out.extend_from_slice(&(batch.edits.len() as u64).to_le_bytes());
            for edit in &batch.edits {
                out.extend_from_slice(&(edit.range.start as u64).to_le_bytes());
                put_bytes(&mut out, edit.old_text.as_bytes());
                put_bytes(&mut out, edit.new_text.as_bytes());
            }
        }
    }
    let checksum = hash_chunks([out.as_slice()]);
    out.extend_from_slice(&checksum.to_le_bytes());
    out
}

fn decode(bytes: &[u8]) -> Decoded {
    let Some(head) = bytes.get(..MAGIC.len() + 4) else {
        return Decoded::Corrupt;
    };
    if &head[..MAGIC.len()] != MAGIC {
        return Decoded::Corrupt;
    }
    let version = u32::from_le_bytes(head[MAGIC.len()..].try_into().expect("four bytes"));
    if version != VERSION {
        return Decoded::UnknownVersion;
    }
    let Some(split) = bytes.len().checked_sub(8) else {
        return Decoded::Corrupt;
    };
    let (body, checksum) = bytes.split_at(split);
    if hash_chunks([body]).to_le_bytes() != checksum {
        return Decoded::Corrupt;
    }
    let mut reader = Reader {
        bytes: &body[head.len()..],
    };
    match reader.record() {
        Some((path, record)) if reader.bytes.is_empty() => Decoded::Record(path, record),
        _ => Decoded::Corrupt,
    }
}

/// Reads what [`encode`] writes, refusing counts the bytes left can't hold.
struct Reader<'a> {
    bytes: &'a [u8],
}

impl Reader<'_> {
    fn record(&mut self) -> Option<(Vec<u8>, UndoRecord)> {
        let path = self.bytes()?.to_vec();
        let text_hash = self.u64()?;
        let text_len = self.u64()?;
        let mut entries = Vec::with_capacity(self.count(17)?);
        for _ in 0..entries.capacity() {
            let at_unix_ms = self.u64()?;
            let undone = match self.u8()? {
                0 => false,
                1 => true,
                _ => return None,
            };
            let mut batches = Vec::with_capacity(self.count(8)?);
            for _ in 0..batches.capacity() {
                let mut edits = Vec::with_capacity(self.count(24)?);
                for _ in 0..edits.capacity() {
                    let start = usize::try_from(self.u64()?).ok()?;
                    let old_text = String::from_utf8(self.bytes()?.to_vec()).ok()?;
                    let new_text = String::from_utf8(self.bytes()?.to_vec()).ok()?;
                    let end = start.checked_add(old_text.len())?;
                    edits.push(UndoEdit {
                        range: start..end,
                        old_text,
                        new_text,
                    });
                }
                batches.push(UndoBatch { edits });
            }
            entries.push(UndoEntry {
                at_unix_ms,
                undone,
                batches,
            });
        }
        Some((
            path,
            UndoRecord {
                text_hash,
                text_len,
                entries,
            },
        ))
    }

    fn take(&mut self, len: usize) -> Option<&[u8]> {
        if len > self.bytes.len() {
            return None;
        }
        let (taken, rest) = self.bytes.split_at(len);
        self.bytes = rest;
        Some(taken)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    /// A count of items each at least `min_len` bytes long.
    fn count(&mut self, min_len: usize) -> Option<usize> {
        let count = usize::try_from(self.u64()?).ok()?;
        (count.checked_mul(min_len)? <= self.bytes.len()).then_some(count)
    }

    fn bytes(&mut self) -> Option<&[u8]> {
        let len = usize::try_from(self.u64()?).ok()?;
        self.take(len)
    }
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

fn is_record_name(name: &str) -> bool {
    name.strip_suffix(EXTENSION)
        .and_then(|stem| stem.strip_suffix('.'))
        .is_some_and(|key| {
            key.len() == KEY_LEN
                && key
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        })
}

/// `.<key>.undo.<pid>-<n>.tmp`, as [`UndoStore::write`] names them.
fn is_temp_name(name: &str) -> bool {
    name.strip_prefix('.')
        .and_then(|name| name.strip_suffix(".tmp"))
        .and_then(|name| name.get(..KEY_LEN + 1 + EXTENSION.len()))
        .is_some_and(is_record_name)
}

/// Writes `temp`, mode 0600, and syncs it, for renaming into place.
fn write_temp(temp: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let result = (|| {
        let mut file = options.open(temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

/// A record file's bytes, up to one more than a record can hold; `None`
/// when there's none.
fn read_record_file(file: &Path) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    match File::open(file) {
        Ok(opened) => {
            opened.take(MAX_FILE_LEN + 1).read_to_end(&mut bytes)?;
            Ok(Some(bytes))
        }
        Err(error) if is_gone(&error) => Ok(None),
        Err(error) => Err(error),
    }
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

/// A path that doesn't lead to a file: removed, or under something that
/// isn't a directory.
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
