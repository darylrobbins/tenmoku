//! Saving View > Toggle Vim Mode and View > Toggle Status Bar to the user's
//! `settings.toml`, as `vim.enabled` and `status_bar.visible`, so they
//! outlast a restart. The settings watcher then sees the write, as it
//! would a hand edit.
//!
//! Nothing is saved until the host calls [`save_toggles_to`]. Each toggle
//! queues the value it left, taken in the dispatch that ran it, so a reload
//! landing before the write can't change what's saved. Writes run on the background
//! executor, one at a time, and each key's last toggle wins: however many
//! toggles queue up, the file ends at the last one's value. Quitting waits
//! for them, up to GPUI's shutdown timeout. A failed write is logged; the
//! toggle still applies for the session.
//!
//! Saves and Settings… need no coordination: both put the file in place
//! whole, through a temporary file, so neither reads the other's half
//! written, and a creation that fails removes only its temporary file.

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex, MutexGuard},
};

use fe_gpui::{commands::RunCommand, menu::MenuCommand};
use gpui::{App, BackgroundExecutor, Global, Task};

/// The setting View > Toggle Vim Mode saves.
const VIM: &[&str] = &["vim", "enabled"];
/// The setting View > Toggle Status Bar saves.
const STATUS_BAR: &[&str] = &["status_bar", "visible"];

/// The command whose successful runs save [`VIM`].
const VIM_TOGGLE: &str = "vim.toggle";

/// The app's [`Writes`], once [`save_toggles_to`] is called.
struct Saver(Writes);

impl Global for Saver {}

/// Where toggles are saved, and the writes waiting. Used on the main
/// thread; only the queue goes to the writer.
#[derive(Clone)]
struct Writes {
    queue: Arc<Mutex<Queue>>,
    executor: BackgroundExecutor,
    /// The task writing, or that last wrote, for quitting to wait on.
    writer: Rc<RefCell<Option<Task<()>>>>,
}

struct Queue {
    path: PathBuf,
    /// Each key's latest value, oldest first.
    pending: Vec<(&'static [&'static str], bool)>,
    /// Whether a background task is writing; it writes whatever is pending
    /// before it stops.
    writing: bool,
}

/// From now on, saves each toggle of vim mode and of the status bar to the
/// settings file at `path`, editing it in place, or creating it from the
/// settings template. Called again, later writes go to the new `path`.
///
/// Call after [`fe_gpui::init`]. Observers of a `vim.toggle` don't get the
/// app, so app listeners for the actions that run it (`RunCommand`, and
/// `MenuCommand` with no window open) read the switch in the same
/// dispatch: GPUI runs the later registered first, so before fe-gpui's
/// `RunCommand` handler, and before or after the menu's, whichever
/// installed first.
pub fn save_toggles_to(cx: &mut App, path: PathBuf) {
    if let Some(Saver(writes)) = cx.try_global::<Saver>() {
        lock(&writes.queue).path = path;
        return;
    }
    let writes = Writes {
        queue: Arc::new(Mutex::new(Queue {
            path,
            pending: Vec::new(),
            writing: false,
        })),
        executor: cx.background_executor().clone(),
        writer: Rc::default(),
    };
    cx.set_global(Saver(writes.clone()));
    let vim = Rc::new(VimToggle::default());
    cx.on_app_quit({
        let (writes, vim) = (writes.clone(), vim.clone());
        move |cx| {
            vim.save_unread(&writes, cx);
            let writer = writes.writer.take();
            async move {
                if let Some(writer) = writer {
                    writer.await;
                }
            }
        }
    })
    .detach();

    cx.on_action({
        let (writes, vim) = (writes.clone(), vim.clone());
        move |run: &RunCommand, cx| {
            vim.listen(run.0.command.as_str() == VIM_TOGGLE, &writes, cx);
            cx.propagate();
        }
    });
    cx.on_action({
        let (writes, vim) = (writes.clone(), vim.clone());
        move |_: &MenuCommand, cx| {
            // Which command the item runs is the menu's to know.
            vim.listen(true, &writes, cx);
            cx.propagate();
        }
    });
    let app = cx.to_async();
    fe_gpui::commands::observe_invocations(cx, move |invocation| {
        if invocation.command.as_str() != VIM_TOGGLE {
            return;
        }
        match vim.before.take() {
            Some(before) => writes.save(VIM, !before),
            None => {
                // A listener running after the command reads it; failing
                // that, as no action of theirs ran it, the next turn.
                vim.unread.set(true);
                let (writes, vim) = (writes.clone(), vim.clone());
                app.spawn(async move |cx| cx.update(|cx| vim.save_unread(&writes, cx)))
                    .detach();
            }
        }
    });
}

/// What the action listeners know of a `vim.toggle` being dispatched.
#[derive(Default)]
struct VimToggle {
    /// The switch before the toggle, read in the dispatch, until it ends.
    before: Cell<Option<bool>>,
    /// Whether a toggle ran with `before` unknown, and isn't saved yet.
    unread: Cell<bool>,
}

impl VimToggle {
    /// For an action being dispatched: saves the switch if a toggle ran
    /// before this listener did, else notes it for a toggle the action
    /// may run.
    fn listen(self: &Rc<Self>, may_toggle: bool, writes: &Writes, cx: &mut App) {
        if self.unread.get() {
            self.save_unread(writes, cx);
        } else if may_toggle {
            self.before.set(Some(fe_gpui::vim_enabled(cx)));
            let this = self.clone();
            cx.defer(move |_| this.before.set(None));
        }
    }

    /// Saves the switch, if a toggle ran unread.
    fn save_unread(&self, writes: &Writes, cx: &App) {
        if self.unread.take() {
            writes.save(VIM, fe_gpui::vim_enabled(cx));
        }
    }
}

/// Saves the status bar's visibility, if [`save_toggles_to`] was called.
pub(crate) fn save_status_bar(visible: bool, cx: &App) {
    if let Some(Saver(writes)) = cx.try_global::<Saver>() {
        writes.save(STATUS_BAR, visible);
    }
}

impl Writes {
    /// Queues `key` to be written as `value`, starting a writer if none is.
    fn save(&self, key: &'static [&'static str], value: bool) {
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
        let queue = self.queue.clone();
        let writer = self.executor.spawn(async move { write_pending(&queue) });
        self.writer.replace(Some(writer));
    }
}

/// Writes what's pending until nothing is.
fn write_pending(queue: &Mutex<Queue>) {
    loop {
        let (path, pending) = {
            let mut queue = lock(queue);
            if queue.pending.is_empty() {
                queue.writing = false;
                return;
            }
            (queue.path.clone(), std::mem::take(&mut queue.pending))
        };
        for (key, value) in pending {
            if let Err(error) = fe_config::write_setting(&path, key, value) {
                log::error!(
                    "can't save {} to {}: {error}",
                    key.join("."),
                    path.display()
                );
            }
        }
    }
}

fn lock(queue: &Mutex<Queue>) -> MutexGuard<'_, Queue> {
    queue.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
#[path = "persist_tests.rs"]
mod tests;
