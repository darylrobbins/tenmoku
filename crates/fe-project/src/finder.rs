//! Every non-ignored file path of a project, indexed up front and matched
//! fuzzily, for Cmd-P.

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    ffi::{OsStr, OsString},
    fs::File,
    io,
    num::NonZero,
    ops::Bound,
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};

use frizbee::{CaseMatching, Config, Matcher, SortStrategy};
use ignore::gitignore::Gitignore;

use crate::{
    FsChange,
    listing::{self, Access, EntryKind, Identity, Rules},
};

/// Paths a walk thread collects before sending them on.
const BATCH: usize = 1024;

/// Batches a walk keeps queued for its sink. Once that many wait, a walk
/// thread with another blocks until the sink takes one.
pub(crate) const QUEUED: usize = 2;

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

/// What a [`FinderIndex::apply`] or [`FinderIndex::finish`] changed in the
/// index: paths relative to the root, sorted, without duplicates.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileSetDelta {
    /// Paths indexed after the change that it added or added again: those
    /// the batch named, and those found walking a directory it named. A
    /// modified file is added again, so it is here.
    pub present: Vec<PathBuf>,
    /// Paths that were indexed before the change and aren't after it.
    pub removed: Vec<PathBuf>,
    /// Whether the whole root was walked again, for a [`FsChange::Rescan`]
    /// or a root found replaced. `present` then holds every path indexed.
    pub rescanned: bool,
}

/// What `insert` and `remove_key` record while a batch is applied, until
/// it's turned into a [`FileSetDelta`].
#[derive(Debug, Default)]
struct Recorded {
    on: bool,
    present: Vec<PathBuf>,
    removed: Vec<PathBuf>,
    /// The keys added while recording: removing one again isn't reported,
    /// since it wasn't indexed before.
    added: HashSet<Box<[u8]>>,
    rescanned: bool,
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
            Some(real) => real.as_os_str().as_encoded_bytes(),
            None => self.path.as_bytes(),
        }
    }

    /// Path order: by `path`, then, for paths that read the same, by the real
    /// bytes, a lossless path first.
    fn order(&self) -> (&str, Option<&[u8]>) {
        (
            &self.path,
            self.real
                .as_deref()
                .map(|real| real.as_os_str().as_encoded_bytes()),
        )
    }

    /// Its key in [`FinderIndex::keys`].
    fn key(&self) -> Box<[u8]> {
        make_key(&self.path, self.order().1)
    }

    fn file_name(&self) -> &str {
        &self.path[self.name as usize..]
    }

    /// The real relative path.
    fn rel(&self) -> PathBuf {
        // SAFETY: the real bytes are a whole `OsStr`'s encoded bytes, or a
        // `str`'s, as `real_bytes` takes them.
        PathBuf::from(unsafe { OsStr::from_encoded_bytes_unchecked(self.real_bytes()) })
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

/// `root` as [`dunce::canonicalize`] resolves it, done once for
/// [`FinderIndex::new`]. A root that is missing, wholly or in part, still gets
/// an absolute path, which later walks reach without following symlinks: its
/// longest existing ancestor resolved, then the rest of it as given, where a
/// `..` goes up a level (and what that reaches is resolved again, if it
/// exists). An empty path, as [`FinderIndex::from_paths`] uses, stays empty.
fn resolve_root(root: &Path) -> PathBuf {
    let Ok(absolute) = std::path::absolute(root) else {
        return root.to_path_buf();
    };
    let mut missing = Vec::new();
    let mut at = absolute.as_path();
    let mut resolved = loop {
        if let Ok(resolved) = dunce::canonicalize(at) {
            break resolved;
        }
        match (at.parent(), at.components().next_back()) {
            (Some(parent), Some(last)) => {
                missing.push(last);
                at = parent;
            }
            // `/` itself wouldn't resolve.
            _ => return absolute,
        }
    };
    for component in missing.into_iter().rev() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(name) => resolved.push(name),
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
        if let Ok(found) = dunce::canonicalize(&resolved) {
            resolved = found;
        }
    }
    resolved
}

/// The key of relative path `rel`.
fn key_of(rel: &Path) -> Box<[u8]> {
    match rel.as_os_str().to_string_lossy() {
        Cow::Borrowed(path) => make_key(path, None),
        Cow::Owned(path) => make_key(&path, Some(rel.as_os_str().as_encoded_bytes())),
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

/// A query word as VS Code's quick open reads it: `\` is `/`, and `*`, `…`
/// and `"` are dropped. `None` when nothing is left. The flag is whether the
/// raw word was wrapped in `"`, which asks for its text unbroken.
fn normalize_word(word: &str) -> Option<(String, bool)> {
    let exact = word.chars().count() >= 2 && word.starts_with('"') && word.ends_with('"');
    let word: String = word
        .chars()
        .filter(|&c| c != '*' && c != '\u{2026}' && c != '"')
        .map(|c| if c == '\\' { '/' } else { c })
        .collect();
    (!word.is_empty()).then_some((word, exact))
}

/// The char index in `haystack` where `text` first appears unbroken, ignoring
/// case as [`case_matches`] does.
fn unbroken(haystack: &str, text: &str) -> Option<usize> {
    let hay: Vec<char> = haystack.chars().collect();
    let want: Vec<char> = text.chars().collect();
    if want.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - want.len()).find(|&at| {
        want.iter()
            .zip(&hay[at..])
            .all(|(&w, &c)| case_matches(w, c))
    })
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
    /// Canonical, as file watchers report paths: resolved once, by
    /// [`resolve_root`].
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
    /// What `apply` changed, kept while building for `finish`.
    recorded: Recorded,
}

impl FinderIndex {
    /// An empty index of the project at `root`, to be built with
    /// [`FinderIndex::extend`] and [`FinderIndex::finish`].
    pub fn new(root: impl AsRef<Path>) -> FinderIndex {
        let given_root = root.as_ref().to_path_buf();
        let root = resolve_root(&given_root);
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
            recorded: Recorded::default(),
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

    /// The canonical root directory (as it will be, if it was missing when
    /// the index was made).
    pub fn root_path(&self) -> &Path {
        &self.root
    }

    /// Walks `root` and hands its non-ignored files to `sink` in batches, as
    /// paths relative to `root`, for [`FinderIndex::extend`]. Returns whether
    /// the walk ran to the end; it stops early once `cancel` is set.
    ///
    /// Walks with `FileTree`'s file system protections, as do the walks
    /// [`FinderIndex::apply`] makes of new directories and of the root.
    /// `root`, which may be relative or reached through symlinks, is
    /// resolved once, here; [`FinderIndex::apply`] uses the root resolved by
    /// [`FinderIndex::new`] and doesn't resolve it again, so a root since
    /// swapped for a symlink has nothing to walk. The root is opened a
    /// component at a time from `/`, and each directory below it through
    /// its parent's handle, never following a symlink: a directory swapped
    /// for a symlink while the walk is under way isn't followed out of the
    /// project. Ignore files are read through the same handles, only when
    /// they are regular files, and opened without following a symlink or
    /// waiting on a FIFO. Each directory's rules are read once, on top of
    /// its parent's. The walk keeps a bounded number of directories open,
    /// however deep the tree.
    ///
    /// The walk runs on threads scoped to this call. `sink` is called on the
    /// calling thread, which the caller owns: typically a background thread
    /// that sends the batches on to the index.
    ///
    /// A slow sink holds the walk up: at most two batches wait for it, and
    /// a walk thread with another full batch blocks until it takes one. So
    /// besides the batch `sink` has, what's listed and waiting is at most
    /// two batches, one blocked per thread, and each thread's batch being
    /// filled. Once `cancel` is set, the walk ends as soon as `sink`
    /// returns; a thread blocked meanwhile gives up then. A panic in `sink`
    /// stops the walk's threads too, before it reaches the caller.
    pub fn walk(
        root: impl AsRef<Path>,
        cancel: Arc<AtomicBool>,
        sink: impl FnMut(Vec<PathBuf>),
    ) -> bool {
        walk_root(root.as_ref(), &cancel, &Walked::default(), sink)
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
    ///
    /// Returns what those changes did to the index, with what applying
    /// them while building did (which `apply` didn't report then); once
    /// complete, an empty delta.
    pub fn finish(&mut self) -> FileSetDelta {
        if self.complete {
            return FileSetDelta::default();
        }
        self.complete = true;
        let pending: Vec<FsChange> = std::mem::take(&mut self.pending).into_iter().collect();
        if pending.is_empty() {
            return self.take_delta();
        }
        self.apply(&pending)
    }

    /// Whether `rel`, relative to the root, is indexed.
    pub fn contains(&self, rel: impl AsRef<Path>) -> bool {
        self.keys.contains_key(&key_of(rel.as_ref()))
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
    /// character of a word must appear in the path, in order.
    ///
    /// `text` splits on whitespace into words, as VS Code's quick open
    /// splits its query. A path matches when every word matches it, and its
    /// score is the saturating sum of the words' scores; its indices are the
    /// union of the words'. No word (empty or whitespace-only `text`) gives
    /// the first `limit` paths in path order.
    ///
    /// Runs over the paths indexed so far. Only the results' match indices
    /// are computed.
    pub fn query(&mut self, text: &str, limit: usize) -> Vec<FinderMatch> {
        if limit == 0 {
            return Vec::new();
        }
        let normalized: Vec<(String, bool)> =
            text.split_whitespace().filter_map(normalize_word).collect();
        let words: Vec<(&str, bool)> = normalized.iter().map(|(w, e)| (w.as_str(), *e)).collect();
        if words.is_empty() {
            return self
                .keys
                .values()
                .take(limit)
                .map(|&item| self.result(item, 0, Vec::new()))
                .collect();
        }
        let mut matchers: Vec<Matcher> =
            words.iter().map(|&(word, _)| word_matcher(word)).collect();
        let rank = |a: &Candidate, b: &Candidate| {
            let (x, y) = (&self.items[a.item as usize], &self.items[b.item as usize]);
            b.score
                .cmp(&a.score)
                .then(x.chars.cmp(&y.chars))
                .then_with(|| x.order().cmp(&y.order()))
        };
        if let [(word, exact)] = words[..] {
            // With enough file name matches, nothing else can make the cut.
            let mut candidates = self.word_candidates(&mut matchers[0], word, exact, limit);
            if candidates.len() > limit {
                candidates.select_nth_unstable_by(limit - 1, rank);
                candidates.truncate(limit);
            }
            candidates.sort_unstable_by(rank);
            return candidates
                .into_iter()
                .map(|candidate| {
                    let indices = self.indices(&mut matchers[0], word, exact, candidate);
                    self.result(candidate.item, candidate.score, indices)
                })
                .collect();
        }
        // item -> (summed score, whether each word matched in the file name)
        let mut joint: HashMap<u32, (u32, Vec<bool>)> = HashMap::new();
        for (ix, &(word, exact)) in words.iter().enumerate() {
            let found = self.word_candidates(&mut matchers[ix], word, exact, usize::MAX);
            if ix == 0 {
                joint.extend(found.iter().map(|c| (c.item, (c.score, vec![c.in_name]))));
                continue;
            }
            let found: HashMap<u32, Candidate> = found.into_iter().map(|c| (c.item, c)).collect();
            joint.retain(|item, (score, in_name)| match found.get(item) {
                Some(c) => {
                    *score = score.saturating_add(c.score);
                    in_name.push(c.in_name);
                    true
                }
                None => false,
            });
        }
        let mut candidates: Vec<Candidate> = joint
            .iter()
            .map(|(&item, &(score, _))| Candidate {
                score,
                item,
                in_name: false,
            })
            .collect();
        if candidates.len() > limit {
            candidates.select_nth_unstable_by(limit - 1, rank);
            candidates.truncate(limit);
        }
        candidates.sort_unstable_by(rank);
        candidates
            .into_iter()
            .map(|candidate| {
                let mut indices = Vec::new();
                for (ix, &(word, exact)) in words.iter().enumerate() {
                    let one = Candidate {
                        in_name: joint[&candidate.item].1[ix],
                        ..candidate
                    };
                    indices.extend(self.indices(&mut matchers[ix], word, exact, one));
                }
                indices.sort_unstable();
                indices.dedup();
                self.result(candidate.item, candidate.score, indices)
            })
            .collect()
    }

    /// The paths that match one `word`: those whose file name does (unless
    /// the word holds a `/`), then, if fewer than `limit` of those, the
    /// others that match across the path.
    ///
    /// An `exact` word keeps only the paths that hold its text unbroken, in
    /// the file name for the first tier, in the path for the second.
    fn word_candidates(
        &self,
        matcher: &mut Matcher,
        word: &str,
        exact: bool,
        limit: usize,
    ) -> Vec<Candidate> {
        let keeps = |haystack: &str| !exact || unbroken(haystack, word).is_some();
        let mut candidates = Vec::new();
        // A file name never holds a `/`.
        if !word.contains('/') {
            let names: Vec<&str> = self.items.iter().map(Item::file_name).collect();
            candidates.extend(
                matcher
                    .match_list(&names)
                    .into_iter()
                    .filter(|m| keeps(names[m.index as usize]))
                    .map(|m| Candidate {
                        score: NAME_TIER + u32::from(m.score),
                        item: m.index,
                        in_name: true,
                    }),
            );
        }
        if candidates.len() < limit {
            let named: HashSet<u32> = candidates.iter().map(|c| c.item).collect();
            candidates.extend(
                matcher
                    .match_list(&self.items)
                    .into_iter()
                    .filter(|m| !named.contains(&m.index))
                    .filter(|m| keeps(&self.items[m.index as usize].path))
                    .map(|m| Candidate {
                        score: u32::from(m.score),
                        item: m.index,
                        in_name: false,
                    }),
            );
        }
        candidates
    }

    /// The match indices of a candidate, as ascending char indices into its
    /// path, one for each char of `text`.
    ///
    /// frizbee gives descending byte offsets, one for each byte of a
    /// multi-byte char. When its alignment is cut short (it can be, for a
    /// match spread over a long path), the first chars that spell `text` are
    /// taken instead, matched as frizbee matches them.
    ///
    /// For an `exact` word, the first unbroken run of `text` in the tier's
    /// haystack.
    fn indices(
        &mut self,
        matcher: &mut Matcher,
        text: &str,
        exact: bool,
        candidate: Candidate,
    ) -> Vec<usize> {
        self.stats.indices_computed += 1;
        let item = &self.items[candidate.item as usize];
        let (haystack, offset) = if candidate.in_name {
            (item.file_name(), item.name as usize)
        } else {
            (item.path.as_str(), 0)
        };
        if exact && let Some(at) = unbroken(haystack, text) {
            let start = item.path[..offset].chars().count() + at;
            return (start..start + text.chars().count()).collect();
        }
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
    ///
    /// Returns what the batch changed in the index. While building, that
    /// is an empty delta: [`FinderIndex::finish`] reports it instead.
    pub fn apply(&mut self, changes: &[FsChange]) -> FileSetDelta {
        self.recorded.on = true;
        self.apply_changes(changes);
        self.recorded.on = false;
        if !self.complete {
            return FileSetDelta::default();
        }
        self.take_delta()
    }

    /// What was recorded since the last delta, as a delta.
    fn take_delta(&mut self) -> FileSetDelta {
        let Recorded {
            mut present,
            mut removed,
            rescanned,
            ..
        } = std::mem::take(&mut self.recorded);
        present.sort();
        present.dedup();
        present.retain(|rel| self.contains(rel));
        removed.sort();
        removed.dedup();
        removed.retain(|rel| !self.contains(rel));
        FileSetDelta {
            present,
            removed,
            rescanned,
        }
    }

    fn apply_changes(&mut self, changes: &[FsChange]) {
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
                    self.clear();
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
                // What is there now says what it is: a file that replaced a
                // directory stays, and what was under the old one goes.
                return self.apply_path(chain, &gone);
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
                // Opened through its parent's handle: gone, or replaced by a
                // symlink, since it was looked up, it has nothing to walk.
                let dir = listing::open_at(level.dir, name, Access::Read).ok()?;
                let start = Start {
                    dir,
                    rel: rel.to_path_buf(),
                    rules: level.rules.cloned(),
                };
                let (root, global) = (self.root.clone(), self.global.clone());
                let never = AtomicBool::new(false);
                walk_dir(&root, start, &global, &never, &Walked::default(), |batch| {
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
        self.clear();
        self.recorded.rescanned = true;
        // The root as found (not resolved again): one replaced by a symlink
        // isn't followed, and has nothing to walk.
        let start = Start::root(&self.root);
        self.root_id = start
            .as_ref()
            .and_then(|start| listing::identity(&start.dir).ok());
        if let Some(start) = start {
            let (root, global) = (self.root.clone(), self.global.clone());
            let never = AtomicBool::new(false);
            walk_dir(&root, start, &global, &never, &Walked::default(), |batch| {
                self.extend(batch)
            });
        }
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
        if self.recorded.on {
            self.recorded.present.push(rel.to_path_buf());
        }
        if self.keys.contains_key(&key) {
            // Handed over again by the build (`extend`) since a batch added
            // it: indexed before any later removal.
            if !self.recorded.on {
                self.recorded.added.remove(&key);
            }
            return;
        }
        if self.recorded.on {
            self.recorded.added.insert(key.clone());
        }
        self.keys.insert(key, to_u32(self.items.len()));
        self.items.push(Item::new(rel));
    }

    /// Removes every path.
    fn clear(&mut self) {
        if self.recorded.on {
            for item in &self.items {
                if !self.recorded.added.remove(&item.key()) {
                    self.recorded.removed.push(item.rel());
                }
            }
        }
        self.items.clear();
        self.keys.clear();
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
        // The separator relative paths are joined with: `\` on Windows.
        const SEP: u8 = std::path::MAIN_SEPARATOR as u8;
        let mut real = rel.as_os_str().as_encoded_bytes().to_vec();
        real.push(SEP);
        let mut low = rel.as_os_str().to_string_lossy().into_owned().into_bytes();
        low.push(SEP);
        let mut high = low.clone();
        *high.last_mut().unwrap() = SEP + 1;
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
        let item = self.items.swap_remove(ix);
        if self.recorded.on && !self.recorded.added.remove(key) {
            self.recorded.removed.push(item.rel());
        }
        if let Some(moved) = self.items.get(ix) {
            *self.keys.get_mut(&moved.key()).expect("indexed") = to_u32(ix);
        }
    }
}

/// What [`walk_root`] counts, for tests.
#[derive(Debug)]
pub(crate) struct Walked {
    /// Paths per batch.
    pub(crate) batch: usize,
    /// Files listed so far, sent on or not.
    pub(crate) listed: AtomicUsize,
}

impl Default for Walked {
    fn default() -> Walked {
        Walked::with_batch(BATCH)
    }
}

impl Walked {
    pub(crate) fn with_batch(batch: usize) -> Walked {
        Walked {
            batch,
            listed: AtomicUsize::new(0),
        }
    }
}

/// [`FinderIndex::walk`], in batches of `walked.batch` paths, counting the
/// files listed into `walked`.
pub(crate) fn walk_root(
    root: &Path,
    cancel: &AtomicBool,
    walked: &Walked,
    sink: impl FnMut(Vec<PathBuf>),
) -> bool {
    if cancel.load(Ordering::Relaxed) {
        return false;
    }
    // Resolved once, here: rules are read and matched by canonical paths, as
    // the tree's are. A root that can't be reached has nothing to walk.
    let Some(root) = dunce::canonicalize(root).ok() else {
        return true;
    };
    let Some(start) = Start::root(&root) else {
        return true;
    };
    walk_dir(
        &root,
        start,
        &listing::global_excludes(),
        cancel,
        walked,
        sink,
    )
}

/// Where a walk starts: a directory open for reading, its path relative to
/// the walk's root, and the rules in force in the directory above it.
struct Start {
    dir: File,
    rel: PathBuf,
    rules: Option<Arc<Rules>>,
}

impl Start {
    /// Opens directory `root`, absolute, a component at a time from `/`
    /// without following a symlink, with the rules of the directories above
    /// it. `None` if it can't be reached.
    fn root(root: &Path) -> Option<Start> {
        let mut dirs = listing::open_dirs(root, Access::Read).ok()?;
        let dir = dirs.pop().expect("`/` is always opened");
        let rules = root.parent().and_then(|parent| Rules::chain(&dirs, parent));
        Some(Start {
            dir,
            rel: PathBuf::new(),
            rules,
        })
    }
}

/// Walks `start`, under `root` (canonical), handing its files to `sink` as
/// paths relative to `root`. Returns whether the walk ran to the end.
///
/// Threads scoped to this call share a stack of directories still to read.
/// Each directory is opened through its parent's handle without following a
/// symlink, then its ignore files are read through it on top of its
/// parent's rules, and its entries read through it. `sink` is called on the
/// calling thread.
///
/// The threads send their batches over a channel bounded at [`QUEUED`], so
/// a slow sink stalls them. Once the walk is cancelled (checked whenever
/// `sink` returns) or `sink` panics, the calling thread drops its end and
/// stops the walk: a thread blocked sending fails, and the others stop at
/// their next entry.
///
/// A directory's handle is kept open while its subdirectories wait to be
/// read, but at most [`MAX_HELD`] are kept: past that, a subdirectory is
/// opened again from the start directory, a component at a time and still
/// without following a symlink, when its turn comes. So a deep tree doesn't
/// hold a descriptor per level.
fn walk_dir(
    root: &Path,
    start: Start,
    global: &Gitignore,
    cancel: &AtomicBool,
    walked: &Walked,
    mut sink: impl FnMut(Vec<PathBuf>),
) -> bool {
    if cancel.load(Ordering::Relaxed) {
        return false;
    }
    let threads = walk_threads();
    let held = Arc::new(AtomicUsize::new(0));
    let base = Arc::new(Held::new(start.dir, &held));
    let queue = Queue {
        state: Mutex::new(QueueState {
            stack: vec![Work {
                dir: Dir::Open(base.clone()),
                rel: start.rel.clone(),
                rules: start.rules,
            }],
            busy: 0,
        }),
        changed: Condvar::new(),
        stopped: AtomicBool::new(false),
    };
    let (tx, rx) = mpsc::sync_channel(QUEUED);
    thread::scope(|scope| {
        for _ in 0..threads {
            let mut walker = Walker {
                root,
                base: &base.file,
                base_rel: &start.rel,
                held: &held,
                global,
                cancel,
                queue: &queue,
                walked,
                batch: Vec::new(),
                tx: tx.clone(),
            };
            scope.spawn(move || walker.run());
        }
        drop(tx);
        // However the loop is left, by a cancel or a panic in `sink`, the
        // threads stop: those blocked sending fail, as the loop owns the
        // receiver, and the others stop at their next entry.
        let _stop = StopOnDrop(&queue);
        for batch in rx {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            sink(batch);
        }
    });
    !cancel.load(Ordering::Relaxed)
}

/// At most this many threads walk, as for the `ignore` crate's walker.
const MAX_THREADS: usize = 12;

/// How many threads a walk runs.
pub(crate) fn walk_threads() -> usize {
    thread::available_parallelism()
        .map_or(1, NonZero::get)
        .min(MAX_THREADS)
}

/// At most this many directory handles are kept open for subdirectories
/// waiting to be read (besides the start directory and those being read).
const MAX_HELD: usize = 32;

/// A directory to read.
struct Work {
    dir: Dir,
    /// Relative to the walk's root.
    rel: PathBuf,
    /// The rules in force in the directory above it.
    rules: Option<Arc<Rules>>,
}

enum Dir {
    /// Open for reading.
    Open(Arc<Held>),
    /// The entry of this name in an open directory, opened when its turn
    /// comes.
    At(Arc<Held>, OsString),
    /// Opened from the start directory when its turn comes, by its path.
    Below,
}

/// An open directory handle, counted while open.
struct Held {
    file: File,
    count: Arc<AtomicUsize>,
}

impl Held {
    fn new(file: File, count: &Arc<AtomicUsize>) -> Held {
        count.fetch_add(1, Ordering::Relaxed);
        Held {
            file,
            count: count.clone(),
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Stops a walk's threads when dropped.
struct StopOnDrop<'a>(&'a Queue);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.stop();
    }
}

/// The directories a walk has yet to read, shared by its threads.
struct Queue {
    state: Mutex<QueueState>,
    /// Signalled when work is added or the walk ends.
    changed: Condvar,
    /// Set when a thread panicked, so the others stop too and the panic
    /// reaches the caller rather than leaving them waiting, or when the
    /// calling thread stopped taking batches.
    stopped: AtomicBool,
}

impl Queue {
    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    /// Stops the walk: every thread finishes what it's reading, without
    /// listing more, and none takes another directory.
    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        // Under the lock, so a thread about to wait sees the flag first.
        let _state = lock(self);
        self.changed.notify_all();
    }
}

struct QueueState {
    stack: Vec<Work>,
    /// Threads reading a directory, which may add more.
    busy: usize,
}

/// One walk thread. Sends what it has left when dropped.
struct Walker<'a> {
    root: &'a Path,
    /// The start directory, and its path relative to `root`.
    base: &'a File,
    base_rel: &'a Path,
    held: &'a Arc<AtomicUsize>,
    global: &'a Gitignore,
    cancel: &'a AtomicBool,
    queue: &'a Queue,
    walked: &'a Walked,
    batch: Vec<PathBuf>,
    tx: mpsc::SyncSender<Vec<PathBuf>>,
}

/// Hands a read directory's subdirectories to the stack and marks its
/// thread idle, when dropped: also when reading it panicked, which stops
/// the walk.
struct Finished<'a> {
    queue: &'a Queue,
    found: Option<Vec<Work>>,
}

impl Drop for Finished<'_> {
    fn drop(&mut self) {
        let found = self.found.take();
        if found.is_none() {
            self.queue.stop();
        }
        let mut state = lock(self.queue);
        state.stack.extend(found.into_iter().flatten());
        state.busy -= 1;
        self.queue.changed.notify_all();
    }
}

fn lock(queue: &Queue) -> MutexGuard<'_, QueueState> {
    queue.state.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Walker<'_> {
    fn run(&mut self) {
        while let Some(work) = self.next() {
            let mut finished = Finished {
                queue: self.queue,
                found: None,
            };
            finished.found = Some(self.read(work));
        }
    }

    /// The next directory to read, waiting for one while others are being
    /// read, or `None` once there are none left or the walk is cancelled.
    fn next(&self) -> Option<Work> {
        let mut state = lock(self.queue);
        loop {
            if self.cancel.load(Ordering::Relaxed) || self.queue.stopped() {
                return None;
            }
            if let Some(work) = state.stack.pop() {
                state.busy += 1;
                return Some(work);
            }
            if state.busy == 0 {
                return None;
            }
            state = self
                .queue
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Opens a directory, without following a symlink anywhere on the way.
    fn open(&self, dir: Dir, rel: &Path) -> io::Result<Arc<Held>> {
        let file = match dir {
            Dir::Open(dir) => return Ok(dir),
            Dir::At(parent, name) => listing::open_at(&parent.file, &name, Access::Read)?,
            Dir::Below => {
                let names: Vec<&OsStr> = rel
                    .strip_prefix(self.base_rel)
                    .expect("under the start directory")
                    .iter()
                    .collect();
                let (last, above) = names.split_last().expect("below the start directory");
                let mut at = None;
                for name in above {
                    let parent = at.as_ref().unwrap_or(self.base);
                    at = Some(listing::open_at(parent, name, Access::Search)?);
                }
                listing::open_at(at.as_ref().unwrap_or(self.base), last, Access::Read)?
            }
        };
        Ok(Arc::new(Held::new(file, self.held)))
    }

    /// Reads one directory: its files go to the batch, and its directories
    /// are returned to be read. A directory that can't be opened (gone,
    /// replaced by a symlink, or unreadable) has nothing to read.
    fn read(&mut self, work: Work) -> Vec<Work> {
        let Ok(dir) = self.open(work.dir, &work.rel) else {
            return Vec::new();
        };
        let path = self.root.join(&work.rel);
        let rules = Rules::load(work.rules, &dir.file, &path);
        let Ok(names) = listing::read_dir(&dir.file) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for (name, kind) in names {
            if self.cancel.load(Ordering::Relaxed) || self.queue.stopped() {
                break;
            }
            if listing::is_git_dir(&name) {
                continue;
            }
            let kind = match kind {
                Some(kind) => kind,
                // The file system didn't say; an entry gone since is skipped.
                None => match listing::stat_at(&dir.file, &name) {
                    Ok(stat) => stat.kind,
                    Err(_) => continue,
                },
            };
            let is_dir = kind == EntryKind::Dir;
            if rules.ignores(self.global, &path.join(&name), is_dir) {
                continue;
            }
            let rel = work.rel.join(&name);
            match kind {
                EntryKind::File => {
                    self.walked.listed.fetch_add(1, Ordering::Relaxed);
                    self.batch.push(rel);
                    if self.batch.len() >= self.walked.batch {
                        self.send();
                    }
                }
                EntryKind::Dir => found.push(Work {
                    dir: Dir::At(dir.clone(), name),
                    rel,
                    rules: Some(rules.clone()),
                }),
                // Never followed.
                EntryKind::Symlink => {}
            }
        }
        // Keep this directory open for its subdirectories only while few
        // others are; it is closed once the last of them is opened.
        if !found.is_empty() && self.held.load(Ordering::Relaxed) > MAX_HELD {
            for work in &mut found {
                work.dir = Dir::Below;
            }
        }
        found
    }
}

impl Walker<'_> {
    /// Sends the batch, waiting while the channel is full. Once the calling
    /// thread has stopped taking batches, stops the walk instead.
    fn send(&mut self) {
        let batch = std::mem::take(&mut self.batch);
        if self.tx.send(batch).is_err() {
            self.queue.stop();
        }
    }
}

impl Drop for Walker<'_> {
    fn drop(&mut self) {
        if !self.batch.is_empty() && !self.cancel.load(Ordering::Relaxed) && !self.queue.stopped() {
            self.send();
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

/// A matcher for one query word. Results are ranked by the caller, so frizbee
/// needn't sort.
fn word_matcher(word: &str) -> Matcher {
    let config = Config::default()
        .casing(CaseMatching::Ignore)
        .sort(SortStrategy::IndexAsc);
    Matcher::new(word, &config)
}
