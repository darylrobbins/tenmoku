//! Open Recent: the folders and files opened lately, kept by
//! [`fe_config::recent`] in `<state_dir>/recent.toml`, and a [`Picker`]
//! over them.
//!
//! The workbench records a folder each time a window takes a root, and a
//! file each time an editor with a path joins it ([`record`]).
//! `workbench.open_recent` ("Open Recent…") shows a [`RecentPicker`]:
//! folders, then files, most recent first, each path shortened by `~`,
//! filtered by the query as a subsequence, leaving out paths that are gone.
//! Confirming a folder re-roots the window as Open Folder does, and a file
//! opens in the active pane, in vim's normal mode if vim is on, whatever
//! mode a tab already open was left in. A folder row's button, or
//! `recent.open_in_new_window` in the picker's context, opens the folder in
//! a new window through the host's opener instead. `workbench.clear_recent`
//! ("Clear Recently Opened") empties the list.
//!
//! Reading and writing the list happen off the UI thread, one at a time
//! and in the order asked, on one background queue per app, so a list
//! read for the picker sees every record made before it opened.
//!
//! The commands are typed: `#[kiln::command]` methods of the workbench
//! (`workbench_commands.rs`), `recent.open_in_new_window` in the picker's
//! context. They have no key yet: the keys card adds them.

use std::path::{Path, PathBuf};

use fe_config::recent::{self, Entry, RecentList};
use fe_gpui::EditorView;
use fe_ui::{
    HighlightedLabel, Icon, IconButton, Tooltip, char_ranges, file_icon_ref, folder_icon_ref,
};
use gpui::{
    AnyElement, App, AppContext as _, Context, DismissEvent, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement as _, Keystroke, ParentElement as _, SharedString,
    Styled as _, Task, WeakEntity, Window, div, px,
};

use crate::{
    Workbench, open,
    picker::{Picker, PickerDelegate},
    toolbar_picker::subsequence,
};

/// Opens the picker's selected folder in a new window.
#[cfg(test)]
pub(crate) const OPEN_IN_NEW_WINDOW: &str = "recent.open_in_new_window";

/// What a recent command asks for: its `#[kiln::command]` method, or vim's
/// `:oldfiles`, runs it with [`Workbench::run_recent_op`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecentOp {
    Open,
    Clear,
    OpenInNewWindow,
}

/// Where the list is kept, if not [`recent::recent_path`].
struct RecentFile(Option<PathBuf>);

impl gpui::Global for RecentFile {}

/// Keeps the list in `path` from now on.
#[cfg(test)]
pub(crate) fn set_recent_file(cx: &mut App, path: PathBuf) {
    cx.set_global(RecentFile(Some(path)));
}

/// The list's file: `<state_dir>/recent.toml`, unless set otherwise. Under
/// test, none unless set, so no test touches the user's.
fn recent_file(cx: &App) -> Option<PathBuf> {
    match cx.try_global::<RecentFile>() {
        Some(RecentFile(path)) => path.clone(),
        None if cfg!(test) => None,
        None => recent::recent_path(),
    }
}

/// Work on the list's file, done off the UI thread.
type Job = Box<dyn FnOnce(&Path) + Send>;

/// The queue the list's reads and writes go through, for its file.
struct Queue {
    file: PathBuf,
    jobs: async_channel::Sender<Job>,
}

impl gpui::Global for Queue {}

/// Counts the changes made to the list through [`record`] and [`clear`]; a
/// view that shows it observes the global and reads it again.
pub(crate) struct Changes(u64);

impl gpui::Global for Changes {}

fn changed(cx: &mut App) {
    let count = cx.try_global::<Changes>().map_or(0, |changes| changes.0);
    cx.set_global(Changes(count + 1));
}

/// Queues `job` on the list's file, after the jobs queued before it. False
/// if there's no file to keep the list in.
fn queue(job: impl FnOnce(&Path) + Send + 'static, cx: &mut App) -> bool {
    let Some(file) = recent_file(cx) else {
        return false;
    };
    let current = cx.try_global::<Queue>().filter(|queue| queue.file == file);
    let jobs = match current {
        Some(queue) => queue.jobs.clone(),
        None => {
            let (jobs, queued) = async_channel::unbounded::<Job>();
            let at = file.clone();
            cx.background_spawn(async move {
                while let Ok(job) = queued.recv().await {
                    job(&at);
                }
            })
            .detach();
            cx.set_global(Queue {
                file,
                jobs: jobs.clone(),
            });
            jobs
        }
    };
    jobs.try_send(Box::new(job)).is_ok()
}

/// Records `path`, a folder or a file, at the front of the list.
pub(crate) fn record(path: PathBuf, folder: bool, cx: &mut App) {
    queue(
        move |file| {
            let entry = match folder {
                true => Entry::Folder(&path),
                false => Entry::File(&path),
            };
            if let Err(error) = recent::record(file, entry) {
                log::warn!(
                    "can't record {} in {}: {error}",
                    path.display(),
                    file.display()
                );
            }
        },
        cx,
    );
    changed(cx);
}

/// Empties the list.
pub(crate) fn clear(cx: &mut App) {
    queue(
        |file| {
            if let Err(error) = recent::clear(file) {
                log::warn!("can't clear {}: {error}", file.display());
            }
        },
        cx,
    );
    changed(cx);
}

/// The list's entries that still exist, read after the jobs queued before:
/// the picker's and the Start page's ([`crate::start_page`]). Empty if
/// there's no file, or it can't be read.
pub(crate) fn load(cx: &mut App) -> Task<RecentList> {
    let (sender, loaded) = async_channel::bounded(1);
    let queued = queue(
        move |file| {
            let (list, error) = RecentList::load(file);
            if let Some(error) = error {
                log::warn!("can't read {}: {error}", file.display());
            }
            _ = sender.try_send(list.existing());
        },
        cx,
    );
    cx.background_spawn(async move {
        match queued {
            true => loaded.recv().await.unwrap_or_default(),
            false => RecentList::default(),
        }
    })
}

/// `path` as shown: under the home directory, from `~`.
fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => path.display().to_string(),
    }
}

/// A recent folder or file.
struct Row {
    path: PathBuf,
    folder: bool,
    /// The path as shown, and matched.
    label: String,
}

pub(crate) struct RecentPicker {
    /// The folders, then the files, most recent first.
    rows: Vec<Row>,
    /// The rows matching the query, each with its label's matched ranges.
    matches: Vec<(usize, Vec<std::ops::Range<usize>>)>,
    query: String,
    /// The window it shows in.
    workbench: WeakEntity<Workbench>,
    /// What was focused before it opened, focused again as it's dismissed.
    previous_focus: Option<FocusHandle>,
    /// The picker it's the delegate of, for its rows' buttons.
    picker: Option<WeakEntity<Picker<Self>>>,
    /// Reading the list for this opening.
    _load: Option<Task<()>>,
    /// Whether this opening's list is read.
    loaded: bool,
}

impl RecentPicker {
    pub(crate) fn new(workbench: WeakEntity<Workbench>) -> Self {
        Self {
            rows: Vec::new(),
            matches: Vec::new(),
            query: String::new(),
            workbench,
            previous_focus: None,
            picker: None,
            _load: None,
            loaded: false,
        }
    }

    /// The picker is opening, with `previous_focus` focused until now, or
    /// opening afresh, keeping what was focused before, if that's `None`:
    /// its rows are read again, in the background.
    pub(crate) fn opened(
        &mut self,
        previous_focus: Option<FocusHandle>,
        cx: &mut Context<Picker<Self>>,
    ) {
        self.previous_focus = previous_focus.or(self.previous_focus.take());
        self.picker = Some(cx.weak_entity());
        self.rows.clear();
        self.matches.clear();
        self.loaded = false;
        let list = load(cx);
        self._load = Some(cx.spawn(async move |picker, cx| {
            let list = list.await;
            picker
                .update(cx, |picker, cx| {
                    picker.delegate.set_list(list);
                    picker.delegate.loaded = true;
                    picker.delegate.refilter(cx);
                })
                .ok();
        }));
    }

    /// Shows `list`'s entries as the rows.
    fn set_list(&mut self, list: RecentList) {
        let home = std::env::home_dir();
        let row = |path: PathBuf, folder: bool| Row {
            label: tilde(&path, home.as_deref()),
            path,
            folder,
        };
        let folders = list.folders.into_iter().map(|path| row(path, true));
        let files = list.files.into_iter().map(|path| row(path, false));
        self.rows = folders.chain(files).collect();
    }

    /// Empties the rows, the list having been cleared: a read still under
    /// way is dropped, as it may have read the list before the clear.
    pub(crate) fn cleared(&mut self, cx: &mut Context<Picker<Self>>) {
        self._load = None;
        self.loaded = true;
        self.rows.clear();
        self.refilter(cx);
    }

    /// Matches the rows against the query, selecting the first.
    fn refilter(&mut self, cx: &mut Context<Picker<Self>>) {
        let query: Vec<char> = self.query.chars().flat_map(char::to_lowercase).collect();
        self.matches = (self.rows.iter().enumerate())
            .filter_map(|(ix, row)| {
                let indices = subsequence(&query, &row.label)?;
                Some((ix, char_ranges(&row.label, &indices)))
            })
            .collect();
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn row(&self, ix: usize) -> &Row {
        &self.rows[self.matches[ix].0]
    }

    /// Opens match `ix` in a new window if it's a folder, through the
    /// host's opener, this window keeping its root, and dismisses the
    /// picker; else confirms it, a file having no window of its own to
    /// open in.
    pub(crate) fn open_in_new_window(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) {
        let Some(&(row, _)) = self.matches.get(ix) else {
            return;
        };
        let row = &self.rows[row];
        if row.folder {
            open::open_window(Some(row.path.clone()), cx);
            self.restore_focus(window, cx);
            cx.emit(DismissEvent);
        } else if self.confirm(ix, window, cx) {
            cx.emit(DismissEvent);
        }
    }

    /// Focuses what was focused before it opened.
    fn restore_focus(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }
}

#[cfg(test)]
impl RecentPicker {
    /// The matches' labels, each with whether it's a folder.
    pub(crate) fn labels(&self) -> Vec<(String, bool)> {
        (0..self.matches.len())
            .map(|ix| (self.row(ix).label.clone(), self.row(ix).folder))
            .collect()
    }

    /// Whether this opening's rows are read.
    pub(crate) fn loaded(&self) -> bool {
        self.loaded
    }
}

impl PickerDelegate for RecentPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query;
        self.refilter(cx);
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// The row's kind and path.
    fn match_id(&self, ix: usize) -> SharedString {
        let row = self.row(ix);
        let kind = if row.folder { "folder" } else { "file" };
        format!("{kind}:{}", row.path.display()).into()
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let row = self.row(ix);
        let name = row.path.file_name().unwrap_or_default().to_string_lossy();
        let icon = match row.folder {
            true => folder_icon_ref(false),
            false => file_icon_ref(&name),
        };
        let path = icon.path();
        let icon = div()
            .size(px(14.))
            .flex_none()
            .debug_selector(|| format!("recent-icon:{name}:{path}"))
            .child(icon.element(px(14.), ui.icon));
        let highlights = self.matches[ix].1.clone();
        let label = HighlightedLabel::new(row.label.clone(), highlights, &ui);
        let new_window = (row.folder).then(|| {
            let picker = self.picker.clone();
            let id = SharedString::from(format!("recent-new-window-{ix}"));
            let button = IconButton::new(id, Icon::Copy, &ui)
                .tooltip(move |_, cx| {
                    let tooltip = Tooltip::new(
                        "Open in New Window",
                        None::<SharedString>,
                        &fe_ui::ui_theme(cx),
                    );
                    cx.new(|_| tooltip).into()
                })
                .on_click(move |_, window, cx| {
                    // Not the row's click, which would re-root this window.
                    cx.stop_propagation();
                    let Some(picker) = picker.as_ref().and_then(WeakEntity::upgrade) else {
                        return;
                    };
                    picker.update(cx, |picker, cx| {
                        picker.delegate.open_in_new_window(ix, window, cx)
                    });
                });
            div().ml_auto().child(button)
        });
        div()
            .flex()
            .flex_1()
            .items_center()
            .gap_2()
            .child(icon)
            .child(label)
            .children(new_window)
            .into_any_element()
    }

    /// A folder re-roots the window, focus going back where it was first; a
    /// file opens in the active pane, as a tree click does, in vim's normal
    /// mode if it's on.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(workbench) = self.workbench.upgrade() else {
            return false;
        };
        let row = self.row(ix);
        let (path, folder) = (row.path.clone(), row.folder);
        if folder {
            self.restore_focus(window, cx);
            workbench.update(cx, |workbench, cx| workbench.set_root(path, window, cx));
        } else {
            self.previous_focus = None;
            let pane = workbench.read(cx).active.clone();
            let open = pane.read(cx).position(&path);
            pane.update(cx, |pane, cx| pane.open(path, window, cx));
            // A new tab starts in normal mode; one already open keeps the
            // mode it was left in.
            let editor = open.and_then(|ix| pane.read(cx).tabs().get(ix)?.editor().cloned());
            if let Some(editor) = editor {
                normal_mode(editor, window, cx);
            }
        }
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.restore_focus(window, cx);
    }

    fn status(&self) -> Option<SharedString> {
        (self.loaded && self.rows.is_empty()).then(|| "No recently opened folders or files".into())
    }
}

/// With vim on, takes `editor` back to normal mode if it was left in
/// another, focusing it.
fn normal_mode(editor: Entity<EditorView>, window: &mut Window, cx: &mut App) {
    let normal = |editor: &Entity<EditorView>, cx: &App| {
        (editor.read(cx).vim_mode()).is_none_or(|mode| mode == "normal")
    };
    if normal(&editor, cx) {
        return;
    }
    // Outside the picker's update: the key runs the editor's own.
    window.defer(cx, move |window, cx| {
        window.focus(&editor.focus_handle(cx), cx);
        // One escape leaves insert, a visual mode or a pending command; a
        // second, should the first only drop a count.
        for _ in 0..2 {
            if normal(&editor, cx) {
                break;
            }
            window.dispatch_keystroke(Keystroke::parse("escape").unwrap(), cx);
        }
    });
}

#[cfg(test)]
#[path = "recent_picker_tests.rs"]
mod recent_picker_tests;
