use std::{
    collections::HashMap,
    ops::Range,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use fe_text::{Bias, EncodeError, FileEncoding, LineEnding, Text};

use crate::{
    BufferId, BufferSnapshot, Edit, Version,
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
    history: Vec<HistoryEntry>,
    open: Option<OpenTransaction>,
    next_transaction: u64,
    undo_clock: u64,
    /// How many undone (redoable) transactions each origin has, so edits only
    /// scan the history to discard redos when there are some.
    redoable: HashMap<Origin, usize>,
    /// Text bytes each origin's history entries hold; see [`entry_bytes`].
    history_bytes: HashMap<Origin, usize>,
    history_limit: usize,
    saved_version: Version,
    pub(crate) subscribers: Vec<Weak<Mutex<Vec<BufferEvent>>>>,
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
    /// Default for [`Buffer::set_history_limit`]: 16 MiB of text per origin.
    ///
    /// T7 (`examples/history_memory.rs`) measured a replace-all of 507k
    /// six-byte words with eight-byte ones on a 33 MB file: about 7.1 MB of
    /// old and new text per pass. This keeps the last two such passes
    /// undoable, and never limits ordinary typing.
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
        buffer.replaced_on_read = decoded.replaced;
        Ok(buffer)
    }

    fn with_text(text: Text, line_ending: LineEnding) -> Self {
        Self {
            snapshot: BufferSnapshot {
                id: BufferId(NEXT_BUFFER_ID.fetch_add(1, Ordering::Relaxed)),
                text,
                log: EditLog::new(),
            },
            line_ending,
            encoding: FileEncoding::UTF8,
            replaced_on_read: false,
            history: Vec::new(),
            open: None,
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

    pub fn is_dirty(&self) -> bool {
        self.version() != self.saved_version
    }

    /// The encoding [`Buffer::write_to`] writes in: the one the file was read
    /// in, or UTF-8 without a BOM.
    pub fn encoding(&self) -> FileEncoding {
        self.encoding
    }

    /// Writes the text with the buffer's line endings in its encoding. Call
    /// [`Buffer::did_save`] once it's safely on disk.
    ///
    /// Refuses with [`std::io::ErrorKind::InvalidData`] rather than lose data:
    /// before writing anything if the file had bytes that were invalid in its
    /// encoding and some of the U+FFFD they became are still in the text, and
    /// part-way through on a char the encoding can't represent. Output may
    /// have been written by then, so write to a temporary file and keep it
    /// only on success, as `EditorModel::save` does.
    pub fn write_to(&self, out: impl std::io::Write) -> std::io::Result<()> {
        let text = &self.snapshot.text;
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

    pub fn did_save(&mut self) {
        self.saved_version = self.version();
        self.emit(BufferEvent::Saved {
            version: self.saved_version,
        });
    }

    /// Caps each origin's undo history at `bytes` of old and new text.
    /// Once a transaction closes, that origin's oldest transactions are
    /// dropped, and can no longer be undone or redone, until it's under the
    /// cap. The newest is always kept, however large. The edit log is
    /// unaffected; see [`Buffer::compact_log`].
    pub fn set_history_limit(&mut self, bytes: usize) {
        self.history_limit = bytes;
        let origins: Vec<Origin> = self.history_bytes.keys().cloned().collect();
        for origin in origins {
            self.trim_history(&origin);
        }
    }

    pub fn history_limit(&self) -> usize {
        self.history_limit
    }

    /// Text bytes `origin`'s undo history holds.
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
        self.emit_batch(id, &origin, EditKind::Edit, &batch, text);
        Some(id)
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
        self.history.push(HistoryEntry {
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
            self.history.remove(ix);
            None
        } else {
            let origin = self.history[ix].origin.clone();
            self.trim_history(&origin);
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
        *self.history_bytes.entry(origin.clone()).or_default() += added;
        *self.redoable.entry(origin.clone()).or_default() += 1;
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
        let held = self.history_bytes.entry(origin.clone()).or_default();
        *held = *held - removed + added;
        if let Some(count) = self.redoable.get_mut(&origin) {
            *count -= 1;
        }
        for (batch, text) in batches.iter().zip(texts) {
            self.emit_batch(id, &origin, EditKind::Redo, batch, text);
        }
        Ok(Some(id))
    }

    fn next_transaction_id(&mut self) -> TransactionId {
        self.next_transaction += 1;
        TransactionId(self.next_transaction)
    }

    fn record(&mut self, origin: Origin, batch: AppliedBatch) -> TransactionId {
        // A fresh edit discards this origin's redo history.
        if self.redoable.remove(&origin).is_some_and(|count| count > 0) {
            let mut freed = 0;
            self.history.retain(|entry| {
                let keep = entry.undone.is_none() || entry.origin != origin;
                if !keep {
                    freed += entry_bytes(entry);
                }
                keep
            });
            if let Some(held) = self.history_bytes.get_mut(&origin) {
                *held -= freed;
            }
        }
        *self.history_bytes.entry(origin.clone()).or_default() += batch_bytes(&batch);

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
        self.history.push(HistoryEntry {
            id,
            origin: origin.clone(),
            batches: vec![batch],
            undone: None,
        });
        self.trim_history(&origin);
        id
    }

    /// Drops `origin`'s oldest closed transactions until its history is
    /// under the limit, always keeping its newest.
    fn trim_history(&mut self, origin: &Origin) {
        let Some(&held) = self.history_bytes.get(origin) else {
            return;
        };
        if held <= self.history_limit {
            return;
        }
        let open = self.open.as_ref().map(|open| open.id);
        // Indices of the origin's closed entries, oldest first; the last
        // is never dropped.
        let mut droppable: Vec<usize> = self
            .history
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.origin == *origin && Some(entry.id) != open)
            .map(|(ix, _)| ix)
            .collect();
        droppable.pop();
        let mut held = held;
        let mut drop_ids = Vec::new();
        let mut undone = 0;
        for ix in droppable {
            if held <= self.history_limit {
                break;
            }
            let entry = &self.history[ix];
            held -= entry_bytes(entry);
            undone += usize::from(entry.undone.is_some());
            drop_ids.push(entry.id);
        }
        if drop_ids.is_empty() {
            return;
        }
        // `drop_ids` is in history order, so one ordered pass finds them.
        let mut next = drop_ids.iter().peekable();
        self.history.retain(|entry| {
            if next.peek() == Some(&&entry.id) {
                next.next();
                false
            } else {
                true
            }
        });
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

/// Text bytes a history entry holds, including its undone batches.
fn entry_bytes(entry: &HistoryEntry) -> usize {
    batches_bytes(&entry.batches)
        + entry
            .undone
            .as_ref()
            .map_or(0, |undone| batches_bytes(&undone.batches))
}

fn batches_bytes(batches: &[AppliedBatch]) -> usize {
    batches.iter().map(batch_bytes).sum()
}

fn batch_bytes(batch: &AppliedBatch) -> usize {
    batch
        .old_texts
        .iter()
        .chain(&batch.new_texts)
        .map(|text| text.len())
        .sum()
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
