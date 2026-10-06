//! Watching config files for changes.

use std::{
    collections::HashSet,
    convert::Infallible,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, mpsc},
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
/// drop events meanwhile (notify restarts its FSEvents stream). For the same
/// reason, every file's links are resolved again after registering, until
/// they lead to no directory not yet watched.
///
/// A directory that doesn't exist isn't watched, and is logged, so creating
/// a missing config directory takes a restart. Symlinked directories are
/// resolved when the watcher starts or when a file's own links change. The
/// watcher never creates directories.
///
/// Dropping the watcher stops it and waits for the thread.
pub struct ConfigWatcher {
    watcher: Option<Arc<Mutex<RecommendedWatcher>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ConfigWatcher {
    pub fn new(
        files: Vec<PathBuf>,
        sink: impl FnMut(Vec<PathBuf>) + Send + 'static,
    ) -> notify::Result<ConfigWatcher> {
        Self::with_hook(files, sink, |_| {})
    }

    /// [`ConfigWatcher::new`], calling `registered` with each directory
    /// just after it's watched.
    pub(crate) fn with_hook(
        files: Vec<PathBuf>,
        mut sink: impl FnMut(Vec<PathBuf>) + Send + 'static,
        mut registered: impl FnMut(&Path) + Send + 'static,
    ) -> notify::Result<ConfigWatcher> {
        let mut tracker = Tracker::new(files);
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = notify::recommended_watcher(tx)?;
        tracker.register(|dir| {
            watcher.watch(dir, RecursiveMode::NonRecursive)?;
            registered(dir);
            Ok::<_, notify::Error>(())
        })?;
        let first = tracker.files();
        let watcher = Arc::new(Mutex::new(watcher));
        // Weak, so dropping the ConfigWatcher drops the watcher, whose
        // sender ends the thread.
        let weak = Arc::downgrade(&watcher);
        let thread = thread::Builder::new()
            .name("config-watcher".into())
            .spawn(move || {
                if !first.is_empty() {
                    sink(first);
                }
                // Ends when the watcher, which owns the sender, is dropped.
                while let Ok(event) = rx.recv() {
                    let mut events = vec![event];
                    events.extend(rx.try_iter());
                    // Directories are registered before the sink reads
                    // anything.
                    let files = tracker.batch(&events, |dir| {
                        let Some(watcher) = weak.upgrade() else {
                            return;
                        };
                        let result = watcher
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .watch(dir, RecursiveMode::NonRecursive);
                        if let Err(error) = result {
                            log::warn!("not watching {}: {error}", dir.display());
                        }
                        registered(dir);
                    });
                    if !files.is_empty() {
                        sink(files);
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

/// Which files events concern, and which directories to watch for them.
pub(crate) struct Tracker {
    files: Vec<PathBuf>,
    /// For each file, every path its resolution goes through: the file, then
    /// each symlink's target in turn, the last maybe missing. Each has its
    /// directory canonicalized, if that exists, as events name them (/var is
    /// /private/var on macOS).
    chains: Vec<Vec<PathBuf>>,
    /// Directories watched.
    dirs: HashSet<PathBuf>,
    /// Directories the chains need that couldn't be watched, logged once.
    unwatched: HashSet<PathBuf>,
}

impl Tracker {
    /// A tracker for `files`, watching nothing yet: see
    /// [`Tracker::register`].
    pub(crate) fn new(files: Vec<PathBuf>) -> Self {
        Self {
            chains: files.iter().map(|file| chain(file)).collect(),
            files,
            dirs: HashSet::new(),
            unwatched: HashSet::new(),
        }
    }

    /// Every file, in order, once each.
    pub(crate) fn files(&self) -> Vec<PathBuf> {
        self.in_order(|_| true)
    }

    /// Watches, with `watch`, each existing directory the chains need that
    /// isn't watched yet. Events during the registration may be lost, links
    /// included, so after it every chain is resolved again, and any
    /// directory they now need is watched in turn, until none is: each
    /// round watches more, or stops. Returns whether any was watched.
    pub(crate) fn register<E>(
        &mut self,
        mut watch: impl FnMut(&Path) -> Result<(), E>,
    ) -> Result<bool, E> {
        let mut any = false;
        loop {
            let new_dirs = self.new_dirs();
            if new_dirs.is_empty() {
                return Ok(any);
            }
            for dir in &new_dirs {
                watch(dir)?;
            }
            any = true;
            self.chains = self.files.iter().map(|file| chain(file)).collect();
        }
    }

    /// The files `events` may have changed, re-resolving their symlinks and
    /// watching, with `watch`, any directories their links now lead to.
    /// Dropped events and rescans may have changed any file, and so may
    /// anything during the registration of new directories, so either
    /// reports every file.
    pub(crate) fn batch(
        &mut self,
        events: &[notify::Result<Event>],
        mut watch: impl FnMut(&Path),
    ) -> Vec<PathBuf> {
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
        let Ok(registered) = self.register(|dir| {
            watch(dir);
            Ok::<_, Infallible>(())
        });
        if registered {
            self.files()
        } else {
            self.in_order(|index| changed[index])
        }
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
    /// count as watched from now on. Missing or inaccessible ones are
    /// logged, once each.
    fn new_dirs(&mut self) -> Vec<PathBuf> {
        let mut new_dirs = Vec::new();
        for path in self.chains.iter().flatten() {
            let Some(dir) = path.parent() else {
                continue;
            };
            if self.dirs.contains(dir) {
                continue;
            }
            if dir.is_dir() {
                self.dirs.insert(dir.to_owned());
                new_dirs.push(dir.to_owned());
            } else if self.unwatched.insert(dir.to_owned()) {
                log::info!(
                    "not watching {}: it's missing or inaccessible, so creating it \
                     takes a restart for settings to reload",
                    dir.display()
                );
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
