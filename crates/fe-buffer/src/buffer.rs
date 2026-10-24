use std::{
    collections::{HashMap, VecDeque},
    fs::{File, Metadata},
    io,
    ops::Range,
    path::Path,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};

use fe_text::{Bias, DecodedFile, EncodeError, FileEncoding, LineEnding, Text};

use crate::{
    Backup, BufferId, BufferSnapshot, Edit, Version,
    edit::{Mapping, normalize},
    log::{Collapse, EditLog},
};

/// Who made an edit. Undo can be scoped by origin, so the AI's edits can be
/// reverted without touching the user's.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    User,
    Ai { agent: Arc<str>, session: Arc<str> },
    Lsp,
    Format,
    Host(Arc<str>),
}

impl Origin {
    pub fn is_ai(&self) -> bool {
        matches!(self, Origin::Ai { .. })
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TransactionId(u64);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EditKind {
    Edit,
    Undo,
    Redo,
}

#[derive(Clone, Debug)]
pub enum BufferEvent {
    /// One batch was applied, taking the buffer from `from` to `to`.
    Edited {
        transaction: TransactionId,
        origin: Origin,
        kind: EditKind,
        from: Version,
        to: Version,
        edits: Arc<[Edit]>,
        /// The text right after this batch.
        text: Text,
    },
    Saved {
        version: Version,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error(
        "the snapshot belongs to a different buffer, a newer version, or one below the edit log's floor"
    )]
    ForeignSnapshot,
    /// The text an edit was meant to replace has changed since the snapshot.
    #[error("edit {index} conflicts with changes made since the snapshot")]
    Conflict { index: usize },
    /// [`Buffer::edit_in`] named a transaction that can't take more edits.
    #[error("the transaction was undone, or is no longer in the history")]
    NoSuchTransaction(TransactionId),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UndoError {
    /// Text the transaction wrote has been changed by a later edit, so undoing
    /// (or redoing) it would clobber that edit.
    #[error("transaction {0:?} was changed by later edits")]
    Conflict(TransactionId),
}

/// Receives the buffer's events. Drop it to unsubscribe.
pub struct Subscription(Arc<Mutex<Vec<BufferEvent>>>);

impl Subscription {
    /// Takes every event received since the last call.
    pub fn drain(&self) -> Vec<BufferEvent> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// What a file on disk looked like when it was read or saved: compared
/// before a save to tell whether something else wrote it since.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DiskStamp {
    pub len: u64,
    pub modified: SystemTime,
    #[cfg(unix)]
    pub dev: u64,
    #[cfg(unix)]
    pub ino: u64,
}

impl DiskStamp {
    pub fn from_metadata(metadata: &Metadata) -> io::Result<Self> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified()?,
            #[cfg(unix)]
            dev: metadata.dev(),
            #[cfg(unix)]
            ino: metadata.ino(),
        })
    }

    /// The stamp of the file at `path`, following symlinks; `None` when
    /// there's no file there.
    pub fn of(path: impl AsRef<Path>) -> io::Result<Option<Self>> {
        match std::fs::metadata(path) {
            Ok(metadata) => Self::from_metadata(&metadata).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// What a save writes: the text as of one version with how to encode it.
/// `Send`, so the write can run off the thread that owns the buffer; take
/// it with [`Buffer::save_snapshot`], and once it's on disk report it with
/// [`Buffer::did_save_snapshot`].
#[derive(Clone)]
pub struct SaveSnapshot {
    buffer: BufferId,
    version: Version,
    text: Text,
    line_ending: LineEnding,
    encoding: FileEncoding,
    replaced_on_read: bool,
}

impl SaveSnapshot {
    pub fn version(&self) -> Version {
        self.version
    }

    pub fn text(&self) -> &Text {
        &self.text
    }

    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    pub fn encoding(&self) -> FileEncoding {
        self.encoding
    }

    /// Writes the text with the snapshot's line endings in its encoding.
    ///
    /// Refuses with [`std::io::ErrorKind::InvalidData`] rather than lose data:
    /// before writing anything if the file had bytes that were invalid in its
    /// encoding and some of the U+FFFD they became are still in the text, and
    /// part-way through on a char the encoding can't represent. Output may
    /// have been written by then, so write to a temporary file and keep it
    /// only on success, as `EditorModel::save` does.
    pub fn write_to(&self, out: impl std::io::Write) -> std::io::Result<()> {
        let text = &self.text;
        if self.replaced_on_read
            && text
                .chunks()
                .any(|chunk| chunk.contains(char::REPLACEMENT_CHARACTER))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "the file had bytes that aren't valid {}, shown as U+FFFD; \
                     saving would replace them",
                    self.encoding.label()
                ),
            ));
        }
        text.write_encoded(self.line_ending, self.encoding, out)
            .map_err(|error| match error {
                EncodeError::Unmappable { ch, offset } => {
                    let point = text.offset_to_point(offset);
                    let row_start = text.row_start(point.row);
                    let column = text.offset_to_char(offset) - text.offset_to_char(row_start) + 1;
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "{ch:?} (U+{:04X}) on line {}, column {column} can't be saved in {}",
                            u32::from(ch),
                            point.row + 1,
                            self.encoding.label()
                        ),
                    )
                }
                EncodeError::Io(error) => error,
            })
    }
}

/// An editable text buffer.
///
/// Every change is a batch of edits applied atomically and recorded with its
/// [`Origin`] in a transaction. Transactions can be undone and redone
/// selectively by origin, even when later edits exist, as long as those edits
/// didn't change the transaction's text.
pub struct Buffer {
    snapshot: BufferSnapshot,
    line_ending: LineEnding,
    /// The encoding the file was read in, and is written back in.
    encoding: FileEncoding,
    /// Whether reading decoded malformed bytes to U+FFFD; see
    /// [`Buffer::write_to`].
    replaced_on_read: bool,
    history: VecDeque<HistoryEntry>,
    open: Option<OpenTransaction>,
    /// The caller's transaction while [`Buffer::with_transaction_aside`] has
    /// it set aside; still protected from history trimming.
    aside: Option<TransactionId>,
    next_transaction: u64,
    undo_clock: u64,
    /// How many undone (redoable) transactions each origin has, so edits only
    /// scan the history to discard redos when there are some.
    redoable: HashMap<Origin, usize>,
    /// Text bytes each origin's history entries hold; see [`entry_bytes`].
    history_bytes: HashMap<Origin, usize>,
    history_limit: usize,
    saved_version: Version,
    /// The text as of the last save (or load). Shares rope storage with the
    /// current text, so it holds only the leaves edited since.
    saved_text: Text,
    saved_line_ending: LineEnding,
    /// The encoding as of the last save (or load); see
    /// [`Buffer::is_dirty`].
    saved_encoding: FileEncoding,
    /// Invariant: outside this range (current coordinates) the text and
    /// `saved_text` are byte-identical; `None` means they're equal.
    changed_since_save: Option<Range<usize>>,
    /// Whether the text differs from `saved_text`; see [`Buffer::is_dirty`].
    dirty: bool,
    /// The file as it was when last read or saved: `Some(None)` when it
    /// was missing, `None` when the buffer doesn't know (it wasn't read
    /// from a file it was told about).
    disk_stamp: Option<Option<DiskStamp>>,
    /// Set by [`Buffer::from_backup`] until the next save.
    restore: Option<Restore>,
    /// Set by [`Buffer::mark_unsaved`] until the next save.
    unsaved: bool,
    pub(crate) subscribers: Vec<Weak<Mutex<Vec<BufferEvent>>>>,
}

/// A restore from a backup; see [`Buffer::from_backup`].
struct Restore {
    transaction: TransactionId,
    /// Whether the transaction is applied (not undone). The buffer is dirty
    /// while it is, whatever its text: the backup was of unsaved changes.
    applied: bool,
    backup: SaveFormat,
    /// The file's, as it was read; undoing the restore returns to it.
    disk: SaveFormat,
}

/// How a save writes the text.
#[derive(Copy, Clone)]
struct SaveFormat {
    line_ending: LineEnding,
    encoding: FileEncoding,
    replaced_on_read: bool,
}

struct OpenTransaction {
    id: TransactionId,
    origin: Origin,
    depth: usize,
}

struct HistoryEntry {
    id: TransactionId,
    origin: Origin,
    batches: Vec<AppliedBatch>,
    undone: Option<Undone>,
}

struct Undone {
    batches: Vec<AppliedBatch>,
    at: u64,
}

#[derive(Clone)]
struct AppliedBatch {
    /// The version this batch produced.
    version: Version,
    edits: Arc<[Edit]>,
    old_texts: Vec<Arc<str>>,
    new_texts: Vec<Arc<str>>,
    /// For undo and redo batches: the log entries whose replacements
    /// collapsed positions this batch was computed from. If one of them is
    /// reverted, this batch's positions can no longer be trusted.
    collapsed_by: Arc<[Collapse]>,
}

static NEXT_BUFFER_ID: AtomicU64 = AtomicU64::new(1);

impl Buffer {
    /// Default for [`Buffer::set_history_limit`]: 16 MiB per origin, counting
    /// each batch's bookkeeping as well as its old and new text.
    ///
    /// A one-character transaction is charged a few hundred bytes, so this
    /// keeps tens of thousands of them undoable (at least 10,000), and bounds
    /// a long typing session's memory. A replace-all of 507k six-byte words
    /// with eight-byte ones (`examples/history_memory.rs`) is charged about
    /// 130 bytes per edit, so only the newest pass or two stay undoable.
    pub const DEFAULT_HISTORY_LIMIT: usize = 16 << 20;

    /// A buffer holding `text`, which must use `\n` line endings.
    pub fn new(text: &str) -> Self {
        Self::with_text(Text::new(text), LineEnding::Lf)
    }

    /// A buffer for text read from disk; line endings are normalised and
    /// restored by [`Buffer::write_to`].
    pub fn from_file_contents(contents: &str) -> Self {
        let (text, line_ending) = Text::from_file_contents(contents);
        Self::with_text(text, line_ending)
    }

    /// Streams a buffer from `reader` (e.g. a file) without an intermediate
    /// copy. Line endings are normalised; invalid UTF-8 becomes U+FFFD.
    pub fn from_reader(reader: impl std::io::Read) -> std::io::Result<Self> {
        let (text, line_ending) = Text::from_reader(reader)?;
        Ok(Self::with_text(text, line_ending))
    }

    /// Like [`Buffer::from_reader`], but detects the file's encoding (see
    /// [`Text::from_reader_detecting`]) and saves back in it.
    pub fn from_reader_detecting(reader: impl std::io::Read) -> std::io::Result<Self> {
        let decoded = Text::from_reader_detecting(reader)?;
        let mut buffer = Self::with_text(decoded.text, decoded.line_ending);
        buffer.encoding = decoded.encoding;
        buffer.saved_encoding = decoded.encoding;
        buffer.replaced_on_read = decoded.replaced;
        Ok(buffer)
    }

    /// Reads `file` as [`Buffer::from_reader_detecting`] does, recording
    /// its [`DiskStamp`] from the handle before reading.
    pub fn from_file(file: File) -> io::Result<Self> {
        let (decoded, stamp) = Self::read_file(file)?;
        let mut buffer = Self::with_text(decoded.text, decoded.line_ending);
        buffer.encoding = decoded.encoding;
        buffer.saved_encoding = decoded.encoding;
        buffer.replaced_on_read = decoded.replaced;
        buffer.disk_stamp = Some(Some(stamp));
        Ok(buffer)
    }

    /// Restores `backup` (see [`BackupStore`](crate::BackupStore)) over
    /// `disk`, the file it was of as it is now, or `None` when the buffer
    /// was untitled or the file is gone.
    ///
    /// The file's text, or no text, is the saved base, and the backup's
    /// text is applied over it as one transaction of [`Origin::User`], so
    /// the user's first undo takes it back, empty when the texts are equal.
    /// The buffer saves in the backup's encoding, BOM and line ending, and
    /// is dirty while the transaction is applied, until a save, even when
    /// the backup saves just what the file holds. Undoing it returns to the
    /// file's text and save format, clean; redoing it restores the
    /// backup's. A backup
    /// of a file keeps the [`DiskStamp`] the buffer had when it was taken,
    /// so if the file changed on disk since, a save is refused as any stale
    /// save is.
    pub fn from_backup(disk: Option<File>, backup: &Backup) -> io::Result<Self> {
        let mut buffer = match disk {
            Some(file) => Self::from_file(file)?,
            None => Self::new(""),
        };
        let origin = Origin::User;
        let (old, new) = differing_ranges(&buffer.snapshot.text, &backup.text);
        let transaction = if old.is_empty() && new.is_empty() {
            buffer.record_empty(origin)
        } else {
            let new_text = backup.text.slice_to_string(new);
            buffer
                .edit([(old, new_text)], origin)
                .expect("the texts differ")
        };
        let meta = &backup.meta;
        let restore = Restore {
            transaction,
            applied: false,
            backup: SaveFormat {
                line_ending: meta.line_ending,
                encoding: meta.encoding,
                replaced_on_read: meta.replaced,
            },
            disk: SaveFormat {
                line_ending: buffer.line_ending,
                encoding: buffer.encoding,
                replaced_on_read: buffer.replaced_on_read,
            },
        };
        buffer.restore = Some(restore);
        buffer.note_restore(transaction, true);
        buffer.disk_stamp = meta.path.is_some().then_some(meta.disk_stamp);
        Ok(buffer)
    }

    /// Records an empty transaction of `origin`, closed, for a restore
    /// that changes no text.
    fn record_empty(&mut self, origin: Origin) -> TransactionId {
        debug_assert!(self.open.is_none() && self.history.is_empty());
        let id = self.next_transaction_id();
        *self.history_bytes.entry(origin.clone()).or_default() += ENTRY_OVERHEAD;
        self.history.push_back(HistoryEntry {
            id,
            origin,
            batches: Vec::new(),
            undone: None,
        });
        id
    }

    /// Follows `transaction` being applied or undone: if it's the restore,
    /// switches to the backup's save format or back to the file's.
    fn note_restore(&mut self, transaction: TransactionId, applied: bool) {
        let Some(restore) = &mut self.restore else {
            return;
        };
        if restore.transaction != transaction {
            return;
        }
        restore.applied = applied;
        let format = if applied {
            restore.backup
        } else {
            restore.disk
        };
        self.line_ending = format.line_ending;
        self.encoding = format.encoding;
        self.replaced_on_read = format.replaced_on_read;
    }

    /// Whether reading the file decoded malformed bytes to U+FFFD.
    pub(crate) fn replaced_on_read(&self) -> bool {
        self.replaced_on_read
    }

    /// Reads and decodes `file` as [`Buffer::from_file`] does, for
    /// [`Buffer::reload`], with its stamp taken before reading.
    pub fn read_file(file: File) -> io::Result<(DecodedFile, DiskStamp)> {
        let stamp = DiskStamp::from_metadata(&file.metadata()?)?;
        let decoded = Text::from_reader_detecting(io::BufReader::with_capacity(256 * 1024, file))?;
        Ok((decoded, stamp))
    }

    fn with_text(text: Text, line_ending: LineEnding) -> Self {
        Self {
            saved_text: text.clone(),
            saved_line_ending: line_ending,
            saved_encoding: FileEncoding::UTF8,
            changed_since_save: None,
            dirty: false,
            disk_stamp: None,
            restore: None,
            unsaved: false,
            snapshot: BufferSnapshot {
                id: BufferId(NEXT_BUFFER_ID.fetch_add(1, Ordering::Relaxed)),
                text,
                log: EditLog::new(),
            },
            line_ending,
            encoding: FileEncoding::UTF8,
            replaced_on_read: false,
            history: VecDeque::new(),
            open: None,
            aside: None,
            next_transaction: 0,
            undo_clock: 0,
            redoable: HashMap::new(),
            history_bytes: HashMap::new(),
            history_limit: Self::DEFAULT_HISTORY_LIMIT,
            saved_version: Version::default(),
            subscribers: Vec::new(),
        }
    }

    pub fn id(&self) -> BufferId {
        self.snapshot.id
    }

    pub fn version(&self) -> Version {
        self.snapshot.version()
    }

    pub fn text(&self) -> &Text {
        &self.snapshot.text
    }

    /// The current state, borrowed. Use [`Buffer::snapshot`] to keep it.
    pub fn current(&self) -> &BufferSnapshot {
        &self.snapshot
    }

    /// An immutable copy of the current state. O(1).
    pub fn snapshot(&self) -> BufferSnapshot {
        self.snapshot.clone()
    }

    pub fn len(&self) -> usize {
        self.snapshot.len()
    }

    pub fn is_empty(&self) -> bool {
        self.snapshot.is_empty()
    }

    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    pub fn set_line_ending(&mut self, line_ending: LineEnding) {
        self.line_ending = line_ending;
    }

    /// Whether the text, line ending or encoding differs from the last
    /// save, or the buffer holds a restored backup (see
    /// [`Buffer::from_backup`]). Undoing or retyping back to the saved text
    /// makes the buffer clean again.
    pub fn is_dirty(&self) -> bool {
        self.dirty
            || self.line_ending != self.saved_line_ending
            || self.encoding != self.saved_encoding
            || self.restore.as_ref().is_some_and(|restore| restore.applied)
            || self.unsaved
    }

    /// Makes the buffer dirty whatever its text, until the next save: for
    /// the text of a file that doesn't exist yet, which may be empty. Undo
    /// and redo don't clear it.
    pub fn mark_unsaved(&mut self) {
        self.unsaved = true;
    }

    /// Widens `changed_since_save` to cover `batch`, an edit just applied.
    fn note_batch(&mut self, edits: &[Edit]) {
        let (Some(first), Some(last)) = (edits.first(), edits.last()) else {
            return;
        };
        let old = first.old.start..last.old.end;
        let new_end = last.new.end;
        let delta = new_end as isize - old.end as isize;
        self.changed_since_save = Some(match self.changed_since_save.take() {
            None => old.start..new_end,
            Some(h) => {
                let end = (h.end.max(old.end) as isize + delta) as usize;
                h.start.min(old.start)..end
            }
        });
    }

    /// Recomputes `dirty` once all of an operation's batches are noted. A
    /// length change is dirty without comparing; otherwise only the changed
    /// region is compared.
    fn refresh_dirty(&mut self) {
        if self.len() != self.saved_text.len() {
            self.dirty = true;
            return;
        }
        let equal = match &self.changed_since_save {
            None => true,
            Some(r) => chunks_equal(
                self.snapshot.text.chunks_in_range(r.clone()),
                self.saved_text.chunks_in_range(r.clone()),
            ),
        };
        if equal {
            self.dirty = false;
            self.changed_since_save = None;
            // Drop the pre-save leaves.
            self.saved_text = self.snapshot.text.clone();
        } else {
            self.dirty = true;
        }
    }

    /// The encoding [`Buffer::write_to`] writes in: the one the file was read
    /// in, or UTF-8 without a BOM.
    pub fn encoding(&self) -> FileEncoding {
        self.encoding
    }

    /// Writes the text with the buffer's line endings in its encoding, as
    /// [`SaveSnapshot::write_to`] does, refusing as it does. Call
    /// [`Buffer::did_save`] once it's safely on disk.
    pub fn write_to(&self, out: impl std::io::Write) -> std::io::Result<()> {
        self.save_snapshot().write_to(out)
    }

    /// What saving now would write, to write on another thread. O(1).
    pub fn save_snapshot(&self) -> SaveSnapshot {
        SaveSnapshot {
            buffer: self.id(),
            version: self.version(),
            text: self.snapshot.text.clone(),
            line_ending: self.line_ending,
            encoding: self.encoding,
            replaced_on_read: self.replaced_on_read,
        }
    }

    /// The file as it was when last read, reloaded or saved: `None` when
    /// it was missing, or when that isn't known (see
    /// [`Buffer::disk_stamp_known`]).
    pub fn disk_stamp(&self) -> Option<DiskStamp> {
        self.disk_stamp.flatten()
    }

    /// Whether the buffer knows the file it was read from or saved to, as
    /// [`Buffer::from_file`], [`Buffer::set_disk_stamp`], a save or a
    /// reload tell it, even if the file was missing. One made from
    /// text or a reader doesn't.
    pub fn disk_stamp_known(&self) -> bool {
        self.disk_stamp.is_some()
    }

    /// Records the file the text was read from as `stamp`, or as missing.
    pub fn set_disk_stamp(&mut self, stamp: Option<DiskStamp>) {
        self.disk_stamp = Some(stamp);
    }

    /// Records that the current text is on disk. A save-as goes through
    /// here; see [`Buffer::reload`] for reading the file again.
    pub fn did_save(&mut self) {
        self.saved_text = self.snapshot.text.clone();
        self.saved_line_ending = self.line_ending;
        self.saved_encoding = self.encoding;
        self.restore = None;
        self.unsaved = false;
        self.changed_since_save = None;
        self.dirty = false;
        self.saved_version = self.version();
        self.emit(BufferEvent::Saved {
            version: self.saved_version,
        });
    }

    /// Records that `snapshot`, taken from this buffer, is on disk as the
    /// file `stamp` describes. Edits made since it leave the buffer dirty
    /// until they're undone, or retyped back to the snapshot's text.
    pub fn did_save_snapshot(&mut self, snapshot: &SaveSnapshot, stamp: Option<DiskStamp>) {
        debug_assert_eq!(snapshot.buffer, self.id(), "another buffer's snapshot");
        if snapshot.version == self.version() {
            self.did_save();
            self.saved_line_ending = snapshot.line_ending;
            self.saved_encoding = snapshot.encoding;
        } else {
            self.saved_text = snapshot.text.clone();
            self.saved_line_ending = snapshot.line_ending;
            self.saved_encoding = snapshot.encoding;
            self.saved_version = snapshot.version;
            self.restore = None;
            self.unsaved = false;
            self.changed_since_save = Some(0..self.len());
            self.refresh_dirty();
            self.emit(BufferEvent::Saved {
                version: snapshot.version,
            });
        }
        self.disk_stamp = Some(stamp);
    }

    /// Replaces the text with `file`'s, read from disk, as one transaction
    /// of `origin`, so undo takes the reload back. The edit covers only the
    /// part that differs, so anchors before and after it keep their places.
    /// Adopts the file's encoding and line ending, then marks the buffer
    /// saved, as of `stamp`. Text equal to the file's makes no edit but is
    /// still marked saved.
    ///
    /// The reload never joins an open transaction. One of another origin is
    /// set aside meanwhile. One of `origin` is split: it closes before the
    /// reload, and a new one, as deeply nested, opens after it, so edits
    /// that follow come after the reload in undo order; the matching
    /// [`Buffer::end_transaction`]s close the new one.
    pub fn reload(
        &mut self,
        file: DecodedFile,
        stamp: Option<DiskStamp>,
        origin: Origin,
    ) -> Option<TransactionId> {
        let (old, new) = differing_ranges(&self.snapshot.text, &file.text);
        let id = if old.is_empty() && new.is_empty() {
            None
        } else {
            let new_text = file.text.slice_to_string(new);
            let split = match self.open.take() {
                Some(open) if open.origin == origin => {
                    self.finish_transaction(open.id);
                    Some(open.depth)
                }
                open => {
                    self.open = open;
                    None
                }
            };
            let id = self
                .with_transaction_aside(|buffer| buffer.edit([(old, new_text)], origin.clone()));
            if let Some(depth) = split {
                self.start_transaction(origin);
                if let Some(open) = &mut self.open {
                    open.depth = depth;
                }
            }
            id
        };
        self.line_ending = file.line_ending;
        self.encoding = file.encoding;
        self.replaced_on_read = file.replaced;
        self.did_save();
        self.disk_stamp = Some(stamp);
        id
    }

    /// Caps each origin's undo history at `bytes`, counting each batch's old
    /// and new text plus the bookkeeping it holds (its edits, text handles
    /// and entry), so a long run of tiny edits is bounded too.
    /// Once a transaction closes, that origin's oldest transactions are
    /// dropped, and can no longer be undone or redone, until it's under the
    /// cap. The newest is always kept, however large. The edit log is
    /// unaffected; see [`Buffer::compact_log`].
    pub fn set_history_limit(&mut self, bytes: usize) {
        self.history_limit = bytes;
        let origins: Vec<Origin> = self.history_bytes.keys().cloned().collect();
        for origin in origins {
            self.trim_history(&origin, None);
        }
    }

    pub fn history_limit(&self) -> usize {
        self.history_limit
    }

    /// Bytes `origin`'s undo history is charged: its batches' old and new
    /// text plus their bookkeeping, undone batches included.
    pub fn history_bytes(&self, origin: &Origin) -> usize {
        self.history_bytes.get(origin).copied().unwrap_or(0)
    }

    /// Drops edit-log entries below `floor` (clamped to the current version),
    /// which the caller promises no live anchor, snapshot or `edits_since`
    /// cursor still needs: the buffer can't see those, so whoever holds them
    /// picks the floor, typically the oldest version among them after
    /// [refreshing](BufferSnapshot::refresh) anchors. Version numbers don't
    /// change, and snapshots taken earlier keep their own log.
    ///
    /// Transactions with a batch below the floor, applied or undone, are
    /// dropped from the history. An open transaction is kept, but undoing it
    /// fails with [`UndoError::Conflict`]. So does undoing or redoing a kept
    /// transaction whose batches derive, through earlier undos and redos,
    /// from an edit below the floor.
    pub fn compact_log(&mut self, floor: Version) {
        self.snapshot.log.compact(floor);
        let floor = self.snapshot.log.floor();
        let open = self.open.as_ref().map(|open| open.id);
        let mut freed: HashMap<Origin, (usize, usize)> = HashMap::new();
        self.history.retain(|entry| {
            let undone = entry.undone.iter().flat_map(|undone| &undone.batches);
            let keep = Some(entry.id) == open
                || entry
                    .batches
                    .iter()
                    .chain(undone)
                    .all(|batch| batch.version > floor);
            if !keep {
                let (bytes, undone) = freed.entry(entry.origin.clone()).or_default();
                *bytes += entry_bytes(entry);
                *undone += usize::from(entry.undone.is_some());
            }
            keep
        });
        for (origin, (bytes, undone)) in freed {
            if let Some(held) = self.history_bytes.get_mut(&origin) {
                *held -= bytes;
            }
            if let Some(count) = self.redoable.get_mut(&origin) {
                *count -= undone;
            }
        }
    }

    pub fn subscribe(&mut self) -> Subscription {
        let queue = Arc::new(Mutex::new(Vec::new()));
        self.subscribers.push(Arc::downgrade(&queue));
        Subscription(queue)
    }

    /// Applies `edits` atomically. Ranges are in the current coordinates and
    /// are clipped to the text; overlapping edits are merged. Returns `None`
    /// if nothing changed.
    pub fn edit<I, S>(&mut self, edits: I, origin: Origin) -> Option<TransactionId>
    where
        I: IntoIterator<Item = (Range<usize>, S)>,
        S: Into<Arc<str>>,
    {
        let text = &self.snapshot.text;
        let edits = edits
            .into_iter()
            .map(|(range, new_text)| (clip_range(text, range), normalize_newlines(new_text.into())))
            .collect();
        let batch = apply(&mut self.snapshot, edits, None)?;
        let id = self.record(origin.clone(), batch.clone());
        let text = self.snapshot.text.clone();
        self.note_batch(&batch.edits);
        self.refresh_dirty();
        self.emit_batch(id, &origin, EditKind::Edit, &batch, text);
        Some(id)
    }

    /// Applies `edits` as one more batch of the history entry `transaction`,
    /// open or closed, under that entry's origin. Undo and redo then treat the
    /// entry's batches as one unit. This is how a long-running edit (an AI
    /// stream) keeps one undo group while other origins edit, and hold their
    /// own open transactions, in between. The open transaction, of any origin,
    /// is left as it was: later edits of its origin still join it.
    ///
    /// Ranges are in the current coordinates, clipped as in [`Buffer::edit`].
    /// Returns `Ok(None)` if nothing changed. Fails with
    /// [`EditError::NoSuchTransaction`], changing nothing, if the entry is
    /// undone or no longer in the history.
    pub fn edit_in<I, S>(
        &mut self,
        transaction: TransactionId,
        edits: I,
    ) -> Result<Option<TransactionId>, EditError>
    where
        I: IntoIterator<Item = (Range<usize>, S)>,
        S: Into<Arc<str>>,
    {
        let origin = self
            .history
            .iter()
            .rfind(|entry| entry.id == transaction && entry.undone.is_none())
            .map(|entry| entry.origin.clone())
            .ok_or(EditError::NoSuchTransaction(transaction))?;
        let text = &self.snapshot.text;
        let edits = edits
            .into_iter()
            .map(|(range, new_text)| (clip_range(text, range), normalize_newlines(new_text.into())))
            .collect();
        let Some(batch) = apply(&mut self.snapshot, edits, None) else {
            return Ok(None);
        };
        self.charge_batch(&origin, &batch);
        self.history
            .iter_mut()
            .rfind(|entry| entry.id == transaction)
            .expect("entry checked above; charging only drops undone entries")
            .batches
            .push(batch.clone());
        self.trim_history(&origin, Some(transaction));
        let text = self.snapshot.text.clone();
        self.note_batch(&batch.edits);
        self.refresh_dirty();
        self.emit_batch(transaction, &origin, EditKind::Edit, &batch, text);
        Ok(Some(transaction))
    }

    /// Applies edits computed against an older snapshot of this buffer, such
    /// as an AI proposal. Each range is mapped to the current text; if the
    /// text it covered has changed since `base`, nothing is applied and a
    /// conflict is returned. Text inserted at a range's edges since `base` is
    /// kept, not replaced.
    pub fn edit_from<I, S>(
        &mut self,
        base: &BufferSnapshot,
        edits: I,
        origin: Origin,
    ) -> Result<Option<TransactionId>, EditError>
    where
        I: IntoIterator<Item = (Range<usize>, S)>,
        S: Into<Arc<str>>,
    {
        if base.id != self.id()
            || base.version() > self.version()
            || base.version() < self.snapshot.oldest_version()
        {
            return Err(EditError::ForeignSnapshot);
        }
        let current = &self.snapshot;
        let mut mapped = Vec::new();
        for (index, (range, new_text)) in edits.into_iter().enumerate() {
            let range = clip_range(&base.text, range);
            let from = base.version();
            let start = current.map_offset(range.start, Bias::Right, from);
            let end = if range.is_empty() {
                start
            } else {
                current.map_offset(range.end, Bias::Left, from)
            };
            let expected = base.text.slice_to_string(range);
            if end < start || !current.text.range_eq(start..end, &expected) {
                return Err(EditError::Conflict { index });
            }
            mapped.push((start..end, new_text.into()));
        }
        Ok(self.edit(mapped, origin))
    }

    /// Groups subsequent edits with the same origin into one transaction,
    /// undone as a unit, until the matching [`Buffer::end_transaction`].
    /// Calls nest; edits from other origins get their own transactions.
    pub fn start_transaction(&mut self, origin: Origin) -> TransactionId {
        if let Some(open) = &mut self.open {
            debug_assert_eq!(
                open.origin, origin,
                "nested transaction with another origin"
            );
            open.depth += 1;
            return open.id;
        }
        let id = self.next_transaction_id();
        *self.history_bytes.entry(origin.clone()).or_default() += ENTRY_OVERHEAD;
        self.history.push_back(HistoryEntry {
            id,
            origin: origin.clone(),
            batches: Vec::new(),
            undone: None,
        });
        self.open = Some(OpenTransaction {
            id,
            origin,
            depth: 1,
        });
        id
    }

    /// The transaction [`Buffer::start_transaction`] opened, until its
    /// matching [`Buffer::end_transaction`] (or an undo or redo) closes it.
    pub fn open_transaction(&self) -> Option<TransactionId> {
        self.open.as_ref().map(|open| open.id)
    }

    /// Runs `f` with any open transaction set aside, so its edits start
    /// transactions of their own even under the open one's origin, then
    /// restores it. The set-aside transaction is protected from history
    /// trimming meanwhile, so it comes back as the caller had it.
    pub fn with_transaction_aside<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let open = self.open.take();
        let outer_aside = std::mem::replace(&mut self.aside, open.as_ref().map(|open| open.id));
        let result = f(self);
        self.aside = outer_aside;
        self.open = open;
        result
    }

    /// Closes the innermost open transaction. Returns its id once the
    /// outermost one closes, if it changed anything.
    pub fn end_transaction(&mut self) -> Option<TransactionId> {
        let open = self.open.as_mut()?;
        open.depth -= 1;
        if open.depth > 0 {
            return None;
        }
        let id = open.id;
        self.open = None;
        self.finish_transaction(id)
    }

    fn close_open_transaction(&mut self) {
        if let Some(open) = self.open.take() {
            self.finish_transaction(open.id);
        }
    }

    fn finish_transaction(&mut self, id: TransactionId) -> Option<TransactionId> {
        let ix = self.history.iter().rposition(|entry| entry.id == id)?;
        if self.history[ix].batches.is_empty() {
            let entry = self.history.remove(ix).expect("index is in range");
            if let Some(held) = self.history_bytes.get_mut(&entry.origin) {
                *held -= ENTRY_OVERHEAD;
            }
            None
        } else {
            let origin = self.history[ix].origin.clone();
            self.trim_history(&origin, None);
            Some(id)
        }
    }

    /// Undoes the most recent transaction whose origin matches `filter`.
    /// Returns `Ok(None)` if there is nothing to undo.
    pub fn undo(
        &mut self,
        filter: impl Fn(&Origin) -> bool,
    ) -> Result<Option<TransactionId>, UndoError> {
        self.close_open_transaction();
        let Some(ix) = self
            .history
            .iter()
            .rposition(|entry| entry.undone.is_none() && filter(&entry.origin))
        else {
            return Ok(None);
        };
        let mut state = self.snapshot.clone();
        let entry = &self.history[ix];
        let (batches, texts): (Vec<_>, Vec<_>) = revert(&mut state, &entry.batches)
            .map_err(|()| UndoError::Conflict(entry.id))?
            .into_iter()
            .unzip();
        self.snapshot = state;
        self.undo_clock += 1;
        let entry = &mut self.history[ix];
        let added = batches_bytes(&batches);
        entry.undone = Some(Undone {
            batches: batches.clone(),
            at: self.undo_clock,
        });
        let (id, origin) = (entry.id, entry.origin.clone());
        self.note_restore(id, false);
        *self.history_bytes.entry(origin.clone()).or_default() += added;
        *self.redoable.entry(origin.clone()).or_default() += 1;
        for batch in &batches {
            self.note_batch(&batch.edits);
        }
        self.refresh_dirty();
        for (batch, text) in batches.iter().zip(texts) {
            self.emit_batch(id, &origin, EditKind::Undo, batch, text);
        }
        Ok(Some(id))
    }

    /// Redoes the most recently undone transaction whose origin matches
    /// `filter`. A new edit from an origin discards that origin's redos.
    pub fn redo(
        &mut self,
        filter: impl Fn(&Origin) -> bool,
    ) -> Result<Option<TransactionId>, UndoError> {
        self.close_open_transaction();
        let Some(ix) = self
            .history
            .iter()
            .enumerate()
            .filter(|(_, entry)| filter(&entry.origin))
            .filter_map(|(ix, entry)| entry.undone.as_ref().map(|undone| (undone.at, ix)))
            .max()
            .map(|(_, ix)| ix)
        else {
            return Ok(None);
        };
        let mut state = self.snapshot.clone();
        let entry = &self.history[ix];
        let undone = entry.undone.as_ref().expect("filtered on undone");
        let (batches, texts): (Vec<_>, Vec<_>) = revert(&mut state, &undone.batches)
            .map_err(|()| UndoError::Conflict(entry.id))?
            .into_iter()
            .unzip();
        self.snapshot = state;
        let entry = &mut self.history[ix];
        let removed = entry_bytes(entry);
        entry.undone = None;
        entry.batches = batches.clone();
        let added = entry_bytes(entry);
        let (id, origin) = (entry.id, entry.origin.clone());
        self.note_restore(id, true);
        let held = self.history_bytes.entry(origin.clone()).or_default();
        *held = *held - removed + added;
        if let Some(count) = self.redoable.get_mut(&origin) {
            *count -= 1;
        }
        for batch in &batches {
            self.note_batch(&batch.edits);
        }
        self.refresh_dirty();
        for (batch, text) in batches.iter().zip(texts) {
            self.emit_batch(id, &origin, EditKind::Redo, batch, text);
        }
        Ok(Some(id))
    }

    fn next_transaction_id(&mut self) -> TransactionId {
        self.next_transaction += 1;
        TransactionId(self.next_transaction)
    }

    /// Bookkeeping for a fresh batch from `origin`: discards the origin's
    /// redo history and counts the batch's bytes.
    fn charge_batch(&mut self, origin: &Origin, batch: &AppliedBatch) {
        // A fresh edit discards this origin's redo history.
        if self.redoable.remove(origin).is_some_and(|count| count > 0) {
            let mut freed = 0;
            self.history.retain(|entry| {
                let keep = entry.undone.is_none() || entry.origin != *origin;
                if !keep {
                    freed += entry_bytes(entry);
                }
                keep
            });
            if let Some(held) = self.history_bytes.get_mut(origin) {
                *held -= freed;
            }
        }
        *self.history_bytes.entry(origin.clone()).or_default() += batch_bytes(batch);
    }

    fn record(&mut self, origin: Origin, batch: AppliedBatch) -> TransactionId {
        self.charge_batch(&origin, &batch);

        if let Some(open) = &self.open
            && open.origin == origin
        {
            let id = open.id;
            let entry = self
                .history
                .iter_mut()
                .rfind(|entry| entry.id == id)
                .expect("open transaction is in the history");
            entry.batches.push(batch);
            return id;
        }
        let id = self.next_transaction_id();
        *self.history_bytes.entry(origin.clone()).or_default() += ENTRY_OVERHEAD;
        self.history.push_back(HistoryEntry {
            id,
            origin: origin.clone(),
            batches: vec![batch],
            undone: None,
        });
        self.trim_history(&origin, None);
        id
    }

    /// Drops `origin`'s oldest closed transactions until its history is
    /// under the limit, always keeping its newest, and `keep` if given.
    fn trim_history(&mut self, origin: &Origin, keep: Option<TransactionId>) {
        let Some(&held) = self.history_bytes.get(origin) else {
            return;
        };
        if held <= self.history_limit {
            return;
        }
        let open = self.open.as_ref().map(|open| open.id);
        let aside = self.aside;
        let droppable = |entry: &HistoryEntry| {
            entry.origin == *origin
                && Some(entry.id) != open
                && Some(entry.id) != aside
                && Some(entry.id) != keep
        };
        // The origin's newest closed entry is never dropped; it is found
        // from the back, so this stops at once in practice.
        let Some(newest) = self.history.iter().rposition(&droppable) else {
            return;
        };
        let mut held = held;
        let mut undone = 0;
        let mut last = None;
        for (ix, entry) in self.history.iter().enumerate().take(newest) {
            if held <= self.history_limit {
                break;
            }
            if droppable(entry) {
                held -= entry_bytes(entry);
                undone += usize::from(entry.undone.is_some());
                last = Some(ix);
            }
        }
        let Some(last) = last else {
            return;
        };
        // Only the prefix up to the last dropped entry is touched; every
        // droppable entry of the origin in it is dropped.
        let mut prefix = last + 1;
        while prefix > 0 && self.history.front().is_some_and(&droppable) {
            self.history.pop_front();
            prefix -= 1;
        }
        if prefix > 0 {
            let kept: Vec<HistoryEntry> = self
                .history
                .drain(..prefix)
                .filter(|entry| !droppable(entry))
                .collect();
            for entry in kept.into_iter().rev() {
                self.history.push_front(entry);
            }
        }
        self.history_bytes.insert(origin.clone(), held);
        if undone > 0
            && let Some(count) = self.redoable.get_mut(origin)
        {
            *count -= undone;
        }
    }

    fn emit_batch(
        &mut self,
        transaction: TransactionId,
        origin: &Origin,
        kind: EditKind,
        batch: &AppliedBatch,
        text: Text,
    ) {
        self.emit(BufferEvent::Edited {
            transaction,
            origin: origin.clone(),
            kind,
            from: Version(batch.version.0 - 1),
            to: batch.version,
            edits: batch.edits.clone(),
            text,
        });
    }

    fn emit(&mut self, event: BufferEvent) {
        self.subscribers
            .retain(|subscriber| match subscriber.upgrade() {
                Some(queue) => {
                    queue
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(event.clone());
                    true
                }
                None => false,
            });
    }
}

/// Fixed cost of a history entry: the entry itself.
const ENTRY_OVERHEAD: usize = std::mem::size_of::<HistoryEntry>();
/// Fixed cost of a batch: the batch, plus the allocation headers (strong and
/// weak counts) of its `Arc<[Edit]>` and `Arc<[Collapse]>`.
const BATCH_OVERHEAD: usize = std::mem::size_of::<AppliedBatch>() + 2 * ARC_HEADER;
/// Cost of an edit: its `Edit`, plus an old and a new `Arc<str>` handle and
/// their allocation headers.
const EDIT_OVERHEAD: usize =
    std::mem::size_of::<Edit>() + 2 * (std::mem::size_of::<Arc<str>>() + ARC_HEADER);
const ARC_HEADER: usize = 2 * std::mem::size_of::<usize>();

/// Bytes a history entry is charged, including its undone batches.
fn entry_bytes(entry: &HistoryEntry) -> usize {
    ENTRY_OVERHEAD
        + batches_bytes(&entry.batches)
        + entry
            .undone
            .as_ref()
            .map_or(0, |undone| batches_bytes(&undone.batches))
}

fn batches_bytes(batches: &[AppliedBatch]) -> usize {
    batches.iter().map(batch_bytes).sum()
}

fn batch_bytes(batch: &AppliedBatch) -> usize {
    let text: usize = batch
        .old_texts
        .iter()
        .chain(&batch.new_texts)
        .map(|text| text.len())
        .sum();
    BATCH_OVERHEAD
        + batch.edits.len() * EDIT_OVERHEAD
        + batch.collapsed_by.len() * std::mem::size_of::<Collapse>()
        + text
}

fn clip_range(text: &Text, range: Range<usize>) -> Range<usize> {
    let (start, end) = if range.start <= range.end {
        (range.start, range.end)
    } else {
        (range.end, range.start)
    };
    text.clip_offset(start, Bias::Left)..text.clip_offset(end, Bias::Right)
}

fn normalize_newlines(text: Arc<str>) -> Arc<str> {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n").into()
    } else {
        text
    }
}

/// Applies edits (in the current coordinates) as one batch and logs it.
/// `inverts` is the version produced by the batch this one exactly reverses.
fn apply(
    snapshot: &mut BufferSnapshot,
    edits: Vec<(Range<usize>, Arc<str>)>,
    inverts: Option<Version>,
) -> Option<AppliedBatch> {
    let edits = normalize(edits);
    if edits.is_empty() {
        return None;
    }
    let mut applied = Vec::with_capacity(edits.len());
    let mut old_texts = Vec::with_capacity(edits.len());
    let mut new_texts = Vec::with_capacity(edits.len());
    let mut delta = 0isize;
    for (range, new_text) in &edits {
        let new_start = range
            .start
            .checked_add_signed(delta)
            .expect("edits are sorted");
        applied.push(Edit {
            old: range.clone(),
            new: new_start..new_start + new_text.len(),
        });
        old_texts.push(snapshot.text.slice_to_string(range.clone()).into());
        new_texts.push(new_text.clone());
        delta += new_text.len() as isize - range.len() as isize;
    }
    for (range, new_text) in edits.iter().rev() {
        snapshot.text.replace(range.clone(), new_text);
    }
    let edits: Arc<[Edit]> = applied.into();
    snapshot.log.push(edits.clone(), inverts);
    Some(AppliedBatch {
        version: snapshot.version(),
        edits,
        old_texts,
        new_texts,
        collapsed_by: Arc::from([]),
    })
}

/// Applies the inverse of `batches`, newest first, mapping each through the
/// edits made since. Fails without a partial result if any text the batches
/// wrote has since changed. Returns the inverse batches, which `revert` can
/// take again to redo, each with the text right after it.
fn revert(
    state: &mut BufferSnapshot,
    batches: &[AppliedBatch],
) -> Result<Vec<(AppliedBatch, Text)>, ()> {
    let mut produced = Vec::with_capacity(batches.len());
    for batch in batches.iter().rev() {
        let mut inverse = Vec::with_capacity(batch.edits.len());
        // Collapses this batch's positions inherited, plus any found now.
        let mut collapsed_by = batch.collapsed_by.to_vec();
        for ((edit, old_text), new_text) in batch
            .edits
            .iter()
            .zip(&batch.old_texts)
            .zip(&batch.new_texts)
        {
            let mut map = |offset, mapping| {
                state
                    .map_for_history(offset, mapping, batch.version, &mut collapsed_by)
                    .ok_or(())
            };
            let range = if edit.new.is_empty() {
                let at = map(edit.new.start, Mapping::RESTORE)?;
                at..at
            } else {
                let start = map(edit.new.start, Bias::Right.into())?;
                let end = map(edit.new.end, Bias::Left.into())?;
                if end < start {
                    return Err(());
                }
                start..end
            };
            if !state.text.range_eq(range.clone(), new_text) {
                return Err(());
            }
            inverse.push((range, old_text.clone()));
        }
        if let Some(mut applied) = apply(state, inverse, Some(batch.version)) {
            collapsed_by.sort_unstable();
            collapsed_by.dedup();
            applied.collapsed_by = collapsed_by.into();
            produced.push((applied, state.text.clone()));
        }
    }
    Ok(produced)
}

/// The ranges of `old` and `new` between their longest common prefix and
/// suffix, on char boundaries: replacing the first with the second's text
/// turns `old` into `new`. Walks both texts without copying either.
fn differing_ranges(old: &Text, new: &Text) -> (Range<usize>, Range<usize>) {
    let prefix = common_prefix(old, new);
    // The bytes before `prefix` are the same in both, so a char boundary in
    // one is one in the other.
    let prefix = old.clip_offset(prefix, Bias::Left);
    let suffix = common_suffix(old, new, old.len().min(new.len()) - prefix);
    // Likewise for the bytes after.
    let old_end = old.clip_offset(old.len() - suffix, Bias::Right);
    let suffix = old.len() - old_end;
    (prefix..old_end, prefix..new.len() - suffix)
}

/// How many leading bytes `a` and `b` share.
fn common_prefix(a: &Text, b: &Text) -> usize {
    let (mut a, mut b) = (a.chunks().map(str::as_bytes), b.chunks().map(str::as_bytes));
    let (mut x, mut y): (&[u8], &[u8]) = (&[], &[]);
    let mut shared = 0;
    loop {
        if x.is_empty() {
            match a.next() {
                Some(chunk) => x = chunk,
                None => return shared,
            }
            continue;
        }
        if y.is_empty() {
            match b.next() {
                Some(chunk) => y = chunk,
                None => return shared,
            }
            continue;
        }
        let n = x.len().min(y.len());
        let same = x[..n]
            .iter()
            .zip(&y[..n])
            .take_while(|(p, q)| p == q)
            .count();
        shared += same;
        if same < n {
            return shared;
        }
        (x, y) = (&x[n..], &y[n..]);
    }
}

/// How many trailing bytes, up to `limit`, `a` and `b` share. Compares a
/// window at a time from the end, since the rope's chunks only go forward.
fn common_suffix(a: &Text, b: &Text, limit: usize) -> usize {
    const WINDOW: usize = 64 * 1024;
    let (mut x, mut y) = (Vec::new(), Vec::new());
    let mut shared = 0;
    while shared < limit {
        let take = (limit - shared).min(WINDOW);
        read_bytes(a, a.len() - shared - take, take, &mut x);
        read_bytes(b, b.len() - shared - take, take, &mut y);
        let same = x
            .iter()
            .rev()
            .zip(y.iter().rev())
            .take_while(|(p, q)| p == q)
            .count();
        shared += same;
        if same < take {
            break;
        }
    }
    shared
}

/// Copies `len` bytes of `text` from `start`, which needn't be a char
/// boundary, into `out`.
fn read_bytes(text: &Text, start: usize, len: usize, out: &mut Vec<u8>) {
    out.clear();
    while out.len() < len {
        let bytes = text.bytes_at(start + out.len());
        let n = bytes.len().min(len - out.len());
        out.extend_from_slice(&bytes[..n]);
    }
}

/// Whether two chunk sequences hold the same bytes, however they're split.
fn chunks_equal<'a>(a: impl Iterator<Item = &'a str>, b: impl Iterator<Item = &'a str>) -> bool {
    let (mut a, mut b) = (a.map(str::as_bytes), b.map(str::as_bytes));
    let (mut x, mut y): (&[u8], &[u8]) = (&[], &[]);
    loop {
        while x.is_empty() {
            match a.next() {
                Some(c) => x = c,
                None => break,
            }
        }
        while y.is_empty() {
            match b.next() {
                Some(c) => y = c,
                None => break,
            }
        }
        if x.is_empty() || y.is_empty() {
            return x.is_empty() && y.is_empty();
        }
        let n = x.len().min(y.len());
        if x[..n] != y[..n] {
            return false;
        }
        (x, y) = (&x[n..], &y[n..]);
    }
}
