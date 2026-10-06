//! [`SymbolIndex`]: the project's definitions by name, for go-to-def by
//! search and workspace symbols.

use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap, HashSet},
    fmt, fs,
    mem::size_of,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use fe_project::{FileSetDelta, FinderIndex, FsChange};
use fe_syntax::OutlineKind;
use frizbee::{CaseMatching, Config, Matcher, SortStrategy};

use crate::{Extracted, FileSymbols, Symbol, extract::symbols_language};

/// No name: a symbol without a container.
const NONE: u32 = u32::MAX;

/// Every definition of the files added to it, by name.
///
/// Each distinct name, and each container name, is stored once. A symbol is
/// a fixed-size record with `u32` offsets and name ids: files over
/// `u32::MAX` bytes are never parsed, so their offsets always fit (a symbol
/// whose offsets don't is left out). A name's symbols are found by a hash
/// lookup, never a scan.
///
/// It's `Send + Sync`, so it can be shared behind an `RwLock` and queried
/// while a [`crate::build`] extends it.
///
/// Watcher batches keep it current, re-extracting only the files that
/// changed, without holding the index while parsing:
///
/// 1. [`SymbolIndex::plan`] applies a batch of [`FsChange`]s (under the
///    lock), drops the symbols of files gone, and returns the files to
///    extract again as a [`Reindex`]. While building, a batch is held for
///    [`SymbolIndex::finish`], which returns its `Reindex` then.
/// 2. The caller extracts them, with [`crate::extract_all`] from
///    [`SymbolIndex::canonical_root`], without the lock.
/// 3. [`SymbolIndex::update`] (under the lock again) puts the results in.
///
/// A file that changes again while it's extracted is caught by its next
/// batch, which compares its length and modification time again.
/// Reindexes are meant to be updated in the order they were planned (one
/// background thread taking them in turn); a batch may still be planned
/// while an earlier one is being extracted.
///
/// Which files exist, and which the project ignores, is a
/// [`FinderIndex`]'s business: the index keeps one as its file set, fed by
/// [`SymbolIndex::extend`] and kept current by `plan`.
#[derive(Debug)]
pub struct SymbolIndex {
    root: PathBuf,
    file_set: FileSet,
    /// By file id; `None` for a free id, reused by the next file added.
    files: Vec<Option<File>>,
    free_files: Vec<u32>,
    by_rel: HashMap<Arc<Path>, u32>,
    names: Names,
    /// By name id: where the symbols with that name are.
    postings: Vec<Vec<Posting>>,
    symbols: usize,
    /// Names with at least one symbol, rather than only containers.
    distinct_names: usize,
    complete: bool,
}

/// The files that exist and aren't ignored, as watcher batches report
/// them.
struct FileSet(FinderIndex);

impl fmt::Debug for FileSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (files, complete) = self.0.progress();
        f.debug_struct("FileSet")
            .field("files", &files)
            .field("complete", &complete)
            .finish()
    }
}

/// The files a watcher batch, or the end of a build, asks to extract
/// again: from [`SymbolIndex::plan`] or [`SymbolIndex::finish`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reindex {
    /// Relative to the root, sorted: each is new or changed, and its
    /// language has definitions.
    pub extract: Vec<PathBuf>,
}

#[derive(Debug)]
struct File {
    rel: Arc<Path>,
    len: u64,
    modified: Option<SystemTime>,
    records: Box<[Record]>,
}

/// One symbol, as stored.
#[derive(Clone, Copy, Debug)]
struct Record {
    name: u32,
    /// A name id, or [`NONE`].
    container: u32,
    kind: OutlineKind,
    name_start: u32,
    name_end: u32,
    start: u32,
    end: u32,
    row: u32,
}

/// A symbol's place: its file id and its index among the file's records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Posting {
    file: u32,
    ix: u32,
}

/// Interned strings, counted by the records naming them, and freed once
/// none do.
#[derive(Debug, Default)]
struct Names {
    /// By id; `None` for a free id.
    strings: Vec<Option<Arc<str>>>,
    ids: HashMap<Arc<str>, u32>,
    /// By id: how many records use the string, as a name or a container.
    refs: Vec<u32>,
    free: Vec<u32>,
    /// The bytes of the strings held.
    bytes: usize,
}

impl Names {
    fn intern(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.ids.get(name) {
            self.refs[id as usize] += 1;
            return id;
        }
        let string: Arc<str> = Arc::from(name);
        self.bytes += name.len();
        let id = match self.free.pop() {
            Some(id) => {
                self.strings[id as usize] = Some(string.clone());
                self.refs[id as usize] = 1;
                id
            }
            None => {
                self.strings.push(Some(string.clone()));
                self.refs.push(1);
                to_u32(self.strings.len() - 1)
            }
        };
        self.ids.insert(string, id);
        id
    }

    fn release(&mut self, id: u32) {
        let refs = &mut self.refs[id as usize];
        *refs -= 1;
        if *refs == 0
            && let Some(string) = self.strings[id as usize].take()
        {
            self.bytes -= string.len();
            self.ids.remove(&*string);
            self.free.push(id);
        }
    }

    fn get(&self, id: u32) -> &str {
        self.strings[id as usize]
            .as_deref()
            .expect("a record's name is interned")
    }
}

/// A symbol in a [`SymbolIndex`], borrowed from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolRef<'a> {
    /// The file, relative to the root.
    pub rel: &'a Path,
    pub name: &'a str,
    pub kind: OutlineKind,
    /// Offsets into the text as the editor loads it; see [`Symbol`].
    pub name_range: Range<usize>,
    pub range: Range<usize>,
    pub row: u32,
    pub container: Option<&'a str>,
}

/// A result of [`SymbolIndex::fuzzy`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolMatch<'a> {
    pub symbol: SymbolRef<'a>,
    /// frizbee's score of the name; higher is better.
    pub score: u16,
    /// The matched chars of the name, as ascending char indices.
    pub indices: Vec<usize>,
}

/// The size of a [`SymbolIndex`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IndexStats {
    pub files: usize,
    pub symbols: usize,
    /// Distinct symbol names (containers that name no symbol don't count).
    pub distinct_names: usize,
    /// The memory the index holds, estimated from its collections'
    /// capacities and strings. Its file set, a [`FinderIndex`] of the
    /// project's paths, isn't counted.
    pub approx_bytes: usize,
}

impl SymbolIndex {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            file_set: FileSet(FinderIndex::new(&root)),
            root,
            files: Vec::new(),
            free_files: Vec::new(),
            by_rel: HashMap::new(),
            names: Names::default(),
            postings: Vec::new(),
            symbols: 0,
            distinct_names: 0,
            complete: false,
        }
    }

    /// The root the files' paths are relative to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The root as the file set resolved it, once, when the index was made:
    /// canonical, as watchers report paths. [`crate::extract_all`] reads
    /// from it, so a root swapped for a symlink since isn't followed.
    pub fn canonical_root(&self) -> &Path {
        self.file_set.0.root_path()
    }

    /// Adds a batch of files. A file already in the index, by its relative
    /// path, has its symbols replaced; within the batch the last copy of a
    /// path wins.
    pub fn extend(&mut self, batch: Vec<FileSymbols>) {
        let mut last: HashMap<PathBuf, FileSymbols> = HashMap::with_capacity(batch.len());
        let mut order: Vec<PathBuf> = Vec::with_capacity(batch.len());
        for file in batch {
            if !last.contains_key(&file.rel) {
                order.push(file.rel.clone());
            }
            last.insert(file.rel.clone(), file);
        }
        let replaced: Vec<u32> = (order.iter())
            .filter_map(|rel| self.by_rel.get(rel.as_path()).copied())
            .collect();
        self.drop_files(&replaced);
        self.file_set.0.extend(order.iter().cloned());
        for rel in order {
            if let Some(file) = last.remove(&rel) {
                self.insert(file);
            }
        }
    }

    /// Removes one file. Returns whether it was indexed.
    pub fn remove(&mut self, rel: impl AsRef<Path>) -> bool {
        match self.by_rel.get(rel.as_ref()) {
            Some(&id) => {
                self.drop_files(&[id]);
                true
            }
            None => false,
        }
    }

    /// Removes every file under the directory `rel_dir`, by whole path
    /// components: `src/a` covers `src/a/x.rs`, not `src/ab.rs`. Returns how
    /// many were removed.
    pub fn remove_under(&mut self, rel_dir: impl AsRef<Path>) -> usize {
        let dir = rel_dir.as_ref();
        let ids: Vec<u32> = (self.by_rel.iter())
            .filter(|(rel, _)| rel.starts_with(dir))
            .map(|(_, &id)| id)
            .collect();
        self.drop_files(&ids);
        ids.len()
    }

    /// Marks the build complete, once its last batch is in. Returns the
    /// files to extract again for batches planned while building, whose
    /// removed files' symbols are dropped here; see [`SymbolIndex::plan`].
    pub fn finish(&mut self) -> Reindex {
        self.complete = true;
        let delta = self.file_set.0.finish();
        self.reindex(delta)
    }

    /// Applies a batch of changes from a [`fe_project::ProjectWatcher`] to
    /// the file set, as [`FinderIndex::apply`] does, with its ignore rules
    /// and its limits. Drops the symbols of every file it removed, and
    /// returns the files to extract again: those it added or found
    /// changed whose language has definitions, and which aren't indexed
    /// or whose length or modification time differs from what was
    /// indexed ([`SymbolIndex::file_state`]).
    ///
    /// While building (before [`SymbolIndex::finish`]) the batch is held,
    /// and the `Reindex` is empty: `finish` returns it.
    ///
    /// Runs on the caller's thread, which holds the index meanwhile: it
    /// checks each changed path on disk, and walks a new directory, or the
    /// whole root for a [`FsChange::Rescan`].
    pub fn plan(&mut self, changes: &[FsChange]) -> Reindex {
        let delta = self.file_set.0.apply(changes);
        self.reindex(delta)
    }

    /// Puts in the files a [`Reindex`] asked for, once extracted. A file
    /// that couldn't be extracted now, for any [`crate::Skipped`] reason,
    /// has its old symbols dropped. A file the file set no longer holds,
    /// because a batch planned since removed it, is left out (and its
    /// symbols dropped), so a late result can't bring it back.
    ///
    /// Use this rather than [`SymbolIndex::extend`] for a `Reindex`'s
    /// results: `extend` is the build's sink, and adds every file it gets
    /// to the file set, so a late result would bring a deleted file back.
    ///
    /// The results of a cancelled [`crate::extract_all`] leave out the
    /// files it didn't start, which keep their old symbols: extract the
    /// whole `Reindex` again rather than dropping it.
    pub fn update(&mut self, extracted: Extracted) {
        let mut dead: Vec<u32> = (extracted.failed.iter())
            .filter_map(|(rel, _)| self.by_rel.get(rel.as_path()).copied())
            .collect();
        let mut files = Vec::with_capacity(extracted.files.len());
        for file in extracted.files {
            if self.file_set.0.contains(&file.rel) {
                files.push(file);
            } else if let Some(&id) = self.by_rel.get(file.rel.as_path()) {
                dead.push(id);
            }
        }
        dead.sort_unstable();
        dead.dedup();
        self.drop_files(&dead);
        self.extend(files);
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// How many files are indexed, and whether the build is complete.
    pub fn progress(&self) -> (usize, bool) {
        (self.by_rel.len(), self.complete)
    }

    /// Every symbol named exactly `name`, case-sensitively, by relative
    /// path and then start. Containers aren't symbols: a name used only as
    /// a container finds nothing.
    pub fn definitions(&self, name: &str) -> Vec<SymbolRef<'_>> {
        let Some(&id) = self.names.ids.get(name) else {
            return Vec::new();
        };
        let mut found: Vec<SymbolRef<'_>> = (self.postings[id as usize].iter())
            .map(|&posting| self.symbol(posting))
            .collect();
        found.sort_by(|a, b| a.rel.cmp(b.rel).then(a.range.start.cmp(&b.range.start)));
        found
    }

    /// The length and modification time a file had when it was read, if
    /// it's indexed.
    pub fn file_state(&self, rel: impl AsRef<Path>) -> Option<(u64, Option<SystemTime>)> {
        let file = self.file(*self.by_rel.get(rel.as_ref())?);
        Some((file.len, file.modified))
    }

    /// A file's symbols in its outline order, if it's indexed.
    pub fn file_symbols(&self, rel: impl AsRef<Path>) -> Option<Vec<SymbolRef<'_>>> {
        let id = *self.by_rel.get(rel.as_ref())?;
        let file = self.file(id);
        Some(
            (0..file.records.len())
                .map(|ix| {
                    self.symbol(Posting {
                        file: id,
                        ix: to_u32(ix),
                    })
                })
                .collect(),
        )
    }

    /// The best `limit` symbols for `query`, for workspace symbols. The
    /// distinct names are matched with frizbee, ignoring case and with
    /// every char of `query` in order, then each matched name stands for
    /// all its symbols. Best first: by score, then kind (types, then
    /// functions and methods, then the rest), then shorter path, then path,
    /// then start. An empty query matches nothing.
    pub fn fuzzy(&self, query: &str, limit: usize) -> Vec<SymbolMatch<'_>> {
        if query.is_empty() || limit == 0 {
            return Vec::new();
        }
        let ids: Vec<u32> = (0..self.postings.len())
            .filter(|&id| !self.postings[id].is_empty())
            .map(to_u32)
            .collect();
        let haystacks: Vec<&str> = ids.iter().map(|&id| self.names.get(id)).collect();
        // Ranked below, so frizbee needn't sort.
        let config = Config::default()
            .casing(CaseMatching::Ignore)
            .sort(SortStrategy::IndexAsc);
        let mut matcher = Matcher::new(query, &config);
        let mut matches = matcher.match_list(&haystacks);
        matches.sort_unstable_by_key(|m| std::cmp::Reverse(m.score));
        // The best `limit` so far, worst on top. Score comes first, so once
        // it's full, names scoring below its worst can't make the cut.
        let mut best: BinaryHeap<Candidate<'_>> = BinaryHeap::with_capacity(limit.min(1024));
        for m in &matches {
            if best.len() == limit && best.peek().is_some_and(|worst| worst.score > m.score) {
                break;
            }
            let id = ids[m.index as usize];
            for &posting in &self.postings[id as usize] {
                let candidate = self.candidate(m.score, id, posting);
                if best.len() < limit {
                    best.push(candidate);
                } else if let Some(mut worst) = best.peek_mut()
                    && candidate < *worst
                {
                    *worst = candidate;
                }
            }
        }
        let keyed: Vec<(u16, u32, SymbolRef<'_>)> = (best.into_sorted_vec().into_iter())
            .map(|candidate| {
                let symbol = self.symbol(candidate.posting);
                (candidate.score, candidate.name, symbol)
            })
            .collect();
        let mut indices: HashMap<u32, Vec<usize>> = HashMap::new();
        keyed
            .into_iter()
            .map(|(score, id, symbol)| {
                let indices = (indices.entry(id))
                    .or_insert_with(|| name_indices(&mut matcher, symbol.name, query))
                    .clone();
                SymbolMatch {
                    symbol,
                    score,
                    indices,
                }
            })
            .collect()
    }

    pub fn stats(&self) -> IndexStats {
        let mut bytes = size_of::<Self>()
            + self.files.capacity() * size_of::<Option<File>>()
            + self.free_files.capacity() * size_of::<u32>()
            + self.by_rel.capacity() * (size_of::<(Arc<Path>, u32)>() + 1)
            + self.postings.capacity() * size_of::<Vec<Posting>>()
            + (self.names.strings.capacity()) * size_of::<Option<Arc<str>>>()
            + self.names.ids.capacity() * (size_of::<(Arc<str>, u32)>() + 1)
            + self.names.refs.capacity() * size_of::<u32>()
            + self.names.free.capacity() * size_of::<u32>()
            // Each string, with its `Arc`'s two counts.
            + self.names.bytes
            + self.names.ids.len() * 2 * size_of::<usize>();
        for file in self.files.iter().flatten() {
            bytes += file.rel.as_os_str().len()
                + 2 * size_of::<usize>()
                + file.records.len() * size_of::<Record>();
        }
        for postings in &self.postings {
            bytes += postings.capacity() * size_of::<Posting>();
        }
        IndexStats {
            files: self.by_rel.len(),
            symbols: self.symbols,
            distinct_names: self.distinct_names,
            approx_bytes: bytes,
        }
    }

    /// Drops the symbols of the files `delta` removed, and lists the files
    /// it added or changed that need extracting.
    fn reindex(&mut self, delta: FileSetDelta) -> Reindex {
        let mut removed: Vec<u32> = (delta.removed.iter())
            .filter_map(|rel| self.by_rel.get(rel.as_path()).copied())
            .collect();
        if delta.rescanned {
            // The repair for anything missed: whatever the walk didn't find
            // goes, whether or not the file set held it.
            removed.extend(
                (self.by_rel.iter())
                    .filter(|(rel, _)| !self.file_set.0.contains(rel))
                    .map(|(_, &id)| id),
            );
            removed.sort_unstable();
            removed.dedup();
        }
        self.drop_files(&removed);
        let root = self.file_set.0.root_path();
        let extract = (delta.present.into_iter())
            .filter(|rel| {
                symbols_language(rel).is_some()
                    && self
                        .file_state(rel)
                        .is_none_or(|indexed| disk_state(root, rel) != Some(indexed))
            })
            .collect();
        Reindex { extract }
    }

    fn file(&self, id: u32) -> &File {
        self.files[id as usize]
            .as_ref()
            .expect("a posting's file is indexed")
    }

    fn symbol(&self, posting: Posting) -> SymbolRef<'_> {
        let file = self.file(posting.file);
        let record = &file.records[posting.ix as usize];
        SymbolRef {
            rel: &file.rel,
            name: self.names.get(record.name),
            kind: record.kind,
            name_range: record.name_start as usize..record.name_end as usize,
            range: record.start as usize..record.end as usize,
            row: record.row,
            container: (record.container != NONE).then(|| self.names.get(record.container)),
        }
    }

    fn candidate(&self, score: u16, name: u32, posting: Posting) -> Candidate<'_> {
        let file = self.file(posting.file);
        let record = &file.records[posting.ix as usize];
        Candidate {
            score,
            kind: kind_rank(record.kind),
            rel: &file.rel,
            start: record.start,
            name,
            posting,
        }
    }

    fn insert(&mut self, file: FileSymbols) {
        let id = match self.free_files.pop() {
            Some(id) => id,
            None => {
                self.files.push(None);
                to_u32(self.files.len() - 1)
            }
        };
        let mut records = Vec::with_capacity(file.symbols.len());
        for symbol in &file.symbols {
            let Some(record) = self.record(symbol) else {
                continue;
            };
            let postings = &mut self.postings[record.name as usize];
            if postings.is_empty() {
                self.distinct_names += 1;
            }
            postings.push(Posting {
                file: id,
                ix: to_u32(records.len()),
            });
            records.push(record);
        }
        self.symbols += records.len();
        let rel: Arc<Path> = Arc::from(file.rel);
        self.by_rel.insert(rel.clone(), id);
        self.files[id as usize] = Some(File {
            rel,
            len: file.len,
            modified: file.modified,
            records: records.into_boxed_slice(),
        });
    }

    /// The record of `symbol`, with its names interned, unless its offsets
    /// don't fit.
    fn record(&mut self, symbol: &Symbol) -> Option<Record> {
        let offset = |offset: usize| u32::try_from(offset).ok();
        let (name_start, name_end) = (
            offset(symbol.name_range.start)?,
            offset(symbol.name_range.end)?,
        );
        let (start, end) = (offset(symbol.range.start)?, offset(symbol.range.end)?);
        let name = self.intern(&symbol.name);
        let container = match &symbol.container {
            Some(container) => self.intern(container),
            None => NONE,
        };
        Some(Record {
            name,
            container,
            kind: symbol.kind,
            name_start,
            name_end,
            start,
            end,
            row: symbol.row,
        })
    }

    fn intern(&mut self, name: &str) -> u32 {
        let id = self.names.intern(name);
        if self.postings.len() <= id as usize {
            self.postings.resize_with(id as usize + 1, Vec::new);
        }
        id
    }

    /// Removes the files with these ids, and their symbols: each name's
    /// symbols are filtered once, however many of the files use it.
    fn drop_files(&mut self, ids: &[u32]) {
        if ids.is_empty() {
            return;
        }
        let dead: HashSet<u32> = ids.iter().copied().collect();
        let touched: HashSet<u32> = (dead.iter())
            .flat_map(|&id| self.file(id).records.iter().map(|record| record.name))
            .collect();
        for name in touched {
            let postings = &mut self.postings[name as usize];
            postings.retain(|posting| !dead.contains(&posting.file));
            if postings.is_empty() {
                self.distinct_names -= 1;
                *postings = Vec::new();
            }
        }
        for id in dead {
            let file = self.files[id as usize]
                .take()
                .expect("a dropped file is indexed");
            for record in &file.records {
                self.names.release(record.name);
                if record.container != NONE {
                    self.names.release(record.container);
                }
            }
            self.symbols -= file.records.len();
            self.by_rel.remove(&file.rel);
            self.free_files.push(id);
        }
    }
}

/// The length and modification time of the regular file `root/rel`, as
/// [`FileSymbols`] records them, or `None` if it isn't one.
fn disk_state(root: &Path, rel: &Path) -> Option<(u64, Option<SystemTime>)> {
    let metadata = fs::symlink_metadata(root.join(rel)).ok()?;
    metadata
        .is_file()
        .then(|| (metadata.len(), metadata.modified().ok()))
}

/// A symbol [`SymbolIndex::fuzzy`] may return, ordered best first: by
/// score, then kind, then shorter path, then path, then start.
#[derive(Debug)]
struct Candidate<'a> {
    score: u16,
    kind: u8,
    rel: &'a Path,
    start: u32,
    name: u32,
    posting: Posting,
}

impl Ord for Candidate<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        (other.score.cmp(&self.score))
            .then(self.kind.cmp(&other.kind))
            .then_with(|| (self.rel.as_os_str().len()).cmp(&other.rel.as_os_str().len()))
            .then_with(|| self.rel.cmp(other.rel))
            .then(self.start.cmp(&other.start))
            .then((self.posting.file, self.posting.ix).cmp(&(other.posting.file, other.posting.ix)))
    }
}

impl PartialOrd for Candidate<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Candidate<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Candidate<'_> {}

/// Types first, then functions and methods, then the rest.
fn kind_rank(kind: OutlineKind) -> u8 {
    match kind {
        OutlineKind::Class
        | OutlineKind::Struct
        | OutlineKind::Enum
        | OutlineKind::Interface
        | OutlineKind::Trait
        | OutlineKind::Type => 0,
        OutlineKind::Function | OutlineKind::Method => 1,
        _ => 2,
    }
}

/// The chars of `name` that `query` matched, as ascending char indices,
/// one for each char of `query`.
///
/// frizbee gives byte offsets, one for each byte of a multi-byte char;
/// they become char indices here. If they don't spell `query` (its
/// alignment can be cut short), the first chars that do are taken
/// instead, matched as frizbee matches them.
fn name_indices(matcher: &mut Matcher, name: &str, query: &str) -> Vec<usize> {
    let mut bytes: Vec<usize> = matcher
        .match_one_indices(name, 0)
        .map(|found| found.indices.iter().map(|&byte| byte as usize).collect())
        .unwrap_or_default();
    bytes.sort_unstable();
    let chars: Vec<usize> = (name.char_indices().enumerate())
        .filter(|&(_, (start, ch))| {
            let end = start + ch.len_utf8();
            bytes
                .binary_search_by(|&byte| {
                    if byte < start {
                        Ordering::Less
                    } else if byte >= end {
                        Ordering::Greater
                    } else {
                        Ordering::Equal
                    }
                })
                .is_ok()
        })
        .map(|(ix, _)| ix)
        .collect();
    let spelled = chars.len() == query.chars().count()
        && (query.chars())
            .zip(chars.iter().filter_map(|&ix| name.chars().nth(ix)))
            .all(|(want, ch)| case_matches(want, ch));
    if spelled {
        return chars;
    }
    let mut needle = query.chars().peekable();
    let mut greedy = Vec::new();
    for (ix, ch) in name.chars().enumerate() {
        if needle.next_if(|&want| case_matches(want, ch)).is_some() {
            greedy.push(ix);
        }
    }
    if needle.peek().is_none() {
        greedy
    } else {
        chars
    }
}

/// Whether query char `want` matches name char `ch`, ignoring case as
/// frizbee does: `ch` is `want` or its other case, where that is a single
/// char of the same UTF-8 length.
fn case_matches(want: char, ch: char) -> bool {
    if want == ch {
        return true;
    }
    let other: Vec<char> = if want.is_uppercase() {
        want.to_lowercase().collect()
    } else if want.is_lowercase() {
        want.to_uppercase().collect()
    } else {
        return false;
    };
    matches!(other[..], [other] if other.len_utf8() == want.len_utf8() && other == ch)
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).expect("fewer than u32::MAX items")
}
