//! The Changes panel on the right dock: the project's files that differ
//! from git's HEAD, each with a one-letter status, its name and its
//! directory, sorted by path. Clicking a row, or Enter on the selected one,
//! opens the file in the active pane; a deleted file's row is dimmed and
//! opens nothing.
//!
//! The workbench's [`GitState`] reads the status of the repository
//! containing the project root, on the background executor, one run at a
//! time: when the project tree opens, when the panel's first drawn unless a
//! run has started, on `git.refresh`, and as files and `.git` change. Each
//! run, and each list given by [`GitPanel::set_statuses`], has a sequence
//! number from the panel, and a run's result shows only if nothing newer
//! has. Only files under the
//! project root are listed: the repository may be larger. Where the root is
//! in the repository is found through symlinks and `..`, so a list given
//! before that's known is kept and listed again once it is.
//!
//! The list is also the project tree's: whenever it changes the panel emits
//! [`GitPanelEvent::Listed`], and the workbench hands
//! [`GitPanel::statuses`] to the root's project panel.

use std::{
    ops::Range,
    path::{Path, PathBuf},
};

use fe_git::{ChangeKind, FileStatus, GitError};
use fe_ui::{ROW_HEIGHT, UiTheme};
use gpui::{
    AnyElement, App, Context, ElementId, EventEmitter, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, ScrollStrategy,
    SharedString, StatefulInteractiveElement as _, Styled as _, UniformListScrollHandle,
    WeakEntity, Window, div, prelude::FluentBuilder as _, px, uniform_list,
};

use crate::git::GitState;

/// The key context the panel's element sets.
pub(crate) const KEY_CONTEXT: &str = "git_panel";

/// The hue of added and untracked files' letters: the accent's, turned
/// green.
const ADDED_HUE: f32 = 130. / 360.;

/// What the panel asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitPanelEvent {
    /// A row was clicked, or Enter pressed on it: open this file in the
    /// active pane.
    OpenFile(PathBuf),
    /// The files listed, or their statuses, changed: see
    /// [`GitPanel::statuses`].
    Listed,
}

/// Where the project root is in its repository, once found.
#[derive(Clone, Debug)]
pub(crate) struct Located {
    workdir: PathBuf,
    /// `workdir` through symlinks and `..`.
    resolved: PathBuf,
    /// The root, relative to `workdir`.
    prefix: PathBuf,
}

/// What the panel says in place of the list, or the list.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Status {
    /// Nothing has landed yet.
    Reading,
    NotARepository,
    /// The error's message.
    Failed(String),
    Listed,
}

/// One changed file under the root.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    /// What a click opens: the root joined with `relative`.
    path: PathBuf,
    /// From the root, which the list is sorted by.
    relative: PathBuf,
    kind: ChangeKind,
}

/// What a run found: where the root is in its repository and the
/// repository's status, or no repository.
pub(crate) type Found = Result<Option<(Located, Vec<FileStatus>)>, GitError>;

pub struct GitPanel {
    root: Option<PathBuf>,
    /// What reads the status.
    git: WeakEntity<GitState>,
    /// Where the root is in its repository, once found.
    located: Option<Located>,
    status: Status,
    /// The statuses `entries` were listed from, to list again once the
    /// root is located.
    listed: Vec<FileStatus>,
    entries: Vec<Entry>,
    selected: Option<usize>,
    /// Whether a run has started or been asked for: the first is when the
    /// project tree opens, or when the panel's first drawn if that's
    /// sooner or files changed before.
    read: bool,
    /// The last run started's, or list given's, sequence number.
    started: u64,
    /// The sequence number of the list shown.
    shown: u64,
    /// The sequence number of the run started and not landed, if any.
    running: Option<u64>,
    scroll: UniformListScrollHandle,
    ui: UiTheme,
    focus: FocusHandle,
    #[cfg(test)]
    test: std::rc::Rc<std::cell::RefCell<TestHooks>>,
}

impl GitPanel {
    /// A panel for the repository containing `root`, if any. Nothing runs
    /// until it's drawn or refreshed, and nothing is read until the
    /// workbench connects its git state.
    pub fn new(root: Option<PathBuf>, ui: UiTheme, cx: &mut App) -> Self {
        let status = match root {
            Some(_) => Status::Reading,
            None => Status::NotARepository,
        };
        Self {
            root,
            git: WeakEntity::new_invalid(),
            located: None,
            status,
            listed: Vec::new(),
            entries: Vec::new(),
            selected: None,
            read: false,
            started: 0,
            shown: 0,
            running: None,
            scroll: UniformListScrollHandle::new(),
            ui,
            focus: cx.focus_handle(),
            #[cfg(test)]
            test: Default::default(),
        }
    }

    /// Reads through `git` from now on.
    pub(crate) fn connect(&mut self, git: WeakEntity<GitState>) {
        self.git = git;
    }

    /// Reads the status again, in the background: now if none is running,
    /// else once more after it lands. With no folder open there's nothing
    /// to read.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.root.is_none() {
            return;
        }
        self.read = true;
        if let Some(git) = self.git.upgrade() {
            git.update(cx, GitState::refresh);
        }
    }

    /// A run starts: its sequence number.
    pub(crate) fn begin(&mut self) -> u64 {
        self.read = true;
        self.started += 1;
        self.running = Some(self.started);
        self.started
    }

    /// Shows run `seq`'s result unless something newer has shown. Where
    /// it found the root is kept even so, and the newer list listed again
    /// where the root is in it.
    pub(crate) fn landed(&mut self, seq: u64, found: Found, cx: &mut Context<Self>) {
        self.running.take_if(|running| *running == seq);
        if seq <= self.shown {
            if let Ok(Some((located, _))) = found
                && self.located.is_none()
            {
                self.located = Some(located);
                if self.status == Status::Listed {
                    let listed = std::mem::take(&mut self.listed);
                    self.show(self.shown, listed, cx);
                    cx.notify();
                }
            }
            return;
        }
        match found {
            Ok(Some((located, statuses))) => {
                self.located = Some(located);
                self.show(seq, statuses, cx);
            }
            Ok(None) => {
                self.located = None;
                self.shown = seq;
                self.status = Status::NotARepository;
                self.clear(cx);
            }
            Err(error) => {
                self.shown = seq;
                self.status = Status::Failed(error.to_string());
                self.clear(cx);
            }
        }
        cx.notify();
    }

    /// Lists nothing, saying so if something was listed.
    fn clear(&mut self, cx: &mut Context<Self>) {
        self.listed.clear();
        self.selected = None;
        if !self.entries.is_empty() {
            self.entries.clear();
            cx.emit(GitPanelEvent::Listed);
        }
    }

    /// Replaces the list with `statuses`, the repository's, as of now: a
    /// run in flight lands unshown, though the repository it finds is kept.
    /// Before the root is located, those of `statuses` under it as spelled
    /// are listed; the rest are once it is.
    pub fn set_statuses(&mut self, statuses: Vec<FileStatus>, cx: &mut Context<Self>) {
        self.started += 1;
        self.show(self.started, statuses, cx);
        cx.notify();
    }

    /// Each listed file's path from the root, and its status, sorted by
    /// path.
    pub(crate) fn statuses(&self) -> impl Iterator<Item = (&Path, ChangeKind)> {
        (self.entries.iter()).map(|entry| (entry.relative.as_path(), entry.kind))
    }

    /// Each listed file's path as opened, and its status.
    pub(crate) fn changed_paths(&self) -> impl Iterator<Item = (&Path, ChangeKind)> {
        (self.entries.iter()).map(|entry| (entry.path.as_path(), entry.kind))
    }

    /// Lists those of `statuses` under the root, sorted by path from it,
    /// keeping the selected file selected if it's still listed, and says
    /// so if that changed what's listed.
    fn show(&mut self, seq: u64, statuses: Vec<FileStatus>, cx: &mut Context<Self>) {
        let selected = self.selected_entry().map(|entry| entry.path.clone());
        self.shown = seq;
        let Some(root) = &self.root else {
            return;
        };
        self.status = Status::Listed;
        let located = self.located.as_ref();
        let mut entries: Vec<Entry> = statuses
            .iter()
            .filter_map(|status| {
                let relative = relative_to(root, located, &status.path)?;
                Some(Entry {
                    path: root.join(&relative),
                    relative,
                    kind: status.kind,
                })
            })
            .collect();
        entries.sort_by(|a, b| a.relative.cmp(&b.relative));
        self.selected = selected.and_then(|path| entries.iter().position(|e| e.path == path));
        let changed = entries != self.entries;
        self.entries = entries;
        self.listed = statuses;
        if changed {
            cx.emit(GitPanelEvent::Listed);
        }
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected?)
    }

    /// What shows in place of the list, if anything.
    fn message(&self) -> Option<String> {
        Some(match &self.status {
            Status::Reading => return None,
            Status::NotARepository => "Not a git repository".into(),
            Status::Failed(message) => message.clone(),
            Status::Listed if self.entries.is_empty() => "No changes".into(),
            Status::Listed => return None,
        })
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Selects row `ix` and opens its file, unless it's deleted.
    fn open(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.get(ix) else {
            return;
        };
        let open = (entry.kind != ChangeKind::Deleted).then(|| entry.path.clone());
        self.select(ix, cx);
        if let Some(path) = open {
            cx.emit(GitPanelEvent::OpenFile(path));
        }
    }

    /// Up and down move the selection, the first press selecting the first
    /// row; Enter opens the selected row.
    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() || self.entries.is_empty() {
            return;
        }
        let last = self.entries.len() - 1;
        match keystroke.key.as_str() {
            "up" => {
                let ix = self.selected.map_or(0, |ix| ix.saturating_sub(1));
                self.select(ix, cx);
            }
            "down" => {
                let ix = self.selected.map_or(0, |ix| (ix + 1).min(last));
                self.select(ix, cx);
            }
            "enter" => match self.selected {
                Some(ix) => self.open(ix, cx),
                None => return,
            },
            _ => return,
        }
        cx.stop_propagation();
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let ui = self.ui;
        let selected_bg = if self.focus.contains_focused(window, cx) {
            ui.row_selected
        } else {
            ui.row_selected_inactive
        };
        // Each list shown has its own IDs, so a row pressed before the list
        // changed isn't clicked as the row that replaced it.
        let name = SharedString::from(format!("change-{}", self.shown));
        range
            .filter_map(|ix| {
                let entry = self.entries.get(ix)?;
                let name_text = entry
                    .relative
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let directory = entry
                    .relative
                    .parent()
                    .map(|dir| dir.display().to_string())
                    .unwrap_or_default();
                let deleted = entry.kind == ChangeKind::Deleted;
                Some(
                    div()
                        .id(ElementId::NamedInteger(name.clone(), ix as u64))
                        .h(ROW_HEIGHT)
                        .w_full()
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .when(deleted, |row| row.opacity(0.5))
                        .when(self.selected == Some(ix), |row| row.bg(selected_bg))
                        .hover(move |row| row.bg(ui.row_hover))
                        .on_click(cx.listener(move |panel, _, _, cx| panel.open(ix, cx)))
                        .child(
                            div()
                                .flex_none()
                                .w(px(10.))
                                .text_color(letter_color(entry.kind, &ui))
                                .child(letter(entry.kind)),
                        )
                        .child(div().flex_none().child(name_text))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_color(ui.muted_text)
                                .child(directory),
                        )
                        .into_any_element(),
                )
            })
            .collect()
    }
}

/// Where `root` is in the working tree at `workdir`, through symlinks and
/// `..`: `None` if it isn't in it. Spelled as given if either is gone.
pub(crate) fn locate(root: &Path, workdir: &Path) -> Option<Located> {
    let (prefix, resolved) = match (root.canonicalize(), workdir.canonicalize()) {
        (Ok(root), Ok(resolved)) => (root.strip_prefix(&resolved).ok()?.to_owned(), resolved),
        _ => (
            root.strip_prefix(workdir).ok()?.to_owned(),
            workdir.to_owned(),
        ),
    };
    Some(Located {
        workdir: workdir.to_owned(),
        resolved,
        prefix,
    })
}

/// `path`, absolute, relative to `root`, if it's under it: directly, or
/// through the working tree `located` names, as found or resolved.
fn relative_to(root: &Path, located: Option<&Located>, path: &Path) -> Option<PathBuf> {
    if let Ok(relative) = path.strip_prefix(root) {
        return (!relative.as_os_str().is_empty()).then(|| relative.to_owned());
    }
    let located = located?;
    let relative = path
        .strip_prefix(&located.workdir)
        .or_else(|_| path.strip_prefix(&located.resolved))
        .ok()?
        .strip_prefix(&located.prefix)
        .ok()?;
    (!relative.as_os_str().is_empty()).then(|| relative.to_owned())
}

/// A status's letter in its row.
pub(crate) fn letter(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Modified => "M",
        ChangeKind::Added => "A",
        ChangeKind::Deleted => "D",
        ChangeKind::Untracked => "U",
        ChangeKind::Conflicted => "C",
    }
}

/// A status letter's colour: deletions and conflicts in the error colour,
/// modifications in the accent, additions in the accent turned green.
pub(crate) fn letter_color(kind: ChangeKind, ui: &UiTheme) -> Hsla {
    match kind {
        ChangeKind::Deleted | ChangeKind::Conflicted => ui.error_text,
        ChangeKind::Modified => ui.accent,
        ChangeKind::Added | ChangeKind::Untracked => Hsla {
            h: ADDED_HUE,
            ..ui.accent
        },
    }
}

impl EventEmitter<GitPanelEvent> for GitPanel {}

impl Focusable for GitPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for GitPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Read when first shown, unless a run has started.
        if !self.read {
            self.refresh(cx);
        }
        let ui = self.ui;
        let list = uniform_list(
            "change-rows",
            self.entries.len(),
            cx.processor(|panel, range, window, cx| panel.render_rows(range, window, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1();
        let error = matches!(self.status, Status::Failed(_));
        div()
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            .on_key_down(cx.listener(Self::on_key_down))
            .debug_selector(|| "sidebar-panel".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(ui.border)
            .bg(ui.panel_background)
            .text_color(ui.text)
            .text_size(px(12.))
            .when_some(self.message(), |panel, message| {
                let color = if error { ui.error_text } else { ui.muted_text };
                panel.child(
                    div()
                        .flex_none()
                        .px_3()
                        .py_2()
                        .text_color(color)
                        .child(message),
                )
            })
            .child(list)
    }
}

/// What the tests hold and count of [`GitState`]'s status runs, which the
/// Changes panel's tests see too.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestHooks {
    /// Runs started.
    started: usize,
    /// Runs that read the status, and of them those that read it off the
    /// main thread.
    ran: std::sync::Arc<std::sync::Mutex<(usize, usize)>>,
    /// While set, runs that have read the status wait for
    /// [`GitPanel::release_runs`] before they land.
    held: Option<Vec<futures::channel::oneshot::Sender<()>>>,
}

#[cfg(test)]
impl TestHooks {
    /// Counts a run started, and returns what it calls once it has read the
    /// status, and the gate it then awaits.
    pub(crate) fn started(
        &mut self,
        cx: &App,
    ) -> (
        impl FnOnce() + Send + 'static,
        impl Future<Output = ()> + Send + 'static,
    ) {
        self.started += 1;
        let (release, gate) = futures::channel::oneshot::channel();
        match &mut self.held {
            Some(held) => held.push(release),
            None => drop(release.send(())),
        }
        let gate = async move {
            gate.await.ok();
        };
        let ran = self.ran.clone();
        let executor = cx.background_executor().clone();
        let ran = move || {
            let mut ran = ran.lock().unwrap();
            ran.0 += 1;
            ran.1 += usize::from(!executor.is_main_thread());
        };
        (ran, gate)
    }

    /// Holds the runs started from now until [`Self::release`], once
    /// they've read the status.
    pub(crate) fn hold(&mut self) {
        self.held.get_or_insert_default();
    }

    /// Lets the held runs, and the ones started from now, land.
    pub(crate) fn release(&mut self) {
        for release in self.held.take().unwrap_or_default() {
            release.send(()).ok();
        }
    }

    /// Runs started, and runs that read the status and of them those off
    /// the main thread.
    pub(crate) fn runs(&self) -> (usize, (usize, usize)) {
        (self.started, *self.ran.lock().unwrap())
    }
}

/// A row as shown: its letter, name and directory, and whether it's dimmed.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShownRow {
    pub letter: &'static str,
    pub name: String,
    pub directory: String,
    pub dimmed: bool,
}

#[cfg(test)]
impl GitPanel {
    /// Holds the runs started from now until [`Self::release_runs`], once
    /// they've read the status.
    pub(crate) fn hold_runs(&mut self) {
        self.test.borrow_mut().hold();
    }

    /// Lets the held runs, and the ones started from now, land.
    pub(crate) fn release_runs(&mut self) {
        self.test.borrow_mut().release();
    }

    /// Runs started.
    pub(crate) fn runs_started(&self) -> usize {
        self.test.borrow().runs().0
    }

    /// Runs that read the status, and of them those off the main thread.
    pub(crate) fn runs_ran(&self) -> (usize, usize) {
        self.test.borrow().runs().1
    }

    pub(crate) fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// What the tests hold and count, shared with the git state.
    pub(crate) fn test_hooks(&self) -> std::rc::Rc<std::cell::RefCell<TestHooks>> {
        self.test.clone()
    }

    pub(crate) fn shown_rows(&self) -> Vec<ShownRow> {
        self.entries
            .iter()
            .map(|entry| ShownRow {
                letter: letter(entry.kind),
                name: entry
                    .relative
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                directory: entry
                    .relative
                    .parent()
                    .map(|dir| dir.display().to_string())
                    .unwrap_or_default(),
                dimmed: entry.kind == ChangeKind::Deleted,
            })
            .collect()
    }

    /// The paths a row would open, in row order.
    pub(crate) fn paths(&self) -> Vec<PathBuf> {
        self.entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect()
    }

    /// What shows in place of the rows, if anything.
    pub(crate) fn message_text(&self) -> Option<String> {
        self.message()
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Where row `ix` was laid out in the last frame, from the list's bounds,
    /// scroll offset and measured row height.
    pub(crate) fn row_bounds(&self, ix: usize) -> Option<gpui::Bounds<gpui::Pixels>> {
        let count = self.entries.len();
        let state = self.scroll.0.borrow();
        let list = state.base_handle.bounds();
        let height = state.last_item_size?.contents.height / count as f32;
        let top = list.origin.y + state.base_handle.offset().y + height * ix as f32;
        Some(gpui::Bounds::new(
            gpui::point(list.origin.x, top),
            gpui::size(list.size.width, height),
        ))
    }
}

#[cfg(test)]
#[path = "git_panel_tests.rs"]
mod tests;
