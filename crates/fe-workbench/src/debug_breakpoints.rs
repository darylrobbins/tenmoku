//! The breakpoint store (docs/decisions/r1b9dw3g0g-debugger.md §5, card
//! D5): every breakpoint of a project, kept on its code as the user edits
//! and persisted in the state directory.
//!
//! - **Per root.** One [`BreakpointStore`] per canonical root, shared by
//!   every window on it ([`BreakpointStore::for_root`]), so two windows
//!   never write one file with different sets.
//! - **Two places.** Each breakpoint has its line in the file on disk,
//!   which is what's persisted, and, in each open buffer of the file, a
//!   row there. Unsaved edits move only the buffer's row; a save makes
//!   the rows of the text saved the lines on disk. So a write never holds
//!   unsaved rows, and closing without saving leaves the lines as they
//!   were. Two buffers of one file (two windows that opened it apart)
//!   each keep their own rows.
//! - **Anchored.** In a buffer, each breakpoint holds an [`Anchor`] with
//!   [`Bias::Right`] at its row's start, and its row is read back from the
//!   anchor after each edit. A newline typed at the very start of the row
//!   (`O` on it, Enter at column 0) pushes the code down and the breakpoint
//!   with it; deleting the newline before the row (joining it to the row
//!   above) collapses the anchor onto the row above, and deleting the row
//!   moves it to the row that survives in its place. Two that land on one
//!   row show as one, the first, the one that was higher up; a save of
//!   that text merges them, its settings kept.
//! - **Known limit: an empty row.** On an empty row, `O` on it and `o` on
//!   the row above are one edit, an insertion at one offset, so no anchor
//!   tells them apart: a breakpoint on an empty row moves down with either.
//! - **Replaced rows.** An edit that replaces rows with rows (a reload
//!   from disk, its undo or redo, a formatter's rewrite) would sweep the
//!   anchors inside it to one end. So such an edit moves the rows by a line
//!   diff of the text before and after instead, as `git_blame` maps rows: a
//!   row the edit kept stays on its text, and one it replaced keeps its
//!   place in the replacement.
//! - **Saves.** A buffer's save (or reload) of a version is the file on
//!   disk, for every buffer of it: the rows that version had, as recorded
//!   when the store saw it, are the lines on disk then, even if edits after
//!   it deleted or joined them. A buffer first seen with unsaved text (a
//!   restored backup) reads the file on disk to place the lines in its
//!   text. A session reads the lines on disk ([`BreakpointStore::enabled`]),
//!   the gutter a buffer's rows.
//! - **Persistence.** `<state dir>/debug/<layout_key(root)>.json`
//!   ([`state_file`]), a `breakpoints` list and a `watches` list. The store
//!   writes `watches` back as it read it, whatever it holds, for the Watch
//!   view (D9), and keeps any other key too. It's written when the set or
//!   the lines on disk change: [`WRITE_REST`] after the last change,
//!   through a temporary file renamed over the old one, off the UI thread
//!   and in order. A write that fails is tried again with the next change
//!   (sooner if changes came while it ran) and at quit, which writes
//!   anything not written yet. A file that can't
//!   be read is said and left as it is until the next change replaces it.
//!   Breakpoints are the user's state, not the project's, so nothing goes
//!   in `.tenmoku/`.
//!
//! Each window's [`init`] hook hands its file editors (tabs and diff
//! views' working sides) to its root's store as they open and close, so
//! the store needs no call from the workbench. Untitled editors join once
//! saved; an editor saved under a new path leaves the old path's
//! breakpoints where they are and takes the new path's.
//!
//! Its users: the gutter (D6) reads [`BreakpointStore::breakpoints_in`]
//! and toggles rows; a session (D7) sends [`BreakpointStore::enabled`]
//! per file in `setBreakpoints` on [`BreakpointEvent::Changed`] and
//! [`BreakpointEvent::Saved`]; the commands (D8) call the rest.

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, VecDeque},
    fs::{self, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use fe_buffer::{
    Anchor, Bias, BufferEvent, BufferId, BufferSnapshot, Edit, Subscription as BufferEvents, Text,
    Version,
};
pub use fe_dap::Breakpoint;
use fe_gpui::EditorView;
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, EventEmitter, Global, Subscription, Task,
    WeakEntity,
};
use imara_diff::{Algorithm, Diff, InternedInput};
use kiln_l10n::{tr, verbatim};
use serde_json::{Map, Value};

use crate::Workbench;

/// How long after the last change the state file is written.
pub const WRITE_REST: Duration = Duration::from_millis(500);

/// The most texts a buffer keeps since its last save, for a save that
/// lands after more edits; past it, the file on disk is read instead.
const KEPT_TEXTS: usize = 1024;

/// The state file of the project at the canonical `root`, under
/// `state_dir`: `debug/<layout_key(root)>.json`.
pub fn state_file(state_dir: &Path, root: &Path) -> PathBuf {
    let mut name = kiln::core::layout::layout_key(root);
    name.push_str(verbatim!(".json"));
    state_dir.join("debug").join(name)
}

/// Where the state files go, if not under the user's state directory.
struct StateDir(Option<PathBuf>);

impl Global for StateDir {}

/// Keeps the state files under `dir` from now on, or nowhere for `None`.
#[cfg(test)]
pub(crate) fn set_state_dir(cx: &mut App, dir: Option<PathBuf>) {
    cx.set_global(StateDir(dir));
}

/// The state directory set, or the user's. Under test, none unless set,
/// so no test touches the user's.
fn state_dir(cx: &App) -> Option<PathBuf> {
    match cx.try_global::<StateDir>() {
        Some(StateDir(dir)) => dir.clone(),
        None if cfg!(test) => None,
        None => fe_config::paths::state_dir(),
    }
}

/// The stores by canonical root, kept for the app's life. The only strong
/// handles to them are here and with callers: a window holds its store
/// weakly, as a closure outliving the app (a test app's observers) would
/// leak it.
#[derive(Default)]
struct Stores(HashMap<PathBuf, Entity<BreakpointStore>>);

impl Global for Stores {}

/// `path` with its symlinks resolved; for a file that's gone or not there
/// yet, its directory's, so it keys and writes (relative to the root) as
/// it would if it were there: a root reached through a symlink (macOS's
/// `/var`, say) resolves to where it is, and so must its missing files.
fn canonical(path: &Path) -> PathBuf {
    crate::disk_sync::canonicalize(path)
}

/// The text of the file at `path` on disk, decoded as a buffer reads it,
/// if it can be read.
fn read_disk(path: &Path) -> Option<Text> {
    let file = fs::File::open(path).ok()?;
    match Text::from_reader_detecting(io::BufReader::new(file)) {
        Ok(decoded) => Some(decoded.text),
        Err(error) => {
            log::warn!("breakpoints: can't read {}: {error}", path.display());
            None
        }
    }
}

/// A breakpoint shown in a buffer ([`BreakpointStore::placements_in`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// Its row in the buffer, from 0.
    pub row: u32,
    /// Its line on disk, from 1, which a session sends.
    pub line: u32,
    pub enabled: bool,
}

/// What a store tells as its breakpoints change, naming the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BreakpointEvent {
    /// The file's set changed through the store: one was added, removed,
    /// enabled or disabled. A session sends the file's set again.
    Changed(PathBuf),
    /// Edits moved the file's breakpoints in a buffer, or showed two as
    /// one. Not sent: the running program doesn't match unsaved text.
    Moved(PathBuf),
    /// A save or a reload of the file moved its lines on disk: a session
    /// sends the file's set again.
    Saved(PathBuf),
}

/// A breakpoint in the store: its line on disk and its settings.
#[derive(Clone, Debug)]
struct Entry {
    /// Unique in the store, and in order of adding.
    id: u64,
    /// Its row (from 0) in the file on disk, as last saved, read or
    /// reloaded.
    disk: u32,
    enabled: bool,
    condition: Option<String>,
    hit_condition: Option<String>,
    log_message: Option<String>,
}

impl Entry {
    fn from_breakpoint(id: u64, breakpoint: &Breakpoint) -> Self {
        Entry {
            id,
            disk: breakpoint.line.saturating_sub(1),
            enabled: breakpoint.enabled,
            condition: breakpoint.condition.clone(),
            hit_condition: breakpoint.hit_condition.clone(),
            log_message: breakpoint.log_message.clone(),
        }
    }

    /// As a breakpoint on `row` (from 0) of `path`.
    fn breakpoint(&self, path: PathBuf, row: u32) -> Breakpoint {
        Breakpoint {
            path,
            line: row + 1,
            enabled: self.enabled,
            condition: self.condition.clone(),
            hit_condition: self.hit_condition.clone(),
            log_message: self.log_message.clone(),
        }
    }
}

/// What a store saw of a buffer.
struct Seen {
    buffer: BufferId,
    dirty: bool,
    snapshot: BufferSnapshot,
}

impl Seen {
    fn of(editor: &EditorView) -> Seen {
        let buffer = editor.model().buffer();
        Seen {
            buffer: buffer.id(),
            dirty: buffer.is_dirty(),
            snapshot: buffer.snapshot(),
        }
    }
}

/// A breakpoint's place in a buffer.
#[derive(Clone, Debug)]
struct Placed {
    id: u64,
    row: u32,
    /// At the row's start, right-biased.
    anchor: Anchor,
}

/// What a buffer held at a version.
struct Kept {
    version: Version,
    text: Text,
    /// The breakpoints' rows then, as (id, row) by row, if the store saw
    /// that version.
    rows: Option<Vec<(u64, u32)>>,
}

/// An open buffer of a file, and the file's breakpoints in it.
struct Open {
    buffer: BufferId,
    events: BufferEvents,
    /// The buffer as of the rows.
    snapshot: BufferSnapshot,
    dirty: bool,
    /// The file on disk, if known: as the buffer last read or saved it,
    /// or as another buffer of the file saved it since.
    disk: Option<Text>,
    /// Whether `disk` is the buffer's own, so that its being clean means
    /// it holds `disk`.
    own_disk: bool,
    /// The buffer's versions since its last save, oldest first.
    kept: VecDeque<Kept>,
    /// Every one of the file's breakpoints, by row. On a row, the first
    /// is the one shown.
    placed: Vec<Placed>,
}

impl Open {
    /// Places `entries` in `seen`'s buffer, whose file on disk holds
    /// `disk`.
    fn new(seen: Seen, events: BufferEvents, disk: Option<Text>, entries: &[Entry]) -> Self {
        let mut open = Open {
            buffer: seen.buffer,
            events,
            kept: VecDeque::new(),
            snapshot: seen.snapshot,
            dirty: seen.dirty,
            disk,
            own_disk: true,
            placed: Vec::new(),
        };
        let rows: Vec<u32> = entries.iter().map(|entry| entry.disk).collect();
        let rows = open.rows_of_disk(&rows);
        open.placed = (entries.iter().zip(rows))
            .map(|(entry, row)| Placed {
                id: entry.id,
                row,
                anchor: row_anchor(&open.snapshot, row),
            })
            .collect();
        open.placed.sort_by_key(|placed| placed.row);
        open.note_rows();
        open
    }

    fn rows(&self) -> Vec<(u64, u32)> {
        (self.placed.iter())
            .map(|placed| (placed.id, placed.row))
            .collect()
    }

    fn keep(&mut self, kept: Kept) {
        self.kept.push_back(kept);
        if self.kept.len() > KEPT_TEXTS {
            self.kept.pop_front();
        }
    }

    /// Records the rows as of the version seen.
    fn note_rows(&mut self) {
        let version = self.snapshot.version();
        let rows = self.rows();
        match self.kept.back_mut() {
            Some(kept) if kept.version == version => kept.rows = Some(rows),
            _ => self.keep(Kept {
                version,
                text: self.snapshot.text().clone(),
                rows: Some(rows),
            }),
        }
    }

    fn last_row(&self) -> u32 {
        self.snapshot.text().row_count() - 1
    }

    /// The file on disk's text, unless the buffer's is known to be it.
    fn differing_disk(&self) -> Option<&Text> {
        self.disk.as_ref().filter(|_| self.dirty || !self.own_disk)
    }

    /// Where `rows` of the buffer are in the file on disk.
    fn disk_of_rows(&self, rows: &[u32]) -> Vec<u32> {
        match self.differing_disk() {
            Some(disk) => map_rows(self.snapshot.text(), disk, rows),
            None => rows.to_vec(),
        }
    }

    /// Where `rows` of the file on disk are in the buffer.
    fn rows_of_disk(&self, rows: &[u32]) -> Vec<u32> {
        let last = self.last_row();
        match self.differing_disk() {
            Some(disk) => map_rows(disk, self.snapshot.text(), rows),
            None => rows.iter().map(|row| (*row).min(last)).collect(),
        }
    }

    /// Places a breakpoint on `row`, after any already there.
    fn place(&mut self, id: u64, row: u32) {
        let at = self.placed.partition_point(|placed| placed.row <= row);
        let anchor = row_anchor(&self.snapshot, row);
        self.placed.insert(at, Placed { id, row, anchor });
    }

    /// The ids on `row`.
    fn ids_on(&self, row: u32) -> Vec<u64> {
        (self.placed.iter())
            .filter(|placed| placed.row == row)
            .map(|placed| placed.id)
            .collect()
    }

    /// The rows read back after the edits to `seen`, by line diff if
    /// `replaced`, else from the anchors.
    fn follow(&mut self, seen: &Seen, replaced: bool) {
        let snapshot = &seen.snapshot;
        let text = snapshot.text();
        if replaced {
            let rows: Vec<u32> = self.placed.iter().map(|placed| placed.row).collect();
            let rows = map_rows(self.snapshot.text(), text, &rows);
            for (placed, row) in self.placed.iter_mut().zip(rows) {
                placed.row = row;
                placed.anchor = row_anchor(snapshot, row);
            }
        } else {
            let floor = snapshot.oldest_version();
            let last = text.row_count() - 1;
            for placed in &mut self.placed {
                if placed.anchor.version() >= floor {
                    placed.row = text.offset_to_point(snapshot.resolve(&placed.anchor)).row;
                    placed.anchor = snapshot.refresh(&placed.anchor);
                } else {
                    // Compacted away below it: by its number.
                    placed.row = placed.row.min(last);
                    placed.anchor = row_anchor(snapshot, placed.row);
                }
            }
        }
        // Stable: of two now on one row, the one higher up stays first.
        self.placed.sort_by_key(|placed| placed.row);
    }

    /// The rows at `version`, whose text is `text`: as seen then, or
    /// carried there by line diff from the last version seen before it.
    /// Only those of breakpoints placed by then.
    fn rows_at(&self, version: Version, text: &Text) -> Vec<(u64, u32)> {
        let seen = (self.kept.iter().rev())
            .filter(|kept| kept.version <= version)
            .find_map(|kept| Some((kept, kept.rows.as_ref()?)));
        let Some((kept, rows)) = seen else {
            return Vec::new();
        };
        if kept.version == version {
            return rows.clone();
        }
        let numbers: Vec<u32> = rows.iter().map(|(_, row)| *row).collect();
        let carried = map_rows(&kept.text, text, &numbers);
        let mut rows: Vec<(u64, u32)> = (rows.iter().map(|(id, _)| *id)).zip(carried).collect();
        rows.sort_by_key(|(_, row)| *row);
        rows
    }
}

/// Whether `range` of `text` holds a newline.
fn spans_rows(text: &Text, range: &std::ops::Range<usize>) -> bool {
    range.end > range.start
        && text.offset_to_point(range.start).row != text.offset_to_point(range.end).row
}

/// Whether an edit of `edits`, from `before` to `after`, replaced rows
/// with rows.
fn replaces_rows(before: &Text, after: &Text, edits: &[Edit]) -> bool {
    (edits.iter()).any(|edit| spans_rows(before, &edit.old) && spans_rows(after, &edit.new))
}

/// A file's breakpoints.
#[derive(Default)]
struct FileBreakpoints {
    /// By line on disk, then id. Two are on one line only while one of
    /// them is on text not saved yet.
    entries: Vec<Entry>,
    /// Its open buffers. The first's rows are the file's.
    open: Vec<Open>,
}

impl FileBreakpoints {
    fn entry_mut(&mut self, id: u64) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|entry| entry.id == id)
    }

    fn sort(&mut self) {
        self.entries.sort_by_key(|entry| (entry.disk, entry.id));
    }

    fn open_index(&self, buffer: Option<BufferId>) -> Option<usize> {
        match buffer {
            Some(buffer) => self.open.iter().position(|open| open.buffer == buffer),
            None => (!self.open.is_empty()).then_some(0),
        }
    }

    /// The breakpoints shown, as (row, entry), by row: those in the
    /// open buffer `at`, or on disk. One per row, the first.
    fn shown(&self, at: Option<usize>) -> Vec<(u32, &Entry)> {
        let mut shown: Vec<(u32, &Entry)> = match at.and_then(|at| self.open.get(at)) {
            Some(open) => (open.placed.iter())
                .filter_map(|placed| {
                    let entry = self.entries.iter().find(|entry| entry.id == placed.id)?;
                    Some((placed.row, entry))
                })
                .collect(),
            None => (self.entries.iter())
                .map(|entry| (entry.disk, entry))
                .collect(),
        };
        shown.dedup_by_key(|(row, _)| *row);
        shown
    }

    /// The rows shown in the first open buffer, or on disk, with whether
    /// each is enabled.
    fn rows(&self) -> Vec<(u32, bool)> {
        (self.shown(self.open_index(None)).into_iter())
            .map(|(row, entry)| (row, entry.enabled))
            .collect()
    }

    /// The ids on `row`: in the open buffer `at`, or on disk.
    fn ids_on(&self, at: Option<usize>, row: u32) -> Vec<u64> {
        match at.and_then(|at| self.open.get(at)) {
            Some(open) => open.ids_on(row),
            None => (self.entries.iter())
                .filter(|entry| entry.disk == row)
                .map(|entry| entry.id)
                .collect(),
        }
    }

    fn remove_ids(&mut self, ids: &[u64]) {
        self.entries.retain(|entry| !ids.contains(&entry.id));
        for open in &mut self.open {
            open.placed.retain(|placed| !ids.contains(&placed.id));
        }
    }

    /// With no buffer open, keeps the first of any on one line.
    fn merge_on_disk(&mut self) {
        if self.open.is_empty() {
            self.entries.dedup_by_key(|entry| entry.disk);
        }
    }
}

/// A right-biased anchor at the start of `row` (clamped to the last).
fn row_anchor(snapshot: &BufferSnapshot, row: u32) -> Anchor {
    snapshot.anchor_at(snapshot.text().row_start(row), Bias::Right)
}

/// An editor handed to the store.
struct Watched {
    /// Its path as its model gave it, and that resolved; `None` while
    /// untitled.
    path: Option<(PathBuf, PathBuf)>,
    buffer: BufferId,
    _subscriptions: [Subscription; 2],
}

/// The writes of a state file, in order: each has a number, and one
/// older than the last written is dropped.
#[derive(Default)]
struct Writes {
    next: AtomicU64,
    written: Mutex<u64>,
}

impl Writes {
    fn number(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Writes `bytes` to `path` as write `number`, unless a later one is
    /// already written.
    fn write(&self, number: u64, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut written = self.written.lock().unwrap_or_else(|e| e.into_inner());
        if *written > number {
            return Ok(());
        }
        write_atomically(path, bytes)?;
        *written = number;
        Ok(())
    }
}

/// Writes `path` through a temporary file beside it, synced and then
/// renamed over it, making its folder first.
fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(verbatim!("."));
    name.push(std::process::id().to_string());
    name.push(verbatim!(".tmp"));
    let temp = dir.join(name);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// A project's breakpoints: see the [module](self).
pub struct BreakpointStore {
    /// Canonical.
    root: PathBuf,
    /// The state file, if there's a state directory.
    file: Option<PathBuf>,
    /// By canonical path.
    files: BTreeMap<PathBuf, FileBreakpoints>,
    next_id: u64,
    /// The state file's `watches`, as read.
    watches: Option<Value>,
    /// The state file's other keys, as read.
    other: Map<String, Value>,
    editors: HashMap<EntityId, Watched>,
    /// Changes to what the state file holds, counted.
    changes: u64,
    /// The count of changes the state file holds, as far as known.
    written: u64,
    /// When the set last changed, so a write waits out the rest after it.
    changed_at: Instant,
    writer: Option<Task<()>>,
    writes: Arc<Writes>,
    /// Holds the next write until sent.
    #[cfg(test)]
    write_gate: Option<futures::channel::oneshot::Receiver<()>>,
    _quit: Subscription,
}

impl EventEmitter<BreakpointEvent> for BreakpointStore {}

impl BreakpointStore {
    /// The store of the project at `root`, shared by every window on it,
    /// reading its state file the first time.
    pub fn for_root(root: &Path, cx: &mut App) -> Entity<BreakpointStore> {
        let root = canonical(root);
        if let Some(store) = (cx.try_global::<Stores>()).and_then(|stores| stores.0.get(&root)) {
            return store.clone();
        }
        let store = cx.new(|cx| BreakpointStore::load(root.clone(), cx));
        cx.default_global::<Stores>().0.insert(root, store.clone());
        store
    }

    fn load(root: PathBuf, cx: &mut Context<Self>) -> Self {
        let file = state_dir(cx).map(|dir| state_file(&dir, &root));
        let quit = cx.on_app_quit(|store, _| store.write_at_quit());
        let mut store = BreakpointStore {
            root,
            file,
            files: BTreeMap::new(),
            next_id: 0,
            watches: None,
            other: Map::new(),
            editors: HashMap::new(),
            changes: 0,
            written: 0,
            changed_at: cx.background_executor().now(),
            writer: None,
            writes: Arc::default(),
            #[cfg(test)]
            write_gate: None,
            _quit: quit,
        };
        // One small file, read once per root before any editor opens a
        // file in it, so nothing done meanwhile has to be merged with it.
        if let Some(path) = store.file.clone() {
            let read = match fs::read(&path) {
                Ok(bytes) => store.read(&bytes),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.to_string()),
            };
            if let Err(error) = read {
                let file = path.display().to_string();
                let message = tr!(
                    "workbench-debug-breakpoints-unreadable",
                    file = file,
                    error = error
                );
                Workbench::notify(cx, log::Level::Warn, message.to_string(), None);
            }
        }
        store
    }

    /// Takes in a state file's contents.
    fn read(&mut self, bytes: &[u8]) -> Result<(), String> {
        let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        let Value::Object(mut map) = value else {
            return Err(tr!("workbench-debug-breakpoints-not-an-object").to_string());
        };
        let breakpoints = map.remove("breakpoints");
        self.watches = map.remove("watches");
        self.other = map;
        let Some(Value::Array(breakpoints)) = breakpoints else {
            return Ok(());
        };
        for value in breakpoints {
            let breakpoint = match serde_json::from_value::<Breakpoint>(value) {
                Ok(breakpoint) if breakpoint.line > 0 => breakpoint,
                Ok(_) => {
                    log::warn!("breakpoints: a breakpoint on line 0, dropped");
                    continue;
                }
                Err(error) => {
                    log::warn!("breakpoints: an unreadable breakpoint, dropped: {error}");
                    continue;
                }
            };
            let path = canonical(&self.root.join(&breakpoint.path));
            let id = self.new_id();
            let file = self.files.entry(path).or_default();
            let entry = Entry::from_breakpoint(id, &breakpoint);
            if !file.entries.iter().any(|other| other.disk == entry.disk) {
                file.entries.push(entry);
            }
        }
        for file in self.files.values_mut() {
            file.sort();
        }
        Ok(())
    }

    fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// The project's root, canonical.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The store's key for `path`: absolute (relative to the root if
    /// it isn't) and resolved.
    fn key(&self, path: &Path) -> PathBuf {
        if self.files.contains_key(path) {
            return path.to_owned();
        }
        canonical(&self.root.join(path))
    }

    /// The breakpoints of the file at `path`, by line: as of the last
    /// version seen of its first open buffer, or on disk if it isn't open.
    pub fn breakpoints(&self, path: &Path) -> Vec<Breakpoint> {
        self.shown(path, None)
    }

    /// The breakpoints of the file at `path` in the open buffer `buffer`
    /// (an editor's), by line, or on disk if that isn't open.
    pub fn breakpoints_in(&self, path: &Path, buffer: BufferId) -> Vec<Breakpoint> {
        self.shown(path, Some(buffer))
    }

    fn shown(&self, path: &Path, buffer: Option<BufferId>) -> Vec<Breakpoint> {
        let path = self.key(path);
        let Some(file) = self.files.get(&path) else {
            return Vec::new();
        };
        (file.shown(file.open_index(buffer)).into_iter())
            .map(|(row, entry)| entry.breakpoint(path.clone(), row))
            .collect()
    }

    /// The breakpoints of the file at `path` in the open buffer `buffer`
    /// (or on disk if that isn't open), as [`BreakpointStore::breakpoints_in`]
    /// shows them, each with the line on disk a session sends it on: what
    /// the session's verification of it is keyed by.
    pub fn placements_in(&self, path: &Path, buffer: BufferId) -> Vec<Placement> {
        let path = self.key(path);
        let Some(file) = self.files.get(&path) else {
            return Vec::new();
        };
        (file.shown(file.open_index(Some(buffer))).into_iter())
            .map(|(row, entry)| Placement {
                row,
                line: entry.disk + 1,
                enabled: entry.enabled,
            })
            .collect()
    }

    /// Where `lines` (from 1) of the file at `path` on disk are in the open
    /// buffer `buffer`, as rows from 0, as the store places breakpoints:
    /// by the buffer's text against the file's on disk. Not open there,
    /// the lines' own rows.
    pub fn rows_of_lines(&self, path: &Path, buffer: BufferId, lines: &[u32]) -> Vec<u32> {
        let disk: Vec<u32> = lines.iter().map(|line| line.saturating_sub(1)).collect();
        let path = self.key(path);
        let open = (self.files.get(&path))
            .and_then(|file| Some(&file.open[file.open_index(Some(buffer))?]));
        match open {
            Some(open) => open.rows_of_disk(&disk),
            None => disk,
        }
    }

    /// The breakpoints of the file at `path` by their lines on disk, as
    /// last saved, whatever buffers of it hold unsaved.
    pub fn on_disk(&self, path: &Path) -> Vec<Breakpoint> {
        let path = self.key(path);
        let Some(file) = self.files.get(&path) else {
            return Vec::new();
        };
        (file.shown(None).into_iter())
            .map(|(row, entry)| entry.breakpoint(path.clone(), row))
            .collect()
    }

    /// The enabled breakpoints of the file at `path`, by line on disk:
    /// the set a session sends for it in `setBreakpoints`, which the
    /// running program's lines match.
    pub fn enabled(&self, path: &Path) -> Vec<Breakpoint> {
        let mut breakpoints = self.on_disk(path);
        breakpoints.retain(|breakpoint| breakpoint.enabled);
        breakpoints
    }

    /// Every breakpoint, by file and line on disk.
    pub fn all(&self) -> Vec<Breakpoint> {
        (self.files.keys())
            .flat_map(|path| self.on_disk(path))
            .collect()
    }

    /// The files with breakpoints, in order.
    pub fn files(&self) -> Vec<PathBuf> {
        (self.files.iter())
            .filter(|(_, file)| !file.entries.is_empty())
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// The state file's `watches`, as read and written back.
    pub fn watches(&self) -> Option<&Value> {
        self.watches.as_ref()
    }

    /// Adds a breakpoint on `row` (from 0) of the file at `path`, or
    /// removes the one there, in its first open buffer or on disk.
    /// Returns whether one was added. While the file is open, a row past
    /// its end is its last.
    pub fn toggle(&mut self, path: &Path, row: u32, cx: &mut Context<Self>) -> bool {
        self.toggle_at(path, None, row, cx)
    }

    /// [`BreakpointStore::toggle`] on `row` of the open buffer `buffer`.
    pub fn toggle_in(
        &mut self,
        path: &Path,
        buffer: BufferId,
        row: u32,
        cx: &mut Context<Self>,
    ) -> bool {
        self.toggle_at(path, Some(buffer), row, cx)
    }

    fn toggle_at(
        &mut self,
        path: &Path,
        buffer: Option<BufferId>,
        row: u32,
        cx: &mut Context<Self>,
    ) -> bool {
        let path = self.key(path);
        let id = self.next_id + 1;
        let file = self.files.entry(path.clone()).or_default();
        let at = file.open_index(buffer);
        let row = match at {
            Some(at) => row.min(file.open[at].last_row()),
            None => row,
        };
        let there = file.ids_on(at, row);
        let added = there.is_empty();
        if added {
            let disk = match at {
                Some(at) => file.open[at].disk_of_rows(&[row])[0],
                None => row,
            };
            file.entries
                .push(Entry::from_breakpoint(id, &Breakpoint::new("", disk + 1)));
            file.sort();
            for (ix, open) in file.open.iter_mut().enumerate() {
                let placed = match Some(ix) == at {
                    true => row,
                    false => open.rows_of_disk(&[disk])[0],
                };
                open.place(id, placed);
                open.note_rows();
            }
            self.next_id = id;
        } else {
            file.remove_ids(&there);
        }
        self.changed(path, cx);
        added
    }

    /// Enables or disables the breakpoint on `row` (from 0) of the file at
    /// `path`, in its first open buffer or on disk. Returns whether
    /// there's one there that wasn't so already.
    pub fn set_enabled(
        &mut self,
        path: &Path,
        row: u32,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        self.set_enabled_at(path, None, row, enabled, cx)
    }

    /// [`BreakpointStore::set_enabled`] on `row` of the open buffer
    /// `buffer`.
    pub fn set_enabled_in(
        &mut self,
        path: &Path,
        buffer: BufferId,
        row: u32,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        self.set_enabled_at(path, Some(buffer), row, enabled, cx)
    }

    fn set_enabled_at(
        &mut self,
        path: &Path,
        buffer: Option<BufferId>,
        row: u32,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let path = self.key(path);
        let Some(file) = self.files.get_mut(&path) else {
            return false;
        };
        let at = file.open_index(buffer);
        let mut changed = false;
        // Every one shown as this one, so they agree if they part again.
        for id in file.ids_on(at, row) {
            if let Some(entry) = file.entry_mut(id)
                && entry.enabled != enabled
            {
                entry.enabled = enabled;
                changed = true;
            }
        }
        if changed {
            self.changed(path, cx);
        }
        changed
    }

    /// Removes the breakpoint on `row` (from 0) of the file at `path`, in
    /// its first open buffer or on disk. Returns whether there was one.
    pub fn remove(&mut self, path: &Path, row: u32, cx: &mut Context<Self>) -> bool {
        self.remove_at(path, None, row, cx)
    }

    /// [`BreakpointStore::remove`] on `row` of the open buffer `buffer`.
    pub fn remove_in(
        &mut self,
        path: &Path,
        buffer: BufferId,
        row: u32,
        cx: &mut Context<Self>,
    ) -> bool {
        self.remove_at(path, Some(buffer), row, cx)
    }

    fn remove_at(
        &mut self,
        path: &Path,
        buffer: Option<BufferId>,
        row: u32,
        cx: &mut Context<Self>,
    ) -> bool {
        let path = self.key(path);
        let Some(file) = self.files.get_mut(&path) else {
            return false;
        };
        let ids = file.ids_on(file.open_index(buffer), row);
        if ids.is_empty() {
            return false;
        }
        file.remove_ids(&ids);
        self.changed(path, cx);
        true
    }

    /// Removes every breakpoint.
    pub fn remove_all(&mut self, cx: &mut Context<Self>) {
        let paths = self.files();
        for path in &paths {
            if let Some(file) = self.files.get_mut(path) {
                file.entries.clear();
                for open in &mut file.open {
                    open.placed.clear();
                }
            }
        }
        for path in paths {
            self.changed(path, cx);
        }
    }

    /// The set of the file at `path` changed through the store.
    fn changed(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.forget_unused(&path);
        self.schedule_write(cx);
        cx.emit(BreakpointEvent::Changed(path));
        cx.notify();
    }

    /// Drops what the store keeps of a file with no breakpoints and no
    /// buffer open.
    fn forget_unused(&mut self, path: &Path) {
        if (self.files.get(path))
            .is_some_and(|file| file.entries.is_empty() && file.open.is_empty())
        {
            self.files.remove(path);
        }
    }

    /// Has the state file written once changes rest.
    fn schedule_write(&mut self, cx: &mut Context<Self>) {
        self.changes += 1;
        self.changed_at = cx.background_executor().now();
        if self.writer.is_some() {
            return;
        }
        self.writer = Some(cx.spawn(async move |store, cx| {
            loop {
                let Ok(due) = store.read_with(cx, |store, _| store.changed_at + WRITE_REST) else {
                    return;
                };
                let now = cx.background_executor().now();
                if due > now {
                    cx.background_executor().timer(due - now).await;
                    continue;
                }
                let Ok((changes, write)) =
                    store.update(cx, |store, _| (store.changes, store.take_write()))
                else {
                    return;
                };
                let result = match write {
                    Some(write) => cx.background_spawn(write).await,
                    None => Ok(()),
                };
                let written = result.is_ok();
                if let Err(error) = result {
                    let error = error.to_string();
                    cx.update(|cx| {
                        let message = tr!("workbench-debug-breakpoints-save-failed", error = error);
                        Workbench::notify(cx, log::Level::Error, message.to_string(), None);
                    });
                }
                // Failed: tried again for changes made since the one tried,
                // else by the next change, or at quit.
                let again = store.update(cx, |store, _| {
                    if written {
                        store.written = store.written.max(changes);
                    }
                    let again =
                        store.written < store.changes && (written || store.changes > changes);
                    if !again {
                        store.writer = None;
                    }
                    again
                });
                if !matches!(again, Ok(true)) {
                    return;
                }
            }
        }));
    }

    /// Holds the next write of the state file until the sender sends, or
    /// goes.
    #[cfg(test)]
    pub(crate) fn hold_next_write(&mut self) -> futures::channel::oneshot::Sender<()> {
        let (send, receive) = futures::channel::oneshot::channel();
        self.write_gate = Some(receive);
        send
    }

    /// The write of the state file as it is now, if there's a file.
    fn take_write(&mut self) -> Option<impl Future<Output = io::Result<()>> + use<>> {
        let path = self.file.clone()?;
        let bytes = self.contents();
        let (writes, number) = (self.writes.clone(), self.writes.number());
        #[cfg(test)]
        let gate = self.write_gate.take();
        Some(async move {
            #[cfg(test)]
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            writes.write(number, &path, &bytes)
        })
    }

    /// At quit: writes what isn't known written, a write still running or
    /// one that failed included. Numbered after any running, so it lands
    /// last.
    fn write_at_quit(&mut self) -> impl Future<Output = ()> + use<> {
        let write = (self.written < self.changes)
            .then(|| self.take_write())
            .flatten();
        async move {
            if let Some(write) = write
                && let Err(error) = write.await
            {
                log::error!("breakpoints: can't save at quit: {error}");
            }
        }
    }

    /// The state file's contents: the lines on disk.
    fn contents(&self) -> Vec<u8> {
        let mut map = self.other.clone();
        let mut breakpoints = Vec::new();
        for (path, file) in &self.files {
            let shown = path.strip_prefix(&self.root).unwrap_or(path).to_owned();
            for (row, entry) in file.shown(None) {
                match serde_json::to_value(entry.breakpoint(shown.clone(), row)) {
                    Ok(value) => breakpoints.push(value),
                    // A path JSON can't hold: not UTF-8.
                    Err(error) => log::warn!("breakpoints: {} not kept: {error}", path.display()),
                }
            }
        }
        map.insert("breakpoints".into(), Value::Array(breakpoints));
        let watches = (self.watches.clone()).unwrap_or_else(|| Value::Array(Vec::new()));
        map.insert("watches".into(), watches);
        let mut bytes = serde_json::to_vec_pretty(&Value::Object(map)).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }

    /// Follows `editor`'s file from now on, placing its breakpoints in
    /// its buffer if it's the first editor of that buffer.
    pub(crate) fn attach(&mut self, editor: &Entity<EditorView>, cx: &mut Context<Self>) {
        let id = editor.entity_id();
        if self.editors.contains_key(&id) {
            return;
        }
        // Subscribed as seen, so its events follow from the version seen.
        let (raw, seen, events) = editor.update(cx, |view, _| {
            let events = view.model_mut().buffer_mut().subscribe();
            (
                view.model().path().map(Path::to_owned),
                Seen::of(view),
                events,
            )
        });
        let subscriptions = [
            cx.observe(editor, |store, editor, cx| {
                store.editor_changed(&editor, cx)
            }),
            cx.observe_release(editor, move |store, view, cx| {
                store.detach_seen(id, Some(Seen::of(view)), true, cx)
            }),
        ];
        let path = raw.map(|raw| {
            let path = self.key(&raw);
            (raw, path)
        });
        self.editors.insert(
            id,
            Watched {
                path: path.clone(),
                buffer: seen.buffer,
                _subscriptions: subscriptions,
            },
        );
        if let Some((_, path)) = path {
            self.open(path, seen, events, cx);
        }
    }

    /// Stops following `editor`'s file.
    pub(crate) fn detach(&mut self, editor: &Entity<EditorView>, cx: &mut Context<Self>) {
        let seen = editor.read_with(cx, |view, _| Seen::of(view));
        self.detach_seen(editor.entity_id(), Some(seen), true, cx);
    }

    /// Stops following the editor `id`, taking in what `seen` saw of its
    /// buffer first if it's the one it had (saves only if `saves`).
    fn detach_seen(
        &mut self,
        id: EntityId,
        seen: Option<Seen>,
        saves: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(watched) = self.editors.remove(&id) else {
            return;
        };
        if let Some((_, path)) = watched.path {
            if let Some(seen) = seen.filter(|seen| seen.buffer == watched.buffer) {
                self.sync(&path, seen, saves, cx);
            }
            self.close(&path, watched.buffer, cx);
        }
    }

    /// `editor` notified: it may have edited, saved or been saved under a
    /// new path.
    fn editor_changed(&mut self, editor: &Entity<EditorView>, cx: &mut Context<Self>) {
        let id = editor.entity_id();
        let Some(watched) = self.editors.get(&id) else {
            return;
        };
        let (raw, seen) = editor.read_with(cx, |view, _| {
            (view.model().path().map(Path::to_owned), Seen::of(view))
        });
        let same_path = match (&watched.path, &raw) {
            (Some((was, _)), Some(raw)) => was == raw,
            (None, None) => true,
            _ => false,
        };
        if same_path && watched.buffer == seen.buffer {
            if let Some((_, path)) = watched.path.clone() {
                self.sync(&path, seen, true, cx);
            }
            return;
        }
        // Saved under a new path, or given another buffer: off the old
        // file, onto the new. A save under a new path isn't one of the
        // old file, whose lines on disk stay as they are.
        self.detach_seen(id, Some(seen), same_path, cx);
        self.attach(editor, cx);
    }

    /// Places the breakpoints of the file at `path` in `seen`'s buffer,
    /// unless they're placed there already.
    fn open(&mut self, path: PathBuf, seen: Seen, events: BufferEvents, cx: &mut Context<Self>) {
        let file = self.files.entry(path.clone()).or_default();
        if file.open.iter().any(|open| open.buffer == seen.buffer) {
            self.sync(&path, seen, true, cx);
            return;
        }
        let before = file.rows();
        // Text not on disk (a restored backup): the file's, to place the
        // lines on disk in it.
        let disk = match seen.dirty {
            true => read_disk(&path),
            false => Some(seen.snapshot.text().clone()),
        };
        let open = Open::new(seen, events, disk, &file.entries);
        file.open.push(open);
        if file.rows() != before {
            self.moved(path, cx);
        }
    }

    /// The last editor of `buffer` on `path` went: its rows go with it,
    /// the lines on disk staying as they are.
    fn close(&mut self, path: &Path, buffer: BufferId, cx: &mut Context<Self>) {
        let others = (self.editors.values()).any(|watched| {
            watched.buffer == buffer && watched.path.as_ref().is_some_and(|(_, at)| at == path)
        });
        let Some(file) = self.files.get_mut(path) else {
            return;
        };
        let Some(at) = file.open.iter().position(|open| open.buffer == buffer) else {
            return;
        };
        if others {
            return;
        }
        let before = file.rows();
        file.open.remove(at);
        file.merge_on_disk();
        let moved = file.rows() != before;
        self.forget_unused(path);
        if moved {
            cx.emit(BreakpointEvent::Moved(path.to_owned()));
            cx.notify();
        }
    }

    /// Takes in the edits and saves of `seen`'s buffer of the file at
    /// `path` since last seen (its saves only if `saves`).
    fn sync(&mut self, path: &Path, seen: Seen, saves: bool, cx: &mut Context<Self>) {
        let Some(file) = self.files.get_mut(path) else {
            return;
        };
        let Some(at) = file.open.iter().position(|open| open.buffer == seen.buffer) else {
            return;
        };
        let events = file.open[at].events.drain();
        let open = &file.open[at];
        if events.is_empty()
            && open.snapshot.version() == seen.snapshot.version()
            && open.dirty == seen.dirty
        {
            return;
        }
        let shown = file.rows();
        let open = &mut file.open[at];
        let mut replaced = false;
        let mut saved = None;
        let mut before = open.snapshot.text().clone();
        for event in events {
            match event {
                BufferEvent::Edited {
                    to, edits, text, ..
                } => {
                    replaced = replaced || replaces_rows(&before, &text, &edits);
                    open.keep(Kept {
                        version: to,
                        text: text.clone(),
                        rows: None,
                    });
                    before = text;
                }
                BufferEvent::Saved { version } if saves => saved = Some(version),
                BufferEvent::Saved { .. } => {}
            }
        }
        let rows_before: Vec<(u64, u32)> = (open.placed.iter())
            .map(|placed| (placed.id, placed.row))
            .collect();
        if open.snapshot.version() != seen.snapshot.version() {
            open.follow(&seen, replaced);
        }
        open.snapshot = seen.snapshot;
        open.dirty = seen.dirty;
        open.note_rows();
        let rows_after: Vec<(u64, u32)> = (open.placed.iter())
            .map(|placed| (placed.id, placed.row))
            .collect();
        let mut on_disk = false;
        if let Some(version) = saved {
            on_disk = self.save(path, at, version);
        }
        let Some(file) = self.files.get(path) else {
            return;
        };
        if rows_after != rows_before || file.rows() != shown {
            self.moved(path.to_owned(), cx);
        }
        if on_disk {
            self.schedule_write(cx);
            cx.emit(BreakpointEvent::Saved(path.to_owned()));
        }
    }

    /// The open buffer `at` of the file at `path` saved (or reloaded)
    /// `version`: the rows of that version are the lines on disk, in this
    /// buffer and every other of the file. Returns whether any line on
    /// disk changed.
    fn save(&mut self, path: &Path, at: usize, version: Version) -> bool {
        let Some(file) = self.files.get_mut(path) else {
            return false;
        };
        let open = &mut file.open[at];
        let now = open.rows();
        let current = open.snapshot.version() == version;
        let (text, then) = match current {
            true => (open.snapshot.text().clone(), now.clone()),
            false => {
                let text = (open.kept.iter())
                    .find(|kept| kept.version == version)
                    .map(|kept| kept.text.clone())
                    .or_else(|| read_disk(path));
                match text {
                    Some(text) => {
                        let then = open.rows_at(version, &text);
                        (text, then)
                    }
                    // Not known: as the text now.
                    None => (open.snapshot.text().clone(), now.clone()),
                }
            }
        };
        // Only those still there: one removed since doesn't merge another.
        let then: Vec<(u64, u32)> = (then.into_iter())
            .filter(|(id, _)| now.iter().any(|(is, _)| is == id))
            .collect();
        // Set since that version: from where they are now.
        let later: Vec<(u64, u32)> = (now.iter())
            .filter(|(id, _)| !then.iter().any(|(was, _)| was == id))
            .copied()
            .collect();
        let numbers: Vec<u32> = later.iter().map(|(_, row)| *row).collect();
        let carried = match current {
            true => numbers,
            false => map_rows(open.snapshot.text(), &text, &numbers),
        };
        // On one row of the text saved: one now, the first shown then.
        let mut merged = Vec::new();
        for pair in then.windows(2) {
            if pair[0].1 == pair[1].1 {
                merged.push(pair[1].0);
            }
        }
        let lines: Vec<(u64, u32)> = (then.into_iter())
            .chain(later.iter().map(|(id, _)| *id).zip(carried))
            .collect();
        open.kept.retain(|kept| kept.version >= version);
        if current {
            open.dirty = false;
        }
        // The file on disk is this text now, for every buffer of it.
        for (ix, other) in file.open.iter_mut().enumerate() {
            other.disk = Some(text.clone());
            other.own_disk = ix == at;
        }
        let mut changed = !merged.is_empty();
        file.remove_ids(&merged);
        for (id, line) in lines {
            if let Some(entry) = file.entry_mut(id)
                && entry.disk != line
            {
                entry.disk = line;
                changed = true;
            }
        }
        file.sort();
        changed
    }

    /// Edits moved the breakpoints of the file at `path` in a buffer.
    fn moved(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.emit(BreakpointEvent::Moved(path));
        cx.notify();
    }
}

/// Where `rows` of `old` are in `new`, by a line diff: a row in text both
/// keep follows it, one in text `new` replaced keeps its place in the
/// replacement (its last row, if the replacement is shorter), and one in
/// text `new` deleted goes to the row after, which took its place.
fn map_rows(old: &Text, new: &Text, rows: &[u32]) -> Vec<u32> {
    let (old_text, new_text) = (old.to_string(), new.to_string());
    let input = InternedInput::new(old_text.as_str(), new_text.as_str());
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let hunks: Vec<_> = diff.hunks().collect();
    let last = new.row_count() - 1;
    (rows.iter())
        .map(|&row| {
            let mut shift = 0i64;
            for hunk in &hunks {
                if row < hunk.before.start {
                    break;
                }
                if row < hunk.before.end {
                    let into = row - hunk.before.start;
                    let len = hunk.after.end - hunk.after.start;
                    return (hunk.after.start + into.min(len.saturating_sub(1))).min(last);
                }
                shift = i64::from(hunk.after.end) - i64::from(hunk.before.end);
            }
            (i64::from(row) + shift).clamp(0, i64::from(last)) as u32
        })
        .collect()
}

impl Workbench {
    /// The breakpoint store of the workbench's root, if it has one.
    pub fn breakpoint_store(&self, cx: &mut App) -> Option<Entity<BreakpointStore>> {
        let root = self.root()?.to_owned();
        Some(BreakpointStore::for_root(&root, cx))
    }
}

/// A window's part: the root it handed editors to the store of, and the
/// editors it handed.
#[derive(Default)]
struct Handed {
    root: Option<PathBuf>,
    store: Option<WeakEntity<BreakpointStore>>,
    editors: HashMap<EntityId, WeakEntity<EditorView>>,
}

/// Hands each window's file editors to its root's store as they open,
/// and takes them back as they close or the root changes.
pub(crate) fn init(cx: &mut App) {
    cx.observe_new(|workbench: &mut Workbench, window, cx| {
        if window.is_none() {
            return;
        }
        let handed = Rc::new(RefCell::new(Handed::default()));
        cx.observe_self({
            let handed = handed.clone();
            move |workbench, cx| hand(workbench, &handed, cx)
        })
        .detach();
        hand(workbench, &handed, cx);
    })
    .detach();
}

fn hand(workbench: &mut Workbench, handed: &Rc<RefCell<Handed>>, cx: &mut Context<Workbench>) {
    let Ok(mut handed) = handed.try_borrow_mut() else {
        return;
    };
    let root = workbench.root().map(Path::to_owned);
    if root != handed.root {
        if let Some(store) = handed.store.take().and_then(|store| store.upgrade()) {
            let editors = std::mem::take(&mut handed.editors);
            for editor in editors.values().filter_map(WeakEntity::upgrade) {
                store.update(cx, |store, cx| store.detach(&editor, cx));
            }
        }
        handed.store = root
            .as_deref()
            .map(|root| BreakpointStore::for_root(root, cx).downgrade());
        handed.root = root;
    }
    let Some(store) = handed.store.as_ref().and_then(WeakEntity::upgrade) else {
        return;
    };
    let editors = workbench.file_editors(cx);
    let gone: Vec<EntityId> = (handed.editors.keys())
        .filter(|id| !editors.iter().any(|editor| editor.entity_id() == **id))
        .copied()
        .collect();
    for id in gone {
        if let Some(editor) = handed
            .editors
            .remove(&id)
            .and_then(|editor| editor.upgrade())
        {
            store.update(cx, |store, cx| store.detach(&editor, cx));
        }
    }
    for editor in editors {
        if handed.editors.contains_key(&editor.entity_id()) {
            continue;
        }
        handed
            .editors
            .insert(editor.entity_id(), editor.downgrade());
        store.update(cx, |store, cx| store.attach(&editor, cx));
    }
}
