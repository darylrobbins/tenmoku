//! Watching the project directory for changes.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
#[cfg(target_os = "linux")]
use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File},
    os::unix::fs::MetadataExt,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

#[cfg(target_os = "linux")]
use ignore::gitignore::Gitignore;
#[cfg(target_os = "linux")]
use kiln_l10n::tr;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

#[cfg(target_os = "linux")]
use crate::listing::{
    Access, Identity, Rules, global_excludes, identity, is_git_dir, open_dirs, read_dir, stat_at,
};

/// Something that may have changed on disk.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FsChange {
    /// Something at this path may have been created, changed or removed.
    Path(PathBuf),
    /// Events were dropped or coalesced; anything may have changed.
    Rescan,
}

/// Watches a directory recursively (FSEvents on macOS, inotify on Linux) and
/// hands batches of [`FsChange`]s to a sink on its own thread. A batch is
/// every event queued when the thread wakes, with each path once.
///
/// inotify watches a directory at a time, and the watches are capped per
/// user (`fs.inotify.max_user_watches`), shared with every other program. So
/// on Linux the watcher watches each directory itself, breadth-first: the
/// ones the project listing shows, then the ignored ones, never
/// `.git/objects` or below. At the cap it keeps the watches made, logs a
/// warning and sends `[FsChange::Rescan]` once, and the rest go unwatched. A
/// directory created later is watched with those under it, and everything
/// found in it is delivered, so what was written before its watch landed
/// isn't lost. After an inotify queue overflow, or an error, the whole
/// project is walked again for directories not watched. Directories are
/// opened without following symlinks, but watches are still added by path:
/// see the known limit on [`Dirs::watch_tree`].
///
/// FSEvents can drop events without flagging it while something writes many
/// files, such as a `git checkout`, a build or `npm install`: delivery stops
/// for tens of seconds, then resumes. So after a burst, [`BURST`] distinct
/// paths delivered with no [`SETTLE`]`[0]` of quiet between them, the
/// watcher also sends `[FsChange::Rescan]` once the stream has been quiet for
/// each of [`SETTLE`]'s periods, measured from the last event received. A
/// loss window looks like quiet, so the first rescan may land mid-flood;
/// the last covers a loss that outlasts the last delivered event by up to a
/// minute. Known limits: a flood outside the root that drops this root's
/// events isn't detected, and neither is a loss window more than
/// [`SETTLE`]`[2]` past the last delivered event.
///
/// Dropping the watcher stops it and waits for the thread.
pub struct ProjectWatcher {
    #[cfg(not(target_os = "linux"))]
    watcher: Option<RecommendedWatcher>,
    /// Shared with the thread, which watches new directories.
    #[cfg(target_os = "linux")]
    watcher: Shared,
    thread: Option<thread::JoinHandle<()>>,
}

/// Distinct paths that make a burst: see [`ProjectWatcher`].
const BURST: usize = 1_000;

/// How long the stream must be quiet, after a burst, for each rescan: see
/// [`ProjectWatcher`]. The first also ends the count towards a burst.
const SETTLE: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(15),
    Duration::from_secs(60),
];

type Events = mpsc::Receiver<notify::Result<Event>>;

/// The watcher, taken out when the [`ProjectWatcher`] is dropped.
#[cfg(target_os = "linux")]
type Shared = Arc<Mutex<Option<RecommendedWatcher>>>;

#[cfg(target_os = "linux")]
fn lock(watcher: &Shared) -> MutexGuard<'_, Option<RecommendedWatcher>> {
    watcher.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ProjectWatcher {
    pub fn new(
        root: impl AsRef<Path>,
        mut sink: impl FnMut(Vec<FsChange>) + Send + 'static,
    ) -> notify::Result<ProjectWatcher> {
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        #[cfg_attr(target_os = "linux", allow(unused_mut))]
        let mut watcher = notify::recommended_watcher(tx)?;
        #[cfg(not(target_os = "linux"))]
        watcher.watch(root.as_ref(), RecursiveMode::Recursive)?;
        #[cfg(not(target_os = "linux"))]
        let watcher = Some(watcher);
        #[cfg(target_os = "linux")]
        let watcher = Arc::new(Mutex::new(Some(watcher)));
        #[cfg(target_os = "linux")]
        let mut dirs = Dirs::new(root.as_ref(), watcher.clone())?;
        let (rx, settle) = source(rx);
        let thread = thread::Builder::new()
            .name("project-watcher".into())
            .spawn(move || {
                #[cfg(target_os = "linux")]
                if dirs.full {
                    sink(vec![FsChange::Rescan]);
                }
                let mut recovery = Recovery::new(settle);
                loop {
                    let first = match recovery.due() {
                        // Ends when the watcher, which owns the sender, is
                        // dropped.
                        None => match rx.recv() {
                            Ok(first) => first,
                            Err(mpsc::RecvError) => break,
                        },
                        Some(due) => {
                            match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
                                Ok(first) => first,
                                Err(mpsc::RecvTimeoutError::Timeout) => {
                                    recovery.rescanned();
                                    sink(vec![FsChange::Rescan]);
                                    continue;
                                }
                                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                            }
                        }
                    };
                    let mut events = vec![first];
                    events.extend(rx.try_iter());
                    #[cfg(all(test, target_os = "linux"))]
                    let events = dirs.tap(events);
                    recovery.received(&events, Instant::now());
                    let mut batch = Batch::default();
                    #[cfg(target_os = "linux")]
                    dirs.received(&events, &mut batch);
                    for event in events {
                        batch.add(event);
                    }
                    if !batch.changes.is_empty() {
                        sink(batch.changes);
                    }
                }
            })?;
        Ok(ProjectWatcher {
            watcher,
            thread: Some(thread),
        })
    }
}

impl Drop for ProjectWatcher {
    fn drop(&mut self) {
        #[cfg(not(target_os = "linux"))]
        let watcher = self.watcher.take();
        #[cfg(target_os = "linux")]
        let watcher = lock(&self.watcher).take();
        drop(watcher);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The directories watched on Linux, one inotify watch each: see
/// [`ProjectWatcher`].
#[cfg(target_os = "linux")]
struct Dirs {
    watcher: Shared,
    /// The root, canonical, which every path watched is under.
    root: PathBuf,
    /// git's global excludes, for telling listed directories from ignored.
    global: Arc<Gitignore>,
    /// Each directory watched, as the events spell it, and the identity of
    /// the directory opened there when it was.
    watched: HashMap<PathBuf, Identity>,
    /// How many watches may be made: the kernel's limit, or a test's.
    cap: usize,
    /// Set once a watch failed for the limit: nothing more is watched.
    full: bool,
    /// What a test does to the events the thread reads.
    #[cfg(test)]
    tap: Option<linux_tests::Tap>,
}

/// Directories left to watch, in order: see [`Dirs::watch_tree`].
#[cfg(target_os = "linux")]
#[derive(Default)]
struct Queue {
    /// Listed directories, breadth-first, each with the rules in force in
    /// its parent.
    listed: VecDeque<(PathBuf, Arc<Rules>)>,
    /// Ignored directories by depth below the directory walked, each depth
    /// in the order found.
    ignored: Vec<VecDeque<PathBuf>>,
    /// The depth of `ignored` being taken from.
    depth: usize,
}

#[cfg(target_os = "linux")]
impl Queue {
    /// The next directory, and its parent's rules if it is listed.
    fn pop(&mut self) -> Option<(PathBuf, Option<Arc<Rules>>)> {
        if let Some((dir, rules)) = self.listed.pop_front() {
            return Some((dir, Some(rules)));
        }
        while let Some(level) = self.ignored.get_mut(self.depth) {
            if let Some(dir) = level.pop_front() {
                return Some((dir, None));
            }
            self.depth += 1;
        }
        None
    }

    fn push_ignored(&mut self, dir: PathBuf, depth: usize) {
        if self.ignored.len() <= depth {
            self.ignored.resize_with(depth + 1, VecDeque::new);
        }
        self.ignored[depth].push_back(dir);
    }

    fn into_iter(self) -> impl Iterator<Item = PathBuf> {
        self.listed
            .into_iter()
            .map(|(dir, _)| dir)
            .chain(self.ignored.into_iter().flatten())
    }
}

#[cfg(target_os = "linux")]
impl Dirs {
    /// Watches `root` and the directories under it. Fails if `root` can't be
    /// watched, except for the limit.
    fn new(root: &Path, watcher: Shared) -> notify::Result<Dirs> {
        // Canonical, so it can be opened a directory at a time without
        // following symlinks.
        let root = dunce::canonicalize(root)
            .map_err(|err| notify::Error::io(err).add_path(root.into()))?;
        let mut dirs = Dirs {
            watcher,
            root: root.clone(),
            global: global_excludes(),
            watched: HashMap::new(),
            cap: cap(),
            full: false,
            #[cfg(test)]
            tap: linux_tests::TAP.take(),
        };
        dirs.watch_tree(&root, None)?;
        Ok(dirs)
    }

    /// Watches the directories that appeared, as `events` show, adding
    /// everything under each to `batch`, and forgets those removed. After an
    /// error or a rescan flag (an inotify queue overflow, say), which may
    /// hide directories made, it walks the whole project again for
    /// directories not watched. Adds [`FsChange::Rescan`] if this hits the
    /// limit.
    fn received(&mut self, events: &[notify::Result<Event>], batch: &mut Batch) {
        if self.full {
            return;
        }
        let mut lost = false;
        for event in events {
            if self.full {
                break;
            }
            let event = match event {
                Ok(event) if !event.need_rescan() => event,
                _ => {
                    lost = true;
                    continue;
                }
            };
            if matches!(
                event.kind,
                EventKind::Remove(_)
                    | EventKind::Modify(notify::event::ModifyKind::Name(
                        notify::event::RenameMode::From
                    ))
            ) {
                for path in &event.paths {
                    // notify drops the watches under a directory removed or
                    // moved from a watched one.
                    if self.watched.remove(path).is_some() {
                        self.watched.retain(|dir, _| !dir.starts_with(path));
                    }
                }
                continue;
            }
            for path in &event.paths {
                if self.watched.contains_key(path)
                    || !path.starts_with(&self.root)
                    || is_git_objects(path)
                    || !fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
                {
                    continue;
                }
                let mut entries = Vec::new();
                // Gone, say: nothing to watch.
                let _ = self.watch_tree(path, Some(&mut entries));
                for entry in entries {
                    batch.push(FsChange::Path(entry));
                }
            }
        }
        if lost && !self.full {
            let root = self.root.clone();
            let _ = self.watch_tree(&root, None);
        }
        if self.full {
            batch.push(FsChange::Rescan);
        }
    }

    /// Watches `dir`, and each directory under it, that isn't watched (or
    /// was watched as another directory since replaced), breadth-first: those
    /// the project listing shows, then the ignored ones, by depth. Each is
    /// opened a directory at a time from `/` without following symlinks,
    /// told listed or ignored by the ignore files read through those
    /// handles, watched, checked to be the directory opened, and read
    /// through its handle after its watch is made, adding its entries to
    /// `entries`. Stops at the limit. Fails if `dir` itself can't be
    /// watched, except for the limit.
    ///
    /// Known limit: notify adds a watch by path, which inotify resolves
    /// following symlinks, so a directory (or one above it) replaced by a
    /// symlink between the check and the watch redirects the watch. The check
    /// after it catches that and removes the watch, and watches again a
    /// directory already watched that the symlink led to (inotify shared its
    /// watch, so removing it ended that one too). But until then, events in
    /// the directory the symlink led to are named by the symlink's path, and
    /// one that is swapped back before the check goes unnoticed. Pinning a
    /// watch to the opened directory needs inotify without notify.
    fn watch_tree(
        &mut self,
        dir: &Path,
        mut entries: Option<&mut Vec<PathBuf>>,
    ) -> notify::Result<()> {
        let fail = |err| notify::Error::io(err).add_path(dir.to_path_buf());
        let chain = open_dirs(dir, Access::Search).map_err(|unreached| fail(unreached.err))?;
        let parent = self.classify(&chain, dir);
        drop(chain);
        let base = dir.components().count();
        let mut queue = Queue::default();
        match parent {
            Some(rules) => queue.listed.push_back((dir.to_path_buf(), rules)),
            None => queue.push_ignored(dir.to_path_buf(), 0),
        }
        while let Some((path, parent)) = queue.pop() {
            let handle = match open(&path) {
                Ok(handle) => handle,
                Err(err) if path == dir => return Err(fail(err)),
                // Gone, or a symlink now.
                Err(_) => continue,
            };
            let Ok(id) = identity(&handle) else {
                continue;
            };
            #[cfg(test)]
            linux_tests::opened(&path);
            if self.watched.get(&path) != Some(&id) {
                match self.watch(&path, id) {
                    Ok(true) => {}
                    Ok(false) if path == dir => {
                        return Err(notify::Error::generic(&tr!("project-io-watched-replaced"))
                            .add_path(path));
                    }
                    Ok(false) => continue,
                    Err(err) if matches!(err.kind, notify::ErrorKind::MaxFilesWatch) => {
                        let rest = std::iter::once(path).chain(queue.into_iter());
                        self.limit(rest);
                        return Ok(());
                    }
                    Err(err) if path == dir => return Err(err),
                    Err(_) => continue,
                }
            }
            let rules = parent.map(|parent| Rules::load(Some(parent), &handle, &path));
            for (child, is_dir) in children(&path, &handle) {
                if let Some(entries) = entries.as_deref_mut() {
                    entries.push(child.clone());
                }
                if !is_dir {
                    continue;
                }
                match &rules {
                    Some(rules) if is_listed(rules, &self.global, &child) => {
                        queue.listed.push_back((child, rules.clone()));
                    }
                    _ => {
                        let depth = child.components().count() - base;
                        queue.push_ignored(child, depth);
                    }
                }
            }
        }
        Ok(())
    }

    /// The rules in force in the parent of `dir`, if the project listing
    /// shows `dir`, from `chain`, the handles [`open_dirs`] opened for it:
    /// the root is shown, and below it, a directory that no ignore file
    /// ignores, nor `.git`, under one that is shown.
    fn classify(&self, chain: &[File], dir: &Path) -> Option<Arc<Rules>> {
        let paths: Vec<&Path> = dir.ancestors().collect();
        let mut rules: Option<Arc<Rules>> = None;
        for (handle, path) in chain.iter().zip(paths.into_iter().rev()) {
            if path
                .strip_prefix(&self.root)
                .is_ok_and(|rel| rel != Path::new(""))
                && !rules
                    .as_ref()
                    .is_some_and(|rules| is_listed(rules, &self.global, path))
            {
                return None;
            }
            if path == dir {
                return rules;
            }
            rules = Some(Rules::load(rules, handle, path));
        }
        None
    }

    /// Watches directory `dir`, opened as the directory with identity `id`,
    /// failing with [`notify::ErrorKind::MaxFilesWatch`] at the limit. False,
    /// with no watch kept, if the path no longer leads to that directory
    /// once the watch is made.
    fn watch(&mut self, dir: &Path, id: Identity) -> notify::Result<bool> {
        if !self.watched.contains_key(dir) && self.watched.len() >= self.cap {
            return Err(notify::Error::new(notify::ErrorKind::MaxFilesWatch));
        }
        let mut watcher = lock(&self.watcher);
        // Dropped: the thread is about to end.
        let Some(watcher) = watcher.as_mut() else {
            return Ok(false);
        };
        watcher.watch(dir, RecursiveMode::NonRecursive)?;
        // See the known limit on `watch_tree`.
        if open(dir).ok().and_then(|now| identity(&now).ok()) != Some(id) {
            let followed = fs::metadata(dir).ok().map(|meta| (meta.dev(), meta.ino()));
            let _ = watcher.unwatch(dir);
            self.watched.remove(dir);
            let shared: Vec<PathBuf> = self
                .watched
                .iter()
                .filter(|&(_, id)| Some(*id) == followed)
                .map(|(path, _)| path.clone())
                .collect();
            for path in shared {
                if watcher.watch(&path, RecursiveMode::NonRecursive).is_err() {
                    self.watched.remove(&path);
                }
            }
            return Ok(false);
        }
        self.watched.insert(dir.to_path_buf(), id);
        Ok(true)
    }

    /// Notes that the limit was hit with `unwatched` (and the directories
    /// under them) left, saying so in the log.
    fn limit(&mut self, unwatched: impl Iterator<Item = PathBuf>) {
        self.full = true;
        let mut left = 0;
        let mut queue: VecDeque<PathBuf> = unwatched.collect();
        while let Some(dir) = queue.pop_front() {
            left += 1;
            if let Ok(handle) = open(&dir) {
                queue.extend(
                    children(&dir, &handle)
                        .into_iter()
                        .filter_map(|(child, is_dir)| is_dir.then_some(child)),
                );
            }
        }
        log::warn!(
            "watching {} of {} directories in {}: the inotify watch limit was reached, so \
             changes in the rest won't show. Raise fs.inotify.max_user_watches to watch them all.",
            self.watched.len(),
            self.watched.len() + left,
            self.root.display(),
        );
    }

    /// Hands the events read to a test, if it asked for them.
    #[cfg(test)]
    fn tap(&mut self, events: Vec<notify::Result<Event>>) -> Vec<notify::Result<Event>> {
        match &mut self.tap {
            Some(tap) => events.into_iter().map(tap).collect(),
            None => events,
        }
    }
}

/// Whether directory `path` is one the project listing shows, given the
/// rules in force in its parent, a shown directory.
#[cfg(target_os = "linux")]
fn is_listed(rules: &Rules, global: &Gitignore, path: &Path) -> bool {
    !path.file_name().is_some_and(is_git_dir) && !rules.ignores(global, path, true)
}

/// Opens directory `dir`, absolute, a directory at a time from `/` without
/// following symlinks.
#[cfg(target_os = "linux")]
fn open(dir: &Path) -> std::io::Result<File> {
    let dirs = open_dirs(dir, Access::Read).map_err(|unreached| unreached.err)?;
    Ok(dirs.into_iter().next_back().expect("`/` at least"))
}

/// The entries of directory `dir`, read through `handle`, each with whether
/// it is a directory to watch: not a symlink, nor `.git/objects`.
#[cfg(target_os = "linux")]
fn children(dir: &Path, handle: &File) -> Vec<(PathBuf, bool)> {
    use crate::listing::EntryKind;

    let Ok(names) = read_dir(handle) else {
        return Vec::new();
    };
    names
        .into_iter()
        .map(|(name, kind)| {
            let kind = kind.or_else(|| stat_at(handle, &name).ok().map(|stat| stat.kind));
            let path = dir.join(name);
            let is_dir = kind == Some(EntryKind::Dir) && !is_git_objects(&path);
            (path, is_dir)
        })
        .collect()
}

/// Whether `path` is a git repository's object store, which is never
/// watched: it changes on every commit and fetch and holds thousands of
/// directories, and nothing shows it.
#[cfg(target_os = "linux")]
fn is_git_objects(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "objects")
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(is_git_dir)
}

/// How many inotify watches [`Dirs`] may make: in tests,
/// [`linux_tests::CAP`] if set on this thread.
#[cfg(all(target_os = "linux", not(test)))]
fn cap() -> usize {
    usize::MAX
}

#[cfg(all(target_os = "linux", test))]
fn cap() -> usize {
    linux_tests::CAP.take().unwrap_or(usize::MAX)
}

/// The events the thread reads, and its [`SETTLE`] periods: in tests,
/// [`crate::tests::watcher_recovery::SOURCE`] stands in, if set on this
/// thread.
#[cfg(not(test))]
fn source(events: Events) -> (Events, [Duration; 3]) {
    (events, SETTLE)
}

#[cfg(test)]
fn source(events: Events) -> (Events, [Duration; 3]) {
    match crate::tests::watcher_recovery::SOURCE.take() {
        Some(source) => (source.events, source.settle),
        None => (events, SETTLE),
    }
}

/// When to rescan after a burst: see [`ProjectWatcher`].
struct Recovery {
    settle: [Duration; 3],
    /// When the last event was received.
    last: Option<Instant>,
    /// Distinct paths delivered since the stream was last quiet for
    /// `settle[0]`, while no burst is pending.
    paths: HashSet<PathBuf>,
    /// After a burst, how many of its rescans were sent.
    rescans: Option<usize>,
}

impl Recovery {
    fn new(settle: [Duration; 3]) -> Self {
        Self {
            settle,
            last: None,
            paths: HashSet::new(),
            rescans: None,
        }
    }

    /// When the next rescan is due, if a burst is pending.
    fn due(&self) -> Option<Instant> {
        let last = self.last?;
        Some(last + self.settle[self.rescans?])
    }

    /// Counts the paths `events`, received `now`, name towards a burst, and
    /// restarts the quiet clock. Every path counts, including those of events
    /// a [`Batch`] turns into [`FsChange::Rescan`], but not unflagged
    /// [`EventKind::Access`] ones.
    fn received(&mut self, events: &[notify::Result<Event>], now: Instant) {
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) >= self.settle[0])
        {
            self.paths.clear();
        }
        self.last = Some(now);
        if self.rescans.is_some() {
            return;
        }
        for event in events.iter().flatten() {
            // Reads don't change the tree, and a rescan's own opens would
            // re-arm it; but a flagged event's paths still count.
            if matches!(event.kind, EventKind::Access(_)) && !event.need_rescan() {
                continue;
            }
            self.paths.extend(event.paths.iter().cloned());
        }
        if self.paths.len() >= BURST {
            self.rescans = Some(0);
            self.paths = HashSet::new();
        }
    }

    /// Notes that the rescan [`Recovery::due`] was sent.
    fn rescanned(&mut self) {
        self.rescans = self
            .rescans
            .map(|sent| sent + 1)
            .filter(|&sent| sent < self.settle.len());
    }
}

#[derive(Default)]
pub(crate) struct Batch {
    pub(crate) changes: Vec<FsChange>,
    seen: HashSet<FsChange>,
}

impl Batch {
    pub(crate) fn add(&mut self, event: notify::Result<Event>) {
        match event {
            Ok(event) if event.need_rescan() => self.push(FsChange::Rescan),
            // Reads don't change the tree.
            Ok(Event {
                kind: EventKind::Access(_),
                ..
            }) => {}
            Ok(event) => {
                for path in event.paths {
                    self.push(FsChange::Path(path));
                }
            }
            Err(_) => self.push(FsChange::Rescan),
        }
    }

    fn push(&mut self, change: FsChange) {
        if self.seen.insert(change.clone()) {
            self.changes.push(change);
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "watcher_linux_tests.rs"]
mod linux_tests;
