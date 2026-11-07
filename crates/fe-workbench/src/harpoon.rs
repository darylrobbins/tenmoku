//! Harpoon-style file marks: a short per-project list of files, kept by
//! [`fe_config::harpoon`] in `<state_dir>/harpoon/`, each in a numbered
//! slot to jump to.
//!
//! The commands (`workbench_commands.rs`) act on the window's root's list:
//! `workbench.harpoon_add` (Harpoon: Add File) marks the active tab's file
//! in the next slot, saying so if it's there already or the list is full;
//! `workbench.harpoon_remove_current` unmarks it; `workbench.harpoon_jump1`
//! to `9` open the file in that slot, through the active pane's open, so a
//! tab already open is activated rather than opened again, a tab opened
//! under another spelling of the path (through a symlink) included. A jump
//! lands only if it's the window's latest and nothing moved while the disk
//! was read: a tab or pane chosen meanwhile stays. A typed command
//! takes no args, so there's an id per slot, as `open_editor_at_index1`
//! has. A marked file that's gone stays listed, so a branch switch doesn't
//! lose it: jumping to it says it's missing. A window with no root has no
//! list: each command says to open a folder.
//!
//! `workbench.harpoon_toggle_list` (Harpoon: Show List) shows, or hides, a
//! [`HarpoonPicker`]: each row its slot, the file's name and its folder,
//! dimmed, a missing file's row dimmed whole. Enter opens the row's file
//! as a jump does, the disk checked again first: a file gone since is said,
//! its row flagged, and the picker stays;
//! `ctrl-k` and `ctrl-j` move the selected row up and down a slot, and
//! `ctrl-d`, or `d d` in vim's normal mode, unmarks it, each written back
//! at once. Those keys are caught ahead of the keymap while the picker has
//! focus ([`App::intercept_keystrokes`]): `ctrl-k` starts chords on Linux.
//!
//! Reading and writing a list happen off the UI thread, one at a time and
//! in the order asked, on one background queue per app, under
//! [`fe_config::harpoon::update`]'s lock, so windows sharing a root, or
//! processes, all land.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use fe_config::harpoon::{self, Added, MarkList};
use fe_ui::{HighlightedLabel, char_ranges, file_icon_ref};
use gpui::{
    AnyElement, App, AppContext as _, Context, DismissEvent, Entity, EntityId, Focusable as _,
    InteractiveElement as _, IntoElement as _, KeystrokeEvent, ParentElement as _, SharedString,
    Styled as _, Subscription, Task, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
};
use kiln_shell::{ModalId, picker::Mode};

use crate::{
    RootChanged, Workbench,
    pane::TabId,
    picker::{Picker, PickerDelegate},
    toolbar_picker::subsequence,
};

/// The list picker's id in the modal layer.
pub(crate) const HARPOON_LIST: ModalId = ModalId("harpoon");

/// What a rootless window's commands say.
pub(crate) const NO_FOLDER: &str = "Open a folder first";

/// Where the lists are kept, if not under the user's state directory.
struct StateDir(PathBuf);

impl gpui::Global for StateDir {}

/// Keeps the lists under `dir` from now on.
#[cfg(test)]
pub(crate) fn set_state_dir(cx: &mut App, dir: PathBuf) {
    cx.set_global(StateDir(dir));
}

/// The list file of the project at `root`: under the state directory set,
/// or the user's. Under test, none unless one is set, so no test touches
/// the user's.
fn list_file(root: &Path, cx: &App) -> Option<PathBuf> {
    match cx.try_global::<StateDir>() {
        Some(StateDir(dir)) => Some(harpoon::marks_file(dir, root)),
        None if cfg!(test) => None,
        None => harpoon::marks_path(root),
    }
}

/// Work on a list, done off the UI thread.
type Job = Box<dyn FnOnce() + Send>;

/// The queue the lists' reads and writes go through.
struct Queue(async_channel::Sender<Job>);

impl gpui::Global for Queue {}

/// Runs `job` on the queue, after the jobs queued before it; `None` if it
/// couldn't be queued.
fn queue<R: Send + 'static>(
    job: impl FnOnce() -> R + Send + 'static,
    cx: &mut App,
) -> Task<Option<R>> {
    let jobs = match cx.try_global::<Queue>() {
        Some(Queue(jobs)) => jobs.clone(),
        None => {
            let (jobs, queued) = async_channel::unbounded::<Job>();
            cx.background_spawn(async move {
                while let Ok(job) = queued.recv().await {
                    job();
                }
            })
            .detach();
            cx.set_global(Queue(jobs.clone()));
            jobs
        }
    };
    let (sender, done) = async_channel::bounded(1);
    let queued = jobs
        .try_send(Box::new(move || {
            _ = sender.try_send(job());
        }))
        .is_ok();
    cx.background_spawn(async move {
        match queued {
            true => done.recv().await.ok(),
            false => None,
        }
    })
}

/// Reads the list at `file`, logging why if it can't: empty then.
fn load(file: &Path) -> MarkList {
    let (list, error) = MarkList::load(file);
    if let Some(error) = error {
        log::warn!("can't read harpoon list {}: {error}", file.display());
    }
    list
}

/// `path` from `root`, if it's under it: as given, or as both resolve, a
/// root reached through a symlink, say.
fn relative(root: &Path, path: &Path) -> Option<PathBuf> {
    let rel = match path.strip_prefix(root) {
        Ok(rel) => rel.to_owned(),
        Err(_) => {
            let root = std::fs::canonicalize(root).ok()?;
            let path = std::fs::canonicalize(path).ok()?;
            path.strip_prefix(&root).ok()?.to_owned()
        }
    };
    harpoon::is_relative_mark(&rel).then_some(rel)
}

/// `rel` as shown: with `/`s.
fn shown(rel: &Path) -> String {
    let parts: Vec<_> = rel.iter().map(|part| part.to_string_lossy()).collect();
    parts.join("/")
}

fn toast(level: log::Level, message: String, cx: &mut App) {
    Workbench::notify(cx, level, message, None);
}

impl Workbench {
    /// The root and its list file, or `None`, having said why: there's no
    /// root, or nowhere to keep its list.
    fn harpoon_place(&self, cx: &mut App) -> Option<(PathBuf, PathBuf)> {
        let Some(root) = self.root().map(Path::to_owned) else {
            toast(log::Level::Info, NO_FOLDER.to_owned(), cx);
            return None;
        };
        let Some(file) = list_file(&root, cx) else {
            let message = "Harpoon marks can't be kept: no state directory".to_owned();
            toast(log::Level::Warn, message, cx);
            return None;
        };
        Some((root, file))
    }

    /// The active tab's file, from `root`, or `None`, having said why.
    fn harpoon_active(&self, root: &Path, cx: &mut App) -> Option<PathBuf> {
        let pane = self.pane().read(cx);
        let path = (pane.active()).and_then(|ix| pane.tabs().get(ix)?.path.clone());
        let Some(path) = path else {
            toast(log::Level::Info, "No file to mark".to_owned(), cx);
            return None;
        };
        let rel = relative(root, &path);
        if rel.is_none() {
            let message = format!("{} isn't in the open folder", path.display());
            toast(log::Level::Info, message, cx);
        }
        rel
    }

    /// Marks the active tab's file in the next slot.
    pub(crate) fn harpoon_add(&mut self, cx: &mut Context<Self>) {
        let Some((root, file)) = self.harpoon_place(cx) else {
            return;
        };
        let Some(rel) = self.harpoon_active(&root, cx) else {
            return;
        };
        let added = queue(
            move || {
                harpoon::update(&file, |list| {
                    let added = list.add(&rel);
                    (matches!(added, Added::New(_)), (added, rel))
                })
            },
            cx,
        );
        cx.spawn(async move |_, cx| {
            let Some(added) = added.await else {
                return;
            };
            cx.update(|cx| {
                let (level, message) = match added {
                    Ok((_, (Added::New(ix), rel))) => (
                        log::Level::Info,
                        format!("Harpoon: marked {} as {}", shown(&rel), ix + 1),
                    ),
                    Ok((_, (Added::Already(ix), rel))) => (
                        log::Level::Info,
                        format!("Harpoon: {} is already {}", shown(&rel), ix + 1),
                    ),
                    Ok((_, (Added::Full, _))) => (
                        log::Level::Info,
                        format!("Harpoon: the list is full ({} files)", harpoon::MAX_MARKS),
                    ),
                    Ok((_, (Added::Invalid, rel))) => (
                        log::Level::Info,
                        format!("Harpoon: can't mark {}", rel.display()),
                    ),
                    Err(error) => (
                        log::Level::Error,
                        format!("Harpoon: can't save the list: {error}"),
                    ),
                };
                toast(level, message, cx);
            });
        })
        .detach();
    }

    /// Unmarks the active tab's file.
    pub(crate) fn harpoon_remove_current(&mut self, cx: &mut Context<Self>) {
        let Some((root, file)) = self.harpoon_place(cx) else {
            return;
        };
        let Some(rel) = self.harpoon_active(&root, cx) else {
            return;
        };
        let removed = queue(
            move || {
                harpoon::update(&file, |list| {
                    let removed = (list.position(&rel)).and_then(|ix| list.remove(ix));
                    (removed.is_some(), (removed.is_some(), rel))
                })
            },
            cx,
        );
        cx.spawn(async move |_, cx| {
            let Some(removed) = removed.await else {
                return;
            };
            cx.update(|cx| {
                let message = match removed {
                    Ok((_, (true, rel))) => format!("Harpoon: unmarked {}", shown(&rel)),
                    Ok((_, (false, rel))) => format!("Harpoon: {} isn't marked", shown(&rel)),
                    Err(error) => {
                        let message = format!("Harpoon: can't save the list: {error}");
                        return toast(log::Level::Error, message, cx);
                    }
                };
                toast(log::Level::Info, message, cx);
            });
        })
        .detach();
    }

    /// Opens the file in `slot`, 1 to 9, in the active pane, activating its
    /// tab if it's open there. A missing file or an empty slot is said.
    pub(crate) fn harpoon_jump(
        &mut self,
        slot: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((root, file)) = self.harpoon_place(cx) else {
            return;
        };
        self.open_mark(root, Mark::Slot(slot, file), window, cx)
            .detach();
    }

    /// Opens `mark` of the list of `root` in the active pane, as a tree
    /// click does, reading the disk off the UI thread: its tab there is
    /// activated if it has one, under any spelling of the file's path. A
    /// missing file or an empty slot is said. It lands only if it's the
    /// window's latest such open and nothing moved meanwhile: the root, the
    /// active pane and its active tab are as they were. Whether it opened,
    /// or `None` if it didn't land.
    fn open_mark(
        &mut self,
        root: PathBuf,
        mark: Mark,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Option<bool>> {
        let asked = Asked::now(self, root.clone(), cx);
        let pane = self.pane().read(cx);
        let tabs: Vec<PathBuf> = (pane.tabs().iter())
            .filter_map(|tab| tab.path.clone())
            .collect();
        let found = queue(
            move || {
                let rel = match mark {
                    Mark::Slot(slot, file) => match load(&file).get(slot.wrapping_sub(1)) {
                        Some(rel) => rel.to_owned(),
                        None => return Found::Empty(slot),
                    },
                    Mark::File(rel) => rel,
                };
                resolve(&root, rel, &tabs)
            },
            cx,
        );
        cx.spawn_in(window, async move |workbench, cx| {
            let found = found.await?;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    if !asked.holds(workbench, cx) {
                        return None;
                    }
                    Some(match found {
                        Found::Empty(slot) => {
                            let message = format!("Harpoon: nothing marked as {slot}");
                            toast(log::Level::Info, message, cx);
                            false
                        }
                        Found::Missing(rel) => {
                            missing(&rel, cx);
                            false
                        }
                        Found::Open(path) => {
                            let pane = workbench.pane().clone();
                            pane.update(cx, |pane, cx| pane.open(path, window, cx));
                            true
                        }
                    })
                })
                .ok()
                .flatten()
        })
    }

    /// Shows the list picker, focused, in place of any palette; or hides it
    /// if it shows.
    pub(crate) fn harpoon_toggle_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let modals = self.shell().read(cx).modals().clone();
        if modals.read(cx).modal() == Some(HARPOON_LIST) {
            modals.update(cx, |layer, cx| layer.dismiss_modal(window, cx));
            return;
        }
        let Some((root, file)) = self.harpoon_place(cx) else {
            return;
        };
        self.dismiss_palette(window, cx);
        let workbench = cx.entity();
        let picker = cx.new(|cx| {
            let delegate = HarpoonPicker::new(workbench.downgrade(), root, file);
            Picker::new(delegate, "Marked files…", fe_ui::ui_theme(cx), cx)
        });
        picker.update(cx, |picker, cx| {
            let root_changed = cx.subscribe_in(
                &workbench,
                window,
                |picker, _, _: &RootChanged, window, cx| picker.dismiss(window, cx),
            );
            picker.delegate.opened(root_changed, cx);
            picker.show(window, cx);
        });
        modals.update(cx, |layer, cx| {
            layer.show_modal(HARPOON_LIST, picker, window, cx)
        });
    }

    /// The list picker, if it shows.
    #[cfg(test)]
    pub(crate) fn harpoon_list(&self, cx: &App) -> Option<Entity<Picker<HarpoonPicker>>> {
        let layer = self.shell().read(cx).modals().read(cx);
        if layer.modal() != Some(HARPOON_LIST) {
            return None;
        }
        layer.modal_view()?.clone().downcast().ok()
    }
}

/// Which marked file to open.
enum Mark {
    /// The one in this slot, 1 to 9, of the list in this file.
    Slot(usize, PathBuf),
    /// This one, from the root.
    File(PathBuf),
}

/// What opening a mark found on disk.
enum Found {
    /// Nothing is in this slot.
    Empty(usize),
    /// This file, from the root, is gone.
    Missing(PathBuf),
    /// The path to open: an open tab's, if one is the file.
    Open(PathBuf),
}

/// `rel` under `root`, as one of `tabs` spells it if one is the same file
/// (through a symlink, say), so its tab is reused; else as joined.
fn resolve(root: &Path, rel: PathBuf, tabs: &[PathBuf]) -> Found {
    let path = root.join(&rel);
    if !path.exists() {
        return Found::Missing(rel);
    }
    if tabs.contains(&path) {
        return Found::Open(path);
    }
    let canonical = std::fs::canonicalize(&path).ok();
    let open = (tabs.iter())
        .find(|tab| canonical.is_some() && std::fs::canonicalize(tab).ok() == canonical)
        .cloned();
    Found::Open(open.unwrap_or(path))
}

/// Each window's latest open of a mark, by its workbench.
#[derive(Default)]
struct Opens(HashMap<EntityId, u64>);

impl gpui::Global for Opens {}

/// Where an open of a mark was asked: it lands only if that still holds.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Asked {
    /// Its number among its window's opens.
    open: u64,
    root: PathBuf,
    pane: EntityId,
    tab: Option<TabId>,
    /// The pane's open and tab navigation serials: another open or
    /// navigation since supersedes this one.
    serials: (u64, u64),
}

impl Opens {
    /// Supersedes `window`'s open number `open`, if it's still the latest.
    fn cancel(window: EntityId, open: u64, cx: &mut App) {
        if !cx.has_global::<Opens>() {
            return;
        }
        if let Some(latest) = cx.global_mut::<Opens>().0.get_mut(&window)
            && *latest == open
        {
            *latest += 1;
        }
    }
}

impl Asked {
    /// A new open asked of `workbench` now, for `root`'s list.
    fn now(workbench: &Workbench, root: PathBuf, cx: &mut Context<Workbench>) -> Asked {
        let id = cx.entity_id();
        let open = cx.default_global::<Opens>().0.entry(id).or_default();
        *open += 1;
        let open = *open;
        let pane = workbench.pane().read(cx);
        Asked {
            open,
            root,
            pane: workbench.pane().entity_id(),
            tab: pane.active_tab().map(|tab| tab.id()),
            serials: (pane.open_serial(), pane.navigations()),
        }
    }

    /// Whether it's `workbench`'s latest open, and nothing has moved since.
    fn holds(&self, workbench: &Workbench, cx: &Context<Workbench>) -> bool {
        let latest = cx
            .try_global::<Opens>()
            .and_then(|opens| opens.0.get(&cx.entity_id()));
        let pane = workbench.pane();
        latest == Some(&self.open)
            && workbench.root() == Some(self.root.as_path())
            && pane.entity_id() == self.pane
            && pane.read(cx).active_tab().map(|tab| tab.id()) == self.tab
            && (pane.read(cx).open_serial(), pane.read(cx).navigations()) == self.serials
    }
}

/// Says the file `rel` is missing.
fn missing(rel: &Path, cx: &mut App) {
    let message = format!("Harpoon: file missing: {}", shown(rel));
    toast(log::Level::Warn, message, cx);
}

/// A marked file, as the picker lists it.
struct Row {
    rel: PathBuf,
    /// `rel` with `/`s, as matched.
    label: String,
    /// Whether it was gone when the list was read.
    missing: bool,
}

/// `list`'s files as rows, each checked on disk under `root`.
fn rows(root: &Path, list: &MarkList) -> Vec<Row> {
    (list.files().iter())
        .map(|rel| Row {
            label: shown(rel),
            missing: !root.join(rel).exists(),
            rel: rel.clone(),
        })
        .collect()
}

/// A change to the selected row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Up,
    Down,
    Remove,
}

/// The marked files of a root, in slot order: see the module's docs.
pub(crate) struct HarpoonPicker {
    workbench: WeakEntity<Workbench>,
    root: PathBuf,
    /// The root's list file.
    file: PathBuf,
    rows: Vec<Row>,
    /// The rows matching the query, each with its label's matched char
    /// indices.
    matches: Vec<(usize, Vec<usize>)>,
    query: String,
    /// Whether the list is read.
    loaded: bool,
    /// Normal mode's `d` waits for a second.
    pending_d: bool,
    /// The last change asked, and the last whose rows show: an earlier
    /// one's landing late is dropped.
    asked: u64,
    shown: u64,
    /// The window's open number of the row last confirmed, until it lands.
    opening: Option<u64>,
    _load: Option<Task<()>>,
    _keys: Option<Subscription>,
    _root_changed: Option<Subscription>,
    _released: Option<Subscription>,
}

impl HarpoonPicker {
    fn new(workbench: WeakEntity<Workbench>, root: PathBuf, file: PathBuf) -> Self {
        Self {
            workbench,
            root,
            file,
            rows: Vec::new(),
            matches: Vec::new(),
            query: String::new(),
            loaded: false,
            pending_d: false,
            asked: 0,
            shown: 0,
            opening: None,
            _load: None,
            _keys: None,
            _root_changed: None,
            _released: None,
        }
    }

    /// Drops the open of the last row confirmed, if still pending.
    fn cancel_opening(&mut self, cx: &mut App) {
        if let (Some(open), Some(workbench)) = (self.opening.take(), self.workbench.upgrade()) {
            Opens::cancel(workbench.entity_id(), open, cx);
        }
    }

    /// It's opening: reads the list, and takes its keys while focused.
    fn opened(&mut self, root_changed: Subscription, cx: &mut Context<Picker<Self>>) {
        self._root_changed = Some(root_changed);
        // However the picker goes (Escape, toggled off, replaced), an open
        // it still waits on is dropped.
        self._released = Some(cx.on_release(|picker, cx| picker.delegate.cancel_opening(cx)));
        let picker = cx.weak_entity();
        self._keys = Some(cx.intercept_keystrokes(move |event, window, cx| {
            if let Some(picker) = picker.upgrade() {
                key(&picker, event, window, cx);
            }
        }));
        let (root, file) = (self.root.clone(), self.file.clone());
        let read = queue(move || rows(&root, &load(&file)), cx);
        self._load = Some(cx.spawn(async move |picker, cx| {
            let rows = read.await.unwrap_or_default();
            (picker.update(cx, |picker, cx| {
                picker.delegate.loaded = true;
                picker.delegate.show_rows(rows, None);
                picker.matches_updated(true, cx);
            }))
            .ok();
        }));
    }

    /// Shows `rows`, matched against the query: the match index of `keep`,
    /// if it's among them.
    fn show_rows(&mut self, rows: Vec<Row>, keep: Option<&Path>) -> Option<usize> {
        self.rows = rows;
        self.rematch();
        let keep = keep?;
        (self.matches.iter()).position(|&(row, _)| self.rows[row].rel == keep)
    }

    fn rematch(&mut self) {
        let query: Vec<char> = self.query.chars().flat_map(char::to_lowercase).collect();
        self.matches = (self.rows.iter().enumerate())
            .filter_map(|(ix, row)| Some((ix, subsequence(&query, &row.label)?)))
            .collect();
    }

    /// Moves or removes the row at match `ix`, writing the list back, then
    /// shows the list as written, the moved row still selected.
    fn change(&mut self, ix: usize, change: Change, cx: &mut Context<Picker<Self>>) {
        let Some(&(row, _)) = self.matches.get(ix) else {
            return;
        };
        let rel = self.rows[row].rel.clone();
        let (root, file) = (self.root.clone(), self.file.clone());
        self.asked += 1;
        let asked = self.asked;
        let keep = rel.clone();
        let written = queue(
            move || {
                let (list, ()) = harpoon::update(&file, |list| {
                    let Some(from) = list.position(&rel) else {
                        return (false, ());
                    };
                    let changed = match change {
                        Change::Up => from.checked_sub(1).is_some_and(|to| list.move_to(from, to)),
                        Change::Down => list.move_to(from, from + 1),
                        Change::Remove => list.remove(from).is_some(),
                    };
                    (changed, ())
                })?;
                Ok::<_, std::io::Error>(rows(&root, &list))
            },
            cx,
        );
        cx.spawn(async move |picker, cx| {
            let Some(written) = written.await else {
                return;
            };
            (picker.update(cx, |picker, cx| match written {
                Ok(rows) => {
                    if asked < picker.delegate.shown {
                        return;
                    }
                    picker.delegate.shown = asked;
                    let keep = (change != Change::Remove).then_some(keep.as_path());
                    match picker.delegate.show_rows(rows, keep) {
                        Some(ix) => picker.select(ix, cx),
                        None => picker.matches_updated(false, cx),
                    }
                }
                Err(error) => {
                    let message = format!("Harpoon: can't save the list: {error}");
                    toast(log::Level::Error, message, cx);
                }
            }))
            .ok();
        })
        .detach();
    }
}

/// A keystroke while `picker` may have focus: its moves and removal, taken
/// ahead of the keymap.
fn key(
    picker: &Entity<Picker<HarpoonPicker>>,
    event: &KeystrokeEvent,
    window: &mut Window,
    cx: &mut App,
) {
    if !picker.focus_handle(cx).contains_focused(window, cx) {
        return;
    }
    let keystroke = &event.keystroke;
    let m = keystroke.modifiers;
    let ctrl = m.control && !(m.alt || m.shift || m.platform || m.function);
    let plain = !m.modified();
    let normal = kiln_shell::picker::vim_keys(cx) && picker.read(cx).mode() == Mode::Normal;
    let taken = picker.update(cx, |picker, cx| {
        let pending_d = std::mem::take(&mut picker.delegate.pending_d);
        let change = match keystroke.key.as_str() {
            "k" if ctrl => Change::Up,
            "j" if ctrl => Change::Down,
            "d" if ctrl => Change::Remove,
            "d" if plain && normal && pending_d => Change::Remove,
            "d" if plain && normal => {
                picker.delegate.pending_d = true;
                return true;
            }
            _ => return false,
        };
        let selected = picker.selected();
        picker.delegate.change(selected, change, cx);
        true
    });
    if taken {
        cx.stop_propagation();
    }
}

#[cfg(test)]
impl HarpoonPicker {
    /// The matches: each slot, file as shown, and whether it's missing.
    pub(crate) fn labels(&self) -> Vec<(usize, String, bool)> {
        (self.matches.iter())
            .map(|&(row, _)| {
                (
                    row + 1,
                    self.rows[row].label.clone(),
                    self.rows[row].missing,
                )
            })
            .collect()
    }

    pub(crate) fn loaded(&self) -> bool {
        self.loaded
    }
}

impl PickerDelegate for HarpoonPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query;
        self.rematch();
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        let row = &self.rows[self.matches[ix].0];
        row.label.clone().into()
    }

    /// The slot, the file's name, matched chars highlighted, and its
    /// folder, dimmed; all dimmed if it's missing.
    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let (row_ix, indices) = &self.matches[ix];
        let row = &self.rows[*row_ix];
        let name = row.rel.file_name().unwrap_or_default().to_string_lossy();
        let name_start = row.label.chars().count() - name.chars().count();
        let name_indices: Vec<usize> = (indices.iter())
            .filter_map(|&at| at.checked_sub(name_start))
            .collect();
        let label = HighlightedLabel::new(name.to_string(), char_ranges(&name, &name_indices), &ui);
        let folder = (row.rel.parent())
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(|parent| div().text_color(ui.muted_text).child(shown(parent)));
        let icon = file_icon_ref(&name).element(px(14.), ui.icon);
        let missing = row
            .missing
            .then(|| div().text_color(ui.muted_text).child("missing"));
        div()
            .flex()
            .flex_1()
            .items_center()
            .gap_2()
            .when(row.missing, |row| row.opacity(0.55))
            .debug_selector(|| format!("harpoon-row:{}", row.label))
            .child(
                div()
                    .w(px(14.))
                    .flex_none()
                    .text_color(ui.muted_text)
                    .child(format!("{}", row_ix + 1)),
            )
            .child(div().size(px(14.)).flex_none().child(icon))
            .child(label)
            .children(folder)
            .children(missing)
            .into_any_element()
    }

    /// Opens the row's file, as a jump does, then closes; a missing one is
    /// said, its row flagged, and the picker stays. Never taken at once:
    /// the disk is read first, off the UI thread.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let (Some(workbench), Some(&(row, _))) = (self.workbench.upgrade(), self.matches.get(ix))
        else {
            return false;
        };
        let rel = self.rows[row].rel.clone();
        let mark = Mark::File(rel.clone());
        let root = self.root.clone();
        let (opened, open) = workbench.update(cx, |workbench, cx| {
            let opened = workbench.open_mark(root, mark, window, cx);
            let open = cx
                .try_global::<Opens>()
                .and_then(|o| o.0.get(&cx.entity_id()).copied());
            (opened, open)
        });
        self.opening = open;
        cx.spawn_in(window, async move |picker, cx| {
            let Some(opened) = opened.await else {
                return;
            };
            (picker.update(cx, |picker, cx| {
                picker.delegate.opening = None;
                if let Some(row) = (picker.delegate.rows.iter_mut()).find(|row| row.rel == rel) {
                    row.missing = !opened;
                }
                match opened {
                    true => cx.emit(DismissEvent),
                    false => cx.notify(),
                }
            }))
            .ok();
        })
        .detach();
        false
    }

    fn dismissed(&mut self, _: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.cancel_opening(cx);
    }

    fn status(&self) -> Option<SharedString> {
        (self.loaded && self.rows.is_empty())
            .then(|| "No marked files: Harpoon: Add File marks the active one".into())
    }
}

#[cfg(test)]
#[path = "harpoon_tests.rs"]
mod harpoon_tests;
