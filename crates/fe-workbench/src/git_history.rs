//! The active file's git history, as fugitive's `:0Gclog` lists it:
//! `git.file_history` ("Git: File History"), typed as `:0Gclog`, `:Gclog %`
//! or `:Gclog -- %`, shows the History panel on the right dock with the
//! commits that changed the file, newest first, following renames
//! ([`Repo::file_log`], at most [`LIMIT`]), read on the background
//! executor. `:Gclog` with no range or argument, and `:Gclog --all`, are
//! the repository's log ([`git_log`]); its other forms answer `Not supported
//! yet: :<line>`.
//!
//! The list is a snapshot: running the command again reads it again, for
//! the file active then. Each row has the commit's short id and summary,
//! then its author and how long ago it was made, in the muted colour, and
//! for a rename the path it was renamed from.
//!
//! Opening a row (a click, Enter, or `o` with vim on) opens the commit's
//! change to the file in a [`DiffView`] tab, both sides read-only: the
//! file at the commit's parent (under its old name, for a rename; empty
//! for a root commit or one that added it) on the left, titled `<short>^`,
//! and at the commit on the right, titled `<short>` (empty for one that
//! deleted it). Opening it again shows that tab. The tab opens in the pane
//! active when it was asked for, and is shown and focused only if it's still
//! the last open asked for and nothing was navigated to meanwhile, as
//! `git_diff`'s diffs are. The sides' host commands (`:q`) are the diff
//! view's, as `git_diff` takes them.
//!
//! The panel takes its keys itself, as [`commit_rows`] says: up and down
//! move the selection and Enter opens; with vim on (its key context then
//! has the `vim` flag), `j` and `k` move, `g g` and `G` go to the first and
//! last row, `o` opens and `q` hides the dock, focusing the active editor.

use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_git::{FileCommit, Repo};
use fe_gpui::{DiffSide, DiffView};
use fe_ui::{ROW_HEIGHT, UiTheme};
use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable, Global,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, ScrollStrategy,
    SharedString, StatefulInteractiveElement as _, Styled as _, UniformListScrollHandle,
    WeakEntity, Window, div, prelude::FluentBuilder as _, px, uniform_list,
};
use kiln_shell::PaneView as _;

use super::{
    GitCommand, GitOp, Owner,
    commit_rows::{self, Key, ListKeys},
    git_log,
};
use crate::{Pane, Workbench, dock::DockOp, git};

/// A file's git history in the sidebar: on the rail once it has shown; focused
/// when shown, as its rows take keys.
pub(crate) const SIDEBAR: crate::dock::SidebarPanel = crate::dock::SidebarPanel {
    panel: crate::DockPanel::new("history"),
    name: "History",
    icon: fe_ui::Icon::FileDiff,
    show_command: "git.file_history",
    show_op: crate::dock::DockOp::ShowHistory,
    order: 70,
    always_on_rail: false,
    focus_on_show: true,
};

/// The most commits listed.
pub(crate) const LIMIT: usize = 1000;

/// The key context the panel's element sets.
const KEY_CONTEXT: &str = "history_panel";

/// What the panel and the command answer outside a repository.
const NO_REPO: &str = "Not in a git repository";

/// What the command answers for an editor without a file.
const NO_FILE: &str = "E32: No file name";

/// What the panel shows until the list lands.
const LOADING: &str = "Loading…";

/// Runs `op`, `git.file_history`: see the module docs.
pub(super) fn run(
    workbench: &mut Workbench,
    op: GitOp,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    if op.typed && !is_file_form(&op) {
        if let Some(command) = log_form(&op) {
            return git_log::run(workbench, GitOp { command, ..op }, window, cx);
        }
        let text = format!("Not supported yet: :{}", typed_line(&op));
        return workbench.answer_git_op(&op, text, cx);
    }
    workbench.show_file_history(op, window, cx);
}

/// Whether `op`, typed as `:Gclog`, is a form for the file: `:0Gclog`,
/// `:Gclog %` or `:Gclog -- %`.
fn is_file_form(op: &GitOp) -> bool {
    match &op.range {
        Some(range) => *range == (0..0) && op.args.is_empty(),
        None => op.args == "%" || op.args == "-- %",
    }
}

/// The log command `op`, typed as `:Gclog`, is a form of: `git.log` with no
/// range or argument, `git.log_all` for `:Gclog --all`.
fn log_form(op: &GitOp) -> Option<GitCommand> {
    match op.args.as_str() {
        _ if op.range.is_some() => None,
        "" => Some(GitCommand::Log),
        "--all" => Some(GitCommand::LogAll),
        _ => None,
    }
}

/// The line `op` was typed as, its range as rows from 1.
fn typed_line(op: &GitOp) -> String {
    let range = match &op.range {
        None => String::new(),
        Some(range) if range.is_empty() => range.start.to_string(),
        Some(Range { start, end }) if *end == start + 1 => end.to_string(),
        Some(Range { start, end }) => format!("{},{end}", start + 1),
    };
    let line = format!("{range}Gclog {}", op.args);
    line.trim_end().to_owned()
}

/// The name a tab gives `path`.
fn file_name(path: &Path) -> String {
    (path.file_name()).map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// What reading a file's history found: its repository and the commits
/// that changed it, none outside a repository, or the error's message.
type Found = Result<Option<(Arc<Repo>, Vec<FileCommit>)>, String>;

/// The repository of the file at `path` and the commits that changed it,
/// or none outside a repository. Runs on the background executor.
fn read_log(path: &Path) -> Found {
    let real = git::resolve(path);
    let Some(folder) = real.ancestors().skip(1).find(|folder| folder.is_dir()) else {
        return Ok(None);
    };
    let Some(repo) = Repo::discover(folder).map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    let commits = (repo.file_log(&real, LIMIT)).map_err(|error| error.to_string())?;
    Ok(Some((Arc::new(repo), commits)))
}

/// The texts of `commit`'s file at its parent and at it, empty where it
/// isn't there. Runs on the background executor.
fn read_change(repo: &Repo, commit: &FileCommit) -> Result<(String, String), String> {
    let id = &commit.commit.id;
    let parent = repo.parent(id).map_err(|error| error.to_string())?;
    let before = commit.from.as_deref().unwrap_or(&commit.path);
    let left = match parent {
        Some(parent) => repo.text_at(&parent, before),
        None => Ok(None),
    };
    let left = left.map_err(|error| error.to_string())?;
    let right = repo.text_at(id, &commit.path);
    let right = right.map_err(|error| error.to_string())?;
    Ok((left.unwrap_or_default(), right.unwrap_or_default()))
}

/// A read-only side titled `title` over `text`.
fn fixed_side(title: impl Into<SharedString>, text: &str) -> DiffSide {
    let buffer = Buffer::from_file_contents(text);
    DiffSide::new(title, EditorModel::new(buffer, None)).read_only()
}

/// The op of a click or key in the panel, answered in a toast.
fn clicked() -> GitOp {
    GitOp {
        command: GitCommand::FileHistory,
        args: String::new(),
        range: None,
        editor: None,
        typed: false,
    }
}

/// The commits' diff views opened, in any window, and the opens asked for.
#[derive(Default)]
struct Opened {
    /// Each one's commit id, file and view.
    views: Vec<(String, PathBuf, WeakEntity<DiffView>)>,
    /// Bumped by each open asked for: only the last is shown as it lands.
    asked: u64,
}

impl Global for Opened {}

impl Workbench {
    /// Lists the history of `op`'s editor's file in the History panel, made
    /// now if it hasn't been, and shows it.
    fn show_file_history(&mut self, op: GitOp, window: &mut Window, cx: &mut Context<Self>) {
        let path = (op.editor.as_ref())
            .and_then(|editor| editor.read(cx).model().path().map(Path::to_owned));
        let Some(path) = path else {
            return self.answer_git_op(&op, NO_FILE.to_owned(), cx);
        };
        let panel = match self.dock.read(cx).history().cloned() {
            Some(panel) => panel,
            None => {
                let (workbench, ui) = (cx.weak_entity(), self.ui);
                let panel = cx.new(|cx| HistoryPanel::new(workbench, ui, cx));
                self.dock
                    .update(cx, |dock, cx| dock.set_history(panel.clone(), cx));
                panel
            }
        };
        let root = self.root.clone();
        panel.update(cx, |panel, cx| panel.list(path, root, op, cx));
        self.run_dock_op(DockOp::ShowHistory, window, cx);
    }

    /// The pane and tab of this window's diff view of `commit`'s change to
    /// its file, if one is open.
    fn commit_diff(&self, commit: &FileCommit, cx: &mut App) -> Option<(Entity<Pane>, usize)> {
        let opened = &mut cx.default_global::<Opened>().views;
        opened.retain(|(.., view)| view.upgrade().is_some());
        let views: Vec<Entity<DiffView>> = (opened.iter())
            .filter(|(id, path, _)| *id == commit.commit.id && *path == commit.path)
            .filter_map(|(.., view)| view.upgrade())
            .collect();
        views.into_iter().find_map(|view| {
            self.panes(cx).into_iter().find_map(|pane| {
                let tabs = pane.read(cx).tabs();
                let ix = (tabs.iter()).position(|tab| tab.item().downcast_ref() == Some(&view))?;
                Some((pane.clone(), ix))
            })
        })
    }

    /// Shows tab `ix` of `pane`, focused.
    fn show_tab(
        &mut self,
        pane: Entity<Pane>,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate(pane.clone(), window, cx);
        pane.update(cx, |pane, cx| pane.activate(ix, window, cx));
    }

    /// Opens `commit`'s change to its file, read from `repo`, in a diff view
    /// tab of the active pane, or shows the one open: see the module docs.
    pub(super) fn open_commit(
        &mut self,
        repo: Arc<Repo>,
        commit: FileCommit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Asked for, even if it's open: an open still being read is no
        // longer the last.
        let landing = self.commit_landing(cx);
        if let Some((pane, ix)) = self.commit_diff(&commit, cx) {
            return self.show_tab(pane, ix, window, cx);
        }
        let at = commit.clone();
        let read = cx.background_spawn(async move { read_change(&repo, &at) });
        cx.spawn_in(window, async move |workbench, cx| {
            let read = read.await;
            (workbench.update_in(cx, |workbench, window, cx| {
                workbench.land_commit(commit, read, landing, window, cx)
            }))
            .ok();
        })
        .detach();
    }

    /// An open asked for now, in the active pane: see [`Landing`].
    fn commit_landing(&self, cx: &mut App) -> Landing {
        let opened = cx.default_global::<Opened>();
        opened.asked += 1;
        let asked = opened.asked;
        let pane = self.active.read(cx);
        Landing {
            asked,
            pane: self.active.downgrade(),
            navigations: pane.navigations(),
            opens: pane.open_serial(),
        }
    }

    /// Whether an open landing as `landing` says is to be shown: it's the
    /// last asked for, its pane is still the active one, and nothing was
    /// navigated to or opened there meanwhile.
    fn may_show_commit(&self, landing: &Landing, cx: &mut App) -> bool {
        let last = cx.default_global::<Opened>().asked == landing.asked;
        last && (landing.pane.upgrade()).is_some_and(|pane| {
            let state = pane.read(cx);
            pane == self.active
                && state.navigations() == landing.navigations
                && state.open_serial() == landing.opens
        })
    }

    /// Opens the diff view of `commit`, its texts as `read` read them, in
    /// `landing`'s pane (or the active pane, if that's gone), shown if
    /// [`Workbench::may_show_commit`]; else it waits in its tab.
    fn land_commit(
        &mut self,
        commit: FileCommit,
        read: Result<(String, String), String>,
        landing: Landing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (left, right) = match read {
            Ok(texts) => texts,
            Err(answer) => return self.answer_git_op(&clicked(), answer, cx),
        };
        // Before its tab's added, which may count as a navigation.
        let show = self.may_show_commit(&landing, cx);
        // Opened meanwhile.
        if let Some((pane, ix)) = self.commit_diff(&commit, cx) {
            if show {
                self.show_tab(pane, ix, window, cx);
            }
            return;
        }
        let short = &commit.commit.short;
        let left = fixed_side(format!("{short}^"), &left);
        let right = fixed_side(short.clone(), &right);
        let name = file_name(&commit.path);
        let diff = cx.new(|cx| DiffView::new(name, left, right, cx));
        let sides = {
            let view = diff.read(cx);
            [view.left().clone(), view.right().clone()]
        };
        for side in &sides {
            self.own_editor(side, Owner::DiffView(diff.downgrade()));
        }
        let opened = &mut cx.default_global::<Opened>().views;
        opened.push((commit.commit.id.clone(), commit.path, diff.downgrade()));
        let pane = (landing.pane.upgrade())
            .filter(|pane| self.panes(cx).contains(pane))
            .unwrap_or_else(|| self.active.clone());
        let ix = pane.update(cx, |pane, cx| {
            let ix = pane.pane_mut().push(Box::new(diff.clone()), cx);
            pane.items_changed(cx);
            cx.notify();
            ix
        });
        if show {
            self.show_tab(pane, ix, window, cx);
        }
    }
}

/// Where a commit's open was asked for, and what decides whether it's shown
/// as it lands, as `git_diff`'s diffs are.
struct Landing {
    /// The open's number.
    asked: u64,
    pane: WeakEntity<Pane>,
    /// The pane's navigations when asked.
    navigations: u64,
    /// The pane's file opens when asked.
    opens: u64,
}

/// What the panel shows in place of the list, or the list.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Status {
    /// Nothing listed yet, or the list being read.
    Reading,
    NotARepository,
    /// The error's message.
    Failed(String),
    Listed,
}

/// The History panel: see the module docs.
pub(crate) struct HistoryPanel {
    workbench: WeakEntity<Workbench>,
    /// The file listed's name.
    name: Option<String>,
    /// What renamed-from paths show relative to, if they're under it.
    root: Option<PathBuf>,
    status: Status,
    /// The repository the list was read from.
    repo: Option<Arc<Repo>>,
    commits: Vec<FileCommit>,
    /// When the list landed, for how long ago each commit was.
    now: i64,
    selected: Option<usize>,
    keys: ListKeys,
    /// The last read started's number: only it lands.
    read: u64,
    scroll: UniformListScrollHandle,
    ui: UiTheme,
    focus: FocusHandle,
}

impl HistoryPanel {
    fn new(workbench: WeakEntity<Workbench>, ui: UiTheme, cx: &mut Context<Self>) -> Self {
        Self {
            workbench,
            name: None,
            root: None,
            status: Status::Reading,
            repo: None,
            commits: Vec::new(),
            now: 0,
            selected: None,
            keys: ListKeys::default(),
            read: 0,
            scroll: UniformListScrollHandle::new(),
            ui,
            focus: cx.focus_handle(),
        }
    }

    /// The dock's title row: `History: <file name>`.
    pub(crate) fn title(&self, _: &App) -> SharedString {
        match &self.name {
            Some(name) => format!("History: {name}").into(),
            None => "History".into(),
        }
    }

    /// Reads the history of the file at `path` on the background executor,
    /// showing it loading until it lands; `op` is answered if it's outside
    /// a repository.
    fn list(&mut self, path: PathBuf, root: Option<PathBuf>, op: GitOp, cx: &mut Context<Self>) {
        self.read += 1;
        let read = self.read;
        self.name = Some(file_name(&path));
        self.root = root;
        self.status = Status::Reading;
        self.repo = None;
        self.commits.clear();
        self.selected = None;
        let found = cx.background_spawn(async move { read_log(&path) });
        cx.spawn(async move |panel, cx| {
            let found = found.await;
            (panel.update(cx, |panel, cx| panel.land(read, found, op, cx))).ok();
        })
        .detach();
        cx.notify();
    }

    /// Shows the list read `read` found, unless a later read started.
    fn land(&mut self, read: u64, found: Found, op: GitOp, cx: &mut Context<Self>) {
        if read != self.read {
            return;
        }
        self.now = commit_rows::now();
        match found {
            Ok(Some((repo, commits))) => {
                self.status = Status::Listed;
                self.repo = Some(repo);
                self.selected = (!commits.is_empty()).then_some(0);
                self.commits = commits;
                self.scroll.scroll_to_item(0, ScrollStrategy::Top);
            }
            Ok(None) => {
                self.status = Status::NotARepository;
                if let Some(workbench) = self.workbench.upgrade() {
                    workbench.update(cx, |workbench, cx| {
                        workbench.answer_git_op(&op, NO_REPO.to_owned(), cx)
                    });
                }
            }
            Err(error) => self.status = Status::Failed(error),
        }
        cx.notify();
    }

    /// What shows in place of the list, if anything.
    fn message(&self) -> Option<String> {
        Some(match &self.status {
            Status::Reading => LOADING.to_owned(),
            Status::NotARepository => NO_REPO.to_owned(),
            Status::Failed(error) => error.clone(),
            Status::Listed if self.commits.is_empty() => {
                format!(
                    "No commits for {}",
                    self.name.as_deref().unwrap_or_default()
                )
            }
            Status::Listed => return None,
        })
    }

    /// Whether the list stopped at [`LIMIT`], which its last row says.
    fn limited(&self) -> bool {
        self.commits.len() >= LIMIT
    }

    /// `path`, as a rename's row shows it: from the root, if it's under it,
    /// else from the repository's working tree.
    fn display_path(&self, path: &Path) -> String {
        let root = self.root.as_deref();
        let bases = [root.map(Path::to_owned), root.map(git::resolve)];
        let workdir = self.repo.as_deref().map(|repo| repo.workdir().to_owned());
        let relative = (bases.into_iter().chain([workdir]).flatten())
            .find_map(|base| Some(path.strip_prefix(base).ok()?.to_owned()));
        relative.as_deref().unwrap_or(path).display().to_string()
    }

    /// Row `ix`'s texts: `<short>  <summary>`, `<author>, <time ago>`, and
    /// for a rename `renamed from <old path>`; or the limit's row.
    fn row_texts(&self, ix: usize) -> Option<Vec<String>> {
        let Some(entry) = self.commits.get(ix) else {
            return (self.limited() && ix == self.commits.len())
                .then(|| vec![format!("Showing the newest {LIMIT} commits")]);
        };
        let mut texts = commit_rows::texts(&entry.commit, self.now).to_vec();
        if let Some(from) = entry.from.as_deref().filter(|from| *from != entry.path) {
            texts.push(format!("renamed from {}", self.display_path(from)));
        }
        Some(texts)
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Selects row `ix` and opens its commit's change to the file.
    fn open(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(commit), Some(repo)) = (self.commits.get(ix).cloned(), self.repo.clone()) else {
            return;
        };
        self.select(ix, cx);
        if let Some(workbench) = self.workbench.upgrade() {
            workbench.update(cx, |workbench, cx| {
                workbench.open_commit(repo, commit, window, cx)
            });
        }
    }

    /// Its keys: see the module docs.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.keys.key(event, cx) else {
            return;
        };
        match key {
            Key::Quit => commit_rows::leave(self.workbench.clone(), window, cx),
            Key::Open => {
                if let Some(ix) = self.selected.filter(|ix| *ix < self.commits.len()) {
                    self.open(ix, window, cx);
                }
            }
            _ => {
                if let Some(ix) = key.moved(self.selected, self.commits.len()) {
                    self.select(ix, cx);
                }
            }
        }
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
        // Each list read has its own IDs, so a row pressed before the list
        // changed isn't clicked as the row that replaced it.
        let name = SharedString::from(format!("history-{}", self.read));
        range
            .filter_map(|ix| {
                let mut texts = self.row_texts(ix)?.into_iter();
                let first = texts.next()?;
                let row = div()
                    .id(ElementId::NamedInteger(name.clone(), ix as u64))
                    .h(ROW_HEIGHT * 2.)
                    .w_full()
                    .px_3()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .overflow_hidden()
                    .whitespace_nowrap();
                if ix >= self.commits.len() {
                    return Some(
                        row.text_color(ui.muted_text)
                            .child(first)
                            .into_any_element(),
                    );
                }
                Some(
                    row.when(self.selected == Some(ix), |row| row.bg(selected_bg))
                        .hover(move |row| row.bg(ui.row_hover))
                        .on_click(
                            cx.listener(move |panel, _, window, cx| panel.open(ix, window, cx)),
                        )
                        .child(div().overflow_hidden().text_ellipsis().child(first))
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .overflow_hidden()
                                .text_color(ui.muted_text)
                                .children(texts.map(|text| div().flex_none().child(text))),
                        )
                        .into_any_element(),
                )
            })
            .collect()
    }
}

impl Focusable for HistoryPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for HistoryPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = self.ui;
        let rows = self.commits.len() + usize::from(self.limited());
        let list = uniform_list(
            "history-rows",
            rows,
            cx.processor(|panel, range, window, cx| panel.render_rows(range, window, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1();
        let error = matches!(self.status, Status::Failed(_));
        div()
            .track_focus(&self.focus)
            .key_context(commit_rows::key_context(KEY_CONTEXT, cx))
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

#[cfg(test)]
impl HistoryPanel {
    /// Each row's texts, as [`HistoryPanel::row_texts`] gives them.
    pub(crate) fn rows(&self) -> Vec<Vec<String>> {
        let rows = self.commits.len() + usize::from(self.limited());
        (0..rows).filter_map(|ix| self.row_texts(ix)).collect()
    }

    /// What shows in place of the list, if anything.
    pub(crate) fn shown_message(&self) -> Option<String> {
        self.message()
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        self.selected
    }
}

#[cfg(test)]
#[path = "git_history_tests.rs"]
mod git_history_tests;
