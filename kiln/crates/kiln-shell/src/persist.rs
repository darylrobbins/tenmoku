// SPDX-License-Identifier: LicenseRef-Proprietary

//! Saving state as it changes, off the main thread: a [`Writer`] queues
//! keyed writes and runs them on the background executor, one at a time.
//! Each key's last write wins: however many queue up for a key, what's
//! written last is its latest value. Quitting can wait for the writes
//! ([`Writer::wait_at_quit`], or [`Writer::finish`] for a host that saves
//! more as it quits).
//!
//! A value is queued as it's taken, in the dispatch that changed it, so a
//! reload landing before the write can't change what's saved.
//!
//! [`setting_writer`] is a writer of boolean settings (a toggle a host
//! keeps, such as whether the status bar shows) into a settings file
//! (kiln-core's [`AppPaths::settings_file`](kiln_core::AppPaths::settings_file)),
//! through [`kiln_core::write_setting`]: the file is put in place whole,
//! through a temporary file, so a reader never sees it half written, and a
//! failed write is logged and leaves it as it was.

use std::{
    cell::RefCell,
    future::Future,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex, MutexGuard},
};

use gpui::{App, BackgroundExecutor, Task};

/// What writes a key's value: see [`Writer::new`].
type Write<K, V> = Arc<dyn Fn(&K, V) + Send + Sync>;

/// Queues keyed writes and runs them in the background, one at a time.
/// Used on the main thread; only the queue and the write go to the
/// writer.
pub struct Writer<K, V> {
    /// The writes waiting. Public so a host's tests can see what's queued.
    pub queue: Arc<Mutex<Queue<K, V>>>,
    /// The task writing, or that last wrote, for quitting to wait on.
    pub writer: Rc<RefCell<Option<Task<()>>>>,
    write: Write<K, V>,
    executor: BackgroundExecutor,
}

impl<K, V> Clone for Writer<K, V> {
    fn clone(&self) -> Self {
        Self {
            queue: self.queue.clone(),
            writer: self.writer.clone(),
            write: self.write.clone(),
            executor: self.executor.clone(),
        }
    }
}

/// A [`Writer`]'s writes waiting.
pub struct Queue<K, V> {
    /// Each key's latest value, oldest first.
    pub pending: Vec<(K, V)>,
    /// Whether a background task is writing; it writes whatever is pending
    /// before it stops.
    writing: bool,
}

impl<K: PartialEq + Send + 'static, V: Send + 'static> Writer<K, V> {
    /// A writer that runs `write` for each key and its latest value, on
    /// `cx`'s background executor. `write` reports its own failures.
    pub fn new(cx: &App, write: impl Fn(&K, V) + Send + Sync + 'static) -> Self {
        Self {
            queue: Arc::new(Mutex::new(Queue {
                pending: Vec::new(),
                writing: false,
            })),
            writer: Rc::default(),
            write: Arc::new(write),
            executor: cx.background_executor().clone(),
        }
    }

    /// Queues `key` to be written as `value`, replacing a value of `key`
    /// still waiting, and starts a writer if none is running.
    pub fn save(&self, key: K, value: V) {
        {
            let mut queue = lock(&self.queue);
            queue.pending.retain(|(pending, _)| *pending != key);
            queue.pending.push((key, value));
            if queue.writing {
                return;
            }
            queue.writing = true;
        }
        // The last writer has set `writing` false, so it's done, or
        // returning.
        let (queue, write) = (self.queue.clone(), self.write.clone());
        let writer = self
            .executor
            .spawn(async move { write_pending(&queue, &*write) });
        self.writer.replace(Some(writer));
    }

    /// Resolves once the writes queued so far are written: for quitting to
    /// wait on, after saving what it saves.
    pub fn finish(&self) -> impl Future<Output = ()> + use<K, V> {
        let writer = self.writer.take();
        async move {
            if let Some(writer) = writer {
                writer.await;
            }
        }
    }

    /// Has quitting wait for the writes, up to GPUI's shutdown timeout.
    pub fn wait_at_quit(&self, cx: &mut App) {
        let this = self.clone();
        cx.on_app_quit(move |_| this.finish()).detach();
    }
}

/// Writes what's pending until nothing is.
fn write_pending<K, V>(queue: &Mutex<Queue<K, V>>, write: &(dyn Fn(&K, V) + Send + Sync)) {
    loop {
        let pending = {
            let mut queue = lock(queue);
            if queue.pending.is_empty() {
                queue.writing = false;
                return;
            }
            std::mem::take(&mut queue.pending)
        };
        for (key, value) in pending {
            write(&key, value);
        }
    }
}

fn lock<K, V>(queue: &Mutex<Queue<K, V>>) -> MutexGuard<'_, Queue<K, V>> {
    queue.lock().unwrap_or_else(|error| error.into_inner())
}

/// A setting's name, table by table: `&["status_bar", "visible"]`.
pub type SettingKey = &'static [&'static str];

/// The settings file a [`setting_writer`] writes, shared so the host can
/// point later writes at another file.
#[derive(Clone, Debug, Default)]
pub struct SettingsPath(Arc<Mutex<PathBuf>>);

impl SettingsPath {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(Arc::new(Mutex::new(path.into())))
    }

    /// Writes from now on go to `path`.
    pub fn set(&self, path: impl Into<PathBuf>) {
        *self.0.lock().unwrap_or_else(|error| error.into_inner()) = path.into();
    }

    pub fn get(&self) -> PathBuf {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

/// A writer of boolean settings into the settings file at `path`, editing
/// it in place, or creating it from `template` (see
/// [`kiln_core::write_setting`]). A failed write is logged, naming the
/// setting and the file.
pub fn setting_writer(
    path: SettingsPath,
    template: &'static str,
    cx: &App,
) -> Writer<SettingKey, bool> {
    Writer::new(cx, move |key: &SettingKey, value| {
        let path = path.get();
        write_setting(&path, key, value, template);
    })
}

fn write_setting(path: &Path, key: &[&str], value: bool, template: &str) {
    if let Err(error) = kiln_core::write_setting(path, key, value, template) {
        log::error!(
            "can't save {} to {}: {error}",
            key.join("."),
            path.display()
        );
    }
}
