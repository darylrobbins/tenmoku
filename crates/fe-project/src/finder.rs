//! Every non-ignored file path of a project, indexed up front and matched
//! fuzzily, for Cmd-P.

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashSet},
    ffi::{OsStr, OsString},
    fs::{self, File},
    io,
    ops::Bound,
    os::unix::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use frizbee::{CaseMatching, Config, Matcher, SortStrategy};
use ignore::{WalkState, gitignore::Gitignore};

use crate::{
    FsChange,
    listing::{self, Access, EntryKind, Identity, Rules},
};

/// Paths a walk worker collects before sending them on.
const BATCH: usize = 1024;

/// Added to the score of a match within the file name, so that it ranks above
/// every match that spans directories (whose scores fit in a `u16`).
const NAME_TIER: u32 = 1 << 16;

/// One result of [`FinderIndex::query`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinderMatch {
    /// Relative to the root, `/`-separated. A name that isn't UTF-8 is
    /// converted lossily.
    pub path: String,
    /// The path relative to the root as it is on disk, which differs from
    /// `path` only for names that aren't UTF-8.
    pub relative: PathBuf,
    /// Higher is better. Matches within the file name score above all others.
    pub score: u32,
    /// The matched characters of `path`, as ascending char indices.
    pub indices: Vec<usize>,
}

/// Counters for tests and benches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FinderStats {
    /// Directories walked by [`FinderIndex::apply`] (or by
    /// [`FinderIndex::finish`], for changes that came while building),
    /// including the root for a [`FsChange::Rescan`].
    pub walks: u64,
    /// Results whose match indices were computed.
    pub indices_computed: u64,
}

/// An indexed path.
#[derive(Debug)]
struct Item {
    /// Relative, `/`-separated, lossy where a name isn't UTF-8.
    path: String,
    /// The real relative path, where `path` is lossy.
    real: Option<Box<Path>>,
    /// The byte offset of the file name in `path`.
    name: u32,
    /// `path`'s length in chars.
    chars: u32,
}

impl Item {
    fn new(rel: &Path) -> Item {
        let lossy = rel.as_os_str().to_string_lossy();
        let real = match lossy {
            Cow::Borrowed(_) => None,
            Cow::Owned(_) => Some(rel.into()),
        };
        let path = lossy.into_owned();
        let name = path.rfind('/').map_or(0, |slash| slash + 1);
        Item {
            name: to_u32(name),
            chars: to_u32(path.chars().count()),
            path,
            real,
        }
    }

    /// The real relative path's bytes.
    fn real_bytes(&self) -> &[u8] {
        match &self.real {
            Some(real) => real.as_os_str().as_bytes(),
            None => self.path.as_bytes(),
        }
    }

    /// Path order: by `path`, then, for paths that read the same, by the real
    /// bytes, a lossless path first.
    fn order(&self) -> (&str, Option<&[u8]>) {
        (
            &self.path,
            self.real.as_deref().map(|real| real.as_os_str().as_bytes()),
        )
    }

    /// Its key in [`FinderIndex::keys`].
    fn key(&self) -> Box<[u8]> {
        make_key(&self.path, self.order().1)
    }

    fn file_name(&self) -> &str {
        &self.path[self.name as usize..]
    }
}

impl AsRef<str> for Item {
    fn as_ref(&self) -> &str {
        &self.path
    }
}

/// An index key, which sorts in [`Item::order`]: the path, then for a lossy
/// one a NUL (which no path holds) and the real bytes.
fn make_key(path: &str, real: Option<&[u8]>) -> Box<[u8]> {
    let mut key = path.as_bytes().to_vec();
    if let Some(real) = real {
        key.push(0);
        key.extend_from_slice(real);
    }
    key.into()
}

/// The key of relative path `rel`.
fn key_of(rel: &Path) -> Box<[u8]> {
    match rel.as_os_str().to_string_lossy() {
        Cow::Borrowed(path) => make_key(path, None),
        Cow::Owned(path) => make_key(&path, Some(rel.as_os_str().as_bytes())),
    }
}

/// The ascending indices of the chars of `path` that hold any of `bytes`.
fn char_indices(path: &str, bytes: impl Iterator<Item = usize>) -> Vec<usize> {
    let mut bytes: Vec<usize> = bytes.collect();
    bytes.sort_unstable();
    let mut chars = Vec::with_capacity(bytes.len());
    let mut wanted = bytes.into_iter().peekable();
    for (char_ix, (start, ch)) in path.char_indices().enumerate() {
        if wanted.peek().is_none() {
            break;
        }
        let end = start + ch.len_utf8();
        let mut hit = false;
        while wanted.next_if(|&byte| byte < end).is_some() {
            hit = true;
        }
        if hit {
            chars.push(char_ix);
        }
    }
    chars
}

/// Whether the chars of `path` at `indices` spell `text`, ignoring case.
fn spells(path: &str, indices: &[usize], text: &str) -> bool {
    let mut wanted = indices.iter().copied().peekable();
    let picked = path
        .chars()
        .enumerate()
        .filter(|&(ix, _)| wanted.next_if_eq(&ix).is_some())
        .map(|(_, ch)| ch);
    indices.len() == text.chars().count()
        && text
            .chars()
            .zip(picked)
            .all(|(want, ch)| case_matches(want, ch))
}

/// Whether needle char `want` matches path char `ch`, ignoring case as
/// frizbee does: `ch` is `want` or its other case, where that is a single
/// char of the same UTF-8 length. So `ς` matches `Σ`, but `Σ` matches only
/// `σ`, its lowercase.
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
    u32::try_from(n).expect("paths and indexes fit in a u32")
}

/// A candidate result: its score, its item, and whether the file name matched.
#[derive(Clone, Copy)]
struct Candidate {
    score: u32,
    item: u32,
    in_name: bool,
}

/// Every non-ignored file of a project, as paths relative to its root, for
/// fuzzy matching with [`FinderIndex::query`].
///
/// Files only: no directories and no symlinks (which, as in
/// [`crate::FileTree`], are never followed). The ignore rules are
/// `FileTree`'s with ignored entries hidden: `.gitignore`, `.ignore` and
/// git's exclude files, and `.git` is never entered.
///
/// Building is split so the walk can run on a background thread while the
/// index answers queries over the paths it has so far: [`FinderIndex::walk`]
/// sends batches of paths, which go to [`FinderIndex::extend`], then
/// [`FinderIndex::finish`] marks the index complete. Batches from a
/// [`crate::ProjectWatcher`] keep it current through [`FinderIndex::apply`].
pub struct FinderIndex {
    /// Canonical, as file watchers report paths.
    root: PathBuf,
    /// The root as given to [`FinderIndex::new`], also accepted by `apply`.
    given_root: PathBuf,
    items: Vec<Item>,
    /// Each item's index in `items`, by [`Item::key`]: in path order, so an
    /// empty query and a directory's removal are range scans.
    keys: BTreeMap<Box<[u8]>, u32>,
    complete: bool,
    /// Changes applied while building, applied again by `finish`: a batch
    /// the walk read before a change can arrive after it.
    pending: HashSet<FsChange>,
    stats: FinderStats,
    global: Arc<Gitignore>,
    /// The root directory's identity when it was last walked or found, or
    /// `None` while it is missing. A root found replaced, or back, is walked
    /// again.
    root_id: Option<Identity>,
}

impl FinderIndex {
    /// An empty index of the project at `root`, to be built with
    /// [`FinderIndex::extend`] and [`FinderIndex::finish`].
    pub fn new(root: impl AsRef<Path>) -> FinderIndex {
        let given_root = root.as_ref().to_path_buf();
        let root = fs::canonicalize(&given_root).unwrap_or_else(|_| given_root.clone());
        let mut index = FinderIndex {
            root,
            given_root,
            items: Vec::new(),
            keys: BTreeMap::new(),
            complete: false,
            pending: HashSet::new(),
            stats: FinderStats::default(),
            global: listing::global_excludes(),
            root_id: None,
        };
        index.root_id = index.find_root();
        index
    }

    /// The root's identity, if it can be reached.
    fn find_root(&self) -> Option<Identity> {
        let dirs = listing::open_dirs(&self.root, Access::Search).ok()?;
        listing::identity(dirs.last()?).ok()
    }

    /// A complete index of `paths`, relative and `/`-separated, with no file
    /// system behind it (for tests and benches).
    pub fn from_paths<P: AsRef<Path>>(paths: impl IntoIterator<Item = P>) -> FinderIndex {
        let mut index = FinderIndex::new("");
        index.global = Arc::new(Gitignore::empty());
        for path in paths {
            index.insert(path.as_ref());
        }
        index.complete = true;
        index
    }

    /// The canonical root directory.
    pub fn root_path(&self) -> &Path {
        &self.root
    }

    /// Walks `root` and hands its non-ignored files to `sink` in batches, as
    /// paths relative to `root`, for [`FinderIndex::extend`]. Returns whether
    /// the walk ran to the end; it stops early once `cancel` is set.
    ///
    /// Runs the `ignore` crate's parallel walker, whose threads are scoped to
    /// this call. `sink` is called on the calling thread, which the caller
    /// owns: typically a background thread that sends the batches on to the
    /// index.
    ///
    /// Known limits of that walker, which lacks `FileTree`'s file system
    /// protections (a handle-based walker is follow-up T91). They hold for
    /// this walk and for the walks [`FinderIndex::apply`] makes of new
    /// directories and of the root:
    ///
    /// - Ignore files are opened with a plain `File::open`, which follows
    ///   symlinks: a symlinked `.gitignore` or `.ignore` is obeyed, where
    ///   `FileTree` skips it.
    /// - An ignore file that is a FIFO blocks the walk until something
    ///   writes to it, and `cancel` isn't seen meanwhile. A walk from `apply`
    ///   then blocks its caller.
    /// - Directories are listed by path. A directory swapped for a symlink
    ///   while the walk is under way can lead it outside the project, and
    ///   the files there are indexed.
    pub fn walk(
        root: impl AsRef<Path>,
        cancel: Arc<AtomicBool>,
        sink: impl FnMut(Vec<PathBuf>),
    ) -> bool {
        let root = root.as_ref();
        walk_dir(root, root, &cancel, sink)
    }

    /// Adds a batch of paths, relative to the root, from
    /// [`FinderIndex::walk`]. Paths already indexed are skipped.
    pub fn extend(&mut self, batch: impl IntoIterator<Item = PathBuf>) {
        for path in batch {
            self.insert(&path);
        }
    }

    /// Marks the index complete, once the walk's last batch is in. Changes
    /// [`FinderIndex::apply`] got while building are applied again, since a
    /// batch read before a change may have arrived after it; a
    /// [`FsChange::Rescan`] among them walks the whole root, here.
    pub fn finish(&mut self) {
        if self.complete {
            return;
        }
        self.complete = true;
        let pending: Vec<FsChange> = std::mem::take(&mut self.pending).into_iter().collect();
        if !pending.is_empty() {
            self.apply(&pending);
        }
    }

    /// How many paths are indexed, and whether the index is complete.
    pub fn progress(&self) -> (usize, bool) {
        (self.items.len(), self.complete)
    }

    pub fn stats(&self) -> FinderStats {
        self.stats
    }

    /// The best `limit` paths for `text`, best first: by score, then shorter
    /// path (in chars), then path order. A match within the file name beats
    /// any match that spans directories. Matching ignores case, and every
    /// character of `text` must appear in the path, in order.
    ///
    /// Runs over the paths indexed so far. Only the results' match indices
    /// are computed. An empty `text` gives the first `limit` paths in path
    /// order.
    pub fn query(&mut self, text: &str, limit: usize) -> Vec<FinderMatch> {
        if limit == 0 {
            return Vec::new();
        }
        if text.is_empty() {
            return self
                .keys
                .values()
                .take(limit)
                .map(|&item| self.result(item, 0, Vec::new()))
                .collect();
        }
        // Results are ranked here, so frizbee needn't sort.
        let config = Config::default()
            .casing(CaseMatching::Ignore)
            .sort(SortStrategy::IndexAsc);
        let mut matcher = Matcher::new(text, &config);
        let mut candidates = Vec::new();
        // A file name never holds a `/`.
        if !text.contains('/') {
            let names: Vec<&str> = self.items.iter().map(Item::file_name).collect();
            candidates.extend(matcher.match_list(&names).into_iter().map(|m| Candidate {
                score: NAME_TIER + u32::from(m.score),
                item: m.index,
                in_name: true,
            }));
        }
        // With enough file name matches, nothing else can make the cut.
        if candidates.len() < limit {
            let named: HashSet<u32> = candidates.iter().map(|c| c.item).collect();
            candidates.extend(
                matcher
                    .match_list(&self.items)
                    .into_iter()
                    .filter(|m| !named.contains(&m.index))
                    .map(|m| Candidate {
                        score: u32::from(m.score),
                        item: m.index,
                        in_name: false,
                    }),
            );
        }
        let rank = |a: &Candidate, b: &Candidate| {
            let (x, y) = (&self.items[a.item as usize], &self.items[b.item as usize]);
            b.score
                .cmp(&a.score)
                .then(x.chars.cmp(&y.chars))
                .then_with(|| x.order().cmp(&y.order()))
        };
        if candidates.len() > limit {
            candidates.select_nth_unstable_by(limit - 1, rank);
            candidates.truncate(limit);
        }
        candidates.sort_unstable_by(rank);
        candidates
            .into_iter()
            .map(|candidate| {
                let indices = self.indices(&mut matcher, text, candidate);
                self.result(candidate.item, candidate.score, indices)
            })
            .collect()
    }

    /// The match indices of a candidate, as ascending char indices into its
    /// path, one for each char of `text`.
    ///
    /// frizbee gives descending byte offsets, one for each byte of a
    /// multi-byte char. When its alignment is cut short (it can be, for a
    /// match spread over a long path), the first chars that spell `text` are
    /// taken instead, matched as frizbee matches them.
    fn indices(&mut self, matcher: &mut Matcher, text: &str, candidate: Candidate) -> Vec<usize> {
        self.stats.indices_computed += 1;
        let item = &self.items[candidate.item as usize];
        let (haystack, offset) = if candidate.in_name {
            (item.file_name(), item.name as usize)
        } else {
            (item.path.as_str(), 0)
        };
        let bytes = matcher
            .match_one_indices(haystack, candidate.item)
            .map(|found| found.indices)
            .unwrap_or_default();
        let chars = char_indices(&item.path, bytes.iter().map(|&byte| offset + byte as usize));
        if spells(&item.path, &chars, text) {
            return chars;
        }
        let skip = item.path[..offset].chars().count();
        let mut needle = text.chars().peekable();
        let mut greedy = Vec::new();
        for (ix, ch) in item.path.chars().enumerate().skip(skip) {
            if needle.next_if(|&want| case_matches(want, ch)).is_some() {
                greedy.push(ix);
            }
        }
        // frizbee matched it, so the whole needle is found; if not, frizbee's
        // alignment is the best there is.
        if needle.peek().is_none() {
            greedy
        } else {
            chars
        }
    }

    fn result(&self, item: u32, score: u32, indices: Vec<usize>) -> FinderMatch {
        let item = &self.items[item as usize];
        FinderMatch {
            path: item.path.clone(),
            relative: item
                .real
                .as_deref()
                .map_or_else(|| PathBuf::from(&item.path), Path::to_path_buf),
            score,
            indices,
        }
    }

    /// Reconciles the index with the disk for a batch of changes, as
    /// [`crate::FileTree::apply`] does for the tree.
    ///
    /// Each changed path is looked up without following symlinks, through
    /// directory handles opened a component at a time from the root, with the
    /// ignore files of each read through them:
    ///
    /// - an existing, non-ignored file is added (or kept);
    /// - a missing, ignored or misspelled path (a case-only rename), or a
    ///   symlink, is removed, with every indexed path under it;
    /// - a path below a directory that is missing, misspelled or not a
    ///   directory any more removes that directory's indexed paths;
    /// - a directory, non-ignored, is walked (only it), after removing what
    ///   was indexed under it. It may be new, another directory put in place
    ///   of an indexed one, or one back from elsewhere with changes reported
    ///   under another name. A directory under one walked in the same batch
    ///   isn't walked again.
    ///
    /// The root itself is checked on every batch: once it is missing the
    /// index is emptied, and once it is back, or found replaced by another
    /// directory, it is walked again, as for [`FsChange::Rescan`]. Nothing
    /// else walks the whole root. While building (before
    /// [`FinderIndex::finish`]) the changes are applied, and kept to be
    /// applied again by `finish`. A whole-root walk is only kept, so `finish`
    /// then walks the root on its caller's thread.
    ///
    /// Known limits, repaired by a `Rescan`:
    ///
    /// - An edit to an ignore file changes only that file's own entry: the
    ///   paths it now ignores, or no longer ignores, stay as they were (as in
    ///   `FileTree`). Re-walking its directory would re-walk the whole root
    ///   for the root's `.gitignore`.
    /// - The walk and the disk race: a path that changes while a walk reads
    ///   its directory can be indexed as it was, until its next event.
    /// - Its walks share [`FinderIndex::walk`]'s known limits: symlinked and
    ///   FIFO ignore files, and directories swapped for symlinks mid-walk.
    pub fn apply(&mut self, changes: &[FsChange]) {
        if !self.complete {
            self.pending.extend(changes.iter().cloned());
        }
        if changes
            .iter()
            .any(|change| matches!(change, FsChange::Rescan))
        {
            self.rescan_when_complete();
            return;
        }
        let mut chain = match Chain::open(&self.root) {
            Ok(chain) => chain,
            Err(err) => {
                if listing::is_absent(&err) {
                    // Gone, perhaps only for a moment: walked again once back.
                    self.items.clear();
                    self.keys.clear();
                    self.root_id = None;
                }
                return;
            }
        };
        if self.root_id != Some(chain.id) {
            self.rescan_when_complete();
            return;
        }
        let mut paths: Vec<PathBuf> = changes
            .iter()
            .filter_map(|change| match change {
                FsChange::Path(path) => self.relative(path),
                FsChange::Rescan => None,
            })
            .collect();
        // In order, so a directory's paths come together, after it.
        paths.sort();
        paths.dedup();
        let mut walked: Option<PathBuf> = None;
        for rel in paths {
            // A walk in this batch saw everything in it at least as late.
            if walked.as_ref().is_some_and(|dir| rel.starts_with(dir)) {
                continue;
            }
            if let Some(dir) = self.apply_path(&mut chain, &rel) {
                walked = Some(dir);
            }
        }
    }

    /// Reconciles one path, relative to the root and not empty. Returns it if
    /// it was a directory and was walked.
    fn apply_path(&mut self, chain: &mut Chain, rel: &Path) -> Option<PathBuf> {
        let names: Vec<&OsStr> = rel.iter().collect();
        let (name, parents) = names.split_last().expect("not the root");
        match chain.reach(&self.root, parents, &self.global) {
            Reached::Yes => {}
            Reached::Gone(depth) => {
                let gone: PathBuf = names[..=depth].iter().collect();
                self.remove(&gone);
                return None;
            }
            Reached::Unknown => return None,
        }
        let level = chain.top();
        let stat = match listing::stat_at(level.dir, name) {
            Ok(stat) => stat,
            Err(err) => {
                if listing::is_absent(&err) {
                    self.remove(rel);
                }
                return None;
            }
        };
        let is_dir = stat.kind == EntryKind::Dir;
        let ignored = level.ignored
            || level
                .rules
                .is_some_and(|rules| rules.ignores(&self.global, &self.root.join(rel), is_dir));
        if !stat.exact || ignored {
            self.remove(rel);
            return None;
        }
        match stat.kind {
            EntryKind::File => {
                self.remove_under(rel);
                self.insert(rel);
                None
            }
            EntryKind::Symlink => {
                self.remove(rel);
                None
            }
            EntryKind::Dir => {
                self.remove(rel);
                let root = self.root.clone();
                walk_dir(&root, &root.join(rel), &AtomicBool::new(false), |batch| {
                    self.extend(batch)
                });
                self.stats.walks += 1;
                Some(rel.to_path_buf())
            }
        }
    }

    /// Walks the root again now, or, while building, in `finish`.
    fn rescan_when_complete(&mut self) {
        if self.complete {
            self.rescan();
        } else {
            self.pending.insert(FsChange::Rescan);
        }
    }

    /// Drops everything and walks the root again.
    fn rescan(&mut self) {
        self.items.clear();
        self.keys.clear();
        self.root_id = self.find_root();
        let root = self.root.clone();
        walk_dir(&root, &root, &AtomicBool::new(false), |batch| {
            self.extend(batch)
        });
        self.stats.walks += 1;
    }

    /// `path` relative to the root, if it is under it, isn't the root, and
    /// isn't in `.git`.
    fn relative(&self, path: &Path) -> Option<PathBuf> {
        let rel = if path.is_relative() {
            path
        } else {
            path.strip_prefix(&self.root)
                .or_else(|_| path.strip_prefix(&self.given_root))
                .ok()?
        };
        let mut clean = PathBuf::new();
        for component in rel.components() {
            match component {
                Component::Normal(name) if !listing::is_git_dir(name) => clean.push(name),
                _ => return None,
            }
        }
        (!clean.as_os_str().is_empty()).then_some(clean)
    }

    fn insert(&mut self, rel: &Path) {
        let key = key_of(rel);
        if self.keys.contains_key(&key) {
            return;
        }
        self.keys.insert(key, to_u32(self.items.len()));
        self.items.push(Item::new(rel));
    }

    /// Removes `rel` and every path under it.
    fn remove(&mut self, rel: &Path) {
        self.remove_key(&key_of(rel));
        self.remove_under(rel);
    }

    /// Removes every path under directory `rel`.
    fn remove_under(&mut self, rel: &Path) {
        // Lossy conversion keeps a `/` and what comes before it, so every
        // path under `rel` reads `rel` lossily, then `/`. Other directories
        // that read the same are told apart by the real bytes.
        let mut real = rel.as_os_str().as_bytes().to_vec();
        real.push(b'/');
        let mut low = rel.as_os_str().to_string_lossy().into_owned().into_bytes();
        low.push(b'/');
        let mut high = low.clone();
        *high.last_mut().unwrap() = b'/' + 1;
        let under: Vec<Box<[u8]>> = self
            .keys
            .range::<[u8], _>((Bound::Included(&low[..]), Bound::Excluded(&high[..])))
            .filter(|&(_, &ix)| self.items[ix as usize].real_bytes().starts_with(&real))
            .map(|(key, _)| key.clone())
            .collect();
        for key in under {
            self.remove_key(&key);
        }
    }

    fn remove_key(&mut self, key: &[u8]) {
        let Some(ix) = self.keys.remove(key) else {
            return;
        };
        let ix = ix as usize;
        self.items.swap_remove(ix);
        if let Some(moved) = self.items.get(ix) {
            *self.keys.get_mut(&moved.key()).expect("indexed") = to_u32(ix);
        }
    }
}

/// Walks `dir`, under `root`, handing its files to `sink` as paths relative
/// to `root`. Returns whether the walk ran to the end.
fn walk_dir(
    root: &Path,
    dir: &Path,
    cancel: &AtomicBool,
    mut sink: impl FnMut(Vec<PathBuf>),
) -> bool {
    if cancel.load(Ordering::Relaxed) {
        return false;
    }
    let (tx, rx) = mpsc::channel();
    let walker = listing::walk_builder(dir).build_parallel();
    thread::scope(|scope| {
        scope.spawn(|| {
            walker.run(|| {
                let mut worker = Worker {
                    root,
                    batch: Vec::new(),
                    tx: tx.clone(),
                    cancel,
                };
                Box::new(move |entry| worker.visit(entry))
            });
            drop(tx);
        });
        for batch in rx {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            sink(batch);
        }
    });
    !cancel.load(Ordering::Relaxed)
}

/// One walker thread's state. Sends what it has left when dropped.
struct Worker<'a> {
    root: &'a Path,
    batch: Vec<PathBuf>,
    tx: mpsc::Sender<Vec<PathBuf>>,
    cancel: &'a AtomicBool,
}

impl Worker<'_> {
    fn visit(&mut self, entry: Result<ignore::DirEntry, ignore::Error>) -> WalkState {
        if self.cancel.load(Ordering::Relaxed) {
            return WalkState::Quit;
        }
        // Unreadable entries are skipped, as the tree shows nothing for them.
        let Ok(entry) = entry else {
            return WalkState::Continue;
        };
        let is_file = entry
            .file_type()
            .is_some_and(|kind| !kind.is_dir() && !kind.is_symlink());
        if let (true, Ok(rel)) = (is_file, entry.path().strip_prefix(self.root)) {
            self.batch.push(rel.to_path_buf());
            if self.batch.len() >= BATCH {
                let _ = self.tx.send(std::mem::take(&mut self.batch));
            }
        }
        WalkState::Continue
    }
}

impl Drop for Worker<'_> {
    fn drop(&mut self) {
        if !self.batch.is_empty() && !self.cancel.load(Ordering::Relaxed) {
            let _ = self.tx.send(std::mem::take(&mut self.batch));
        }
    }
}

/// One directory open on the way to a changed path.
struct Level {
    name: OsString,
    dir: File,
    /// `None` below an ignored directory, where nothing is read.
    rules: Option<Arc<Rules>>,
    ignored: bool,
}

/// What [`FinderIndex::apply`] has open: the root, and the directories from
/// it to the last changed path's parent. Changes come sorted, so the next one
/// usually shares most of the way.
struct Chain {
    /// The root's identity.
    id: Identity,
    root: Level,
    below: Vec<Level>,
}

/// A borrowed view of a [`Level`].
struct Top<'a> {
    dir: &'a File,
    rules: Option<&'a Arc<Rules>>,
    ignored: bool,
}

enum Reached {
    Yes,
    /// The directory at this depth (0 for the root's child) is missing,
    /// misspelled or not a directory.
    Gone(usize),
    /// It couldn't be told (say, no permission).
    Unknown,
}

impl Chain {
    /// Opens the root. The error says whether it is missing.
    fn open(root: &Path) -> io::Result<Chain> {
        let mut dirs =
            listing::open_dirs(root, Access::Search).map_err(|unreached| unreached.err)?;
        let rules = Rules::chain(&dirs, root);
        let dir = dirs.pop().expect("`/` is always opened");
        Ok(Chain {
            id: listing::identity(&dir)?,
            root: Level {
                name: OsString::new(),
                dir,
                rules,
                ignored: false,
            },
            below: Vec::new(),
        })
    }

    fn top(&self) -> Top<'_> {
        let level = self.below.last().unwrap_or(&self.root);
        Top {
            dir: &level.dir,
            rules: level.rules.as_ref(),
            ignored: level.ignored,
        }
    }

    /// Opens the directories `names`, from the root down, keeping those
    /// already open on the way.
    fn reach(&mut self, root: &Path, names: &[&OsStr], global: &Gitignore) -> Reached {
        let common = self
            .below
            .iter()
            .zip(names)
            .take_while(|(level, name)| level.name == **name)
            .count();
        self.below.truncate(common);
        let mut path: PathBuf = root.join(names[..common].iter().collect::<PathBuf>());
        for (depth, &name) in names.iter().enumerate().skip(common) {
            path.push(name);
            let top = self.top();
            // On a case-insensitive volume the old spelling still opens.
            match listing::stat_at(top.dir, name) {
                Ok(stat) if stat.exact && stat.kind == EntryKind::Dir => {}
                Ok(_) => return Reached::Gone(depth),
                Err(err) if listing::is_absent(&err) => return Reached::Gone(depth),
                Err(_) => return Reached::Unknown,
            }
            let dir = match listing::open_at(top.dir, name, Access::Search) {
                Ok(dir) => dir,
                Err(err) if listing::is_absent(&err) => return Reached::Gone(depth),
                Err(_) => return Reached::Unknown,
            };
            let ignored = top.ignored
                || top
                    .rules
                    .is_some_and(|rules| rules.ignores(global, &path, true));
            let rules = if ignored {
                None
            } else {
                Some(Rules::load(top.rules.cloned(), &dir, &path))
            };
            self.below.push(Level {
                name: name.to_os_string(),
                dir,
                rules,
                ignored,
            });
        }
        Reached::Yes
    }
}
