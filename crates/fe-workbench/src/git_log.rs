//! The repository's log with its branch graph, as VS Code's Source Control
//! Graph and fugitive's `:Gclog` show it: `git.log` ("Git: Log", typed as
//! `:Gclog`) lists HEAD's history and `git.log_all` ("Git: Log (All
//! Branches)", `:Gclog --all`) every branch's, remote-tracking branch's and
//! tag's, newest first in topo order ([`Repo::log`]), in the Git Log panel
//! on the right dock. The repository is the active file's, else the root's.
//!
//! The list is read on the background executor a page of [`PAGE`] commits
//! at a time: the first when the command runs, the next once the selection
//! or the rows drawn come within [`NEAR`] rows of the end, laid out with
//! the same [`GraphLayout`] so the graph goes on. A last row says
//! `Loading…` while a page is read; there are no more once a page comes
//! back short. The list is a snapshot: every page is read from the commits
//! the refs pointed at when the first was ([`Repo::log_tips`]), so commits,
//! resets and branch switches meanwhile don't shift it; running the command
//! again reads it again.
//!
//! Each row has the commit's graph cell (its lanes drawn on a canvas, in
//! VS Code's five graph colours, its dot a ring for a merge), the refs
//! pointing at it as small labels (HEAD's branch bold, a tag's prefixed
//! `tag`, a remote-tracking branch's muted), then its short id and summary
//! and, muted, its author and how long ago it was made ([`commit_rows`]).
//!
//! The panel takes its keys as [`commit_rows`] says: up and down move the
//! selection; with vim on, `j` and `k` move, `g g` and `G` go to the first
//! and last row loaded and `q` hides the dock, focusing the active editor.
//! `G` reads no further page: none is read until the selection moves or the
//! list is scrolled (by the wheel, even at its end, or to other rows).
//!
//! Enter, `o` and a click on a commit expand it: its files
//! ([`Repo::commit_files`], against its first parent, read on the background
//! executor, a `Loading…` row under it until they arrive and a muted row for
//! an error) are listed under it, indented, as `A`, `M` or `D` and the path
//! from the root, or `R old → new`; doing it again collapses it. Enter, `o`
//! or a click on a file opens its change at that commit as the History panel
//! opens one ([`Workbench::open_commit`]), so the same one is the same tab.
//! The selection moves over commit and file rows alike.

use std::{
    collections::BTreeMap,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_git::{
    ChangeKind, CommitFile, FileCommit, Head, LogCommit, LogScope, RefKind, RefLabel, Repo,
    graph::{GraphLayout, GraphRow},
};
use fe_ui::{ROW_HEIGHT, UiTheme};
use gpui::{
    AnyElement, App, AppContext as _, BorderStyle, Bounds, Context, ElementId, FocusHandle,
    Focusable, FontWeight, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, PathBuilder, Pixels, Point, Render, ScrollStrategy, ScrollWheelEvent,
    SharedString, StatefulInteractiveElement as _, Styled as _, UniformListScrollHandle,
    WeakEntity, Window, canvas, div, point, prelude::FluentBuilder as _, px, quad, size,
    transparent_black, uniform_list,
};

use super::{
    GitCommand, GitOp,
    commit_rows::{self, Key, ListKeys},
};
use crate::{Workbench, dock::DockOp, git};

/// The commits a page reads.
pub(crate) const PAGE: usize = 200;

/// How near the end of the rows loaded the selection or the rows drawn
/// come before the next page is read.
pub(crate) const NEAR: usize = 20;

/// The key context the panel's element sets.
const KEY_CONTEXT: &str = "git_log_panel";

/// What the panel and the command answer outside a repository.
const NO_REPO: &str = "Not in a git repository";

/// What the panel shows for an unborn HEAD.
const NO_COMMITS: &str = "No commits yet";

/// What the last row says while a page is read.
const LOADING: &str = "Loading…";

/// A lane's width in the graph cell.
const LANE_WIDTH: Pixels = px(12.);

/// A commit's dot's diameter.
const DOT: Pixels = px(7.);

/// A lane's line width, and a merge's hollow dot's.
const LINE: Pixels = px(1.5);

/// The lanes' colours, by [`GraphRow::color`] modulo their number: VS
/// Code's `scmGraph.foreground1` to `5` defaults.
const GRAPH_COLORS: [u32; 5] = [0xFFB000, 0xDC267F, 0x994F00, 0x40B0A6, 0xB66DFF];

/// Runs `op`, `git.log` or `git.log_all`: see the module docs.
pub(super) fn run(
    workbench: &mut Workbench,
    op: GitOp,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) {
    workbench.show_git_log(op, window, cx);
}

/// The colour of lane colour `color`.
fn lane_color(color: usize) -> Hsla {
    gpui::rgb(GRAPH_COLORS[color % GRAPH_COLORS.len()]).into()
}

/// How many lanes `row` reaches across.
fn lanes(row: &GraphRow) -> usize {
    let lines = row.top.iter().chain(&row.bottom);
    let widest = lines.map(|line| line.from.max(line.to)).max();
    widest.unwrap_or(0).max(row.column) + 1
}

/// What a row of the list is: a commit (by its index), one of its files
/// (the commit's, then the file's index), or the row under a commit saying
/// its files are loading or couldn't be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Line {
    Commit(usize),
    File(usize, usize),
    Note(usize),
}

impl Line {
    /// An identity for the row's element that holds whatever rows are
    /// listed before it.
    fn id(self) -> u64 {
        const ROW: u64 = 1 << 24;
        match self {
            Line::Commit(ix) => ix as u64 * ROW,
            Line::File(ix, file) => ix as u64 * ROW + file as u64 + 1,
            Line::Note(ix) => ix as u64 * ROW + ROW - 1,
        }
    }

    /// The index of the commit the row is or is under.
    fn commit(self) -> usize {
        match self {
            Line::Commit(ix) | Line::File(ix, _) | Line::Note(ix) => ix,
        }
    }
}

/// A commit's files, once it's expanded.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Listing {
    /// Being read by the request with this number.
    Loading(u64),
    Files(Vec<CommitFile>),
    Failed(String),
}

/// What reading a log's first page found: its repository, where HEAD is,
/// the commits the log starts from and the first page; none outside a
/// repository; or the error's message.
type Found = Result<Option<(Arc<Repo>, Head, Arc<[String]>, Vec<LogCommit>)>, String>;

/// The repository of `at`, a file or folder, where its HEAD is, the
/// commits its `scope` log starts from now and the log's first page, or
/// none outside a repository. Runs on the background executor.
fn read_first(at: &Path, scope: LogScope) -> Found {
    let real = git::resolve(at);
    let Some(folder) = real.ancestors().find(|folder| folder.is_dir()) else {
        return Ok(None);
    };
    let Some(repo) = Repo::discover(folder).map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    let head = repo.current_branch().map_err(|error| error.to_string())?;
    let tips = repo.log_tips(scope).map_err(|error| error.to_string())?;
    let commits = (repo.log_from(&tips, 0, PAGE)).map_err(|error| error.to_string())?;
    Ok(Some((Arc::new(repo), head, tips.into(), commits)))
}

impl Workbench {
    /// Lists `op`'s log, of the active file's repository or else the
    /// root's, in the Git Log panel, made now if it hasn't been, and shows
    /// it.
    fn show_git_log(&mut self, op: GitOp, window: &mut Window, cx: &mut Context<Self>) {
        let scope = match op.command {
            GitCommand::LogAll => LogScope::All,
            _ => LogScope::Head,
        };
        let path = (op.editor.as_ref())
            .and_then(|editor| editor.read(cx).model().path().map(Path::to_owned));
        let Some(at) = path.or_else(|| self.root.clone()) else {
            return self.answer_git_op(&op, NO_REPO.to_owned(), cx);
        };
        let panel = match self.dock.read(cx).git_log().cloned() {
            Some(panel) => panel,
            None => {
                let (workbench, ui) = (cx.weak_entity(), self.ui);
                let panel = cx.new(|cx| GitLogPanel::new(workbench, ui, cx));
                self.dock
                    .update(cx, |dock, cx| dock.set_git_log(panel.clone(), cx));
                panel
            }
        };
        let root = self.root.clone();
        panel.update(cx, |panel, cx| panel.list(at, root, scope, op, cx));
        self.run_dock_op(DockOp::ShowGitLog, window, cx);
    }
}

/// What the list's last row says, after the commits, if anything.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Tail {
    None,
    /// A page is being read.
    Loading,
    /// Reading a page failed: the error's message.
    Failed(String),
}

/// Whether `G` holds the next page back: until the selection moves, the
/// wheel turns over the list, or rows other than those it scrolled to are
/// drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Hold {
    None,
    /// `G` was pressed; the rows it scrolls to aren't drawn yet.
    Scrolling,
    /// The rows drawn once it had scrolled.
    Drawn(Range<usize>),
}

/// The Git Log panel: see the module docs.
pub(crate) struct GitLogPanel {
    workbench: WeakEntity<Workbench>,
    scope: LogScope,
    /// Where HEAD was when the list was read, once it has been.
    head: Option<Head>,
    /// Whether the list was asked for outside a repository.
    not_repository: bool,
    /// The repository the list is read from.
    repo: Option<Arc<Repo>>,
    /// The commits its pages are read from.
    tips: Arc<[String]>,
    commits: Vec<LogCommit>,
    /// Each commit's graph row.
    rows: Vec<GraphRow>,
    /// The layout after the last row, which the next page goes on with.
    layout: GraphLayout,
    /// The most lanes a row reaches across: the graph cells' width.
    lanes: usize,
    tail: Tail,
    /// Whether the last page read was full, so there may be more.
    more: bool,
    /// When the first page landed, for how long ago each commit was.
    now: i64,
    /// The expanded commits' files, by commit index.
    expanded: BTreeMap<usize, Listing>,
    /// The last expansion request's number: only the one a commit's
    /// `Loading` names lands.
    requests: u64,
    /// The rows of the list: each commit, and under the expanded ones their
    /// files.
    lines: Vec<Line>,
    /// What file paths show relative to, if they're under it.
    root: Option<PathBuf>,
    /// The selected row of `lines`.
    selected: Option<usize>,
    /// Whether `G` holds the next page back.
    hold: Hold,
    keys: ListKeys,
    /// The last list started's number: only its pages land.
    read: u64,
    scroll: UniformListScrollHandle,
    ui: UiTheme,
    focus: FocusHandle,
}

impl GitLogPanel {
    fn new(workbench: WeakEntity<Workbench>, ui: UiTheme, cx: &mut Context<Self>) -> Self {
        Self {
            workbench,
            scope: LogScope::Head,
            head: None,
            not_repository: false,
            repo: None,
            tips: Arc::from([]),
            commits: Vec::new(),
            rows: Vec::new(),
            layout: GraphLayout::new(),
            lanes: 0,
            tail: Tail::None,
            more: false,
            now: 0,
            expanded: BTreeMap::new(),
            requests: 0,
            lines: Vec::new(),
            root: None,
            selected: None,
            hold: Hold::None,
            keys: ListKeys::default(),
            read: 0,
            scroll: UniformListScrollHandle::new(),
            ui,
            focus: cx.focus_handle(),
        }
    }

    /// The dock's title row: `Git Log: <branch>`, `Git Log: HEAD
    /// (detached)` or `Git Log: All Branches`.
    pub(crate) fn title(&self, _: &App) -> SharedString {
        match (self.scope, &self.head) {
            (LogScope::All, _) => "Git Log: All Branches".into(),
            (_, Some(Head::Branch(name) | Head::Unborn(name))) => format!("Git Log: {name}").into(),
            (_, Some(Head::Detached(_))) => "Git Log: HEAD (detached)".into(),
            (_, None) => "Git Log".into(),
        }
    }

    /// Reads the first page of the `scope` log of `at`'s repository on the
    /// background executor, showing it loading until it lands; `op` is
    /// answered if it's outside a repository.
    fn list(
        &mut self,
        at: PathBuf,
        root: Option<PathBuf>,
        scope: LogScope,
        op: GitOp,
        cx: &mut Context<Self>,
    ) {
        self.read += 1;
        let read = self.read;
        self.scope = scope;
        self.head = None;
        self.not_repository = false;
        self.repo = None;
        self.tips = Arc::from([]);
        self.commits.clear();
        self.rows.clear();
        self.expanded.clear();
        self.lines.clear();
        self.root = root;
        self.layout = GraphLayout::new();
        self.lanes = 0;
        self.tail = Tail::Loading;
        self.more = false;
        self.selected = None;
        self.hold = Hold::None;
        let found = cx.background_spawn(async move { read_first(&at, scope) });
        cx.spawn(async move |panel, cx| {
            let found = found.await;
            (panel.update(cx, |panel, cx| panel.land(read, found, op, cx))).ok();
        })
        .detach();
        cx.notify();
    }

    /// Shows the first page read `read` found, unless a later read started.
    fn land(&mut self, read: u64, found: Found, op: GitOp, cx: &mut Context<Self>) {
        if read != self.read {
            return;
        }
        self.now = commit_rows::now();
        self.tail = Tail::None;
        match found {
            Ok(Some((repo, head, tips, commits))) => {
                self.repo = Some(repo);
                self.tips = tips;
                self.head = Some(head);
                self.append(commits);
                self.selected = (!self.lines.is_empty()).then_some(0);
                self.scroll.scroll_to_item(0, ScrollStrategy::Top);
            }
            Ok(None) => {
                self.not_repository = true;
                if let Some(workbench) = self.workbench.upgrade() {
                    workbench.update(cx, |workbench, cx| {
                        workbench.answer_git_op(&op, NO_REPO.to_owned(), cx)
                    });
                }
            }
            Err(error) => self.tail = Tail::Failed(error),
        }
        cx.notify();
    }

    /// Adds `page` to the list, its graph going on from the rows before.
    fn append(&mut self, page: Vec<LogCommit>) {
        self.more = page.len() >= PAGE;
        for entry in &page {
            let row = self.layout.push(&entry.commit.id, &entry.parents);
            self.lanes = self.lanes.max(lanes(&row));
            self.rows.push(row);
        }
        self.commits.extend(page);
        self.relayout();
    }

    /// Lists the rows again, the selection staying on its row, or else the
    /// commit it was under.
    fn relayout(&mut self) {
        let was = self.selected.and_then(|ix| self.lines.get(ix).copied());
        self.lines.clear();
        for ix in 0..self.commits.len() {
            self.lines.push(Line::Commit(ix));
            match self.expanded.get(&ix) {
                None => {}
                Some(Listing::Files(files)) => {
                    (self.lines).extend((0..files.len()).map(|file| Line::File(ix, file)));
                }
                Some(_) => self.lines.push(Line::Note(ix)),
            }
        }
        if let Some(was) = was {
            let find = |line: Line| self.lines.iter().position(|other| *other == line);
            self.selected = find(was).or_else(|| find(Line::Commit(was.commit())));
        }
    }

    /// `path` as a file row shows it: from the root, if it's under it, else
    /// from the repository's working tree.
    fn display_path(&self, path: &Path) -> String {
        let root = self.root.as_deref();
        let bases = [root.map(Path::to_owned), root.map(git::resolve)];
        let workdir = self.repo.as_deref().map(|repo| repo.workdir().to_owned());
        let relative = (bases.into_iter().chain([workdir]).flatten())
            .find_map(|base| Some(path.strip_prefix(base).ok()?.to_owned()));
        relative.as_deref().unwrap_or(path).display().to_string()
    }

    /// The text of file `file` of commit `commit`: `M path`, `R old → new`.
    fn file_text(&self, commit: usize, file: usize) -> Option<String> {
        let Some(Listing::Files(files)) = self.expanded.get(&commit) else {
            return None;
        };
        let file = files.get(file)?;
        let path = self.display_path(&file.path);
        Some(match (&file.from, file.kind) {
            (Some(from), _) => format!("R {} → {path}", self.display_path(from)),
            (None, ChangeKind::Added) => format!("A {path}"),
            (None, ChangeKind::Deleted) => format!("D {path}"),
            (None, _) => format!("M {path}"),
        })
    }

    /// Expands commit `ix`, reading its files on the background executor, or
    /// collapses it.
    fn toggle(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.expanded.remove(&ix).is_some() {
            self.relayout();
            return cx.notify();
        }
        let (Some(repo), Some(entry)) = (self.repo.clone(), self.commits.get(ix)) else {
            return;
        };
        let (read, id) = (self.read, entry.commit.id.clone());
        self.requests += 1;
        let request = self.requests;
        self.expanded.insert(ix, Listing::Loading(request));
        self.relayout();
        let files = cx.background_spawn(async move {
            (repo.commit_files(&id)).map_err(|error| error.to_string())
        });
        cx.spawn(async move |panel, cx| {
            let files = files.await;
            (panel.update(cx, |panel, cx| {
                panel.land_files(read, (ix, request), files, cx)
            }))
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Lists the files read for commit `ix`, if it's still expanded in the
    /// list they were read for.
    fn land_files(
        &mut self,
        read: u64,
        (ix, request): (usize, u64),
        files: Result<Vec<CommitFile>, String>,
        cx: &mut Context<Self>,
    ) {
        if read != self.read || self.expanded.get(&ix) != Some(&Listing::Loading(request)) {
            return;
        }
        let listing = match files {
            Ok(files) => Listing::Files(files),
            Err(error) => Listing::Failed(error),
        };
        self.expanded.insert(ix, listing);
        self.relayout();
        cx.notify();
    }

    /// Opens file `file` of commit `commit`'s change.
    fn open_file(&mut self, commit: usize, file: usize, window: &mut Window, cx: &mut App) {
        let (Some(repo), Some(entry)) = (self.repo.clone(), self.commits.get(commit)) else {
            return;
        };
        let Some(Listing::Files(files)) = self.expanded.get(&commit) else {
            return;
        };
        let Some(file) = files.get(file) else {
            return;
        };
        let commit = FileCommit {
            commit: entry.commit.clone(),
            path: file.path.clone(),
            from: file.from.clone(),
        };
        let workbench = self.workbench.clone();
        // Once this update is over: the workbench moves focus.
        window.defer(cx, move |window, cx| {
            let open = |workbench: &mut Workbench, cx: &mut Context<Workbench>| {
                workbench.open_commit(repo, commit, window, cx)
            };
            workbench.update(cx, open).ok();
        });
    }

    /// A click on `line`, wherever its row is by now: selects it and, if
    /// `open`, opens it.
    fn click(&mut self, line: Line, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.lines.iter().position(|other| *other == line) else {
            return;
        };
        self.select(ix, cx);
        if open {
            self.open(ix, window, cx);
        }
    }

    /// Enter, `o` or a click on row `ix`: expands or collapses a commit,
    /// opens a file.
    fn open(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        match self.lines.get(ix).copied() {
            Some(Line::Commit(commit)) => self.toggle(commit, cx),
            Some(Line::File(commit, file)) => self.open_file(commit, file, window, cx),
            Some(Line::Note(_)) | None => {}
        }
    }

    /// Whether row `ix` is within [`NEAR`] rows of the end of those loaded.
    fn near_end(&self, ix: usize) -> bool {
        ix + NEAR >= self.commits.len()
    }

    /// Reads the next page on the background executor, unless one is being
    /// read, the last came back short or failed.
    fn load_more(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else {
            return;
        };
        if !self.more || self.tail != Tail::None {
            return;
        }
        self.tail = Tail::Loading;
        let (read, tips, skip) = (self.read, self.tips.clone(), self.commits.len());
        let page = cx.background_spawn(async move {
            (repo.log_from(&tips, skip, PAGE)).map_err(|error| error.to_string())
        });
        cx.spawn(async move |panel, cx| {
            let page = page.await;
            (panel.update(cx, |panel, cx| panel.land_page(read, skip, page, cx))).ok();
        })
        .detach();
        cx.notify();
    }

    /// Adds the page read from `skip` on to the list, unless it's no longer
    /// the list it was read for.
    fn land_page(
        &mut self,
        read: u64,
        skip: usize,
        page: Result<Vec<LogCommit>, String>,
        cx: &mut Context<Self>,
    ) {
        if read != self.read || skip != self.commits.len() {
            return;
        }
        match page {
            Ok(page) => {
                self.tail = Tail::None;
                self.append(page);
            }
            Err(error) => self.tail = Tail::Failed(error),
        }
        cx.notify();
    }

    /// What shows in place of the list, if anything.
    fn message(&self) -> Option<&'static str> {
        if self.not_repository {
            Some(NO_REPO)
        } else if self.commits.is_empty() && self.tail == Tail::None {
            Some(NO_COMMITS)
        } else {
            None
        }
    }

    /// The last row's text, after the commits, if there is one.
    fn tail_text(&self) -> Option<&str> {
        match &self.tail {
            Tail::None => None,
            Tail::Loading => Some(LOADING),
            Tail::Failed(error) => Some(error),
        }
    }

    fn row_count(&self) -> usize {
        self.lines.len() + usize::from(self.tail_text().is_some())
    }

    /// Selects row `ix`, reading the next page if it's near the end.
    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        self.hold = Hold::None;
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        if self
            .lines
            .get(ix)
            .is_some_and(|line| self.near_end(line.commit()))
        {
            self.load_more(cx);
        }
        cx.notify();
    }

    /// `G`: selects the last row loaded, reading no further page until the
    /// selection moves or the list scrolls.
    fn select_last(&mut self, cx: &mut Context<Self>) {
        let Some(last) = self.lines.len().checked_sub(1) else {
            return;
        };
        self.selected = Some(last);
        self.hold = Hold::Scrolling;
        self.scroll.scroll_to_item(last, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// The wheel over the list: a scroll by hand, which `G`'s hold gives way
    /// to, even at the end of the rows, where nothing moves.
    fn on_scroll_wheel(&mut self, _: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.hold != Hold::None {
            self.hold = Hold::None;
            cx.notify();
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
                if let Some(ix) = self.selected {
                    self.open(ix, window, cx);
                }
            }
            Key::Last => self.select_last(cx),
            _ => {
                if let Some(ix) = key.moved(self.selected, self.lines.len()) {
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
        // Held from the rows `G` scrolled to until others are drawn. The
        // list also renders one row to measure it: that's not drawing.
        match &self.hold {
            _ if range.len() <= 1 => {}
            Hold::Scrolling => self.hold = Hold::Drawn(range.clone()),
            Hold::Drawn(drawn) if *drawn != range => self.hold = Hold::None,
            _ => {}
        }
        // Drawn near the end: the next page, once the frame is over.
        let last = (range.end.checked_sub(1)).and_then(|ix| self.lines.get(ix));
        let near = last.is_some_and(|line| self.near_end(line.commit()));
        if near && self.hold == Hold::None && self.more && self.tail == Tail::None {
            cx.spawn(async move |panel, cx| panel.update(cx, |panel, cx| panel.load_more(cx)).ok())
                .detach();
        }
        let ui = self.ui;
        let selected_bg = if self.focus.contains_focused(window, cx) {
            ui.row_selected
        } else {
            ui.row_selected_inactive
        };
        // Each list read has its own IDs, so a row pressed before the list
        // changed isn't clicked as the row that replaced it.
        let name = SharedString::from(format!("git-log-{}", self.read));
        range
            .filter_map(|ix| {
                let line_id = self.lines.get(ix).map_or(u64::MAX, |line| line.id());
                let row = div()
                    .id(ElementId::NamedInteger(name.clone(), line_id))
                    .h(ROW_HEIGHT)
                    .w_full()
                    .pl_2()
                    .pr_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .whitespace_nowrap();
                let selected = self.selected == Some(ix);
                let line = match self.lines.get(ix) {
                    Some(line) => *line,
                    None => {
                        let error = matches!(self.tail, Tail::Failed(_));
                        let color = if error { ui.error_text } else { ui.muted_text };
                        let text = self.tail_text()?.to_owned();
                        return Some(row.text_color(color).child(text).into_any_element());
                    }
                };
                let indent = LANE_WIDTH * self.lanes as f32 + px(24.);
                let row = match line {
                    Line::Commit(_) => row,
                    Line::File(commit, file) => {
                        let text = self.file_text(commit, file).unwrap_or_default();
                        return Some(
                            row.pl(indent)
                                .when(selected, |row| row.bg(selected_bg))
                                .hover(move |row| row.bg(ui.row_hover))
                                .on_click(cx.listener(move |panel, _, window, cx| {
                                    panel.click(line, true, window, cx)
                                }))
                                .child(text)
                                .into_any_element(),
                        );
                    }
                    Line::Note(commit) => {
                        let (text, color) = match self.expanded.get(&commit) {
                            Some(Listing::Failed(error)) => (error.clone(), ui.error_text),
                            _ => (LOADING.to_owned(), ui.muted_text),
                        };
                        return Some(
                            row.pl(indent)
                                .when(selected, |row| row.bg(selected_bg))
                                .text_color(color)
                                .on_click(cx.listener(move |panel, _, window, cx| {
                                    panel.click(line, false, window, cx)
                                }))
                                .child(text)
                                .into_any_element(),
                        );
                    }
                };
                let Line::Commit(ix_commit) = line else {
                    return None;
                };
                let entry = self.commits.get(ix_commit)?;
                let merge = entry.parents.len() > 1;
                let graph = graph_cell(self.rows[ix_commit].clone(), merge, self.lanes);
                let [summary, when] = commit_rows::texts(&entry.commit, self.now);
                Some(
                    row.when(selected, |row| row.bg(selected_bg))
                        .hover(move |row| row.bg(ui.row_hover))
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            panel.click(line, true, window, cx)
                        }))
                        .child(graph)
                        .children(entry.refs.iter().map(|label| ref_label(label, ui)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(summary),
                        )
                        .child(div().flex_none().text_color(ui.muted_text).child(when))
                        .into_any_element(),
                )
            })
            .collect()
    }
}

/// A ref label's text: a tag's prefixed `tag`.
fn label_text(label: &RefLabel) -> String {
    match label.kind {
        RefKind::Tag => format!("tag {}", label.name),
        _ => label.name.clone(),
    }
}

/// A ref pointing at a commit, as a small label: HEAD's bold, a remote's
/// muted.
fn ref_label(label: &RefLabel, ui: UiTheme) -> impl IntoElement {
    div()
        .flex_none()
        .px_1()
        .rounded_sm()
        .border_1()
        .border_color(ui.border)
        .text_size(px(11.))
        .when(label.is_head, |label| label.font_weight(FontWeight::BOLD))
        .when(label.kind == RefKind::Remote, |label| {
            label.text_color(ui.muted_text)
        })
        .child(label_text(label))
}

/// A commit's graph cell, [`LANE_WIDTH`] per lane of `lanes`: its row's
/// lines and its dot, a ring for a merge.
fn graph_cell(row: GraphRow, merge: bool, lanes: usize) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| paint_graph(bounds, &row, merge, window),
    )
    .flex_none()
    .w(LANE_WIDTH * lanes as f32)
    .h(ROW_HEIGHT)
}

/// Paints `row` in `bounds`: each line of its top half from the top edge
/// to the middle and of its bottom half from the middle to the bottom
/// edge, then the commit's dot. A merge's is a ring the lines meeting it
/// stop at, so the row's background, selected or hovered, shows inside.
fn paint_graph(bounds: Bounds<Pixels>, row: &GraphRow, merge: bool, window: &mut Window) {
    let x = |column: usize| bounds.left() + LANE_WIDTH * (column as f32 + 0.5);
    let middle = bounds.center().y;
    let radius = DOT / 2.;
    let center = point(x(row.column), middle);
    let halves = [
        (&row.top, bounds.top(), middle),
        (&row.bottom, middle, bounds.bottom()),
    ];
    for (half, (lines, from_y, to_y)) in halves.into_iter().enumerate() {
        for line in lines {
            let mut from = point(x(line.from), from_y);
            let mut to = point(x(line.to), to_y);
            if merge && half == 0 && line.to == row.column {
                to = toward(center, from, radius);
            }
            if merge && half == 1 && line.from == row.column {
                from = toward(center, to, radius);
            }
            let mut path = PathBuilder::stroke(LINE);
            path.move_to(from);
            path.line_to(to);
            if let Ok(path) = path.build() {
                window.paint_path(path, lane_color(line.color));
            }
        }
    }
    let color = lane_color(row.color);
    let origin = point(center.x - radius, center.y - radius);
    let dot = Bounds::new(origin, size(DOT, DOT));
    let (fill, border) = if merge {
        (transparent_black(), LINE)
    } else {
        (color, px(0.))
    };
    window.paint_quad(quad(dot, radius, fill, border, color, BorderStyle::Solid));
}

/// The point `by` from `from` toward `to`, or `to` if that's nearer.
fn toward(from: Point<Pixels>, to: Point<Pixels>, by: Pixels) -> Point<Pixels> {
    let (dx, dy) = (f32::from(to.x - from.x), f32::from(to.y - from.y));
    let length = dx.hypot(dy);
    if length <= f32::from(by) {
        return to;
    }
    let scale = f32::from(by) / length;
    point(from.x + px(dx * scale), from.y + px(dy * scale))
}

impl Focusable for GitLogPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for GitLogPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = self.ui;
        let list = uniform_list(
            "git-log-rows",
            self.row_count(),
            cx.processor(|panel, range, window, cx| panel.render_rows(range, window, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1();
        div()
            .track_focus(&self.focus)
            .key_context(commit_rows::key_context(KEY_CONTEXT, cx))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
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
                panel.child(
                    div()
                        .flex_none()
                        .px_3()
                        .py_2()
                        .text_color(ui.muted_text)
                        .child(message),
                )
            })
            .child(list)
    }
}

#[cfg(test)]
impl GitLogPanel {
    /// Each row's ref labels and texts, then the last row's text, if any,
    /// with no labels.
    pub(crate) fn rows(&self) -> Vec<(Vec<String>, Vec<String>)> {
        let commits = self.commits.iter().map(|entry| {
            let labels = entry.refs.iter().map(label_text).collect();
            (labels, commit_rows::texts(&entry.commit, self.now).to_vec())
        });
        let tail = self
            .tail_text()
            .map(|text| (Vec::new(), vec![text.to_owned()]));
        commits.chain(tail).collect()
    }

    /// Each row's text, files and notes under their commits included.
    pub(crate) fn lines(&self) -> Vec<String> {
        (self.lines.iter())
            .map(|line| match *line {
                Line::Commit(ix) => {
                    commit_rows::texts(&self.commits[ix].commit, self.now)[0].clone()
                }
                Line::File(ix, file) => self.file_text(ix, file).unwrap_or_default(),
                Line::Note(ix) => match self.expanded.get(&ix) {
                    Some(Listing::Failed(error)) => error.clone(),
                    _ => LOADING.to_owned(),
                },
            })
            .collect()
    }

    /// Asks for row `ix` to be opened, as Enter does.
    pub(crate) fn open_row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.open(ix, window, cx);
    }

    /// Each commit's id and graph row.
    pub(crate) fn graph(&self) -> Vec<(String, GraphRow)> {
        let ids = self.commits.iter().map(|entry| entry.commit.id.clone());
        ids.zip(self.rows.iter().cloned()).collect()
    }

    /// What shows in place of the list, if anything.
    pub(crate) fn shown_message(&self) -> Option<&'static str> {
        self.message()
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        self.selected
    }
}

#[cfg(test)]
#[path = "git_log_tests.rs"]
mod git_log_tests;

#[cfg(test)]
#[path = "git_log_open_tests.rs"]
mod git_log_open_tests;
