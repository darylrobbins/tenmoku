//! Decorations and gutter entries plugins draw on buffers, kept by the host.
//!
//! A plugin with the `decorations` capability sets named sets of decorations and of gutter
//! entries on a buffer, against a version of it. The [`DecorationStore`] keeps them, keyed by
//! plugin and buffer, anchored so they follow later edits; a set made against an older version
//! is first mapped through the edits since, and whatever of it those edits deleted is dropped.
//! The editor reads the store's copy through a [`PluginDecorations`], its
//! [`DecorationProvider`], and never calls the plugin: a plugin stuck in a call, or gone, can't
//! hold up a frame. Unloading a plugin clears its sets.
//!
//! The editor has no seam for gutter-column entries (its gutter marks are bars, with no symbol
//! or tooltip), so the store hands them out with [`DecorationStore::gutter_entries`] and painting
//! them is the app's. Nor has it one for struck-through text, which the provider leaves out;
//! [`DecorationStore::decorations`] returns every decoration as the plugin set it, hover text
//! included.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use fe_buffer::{Bias, BufferId, BufferSnapshot, Text, Version};
use fe_editor::decorations::{
    Decoration as Painted, DecorationKind, DecorationProvider, DecorationSet, Tint,
};
use kiln_command::capability::{Capability, Grants};
use kiln_plugin::bindings::{Error, unimplemented};

use crate::HostState;
use crate::fastedit::editor::decorations::{
    self as wit, Decoration, DecorationStyle, GutterEntry, Host,
};

/// How many decorations and gutter entries one plugin may keep on one buffer, over all its sets.
/// A set that would take it past this is refused.
pub const MAX_ENTRIES_PER_BUFFER: usize = 10_000;

/// Where a range's ends go when text is inserted right at them: inside neither, as the editor's
/// own decorations do.
const START: Bias = Bias::Right;
const END: Bias = Bias::Left;

/// The app's buffers, as plugins' decorations name them: by the ids and versions plugins see.
pub trait BufferVersions: Send {
    /// `buffer` as it is now; none when no open buffer has that id.
    fn now(&mut self, buffer: u64) -> Option<BufferSnapshot>;

    /// `buffer` as plugins were shown it at `version`; none if they weren't, or the app no
    /// longer holds that snapshot. A set against a version this returns none for is refused,
    /// and the plugin sets it again against the version now.
    fn at(&mut self, buffer: u64, version: u64) -> Option<BufferSnapshot>;
}

/// Buffers the app also holds.
impl<T: BufferVersions> BufferVersions for Arc<Mutex<T>> {
    fn now(&mut self, buffer: u64) -> Option<BufferSnapshot> {
        lock(self).now(buffer)
    }

    fn at(&mut self, buffer: u64, version: u64) -> Option<BufferSnapshot> {
        lock(self).at(buffer, version)
    }
}

/// A plugin's decoration as it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginDecoration {
    pub plugin: String,
    pub set: String,
    /// Bytes of the snapshot it was read in.
    pub range: Range<usize>,
    pub style: DecorationStyle,
    pub color: Option<String>,
    pub hover: Option<String>,
}

/// A plugin's gutter entry as it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginGutterEntry {
    pub plugin: String,
    pub set: String,
    /// Zero-based, in the snapshot it was read in.
    pub line: u32,
    pub symbol: String,
    pub color: Option<String>,
    pub tooltip: Option<String>,
}

/// Every plugin's decorations and gutter entries, which the app shares with every plugin's
/// decorations slice and with the editor. Clones share it.
#[derive(Clone, Default)]
pub struct DecorationStore {
    shared: Arc<Mutex<Shared>>,
}

#[derive(Default)]
struct Shared {
    /// Each loaded plugin's attachment: the number of its load.
    attached: BTreeMap<String, u64>,
    loads: u64,
    /// By buffer, then plugin.
    buffers: BTreeMap<u64, BTreeMap<String, Sets>>,
}

/// One plugin's sets on one buffer, by name.
#[derive(Default)]
struct Sets {
    decorations: BTreeMap<String, Drawn<Decoration>>,
    gutter: BTreeMap<String, Drawn<GutterEntry>>,
}

impl Sets {
    fn len(&self) -> usize {
        let decorations = self.decorations.values().map(|drawn| drawn.records.len());
        let gutter = self.gutter.values().map(|drawn| drawn.records.len());
        decorations.sum::<usize>() + gutter.sum::<usize>()
    }

    fn is_empty(&self) -> bool {
        self.decorations.is_empty() && self.gutter.is_empty()
    }

    /// Rebases every set onto `snapshot`, without what edits have taken since.
    fn refresh(&mut self, snapshot: &BufferSnapshot) {
        for drawn in self.decorations.values_mut() {
            drawn.refresh(snapshot);
        }
        for drawn in self.gutter.values_mut() {
            drawn.refresh(snapshot);
        }
        self.decorations
            .retain(|_, drawn| !drawn.records.is_empty());
        self.gutter.retain(|_, drawn| !drawn.records.is_empty());
    }
}

/// One set as the host keeps it. `anchored` holds only the ranges, which follow edits: each
/// entry's priority is the index of its record in `records`. Every anchor, there and in the
/// records' spans, is of `version`.
struct Drawn<T> {
    /// The buffer whose snapshots the anchors resolve in.
    buffer: BufferId,
    version: Version,
    anchored: DecorationSet,
    records: Vec<Record<T>>,
}

struct Record<T> {
    /// As the plugin set it; its range or line is as of the version it named.
    item: T,
    /// What edits must not take for it to stay.
    watch: Watch,
}

/// What an entry stays for, as of `version`: edits take it by deleting or replacing all of the
/// text it sits on (see [`neighbours`]), or, for an empty one, everything around the point it
/// sits at. It is rebased as the set is, so edits are only ever followed from `version`, which
/// the log is kept back to for as long as the set's anchors resolve.
#[derive(Clone)]
struct Watch {
    version: Version,
    kind: Kind,
}

#[derive(Clone)]
enum Kind {
    /// What is left of the text of a range, bytes of the text at `version`; taken when none is.
    Text(Vec<Range<usize>>),
    /// Where an empty entry sits (it moves with [`START`]) in a text of `len` bytes, and the
    /// text around it whose replacement takes it, none while it is at either end of the text.
    Point {
        offset: usize,
        len: usize,
        span: Option<Vec<Range<usize>>>,
    },
}

impl Watch {
    /// A watch for an entry over `range` of `text`, the text at `text_version`.
    fn new(text_version: Version, text: &Text, range: &Range<usize>) -> Self {
        let kind = if range.is_empty() {
            Kind::Point {
                offset: range.start,
                len: text.len(),
                span: neighbours(text, range.start),
            }
        } else {
            Kind::Text(vec![range.clone()])
        };
        Self {
            version: text_version,
            kind,
        }
    }

    /// The watch as of `snapshot`, or none if edits since have taken the entry. One older than
    /// the snapshot's edit log can't be told, and stays as it is.
    fn advance(&self, snapshot: &BufferSnapshot) -> Option<Self> {
        if self.version < snapshot.oldest_version() || self.version > snapshot.version() {
            return Some(self.clone());
        }
        let kind = match &self.kind {
            Kind::Text(spans) => {
                let spans = snapshot.carried(spans.clone(), self.version);
                if spans.is_empty() {
                    return None;
                }
                Kind::Text(spans)
            }
            Kind::Point { offset, len, span } => {
                let (offset, len, span) =
                    snapshot.followed((*offset, *len), START, span.clone(), self.version)?;
                Kind::Point { offset, len, span }
            }
        };
        Some(Self {
            version: snapshot.version(),
            kind,
        })
    }

    /// Whether edits since have taken the entry, in `snapshot`.
    fn is_taken(&self, snapshot: &BufferSnapshot) -> bool {
        self.advance(snapshot).is_none()
    }
}

impl<T> Record<T> {
    /// Whether edits have taken it, in `snapshot`.
    fn is_gone(&self, snapshot: &BufferSnapshot) -> bool {
        self.watch.is_taken(snapshot)
    }
}

/// An entry being kept: its range and its span, bytes of one snapshot.
struct Placed<T> {
    range: Range<usize>,
    watch: Watch,
    item: T,
}

impl<T> Drawn<T> {
    /// A set of `placed`, its ranges in `snapshot`'s text.
    fn new(snapshot: &BufferSnapshot, placed: Vec<Placed<T>>) -> Self {
        let mut entries = Vec::with_capacity(placed.len());
        let mut records = Vec::with_capacity(placed.len());
        for (index, placed) in placed.into_iter().enumerate() {
            let priority = i32::try_from(index).unwrap_or(i32::MAX);
            entries.push(
                Painted::new(placed.range, DecorationKind::Highlight, Tint::Accent)
                    .with_priority(priority),
            );
            records.push(Record {
                item: placed.item,
                watch: placed.watch,
            });
        }
        let mut anchored = DecorationSet::new();
        anchored.apply(snapshot, snapshot.version(), entries);
        Self {
            buffer: snapshot.id(),
            version: snapshot.version(),
            anchored,
            records,
        }
    }

    /// Whether the anchors resolve in `snapshot`: it's of the same buffer, at or after their
    /// version, and their version is above its edit-log floor.
    fn resolves_in(&self, snapshot: &BufferSnapshot) -> bool {
        self.buffer == snapshot.id()
            && self.version <= snapshot.version()
            && self.version >= snapshot.oldest_version()
    }

    /// The records whose ranges overlap `range` in `snapshot`, with their ranges there.
    fn visit(
        &self,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
        mut visit: impl FnMut(Range<usize>, &T),
    ) {
        if !self.resolves_in(snapshot) {
            return;
        }
        let mut found = Vec::new();
        self.anchored.decorations(snapshot, range, &mut found);
        for entry in found {
            let Some(record) = usize::try_from(entry.priority)
                .ok()
                .and_then(|index| self.records.get(index))
            else {
                continue;
            };
            if !record.is_gone(snapshot) {
                visit(entry.range, &record.item);
            }
        }
    }

    /// Rebases the set onto `snapshot`, dropping the records edits have taken, so they no
    /// longer count toward the cap. Does nothing with an older snapshot or another buffer's;
    /// drops every record if the log has been compacted past the anchors.
    fn refresh(&mut self, snapshot: &BufferSnapshot) {
        if self.buffer != snapshot.id() || self.version >= snapshot.version() {
            return;
        }
        if self.version < snapshot.oldest_version() {
            *self = Self::new(snapshot, Vec::new());
            return;
        }
        let mut found = Vec::new();
        self.anchored
            .decorations(snapshot, 0..snapshot.len(), &mut found);
        let mut records: Vec<Option<Record<T>>> = std::mem::take(&mut self.records)
            .into_iter()
            .map(Some)
            .collect();
        let placed = found.into_iter().filter_map(|entry| {
            let index = usize::try_from(entry.priority).ok()?;
            let record = records.get_mut(index)?.take()?;
            let watch = record.watch.advance(snapshot)?;
            Some(Placed {
                range: entry.range,
                watch,
                item: record.item,
            })
        });
        *self = Self::new(snapshot, placed.collect());
    }
}

impl DecorationStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The editor's provider for buffer `buffer`, which reads every plugin's decorations on it.
    pub fn provider(&self, buffer: u64) -> PluginDecorations {
        PluginDecorations {
            store: self.clone(),
            buffer,
        }
    }

    /// Every plugin's decorations on `buffer` that overlap `range` (bytes of `snapshot`, the
    /// buffer's), by plugin and set, an empty one at either end included.
    pub fn decorations(
        &self,
        buffer: u64,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
    ) -> Vec<PluginDecoration> {
        let mut decorations = Vec::new();
        self.visit_decorations(buffer, snapshot, range, |plugin, set, range, item| {
            decorations.push(PluginDecoration {
                plugin: plugin.to_owned(),
                set: set.to_owned(),
                range,
                style: item.style,
                color: item.color.clone(),
                hover: item.hover.clone(),
            });
        });
        decorations
    }

    /// Every plugin's gutter entries on `buffer` for the lines in `lines` of `snapshot`, the
    /// buffer's, by line.
    pub fn gutter_entries(
        &self,
        buffer: u64,
        snapshot: &BufferSnapshot,
        lines: Range<u32>,
    ) -> Vec<PluginGutterEntry> {
        let text = snapshot.text();
        if lines.start >= text.row_count() || lines.is_empty() {
            return Vec::new();
        }
        let start = text.row_start(lines.start);
        let end = if lines.end < text.row_count() {
            text.row_start(lines.end)
        } else {
            text.len()
        };
        let mut entries = Vec::new();
        let shared = self.lock();
        for (plugin, sets) in shared.buffers.get(&buffer).into_iter().flatten() {
            for (set, drawn) in &sets.gutter {
                drawn.visit(snapshot, start..end, |range, item| {
                    let line = text.offset_to_point(range.start).row;
                    if lines.contains(&line) {
                        entries.push(PluginGutterEntry {
                            plugin: plugin.clone(),
                            set: set.clone(),
                            line,
                            symbol: item.symbol.clone(),
                            color: item.color.clone(),
                            tooltip: item.tooltip.clone(),
                        });
                    }
                });
            }
        }
        entries.sort_by_key(|entry| entry.line);
        entries
    }

    /// Forgets plugin `plugin`: its sets go, and its slice sets nothing more.
    pub fn unload(&self, plugin: &str) {
        let mut shared = self.lock();
        shared.attached.remove(plugin);
        shared.forget(plugin);
    }

    /// Drops every plugin's sets on `buffer`, which was closed.
    pub fn close(&self, buffer: u64) {
        self.lock().buffers.remove(&buffer);
    }

    /// Attaches a new load of `plugin`, which starts with no sets.
    fn attach(&self, plugin: &str) -> u64 {
        let mut shared = self.lock();
        shared.loads += 1;
        let load = shared.loads;
        shared.attached.insert(plugin.to_owned(), load);
        shared.forget(plugin);
        load
    }

    fn visit_decorations(
        &self,
        buffer: u64,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
        mut visit: impl FnMut(&str, &str, Range<usize>, &Decoration),
    ) {
        let shared = self.lock();
        for (plugin, sets) in shared.buffers.get(&buffer).into_iter().flatten() {
            for (set, drawn) in &sets.decorations {
                drawn.visit(snapshot, range.clone(), |range, item| {
                    visit(plugin, set, range, item);
                });
            }
        }
    }

    /// Rebases every set on `buffer` onto `snapshot`, without what edits have taken.
    fn refresh(&self, buffer: u64, snapshot: &BufferSnapshot) {
        let mut shared = self.lock();
        let Some(plugins) = shared.buffers.get_mut(&buffer) else {
            return;
        };
        for sets in plugins.values_mut() {
            sets.refresh(snapshot);
        }
        plugins.retain(|_, sets| !sets.is_empty());
        if plugins.is_empty() {
            shared.buffers.remove(&buffer);
        }
    }

    /// Replaces `plugin`'s set `set` on `buffer` with `drawn`, made in `now`, if `load` is still
    /// the plugin's and its sets on the buffer stay within [`MAX_ENTRIES_PER_BUFFER`]. What
    /// edits have taken from its other sets by `now` doesn't count.
    #[allow(clippy::too_many_arguments)]
    fn replace<T>(
        &self,
        plugin: &str,
        load: u64,
        buffer: u64,
        now: &BufferSnapshot,
        set: String,
        drawn: Drawn<T>,
        pick: impl FnOnce(&mut Sets) -> &mut BTreeMap<String, Drawn<T>>,
    ) -> Result<(), Error> {
        let mut shared = self.lock();
        current(&shared, plugin, load)?;
        let sets = shared
            .buffers
            .entry(buffer)
            .or_default()
            .entry(plugin.to_owned())
            .or_default();
        sets.refresh(now);
        let total = sets.len();
        let sets_of_kind = pick(sets);
        let replaced = sets_of_kind.get(&set).map_or(0, |old| old.records.len());
        let after = total - replaced + drawn.records.len();
        let replaced = if after > MAX_ENTRIES_PER_BUFFER {
            Err(Error::Failed(format!(
                "plugin {plugin} would keep {after} decorations and gutter entries on buffer \
                 {buffer}, over the limit of {MAX_ENTRIES_PER_BUFFER}"
            )))
        } else {
            if drawn.records.is_empty() {
                sets_of_kind.remove(&set);
            } else {
                sets_of_kind.insert(set, drawn);
            }
            Ok(())
        };
        shared.prune(buffer, plugin);
        replaced
    }

    /// Removes both kinds of `plugin`'s set `set` from `buffer`, if `load` is still the plugin's.
    fn clear(&self, plugin: &str, load: u64, buffer: u64, set: &str) -> Result<(), Error> {
        let mut shared = self.lock();
        current(&shared, plugin, load)?;
        if let Some(sets) = shared
            .buffers
            .get_mut(&buffer)
            .and_then(|plugins| plugins.get_mut(plugin))
        {
            sets.decorations.remove(set);
            sets.gutter.remove(set);
        }
        shared.prune(buffer, plugin);
        Ok(())
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }
}

impl Shared {
    /// Drops every set of `plugin`'s.
    fn forget(&mut self, plugin: &str) {
        self.buffers.retain(|_, plugins| {
            plugins.remove(plugin);
            !plugins.is_empty()
        });
    }

    /// Drops `plugin`'s entry for `buffer` if it has no sets left, and the buffer's if no plugin
    /// has.
    fn prune(&mut self, buffer: u64, plugin: &str) {
        let Entry::Occupied(mut plugins) = self.buffers.entry(buffer) else {
            return;
        };
        if plugins.get().get(plugin).is_some_and(Sets::is_empty) {
            plugins.get_mut().remove(plugin);
        }
        if plugins.get().is_empty() {
            plugins.remove();
        }
    }
}

/// Refuses `plugin` unless `load` is its current one.
fn current(shared: &Shared, plugin: &str, load: u64) -> Result<(), Error> {
    if shared.attached.get(plugin) == Some(&load) {
        Ok(())
    } else {
        Err(Error::Denied(format!("plugin {plugin} was unloaded")))
    }
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The editor's [`DecorationProvider`] for one buffer's plugin decorations. It reads the
/// [`DecorationStore`] alone, so it answers at once whatever the plugins are doing.
pub struct PluginDecorations {
    store: DecorationStore,
    buffer: u64,
}

impl DecorationProvider for PluginDecorations {
    fn decorations(&self, snapshot: &BufferSnapshot, range: Range<usize>, out: &mut Vec<Painted>) {
        self.store
            .visit_decorations(self.buffer, snapshot, range, |_, _, range, item| {
                if let Some((kind, tint)) = painted(item) {
                    out.push(Painted::new(range, kind, tint));
                }
            });
    }

    fn refresh(&mut self, snapshot: &BufferSnapshot) {
        self.store.refresh(self.buffer, snapshot);
    }
}

/// How the editor draws `decoration`; none for a style it has no way to draw.
fn painted(decoration: &Decoration) -> Option<(DecorationKind, Tint)> {
    let (kind, default) = match decoration.style {
        DecorationStyle::Highlight => (DecorationKind::Highlight, Tint::Accent),
        DecorationStyle::Underline => (DecorationKind::Underline { wavy: false }, Tint::Accent),
        DecorationStyle::Squiggle => (DecorationKind::Underline { wavy: true }, Tint::Error),
        DecorationStyle::Faded => (DecorationKind::Foreground("comment".into()), Tint::Muted),
        DecorationStyle::Strikethrough => return None,
    };
    let tint = decoration
        .color
        .as_deref()
        .and_then(tint)
        .unwrap_or(default);
    Some((kind, tint))
}

/// The tint a theme colour such as `diagnostic.warning` names, by its last segment.
fn tint(color: &str) -> Option<Tint> {
    Some(match color.rsplit('.').next()? {
        "error" => Tint::Error,
        "warning" => Tint::Warning,
        "info" | "information" => Tint::Information,
        "hint" => Tint::Hint,
        "accent" => Tint::Accent,
        "muted" => Tint::Muted,
        "added" => Tint::Added,
        "modified" => Tint::Modified,
        "removed" => Tint::Removed,
        _ => return None,
    })
}

/// The decorations slice of [`HostState`]. The default has no [`DecorationStore`] and answers
/// every import with [`unimplemented`].
#[derive(Default)]
pub struct State {
    attached: Option<Attached>,
}

struct Attached {
    plugin: String,
    grants: Grants,
    store: DecorationStore,
    /// The load of the plugin this slice is.
    load: u64,
    buffers: Box<dyn BufferVersions>,
}

impl State {
    /// Plugin `plugin`'s slice on `store`, over `buffers`, as far as `grants` allow. It is a new
    /// load of the plugin: whatever sets an earlier load left are dropped, and that load's slice
    /// sets nothing more.
    pub fn new(
        plugin: &str,
        grants: &Grants,
        store: &DecorationStore,
        buffers: Box<dyn BufferVersions>,
    ) -> Self {
        Self {
            attached: Some(Attached {
                plugin: plugin.to_owned(),
                grants: grants.clone(),
                load: store.attach(plugin),
                store: store.clone(),
                buffers,
            }),
        }
    }

    /// The attachment, if its grants include `decorations`, which `import` needs.
    fn granted(&mut self, import: &str) -> Result<&mut Attached, Error> {
        let Some(attached) = self.attached.as_mut() else {
            return unimplemented(import);
        };
        if !attached.grants.allows(&Capability::Decorations) {
            return Err(Error::Denied(format!(
                "{import} needs the {} capability",
                Capability::Decorations
            )));
        }
        Ok(attached)
    }
}

impl Attached {
    /// `buffer` as it was at `version` and as it is now, if the one maps onto the other.
    fn snapshots(
        &mut self,
        buffer: u64,
        version: u64,
    ) -> Result<(BufferSnapshot, BufferSnapshot), Error> {
        let now = self
            .buffers
            .now(buffer)
            .ok_or_else(|| Error::NotFound(format!("no open buffer has id {buffer}")))?;
        let then = self.buffers.at(buffer, version).filter(|then| {
            then.id() == now.id()
                && then.version() <= now.version()
                && then.version() >= now.oldest_version()
        });
        let then = then.ok_or_else(|| {
            Error::Conflict(format!(
                "buffer {buffer}'s version {version} can't be mapped to the buffer now"
            ))
        })?;
        Ok((then, now))
    }

    /// `items`, each with its range in the text at `version` from `range`, as a set on `buffer`
    /// now: mapped through the edits since as anchors are, less whatever those edits took.
    /// Returns the buffer now too.
    fn drawn<T>(
        &mut self,
        buffer: u64,
        version: u64,
        items: Vec<T>,
        range: fn(&Text, &T) -> Result<Range<usize>, String>,
    ) -> Result<(Drawn<T>, BufferSnapshot), Error> {
        if items.len() > MAX_ENTRIES_PER_BUFFER {
            return Err(Error::Failed(format!(
                "{} entries are over the limit of {MAX_ENTRIES_PER_BUFFER} on a buffer",
                items.len()
            )));
        }
        let (then, now) = self.snapshots(buffer, version)?;
        let from = then.version();
        let mapped = |range: Range<usize>| {
            let start = now.map_offset(range.start, START, from);
            start..now.map_offset(range.end, END, from).max(start)
        };
        let mut placed = Vec::with_capacity(items.len());
        for (index, item) in items.into_iter().enumerate() {
            let range = range(then.text(), &item)
                .map_err(|why| Error::InvalidArgument(format!("entry {index}'s {why}")))?;
            let Some(watch) = Watch::new(from, then.text(), &range).advance(&now) else {
                continue;
            };
            placed.push(Placed {
                range: mapped(range),
                watch,
                item,
            });
        }
        Ok((Drawn::new(&now, placed), now))
    }
}

/// The text whose deletion takes an empty entry at `at` in `text`, bytes of it: the characters
/// either side. None at either end of the text, where no deletion takes it until it has text on
/// both sides.
fn neighbours(text: &Text, at: usize) -> Option<Vec<Range<usize>>> {
    if at == 0 || at >= text.len() {
        return None;
    }
    Some(vec![
        text.clip_offset(at - 1, Bias::Left)..at,
        at..text.clip_offset(at + 1, Bias::Right),
    ])
}

/// `decoration`'s range in `text`, or why it isn't one.
fn decoration_range(text: &Text, decoration: &Decoration) -> Result<Range<usize>, String> {
    let wit::Range { start, end } = decoration.range;
    let len = text.len();
    let offset = |offset: u64| usize::try_from(offset).ok().filter(|&offset| offset <= len);
    let (Some(start), Some(end)) = (offset(start), offset(end)) else {
        return Err(format!(
            "range {start}..{end} is past the end of the text, at {len}"
        ));
    };
    if start > end {
        return Err(format!("range {start}..{end} ends before it starts"));
    }
    if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return Err(format!("range {start}..{end} splits a character"));
    }
    Ok(start..end)
}

/// The bytes of `entry`'s line in `text`, its line break included, or why it isn't one.
fn line_range(text: &Text, entry: &GutterEntry) -> Result<Range<usize>, String> {
    let (line, rows) = (entry.line, text.row_count());
    if line >= rows {
        return Err(format!("line {line} is past the last line, {}", rows - 1));
    }
    let end = if line + 1 < rows {
        text.row_start(line + 1)
    } else {
        text.len()
    };
    Ok(text.row_start(line)..end)
}

impl Host for HostState {
    fn set_decorations(
        &mut self,
        buffer: u64,
        version: u64,
        set: String,
        decorations: Vec<Decoration>,
    ) -> Result<(), Error> {
        let attached = self
            .decorations
            .granted("fastedit:editor/decorations.set-decorations")?;
        let (drawn, now) = attached.drawn(buffer, version, decorations, decoration_range)?;
        let (plugin, load) = (&attached.plugin, attached.load);
        (attached.store).replace(plugin, load, buffer, &now, set, drawn, |sets| {
            &mut sets.decorations
        })
    }

    fn set_gutter(
        &mut self,
        buffer: u64,
        version: u64,
        set: String,
        entries: Vec<GutterEntry>,
    ) -> Result<(), Error> {
        let attached = self
            .decorations
            .granted("fastedit:editor/decorations.set-gutter")?;
        let (drawn, now) = attached.drawn(buffer, version, entries, line_range)?;
        let (plugin, load) = (&attached.plugin, attached.load);
        (attached.store).replace(plugin, load, buffer, &now, set, drawn, |sets| {
            &mut sets.gutter
        })
    }

    fn clear(&mut self, buffer: u64, set: String) -> Result<(), Error> {
        let attached = self
            .decorations
            .granted("fastedit:editor/decorations.clear")?;
        (attached.store).clear(&attached.plugin, attached.load, buffer, &set)
    }
}
