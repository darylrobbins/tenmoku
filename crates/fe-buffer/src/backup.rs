//! Backups of unsaved buffers, for hot exit and crash recovery.
//!
//! A [`BackupStore`] keeps each backup as two files in one private
//! directory: `<id>.txt`, the text as UTF-8 with `\n` line endings, and
//! `<id>.toml`, its [`BackupMeta`]. A write removes the old meta, if any,
//! then writes the text and then the meta, each to a temporary file renamed
//! into place, so the meta's rename commits the backup: a crash leaves the
//! old pair or the new pair, or a text file without its meta, which
//! [`BackupStore::ids`] skips and [`BackupStore::prune`] removes. The meta
//! also records a hash of its text, so a pair that isn't one write's reads
//! as invalid rather than as one text under another's meta.
//!
//! Writes hold a shared lock on the directory's `.lock` file, and
//! [`BackupStore::prune`] and [`BackupStore::remove`] an exclusive one, so
//! neither, from any process, removes the half-written pair or the
//! temporary file of a write in progress.
//!
//! Backups may hold anything the user typed, so the directory is mode 0700
//! and the files 0600. The meta is a small TOML document written and read
//! here, without a TOML dependency.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use fe_text::{Encoding, FileEncoding, LineEnding, Text};

use crate::{Buffer, DiskStamp};

/// Names a backup: `<unix-millis>-<pid>-<counter>`, unique among those the
/// process makes and, by the pid, among processes running at once. Orders
/// by time.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BackupId {
    millis: u64,
    pid: u32,
    counter: u64,
}

static NEXT_COUNTER: AtomicU64 = AtomicU64::new(0);

impl BackupId {
    /// A new id, never returned before by this process.
    #[expect(clippy::new_without_default, reason = "each call is a new id")]
    pub fn new() -> Self {
        Self {
            millis: unix_millis(SystemTime::now()),
            pid: std::process::id(),
            counter: NEXT_COUNTER.fetch_add(1, Ordering::Relaxed),
        }
    }
}

impl fmt::Display for BackupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}-{}", self.millis, self.pid, self.counter)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("not a backup id: {0:?}")]
pub struct ParseBackupIdError(String);

impl FromStr for BackupId {
    type Err = ParseBackupIdError;

    /// Parses exactly what [`BackupId`]'s `Display` writes, so an id and
    /// its file names correspond one to one.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || ParseBackupIdError(s.to_owned());
        let mut parts = s.split('-');
        let (Some(millis), Some(pid), Some(counter), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(error());
        };
        let id = Self {
            millis: millis.parse().map_err(|_| error())?,
            pid: pid.parse().map_err(|_| error())?,
            counter: counter.parse().map_err(|_| error())?,
        };
        // Rejects `+1`, `01` and the like.
        if id.to_string() != s {
            return Err(error());
        }
        Ok(id)
    }
}

/// What a backup's text belongs to, and what saving it needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupMeta {
    /// The file the buffer was editing; `None` for an untitled buffer.
    pub path: Option<PathBuf>,
    /// The tab's title, which an untitled buffer is restored with.
    pub title: String,
    /// The file as the buffer last read or saved it; `None` when it was
    /// missing, or for an untitled buffer.
    pub disk_stamp: Option<DiskStamp>,
    /// The encoding, and BOM, a save writes in.
    pub encoding: FileEncoding,
    pub line_ending: LineEnding,
    /// Whether reading the file decoded malformed bytes to U+FFFD, so that
    /// a save still refuses to write them back as U+FFFD.
    pub replaced: bool,
    /// When the backup was taken.
    pub written: SystemTime,
}

impl BackupMeta {
    /// The meta for backing up `buffer` now.
    pub fn for_buffer(buffer: &Buffer, path: Option<PathBuf>, title: impl Into<String>) -> Self {
        Self {
            path,
            title: title.into(),
            disk_stamp: buffer.disk_stamp(),
            encoding: buffer.encoding(),
            line_ending: buffer.line_ending(),
            replaced: buffer.replaced_on_read(),
            written: SystemTime::now(),
        }
    }
}

/// A backup read back by [`BackupStore::read`].
#[derive(Clone, Debug)]
pub struct Backup {
    pub id: BackupId,
    pub meta: BackupMeta,
    pub text: Text,
}

/// A directory of backups; see the [module docs](self).
#[derive(Clone, Debug)]
pub struct BackupStore {
    dir: PathBuf,
}

const TEXT_EXT: &str = "txt";
const META_EXT: &str = "toml";
const TEMP_EXT: &str = "tmp";
const LOCK_NAME: &str = ".lock";

/// Where [`BackupStore::write_with`] pauses, for tests of what runs
/// meanwhile.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum WriteStep {
    /// The text is in its temporary file.
    BeforeTextRename,
    /// The text is in place, its meta not yet.
    BetweenRenames,
}

/// `<id>.txt` or `<id>.toml`, as the id and extension.
fn parse_backup_name(name: &str) -> Option<(BackupId, &str)> {
    let (stem, ext) = name.rsplit_once('.')?;
    let id = stem.parse().ok()?;
    [TEXT_EXT, META_EXT].contains(&ext).then_some((id, ext))
}

/// Whether `name` is a temporary file [`BackupStore::write_atomically`]
/// makes: `.<id>.<ext>.<pid>-<counter>.tmp`.
fn is_temp_name(name: &str) -> bool {
    fn canonical<T: FromStr + ToString>(s: &str) -> bool {
        s.parse::<T>().is_ok_and(|n| n.to_string() == s)
    }
    let Some(inner) = name
        .strip_prefix('.')
        .and_then(|name| name.strip_suffix(&format!(".{TEMP_EXT}")))
    else {
        return false;
    };
    let Some((target, unique)) = inner.rsplit_once('.') else {
        return false;
    };
    let Some((pid, counter)) = unique.split_once('-') else {
        return false;
    };
    canonical::<u32>(pid) && canonical::<u64>(counter) && parse_backup_name(target).is_some()
}

impl BackupStore {
    /// A store in `dir`, which is created, mode 0700, on the first write.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes `text` as the backup `id`, replacing any backup by that id.
    /// One writer per id at a time; writes of different ids, from any
    /// process, may run at once.
    pub fn write(&self, id: BackupId, meta: &BackupMeta, text: &Text) -> io::Result<()> {
        self.write_with(id, meta, text, |_| {})
    }

    /// [`BackupStore::write`], calling `pause` at each [`WriteStep`].
    fn write_with(
        &self,
        id: BackupId,
        meta: &BackupMeta,
        text: &Text,
        mut pause: impl FnMut(WriteStep),
    ) -> io::Result<()> {
        self.create_dir()?;
        let lock = self.open_lock()?;
        lock.lock_shared()?;
        // Uncommits the backup this replaces before its text changes.
        if remove_if_present(&self.file(id, META_EXT))? {
            sync_dir(&self.dir)?;
        }
        let mut hash = Fnv1a64::default();
        self.write_atomically(
            &self.file(id, TEXT_EXT),
            |out| {
                for chunk in text.chunks() {
                    out.write_all(chunk.as_bytes())?;
                    hash.write(chunk.as_bytes());
                }
                Ok(())
            },
            || pause(WriteStep::BeforeTextRename),
        )?;
        pause(WriteStep::BetweenRenames);
        let meta = encode_meta(meta, hash.0)?;
        self.write_atomically(
            &self.file(id, META_EXT),
            |out| out.write_all(meta.as_bytes()),
            || {},
        )?;
        sync_dir(&self.dir)
    }

    /// Reads the backup `id`. Fails with [`io::ErrorKind::NotFound`] if it
    /// has no meta, and [`io::ErrorKind::InvalidData`] if the meta can't be
    /// read or wasn't written with this text.
    pub fn read(&self, id: BackupId) -> io::Result<Backup> {
        let meta = fs::read_to_string(self.file(id, META_EXT))?;
        let (meta, expected) = decode_meta(&meta)?;
        let mut text = String::new();
        File::open(self.file(id, TEXT_EXT))?.read_to_string(&mut text)?;
        let mut hash = Fnv1a64::default();
        hash.write(text.as_bytes());
        if hash.0 != expected {
            return Err(invalid(format!(
                "backup {id}: the text isn't the one its meta was written with"
            )));
        }
        // Written from a `Text`, so already `\n`; this only guards a file
        // edited by hand.
        let (text, _) = Text::from_file_contents(&text);
        Ok(Backup { id, meta, text })
    }

    /// Removes the backup `id`, meta first, so a crash part-way leaves only
    /// a text file for [`BackupStore::prune`]. A missing backup is no error.
    /// Like a prune, it waits for writes in progress to finish.
    pub fn remove(&self, id: BackupId) -> io::Result<()> {
        let Some(_lock) = self.lock_exclusive()? else {
            return Ok(());
        };
        self.remove_locked(id)
    }

    /// [`BackupStore::remove`], under a lock the caller holds.
    fn remove_locked(&self, id: BackupId) -> io::Result<()> {
        for ext in [META_EXT, TEXT_EXT] {
            remove_if_present(&self.file(id, ext))?;
        }
        Ok(())
    }

    /// The backups with both their text and meta, oldest first.
    pub fn ids(&self) -> io::Result<Vec<BackupId>> {
        let (complete, _) = self.scan()?;
        Ok(complete.into_iter().collect())
    }

    /// Removes every backup `keep` rejects, every half-written one (a text
    /// without its meta or a meta without its text), and the temporary
    /// files of interrupted writes. It waits for writes in progress, from
    /// any process, to finish, and writes wait for it. Files the store
    /// didn't name are left alone.
    pub fn prune(&self, keep: impl Fn(&BackupId) -> bool) -> io::Result<()> {
        let Some(_lock) = self.lock_exclusive()? else {
            return Ok(());
        };
        let (complete, strays) = self.scan()?;
        for id in complete {
            if !keep(&id) {
                self.remove_locked(id)?;
            }
        }
        for path in strays {
            remove_if_present(&path)?;
        }
        Ok(())
    }

    /// The complete pairs, and the files [`BackupStore::prune`] removes
    /// whatever `keep` says.
    fn scan(&self) -> io::Result<(BTreeSet<BackupId>, Vec<PathBuf>)> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Default::default());
            }
            Err(error) => return Err(error),
        };
        let mut found: BTreeMap<BackupId, (Option<PathBuf>, Option<PathBuf>)> = BTreeMap::new();
        let mut strays = Vec::new();
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if is_temp_name(name) {
                strays.push(entry.path());
                continue;
            }
            let Some((id, ext)) = parse_backup_name(name) else {
                continue;
            };
            let slot = found.entry(id).or_default();
            if ext == TEXT_EXT {
                slot.0 = Some(entry.path());
            } else {
                slot.1 = Some(entry.path());
            }
        }
        let mut complete = BTreeSet::new();
        for (id, pair) in found {
            match pair {
                (Some(_), Some(_)) => {
                    complete.insert(id);
                }
                (text, meta) => strays.extend(text.into_iter().chain(meta)),
            }
        }
        Ok((complete, strays))
    }

    /// The lock held exclusively, once writes in progress finish; `None`
    /// when the directory doesn't exist, so there's nothing to change.
    fn lock_exclusive(&self) -> io::Result<Option<File>> {
        let lock = match self.open_lock() {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            lock => lock?,
        };
        lock.lock()?;
        Ok(Some(lock))
    }

    /// The file writes hold shared, and [`BackupStore::prune`] and
    /// [`BackupStore::remove`] exclusively.
    fn open_lock(&self) -> io::Result<File> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(self.dir.join(LOCK_NAME))
    }

    fn file(&self, id: BackupId, ext: &str) -> PathBuf {
        self.dir.join(format!("{id}.{ext}"))
    }

    fn create_dir(&self) -> io::Result<()> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
            builder.mode(0o700);
            builder.create(&self.dir)?;
            // The mode is masked by the umask, and an existing directory
            // keeps its own.
            fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))
        }
        #[cfg(not(unix))]
        builder.create(&self.dir)
    }

    /// Writes `path` through a temporary file, created exclusively with
    /// owner-only permissions, synced and then renamed over it.
    fn write_atomically(
        &self,
        path: &Path,
        write: impl FnOnce(&mut io::BufWriter<File>) -> io::Result<()>,
        before_rename: impl FnOnce(),
    ) -> io::Result<()> {
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let temp = self.dir.join(format!(
            ".{name}.{}-{}.{TEMP_EXT}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let file = match options.open(&temp) {
            // Left by a crashed process that had our pid.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                fs::remove_file(&temp)?;
                options.open(&temp)
            }
            result => result,
        }?;
        let result = (|| {
            let mut out = io::BufWriter::new(file);
            write(&mut out)?;
            let file = out.into_inner().map_err(|error| error.into_error())?;
            file.sync_all()?;
            before_rename();
            fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

/// Removes `path`, returning whether it was there.
fn remove_if_present(path: &Path) -> io::Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// FNV-1a, 64 bits: binds a meta to the text it was written with. Not
/// for adversaries, only crashes and stray edits.
struct Fnv1a64(u64);

impl Default for Fnv1a64 {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv1a64 {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

/// Makes the renames into `dir` durable.
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn unix_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().try_into().unwrap_or(u64::MAX))
}

/// A time as whole seconds from the epoch, negative before it, and the
/// nanoseconds after those seconds.
fn split_time(time: SystemTime) -> (i64, u32) {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => (after.as_secs() as i64, after.subsec_nanos()),
        Err(before) => {
            let before = before.duration();
            match before.subsec_nanos() {
                0 => (-(before.as_secs() as i64), 0),
                nanos => (-(before.as_secs() as i64) - 1, 1_000_000_000 - nanos),
            }
        }
    }
}

fn join_time(secs: i64, nanos: u32) -> Option<SystemTime> {
    let base = if secs >= 0 {
        UNIX_EPOCH.checked_add(Duration::from_secs(secs as u64))?
    } else {
        UNIX_EPOCH.checked_sub(Duration::from_secs(secs.unsigned_abs()))?
    };
    base.checked_add(Duration::from_nanos(nanos.into()))
}

const META_VERSION: u64 = 1;

fn encode_meta(meta: &BackupMeta, text_hash: u64) -> io::Result<String> {
    use fmt::Write as _;
    let mut out = String::from("# A Tenmoku backup of an unsaved buffer.\n");
    let mut line = |key: &str, value: String| {
        let _ = writeln!(out, "{key} = {value}");
    };
    line("version", META_VERSION.to_string());
    if let Some(path) = &meta.path {
        match path.to_str() {
            Some(path) => line("path", quote(path)),
            None => line("path_bytes", quote(&hex(&path_bytes(path)?))),
        }
    }
    line("title", quote(&meta.title));
    line("encoding", quote(meta.encoding.name()));
    line("bom", meta.encoding.bom.to_string());
    let line_ending = match meta.line_ending {
        LineEnding::Lf => "lf",
        LineEnding::CrLf => "crlf",
    };
    line("line_ending", quote(line_ending));
    line("replaced", meta.replaced.to_string());
    let (secs, nanos) = split_time(meta.written);
    line("written_secs", secs.to_string());
    line("written_nanos", nanos.to_string());
    line("text_fnv1a64", quote(&format!("{text_hash:016x}")));
    if let Some(stamp) = &meta.disk_stamp {
        out.push_str("\n[disk_stamp]\n");
        let mut line = |key: &str, value: String| {
            let _ = writeln!(out, "{key} = {value}");
        };
        line("len", stamp.len.to_string());
        let (secs, nanos) = split_time(stamp.modified);
        line("modified_secs", secs.to_string());
        line("modified_nanos", nanos.to_string());
        // Written as unsigned: may exceed TOML's i64, which only this
        // reader has to accept.
        #[cfg(unix)]
        {
            line("dev", stamp.dev.to_string());
            line("ino", stamp.ino.to_string());
        }
    }
    Ok(out)
}

fn decode_meta(source: &str) -> io::Result<(BackupMeta, u64)> {
    let fields = parse_toml(source)?;
    let get = |key: &str| fields.get(key);
    let string = |key: &str| match get(key) {
        Some(Value::String(s)) => Ok(s.as_str()),
        _ => Err(invalid(format!("backup meta: no string {key}"))),
    };
    let int = |key: &str| match get(key) {
        Some(Value::Int(n)) => Ok(*n),
        _ => Err(invalid(format!("backup meta: no integer {key}"))),
    };
    let uint = |key: &str| {
        int(key).and_then(|n| {
            u64::try_from(n).map_err(|_| invalid(format!("backup meta: {key} is negative")))
        })
    };
    let boolean = |key: &str| match get(key) {
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(invalid(format!("backup meta: no boolean {key}"))),
    };
    let time = |prefix: &str| {
        let nanos = uint(&format!("{prefix}_nanos"))?;
        let secs = int(&format!("{prefix}_secs"))?;
        let secs = i64::try_from(secs).map_err(|_| invalid("backup meta: time out of range"))?;
        u32::try_from(nanos)
            .ok()
            .filter(|&n| n < 1_000_000_000)
            .and_then(|nanos| join_time(secs, nanos))
            .ok_or_else(|| invalid("backup meta: time out of range"))
    };

    let version = uint("version")?;
    if version != META_VERSION {
        return Err(invalid(format!("backup meta: unknown version {version}")));
    }
    let path = if get("path").is_some() {
        Some(PathBuf::from(string("path")?))
    } else if get("path_bytes").is_some() {
        let bytes =
            unhex(string("path_bytes")?).ok_or_else(|| invalid("backup meta: bad path_bytes"))?;
        Some(path_from_bytes(bytes)?)
    } else {
        None
    };
    let encoding = string("encoding")?;
    let encoding = Encoding::for_label_no_replacement(encoding.as_bytes())
        .ok_or_else(|| invalid(format!("backup meta: unknown encoding {encoding:?}")))?;
    let line_ending = match string("line_ending")? {
        "lf" => LineEnding::Lf,
        "crlf" => LineEnding::CrLf,
        other => return Err(invalid(format!("backup meta: bad line_ending {other:?}"))),
    };
    let disk_stamp = if get("disk_stamp.len").is_some() {
        Some(DiskStamp {
            len: uint("disk_stamp.len")?,
            modified: time("disk_stamp.modified")?,
            #[cfg(unix)]
            dev: uint("disk_stamp.dev")?,
            #[cfg(unix)]
            ino: uint("disk_stamp.ino")?,
        })
    } else {
        None
    };
    let meta = BackupMeta {
        path,
        title: string("title")?.to_owned(),
        disk_stamp,
        encoding: FileEncoding {
            encoding,
            bom: boolean("bom")?,
        },
        line_ending,
        replaced: boolean("replaced")?,
        written: time("written")?,
    };
    let text_hash = string("text_fnv1a64")?;
    let text_hash = u64::from_str_radix(text_hash, 16)
        .ok()
        .filter(|_| text_hash.len() == 16)
        .ok_or_else(|| invalid("backup meta: bad text_fnv1a64"))?;
    Ok((meta, text_hash))
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> io::Result<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    Ok(path.as_os_str().as_bytes().to_vec())
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> io::Result<Vec<u8>> {
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("can't back up a path that isn't Unicode: {path:?}"),
    ))
}

#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Ok(std::ffi::OsString::from_vec(bytes).into())
}

#[cfg(not(unix))]
fn path_from_bytes(_: Vec<u8>) -> io::Result<PathBuf> {
    Err(invalid(
        "backup meta: path_bytes on a platform without byte paths",
    ))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// A TOML basic string.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[derive(Debug)]
enum Value {
    String(String),
    Int(i128),
    Bool(bool),
}

/// The subset of TOML [`encode_meta`] writes: comments, `[table]` headers,
/// and `key = value` lines of basic strings, integers and booleans. Keys
/// under a table come back as `table.key`.
fn parse_toml(source: &str) -> io::Result<HashMap<String, Value>> {
    let mut fields = HashMap::new();
    let mut table = String::new();
    for (row, line) in source.lines().enumerate() {
        let error = || invalid(format!("backup meta: can't read line {}", row + 1));
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[') {
            let name = name.strip_suffix(']').ok_or_else(error)?.trim();
            table = format!("{name}.");
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(error)?;
        let key = key.trim();
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(error());
        }
        let value = value.trim();
        let value = if value.starts_with('"') {
            Value::String(unquote(value).ok_or_else(error)?)
        } else if value == "true" || value == "false" {
            Value::Bool(value == "true")
        } else {
            Value::Int(value.parse().map_err(|_| error())?)
        };
        if fields.insert(format!("{table}{key}"), value).is_some() {
            return Err(error());
        }
    }
    Ok(fields)
}

/// Reads a TOML basic string that is all of `s`, with an optional comment
/// after it.
fn unquote(s: &str) -> Option<String> {
    let mut chars = s.strip_prefix('"')?.chars();
    let mut out = String::new();
    loop {
        match chars.next()? {
            '"' => break,
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'e' => out.push('\u{1b}'),
                escape @ ('u' | 'U') => {
                    let len = if escape == 'u' { 4 } else { 8 };
                    let digits: String = chars.by_ref().take(len).collect();
                    if digits.len() != len {
                        return None;
                    }
                    out.push(char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?);
                }
                _ => return None,
            },
            ch => out.push(ch),
        }
    }
    let rest = chars.as_str().trim_start();
    (rest.is_empty() || rest.starts_with('#')).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_reject_non_canonical_forms() {
        let id = BackupId::new();
        assert_eq!(id.to_string().parse::<BackupId>(), Ok(id));
        assert_ne!(BackupId::new(), id);
        for bad in [
            "",
            "1-2",
            "1-2-3-4",
            "01-2-3",
            "+1-2-3",
            "1-2-x",
            "1--3",
            "1-99999999999-3",
        ] {
            assert!(bad.parse::<BackupId>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn strings_quote_and_unquote() {
        for s in [
            "",
            "plain",
            "a \"q\" \\ b",
            "tab\tnl\ncr\r",
            "\u{1}\u{7f}",
            "日本 😀",
        ] {
            assert_eq!(unquote(&quote(s)).as_deref(), Some(s));
        }
        assert_eq!(unquote(r#""é\U0001F600" # note"#).as_deref(), Some("é😀"));
        assert_eq!(unquote(r#""open"#), None);
        assert_eq!(unquote(r#""x" y"#), None);
        assert_eq!(unquote(r#""\q""#), None);
    }

    #[test]
    fn temp_names_follow_their_grammar() {
        let id = BackupId::new();
        for name in [
            format!(".{id}.txt.1-2.tmp"),
            format!(".{id}.toml.4294967295-0.tmp"),
        ] {
            assert!(is_temp_name(&name), "{name}");
        }
        for name in [
            ".notes.tmp".to_owned(),
            ".tmp".to_owned(),
            format!("{id}.txt.1-2.tmp"),
            format!(".{id}.md.1-2.tmp"),
            format!(".{id}.txt.1-2.temp"),
            format!(".{id}.txt.1.tmp"),
            format!(".{id}.txt.01-2.tmp"),
            format!(".{id}.txt.1-x.tmp"),
            format!(".{id}.txt.4294967296-2.tmp"),
            ".1-2.txt.1-2.tmp".to_owned(),
            ".lock".to_owned(),
        ] {
            assert!(!is_temp_name(&name), "{name}");
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// Copies the files in `from` to `to`: the directory as a crash at
    /// that moment would leave it.
    fn crash_image(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for name in names(from) {
            fs::copy(from.join(&name), to.join(&name)).unwrap();
        }
    }

    #[test]
    fn an_interrupted_rewrite_leaves_no_backup_by_that_id() {
        let root = tempfile::tempdir().unwrap();
        let store = BackupStore::new(root.path().join("live"));
        let meta = |title: &str| BackupMeta {
            path: None,
            title: title.into(),
            disk_stamp: None,
            encoding: FileEncoding::UTF8,
            line_ending: LineEnding::Lf,
            replaced: false,
            written: SystemTime::now(),
        };
        let id = BackupId::new();
        store
            .write(id, &meta("old"), &Text::new("some text"))
            .unwrap();
        // Equal and different lengths.
        for (n, new) in ["else text", "different text"].into_iter().enumerate() {
            for step in [WriteStep::BeforeTextRename, WriteStep::BetweenRenames] {
                let image = root.path().join(format!("crash-{n}-{step:?}"));
                store
                    .write_with(id, &meta("new"), &Text::new(new), |at| {
                        if at == step {
                            crash_image(store.dir(), &image);
                        }
                    })
                    .unwrap();
                let crashed = BackupStore::new(&image);
                assert_eq!(crashed.ids().unwrap(), [], "{new} {step:?}");
                let error = crashed.read(id).unwrap_err();
                assert_eq!(error.kind(), io::ErrorKind::NotFound, "{new} {step:?}");
                crashed.prune(|_| true).unwrap();
                assert_eq!(names(&image), [LOCK_NAME], "{new} {step:?}");

                let backup = store.read(id).unwrap();
                assert_eq!(backup.text.to_string(), new);
                assert_eq!(backup.meta.title, "new");
                store
                    .write(id, &meta("old"), &Text::new("some text"))
                    .unwrap();
            }
        }
    }

    #[test]
    fn prune_waits_for_writes_in_progress() {
        let root = tempfile::tempdir().unwrap();
        let store = BackupStore::new(root.path());
        let meta = BackupMeta {
            path: None,
            title: "t".into(),
            disk_stamp: None,
            encoding: FileEncoding::UTF8,
            line_ending: LineEnding::Lf,
            replaced: false,
            written: SystemTime::now(),
        };
        for step in [WriteStep::BeforeTextRename, WriteStep::BetweenRenames] {
            let id = BackupId::new();
            let mut pruning = None;
            store
                .write_with(id, &meta, &Text::new("text"), |at| {
                    if at != step {
                        return;
                    }
                    // Another store on the directory, as another process
                    // would have.
                    let other = BackupStore::new(root.path());
                    let prune = std::thread::spawn(move || other.prune(|_| true));
                    std::thread::sleep(Duration::from_millis(100));
                    assert!(!prune.is_finished(), "{step:?}: prune didn't wait");
                    pruning = Some(prune);
                })
                .unwrap();
            pruning.unwrap().join().unwrap().unwrap();
            assert_eq!(store.read(id).unwrap().text.to_string(), "text");
            assert!(store.ids().unwrap().contains(&id), "{step:?}");
            assert!(names(root.path()).iter().all(|name| !is_temp_name(name)));
        }
    }

    #[test]
    fn remove_waits_for_writes_in_progress() {
        let root = tempfile::tempdir().unwrap();
        let store = BackupStore::new(root.path());
        let meta = BackupMeta {
            path: None,
            title: "t".into(),
            disk_stamp: None,
            encoding: FileEncoding::UTF8,
            line_ending: LineEnding::Lf,
            replaced: false,
            written: SystemTime::now(),
        };
        for step in [WriteStep::BeforeTextRename, WriteStep::BetweenRenames] {
            let id = BackupId::new();
            let mut removing = None;
            store
                .write_with(id, &meta, &Text::new("text"), |at| {
                    if at != step {
                        return;
                    }
                    let other = BackupStore::new(root.path());
                    let remove = std::thread::spawn(move || other.remove(id));
                    std::thread::sleep(Duration::from_millis(100));
                    assert!(!remove.is_finished(), "{step:?}: remove didn't wait");
                    removing = Some(remove);
                })
                .unwrap();
            removing.unwrap().join().unwrap().unwrap();
            // Removed whole, after the write: no half of it left.
            assert_eq!(store.ids().unwrap(), [], "{step:?}");
            assert_eq!(names(root.path()), [LOCK_NAME], "{step:?}");
        }
        // Nothing to remove from a directory never made.
        BackupStore::new(root.path().join("unmade"))
            .remove(BackupId::new())
            .unwrap();
    }

    #[test]
    fn times_split_and_join() {
        for time in [
            UNIX_EPOCH,
            UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_789),
            UNIX_EPOCH - Duration::new(5, 0),
            UNIX_EPOCH - Duration::new(5, 250),
        ] {
            let (secs, nanos) = split_time(time);
            assert!(nanos < 1_000_000_000);
            assert_eq!(join_time(secs, nanos), Some(time));
        }
    }
}
