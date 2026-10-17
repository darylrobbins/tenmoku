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
//! list is scrolled (by the wheel, even at its end, or to other rows). Enter, `o` and a click only select:
//! opening a commit is a later card's.

use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_git::{
    Head, LogCommit, LogScope, RefKind, RefLabel, Repo,
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
        panel.update(cx, |panel, cx| panel.list(at, scope, op, cx));
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
    fn list(&mut self, at: PathBuf, scope: LogScope, op: GitOp, cx: &mut Context<Self>) {
        self.read += 1;
        let read = self.read;
        self.scope = scope;
        self.head = None;
        self.not_repository = false;
        self.repo = None;
        self.tips = Arc::from([]);
        self.commits.clear();
        self.rows.clear();
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
                self.selected = (!commits.is_empty()).then_some(0);
                self.append(commits);
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
        self.commits.len() + usize::from(self.tail_text().is_some())
    }

    /// Selects row `ix`, reading the next page if it's near the end.
    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        self.hold = Hold::None;
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        if self.near_end(ix) {
            self.load_more(cx);
        }
        cx.notify();
    }

    /// `G`: selects the last row loaded, reading no further page until the
    /// selection moves or the list scrolls.
    fn select_last(&mut self, cx: &mut Context<Self>) {
        let Some(last) = self.commits.len().checked_sub(1) else {
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
            // Opening a commit is a later card's.
            Key::Open => {}
            Key::Last => self.select_last(cx),
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
        // Held from the rows `G` scrolled to until others are drawn. The
        // list also renders one row to measure it: that's not drawing.
        match &self.hold {
            _ if range.len() <= 1 => {}
            Hold::Scrolling => self.hold = Hold::Drawn(range.clone()),
            Hold::Drawn(drawn) if *drawn != range => self.hold = Hold::None,
            _ => {}
        }
        // Drawn near the end: the next page, once the frame is over.
        let near = range.end > 0 && self.near_end(range.end - 1);
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
                let row = div()
                    .id(ElementId::NamedInteger(name.clone(), ix as u64))
                    .h(ROW_HEIGHT)
                    .w_full()
                    .pl_2()
                    .pr_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .whitespace_nowrap();
                let Some(entry) = self.commits.get(ix) else {
                    let error = matches!(self.tail, Tail::Failed(_));
                    let color = if error { ui.error_text } else { ui.muted_text };
                    let text = self.tail_text()?.to_owned();
                    return Some(row.text_color(color).child(text).into_any_element());
                };
                let selected = self.selected == Some(ix);
                let merge = entry.parents.len() > 1;
                let graph = graph_cell(self.rows[ix].clone(), merge, self.lanes);
                let [summary, when] = commit_rows::texts(&entry.commit, self.now);
                Some(
                    row.when(selected, |row| row.bg(selected_bg))
                        .hover(move |row| row.bg(ui.row_hover))
                        .on_click(cx.listener(move |panel, _, _, cx| panel.select(ix, cx)))
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
