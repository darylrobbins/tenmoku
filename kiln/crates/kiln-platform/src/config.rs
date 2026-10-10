// SPDX-License-Identifier: LicenseRef-Proprietary

//! The config service: watches an app's config files with one
//! [`ConfigWatcher`] and applies each change on the main thread, so every
//! window follows.
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
//! reads them all with [`diagnostics`].

use std::{
    collections::VecDeque,
    future::poll_fn,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    task::{Poll, Waker},
};

use gpui::{App, Global, Task};
use kiln_core::{ConfigWatcher, Diagnostic, KeymapStore, SettingsStore, SettingsType};

use crate::keys;

/// One kind of config file the service reloads: a settings store, a
/// keymap, a theme. Implement it for a file kiln doesn't know.
pub trait ConfigSource: 'static {
    /// The files it reads, all watched.
    fn paths(&self) -> Vec<PathBuf>;

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
    for diagnostic in diagnostics {
        if diagnostic.is_error() {
            log::error!("{diagnostic}");
        } else {
            log::warn!("{diagnostic}");
        }
    }
}

/// Watches config files and applies their changes; see the module docs.
#[derive(Default)]
pub struct ConfigService {
    sources: Vec<Box<dyn ConfigSource>>,
    dirs: Vec<PathBuf>,
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
        self.source(KeymapSource { store })
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

    /// Applies what each source loaded, then watches their files,
    /// reloading on the main thread for as long as the task runs: detach
    /// it, or hold it. Each user file's directory is created first if it's
    /// missing, as are those from [`ConfigService::create_dir`]. The
    /// watcher's first batch names every file, so a save made between
    /// loading and watching applies too.
    ///
    /// If the files can't be watched, what was loaded still applies, and
    /// the error says why they won't reload.
    pub fn start(mut self, cx: &mut App) -> io::Result<Task<()>> {
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
        let (sender, batches) = batch_channel();
        let paths = self.paths();
        self.install(cx);
        let watcher =
            ConfigWatcher::new(paths, move |batch| sender.send(batch)).map_err(io::Error::other)?;
        Ok(self.run(batches, Some(watcher), cx))
    }

    /// [`ConfigService::start`] without a watcher: `batches` says which
    /// files changed, as the watcher would. For tests, and for an app with
    /// its own watcher.
    pub fn start_with(mut self, batches: Batches, cx: &mut App) -> Task<()> {
        self.install(cx);
        self.run(batches, None, cx)
    }

    fn install(&mut self, cx: &mut App) {
        for source in &mut self.sources {
            source.install(cx);
        }
        let all = self.sources.iter().flat_map(|source| source.diagnostics());
        cx.set_global(ConfigDiagnostics(all.collect()));
    }

    fn run(self, mut batches: Batches, watcher: Option<ConfigWatcher>, cx: &mut App) -> Task<()> {
        let mut sources = self.sources;
        cx.spawn(async move |cx| {
            // Watching for as long as this task runs.
            let _watcher = watcher;
            while let Some(batch) = batches.next().await {
                cx.update(|cx| apply(&mut sources, &batch, cx));
            }
        })
    }
}

/// Hands `batch` to every source, logging and publishing diagnostics that
/// changed.
fn apply(sources: &mut [Box<dyn ConfigSource>], batch: &[PathBuf], cx: &mut App) {
    let mut changed = false;
    for source in sources.iter_mut() {
        let previous = source.diagnostics();
        source.reload(batch, cx);
        let diagnostics = source.diagnostics();
        if diagnostics != previous {
            log_diagnostics(&diagnostics);
            changed = true;
        }
    }
    if changed {
        let all = sources.iter().flat_map(|source| source.diagnostics());
        cx.set_global(ConfigDiagnostics(all.collect()));
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
            log::info!("settings reloaded");
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
    store: KeymapStore,
}

impl ConfigSource for KeymapSource {
    fn paths(&self) -> Vec<PathBuf> {
        self.store.paths()
    }

    fn user_files(&self) -> Vec<PathBuf> {
        self.store.paths()
    }

    fn install(&mut self, cx: &mut App) {
        keys::init(cx, self.store.keymap().clone());
    }

    fn reload(&mut self, changed: &[PathBuf], cx: &mut App) {
        let Some(event) = self.store.reload(changed) else {
            return;
        };
        if event.keymap != event.previous {
            log::info!("keymap reloaded");
            keys::set_keymap(cx, event.keymap);
        }
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        self.store.diagnostics().to_vec()
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
