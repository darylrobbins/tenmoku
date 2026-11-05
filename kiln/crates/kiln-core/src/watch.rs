// SPDX-License-Identifier: Apache-2.0

//! Watching config files for changes.

use std::{
    collections::HashSet,
    convert::Infallible,
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, Weak, mpsc},
    thread,
    time::{Duration, Instant},
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::platform::fs::FileStamp;

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
/// FSEvents can drop events without flagging it when something writes many
/// files at once, anywhere. So every [`RECONCILE`] the thread also re-stats
/// each file's chain, and reports every file whose chain changed since it
/// was last reported, as if an event had named it: a change no event
/// reported is picked up within [`RECONCILE`]. A directory that doesn't
/// exist isn't watched, and is logged. Nothing reports creating it, but its
/// file's chain then changes, so it's watched within [`RECONCILE`] too. Symlinked directories are
/// resolved when the watcher starts or when a file's own links change. The
/// watcher never creates directories.
///
/// Files can be added to those it was given, and changed, while it runs:
/// see [`ExtraFiles`].
///
/// Dropping the watcher stops it and waits for the thread.
pub struct ConfigWatcher {
    watcher: Option<Arc<Mutex<RecommendedWatcher>>>,
    extra: Option<Arc<Extra>>,
    thread: Option<thread::JoinHandle<()>>,
}

/// Sets the files a [`ConfigWatcher`] watches besides those it was given
/// ([`ConfigWatcher::extra_files`]), from any thread. It doesn't keep the
/// watcher running: once that's dropped, setting files does nothing.
#[derive(Clone)]
pub struct ExtraFiles(Weak<Extra>);

/// What an [`ExtraFiles`] hands the watcher's thread.
struct Extra {
    /// The files last set, until the thread takes them up.
    pending: Arc<Mutex<Option<Vec<PathBuf>>>>,
    /// Wakes the thread to take them up: an event naming nothing.
    wake: mpsc::Sender<notify::Result<Event>>,
}

impl ExtraFiles {
    /// Watches `files` as well as the watcher's own, in place of those set
    /// before. The next batch names each file that wasn't watched before,
    /// as it may have changed since it was read, or every file if watching
    /// them took up a directory not watched yet (see [`ConfigWatcher`]). A
    /// file no longer set is no longer reported; its directory stays
    /// watched, as unwatching one can drop events as watching one can.
    pub fn set(&self, files: Vec<PathBuf>) {
        let Some(extra) = self.0.upgrade() else {
            return;
        };
        *extra.pending.lock().unwrap_or_else(PoisonError::into_inner) = Some(files);
        // Gone only if the thread's events were swapped out, in tests: it
        // takes the files up on its next wake.
        let _ = extra.wake.send(Ok(Event::new(EventKind::Other)));
    }
}

/// How often the watcher re-stats its files: see [`ConfigWatcher`].
pub(crate) const RECONCILE: Duration = Duration::from_secs(3);

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
        let pending = Arc::new(Mutex::new(None));
        let extra = Arc::new(Extra {
            pending: pending.clone(),
            wake: tx.clone(),
        });
        let mut watcher = notify::recommended_watcher(tx)?;
        let rx = source(rx);
        let clock = Clock::take();
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
                // Directories are registered before the sink reads anything.
                let mut watch = |dir: &Path| {
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
                };
                // On a deadline, checked before every wait and while
                // draining, so a steady stream of events can't put the
                // re-stat off.
                let mut reconcile_at = clock.now() + RECONCILE;
                loop {
                    let extra = pending
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take();
                    if let Some(extra) = extra {
                        let files = tracker.set_extra(extra, &mut watch);
                        if !files.is_empty() {
                            sink(files);
                        }
                    }
                    let now = clock.now();
                    let files = if now >= reconcile_at {
                        if weak.strong_count() == 0 {
                            break;
                        }
                        let files = tracker.reconcile(&mut watch);
                        // From when it ends, so a slow one leaves a full
                        // period for events, and the disconnect check.
                        reconcile_at = clock.now() + RECONCILE;
                        files
                    } else {
                        match rx.recv_timeout(reconcile_at - now) {
                            Ok(event) => {
                                let mut events = vec![event];
                                while clock.now() < reconcile_at {
                                    let Ok(event) = rx.try_recv() else {
                                        break;
                                    };
                                    events.push(event);
                                }
                                tracker.batch(&events, &mut watch)
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            // The watcher, which owns the senders, was dropped.
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    };
                    if !files.is_empty() {
                        sink(files);
                    }
                }
            })?;
        Ok(ConfigWatcher {
            watcher: Some(watcher),
            extra: Some(extra),
            thread: Some(thread),
        })
    }

    /// A handle that sets the files watched besides those it was given.
    pub fn extra_files(&self) -> ExtraFiles {
        ExtraFiles(self.extra.as_ref().map_or_else(Weak::new, Arc::downgrade))
    }
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        // Its sender too, so the thread sees every sender gone.
        drop(self.extra.take());
        drop(self.watcher.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

type Events = mpsc::Receiver<notify::Result<Event>>;

/// The events the thread reads: in tests,
/// [`crate::tests::watch_reconcile::SOURCE`] stands in, if set on this
/// thread.
#[cfg(not(test))]
fn source(events: Events) -> Events {
    events
}

#[cfg(test)]
fn source(events: Events) -> Events {
    crate::tests::watch_reconcile::SOURCE
        .take()
        .unwrap_or(events)
}

/// The time the thread's deadlines are read from: in tests,
/// [`crate::tests::watch_reconcile::CLOCK`] stands in, if set on this thread.
#[cfg(not(test))]
struct Clock;

#[cfg(not(test))]
impl Clock {
    fn take() -> Clock {
        Clock
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[cfg(test)]
struct Clock(Option<Arc<crate::tests::watch_reconcile::FakeClock>>);

#[cfg(test)]
impl Clock {
    fn take() -> Clock {
        Clock(crate::tests::watch_reconcile::CLOCK.take())
    }

    fn now(&self) -> Instant {
        self.0.as_ref().map_or_else(Instant::now, |fake| fake.now())
    }
}

/// Symlinks followed before giving up on a loop, as the OS does.
const MAX_LINKS: usize = 40;

/// Which files events concern, and which directories to watch for them.
pub(crate) struct Tracker {
    /// The files given, then those set since ([`Tracker::set_extra`]).
    files: Vec<PathBuf>,
    /// How many of `files` were given.
    given: usize,
    /// For each file, every path its resolution goes through: the file, then
    /// each symlink's target in turn, the last maybe missing. Each has its
    /// directory canonicalized, if that exists, as events name them (/var is
    /// /private/var on macOS).
    chains: Vec<Vec<PathBuf>>,
    /// Directories watched.
    dirs: HashSet<PathBuf>,
    /// Directories the chains need that couldn't be watched, logged once.
    unwatched: HashSet<PathBuf>,
    /// For each file, its [`fingerprint`] when it was last reported, taken
    /// before the sink reads it.
    prints: Vec<Fingerprint>,
}

impl Tracker {
    /// A tracker for `files`, watching nothing yet: see
    /// [`Tracker::register`]. The first batch reports every file, so their
    /// fingerprints are taken now.
    pub(crate) fn new(files: Vec<PathBuf>) -> Self {
        Self {
            chains: files.iter().map(|file| chain(file)).collect(),
            prints: files.iter().map(|file| fingerprint(file)).collect(),
            given: files.len(),
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
    /// round watches more, or stops. Returns whether any was watched, and if
    /// so, as every file is then reported, takes their fingerprints.
    pub(crate) fn register<E>(
        &mut self,
        mut watch: impl FnMut(&Path) -> Result<(), E>,
    ) -> Result<bool, E> {
        let mut any = false;
        loop {
            let new_dirs = self.new_dirs();
            if new_dirs.is_empty() {
                if any {
                    self.prints = self.files.iter().map(|file| fingerprint(file)).collect();
                }
                return Ok(any);
            }
            for dir in &new_dirs {
                watch(dir)?;
            }
            any = true;
            self.chains = self.files.iter().map(|file| chain(file)).collect();
        }
    }

    /// Tracks `extra` as well as the files given, in place of those set
    /// before, watching with `watch` any directory they need, and returns
    /// the files to report: every one if a directory was watched, else
    /// those not tracked before, as they may have changed since they were
    /// read. A file set again keeps its chain and fingerprint.
    pub(crate) fn set_extra(
        &mut self,
        extra: Vec<PathBuf>,
        mut watch: impl FnMut(&Path),
    ) -> Vec<PathBuf> {
        let files: Vec<PathBuf> = (self.files[..self.given].iter().cloned())
            .chain(extra)
            .collect();
        let mut old: Vec<Option<(Vec<PathBuf>, Fingerprint)>> = (std::mem::take(&mut self.chains)
            .into_iter())
        .zip(std::mem::take(&mut self.prints))
        .map(Some)
        .collect();
        let (mut chains, mut prints, mut added) = (Vec::new(), Vec::new(), Vec::new());
        for file in &files {
            let kept =
                (self.files.iter().position(|was| was == file)).and_then(|index| old[index].take());
            let (chain, print) = kept.unwrap_or_else(|| (chain(file), fingerprint(file)));
            chains.push(chain);
            prints.push(print);
            added.push(!self.files.contains(file));
        }
        (self.files, self.chains, self.prints) = (files, chains, prints);
        // Takes every fingerprint if it watched anything.
        let Ok(registered) = self.register(|dir| {
            watch(dir);
            Ok::<_, Infallible>(())
        });
        if registered {
            return self.files();
        }
        self.in_order(|index| added[index])
    }

    /// The files `events` may have changed, re-resolving their symlinks and
    /// watching, with `watch`, any directories their links now lead to.
    /// Dropped events and rescans may have changed any file, and so may
    /// anything during the registration of new directories, so either
    /// reports every file. The files reported have their fingerprints taken
    /// before this returns, so [`Tracker::reconcile`] doesn't report them
    /// again unless they change afterwards.
    pub(crate) fn batch(
        &mut self,
        events: &[notify::Result<Event>],
        watch: impl FnMut(&Path),
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
        self.report(&changed, watch)
    }

    /// The files whose [`fingerprint`] changed since they were last
    /// reported, handled as if an event had named each: FSEvents can drop
    /// events without flagging it, and creating a directory a chain needs
    /// gets no event at all. A new directory is watched with `watch`, which
    /// reports every file, as [`Tracker::batch`] does.
    pub(crate) fn reconcile(&mut self, watch: impl FnMut(&Path)) -> Vec<PathBuf> {
        let changed: Vec<bool> = self
            .files
            .iter()
            .zip(&self.prints)
            .map(|(file, print)| fingerprint(file) != *print)
            .collect();
        self.report(&changed, watch)
    }

    /// Re-resolves the `changed` files' chains, watches with `watch` any
    /// directory they now need, and returns the files to report: every one
    /// if a directory was watched, else the changed ones. Takes the
    /// fingerprints of those it returns.
    fn report(&mut self, changed: &[bool], mut watch: impl FnMut(&Path)) -> Vec<PathBuf> {
        for (index, changed) in changed.iter().enumerate() {
            if *changed {
                self.chains[index] = chain(&self.files[index]);
            }
        }
        // Takes every fingerprint if it watched anything.
        let Ok(registered) = self.register(|dir| {
            watch(dir);
            Ok::<_, Infallible>(())
        });
        if registered {
            return self.files();
        }
        for (index, changed) in changed.iter().enumerate() {
            if *changed {
                self.prints[index] = fingerprint(&self.files[index]);
            }
        }
        self.in_order(|index| changed[index])
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
                    "not watching {}: it's missing or inaccessible; it's watched once \
                     it's created, within {} s",
                    dir.display(),
                    RECONCILE.as_secs()
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

/// What [`Tracker::reconcile`] compares: for each path resolving a file
/// goes through, its [`Stat`]. Any change to the file, a link or the chain
/// itself changes it.
type Fingerprint = Vec<(PathBuf, Stat)>;

/// What `symlink_metadata` says of a path, as far as a change shows in it.
#[derive(Debug, PartialEq, Eq)]
enum Stat {
    Present {
        kind: fs::FileType,
        stamp: FileStamp,
    },
    Missing,
    Error(io::ErrorKind),
}

/// `file`'s [`Fingerprint`], resolving its chain afresh.
fn fingerprint(file: &Path) -> Fingerprint {
    chain(file)
        .into_iter()
        .map(|path| {
            let stat = match fs::symlink_metadata(&path) {
                Ok(meta) => Stat::Present {
                    kind: meta.file_type(),
                    stamp: FileStamp::of(&meta),
                },
                Err(error) if error.kind() == io::ErrorKind::NotFound => Stat::Missing,
                Err(error) => Stat::Error(error.kind()),
            };
            (path, stat)
        })
        .collect()
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
