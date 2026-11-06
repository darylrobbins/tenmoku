//! Each plugin's own log, apart from the app's: the last [`MAX_LINES`]
//! lines its `log` import wrote, each with its time and level, and the
//! host's diagnostics of it: loaded, reloaded, failed, stopped and shut
//! down ([`crate::plugin_host`] writes both).
//!
//! Extensions: Show Plugin Log picks a plugin, as Uninstall Plugin does,
//! and opens its log in a read-only tab, as Language Server Log opens a
//! server's, that follows the lines written after: each is added at the
//! end, and the oldest dropped past [`MAX_LINES`], while the log shows in
//! any tab, a split of it too.

use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{SystemTime, UNIX_EPOCH},
};

use fe_buffer::{Buffer, BufferId, Origin};
use fe_editor::EditorModel;
use fe_gpui::EditorView;
use gpui::{App, Context, Entity, Focusable as _, Global, Window};

use crate::Workbench;

/// The most lines a plugin's log keeps: the oldest go first.
pub(crate) const MAX_LINES: usize = 1000;

/// A line of a plugin's log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LogLine {
    pub(crate) time: SystemTime,
    pub(crate) level: log::Level,
    pub(crate) message: String,
}

impl fmt::Display for LogLine {
    /// `2026-11-06 14:05:07 INFO  message`, in local time.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {:<5} {}", stamp(self.time), self.level, self.message)
    }
}

/// The plugins' logs, shared with the threads they run on.
#[derive(Clone, Default)]
pub(crate) struct PluginLogs(Arc<Mutex<Logs>>);

#[derive(Default)]
struct Logs {
    plugins: HashMap<String, Log>,
}

/// One plugin's log.
#[derive(Default)]
struct Log {
    lines: VecDeque<LogLine>,
    /// How many lines it was ever written: the newest's number.
    written: u64,
    /// Woken with each line, while a tab follows it.
    followers: Vec<async_channel::Sender<()>>,
}

impl PluginLogs {
    /// Writes `message` at `level` to plugin `id`'s log, now, and wakes the
    /// tabs following it. From any thread.
    pub(crate) fn push(&self, id: &str, level: log::Level, message: impl Into<String>) {
        let line = LogLine {
            time: SystemTime::now(),
            level,
            message: message.into(),
        };
        let mut logs = self.lock();
        let log = logs.plugins.entry(id.to_owned()).or_default();
        if log.lines.len() == MAX_LINES {
            log.lines.pop_front();
        }
        log.lines.push_back(line);
        log.written += 1;
        // A full channel has a wake waiting already.
        log.followers
            .retain(|follower| follower.try_send(()).is_ok() || !follower.is_closed());
    }

    /// Plugin `id`'s lines, oldest first.
    #[cfg(test)]
    pub(crate) fn lines(&self, id: &str) -> Vec<LogLine> {
        self.since(id, 0).1
    }

    /// Plugin `id`'s lines after its line number `seen` that it still
    /// keeps, and the newest's number.
    fn since(&self, id: &str, seen: u64) -> (u64, Vec<LogLine>) {
        let logs = self.lock();
        let Some(log) = logs.plugins.get(id) else {
            return (0, Vec::new());
        };
        let new = usize::try_from(log.written - seen.min(log.written)).unwrap_or(usize::MAX);
        let skip = log.lines.len().saturating_sub(new);
        (log.written, log.lines.iter().skip(skip).cloned().collect())
    }

    /// Plugin `id`'s lines so far, the newest's number and what wakes with
    /// each line after.
    fn follow(&self, id: &str) -> (u64, Vec<LogLine>, async_channel::Receiver<()>) {
        let (wake, woken) = async_channel::bounded(1);
        let mut logs = self.lock();
        let log = logs.plugins.entry(id.to_owned()).or_default();
        log.followers.push(wake);
        let lines = log.lines.iter().cloned().collect();
        (log.written, lines, woken)
    }

    fn lock(&self) -> MutexGuard<'_, Logs> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The app's plugin logs.
#[derive(Default)]
struct Shared(PluginLogs);

impl Global for Shared {}

/// The app's plugin logs, to write to from a plugin's thread.
pub(crate) fn logs(cx: &mut App) -> PluginLogs {
    cx.default_global::<Shared>().0.clone()
}

/// Plugin `id`'s lines, oldest first.
#[cfg(test)]
pub(crate) fn lines(id: &str, cx: &App) -> Vec<LogLine> {
    (cx.try_global::<Shared>())
        .map(|shared| shared.0.lines(id))
        .unwrap_or_default()
}

/// Writes `message` at `level` to plugin `id`'s log.
pub(crate) fn push(id: &str, level: log::Level, message: impl Into<String>, cx: &mut App) {
    logs(cx).push(id, level, message);
}

/// `time` in local time: `2026-11-06 14:05:07`.
fn stamp(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let seconds = libc::time_t::try_from(seconds).unwrap_or(libc::time_t::MAX);
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: `seconds` and `tm` are valid for the call.
    if unsafe { libc::localtime_r(&seconds, tm.as_mut_ptr()) }.is_null() {
        return String::from("1970-01-01 00:00:00");
    }
    // SAFETY: `localtime_r` succeeded, so it filled `tm`.
    let tm = unsafe { tm.assume_init() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        i64::from(tm.tm_year) + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
    )
}

/// `lines`, a line each.
fn text(lines: &[LogLine]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// Adds `text`, whole lines, at the end of `editor`'s buffer, dropping the
/// oldest lines past [`MAX_LINES`]. The buffer stays clean: there's nothing
/// to save.
fn append(editor: &mut EditorView, text: &str) {
    let model = editor.model_mut();
    model.sync_display();
    let mut buffer = model.buffer_mut();
    let end = buffer.len();
    let shown = buffer.text().row_count().saturating_sub(1) as usize;
    let excess = (shown + text.matches('\n').count()).saturating_sub(MAX_LINES);
    let cut = buffer.text().row_start(excess.min(shown) as u32);
    buffer.edit([(0..cut, ""), (end..end, text)], Origin::User);
    buffer.did_save();
}

/// The editors showing buffer `id`, in any window.
fn showing(id: BufferId, cx: &App) -> Vec<Entity<EditorView>> {
    (cx.windows().into_iter())
        .filter_map(|window| window.downcast::<Workbench>()?.read(cx).ok())
        .flat_map(|workbench| workbench.open_editors(cx))
        .filter(|editor| editor.read(cx).model().buffer().id() == id)
        .collect()
}

impl Workbench {
    /// Opens plugin `id`'s log, titled with its `name`, in a read-only tab
    /// that follows it, and focuses it.
    pub(crate) fn show_plugin_log(
        &mut self,
        id: &str,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (seen, lines, woken) = logs(cx).follow(id);
        let model = EditorModel::new(Buffer::from_file_contents(&text(&lines)), None);
        let editor = self.pane().update(cx, |pane, cx| {
            let editor = pane.new_editor(model, cx);
            editor.update(cx, |editor, cx| {
                editor.set_read_only(true, cx);
                editor.set_tab_title(format!("{name} Log"), "plugin.log", cx);
            });
            pane.add_item(Box::new(editor.clone()), cx);
            editor
        });
        window.focus(&editor.focus_handle(cx), cx);
        let buffer = editor.read(cx).model().buffer().id();
        follow(id.to_owned(), seen, woken, buffer, cx);
    }
}

/// Adds plugin `id`'s lines after number `seen` to `buffer` each time
/// `woken` wakes, through an editor showing it, until none does.
fn follow(
    id: String,
    mut seen: u64,
    woken: async_channel::Receiver<()>,
    buffer: BufferId,
    cx: &mut Context<Workbench>,
) {
    cx.spawn(async move |_, cx| {
        while woken.recv().await.is_ok() {
            let shown = cx.update(|cx| {
                let editors = showing(buffer, cx);
                let editor = editors.first()?;
                let (newest, lines) = logs(cx).since(&id, seen);
                seen = newest;
                let text = text(&lines);
                editor.update(cx, |editor, _| append(editor, &text));
                // Each view of it catches up on its next frame.
                editors
                    .iter()
                    .for_each(|editor| cx.notify(editor.entity_id()));
                Some(())
            });
            if shown.is_none() {
                return;
            }
        }
    })
    .detach();
}

#[cfg(test)]
#[path = "plugin_log_tests.rs"]
mod plugin_log_tests;
