// SPDX-License-Identifier: LicenseRef-Proprietary

//! Watching directory trees, such as the extensions directory, for any
//! change under them.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, mpsc},
    thread,
    time::Instant,
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::{platform::fs::FileStamp, watch::RECONCILE};

/// The most entries a root's re-stat reads; a root with more is reported
/// on every re-stat.
const MAX_ENTRIES: usize = 10_000;

/// Watches directory trees, each recursively, and hands the roots that
/// had something change under them to a sink, on its own thread: a batch
/// names each root once, in the order given.
///
/// The first batch names every root that exists, before any event: the
/// trees may have changed between reading them and starting to watch.
/// Symlinks under a root are followed, each directory once: a directory
/// one leads to outside the root is watched too, recursively, and a file's
/// directory not, and a change there is reported as the root's.
/// FSEvents can drop events without flagging it under a flood, so every
/// [`RECONCILE`] the thread also re-stats each root's tree (its listing,
/// and each entry's [`FileStamp`], and a link's target's) and reports a
/// root whose tree changed since it was last reported, as if an event had
/// named it. A tree of more than 10,000 entries is reported on every
/// re-stat, as a change past them couldn't be seen. A root that
/// doesn't exist yet, or is removed and made again, is watched once the
/// re-stat sees it, and reported then. The watcher never creates
/// directories.
///
/// Dropping the watcher stops it and waits for the thread.
pub struct TreeWatcher {
    watcher: Option<Arc<Mutex<RecommendedWatcher>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl TreeWatcher {
    pub fn new(
        roots: Vec<PathBuf>,
        mut sink: impl FnMut(Vec<PathBuf>) + Send + 'static,
    ) -> notify::Result<TreeWatcher> {
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = notify::recommended_watcher(tx)?;
        let mut trees = Trees::new(roots);
        let first = trees.register(&mut |path, mode| watcher.watch(path, mode));
        let watcher = Arc::new(Mutex::new(watcher));
        // Weak, so dropping the TreeWatcher drops the watcher, whose
        // sender ends the thread.
        let weak = Arc::downgrade(&watcher);
        let thread = thread::Builder::new()
            .name("tree-watcher".into())
            .spawn(move || {
                if !first.is_empty() {
                    sink(first);
                }
                let mut watch = |path: &Path, mode| {
                    let Some(watcher) = weak.upgrade() else {
                        return Ok(());
                    };
                    let mut watcher = watcher.lock().unwrap_or_else(PoisonError::into_inner);
                    // A root made again is a new directory: its old watch
                    // is gone, or watches nothing.
                    let _ = watcher.unwatch(path);
                    watcher.watch(path, mode)
                };
                let mut reconcile_at = Instant::now() + RECONCILE;
                loop {
                    let now = Instant::now();
                    let roots = if now >= reconcile_at {
                        if weak.strong_count() == 0 {
                            break;
                        }
                        let roots = trees.reconcile(&mut watch);
                        reconcile_at = Instant::now() + RECONCILE;
                        roots
                    } else {
                        match rx.recv_timeout(reconcile_at - now) {
                            Ok(event) => {
                                let mut events = vec![event];
                                while Instant::now() < reconcile_at {
                                    let Ok(event) = rx.try_recv() else {
                                        break;
                                    };
                                    events.push(event);
                                }
                                trees.batch(&events, &mut watch)
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            // The watcher, which owns the sender, was dropped.
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    };
                    if !roots.is_empty() {
                        sink(roots);
                    }
                }
            })?;
        Ok(TreeWatcher {
            watcher: Some(watcher),
            thread: Some(thread),
        })
    }
}

impl Drop for TreeWatcher {
    fn drop(&mut self) {
        drop(self.watcher.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Each root, as given and as events name it, its tree when last
/// reported, and whether it's watched.
pub(crate) struct Trees {
    roots: Vec<Root>,
    /// The most entries a root's re-stat reads.
    limit: usize,
}

struct Root {
    path: PathBuf,
    /// `path` canonicalized, once it exists (/var is /private/var on
    /// macOS).
    canonical: Option<PathBuf>,
    tree: Option<Tree>,
    watched: bool,
    /// The link targets outside the root watched for it, each with
    /// whether it's watched recursively.
    links: HashSet<(PathBuf, bool)>,
    /// Why it isn't watched was logged, once until it is.
    logged: bool,
}

/// What a re-stat compares: each entry under a root, by path as reached
/// from it, and what `symlink_metadata` says of it, and of a symlink's
/// target too. Symlinks are followed, each directory once, so a change to
/// a linked extension is seen wherever its files are.
#[derive(Debug, Default, PartialEq, Eq)]
struct Tree {
    entries: BTreeMap<PathBuf, Stat>,
    /// The directories links lead to outside the root, canonical: each is
    /// watched recursively for the root.
    linked_dirs: BTreeSet<PathBuf>,
    /// The files links lead to outside the root, canonical: each one's
    /// directory is watched, not recursively.
    linked_files: BTreeSet<PathBuf>,
    /// It has more than the limit's entries, so some went unread: it's
    /// reported on every re-stat, as a change to one couldn't be seen.
    truncated: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Stat {
    Present {
        kind: fs::FileType,
        stamp: FileStamp,
    },
    Link {
        stamp: FileStamp,
        target: Box<Stat>,
    },
    Error(io::ErrorKind),
}

impl Stat {
    fn of(meta: &fs::Metadata) -> Stat {
        Stat::Present {
            kind: meta.file_type(),
            stamp: FileStamp::of(meta),
        }
    }
}

/// Watches a path, recursively or not.
pub(crate) type Watch<'a> = dyn FnMut(&Path, RecursiveMode) -> notify::Result<()> + 'a;

impl Trees {
    pub(crate) fn new(roots: Vec<PathBuf>) -> Self {
        Self::with_limit(roots, MAX_ENTRIES)
    }

    /// Trees whose re-stat reads at most `limit` entries a root.
    pub(crate) fn with_limit(roots: Vec<PathBuf>, limit: usize) -> Self {
        let mut unique: Vec<PathBuf> = Vec::new();
        for root in roots {
            if !unique.contains(&root) {
                unique.push(root);
            }
        }
        Self {
            roots: unique
                .into_iter()
                .map(|path| Root {
                    path,
                    canonical: None,
                    tree: None,
                    watched: false,
                    links: HashSet::new(),
                    logged: false,
                })
                .collect(),
            limit,
        }
    }

    /// Watches each existing root not watched yet, and returns them, their
    /// trees read. One that can't be watched is logged, and tried again on
    /// the next re-stat.
    pub(crate) fn register(&mut self, watch: &mut Watch<'_>) -> Vec<PathBuf> {
        let mut registered = Vec::new();
        for root in &mut self.roots {
            if root.watched {
                continue;
            }
            if !root.path.is_dir() {
                if !std::mem::replace(&mut root.logged, true) {
                    log::info!(
                        "not watching {}: it's missing or inaccessible; it's watched once \
                         it's created, within {} s",
                        root.path.display(),
                        RECONCILE.as_secs()
                    );
                }
                root.tree = None;
                continue;
            }
            if let Err(error) = watch(&root.path, RecursiveMode::Recursive) {
                if !std::mem::replace(&mut root.logged, true) {
                    log::warn!("not watching {}: {error}", root.path.display());
                }
                continue;
            }
            root.watched = true;
            root.logged = false;
            root.canonical = root.path.canonicalize().ok();
            // After watching, so a change while reading it is an event.
            root.tree = Some(tree(&root.path, self.limit));
            root.follow_links(watch);
            registered.push(root.path.clone());
        }
        registered
    }

    /// The roots `events` name a path under, or every root for a rescan or
    /// an error.
    pub(crate) fn batch(
        &mut self,
        events: &[notify::Result<Event>],
        watch: &mut Watch<'_>,
    ) -> Vec<PathBuf> {
        let mut changed = vec![false; self.roots.len()];
        for event in events {
            match event {
                Ok(event) if event.need_rescan() => changed.fill(true),
                Ok(Event {
                    kind: EventKind::Access(_),
                    ..
                }) => {}
                Ok(event) => {
                    for (changed, root) in changed.iter_mut().zip(&self.roots) {
                        *changed |= event.paths.iter().any(|path| root.contains(path));
                    }
                }
                Err(_) => changed.fill(true),
            }
        }
        self.report(changed, watch)
    }

    /// The roots whose tree changed since they were last reported, though
    /// no event said so, and those too big to read whole.
    pub(crate) fn reconcile(&mut self, watch: &mut Watch<'_>) -> Vec<PathBuf> {
        let changed = self
            .roots
            .iter()
            .map(|root| {
                let now = root.path.is_dir().then(|| tree(&root.path, self.limit));
                now.as_ref().is_some_and(|tree| tree.truncated) || now != root.tree
            })
            .collect();
        self.report(changed, watch)
    }

    /// The `changed` roots and those newly watched, their trees read
    /// again. A root that's gone is watched again once it's back.
    fn report(&mut self, mut changed: Vec<bool>, watch: &mut Watch<'_>) -> Vec<PathBuf> {
        for (changed, root) in changed.iter_mut().zip(&mut self.roots) {
            if !*changed {
                continue;
            }
            if root.path.is_dir() {
                root.tree = Some(tree(&root.path, self.limit));
                root.follow_links(watch);
            } else {
                root.tree = None;
                root.watched = false;
                root.links.clear();
            }
        }
        let registered = self.register(watch);
        let roots = self.roots.iter().zip(&changed);
        roots
            .filter(|(root, changed)| **changed || registered.contains(&root.path))
            .map(|(root, _)| root.path.clone())
            .collect()
    }
}

impl Root {
    /// Whether `path`, as an event names it, is this root or under it, or
    /// under a directory or a file its links lead to.
    fn contains(&self, path: &Path) -> bool {
        let tree = self.tree.as_ref();
        path.starts_with(&self.path)
            || self
                .canonical
                .as_ref()
                .is_some_and(|canonical| path.starts_with(canonical))
            || tree.is_some_and(|tree| {
                tree.linked_dirs.iter().any(|dir| path.starts_with(dir))
                    || tree.linked_files.contains(path)
            })
    }

    /// Watches, with `watch`, each link target outside the root its tree
    /// has that isn't watched yet: a directory recursively, a file's
    /// directory not. One that can't be watched is logged, and left to the
    /// re-stat.
    fn follow_links(&mut self, watch: &mut Watch<'_>) {
        let Some(tree) = &self.tree else {
            return;
        };
        let dirs = tree.linked_dirs.iter().map(|dir| (dir.clone(), true));
        let files = tree.linked_files.iter().filter_map(|file| {
            let dir = file.parent()?;
            Some((dir.to_owned(), false))
        });
        for (path, recursive) in dirs.chain(files).collect::<Vec<_>>() {
            if !self.links.insert((path.clone(), recursive)) {
                continue;
            }
            let mode = if recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            if let Err(error) = watch(&path, mode) {
                log::warn!(
                    "not watching {}, which {} links to: {error}",
                    path.display(),
                    self.path.display()
                );
            }
        }
    }
}

/// Every entry under `root`, up to `limit`: see [`Tree`].
fn tree(root: &Path, limit: usize) -> Tree {
    let mut tree = Tree::default();
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_owned());
    let mut visited = HashSet::from([canonical_root.clone()]);
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                tree.entries.insert(dir, Stat::Error(error.kind()));
                continue;
            }
        };
        for entry in entries {
            if tree.entries.len() >= limit {
                tree.truncated = true;
                return tree;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    tree.entries.insert(dir.clone(), Stat::Error(error.kind()));
                    continue;
                }
            };
            let path = entry.path();
            let meta = match fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(error) => {
                    tree.entries.insert(path, Stat::Error(error.kind()));
                    continue;
                }
            };
            let stat = if meta.file_type().is_symlink() {
                let target = match fs::metadata(&path) {
                    Ok(target) => {
                        let canonical = path.canonicalize().ok();
                        let outside = canonical
                            .clone()
                            .filter(|canonical| !canonical.starts_with(&canonical_root));
                        if target.is_dir() {
                            // Each directory once, so a loop of links ends.
                            if canonical.is_some_and(|canonical| visited.insert(canonical)) {
                                pending.push(path.clone());
                            }
                            tree.linked_dirs.extend(outside);
                        } else {
                            tree.linked_files.extend(outside);
                        }
                        Stat::of(&target)
                    }
                    Err(error) => Stat::Error(error.kind()),
                };
                Stat::Link {
                    stamp: FileStamp::of(&meta),
                    target: Box::new(target),
                }
            } else {
                if meta.is_dir() {
                    let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                    if visited.insert(canonical) {
                        pending.push(path.clone());
                    }
                }
                Stat::of(&meta)
            };
            tree.entries.insert(path, stat);
        }
    }
    tree
}
