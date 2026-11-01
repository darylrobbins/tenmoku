//! Open files kept in step with disk, a diff view's working side among
//! them (`git_diff`'s [`Workbench::file_editors`]): a watcher batch naming
//! an open editor's file, a rescan, or the window becoming active checks the
//! buffers shown. A file changed on disk reloads into a clean buffer, and
//! asks first (Reload or Keep Mine) over unsaved changes, one prompt at a
//! time. Stats and reads run on the background executor.
//!
//! A buffer is checked once however many panes or splits show it, and not
//! while a save of it runs: then once the save is over, so a save's own
//! write is never taken for someone else's. Each step of a check holds to
//! the buffer's stamp as the check started: if a save or a reload moved it
//! meanwhile, what the step found is stale, and the check starts over.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::File,
    io,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use fe_buffer::{Buffer, BufferId, DecodedFile, DiskStamp, Text, local_history::Reason};
use fe_gpui::{EditorView, pane_item::file_name};
use fe_project::FsChange;
use gpui::{AppContext as _, Context, Entity, EntityId, PromptLevel, Subscription, Window};

use crate::Workbench;

/// The buttons of the prompt over a dirty buffer whose file changed.
const RELOAD_BUTTONS: [&str; 2] = ["Reload", "Keep Mine"];

/// The answers to [`RELOAD_BUTTONS`]. A prompt dismissed without either
/// (another prompt replaced it) changes nothing.
const RELOAD: usize = 0;
const KEEP_MINE: usize = 1;

/// A batch naming more paths than this checks every open buffer, as a
/// rescan does: a stat per buffer is cheaper than resolving every path.
const MAX_BATCH_PATHS: usize = 512;

/// How often buffers waiting for their save look again, in case no editor
/// of theirs is notified as it ends (the one saving closed).
const SAVE_POLL: Duration = Duration::from_millis(250);

/// The workbench's part: what's being checked, and what waits.
pub(crate) struct DiskSync {
    /// Per editor checked: its path, and the canonical path that resolves
    /// to, as first checked.
    canonical: HashMap<EntityId, (PathBuf, PathBuf)>,
    /// The buffers being checked (or waiting to ask, or asking), and
    /// whether each is to be checked again once that's done.
    checks: HashMap<BufferId, bool>,
    /// The buffers to check once their save is over, with the observations
    /// of the editors showing each.
    after_save: HashMap<BufferId, Vec<Subscription>>,
    /// Whether `after_save` is being polled.
    polling: bool,
    /// The dirty buffers whose files changed, waiting to ask in turn.
    asks: VecDeque<Ask>,
    /// Whether a prompt is up: the window shows one at a time, and a
    /// second would replace the first.
    asking: bool,
    /// To the window becoming active.
    _activation: Subscription,
    #[cfg(test)]
    pub(crate) test: TestCounts,
}

impl DiskSync {
    /// Checks the open buffers each time `window` becomes active.
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Workbench>) -> Self {
        let activation = cx.observe_window_activation(window, |workbench, window, cx| {
            if window.is_window_active() {
                workbench.check_open_files(window, cx);
            }
        });
        Self {
            canonical: HashMap::new(),
            checks: HashMap::new(),
            after_save: HashMap::new(),
            polling: false,
            asks: VecDeque::new(),
            asking: false,
            _activation: activation,
            #[cfg(test)]
            test: TestCounts::default(),
        }
    }
}

/// A check of `buffer`, begun when its stamp was `expected` and its file
/// `path`.
#[derive(Clone)]
struct Check {
    buffer: BufferId,
    expected: Option<DiskStamp>,
    /// What it checks: a step landing after the buffer was given another
    /// path (its file renamed) is about a file no longer its own.
    path: Rc<Path>,
}

/// A dirty buffer's changed file, to ask about.
struct Ask {
    check: Check,
    file: DecodedFile,
    stamp: DiskStamp,
}

/// A check's next step, decided on the main thread.
enum Next {
    Done,
    /// The file changed: read it.
    Read(PathBuf),
}

impl Workbench {
    /// A watcher batch: checks the buffers whose files it names, compared
    /// by canonical path, or every one for a rescan or a large batch.
    pub(crate) fn disk_fs_changed(
        &mut self,
        changes: &[FsChange],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if changes.contains(&FsChange::Rescan) || changes.len() > MAX_BATCH_PATHS {
            self.check_open_files(window, cx);
            return;
        }
        let editors = self.file_editors(cx);
        let open: HashSet<EntityId> = editors.iter().map(Entity::entity_id).collect();
        let canonical = &mut self.disk_sync.canonical;
        canonical.retain(|editor, _| open.contains(editor));
        let editors: Vec<(EntityId, PathBuf, Option<PathBuf>)> = (editors.iter())
            .filter_map(|editor| {
                let path = editor.read(cx).model().path()?.to_owned();
                let cached = (canonical.get(&editor.entity_id()))
                    .filter(|(cached, _)| *cached == path)
                    .map(|(_, canonical)| canonical.clone());
                Some((editor.entity_id(), path, cached))
            })
            .collect();
        let changed: Vec<PathBuf> = (changes.iter())
            .filter_map(|change| match change {
                FsChange::Path(path) => Some(path.clone()),
                FsChange::Rescan => None,
            })
            .collect();
        if editors.is_empty() || changed.is_empty() {
            return;
        }
        let matching = cx.background_spawn(async move { matching(editors, changed) });
        cx.spawn_in(window, async move |workbench, cx| {
            let (resolved, matched) = matching.await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    for (editor, path, canonical) in resolved {
                        let canonical = (path, canonical);
                        workbench.disk_sync.canonical.insert(editor, canonical);
                    }
                    let editors = (workbench.file_editors(cx).into_iter())
                        .filter(|editor| matched.contains(&editor.entity_id()));
                    workbench.check_buffers(editors, window, cx);
                })
                .ok();
        })
        .detach();
    }

    /// The editors showing `buffer`, a diff view's working side among them.
    fn buffer_editors(&self, buffer: BufferId, cx: &gpui::App) -> Vec<Entity<EditorView>> {
        let mut editors = self.file_editors(cx);
        editors.retain(|editor| editor.read(cx).model().buffer().id() == buffer);
        editors
    }

    /// Checks every open buffer that has a file.
    pub(crate) fn check_open_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editors = self.file_editors(cx);
        self.check_buffers(editors.into_iter(), window, cx);
    }

    /// Checks the buffers `editors` show, each once.
    fn check_buffers(
        &mut self,
        editors: impl Iterator<Item = Entity<EditorView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut seen = HashSet::new();
        for editor in editors {
            if seen.insert(editor.read(cx).model().buffer().id()) {
                self.check_file(&editor, window, cx);
            }
        }
    }

    /// Compares the file of `editor`'s buffer with the buffer's stamp, in
    /// the background, and acts on what changed. Once more after the check
    /// running, if one is; after the save, if one runs. A buffer that
    /// doesn't know the file it came from (see
    /// [`Buffer::disk_stamp_known`]) isn't checked, as a save doesn't check
    /// it either.
    fn check_file(
        &mut self,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = editor.read(cx);
        let Some(path) = view.model().path().map(Path::to_owned) else {
            return;
        };
        let model = view.model();
        if !model.buffer().disk_stamp_known() {
            return;
        }
        let check = Check {
            buffer: model.buffer().id(),
            expected: model.buffer().disk_stamp(),
            path: Rc::from(path.as_path()),
        };
        if let Some(again) = self.disk_sync.checks.get_mut(&check.buffer) {
            *again = true;
            return;
        }
        if view.is_saving() {
            self.check_after_save(check.buffer, window, cx);
            return;
        }
        self.disk_sync.checks.insert(check.buffer, false);
        #[cfg(test)]
        let ((stat_ran, stat_gate), (read_ran, read_gate)) = {
            let test = &mut self.disk_sync.test;
            test.checks += 1;
            (test.job(false, cx), test.job(true, cx))
        };
        let stat_path = path.clone();
        let stat = cx.background_spawn(async move {
            let stat = DiskStamp::of(stat_path);
            #[cfg(test)]
            {
                stat_ran();
                stat_gate.await;
            }
            stat
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let stat = stat.await;
            let next = workbench.update_in(cx, |workbench, window, cx| {
                workbench.stat_landed(check.clone(), path, stat, window, cx)
            });
            let Ok(Next::Read(path)) = next else {
                return;
            };
            let read = cx
                .background_spawn(async move {
                    let read = File::open(path).and_then(Buffer::read_file);
                    #[cfg(test)]
                    {
                        read_ran();
                        read_gate.await;
                    }
                    read
                })
                .await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench.read_landed(check, read, window, cx)
                })
                .ok();
        })
        .detach();
    }

    /// Checks `buffer` once the save of it running is over: as an editor of
    /// it is notified then, or, if none is (the one saving closed), when
    /// the poll finds it over.
    fn check_after_save(&mut self, buffer: BufferId, window: &mut Window, cx: &mut Context<Self>) {
        if self.disk_sync.after_save.contains_key(&buffer) {
            return;
        }
        let observed = (self.buffer_editors(buffer, cx).iter())
            .map(|editor| {
                cx.observe_in(editor, window, move |workbench, _, window, cx| {
                    workbench.save_maybe_over(buffer, window, cx)
                })
            })
            .collect();
        self.disk_sync.after_save.insert(buffer, observed);
        if self.disk_sync.polling {
            return;
        }
        self.disk_sync.polling = true;
        cx.spawn_in(window, async move |workbench, cx| {
            loop {
                cx.background_executor().timer(SAVE_POLL).await;
                let waiting = workbench.update_in(cx, |workbench, window, cx| {
                    let waiting: Vec<_> = workbench.disk_sync.after_save.keys().copied().collect();
                    for buffer in waiting {
                        workbench.save_maybe_over(buffer, window, cx);
                    }
                    let waiting = !workbench.disk_sync.after_save.is_empty();
                    workbench.disk_sync.polling = waiting;
                    waiting
                });
                if !matches!(waiting, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Checks `buffer`, if it waits for its save and that's over.
    fn save_maybe_over(&mut self, buffer: BufferId, window: &mut Window, cx: &mut Context<Self>) {
        if !self.disk_sync.after_save.contains_key(&buffer) {
            return;
        }
        let editor = self.buffer_editors(buffer, cx).into_iter().next();
        if editor
            .as_ref()
            .is_some_and(|editor| editor.read(cx).is_saving())
        {
            return;
        }
        self.disk_sync.after_save.remove(&buffer);
        if let Some(editor) = editor {
            self.check_file(&editor, window, cx);
        }
    }

    /// The editor of `check`'s buffer to act on, as a step of the check
    /// lands. None if it's closed, if a save of it runs (the check then
    /// waits for the save), or if its stamp or path isn't the one the check
    /// began with (a save or reload landed meanwhile, or its file was
    /// renamed: the check starts over, on the path it has now).
    fn checked_editor(
        &mut self,
        check: &Check,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<EditorView>> {
        let Some(editor) = self.buffer_editors(check.buffer, cx).into_iter().next() else {
            self.disk_sync.checks.remove(&check.buffer);
            return None;
        };
        let view = editor.read(cx);
        let stale = view.model().buffer().disk_stamp() != check.expected
            || view.model().path() != Some(&*check.path);
        if view.is_saving() {
            self.disk_sync.checks.remove(&check.buffer);
            self.check_after_save(check.buffer, window, cx);
            None
        } else if stale {
            self.disk_sync.checks.remove(&check.buffer);
            self.check_file(&editor, window, cx);
            None
        } else {
            Some(editor)
        }
    }

    /// The file's stamp, from the background: nothing to do if it's the
    /// buffer's; a missing file clears the buffer's stamp, so the next save
    /// recreates it without asking; else the file's to be read.
    fn stat_landed(
        &mut self,
        check: Check,
        path: PathBuf,
        stat: io::Result<Option<DiskStamp>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Next {
        let Some(editor) = self.checked_editor(&check, window, cx) else {
            return Next::Done;
        };
        let next = match stat {
            Ok(stamp) if stamp == check.expected => Next::Done,
            Ok(None) => {
                editor.update(cx, |editor, _| {
                    editor.model_mut().buffer_mut().set_disk_stamp(None)
                });
                Next::Done
            }
            Ok(Some(_)) => Next::Read(path),
            Err(_) => Next::Done,
        };
        if matches!(next, Next::Done) {
            self.finish_check(check.buffer, window, cx);
        }
        next
    }

    /// The changed file, read and decoded in the background: its text
    /// already the buffer's only takes its stamp; a clean buffer reloads; a
    /// dirty one asks, once no other prompt is up.
    fn read_landed(
        &mut self,
        check: Check,
        read: io::Result<(DecodedFile, DiskStamp)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.checked_editor(&check, window, cx) else {
            return;
        };
        let Ok((file, stamp)) = read else {
            self.finish_check(check.buffer, window, cx);
            return;
        };
        let (same, dirty) = {
            let buffer = editor.read(cx).model().buffer();
            (same_text(buffer.text(), &file.text), buffer.is_dirty())
        };
        if !same && let Some(history) = fe_gpui::pane_item::local_history(cx) {
            let path = editor.read(cx).model().path().map(Path::to_owned);
            if let Some(path) = path {
                fe_gpui::pane_item::record_history(
                    history,
                    path,
                    file.text.clone(),
                    Reason::External,
                    cx.background_executor(),
                );
            }
        }
        if same {
            set_stamp(&editor, stamp, cx);
        } else if !dirty {
            reload(&editor, file, stamp, cx);
        } else {
            let ask = Ask { check, file, stamp };
            self.disk_sync.asks.push_back(ask);
            self.ask_next(window, cx);
            return;
        }
        self.finish_check(check.buffer, window, cx);
    }

    /// Asks about the next buffer waiting to, unless a prompt is up.
    fn ask_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while !self.disk_sync.asking {
            let Some(ask) = self.disk_sync.asks.pop_front() else {
                return;
            };
            let Some(editor) = self.checked_editor(&ask.check, window, cx) else {
                continue;
            };
            let (dirty, name) = {
                let model = editor.read(cx).model();
                (model.is_dirty(), file_name(model.path()))
            };
            if !dirty {
                // Saved or undone while it waited.
                reload(&editor, ask.file, ask.stamp, cx);
                self.finish_check(ask.check.buffer, window, cx);
                continue;
            }
            let answer = window.prompt(
                PromptLevel::Warning,
                &format!("“{name}” changed on disk."),
                Some("Reload it and lose your unsaved changes?"),
                &RELOAD_BUTTONS,
                cx,
            );
            self.disk_sync.asking = true;
            cx.spawn_in(window, async move |workbench, cx| {
                let answer = answer.await.ok();
                workbench
                    .update_in(cx, |workbench, window, cx| {
                        workbench.answered(ask, answer, window, cx)
                    })
                    .ok();
            })
            .detach();
        }
    }

    /// The answer about a dirty buffer: Reload takes the file's text, Keep
    /// Mine its stamp, so the next save overwrites it without asking. No
    /// answer (the prompt was replaced) changes nothing, so the next save
    /// still asks before it overwrites.
    fn answered(
        &mut self,
        ask: Ask,
        answer: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.disk_sync.asking = false;
        if let Some(editor) = self.checked_editor(&ask.check, window, cx) {
            match answer {
                Some(RELOAD) => reload(&editor, ask.file, ask.stamp, cx),
                Some(KEEP_MINE) => set_stamp(&editor, ask.stamp, cx),
                _ => {}
            }
            self.finish_check(ask.check.buffer, window, cx);
        }
        self.ask_next(window, cx);
    }

    /// Ends `buffer`'s check, and checks it again if that was asked for
    /// meanwhile.
    fn finish_check(&mut self, buffer: BufferId, window: &mut Window, cx: &mut Context<Self>) {
        if self.disk_sync.checks.remove(&buffer) == Some(true)
            && let Some(editor) = self.buffer_editors(buffer, cx).into_iter().next()
        {
            self.check_file(&editor, window, cx);
        }
    }
}

/// Reloads `editor`'s buffer from `file`, as one undoable edit; the
/// workbench passes the notification on to its splits.
fn reload(editor: &Entity<EditorView>, file: DecodedFile, stamp: DiskStamp, cx: &mut gpui::App) {
    editor.update(cx, |editor, cx| {
        editor.model_mut().reload(file, Some(stamp));
        cx.notify();
    });
}

fn set_stamp(editor: &Entity<EditorView>, stamp: DiskStamp, cx: &mut gpui::App) {
    editor.update(cx, |editor, _| {
        editor.model_mut().buffer_mut().set_disk_stamp(Some(stamp))
    });
}

/// Whether `a` and `b` hold the same text.
fn same_text(a: &Text, b: &Text) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let (mut a, mut b) = (a.chunks().map(str::as_bytes), b.chunks().map(str::as_bytes));
    let (mut x, mut y): (&[u8], &[u8]) = (&[], &[]);
    loop {
        if x.is_empty() {
            match a.next() {
                Some(chunk) => x = chunk,
                None => return true,
            }
            continue;
        }
        if y.is_empty() {
            match b.next() {
                Some(chunk) => y = chunk,
                None => return true,
            }
            continue;
        }
        let n = x.len().min(y.len());
        if x[..n] != y[..n] {
            return false;
        }
        (x, y) = (&x[n..], &y[n..]);
    }
}

/// Off the main thread: resolves each editor's path, where it's not cached,
/// and each changed path, and returns the editors resolved, with those
/// whose files `changed` names, as given or resolved.
fn matching(
    editors: Vec<(EntityId, PathBuf, Option<PathBuf>)>,
    changed: Vec<PathBuf>,
) -> (Vec<(EntityId, PathBuf, PathBuf)>, HashSet<EntityId>) {
    let mut resolved = Vec::new();
    let editors: Vec<(EntityId, PathBuf, PathBuf)> = (editors.into_iter())
        .map(|(editor, path, cached)| {
            let canonical = cached.unwrap_or_else(|| {
                let canonical = canonicalize(&path);
                resolved.push((editor, path.clone(), canonical.clone()));
                canonical
            });
            (editor, path, canonical)
        })
        .collect();
    let changed: HashSet<PathBuf> = (changed.into_iter())
        .flat_map(|path| [canonicalize(&path), path])
        .collect();
    let matched = (editors.into_iter())
        .filter(|(_, path, canonical)| changed.contains(path) || changed.contains(canonical))
        .map(|(editor, _, _)| editor)
        .collect();
    (resolved, matched)
}

/// `path` with its symlinks resolved; for a missing file, its directory's.
fn canonicalize(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    match (path.parent(), path.file_name()) {
        (Some(dir), Some(name)) => match std::fs::canonicalize(dir) {
            Ok(dir) => dir.join(name),
            Err(_) => path.to_owned(),
        },
        _ => path.to_owned(),
    }
}

/// What the tests count and hold.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestCounts {
    /// Buffers' checks started.
    pub(crate) checks: usize,
    /// Stats run, and of them those off the main thread.
    pub(crate) stats: std::sync::Arc<std::sync::Mutex<(usize, usize)>>,
    /// Reads run, and of them those off the main thread.
    pub(crate) reads: std::sync::Arc<std::sync::Mutex<(usize, usize)>>,
    /// While set, the stats (reads) of checks started from now wait, once
    /// run, for [`TestCounts::release`] before they land.
    pub(crate) hold_stats: bool,
    pub(crate) hold_reads: bool,
    held: Vec<futures::channel::oneshot::Sender<()>>,
}

#[cfg(test)]
impl TestCounts {
    /// What a stat or a read job calls as it runs, to count it, and the
    /// gate it then awaits.
    fn job(
        &mut self,
        read: bool,
        cx: &gpui::App,
    ) -> (
        impl FnOnce() + Send + 'static,
        impl Future<Output = ()> + Send + 'static,
    ) {
        let (release, gate) = futures::channel::oneshot::channel();
        match if read {
            self.hold_reads
        } else {
            self.hold_stats
        } {
            true => self.held.push(release),
            false => drop(release.send(())),
        }
        let count = if read { &self.reads } else { &self.stats }.clone();
        let executor = cx.background_executor().clone();
        let ran = move || {
            let mut count = count.lock().unwrap();
            count.0 += 1;
            count.1 += usize::from(!executor.is_main_thread());
        };
        (ran, async move {
            gate.await.ok();
        })
    }

    /// Lets the held jobs land, and holds no more.
    pub(crate) fn release(&mut self) {
        (self.hold_stats, self.hold_reads) = (false, false);
        for release in self.held.drain(..) {
            release.send(()).ok();
        }
    }
}

#[cfg(test)]
#[path = "disk_sync_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "disk_sync_rename_tests.rs"]
mod disk_sync_rename_tests;

#[cfg(test)]
#[path = "local_history_record_tests.rs"]
mod local_history_record_tests;
