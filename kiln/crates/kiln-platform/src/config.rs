// SPDX-License-Identifier: LicenseRef-Proprietary

//! The config service: watches an app's config files with one
//! [`ConfigWatcher`], and its directories, such as the extensions
//! directory, with one [`TreeWatcher`], and applies each change on the main
//! thread, so every window follows.
//!
//! ```ignore
//! let settings = SettingsStore::<NotesSettings>::load(Some(user), project);
//! let keymap = KeymapStore::with_default(app_layer, diagnostics, Some(user_keymap), catalog);
//! ConfigService::new()
//!     .settings(settings)
//!     .keymap(keymap)
//!     .start(cx)?
//!     .detach();
//! // Anywhere, then:
//! let font_size = config::settings::<NotesSettings>(cx).font_size;
//! cx.observe_global::<GlobalSettings<NotesSettings>>(|cx| ..).detach();
//! ```
//!
//! Each kind of file is a [`ConfigSource`]: typed settings
//! ([`ConfigService::settings`]) become a [`GlobalSettings`]; a keymap
//! ([`ConfigService::keymap`]) goes to [`keys::set_keymap`], which
//! reinstalls the menus so their shortcuts follow; anything else, such as a
//! theme file, is the app's own source ([`ConfigService::source`],
//! [`ConfigService::on_change`]). Every source's diagnostics are logged
//! when they change, errors at `error` and the rest at `warn`, and the app
//! reads them all with [`diagnostics`]. They and the reloads' `info` lines
//! go under this module's log target, or the app's
//! ([`ConfigService::log_target`]).

use std::{
    cell::RefCell,
    collections::VecDeque,
    future::poll_fn,
    io,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex, PoisonError},
    task::{Poll, Waker},
};

use gpui::{App, Global, Task};
use kiln_core::{
    ConfigWatcher, Diagnostic, KeymapEvent, KeymapStore, SettingsStore, SettingsType, TreeWatcher,
};

/// The log target the service logs under unless the app names its own:
/// `kiln_platform::config`.
pub const DEFAULT_LOG_TARGET: &str = module_path!();

/// The target the service logs under, as a GPUI global.
struct LogTarget(&'static str);

impl Global for LogTarget {}

/// The log target the config service logs its diagnostics and reloads
/// under: the app's, if it named one ([`ConfigService::log_target`]),
/// once the service has started. A [`ConfigSource`] logging what it
/// reloaded logs under it too.
pub fn log_target(cx: &App) -> &'static str {
    cx.try_global::<LogTarget>()
        .map_or(DEFAULT_LOG_TARGET, |target| target.0)
}

use crate::keys;

/// One kind of config file the service reloads: a settings store, a
/// keymap, a theme. Implement it for a file kiln doesn't know.
pub trait ConfigSource: 'static {
    /// The files it reads, all watched.
    fn paths(&self) -> Vec<PathBuf>;

    /// Directories it reads, each watched recursively: a change anywhere
    /// under one is a batch naming the directory itself.
    fn trees(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    /// The user's own files among [`ConfigSource::paths`]: the service
    /// creates each one's directory if it's missing, before watching.
    fn user_files(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Applies what was loaded before the service started: called once,
    /// from [`ConfigService::start`].
    fn install(&mut self, _cx: &mut App) {}

    /// Re-reads whichever of its files are in `changed`, if any, and applies
    /// what changed. A batch may name only other sources' files.
    fn reload(&mut self, changed: &[PathBuf], cx: &mut App);

    /// Its files' problems now.
    fn diagnostics(&self) -> Vec<Diagnostic> {
        Vec::new()
    }
}

/// The settings of type `S` in use, as a GPUI global: read them with
/// [`settings`], and follow changes with
/// `cx.observe_global::<GlobalSettings<S>>(..)`, which a reload that
/// changes the settings or their diagnostics notifies.
pub struct GlobalSettings<S> {
    settings: Arc<S>,
    previous: Arc<S>,
    diagnostics: Vec<Diagnostic>,
}

impl<S: 'static> Global for GlobalSettings<S> {}

impl<S> GlobalSettings<S> {
    /// The settings now.
    pub fn settings(&self) -> &Arc<S> {
        &self.settings
    }

    /// The settings before the last reload, to tell what it changed: the
    /// same as [`GlobalSettings::settings`] before any.
    pub fn previous(&self) -> &Arc<S> {
        &self.previous
    }

    /// The settings files' diagnostics now.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

/// The settings of type `S` in use: `S::default()` if no service holds
/// any.
pub fn settings<S: Default + 'static>(cx: &App) -> Arc<S> {
    cx.try_global::<GlobalSettings<S>>()
        .map(|global| global.settings.clone())
        .unwrap_or_default()
}

/// Every config file's diagnostics, as a GPUI global: observe it to show
/// them as they change.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigDiagnostics(pub Vec<Diagnostic>);

impl Global for ConfigDiagnostics {}

/// Every config file's problems now, in the order the service's sources
/// were added.
pub fn diagnostics(cx: &App) -> &[Diagnostic] {
    cx.try_global::<ConfigDiagnostics>()
        .map_or(&[], |diagnostics| &diagnostics.0)
}

/// Logs `diagnostics`: errors at `error`, the rest at `warn`.
pub fn log_diagnostics(diagnostics: &[Diagnostic]) {
    log_diagnostics_as(DEFAULT_LOG_TARGET, diagnostics);
}

/// Logs `diagnostics` as [`log_diagnostics`] does, under `target`.
pub fn log_diagnostics_as(target: &str, diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        if diagnostic.is_error() {
            log::error!(target: target, "{diagnostic}");
        } else {
            log::warn!(target: target, "{diagnostic}");
        }
    }
}

/// Watches config files and applies their changes; see the module docs.
#[derive(Default)]
pub struct ConfigService {
    sources: Vec<Box<dyn ConfigSource>>,
    dirs: Vec<PathBuf>,
    log_target: Option<&'static str>,
    keymap: Option<SharedKeymap>,
}

type Sources = RefCell<Vec<Box<dyn ConfigSource>>>;

/// The sources of the service that started last, kept once its task ends
/// (its batches closed, or nothing to watch): for [`SharedKeymap::update`]
/// to publish every source's diagnostics.
struct Running(Rc<Sources>);

impl Global for Running {}

/// The keymap store a [`ConfigService`] reloads, shared so the app can
/// reach it after the service starts ([`ConfigService::keymap_store`]):
/// to lay new default layers under the user's file, say, as commands are
/// registered.
#[derive(Clone)]
pub struct SharedKeymap(Rc<RefCell<KeymapStore>>);

impl SharedKeymap {
    /// The store's effective keymap now.
    pub fn keymap(&self) -> Arc<kiln_core::Keymap> {
        self.0.borrow().keymap().clone()
    }

    /// The store's diagnostics now.
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.0.borrow().diagnostics().to_vec()
    }

    /// Changes the store with `change`, such as
    /// [`KeymapStore::set_defaults`], and applies what it says changed as
    /// a reload of the user's file does: a new keymap goes to
    /// [`keys::init`] (which swaps it, reinstalling the menus), and new
    /// diagnostics are logged and published in [`ConfigDiagnostics`]. The
    /// service's `reloaded` hook isn't called: that's for the user's file.
    pub fn update(
        &self,
        cx: &mut App,
        change: impl FnOnce(&mut KeymapStore) -> Option<KeymapEvent>,
    ) {
        let (event, previous) = {
            let mut store = self.0.borrow_mut();
            let previous = store.diagnostics().to_vec();
            (change(&mut store), previous)
        };
        let Some(event) = event else {
            return;
        };
        if event.keymap != event.previous {
            keys::init(cx, event.keymap);
        }
        if event.diagnostics != previous {
            log_diagnostics_as(log_target(cx), &event.diagnostics);
            let running = cx.try_global::<Running>().map(|running| running.0.clone());
            // Borrowed only while a batch applies, which publishes once
            // it's done.
            if let Some(sources) = running
                && let Ok(sources) = sources.try_borrow()
            {
                publish(&sources, cx);
            }
        }
    }
}

impl ConfigService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Typed settings: [`GlobalSettings<S>`] holds them from
    /// [`ConfigService::start`] on, and is set again on each reload that
    /// changes them or their diagnostics. A change to the settings
    /// redraws every window.
    pub fn settings<S: SettingsType>(self, store: SettingsStore<S>) -> Self {
        self.source(SettingsSource { store })
    }

    /// The keymap: installed with [`keys::init`] on start, then each
    /// reload that changes it goes to [`keys::set_keymap`], which also
    /// reinstalls the menus with their new shortcuts.
    pub fn keymap(self, store: KeymapStore) -> Self {
        self.keymap_with(store, |_| {})
    }

    /// [`ConfigService::keymap`], calling `reloaded` after each reload
    /// that swaps the keymap, once it's in use: to report on the new
    /// keymap, say.
    pub fn keymap_with(
        mut self,
        store: KeymapStore,
        reloaded: impl FnMut(&mut App) + 'static,
    ) -> Self {
        let store = SharedKeymap(Rc::new(RefCell::new(store)));
        self.keymap = Some(store.clone());
        self.source(KeymapSource {
            store,
            reloaded: Box::new(reloaded),
        })
    }

    /// The keymap store, shared with the service, so the app can change
    /// it once the service has started: the last one given to
    /// [`ConfigService::keymap`] or [`ConfigService::keymap_with`], if any.
    pub fn keymap_store(&self) -> Option<SharedKeymap> {
        self.keymap.clone()
    }

    /// Logs diagnostics and reloads under `target`, such as the app's
    /// crate name, instead of this module's, so the app's logger treats
    /// them as its own: see [`log_target`].
    pub fn log_target(mut self, target: &'static str) -> Self {
        self.log_target = Some(target);
        self
    }

    /// Any other kind of file.
    pub fn source(mut self, source: impl ConfigSource) -> Self {
        self.sources.push(Box::new(source));
        self
    }

    /// Calls `reload` with every batch naming any of `paths`, with the
    /// batch: for an app that reloads its files itself.
    pub fn on_change(
        self,
        paths: Vec<PathBuf>,
        reload: impl FnMut(&[PathBuf], &mut App) + 'static,
    ) -> Self {
        self.source(OnChange { paths, reload })
    }

    /// Creates `dir` if it's missing when the service starts. The watcher
    /// takes a missing directory up only when its periodic re-stat sees it
    /// created, so a file the app creates in it at once, as a Settings
    /// command does, would apply only seconds later.
    pub fn create_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dirs.push(dir.into());
        self
    }

    /// Every source's files, each once, in order.
    pub fn paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = Vec::new();
        for path in self.sources.iter().flat_map(|source| source.paths()) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        paths
    }

    /// Every source's directories watched recursively
    /// ([`ConfigSource::trees`]), each once, in order.
    pub fn trees(&self) -> Vec<PathBuf> {
        let mut trees: Vec<PathBuf> = Vec::new();
        for tree in self.sources.iter().flat_map(|source| source.trees()) {
            if !trees.contains(&tree) {
                trees.push(tree);
            }
        }
        trees
    }

    /// Applies what each source loaded, then watches their files,
    /// reloading on the main thread for as long as the task runs: detach
    /// it, or hold it. Each user file's directory is created first if it's
    /// missing, as are those from [`ConfigService::create_dir`]. The
    /// watcher's first batch names every file, so a save made between
    /// loading and watching applies too. The sources' directories
    /// ([`ConfigSource::trees`]) are watched by a [`TreeWatcher`] into the
    /// same batches, its first naming each that exists.
    ///
    /// If the files can't be watched, what was loaded still applies, and
    /// the error says why they won't reload.
    pub fn start(mut self, cx: &mut App) -> io::Result<Task<()>> {
        self.create_dirs();
        let (sender, batches) = batch_channel();
        let paths = self.paths();
        let trees = self.trees();
        self.install(cx);
        let tree_watcher = if trees.is_empty() {
            None
        } else {
            let sender = sender.clone();
            let watcher = TreeWatcher::new(trees, move |batch| sender.send(batch));
            Some(watcher.map_err(io::Error::other)?)
        };
        let watcher =
            ConfigWatcher::new(paths, move |batch| sender.send(batch)).map_err(io::Error::other)?;
        Ok(self.run(batches, Some(Watchers(watcher, tree_watcher)), cx))
    }

    /// [`ConfigService::start`] without a watcher: `batches` says which
    /// files changed, as the watcher would, and names a source's
    /// directory ([`ConfigSource::trees`]) for a change under it.
    /// Directories are created as
    /// `start` creates them. For tests, and for an app with
    /// its own watcher.
    pub fn start_with(mut self, batches: Batches, cx: &mut App) -> Task<()> {
        self.create_dirs();
        self.install(cx);
        self.run(batches, None, cx)
    }

    /// Creates each user file's directory, and those from
    /// [`ConfigService::create_dir`], if missing.
    fn create_dirs(&mut self) {
        let dirs = self.sources.iter().flat_map(|source| {
            source
                .user_files()
                .into_iter()
                .filter_map(|file| file.parent().map(Path::to_owned))
        });
        let dirs: Vec<PathBuf> = dirs.chain(self.dirs.drain(..)).collect();
        for dir in dirs {
            if !dir.as_os_str().is_empty()
                && let Err(error) = std::fs::create_dir_all(&dir)
            {
                log::warn!("can't create {}: {error}", dir.display());
            }
        }
    }

    fn install(&mut self, cx: &mut App) {
        if let Some(target) = self.log_target {
            cx.set_global(LogTarget(target));
        }
        for source in &mut self.sources {
            source.install(cx);
        }
        let all = self.sources.iter().flat_map(|source| source.diagnostics());
        cx.set_global(ConfigDiagnostics(all.collect()));
    }

    fn run(self, mut batches: Batches, watcher: Option<Watchers>, cx: &mut App) -> Task<()> {
        let sources = Rc::new(RefCell::new(self.sources));
        cx.set_global(Running(sources.clone()));
        cx.spawn(async move |cx| {
            // Watching for as long as this task runs.
            let _watcher = watcher;
            while let Some(batch) = batches.next().await {
                cx.update(|cx| apply(&sources, &batch, cx));
            }
        })
    }
}

/// What [`ConfigService::start`] watches with, held by its task.
struct Watchers(
    #[allow(dead_code, reason = "held, not read")] ConfigWatcher,
    #[allow(dead_code, reason = "held, not read")] Option<TreeWatcher>,
);

/// Hands `batch` to every source, logging diagnostics that changed, then
/// publishing them all if they did.
fn apply(sources: &Sources, batch: &[PathBuf], cx: &mut App) {
    let mut sources = sources.borrow_mut();
    for source in sources.iter_mut() {
        let previous = source.diagnostics();
        source.reload(batch, cx);
        let diagnostics = source.diagnostics();
        if diagnostics != previous {
            log_diagnostics_as(log_target(cx), &diagnostics);
        }
    }
    publish(&sources, cx);
}

/// Sets [`ConfigDiagnostics`] to every source's diagnostics, if they
/// differ from it.
fn publish(sources: &[Box<dyn ConfigSource>], cx: &mut App) {
    let all = ConfigDiagnostics(
        sources
            .iter()
            .flat_map(|source| source.diagnostics())
            .collect(),
    );
    if cx.try_global::<ConfigDiagnostics>() != Some(&all) {
        cx.set_global(all);
    }
}

struct SettingsSource<S> {
    store: SettingsStore<S>,
}

impl<S: SettingsType> ConfigSource for SettingsSource<S> {
    fn paths(&self) -> Vec<PathBuf> {
        self.store.paths()
    }

    fn user_files(&self) -> Vec<PathBuf> {
        self.store
            .user_path()
            .map(Path::to_owned)
            .into_iter()
            .collect()
    }

    fn install(&mut self, cx: &mut App) {
        let settings = self.store.settings().clone();
        cx.set_global(GlobalSettings {
            previous: settings.clone(),
            settings,
            diagnostics: self.store.diagnostics().to_vec(),
        });
    }

    fn reload(&mut self, changed: &[PathBuf], cx: &mut App) {
        let Some(event) = self.store.reload(changed) else {
            return;
        };
        let refresh = event.settings != event.previous;
        if refresh {
            log::info!(target: log_target(cx), "settings reloaded");
        }
        cx.set_global(GlobalSettings {
            settings: event.settings,
            previous: event.previous,
            diagnostics: event.diagnostics,
        });
        if refresh {
            cx.refresh_windows();
        }
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        self.store.diagnostics().to_vec()
    }
}

struct KeymapSource {
    store: SharedKeymap,
    reloaded: Box<dyn FnMut(&mut App)>,
}

impl ConfigSource for KeymapSource {
    fn paths(&self) -> Vec<PathBuf> {
        self.store.0.borrow().paths()
    }

    fn user_files(&self) -> Vec<PathBuf> {
        self.paths()
    }

    fn install(&mut self, cx: &mut App) {
        keys::init(cx, self.store.keymap());
    }

    fn reload(&mut self, changed: &[PathBuf], cx: &mut App) {
        // Not borrowed while the hook runs, which may change it.
        let event = self.store.0.borrow_mut().reload(changed);
        let Some(event) = event else {
            return;
        };
        if event.keymap != event.previous {
            log::info!(target: log_target(cx), "keymap reloaded");
            keys::set_keymap(cx, event.keymap);
            (self.reloaded)(cx);
        }
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        self.store.diagnostics()
    }
}

struct OnChange<F> {
    paths: Vec<PathBuf>,
    reload: F,
}

impl<F: FnMut(&[PathBuf], &mut App) + 'static> ConfigSource for OnChange<F> {
    fn paths(&self) -> Vec<PathBuf> {
        self.paths.clone()
    }

    fn reload(&mut self, changed: &[PathBuf], cx: &mut App) {
        if changed.iter().any(|path| self.paths.contains(path)) {
            (self.reload)(changed, cx);
        }
    }
}

/// A queue of batches of changed files, from a [`BatchSender`] on any
/// thread to the service on the main thread. kiln's own, so kiln needs no
/// channel crate.
struct Queue {
    batches: VecDeque<Vec<PathBuf>>,
    waker: Option<Waker>,
    senders: usize,
}

type Shared = Arc<Mutex<Queue>>;

fn lock(queue: &Shared) -> std::sync::MutexGuard<'_, Queue> {
    queue.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A channel of batches of changed files, for
/// [`ConfigService::start_with`].
pub fn batch_channel() -> (BatchSender, Batches) {
    let queue = Arc::new(Mutex::new(Queue {
        batches: VecDeque::new(),
        waker: None,
        senders: 1,
    }));
    (BatchSender(queue.clone()), Batches(queue))
}

/// Sends batches of changed files, from any thread. Once every sender is
/// dropped, the service stops.
pub struct BatchSender(Shared);

impl BatchSender {
    /// Queues `batch`: the files that changed, as [`ConfigWatcher`] names
    /// them.
    pub fn send(&self, batch: Vec<PathBuf>) {
        let waker = {
            let mut queue = lock(&self.0);
            queue.batches.push_back(batch);
            queue.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl Clone for BatchSender {
    fn clone(&self) -> Self {
        lock(&self.0).senders += 1;
        Self(self.0.clone())
    }
}

impl Drop for BatchSender {
    fn drop(&mut self) {
        let waker = {
            let mut queue = lock(&self.0);
            queue.senders -= 1;
            if queue.senders == 0 {
                queue.waker.take()
            } else {
                None
            }
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

/// The receiving end of [`batch_channel`].
pub struct Batches(Shared);

impl Batches {
    /// The next batch, or `None` once the queue is empty and every sender
    /// is dropped.
    pub async fn next(&mut self) -> Option<Vec<PathBuf>> {
        poll_fn(|context| {
            let mut queue = lock(&self.0);
            if let Some(batch) = queue.batches.pop_front() {
                Poll::Ready(Some(batch))
            } else if queue.senders == 0 {
                Poll::Ready(None)
            } else {
                queue.waker = Some(context.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}

impl Drop for Batches {
    /// Drops the waker here, on the thread that took it: a sender on the
    /// watcher's thread then has nothing to wake.
    fn drop(&mut self) {
        let waker = lock(&self.0).waker.take();
        drop(waker);
    }
}
