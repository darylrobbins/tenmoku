//! Hot exit: quitting doesn't ask about unsaved changes, and the next
//! launch brings them back with the layout, untitled buffers too; a crash
//! loses at most the last second of typing.
//!
//! A window with a saved layout ([`Workbench::set_layout`]) keeps its
//! buffers' backups in its root's [`BackupStore`], `<state_dir>/backups/<key>`
//! beside its layouts' `<state_dir>/layouts/<key>.json`, and gives fe-gpui
//! the store for the editors it restores
//! ([`pane_item::set_window_backups`]). Each open buffer, once however many
//! editors show it, has a backup ([`BufferBackup`]), which it owns while
//! it's on disk:
//!
//! - while it's dirty, it's written off the main thread [`BACKUP_DELAY`]
//!   after its last edit, or its last change of path, save format or file
//!   on disk ([`BackupRevision`]), one write at a time, to the slot that
//!   doesn't hold the newest backup, so a crash mid-write leaves the one
//!   before;
//! - once it's clean (saved, or undone to clean), or its last editor has
//!   closed other than by quitting, it's removed;
//! - a write or removal that fails is tried again, after a wait doubling
//!   up to [`RETRY_MAX`], until it's done or overtaken: a removal for a
//!   closed buffer, however long it takes, and quitting finishes any still
//!   to do;
//! - once a write or removal is made, the layout is saved again, with the
//!   editors' carets, so a crash restores as a quit does.
//!
//! `app.quit` asks the windows' items through a close transaction of its
//! own, in which a dirty editor writes its backup and closes without
//! asking ([`pane_item::back_up_to_close`]); any other close, of a tab or
//! a window, asks as ever, even with a quit waiting behind it. The layout
//! saved at quit is saved once every backup has been written.
//!
//! The first window on a root in the process opens as tabs, once its own
//! layout is restored, the root's backups that no saved layout names and
//! no live editor, in this process or another, owns. One that doesn't read
//! (its file can't be opened, say) is left for the next launch. Then the
//! store is pruned of half-written backups, of the corrupt slots (text and
//! meta disagreeing) of backups neither of whose slots reads, and of owner
//! locks left by gone backups; nothing is pruned if the store can't be
//! listed.

use std::{
    collections::{BTreeSet, HashMap, HashSet, hash_map::Entry},
    io,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use fe_buffer::{BackupId, BackupStore, Buffer, BufferId};
use fe_command::Value;
use fe_editor::EditorModel;
use fe_gpui::{
    EditorView,
    pane_item::{self, BackupRevision, BufferBackup},
};
use gpui::{
    AnyView, App, AppContext as _, BackgroundExecutor, Context, Entity, FocusHandle, Global, Hsla,
    SharedString, Subscription, Task, Window, WindowId,
};
use kiln_shell::{
    CloseAnswer, CloseKind, CloseRequest, Closing, Icon, ItemCrumb, ItemHandle, ItemId, StatusItem,
    close::{self, Items, LiveItems},
    layout::LayoutStore,
};

use super::Workbench;

/// How long after its last edit a dirty buffer is backed up.
pub(crate) const BACKUP_DELAY: Duration = Duration::from_secs(1);

/// The longest a failed write or removal of a backup waits to be tried
/// again.
pub(crate) const RETRY_MAX: Duration = Duration::from_secs(30);

/// A window's backups: see the module docs.
#[derive(Default)]
pub(super) struct HotExit {
    /// The window's, while it has a store.
    window: Option<WindowId>,
    store: Option<BackupStore>,
    buffers: HashMap<BufferId, Tracked>,
    /// The backups the layout last saved named.
    saved: HashSet<String>,
    /// The root's backups to open as tabs, read, once the layout is in.
    orphans: Option<Orphans>,
}

/// An open buffer's backup.
struct Tracked {
    backup: BufferBackup,
    /// What was last backed up, or is waiting to be, while it's dirty.
    revision: Option<BackupRevision>,
    /// The wait for its next write and then the write, or its removal.
    pending: Option<Task<()>>,
}

/// What a launch found in the store.
struct Orphans {
    /// Whether the store could be listed: if not, nothing is pruned.
    listed: bool,
    /// The slots whose text and meta disagree, of backups neither of whose
    /// slots reads ([`pane_item::read_newest`]).
    corrupt: Vec<BackupId>,
    /// The backups no saved layout names and no one owned, now owned, read.
    read: Vec<(BufferBackup, Buffer, Option<PathBuf>)>,
}

/// What hot exit keeps for the app: the stores whose orphans a window has
/// taken, whether `app.quit` is hot exit's, the closed buffers' backups
/// still to remove, and, in tests, what a quit runs once its items agree.
#[derive(Default)]
struct HotExitApp {
    started: HashSet<PathBuf>,
    quits: bool,
    /// The removals of closed buffers' backups not yet done, which quitting
    /// runs ([`flush_cleanups`]).
    cleanups: Cleanups,
    #[cfg(test)]
    quit: Option<Rc<Quit>>,
}

/// The removals still to do of closed buffers' backups, by id.
type Cleanups = Arc<Mutex<HashMap<BackupId, BufferBackup>>>;

/// What a quit runs, in tests.
#[cfg(test)]
type Quit = dyn Fn(&mut App);

impl Global for HotExitApp {}

impl Workbench {
    /// Keeps the window's backups in the store beside `layouts` from now
    /// on; the first window on its root in the process takes its orphans.
    /// See the module docs.
    pub(super) fn start_hot_exit(
        &mut self,
        layouts: &LayoutStore,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dir) = backups_dir(layouts) else {
            return;
        };
        let store = BackupStore::new(&dir);
        let id = window.window_handle().window_id();
        pane_item::set_window_backups(id, Some(store.clone()), cx);
        self.hot_exit.window = Some(id);
        self.hot_exit.store = Some(store.clone());
        take_quits(cx);
        if cx.default_global::<HotExitApp>().started.insert(dir) {
            let named: HashSet<BackupId> = (layouts.entries().iter())
                .flat_map(named)
                .filter_map(|id| id.parse().ok())
                .collect();
            let read = cx.background_spawn(async move { orphans(&store, &named) });
            cx.spawn(async move |workbench, cx| {
                let orphans = read.await;
                (workbench.update(cx, |workbench, cx| {
                    workbench.hot_exit.orphans = Some(orphans);
                    workbench.adopt_orphans(cx);
                }))
                .ok();
            })
            .detach();
        }
        self.sync_backups(cx);
    }

    /// Opens the orphans read, unless the layout is still being restored,
    /// as tabs in the active pane, then prunes the store.
    pub(super) fn adopt_orphans(&mut self, cx: &mut Context<Self>) {
        if self.restoring {
            return;
        }
        let (Some(orphans), Some(store)) =
            (self.hot_exit.orphans.take(), self.hot_exit.store.clone())
        else {
            return;
        };
        let Orphans {
            listed,
            corrupt,
            read,
        } = orphans;
        for (backup, buffer, path) in read {
            let model = EditorModel::new(buffer, path);
            self.active.update(cx, |pane, cx| pane.add(model, cx));
            if let Some(editor) = self.active.read(cx).active_editor().cloned() {
                pane_item::adopt_backup(&editor, backup, cx);
            }
        }
        self.track_editors(cx);
        if listed {
            cx.background_spawn(async move { prune(&store, &corrupt) })
                .detach();
        }
    }

    /// Follows the open buffers: a dirty one is backed up a moment after
    /// it last changed, a clean one's backup is removed, and so is that of
    /// one whose last editor has gone, unless the app is quitting.
    pub(super) fn sync_backups(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.hot_exit.store.clone() else {
            return;
        };
        let mut open: Vec<(BufferId, Entity<EditorView>)> = Vec::new();
        for editor in self.open_editors(cx) {
            let buffer = editor.read(cx).model().buffer().id();
            if !open.iter().any(|(id, _)| *id == buffer) {
                open.push((buffer, editor));
            }
        }
        let closed: Vec<BufferId> = (self.hot_exit.buffers.keys())
            .filter(|buffer| !open.iter().any(|(id, _)| id == *buffer))
            .copied()
            .collect();
        let quitting = kiln_shell::layout::quitting(cx);
        for buffer in closed {
            let Some(tracked) = self.hot_exit.buffers.remove(&buffer) else {
                continue;
            };
            pane_item::set_buffer_backup(buffer, None, cx);
            if !quitting {
                remove_detached(&tracked.backup, cx);
            }
        }
        for (buffer, editor) in open {
            let (dirty, revision) = {
                let editor = editor.read(cx);
                (editor.model().is_dirty(), BackupRevision::of(editor))
            };
            let tracked = match self.hot_exit.buffers.entry(buffer) {
                Entry::Occupied(tracked) => tracked.into_mut(),
                Entry::Vacant(slot) => {
                    let backup = pane_item::buffer_backup(buffer, cx)
                        .unwrap_or_else(|| BufferBackup::new(store.clone(), BackupId::new()));
                    pane_item::set_buffer_backup(buffer, Some(backup.clone()), cx);
                    slot.insert(Tracked {
                        backup,
                        revision: None,
                        pending: None,
                    })
                }
            };
            if dirty {
                if tracked.revision.as_ref() != Some(&revision) {
                    tracked.revision = Some(revision);
                    tracked.pending = Some(write_later(editor, tracked.backup.clone(), cx));
                }
            } else if tracked.revision.take().is_some() {
                tracked.pending = Some(remove_now(&tracked.backup, cx));
            }
        }
    }

    /// Writes, now, each dirty buffer's backup that's behind it, and
    /// removes those of clean and closed buffers still to go: as the app
    /// quits.
    pub(super) fn flush_backups(&mut self, cx: &mut Context<Self>) {
        flush_cleanups(cx);
        if self.hot_exit.store.is_none() {
            return;
        }
        for editor in self.open_editors(cx) {
            let editor = editor.read(cx);
            let buffer = editor.model().buffer().id();
            let Some(tracked) = self.hot_exit.buffers.get_mut(&buffer) else {
                continue;
            };
            let dirty = editor.model().is_dirty();
            let job = match (dirty, tracked.pending.take()) {
                (true, _) if !tracked.backup.holds(&BackupRevision::of(editor)) => {
                    tracked.backup.write_job(editor)
                }
                (false, Some(_)) => tracked.backup.remove_job(),
                _ => continue,
            };
            if let Err(error) = job.run() {
                log::error!("can't back up an unsaved buffer at quit: {error}");
            }
        }
    }

    /// The window has closed: unless the app is quitting, its backups go,
    /// as it asked about its unsaved changes first.
    pub(super) fn end_hot_exit(&mut self, cx: &mut App) {
        if let Some(window) = self.hot_exit.window.take() {
            pane_item::set_window_backups(window, None, cx);
        }
        if self.hot_exit.store.take().is_none() {
            return;
        }
        let quitting = kiln_shell::layout::quitting(cx);
        for (buffer, tracked) in self.hot_exit.buffers.drain() {
            pane_item::set_buffer_backup(buffer, None, cx);
            if !quitting {
                remove_detached(&tracked.backup, cx);
            }
        }
    }

    /// Whether `entry`, about to be saved, names other backups than the
    /// layout last saved did; noting them as saved if so.
    pub(super) fn backups_changed(&mut self, entry: &Value) -> bool {
        let named = named(entry);
        let changed = named != self.hot_exit.saved;
        self.hot_exit.saved = named;
        changed
    }
}

/// Writes `backup` of `editor`'s buffer [`BACKUP_DELAY`] from now, if it's
/// dirty then and the backup is behind it, then saves the layout in full.
/// A write that fails is tried again, as the buffer is then, after a wait
/// that doubles each time, up to [`RETRY_MAX`].
fn write_later(
    editor: Entity<EditorView>,
    backup: BufferBackup,
    cx: &mut Context<Workbench>,
) -> Task<()> {
    cx.spawn(async move |workbench, cx| {
        let mut wait = BACKUP_DELAY;
        loop {
            cx.background_executor().timer(wait).await;
            let job = editor.read_with(cx, |editor, _| {
                let behind = !backup.holds(&BackupRevision::of(editor));
                (editor.model().is_dirty() && behind).then(|| backup.write_job(editor))
            });
            let Some(job) = job else {
                return;
            };
            match cx.background_spawn(async move { job.run() }).await {
                Ok(()) => break,
                Err(error) => {
                    log::error!("can't back up an unsaved buffer, so trying again: {error}")
                }
            }
            wait = (wait * 2).min(RETRY_MAX);
        }
        (workbench.update(cx, |workbench, cx| workbench.write_layout(true, cx))).ok();
    })
}

/// Removes `backup` off the main thread ([`remove`]), then saves the
/// layout in full.
fn remove_now(backup: &BufferBackup, cx: &mut Context<Workbench>) -> Task<()> {
    let backup = backup.clone();
    cx.spawn(async move |workbench, cx| {
        remove(&backup, cx.background_executor()).await;
        (workbench.update(cx, |workbench, cx| workbench.write_layout(true, cx))).ok();
    })
}

/// Removes `backup` off the main thread ([`remove`]), however long it
/// takes, as its buffer has gone; quitting first finishes it, if it's still
/// to do ([`flush_cleanups`]).
fn remove_detached(backup: &BufferBackup, cx: &mut App) {
    let cleanups = cx.default_global::<HotExitApp>().cleanups.clone();
    lock(&cleanups).insert(backup.id(), backup.clone());
    let backup = backup.clone();
    let executor = cx.background_executor().clone();
    cx.background_spawn(async move {
        remove(&backup, &executor).await;
        lock(&cleanups).remove(&backup.id());
    })
    .detach();
}

/// Runs, now, the removals of closed buffers' backups still to do: as the
/// app quits.
pub(super) fn flush_cleanups(cx: &App) {
    let Some(app) = cx.try_global::<HotExitApp>() else {
        return;
    };
    let cleanups: Vec<BufferBackup> = lock(&app.cleanups)
        .drain()
        .map(|(_, backup)| backup)
        .collect();
    for backup in cleanups {
        if let Err(error) = backup.remove_job().run() {
            log::error!("can't remove a closed buffer's backup at quit: {error}");
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Removes `backup` on `executor`, trying again after a failure, after a
/// wait that doubles each time, up to [`RETRY_MAX`].
async fn remove(backup: &BufferBackup, executor: &BackgroundExecutor) {
    let mut wait = BACKUP_DELAY;
    loop {
        let job = backup.remove_job();
        match executor.spawn(async move { job.run() }).await {
            Ok(()) => return,
            Err(error) => log::error!("can't remove a backup, so trying again: {error}"),
        }
        executor.timer(wait).await;
        wait = (wait * 2).min(RETRY_MAX);
    }
}

/// Has `app.quit` be hot exit's ([`quit`]), once.
fn take_quits(cx: &mut App) {
    let app = cx.default_global::<HotExitApp>();
    if std::mem::replace(&mut app.quits, true) {
        return;
    }
    // Windows that closed before the quit have no hook of their own.
    cx.on_app_quit(|cx| {
        flush_cleanups(cx);
        async {}
    })
    .detach();
    kiln_platform::commands::set_close_handlers(
        cx,
        quit,
        // As kiln-shell's own.
        |handle, cx| match close::close_window(handle, None, cx) {
            Closing::Now => {
                (handle.update(cx, |_, window, _| window.remove_window())).ok();
            }
            Closing::Refused => {}
            Closing::Later(closing) => closing.detach(),
        },
    );
}

/// Quits once every window's items agree, as kiln-shell's `app.quit` does,
/// but through items that, in this close alone, write a dirty editor's
/// backup and close it without asking ([`BackedUp`]). It runs once the
/// current update is over, so that no window is being updated.
fn quit(cx: &mut App) {
    cx.defer(|cx| {
        let mut request = CloseRequest::new(CloseKind::Quit);
        for handle in cx.windows() {
            if close::window_items(handle, cx).is_none() {
                continue;
            }
            let live: LiveItems = Rc::new(move |cx| {
                let open: Vec<Box<dyn ItemHandle>> = (close::window_items(handle, cx)?)
                    .into_iter()
                    .map(|item| Box::new(BackedUp(item)) as Box<dyn ItemHandle>)
                    .collect();
                Some(Items {
                    closing: open.clone(),
                    open,
                })
            });
            request = request.scope(handle, None, live);
        }
        match close::confirm(request, |_, cx| run_quit(cx), None, cx) {
            Closing::Now => run_quit(cx),
            Closing::Refused => {}
            Closing::Later(task) => task.detach(),
        }
    });
}

/// What a quit runs once its items agree.
fn run_quit(cx: &mut App) {
    #[cfg(test)]
    if let Some(quit) = (cx.try_global::<HotExitApp>()).and_then(|app| app.quit.clone()) {
        return quit(cx);
    }
    cx.quit();
}

/// Has a quit run `quit` once its items agree, in place of quitting.
#[cfg(test)]
pub(super) fn set_quit(cx: &mut App, quit: impl Fn(&mut App) + 'static) {
    cx.default_global::<HotExitApp>().quit = Some(Rc::new(quit));
}

/// An item as a quit asks it: a dirty editor whose changes are in its
/// backup ([`pane_item::back_up_to_close`]) may close without asking.
/// Everything else is the item's own.
struct BackedUp(Box<dyn ItemHandle>);

impl ItemHandle for BackedUp {
    fn may_close(&self, window: &mut Window, cx: &mut App) -> CloseAnswer {
        if let Ok(editor) = self.0.to_any_view().downcast::<EditorView>() {
            let editor = editor.read(cx);
            if editor.model().is_dirty() && pane_item::back_up_to_close(editor, cx) {
                return CloseAnswer::Close;
            }
        }
        self.0.may_close(window, cx)
    }

    fn boxed_clone(&self) -> Box<dyn ItemHandle> {
        Box::new(BackedUp(self.0.boxed_clone()))
    }

    fn id(&self) -> ItemId {
        self.0.id()
    }

    fn kind(&self) -> &'static str {
        self.0.kind()
    }

    fn to_any_view(&self) -> AnyView {
        self.0.to_any_view()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self.0.as_any()
    }

    fn item_focus_handle(&self, cx: &App) -> FocusHandle {
        self.0.item_focus_handle(cx)
    }

    fn observe(&self, changed: Box<dyn FnMut(&mut App)>, cx: &mut App) -> Subscription {
        self.0.observe(changed, cx)
    }

    fn observe_in(
        &self,
        window: &mut Window,
        changed: Box<dyn FnMut(&mut Window, &mut App)>,
        cx: &mut App,
    ) -> Subscription {
        self.0.observe_in(window, changed, cx)
    }

    fn tab_title(&self, cx: &App) -> SharedString {
        self.0.tab_title(cx)
    }

    fn tab_icon(&self, cx: &App) -> Option<Icon> {
        self.0.tab_icon(cx)
    }

    fn window_title(&self, cx: &App) -> SharedString {
        self.0.window_title(cx)
    }

    fn tab_title_color(&self, cx: &App) -> Option<Hsla> {
        self.0.tab_title_color(cx)
    }

    fn is_dirty(&self, cx: &App) -> bool {
        self.0.is_dirty(cx)
    }

    fn shares_document(&self, other: &dyn ItemHandle, cx: &App) -> bool {
        self.0.shares_document(other, cx)
    }

    fn close_revision(&self, cx: &App) -> u64 {
        self.0.close_revision(cx)
    }

    fn save(&self, window: &mut Window, cx: &mut App) -> Option<Task<gpui::Result<()>>> {
        self.0.save(window, cx)
    }

    fn path(&self, cx: &App) -> Option<PathBuf> {
        self.0.path(cx)
    }

    fn breadcrumbs(&self, cx: &App) -> Vec<ItemCrumb> {
        self.0.breadcrumbs(cx)
    }

    fn status_items(&self, cx: &App) -> Vec<StatusItem> {
        self.0.status_items(cx)
    }

    fn split(&self, window: &mut Window, cx: &mut App) -> Option<Box<dyn ItemHandle>> {
        self.0.split(window, cx)
    }

    fn serialize(&self, cx: &App) -> Option<Value> {
        self.0.serialize(cx)
    }

    fn apply_state(&self, state: &Value, window: &mut Window, cx: &mut App) {
        self.0.apply_state(state, window, cx)
    }
}

/// `<state_dir>/backups/<key>` for `layouts`' `<state_dir>/layouts/<key>.json`.
fn backups_dir(layouts: &LayoutStore) -> Option<PathBuf> {
    let file = layouts.file();
    let state = file.parent()?.parent()?;
    Some(state.join("backups").join(file.file_stem()?))
}

/// The backups a window's saved entry names, by id.
pub(super) fn named(entry: &Value) -> HashSet<String> {
    fn walk(value: &Value, named: &mut HashSet<String>) {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    match (key.as_str(), value) {
                        ("backup", Value::String(id)) => {
                            named.insert(id.clone());
                        }
                        _ => walk(value, named),
                    }
                }
            }
            Value::Array(values) => values.iter().for_each(|value| walk(value, named)),
            _ => {}
        }
    }
    let mut found = HashSet::new();
    if let Some(panes) = entry.get("panes") {
        walk(panes, &mut found);
    }
    found
}

/// The backups in `store` that `named` leaves out and no one owns, each
/// claimed and restored over its file ([`pane_item::restore_backup`]).
/// One whose text and meta disagree is noted as corrupt; one that fails to
/// read otherwise is let go, for the next launch.
fn orphans(store: &BackupStore, named: &HashSet<BackupId>) -> Orphans {
    let mut orphans = Orphans {
        listed: true,
        corrupt: Vec::new(),
        read: Vec::new(),
    };
    let ids = match store.ids() {
        Ok(ids) => ids,
        Err(error) => {
            log::error!(
                "can't list the backups in {}: {error}",
                store.dir().display()
            );
            orphans.listed = false;
            return orphans;
        }
    };
    let backups: BTreeSet<BackupId> = ids.into_iter().map(pane_item::primary).collect();
    for id in backups.into_iter().filter(|id| !named.contains(id)) {
        let backup = match BufferBackup::claim(store.clone(), id) {
            Ok(Some(backup)) => backup,
            Ok(None) => continue,
            Err(error) => {
                log::info!("not reopening backup {id}: {error}");
                continue;
            }
        };
        let read = backup.read().and_then(|read| {
            pane_item::restore_backup(&read).map_err(|error| io::Error::other(error.to_string()))
        });
        match read {
            Ok((buffer, path)) => orphans.read.push((backup, buffer, path)),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                log::error!("backup {id} is corrupt: {error}");
                let slots = [id, pane_item::twin(id)].into_iter();
                let corrupt = slots.filter(|slot| {
                    (store.read(*slot))
                        .is_err_and(|error| error.kind() == io::ErrorKind::InvalidData)
                });
                orphans.corrupt.extend(corrupt);
            }
            Err(error) => log::info!("not reopening backup {id} yet: {error}"),
        }
    }
    orphans
}

/// Prunes `store` of half-written backups and `corrupt` slots, then of the owner files no backup has and no one holds.
fn prune(store: &BackupStore, corrupt: &[BackupId]) {
    let pruned = store
        .prune(|id| !corrupt.contains(id))
        .and_then(|()| pane_item::prune_owners(store, &store.ids()?));
    if let Err(error) = pruned {
        log::error!(
            "can't prune the backups in {}: {error}",
            store.dir().display()
        );
    }
}

#[cfg(test)]
#[path = "hot_exit_tests.rs"]
mod tests;
