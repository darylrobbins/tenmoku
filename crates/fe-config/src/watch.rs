//! Watching config files for changes.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Watches config files and hands the ones that changed to a sink, on its
/// own thread. Each file's parent directory is watched, not recursively, so
/// saves that write a temporary file and rename it over the original are
/// seen. A file that's a symlink is followed: the directory of each link
/// and target along the way is watched too, so editing or replacing any of
/// them is seen, including creating a missing target. A batch is every
/// event queued when the thread wakes, with each file once, in the order
/// the files were given.
///
/// The first batch names every file, before any event: a file may have
/// changed between loading it and starting to watch. A batch that starts
/// watching another directory also names every file, as registering one can
/// drop events meanwhile (notify restarts its FSEvents stream).
///
/// A directory that doesn't exist isn't watched, so creating a missing
/// config directory takes a restart. Symlinked directories are resolved
/// when the watcher starts or when a file's own links change. The watcher
/// never creates directories.
///
/// Dropping the watcher stops it and waits for the thread.
pub struct ConfigWatcher {
    watcher: Option<Arc<Mutex<RecommendedWatcher>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ConfigWatcher {
    pub fn new(
        files: Vec<PathBuf>,
        mut sink: impl FnMut(Vec<PathBuf>) + Send + 'static,
    ) -> notify::Result<ConfigWatcher> {
        let (mut tracker, first) = Tracker::new(files);
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = notify::recommended_watcher(tx)?;
        for dir in &first.new_dirs {
            watcher.watch(dir, RecursiveMode::NonRecursive)?;
        }
        let watcher = Arc::new(Mutex::new(watcher));
        // Weak, so dropping the ConfigWatcher drops the watcher, whose
        // sender ends the thread.
        let weak = Arc::downgrade(&watcher);
        let thread = thread::Builder::new()
            .name("config-watcher".into())
            .spawn(move || {
                if !first.files.is_empty() {
                    sink(first.files);
                }
                // Ends when the watcher, which owns the sender, is dropped.
                while let Ok(event) = rx.recv() {
                    let mut events = vec![event];
                    events.extend(rx.try_iter());
                    let batch = tracker.batch(&events);
                    // Registered before the sink reads anything.
                    if !batch.new_dirs.is_empty()
                        && let Some(watcher) = weak.upgrade()
                    {
                        let mut watcher = watcher
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        for dir in &batch.new_dirs {
                            if let Err(error) = watcher.watch(dir, RecursiveMode::NonRecursive) {
                                log::warn!("not watching {}: {error}", dir.display());
                            }
                        }
                    }
                    if !batch.files.is_empty() {
                        sink(batch.files);
                    }
                }
            })?;
        Ok(ConfigWatcher {
            watcher: Some(watcher),
            thread: Some(thread),
        })
    }
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        drop(self.watcher.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Symlinks followed before giving up on a loop, as the OS does.
const MAX_LINKS: usize = 40;

/// What to do about some events.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Batch {
    /// The files to report, in order, once each.
    pub(crate) files: Vec<PathBuf>,
    /// Directories to start watching before reporting them.
    pub(crate) new_dirs: Vec<PathBuf>,
}

/// Which files events concern, and which directories to watch for them.
pub(crate) struct Tracker {
    files: Vec<PathBuf>,
    /// For each file, every path its resolution goes through: the file, then
    /// each symlink's target in turn, the last maybe missing. Each has its
    /// directory canonicalized, if that exists, as events name them (/var is
    /// /private/var on macOS).
    chains: Vec<Vec<PathBuf>>,
    /// Directories watched, or about to be.
    dirs: HashSet<PathBuf>,
}

impl Tracker {
    /// A tracker for `files`, and the first batch: every file, and the
    /// directories to watch.
    pub(crate) fn new(files: Vec<PathBuf>) -> (Self, Batch) {
        let mut tracker = Self {
            chains: files.iter().map(|file| chain(file)).collect(),
            files,
            dirs: HashSet::new(),
        };
        let new_dirs = tracker.new_dirs();
        let files = tracker.in_order(|_| true);
        (tracker, Batch { files, new_dirs })
    }

    /// The files `events` may have changed, re-resolving their symlinks, and
    /// any directories their links now lead to. Dropped events and rescans
    /// may have changed any file, and so may anything during the
    /// registration of new directories, so either reports every file.
    pub(crate) fn batch(&mut self, events: &[notify::Result<Event>]) -> Batch {
        let mut changed = vec![false; self.files.len()];
        for event in events {
            match event {
                Ok(event) if event.need_rescan() => changed.fill(true),
                // Reads, including our own, don't change anything.
                Ok(Event {
                    kind: EventKind::Access(_),
                    ..
                }) => {}
                Ok(event) => {
                    for (changed, chain) in changed.iter_mut().zip(&self.chains) {
                        *changed |= event.paths.iter().any(|path| chain.contains(path));
                    }
                }
                Err(_) => changed.fill(true),
            }
        }
        for (index, changed) in changed.iter().enumerate() {
            if *changed {
                self.chains[index] = chain(&self.files[index]);
            }
        }
        let mut new_dirs = self.new_dirs();
        if new_dirs.is_empty() {
            let files = self.in_order(|index| changed[index]);
            return Batch { files, new_dirs };
        }
        // Events during the registration may be lost, links included, so
        // every chain is resolved again, until no more directories are
        // needed: each round adds one, or stops.
        loop {
            self.chains = self.files.iter().map(|file| chain(file)).collect();
            let more = self.new_dirs();
            if more.is_empty() {
                break;
            }
            new_dirs.extend(more);
        }
        let files = self.in_order(|_| true);
        Batch { files, new_dirs }
    }

    fn in_order(&self, include: impl Fn(usize) -> bool) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = Vec::new();
        for (index, file) in self.files.iter().enumerate() {
            if include(index) && !files.contains(file) {
                files.push(file.clone());
            }
        }
        files
    }

    /// Existing directories the chains need that aren't watched yet; they
    /// count as watched from now on.
    fn new_dirs(&mut self) -> Vec<PathBuf> {
        let mut new_dirs = Vec::new();
        for path in self.chains.iter().flatten() {
            if let Some(dir) = path.parent()
                && !self.dirs.contains(dir)
                && dir.is_dir()
            {
                self.dirs.insert(dir.to_owned());
                new_dirs.push(dir.to_owned());
            }
        }
        new_dirs
    }
}

/// The paths resolving `file` goes through: see [`Tracker::chains`].
fn chain(file: &Path) -> Vec<PathBuf> {
    let mut path = canonical_dir(file);
    let mut chain = Vec::new();
    for _ in 0..MAX_LINKS {
        chain.push(path.clone());
        let Ok(target) = std::fs::read_link(&path) else {
            break;
        };
        let target = match path.parent() {
            Some(dir) if target.is_relative() => dir.join(target),
            _ => target,
        };
        path = canonical_dir(&target);
    }
    chain
}

/// `path` with its directory canonicalized, if that exists.
fn canonical_dir(path: &Path) -> PathBuf {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return path.to_owned();
    };
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    dir.canonicalize()
        .map_or_else(|_| path.to_owned(), |dir| dir.join(name))
}
