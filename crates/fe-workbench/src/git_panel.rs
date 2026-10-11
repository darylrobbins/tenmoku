//! The Changes panel on the right dock: the project's files that differ
//! from git's HEAD, each with a one-letter status, its name and its
//! directory, sorted by path. Clicking a row, or Enter on the selected one,
//! opens the file in the active pane; a deleted file's row is dimmed and
//! opens nothing.
//!
//! The panel finds the repository containing the project root and reads
//! its status with fe-git on the background executor, the first time it's
//! drawn and on `git.refresh`. At most one status runs at a time: a refresh
//! meanwhile runs once more after it lands. Each run, and each list given
//! by [`GitPanel::set_statuses`], has a sequence number, and a run's result
//! shows only if nothing newer has. Only files under the project root are
//! listed: the repository may be larger.

use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_git::{ChangeKind, FileStatus, GitError, Repo};
use fe_ui::{ROW_HEIGHT, UiTheme};
use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, EventEmitter, FocusHandle, Focusable,
    Hsla, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render,
    ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _, Task,
    UniformListScrollHandle, Window, div, prelude::FluentBuilder as _, px, uniform_list,
};

use crate::PANEL_WIDTH;

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
}

/// Where the project root is in its repository, once found.
#[derive(Clone, Debug)]
struct Located {
    workdir: PathBuf,
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

/// What a background run found: where the root is in its repository and
/// the repository's status, or no repository.
type Found = Result<Option<(Arc<Repo>, Located, Vec<FileStatus>)>, GitError>;

pub struct GitPanel {
    root: Option<PathBuf>,
    /// The repository, once found, for later runs.
    repo: Option<(Arc<Repo>, Located)>,
    status: Status,
    entries: Vec<Entry>,
    selected: Option<usize>,
    /// The last run started's, or list given's, sequence number.
    started: u64,
    /// The sequence number of the list shown.
    shown: u64,
    /// The run in flight.
    running: Option<Task<()>>,
    /// Whether a refresh came while one ran.
    again: bool,
    scroll: UniformListScrollHandle,
    ui: UiTheme,
    focus: FocusHandle,
    #[cfg(test)]
    test: TestHooks,
}

impl GitPanel {
    /// A panel for the repository containing `root`, if any. Nothing runs
    /// until it's drawn or refreshed.
    pub fn new(root: Option<PathBuf>, ui: UiTheme, cx: &mut App) -> Self {
        let status = match root {
            Some(_) => Status::Reading,
            None => Status::NotARepository,
        };
        Self {
            root,
            repo: None,
            status,
            entries: Vec::new(),
            selected: None,
            started: 0,
            shown: 0,
            running: None,
            again: false,
            scroll: UniformListScrollHandle::new(),
            ui,
            focus: cx.focus_handle(),
            #[cfg(test)]
            test: TestHooks::default(),
        }
    }

    /// Reads the status again, in the background: now if none is running,
    /// else once more after it lands. With no folder open there's nothing
    /// to read.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.running.is_some() {
            self.again = true;
            return;
        }
        self.started += 1;
        let seq = self.started;
        let repo = self.repo.clone();
        #[cfg(test)]
        let (ran, gate) = self.test.started(cx);
        let found = cx.background_spawn(async move {
            let found = read(&root, repo);
            #[cfg(test)]
            {
                ran();
                gate.await;
            }
            found
        });
        self.running = Some(cx.spawn(async move |panel, cx| {
            let found = found.await;
            panel
                .update(cx, |panel, cx| panel.landed(seq, found, cx))
                .ok();
        }));
    }

    /// Shows a run's result unless something newer has shown, then runs
    /// again if a refresh came meanwhile.
    fn landed(&mut self, seq: u64, found: Found, cx: &mut Context<Self>) {
        self.running = None;
        if seq > self.shown {
            match found {
                Ok(Some((repo, located, statuses))) => {
                    self.repo = Some((repo, located));
                    self.show(seq, statuses);
                }
                Ok(None) => {
                    self.repo = None;
                    self.shown = seq;
                    self.status = Status::NotARepository;
                    self.entries.clear();
                    self.selected = None;
                }
                Err(error) => {
                    self.shown = seq;
                    self.status = Status::Failed(error.to_string());
                    self.entries.clear();
                    self.selected = None;
                }
            }
            cx.notify();
        }
        if std::mem::take(&mut self.again) {
            self.refresh(cx);
        }
    }

    /// Replaces the list with `statuses`, the repository's, as of now: a
    /// run in flight lands unshown.
    pub fn set_statuses(&mut self, statuses: Vec<FileStatus>, cx: &mut Context<Self>) {
        self.started += 1;
        self.show(self.started, statuses);
        cx.notify();
    }

    /// Lists those of `statuses` under the root, sorted by path from it,
    /// keeping the selected file selected if it's still listed.
    fn show(&mut self, seq: u64, statuses: Vec<FileStatus>) {
        let selected = self.selected_entry().map(|entry| entry.path.clone());
        self.shown = seq;
        let Some(root) = &self.root else {
            return;
        };
        self.status = Status::Listed;
        let located = self.repo.as_ref().map(|(_, located)| located);
        let mut entries: Vec<Entry> = statuses
            .into_iter()
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
        self.entries = entries;
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

/// Finds the repository containing `root`, unless `repo` is it already,
/// and reads its status. Runs on the background executor.
fn read(root: &Path, repo: Option<(Arc<Repo>, Located)>) -> Found {
    let (repo, located) = match repo {
        Some(found) => found,
        None => {
            let Some(repo) = Repo::discover(root)? else {
                return Ok(None);
            };
            let Some(located) = locate(root, repo.workdir()) else {
                return Ok(None);
            };
            (Arc::new(repo), located)
        }
    };
    let statuses = repo.status()?;
    Ok(Some((repo, located, statuses)))
}

/// Where `root` is in the working tree at `workdir`, through symlinks if
/// need be: `None` if it isn't in it.
fn locate(root: &Path, workdir: &Path) -> Option<Located> {
    let prefix = match root.strip_prefix(workdir) {
        Ok(prefix) => prefix.to_owned(),
        Err(_) => {
            let root = root.canonicalize().ok()?;
            let workdir = workdir.canonicalize().ok()?;
            root.strip_prefix(workdir).ok()?.to_owned()
        }
    };
    Some(Located {
        workdir: workdir.to_owned(),
        prefix,
    })
}

/// `path`, absolute, relative to `root`, if it's under it: directly, or
/// through the working tree `located` names.
fn relative_to(root: &Path, located: Option<&Located>, path: &Path) -> Option<PathBuf> {
    if let Ok(relative) = path.strip_prefix(root) {
        return (!relative.as_os_str().is_empty()).then(|| relative.to_owned());
    }
    let located = located?;
    let relative = path
        .strip_prefix(&located.workdir)
        .ok()?
        .strip_prefix(&located.prefix)
        .ok()?;
    (!relative.as_os_str().is_empty()).then(|| relative.to_owned())
}

/// A status's letter in its row.
fn letter(kind: ChangeKind) -> &'static str {
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
fn letter_color(kind: ChangeKind, ui: &UiTheme) -> Hsla {
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
        // Read when first shown.
        if self.started == 0 {
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
            .w(PANEL_WIDTH)
            .h_full()
            .flex_none()
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

/// What the tests hold and count.
#[cfg(test)]
#[derive(Default)]
struct TestHooks {
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
    fn started(
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
        self.test.held.get_or_insert_default();
    }

    /// Lets the held runs, and the ones started from now, land.
    pub(crate) fn release_runs(&mut self) {
        for release in self.test.held.take().unwrap_or_default() {
            release.send(()).ok();
        }
    }

    /// Runs started.
    pub(crate) fn runs_started(&self) -> usize {
        self.test.started
    }

    /// Runs that read the status, and of them those off the main thread.
    pub(crate) fn runs_ran(&self) -> (usize, usize) {
        *self.test.ran.lock().unwrap()
    }

    pub(crate) fn is_running(&self) -> bool {
        self.running.is_some()
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
