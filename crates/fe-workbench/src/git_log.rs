//! The repository's log with its branch graph, as VS Code's Source Control
//! Graph and fugitive's `:Gclog` show it: `git.log` ("Git: Log", typed as
//! `:Gclog`) lists HEAD's history and `git.log_all` ("Git: Log (All
//! Branches)", `:Gclog --all`) every branch's, remote-tracking branch's and
//! tag's, newest first in topo order ([`Repo::log`]), in the Git Log panel,
//! the Graph view of the right dock's Source Control
//! (`dock::source_control`), which they show expanded and focused. The
//! repository is the active file's, else the root's.
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
//! and, muted, how long ago it was made. Hovering a commit row shows its
//! whole message in a popover, as VS Code's graph and GitLens do: the
//! summary, bold, the body, its author and email, its date as
//! `git.blame_line`'s popover gives it and how long ago that was, and its
//! full id, muted ([`CommitPopover`]). It's no taller than the window: a
//! longer message scrolls under the mouse, the popover staying open while
//! the mouse is over it, its author, date and id kept in view below.
//!
//! The panel takes its keys as [`commit_rows`] says: up and down move the
//! selection; with vim on, `j` and `k` move, `g g` and `G` go to the first
//! and last row loaded and `q` hides the dock, focusing the active editor.
//! `G` reads no further page: none is read until the selection moves or the
//! list is scrolled (by the wheel, even at its end, or to other rows).
//!
//! Enter, `o` and a click on a commit expand it: its message's body, one
//! muted row a line (its blank lines kept), then its files
//! ([`Repo::commit_files`], against its first parent, read on the background
//! executor, a `Loading…` row under it until they arrive and a muted row for
//! an error) are listed under it, indented, as `A`, `M` or `D` and the path
//! from the root, or `R old → new`; doing it again collapses it. Enter, `o`
//! or a click on a file opens its change at that commit as the History panel
//! opens one ([`Workbench::open_commit`]), so the same one is the same tab.
//! The selection moves over commit, body and file rows alike; Enter, `o`
//! and a click on a body row do nothing. Rows too long for the panel end in
//! an ellipsis.

#[cfg(test)]
use std::{cell::RefCell, rc::Rc};
use std::{
    collections::BTreeMap,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_git::{
    ChangeKind, CommitFile, CommitInfo, FileCommit, Head, LogCommit, LogScope, RefKind, RefLabel,
    Repo,
    graph::{GraphLayout, GraphRow},
};
use fe_ui::{ROW_HEIGHT, UiTheme};
use gpui::{
    AnyElement, App, AppContext as _, BorderStyle, Bounds, Context, Div, ElementId, Entity,
    FocusHandle, Focusable, FontWeight, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, PathBuilder, Pixels, Point, Render, ScrollHandle, ScrollStrategy,
    ScrollWheelEvent, SharedString, StatefulInteractiveElement as _, Styled as _,
    UniformListScrollHandle, WeakEntity, Window, canvas, div, point, prelude::FluentBuilder as _,
    px, quad, size, transparent_black, uniform_list,
};

use super::{
    super::git_blame,
    GitCommand, GitOp,
    commit_rows::{self, Key, ListKeys},
};
use crate::{Workbench, dock::DockOp, git};

/// The id the Git Log panel had on the sidebar, before it was Source
/// Control's Graph view, which layouts saved then name.
pub(crate) const OLD_PANEL: &str = "git_log";

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

/// The commit popover's widest.
const POPOVER_WIDTH: Pixels = px(480.);

/// How much shorter than the window the commit popover is, at most.
const POPOVER_MARGIN: Pixels = px(16.);

/// The commit popover's text size.
const POPOVER_TEXT: Pixels = px(12.);

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

/// What a row of the list is: a commit (by its index), a line of its
/// message's body or one of its files (the commit's, then the line's or the
/// file's index), or the row under a commit saying its files are loading or
/// couldn't be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Line {
    Commit(usize),
    Body(usize, usize),
    File(usize, usize),
    Note(usize),
}

impl Line {
    /// An identity for the row's element that holds whatever rows are
    /// listed before it. Body rows' have the top bit set, so they're none
    /// of a commit's, a file's or a note's.
    fn id(self) -> u64 {
        const ROW: u64 = 1 << 24;
        const BODY: u64 = 1 << 63;
        match self {
            Line::Commit(ix) => ix as u64 * ROW,
            Line::Body(ix, row) => BODY | (ix as u64 * ROW + row as u64),
            Line::File(ix, file) => ix as u64 * ROW + file as u64 + 1,
            Line::Note(ix) => ix as u64 * ROW + ROW - 1,
        }
    }

    /// The index of the commit the row is or is under.
    fn commit(self) -> usize {
        match self {
            Line::Commit(ix) | Line::Body(ix, _) | Line::File(ix, _) | Line::Note(ix) => ix,
        }
    }
}

/// The body of `message`: what follows its first line with text, leading
/// blank lines dropped and trailing whitespace trimmed, by line.
fn body_lines(message: &str) -> Vec<&str> {
    let mut lines = message.trim_end().lines();
    lines.by_ref().find(|line| !line.trim().is_empty());
    lines.skip_while(|line| line.trim().is_empty()).collect()
}

/// How long ago `commit` was made, at `now`: `just now`, `6 days ago`.
fn age(commit: &CommitInfo, now: i64) -> String {
    let [_, when] = commit_rows::texts(commit, now);
    let author = format!("{}, ", commit.author);
    when.strip_prefix(&author)
        .map_or(when.clone(), str::to_owned)
}

/// What shows on hovering a commit row: its summary, bold; its message's
/// body, wrapped, its blank lines kept; `<author> <email>`; `<date>
/// (<ago>)`, the date as `git.blame_line`'s popover gives it; and its full
/// id, muted. Wraps at [`POPOVER_WIDTH`]; no taller than the window less
/// [`POPOVER_MARGIN`], the summary and body scrolling above the rest.
struct CommitPopover {
    summary: String,
    body: Option<String>,
    author: String,
    date: String,
    id: String,
    /// The summary and body's scroll.
    scroll: ScrollHandle,
    #[cfg(test)]
    probe: Rc<RefCell<PopoverProbe>>,
}

/// What the tests see of the last commit popover built.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct PopoverProbe {
    /// Its text, as [`CommitPopover::text`] gives it.
    pub(crate) text: Option<String>,
    /// Its summary and body's scroll.
    pub(crate) scroll: Option<ScrollHandle>,
    /// Its parts' bounds as last drawn: the summary and body, then the
    /// author, date and id.
    pub(crate) drawn: Vec<Bounds<Pixels>>,
}

impl CommitPopover {
    fn new(commit: &CommitInfo, now: i64) -> Self {
        let body = body_lines(&commit.message);
        Self {
            summary: commit.summary.clone(),
            body: (!body.is_empty()).then(|| body.join("\n")),
            author: format!("{} {}", commit.author, commit.email),
            date: format!("{} ({})", git_blame::date(commit), age(commit, now)),
            id: commit.id.clone(),
            scroll: ScrollHandle::new(),
            #[cfg(test)]
            probe: Rc::default(),
        }
    }

    /// It, telling `probe` what it is and how it's drawn.
    #[cfg(test)]
    fn probed(mut self, probe: &Rc<RefCell<PopoverProbe>>) -> Self {
        probe.replace(PopoverProbe {
            text: Some(self.text()),
            scroll: Some(self.scroll.clone()),
            drawn: Vec::new(),
        });
        self.probe = probe.clone();
        self
    }

    /// Its lines, joined with `\n`.
    #[cfg(test)]
    fn text(&self) -> String {
        let lines = [Some(&self.summary), self.body.as_ref()]
            .into_iter()
            .flatten();
        let lines = lines.chain([&self.author, &self.date, &self.id]);
        lines.map(String::as_str).collect::<Vec<_>>().join("\n")
    }
}

impl CommitPopover {
    /// Its width: its widest line's, up to [`POPOVER_WIDTH`]. GPUI lays a
    /// tooltip out at its min-content size, so wrapped text needs a width
    /// set to take the height it wraps to.
    fn width(&self, window: &Window) -> Pixels {
        let style = window.text_style();
        let measure = |text: &str, weight: FontWeight| {
            let mut run = style.to_run(text.len());
            run.font.weight = weight;
            let text = SharedString::from(text.to_owned());
            (window
                .text_system()
                .shape_line(text, POPOVER_TEXT, &[run], None))
            .width
        };
        let body = self.body.iter().flat_map(|body| body.lines());
        let rest = body.chain([&self.author, &self.date, &self.id].map(String::as_str));
        let widest = (rest.map(|line| measure(line, FontWeight::NORMAL)))
            .fold(measure(&self.summary, FontWeight::BOLD), Pixels::max);
        // Its padding and border, and a pixel so the widest doesn't wrap.
        (widest + px(19.)).min(POPOVER_WIDTH)
    }
}

impl Render for CommitPopover {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = fe_ui::ui_theme(cx);
        let message = div()
            .id("commit-popover-message")
            .flex()
            .flex_col()
            .gap_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child(self.summary.clone()),
            )
            .when_some(self.body.clone(), |message, body| {
                message.child(div().child(body))
            });
        #[cfg(test)]
        let probe = self.probe.clone();
        let popover = div()
            .w(self.width(window))
            .max_h(window.viewport_size().height - POPOVER_MARGIN)
            .occlude()
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(ui.panel_background)
            .border_1()
            .border_color(ui.border)
            .text_size(POPOVER_TEXT)
            .text_color(ui.text)
            .whitespace_normal()
            .child(message)
            .child(div().flex_none().child(self.author.clone()))
            .child(div().flex_none().child(self.date.clone()))
            .child(
                div()
                    .flex_none()
                    .text_color(ui.muted_text)
                    .child(self.id.clone()),
            );
        #[cfg(test)]
        let popover = popover.on_children_prepainted(move |drawn, _, _| {
            probe.borrow_mut().drawn = drawn;
        });
        popover
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
    /// root's, in the Git Log panel and shows it.
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
        let panel =
            (self.dock.read(cx).git_log().cloned()).expect("the dock has its Git Log panel");
        let root = self.root.clone();
        panel.update(cx, |panel, cx| panel.list(at, root, scope, Some(op), cx));
        self.run_dock_op(DockOp::ShowGitLog, window, cx);
    }

    /// Lists HEAD's log of the active file's repository or else the root's
    /// in `panel`, if nothing has listed it since it was asked to. Without
    /// a command, so outside a repository it says so without a toast.
    fn list_first_git_log(&mut self, panel: &Entity<GitLogPanel>, cx: &mut Context<Self>) {
        if self.restoring {
            // The restored active file isn't in yet: ask again, once the
            // workbench has taken up the restored layout.
            panel.update(cx, |panel, _| panel.first_asked = false);
            return;
        }
        let path = (self.active_editor(cx))
            .and_then(|editor| editor.read(cx).model().path().map(Path::to_owned));
        let at = path.or_else(|| self.root.clone());
        let root = self.root.clone();
        panel.update(cx, |panel, cx| {
            if panel.read != 0 {
                return;
            }
            match at {
                Some(at) => panel.list(at, root, LogScope::Head, None, cx),
                None => {
                    panel.not_repository = true;
                    cx.notify();
                }
            }
        });
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
    /// bodies' lines and their files.
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
    /// Whether the workbench was asked to list the panel the first time it
    /// was drawn.
    first_asked: bool,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    /// What the tests see of the last commit popover built.
    #[cfg(test)]
    popover: Rc<RefCell<PopoverProbe>>,
    /// What the tests see of the rows drawn.
    rows_seen: RowsProbe,
}

impl GitLogPanel {
    pub(crate) fn new(workbench: WeakEntity<Workbench>, cx: &mut Context<Self>) -> Self {
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
            first_asked: false,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            #[cfg(test)]
            popover: Rc::default(),
            rows_seen: RowsProbe::default(),
        }
    }

    /// Graph's header in Source Control: `Graph: <branch>`, `Graph: HEAD
    /// (detached)` or `Graph: All Branches`.
    pub(crate) fn title(&self, _: &App) -> SharedString {
        match (self.scope, &self.head) {
            (LogScope::All, _) => "Graph: All Branches".into(),
            (_, Some(Head::Branch(name) | Head::Unborn(name))) => format!("Graph: {name}").into(),
            (_, Some(Head::Detached(_))) => "Graph: HEAD (detached)".into(),
            (_, None) => "Graph".into(),
        }
    }

    /// Reads the first page of the `scope` log of `at`'s repository on the
    /// background executor, showing it loading until it lands; `op`, if
    /// any, is answered if it's outside a repository.
    fn list(
        &mut self,
        at: PathBuf,
        root: Option<PathBuf>,
        scope: LogScope,
        op: Option<GitOp>,
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
    fn land(&mut self, read: u64, found: Found, op: Option<GitOp>, cx: &mut Context<Self>) {
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
                if let (Some(op), Some(workbench)) = (op, self.workbench.upgrade()) {
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
            let Some(listing) = self.expanded.get(&ix) else {
                continue;
            };
            let body = body_lines(&self.commits[ix].commit.message).len();
            (self.lines).extend((0..body).map(|row| Line::Body(ix, row)));
            match listing {
                Listing::Files(files) => {
                    (self.lines).extend((0..files.len()).map(|file| Line::File(ix, file)));
                }
                _ => self.lines.push(Line::Note(ix)),
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

    /// Line `row` of commit `commit`'s message's body.
    fn body_text(&self, commit: usize, row: usize) -> Option<String> {
        let entry = self.commits.get(commit)?;
        Some(body_lines(&entry.commit.message).get(row)?.to_string())
    }

    /// The text of file `file` of commit `commit`: `M path`, `R old → new`.
    #[cfg(test)]
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

    /// File `file` of commit `commit` as its row shows it: the change's
    /// letter, the file's name, then its directory (a rename's old path
    /// after it) dimmed.
    fn file_parts(&self, commit: usize, file: usize) -> Option<FileParts> {
        let Some(Listing::Files(files)) = self.expanded.get(&commit) else {
            return None;
        };
        let file = files.get(file)?;
        let path = self.display_path(&file.path);
        let (dir, name) = match path.rsplit_once('/') {
            Some((dir, name)) => (dir.to_owned(), name.to_owned()),
            None => (String::new(), path),
        };
        let kind = match (&file.from, file.kind) {
            (Some(_), _) => 'R',
            (None, ChangeKind::Added) => 'A',
            (None, ChangeKind::Deleted) => 'D',
            (None, _) => 'M',
        };
        let dir = match &file.from {
            Some(from) if dir.is_empty() => format!("← {}", self.display_path(from)),
            Some(from) => format!("{dir} ← {}", self.display_path(from)),
            None => dir,
        };
        Some(FileParts { kind, name, dir })
    }

    /// The lanes' columns and colours running through the rows under
    /// commit `commit`'s own: its row's bottom half's lines, ending where
    /// the next row's top half's begin.
    fn rail(&self, commit: usize) -> Vec<(usize, usize)> {
        let Some(row) = self.rows.get(commit) else {
            return Vec::new();
        };
        row.bottom
            .iter()
            .map(|line| (line.to, line.color))
            .collect()
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
    /// opens a file; nothing on a body or note row.
    fn open(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        match self.lines.get(ix).copied() {
            Some(Line::Commit(commit)) => self.toggle(commit, cx),
            Some(Line::File(commit, file)) => self.open_file(commit, file, window, cx),
            Some(Line::Body(..) | Line::Note(_)) | None => {}
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

    /// Asks the workbench to list HEAD's log, once, if the panel is drawn
    /// before anything has listed it: a rail click only shows it.
    fn ask_for_first_list(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.read != 0 || self.first_asked {
            return;
        }
        let Some(workbench) = self.workbench.upgrade() else {
            return;
        };
        self.first_asked = true;
        let panel = cx.entity();
        window.defer(cx, move |_, cx| {
            workbench.update(cx, |workbench, cx| workbench.list_first_git_log(&panel, cx));
        });
    }

    /// Each listed commit's summary, and what shows in place of the list.
    pub(crate) fn listing(&self) -> (Vec<String>, Option<&'static str>) {
        let summaries = (self.commits.iter())
            .map(|entry| commit_rows::texts(&entry.commit, self.now)[0].clone())
            .collect();
        (summaries, self.message())
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
        let ui = fe_ui::ui_theme(cx);
        #[cfg(test)]
        {
            self.rows_seen.0.borrow_mut().muted = ui.muted_text;
        }
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
                let seen = self.rows_seen.clone();
                let rail =
                    |commit: usize| rail_cell(self.rail(commit), self.lanes, ix, seen.clone());
                let row = match line {
                    Line::Commit(_) => row,
                    Line::Body(commit, body) => {
                        let text = self.body_text(commit, body).unwrap_or_default();
                        return Some(
                            row.when(selected, |row| row.bg(selected_bg))
                                .text_color(ui.muted_text)
                                .on_click(cx.listener(move |panel, _, window, cx| {
                                    panel.click(line, false, window, cx)
                                }))
                                .child(rail(commit))
                                .child(ellipsized(text))
                                .into_any_element(),
                        );
                    }
                    Line::File(commit, file) => {
                        let parts = self.file_parts(commit, file)?;
                        return Some(
                            row.when(selected, |row| row.bg(selected_bg))
                                .hover(move |row| row.bg(ui.row_hover))
                                .on_click(cx.listener(move |panel, _, window, cx| {
                                    panel.click(line, true, window, cx)
                                }))
                                .child(rail(commit))
                                .child(file_cell(parts, &ui, ix, seen.clone()))
                                .into_any_element(),
                        );
                    }
                    Line::Note(commit) => {
                        let (text, color) = match self.expanded.get(&commit) {
                            Some(Listing::Failed(error)) => (error.clone(), ui.error_text),
                            _ => (LOADING.to_owned(), ui.muted_text),
                        };
                        return Some(
                            row.when(selected, |row| row.bg(selected_bg))
                                .text_color(color)
                                .on_click(cx.listener(move |panel, _, window, cx| {
                                    panel.click(line, false, window, cx)
                                }))
                                .child(rail(commit))
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
                let graph = graph_cell(
                    self.rows[ix_commit].clone(),
                    merge,
                    self.lanes,
                    ix,
                    seen.clone(),
                );
                let [summary, _] = commit_rows::texts(&entry.commit, self.now);
                let when = age(&entry.commit, self.now);
                let (commit, now) = (entry.commit.clone(), self.now);
                #[cfg(test)]
                let probe = self.popover.clone();
                Some(
                    row.when(selected, |row| row.bg(selected_bg))
                        .hover(move |row| row.bg(ui.row_hover))
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            panel.click(line, true, window, cx)
                        }))
                        .hoverable_tooltip(move |_, cx| {
                            let popover = CommitPopover::new(&commit, now);
                            #[cfg(test)]
                            let popover = popover.probed(&probe);
                            cx.new(|_| popover).into()
                        })
                        .child(graph)
                        .children(entry.refs.iter().map(|label| ref_label(label, &ui)))
                        .child(ellipsized(summary))
                        .child(div().flex_none().text_color(ui.muted_text).child(when))
                        .into_any_element(),
                )
            })
            .collect()
    }
}

/// `text` as a row's, ending in an ellipsis if it's too long for the row.
fn ellipsized(text: String) -> impl IntoElement {
    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .text_ellipsis()
        .child(text)
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
fn ref_label(label: &RefLabel, ui: &UiTheme) -> impl IntoElement {
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

/// A file row's text: the change's letter, the file's name, then its
/// directory dimmed, the name shrinking only after the directory has.
fn file_cell(parts: FileParts, ui: &UiTheme, ix: usize, probe: RowsProbe) -> impl IntoElement {
    let kind = div().flex_none();
    let name = div().min_w_0().overflow_hidden().text_ellipsis();
    let dir = (div().flex_shrink(100.).min_w_0().overflow_hidden())
        .text_ellipsis()
        .text_color(ui.muted_text);
    let mut cells = [
        (parts.kind.to_string(), kind),
        (parts.name, name),
        (parts.dir, dir),
    ];
    probe.cells(
        ix,
        cells.iter_mut().map(|(text, cell)| (text.clone(), cell)),
    );
    let cell = (div().flex_1().min_w_0().flex().gap_2().overflow_hidden())
        .children(cells.into_iter().map(|(text, cell)| cell.child(text)));
    #[cfg(test)]
    let cell = cell.on_children_prepainted(move |drawn, _, _| probe.bounds(ix, &drawn));
    cell
}

/// What the rail tests hand the painter, defined in
/// `git_log_rail_painted_tests.rs` and linked by name: a test of the painted
/// stroke must build against code that doesn't call them yet.
#[cfg(test)]
mod rail_hooks {
    unsafe extern "Rust" {
        /// Pixels every stroke's end is pulled in by.
        #[link_name = "fe_rail_pull_in"]
        pub(super) safe fn pull_in() -> f32;
        /// Row `ix` starts painting again.
        #[link_name = "fe_rail_begin"]
        pub(super) safe fn begin(ix: usize);
        /// A stroke painted in row `ix`: from x, y, to x, y.
        #[link_name = "fe_rail_painted"]
        pub(super) safe fn painted(ix: usize, fx: f32, fy: f32, tx: f32, ty: f32);
    }
}

/// What the tests see of the rows drawn, by the row's index in the list.
#[derive(Clone, Default)]
struct RowsProbe(#[cfg(test)] Rc<RefCell<RowsSeen>>);

#[cfg(test)]
#[derive(Default)]
pub(crate) struct RowsSeen {
    /// The segments painted in a graph cell or a rail, as `(from, to,
    /// colour)`.
    pub(crate) segments: BTreeMap<usize, Vec<Segment>>,
    /// A file row's cells in order: text, text colour set on it and bounds.
    pub(crate) cells: BTreeMap<usize, Vec<CellSeen>>,
    /// The muted text colour the rows were drawn with.
    pub(crate) muted: Hsla,
}

/// A painted segment: from, to and colour.
#[cfg(test)]
pub(crate) type Segment = (Point<Pixels>, Point<Pixels>, Hsla);

#[cfg(test)]
pub(crate) struct CellSeen {
    pub(crate) text: String,
    pub(crate) color: Option<Hsla>,
    pub(crate) bounds: Option<Bounds<Pixels>>,
}

impl RowsProbe {
    /// Forgets the segments row `ix` painted last frame.
    fn begin(&self, _ix: usize) {
        #[cfg(test)]
        {
            self.0.borrow_mut().segments.remove(&_ix);
            rail_hooks::begin(_ix);
        }
    }

    /// Strokes the segment `from`..`to` in row `ix` and notes it: the one
    /// place a lane is painted, so what the tests see is what was drawn.
    fn stroke(
        &self,
        window: &mut Window,
        _ix: usize,
        from: Point<Pixels>,
        to: Point<Pixels>,
        color: Hsla,
    ) {
        #[cfg(test)]
        let to = point(to.x, to.y - px(rail_hooks::pull_in()));
        let mut path = PathBuilder::stroke(LINE);
        path.move_to(from);
        path.line_to(to);
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
            #[cfg(test)]
            {
                let (x, y) = (f32::from(from.x), f32::from(from.y));
                rail_hooks::painted(_ix, x, y, f32::from(to.x), f32::from(to.y));
                self.0
                    .borrow_mut()
                    .segments
                    .entry(_ix)
                    .or_default()
                    .push((from, to, color));
            }
        }
    }

    /// Notes row `ix`'s cells, text and style.
    fn cells<'a>(&self, _ix: usize, _cells: impl Iterator<Item = (String, &'a mut Div)>) {
        #[cfg(test)]
        {
            let cells = _cells.map(|(text, cell)| CellSeen {
                text,
                color: cell.style().text.color,
                bounds: None,
            });
            self.0.borrow_mut().cells.insert(_ix, cells.collect());
        }
    }

    /// Notes row `ix`'s cells' bounds.
    #[cfg(test)]
    fn bounds(&self, ix: usize, drawn: &[Bounds<Pixels>]) {
        if let Some(cells) = self.0.borrow_mut().cells.get_mut(&ix) {
            for (cell, bounds) in cells.iter_mut().zip(drawn) {
                cell.bounds = Some(*bounds);
            }
        }
    }
}

/// A file row's parts: see [`GitLogPanel::file_parts`].
struct FileParts {
    kind: char,
    name: String,
    dir: String,
}

/// The lanes `rail` runs through a row under a commit, [`LANE_WIDTH`] per
/// lane of `lanes`: a vertical line the row's height each.
fn rail_cell(
    rail: Vec<(usize, usize)>,
    lanes: usize,
    ix: usize,
    probe: RowsProbe,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            probe.begin(ix);
            for &(column, color) in &rail {
                let x = bounds.left() + LANE_WIDTH * (column as f32 + 0.5);
                let (top, bottom) = (point(x, bounds.top()), point(x, bounds.bottom()));
                probe.stroke(window, ix, top, bottom, lane_color(color));
            }
        },
    )
    .flex_none()
    .w(LANE_WIDTH * lanes as f32)
    .h(ROW_HEIGHT)
}

/// A commit's graph cell, [`LANE_WIDTH`] per lane of `lanes`: its row's
/// lines and its dot, a ring for a merge.
fn graph_cell(
    row: GraphRow,
    merge: bool,
    lanes: usize,
    ix: usize,
    probe: RowsProbe,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| paint_graph(bounds, &row, merge, window, ix, &probe),
    )
    .flex_none()
    .w(LANE_WIDTH * lanes as f32)
    .h(ROW_HEIGHT)
}

/// Paints `row` in `bounds`: each line of its top half from the top edge
/// to the middle and of its bottom half from the middle to the bottom
/// edge, then the commit's dot. A merge's is a ring the lines meeting it
/// stop at, so the row's background, selected or hovered, shows inside.
fn paint_graph(
    bounds: Bounds<Pixels>,
    row: &GraphRow,
    merge: bool,
    window: &mut Window,
    ix: usize,
    probe: &RowsProbe,
) {
    probe.begin(ix);
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
            probe.stroke(window, ix, from, to, lane_color(line.color));
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = fe_ui::ui_theme(cx);
        self.ask_for_first_list(window, cx);
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
            let [summary, _] = commit_rows::texts(&entry.commit, self.now);
            (labels, vec![summary, age(&entry.commit, self.now)])
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
                Line::Body(ix, row) => self.body_text(ix, row).unwrap_or_default(),
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

    /// The text of the last commit popover built.
    pub(crate) fn last_popover(&self) -> Option<String> {
        self.popover.borrow().text.clone()
    }

    /// The muted text colour the last rows drawn used.
    pub(crate) fn muted_text(&self) -> Hsla {
        self.rows_seen.0.borrow().muted
    }

    /// What the tests see of the rows drawn.
    pub(crate) fn rows_seen(&self) -> Rc<RefCell<RowsSeen>> {
        self.rows_seen.0.clone()
    }

    /// What the tests see of the last commit popover built.
    pub(crate) fn popover_probe(&self) -> Rc<RefCell<PopoverProbe>> {
        self.popover.clone()
    }
}

#[cfg(test)]
#[path = "git_log_tests.rs"]
mod git_log_tests;

#[cfg(test)]
#[path = "git_log_open_tests.rs"]
mod git_log_open_tests;
