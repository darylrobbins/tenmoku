//! The repository's log with its branch graph, as VS Code's Source Control
//! Graph and fugitive's `:Gclog` show it: `git.log` ("Git: Log", typed as
//! `:Gclog`) lists HEAD's history and `git.log_all` ("Git: Log (All
//! Branches)", `:Gclog --all`) every branch's, remote-tracking branch's and
//! tag's, newest first in topo order ([`Repo::log`]), in the Git Log panel,
//! a tool window of the bottom panel (`dock::GIT_LOG_BUTTON`), which they
//! show focused. The repository is the active file's, else the root's.
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
//! `tag`, a remote-tracking branch's muted), then its short id and summary;
//! who made it and when are left to its card. Hovering a commit row shows
//! the card, as GitLens's hover does: an avatar of the author's initials,
//! the author and how long ago and when it was made, its whole message, its
//! co-author and sign-off trailers, its refs, its files changed and lines
//! added and removed, its short id with a button copying the full one, and
//! a link to its page where its remote's host has one ([`CommitCard`]). A
//! click on its files changed opens its change to all its files in one tab
//! ([`commit_diff_view`]), as its menu's Open Changes does.
//! It's no taller than the window: a longer message scrolls under the
//! mouse, the card staying open while the mouse is over it, the rest kept
//! in view around it. Every row with a graph cell or a rail keeps a file
//! row's letter's room at its right edge, so however narrow the list the
//! lanes clip at the same place in every row.
//!
//! The panel takes its keys as [`commit_rows`] says: up and down move the
//! selection; with vim on, `j` and `k` move, `g g` and `G` go to the first
//! and last row loaded and `q` hides the bottom panel, focusing the active
//! editor.
//! `G` reads no further page: none is read until the selection moves or the
//! list is scrolled (by the wheel, even at its end, or to other rows).
//!
//! Enter, `o` and a click on a commit expand it: its files
//! ([`Repo::commit_files`], against its first parent, read on the background
//! executor, a `Loading…` row under it until they arrive and a muted row for
//! an error) are listed under it, indented: each its file-type icon from the
//! installed icon set, its name and, dimmed, its directory from the root (a
//! rename's old path after it), then at the right edge its lines added and
//! removed, muted green and red, once they're read beside the files
//! ([`Repo::commit_stats`]; none for a binary file), and its change's letter,
//! `M` amber, `A` green, `D` red, `R` blue; doing it again collapses it.
//! Enter, `o` or a click on a file opens its change at that commit as the
//! History panel opens one ([`Workbench::open_commit`]), so the same one is
//! the same tab; with the commit's change to all its files open
//! ([`commit_diff_view`]), it shows that tab scrolled to the file instead.
//! Only its file rows are listed: its message, trailers and totals are left
//! to its card. The selection moves over commit and file rows alike. Rows too long for the panel end in
//! an ellipsis. A right click on a commit or file row selects it and opens
//! its menu, whose items are the panel's commands too ([`git_log_menu`]).
//!
//! A box at the top of the panel filters the list, as GitLens's Commit
//! Graph search does: [`FILTER_DELAY`] after the typing stops, the list is
//! read again from the same commits, listing only those [`LogFilter`]
//! matches by message, author or id prefix ([`Repo::log_from_matching`]),
//! paged as the whole list is; `No commits match` shows if none does. Their
//! graph is laid out over the rows listed: a commit's lane runs on to each
//! parent listed, and ends at its dot where none is. Enter in the box reads
//! at once and gives the keys back to the list; Escape, or the box's clear
//! button, empties the box and lists every commit again. With vim on, `/`
//! in the list focuses the box, its text selected. The box is a
//! [`TextInput`] in the `text_input` key context, and the panel drops its
//! `vim` flag while the box has focus, so the list's keys don't take what's
//! typed. Running `git.log` again keeps the filter.

#[cfg(test)]
use std::{cell::RefCell, rc::Rc};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use fe_git::{
    ChangeKind, CommitFile, CommitInfo, CommitStats, FileCommit, Head, LogCommit, LogFilter,
    LogScope, RefKind, RefLabel, Repo,
    graph::{GraphLayout, GraphRow},
};
use fe_gpui::commands::RunCommand;
use fe_ui::{Icon, IconButton, ROW_HEIGHT, TextInput, TextInputEvent, UiTheme, file_icon_ref};
use gpui::{
    AnyElement, App, AppContext as _, BorderStyle, Bounds, ClickEvent, ClipboardItem, Context, Div,
    ElementId, Entity, FocusHandle, Focusable, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, KeyContext, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement as _,
    PathBuilder, Pixels, Point, Render, ScrollHandle, ScrollStrategy, ScrollWheelEvent,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Subscription,
    UniformListScrollHandle, WeakEntity, Window, anchored, canvas, deferred, div, point,
    prelude::FluentBuilder as _, px, quad, rgba, size, svg, transparent_black, uniform_list,
};

use super::{
    super::git_blame,
    GitCommand, GitOp,
    commit_rows::{self, Key, ListKeys},
};
use crate::{
    Workbench,
    dock::DockOp,
    git,
    picker::{self, PaletteOp},
};

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

/// What the panel shows when the filter matches no commit.
const NO_MATCHES: &str = "No commits match";

/// What the filter box shows while it's empty.
const FILTER_PLACEHOLDER: &str = "Filter by message, author or SHA";

/// How long after the last edit of the filter box the list is read again.
pub(crate) const FILTER_DELAY: Duration = Duration::from_millis(250);

/// The filter box's clear button's id.
const FILTER_CLEAR: &str = "git-log-filter-clear";

/// What the last row says while a page is read.
const LOADING: &str = "Loading…";

/// The commit card's widest.
const CARD_WIDTH: Pixels = px(480.);

/// The commit card's narrowest, so its id, copy button and link fit.
const CARD_MIN_WIDTH: Pixels = px(280.);

/// How much shorter than the window the commit card is, at most.
const CARD_MARGIN: Pixels = px(16.);

/// The commit card's text size.
const CARD_TEXT: Pixels = px(12.);

/// The commit card's avatar's diameter.
const AVATAR: Pixels = px(28.);

/// How long the commit card's copy button shows a check after copying.
const COPIED_FOR: Duration = Duration::from_millis(1500);

/// The width of a file row's change letter, which every row with a graph
/// cell or a rail keeps clear at its right edge.
const LETTER_WIDTH: Pixels = px(12.);

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

/// The body of `message`: what follows its first line with text, leading
/// blank lines dropped and trailing whitespace trimmed, by line.
fn body_lines(message: &str) -> Vec<&str> {
    let mut lines = message.trim_end().lines();
    lines.by_ref().find(|line| !line.trim().is_empty());
    lines.skip_while(|line| line.trim().is_empty()).collect()
}

/// The trailers a commit card lists, by key in any case.
const TRAILERS: [&str; 2] = ["Co-authored-by", "Signed-off-by"];

/// A [`TRAILERS`] line of a message: its key as written and who it names,
/// their email dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Trailer {
    key: String,
    name: String,
}

/// `line` as a [`TRAILERS`] trailer, if it is one.
fn trailer(line: &str) -> Option<Trailer> {
    let (key, value) = line.split_once(':')?;
    if !TRAILERS.iter().any(|known| known.eq_ignore_ascii_case(key)) {
        return None;
    }
    let value = value.trim();
    let name = value.split_once('<').map_or(value, |(name, _)| name).trim();
    let name = if name.is_empty() { value } else { name };
    (!name.is_empty()).then(|| Trailer {
        key: key.to_owned(),
        name: name.to_owned(),
    })
}

/// `message`'s body as [`body_lines`] gives it, less the [`TRAILERS`] lines
/// of its last paragraph, and those trailers in order.
fn split_trailers(message: &str) -> (Vec<&str>, Vec<Trailer>) {
    let mut body = body_lines(message);
    let last = (body.iter().rposition(|line| line.trim().is_empty())).map_or(0, |ix| ix + 1);
    let mut trailers = Vec::new();
    let paragraph: Vec<&str> = body.drain(last..).collect();
    for line in paragraph {
        match trailer(line) {
            Some(trailer) => trailers.push(trailer),
            None => body.push(line),
        }
    }
    while body.last().is_some_and(|line| line.trim().is_empty()) {
        body.pop();
    }
    (body, trailers)
}

/// The initials of `author`: the first letters of their first and last
/// names, upper case; `?` for a name with no letters.
fn initials(author: &str) -> String {
    let mut words =
        (author.split_whitespace()).filter_map(|word| word.chars().find(|c| c.is_alphanumeric()));
    let first = words.next();
    let last = words.next_back();
    let initials: String = [first, last]
        .into_iter()
        .flatten()
        .flat_map(char::to_uppercase)
        .collect();
    if initials.is_empty() {
        "?".to_owned()
    } else {
        initials
    }
}

/// The avatar's colour for `email`: a hue hashed from it (FNV-1a, so the
/// same in every run), any case and surrounding space alike.
fn avatar_color(email: &str) -> Hsla {
    let email = email.trim().to_lowercase();
    let hash = (email.bytes()).fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    gpui::hsla((hash % 360) as f32 / 360., 0.5, 0.4, 1.)
}

/// A commit's totals as `git show --shortstat` words them: `N files
/// changed`, `X insertions(+)` and `Y deletions(-)`, each singular for 1.
fn totals_text(files: usize, added: u32, removed: u32) -> [String; 3] {
    let plural = |count: u64| if count == 1 { "" } else { "s" };
    [
        format!("{files} file{} changed", plural(files as u64)),
        format!("{added} insertion{}(+)", plural(added.into())),
        format!("{removed} deletion{}(-)", plural(removed.into())),
    ]
}

/// What a commit card's web link says for `url`: the host's name where
/// it's known.
fn link_label(url: &str) -> &'static str {
    if url.starts_with("https://github.com/") {
        "Open on GitHub"
    } else {
        "Open in Browser"
    }
}

/// What shows on hovering a commit row, as GitLens's hover does: an avatar,
/// a circle of the author's initials in a colour from their email (nothing
/// is fetched); the author, bold, and `<ago> (<date>)`, the date as
/// `git.blame_line`'s popover gives it; the summary, bold, and the body,
/// wrapped, its blank lines kept, less its [`TRAILERS`], which follow as
/// rows naming who they credit; the refs pointing at it, as the row's
/// labels; its files changed and lines added and removed, green and red,
/// once read ([`Repo::commit_stats`]); then its short id, a button copying
/// its full id (a check for [`COPIED_FOR`] after) and, where its remote's
/// host has commit pages ([`Repo::commit_web_url`]), a link opening its
/// page. Wraps at [`CARD_WIDTH`]; no taller than the window less
/// [`CARD_MARGIN`], the summary, body, trailers and refs scrolling between
/// the header and the totals and footer, which stay in view.
struct CommitCard {
    commit: Arc<CommitInfo>,
    /// The commit and where it's read from, which its files-changed count
    /// opens in one tab in `workbench`.
    entry: LogCommit,
    repo: Option<Arc<Repo>>,
    workbench: WeakEntity<Workbench>,
    refs: Vec<RefLabel>,
    /// The lane colour index of its commit's row, which its ref pills take.
    lane: usize,
    initials: String,
    avatar: Hsla,
    /// `<ago> (<date>)`.
    dates: String,
    body: Option<String>,
    trailers: Vec<Trailer>,
    /// Files changed and lines added and removed, once read.
    totals: Option<(usize, u32, u32)>,
    /// The commit's web page, once found, if its remote has one.
    web: Option<String>,
    /// The copy the check shows for, while it does.
    copied: Option<u64>,
    /// How many copies were made.
    copies: u64,
    /// The scroll of the summary, body, trailers and refs.
    scroll: ScrollHandle,
    #[cfg(test)]
    probe: Rc<RefCell<CardProbe>>,
}

/// What the tests see of the last commit card built, as last drawn.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct CardProbe {
    /// How many cards were built.
    pub(crate) built: usize,
    pub(crate) initials: String,
    pub(crate) avatar: Option<Hsla>,
    pub(crate) author: String,
    pub(crate) dates: String,
    /// The summary and body, joined with `\n`.
    pub(crate) message: String,
    /// Each trailer's key and who it names.
    pub(crate) trailers: Vec<(String, String)>,
    pub(crate) refs: Vec<String>,
    pub(crate) short: String,
    /// The totals' parts, each with the text colour set on it, if any.
    pub(crate) totals: Vec<(String, Option<Hsla>)>,
    /// The copy button's icon.
    pub(crate) copy_icon: Option<Icon>,
    /// The web link's text and page.
    pub(crate) link: Option<(String, String)>,
    /// The scroll of its summary, body, trailers and refs.
    pub(crate) scroll: Option<ScrollHandle>,
    /// Its parts' bounds as last drawn, by name: the header, what scrolls,
    /// the totals and the footer.
    pub(crate) drawn: Vec<(&'static str, Bounds<Pixels>)>,
    /// The bounds of the sections that scroll, as last drawn, by name.
    pub(crate) scrolled: Vec<(&'static str, Bounds<Pixels>)>,
}

impl CommitCard {
    /// `entry`'s card at `now`, reading its totals and web page from `repo`
    /// on the background executor.
    fn new(
        entry: &LogCommit,
        lane: usize,
        now: i64,
        repo: Option<Arc<Repo>>,
        workbench: WeakEntity<Workbench>,
        cx: &mut Context<Self>,
    ) -> Self {
        let commit = entry.commit.clone();
        let (body, trailers) = split_trailers(&commit.message);
        if let Some(repo) = repo.clone() {
            let (id, at) = (commit.id.clone(), repo.clone());
            let web = cx.background_spawn(async move { at.commit_web_url(&id) });
            let id = commit.id.clone();
            let totals = cx.background_spawn(async move {
                let stats = repo.commit_stats(&id).ok()?;
                let (added, removed) = stats.total();
                Some((stats.files.len(), added, removed))
            });
            cx.spawn(async move |card, cx| {
                let web = web.await;
                card.update(cx, |card, cx| {
                    card.web = web;
                    cx.notify();
                })
            })
            .detach();
            cx.spawn(async move |card, cx| {
                let totals = totals.await;
                card.update(cx, |card, cx| {
                    card.totals = totals;
                    cx.notify();
                })
            })
            .detach();
        }
        Self {
            entry: entry.clone(),
            repo,
            workbench,
            refs: entry.refs.clone(),
            lane,
            initials: initials(&commit.author),
            avatar: avatar_color(&commit.email),
            dates: format!(
                "{} ({})",
                commit_rows::ago(now - commit.time),
                git_blame::date(&commit)
            ),
            body: (!body.is_empty()).then(|| body.join("\n")),
            trailers,
            totals: None,
            web: None,
            copied: None,
            copies: 0,
            scroll: ScrollHandle::new(),
            commit,
            #[cfg(test)]
            probe: Rc::default(),
        }
    }

    /// It, telling `probe` what it is and how it's drawn.
    #[cfg(test)]
    fn probed(mut self, probe: &Rc<RefCell<CardProbe>>) -> Self {
        let built = probe.borrow().built + 1;
        probe.replace(CardProbe {
            built,
            ..CardProbe::default()
        });
        self.probe = probe.clone();
        self
    }

    /// Puts the full id on the clipboard, the button showing a check for
    /// [`COPIED_FOR`].
    fn copy(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.commit.id.clone()));
        self.copies += 1;
        let copy = self.copies;
        self.copied = Some(copy);
        cx.notify();
        cx.spawn(async move |card, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            card.update(cx, |card, cx| {
                if card.copied == Some(copy) {
                    card.copied = None;
                    cx.notify();
                }
            })
        })
        .detach();
    }

    /// The lines of text it shows, as one string each, for its width.
    fn lines(&self) -> Vec<(String, FontWeight)> {
        let normal = |text: String| (text, FontWeight::NORMAL);
        let mut lines = vec![
            (self.commit.author.clone(), FontWeight::BOLD),
            normal(self.dates.clone()),
            (self.commit.summary.clone(), FontWeight::BOLD),
        ];
        let body = self.body.iter().flat_map(|body| body.lines());
        lines.extend(body.map(|line| normal(line.to_owned())));
        let trailers = self.trailers.iter();
        lines.extend(trailers.map(|trailer| normal(format!("{}: {}", trailer.key, trailer.name))));
        if let Some((files, added, removed)) = self.totals {
            lines.push(normal(totals_text(files, added, removed).join(", ")));
        }
        lines
    }

    /// Its width: its widest line's, the avatar's beside the author's, at
    /// least [`CARD_MIN_WIDTH`] and up to [`CARD_WIDTH`]. GPUI lays a
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
                .shape_line(text, CARD_TEXT, &[run], None))
            .width
        };
        let lines = self.lines();
        let widest = (lines.iter().enumerate())
            .map(|(ix, (line, weight))| {
                // The author and dates sit beside the avatar.
                let beside = if ix < 2 { AVATAR + px(8.) } else { px(0.) };
                measure(line, *weight) + beside
            })
            .fold(px(0.), Pixels::max);
        // Its padding and border, and a pixel so the widest doesn't wrap.
        (widest + px(19.)).clamp(CARD_MIN_WIDTH, CARD_WIDTH)
    }

    /// Tells the probe what it shows.
    #[cfg(test)]
    fn note(&self, colors: &FileColors) {
        let mut probe = self.probe.borrow_mut();
        probe.initials = self.initials.clone();
        probe.avatar = Some(self.avatar);
        probe.author = self.commit.author.clone();
        probe.dates = self.dates.clone();
        let message = [Some(&self.commit.summary), self.body.as_ref()];
        let message = message.into_iter().flatten().map(String::as_str);
        probe.message = message.collect::<Vec<_>>().join("\n");
        probe.trailers = (self.trailers.iter())
            .map(|trailer| (trailer.key.clone(), trailer.name.clone()))
            .collect();
        probe.refs = self.refs.iter().map(label_text).collect();
        probe.short = self.commit.short.clone();
        probe.totals = match self.totals {
            Some((files, added, removed)) => {
                let [files, added, removed] = totals_text(files, added, removed);
                vec![
                    (files, None),
                    (added, Some(colors.added)),
                    (removed, Some(colors.deleted)),
                ]
            }
            None => Vec::new(),
        };
        probe.copy_icon = Some(self.copy_icon());
        probe.link = (self.web.as_ref()).map(|url| (link_label(url).to_owned(), url.clone()));
        probe.scroll = Some(self.scroll.clone());
    }

    /// The copy button's icon: a check just after copying.
    fn copy_icon(&self) -> Icon {
        if self.copied.is_some() {
            Icon::Check
        } else {
            Icon::Copy
        }
    }
}

impl Render for CommitCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = fe_ui::ui_theme(cx);
        let colors = FileColors::new(&ui, cx);
        #[cfg(test)]
        self.note(&colors);
        let avatar = (div()
            .flex_none()
            .size(AVATAR)
            .rounded_full()
            .bg(self.avatar))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.))
        .font_weight(FontWeight::BOLD)
        .text_color(gpui::white())
        .child(self.initials.clone());
        let who = (div().flex_1().min_w_0().flex().flex_col())
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child(self.commit.author.clone()),
            )
            .child(div().text_color(ui.muted_text).child(self.dates.clone()));
        let header = (div().flex_none().flex().items_center().gap_2())
            .child(avatar)
            .child(who);
        // The message, trailers and refs scroll together between the header
        // and the totals and footer, which stay in view however many there
        // are of each.
        let mut sections: Vec<(&'static str, AnyElement)> = vec![(
            "summary",
            div()
                .font_weight(FontWeight::BOLD)
                .child(self.commit.summary.clone())
                .into_any_element(),
        )];
        if let Some(body) = self.body.clone() {
            sections.push(("body", div().child(body).into_any_element()));
        }
        if !self.trailers.is_empty() {
            let rows = self.trailers.iter().map(|trailer| {
                (div().flex().gap_1())
                    .child(
                        div()
                            .flex_none()
                            .text_color(ui.muted_text)
                            .child(format!("{}:", trailer.key)),
                    )
                    .child(div().min_w_0().child(trailer.name.clone()))
            });
            let trailers = div().flex().flex_col().children(rows);
            sections.push(("trailers", trailers.into_any_element()));
        }
        if !self.refs.is_empty() {
            let refs = (div().flex().flex_wrap().gap_1()).children(
                self.refs
                    .iter()
                    .map(|label| ref_label(label, self.lane, &ui)),
            );
            sections.push(("refs", refs.into_any_element()));
        }
        #[cfg(test)]
        let (inner_probe, inner) = (
            self.probe.clone(),
            sections.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        );
        let content = (div().flex().flex_col().gap_2())
            .children(sections.into_iter().map(|(_, section)| section));
        #[cfg(test)]
        let content = content.on_children_prepainted(move |drawn, _, _| {
            inner_probe.borrow_mut().scrolled = inner.iter().copied().zip(drawn).collect();
        });
        let scrolled = div()
            .id("commit-card-message")
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(content);
        let mut parts: Vec<(&'static str, AnyElement)> = vec![
            ("header", header.into_any_element()),
            ("scrolled", scrolled.into_any_element()),
        ];
        if let Some((files, added, removed)) = self.totals {
            let [files, added, removed] = totals_text(files, added, removed);
            let comma = |part: AnyElement| div().flex().child(part).child(",");
            // The count opens the commit's change to all its files.
            let files = match self.repo.clone() {
                Some(repo) => {
                    let (workbench, entry) = (self.workbench.clone(), self.entry.clone());
                    div()
                        .id("commit-card-files")
                        .debug_selector(|| "commit-card-files".into())
                        .cursor_pointer()
                        .hover(|link| link.underline())
                        .child(files)
                        .on_click(move |_, window, cx| {
                            let (workbench, repo) = (workbench.clone(), repo.clone());
                            open_commit_diff(workbench, repo, entry.clone(), window, cx);
                        })
                        .into_any_element()
                }
                None => div().child(files).into_any_element(),
            };
            let totals = (div().flex_none().flex().flex_wrap().gap_1())
                .child(comma(files))
                .child(comma(
                    div()
                        .text_color(colors.added)
                        .child(added)
                        .into_any_element(),
                ))
                .child(div().text_color(colors.deleted).child(removed));
            parts.push(("totals", totals.into_any_element()));
        }
        let copy = IconButton::new("commit-card-copy", self.copy_icon(), &ui)
            .name("Copy Commit Hash")
            .on_click(cx.listener(|card, _, _, cx| card.copy(cx)));
        let footer = (div().flex_none().flex().items_center().gap_1())
            .child(
                div()
                    .text_color(ui.muted_text)
                    .child(self.commit.short.clone()),
            )
            .child(copy)
            .child(div().flex_1())
            .when_some(self.web.clone(), |footer, url| {
                footer.child(
                    div()
                        .id("commit-card-web")
                        .debug_selector(|| "commit-card-web".into())
                        .flex_none()
                        .cursor_pointer()
                        .text_color(ui.accent)
                        .hover(|link| link.underline())
                        .child(link_label(&url))
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                )
            });
        parts.push(("footer", footer.into_any_element()));
        #[cfg(test)]
        let (probe, names) = (
            self.probe.clone(),
            parts.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        );
        let card = div()
            .w(self.width(window))
            .max_h(window.viewport_size().height - CARD_MARGIN)
            .occlude()
            .flex()
            .flex_col()
            .gap_2()
            .px_2()
            .py_2()
            .rounded_md()
            .bg(ui.panel_background)
            .border_1()
            .border_color(ui.border)
            .text_size(CARD_TEXT)
            .text_color(ui.text)
            .whitespace_normal()
            .children(parts.into_iter().map(|(_, part)| part));
        #[cfg(test)]
        let card = card.on_children_prepainted(move |drawn, _, _| {
            probe.borrow_mut().drawn = names.iter().copied().zip(drawn).collect();
        });
        card
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

/// An expanded commit's files' line counts, read beside its files.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Counts {
    /// Being read by the expansion request with this number.
    Loading(u64),
    /// Lines added and removed by path; a binary file has none, nor does
    /// any file if they couldn't be read.
    Read(HashMap<PathBuf, (u32, u32)>),
}

/// What reading a log's first page found: its repository, where HEAD is,
/// the commits the log starts from and the first page; none outside a
/// repository; or the error's message.
type Found = Result<Option<(Arc<Repo>, Head, Arc<[String]>, Vec<LogCommit>)>, String>;

/// The repository of `at`, a file or folder, where its HEAD is, the
/// commits its `scope` log starts from now and the first page of the log's
/// commits `filter` matches, or none outside a repository. Runs on the
/// background executor.
fn read_first(at: &Path, scope: LogScope, filter: &LogFilter) -> Found {
    let real = git::resolve(at);
    let Some(folder) = real.ancestors().find(|folder| folder.is_dir()) else {
        return Ok(None);
    };
    let Some(repo) = Repo::discover(folder).map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    let head = repo.current_branch().map_err(|error| error.to_string())?;
    let tips = repo.log_tips(scope).map_err(|error| error.to_string())?;
    let commits =
        (repo.log_from_matching(&tips, filter, 0, PAGE)).map_err(|error| error.to_string())?;
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

    /// The Git Log panel, for the UI audit's captures
    /// ([`GitLogPanel::show_card`]).
    #[doc(hidden)]
    pub fn git_log_panel(&self, cx: &App) -> Option<Entity<GitLogPanel>> {
        self.dock.read(cx).git_log().cloned()
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
pub struct GitLogPanel {
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
    /// The expanded commits' files' line counts, by commit index.
    counts: BTreeMap<usize, Counts>,
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
    /// A row's open menu (`git_log_menu`).
    menu: Option<git_log_menu::RowMenu>,
    keys: ListKeys,
    /// The last list started's number: only its pages land.
    read: u64,
    /// What the list was last asked for, a file or folder in its
    /// repository, once it has been.
    at: Option<PathBuf>,
    /// The filter the list is read with.
    filter: LogFilter,
    /// The filter box.
    filter_input: Entity<TextInput>,
    /// Moves on with each edit of the filter box: only the last edit's
    /// delayed read happens.
    filter_edits: u64,
    _filter_input: Subscription,
    /// Whether the workbench was asked to list the panel the first time it
    /// was drawn.
    first_asked: bool,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    /// What the tests see of the last commit card built.
    #[cfg(test)]
    card: Rc<RefCell<CardProbe>>,
    /// What the tests see of the rows drawn.
    rows_seen: RowsProbe,
    /// The commit whose card shows beside its row with no mouse over it
    /// ([`GitLogPanel::show_card`]), and the card once its row has drawn.
    shown_card: Option<(usize, Option<Entity<CommitCard>>)>,
}

impl GitLogPanel {
    pub(crate) fn new(workbench: WeakEntity<Workbench>, cx: &mut Context<Self>) -> Self {
        let ui = fe_ui::ui_theme(cx);
        let filter_input = cx.new(|cx| TextInput::new(FILTER_PLACEHOLDER, &ui, cx).bordered());
        let filter_subscription = cx.subscribe(&filter_input, |panel, _, event, cx| {
            if *event == TextInputEvent::Changed {
                panel.filter_edited(cx);
            }
        });
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
            counts: BTreeMap::new(),
            requests: 0,
            lines: Vec::new(),
            root: None,
            selected: None,
            hold: Hold::None,
            menu: None,
            keys: ListKeys::default(),
            read: 0,
            at: None,
            filter: LogFilter::default(),
            filter_input,
            filter_edits: 0,
            _filter_input: filter_subscription,
            first_asked: false,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            #[cfg(test)]
            card: Rc::default(),
            rows_seen: RowsProbe::default(),
            shown_card: None,
        }
    }

    /// Shows commit `ix`'s card (from 0, newest first) as hovering its row
    /// shows it, at the row's start, until the list is read or filtered
    /// again: for the UI audit's captures, which have no mouse. A commit not
    /// listed yet shows it once it's listed.
    #[doc(hidden)]
    pub fn show_card(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.shown_card = Some((ix, None));
        cx.notify();
    }

    /// Its tab in the bottom panel: `Graph: <branch>`, `Graph: HEAD
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
        self.root = root;
        self.at = Some(at.clone());
        self.clear_rows();
        let filter = self.filter.clone();
        let found = cx.background_spawn(async move { read_first(&at, scope, &filter) });
        cx.spawn(async move |panel, cx| {
            let found = found.await;
            (panel.update(cx, |panel, cx| panel.land(read, found, op, cx))).ok();
        })
        .detach();
        cx.notify();
    }

    /// Forgets the commits listed, their rows, the selection and any card
    /// shown by [`GitLogPanel::show_card`], showing the list loading.
    fn clear_rows(&mut self) {
        self.commits.clear();
        self.rows.clear();
        self.expanded.clear();
        self.counts.clear();
        self.lines.clear();
        self.layout = GraphLayout::new();
        self.lanes = 0;
        self.tail = Tail::Loading;
        self.more = false;
        self.selected = None;
        self.hold = Hold::None;
        self.menu = None;
        self.shown_card = None;
    }

    /// An edit of the filter box: the list is read again [`FILTER_DELAY`]
    /// after the last.
    fn filter_edited(&mut self, cx: &mut Context<Self>) {
        self.filter_edits += 1;
        let edits = self.filter_edits;
        cx.spawn(async move |panel, cx| {
            cx.background_executor().timer(FILTER_DELAY).await;
            panel.update(cx, |panel, cx| {
                if panel.filter_edits == edits {
                    panel.apply_filter(cx);
                }
            })
        })
        .detach();
        cx.notify();
    }

    /// Reads the list again with the filter box's text as its filter, now,
    /// if that's not the filter it was read with. A delayed read waiting
    /// for the box is dropped.
    fn apply_filter(&mut self, cx: &mut Context<Self>) {
        self.filter_edits += 1;
        let filter = LogFilter::new(self.filter_input.read(cx).text().trim());
        if filter == self.filter {
            return;
        }
        self.filter = filter;
        self.refilter(cx);
    }

    /// Reads the first page of the list again with its filter, from the
    /// commits it was read from, or, if it wasn't read, from what it was
    /// asked for.
    fn refilter(&mut self, cx: &mut Context<Self>) {
        let (Some(repo), Some(head)) = (self.repo.clone(), self.head.clone()) else {
            if let Some(at) = self.at.clone() {
                self.list(at, self.root.clone(), self.scope, None, cx);
            }
            return;
        };
        self.read += 1;
        let read = self.read;
        self.clear_rows();
        let (tips, filter) = (self.tips.clone(), self.filter.clone());
        let found = cx.background_spawn(async move {
            let commits = (repo.log_from_matching(&tips, &filter, 0, PAGE))
                .map_err(|error| error.to_string())?;
            Ok(Some((repo, head, tips, commits)))
        });
        cx.spawn(async move |panel, cx| {
            let found = found.await;
            (panel.update(cx, |panel, cx| panel.land(read, found, None, cx))).ok();
        })
        .detach();
        cx.notify();
    }

    /// Empties the filter box and lists every commit again, at once.
    fn clear_filter(&mut self, cx: &mut Context<Self>) {
        self.filter_input.update(cx, |input, cx| input.clear(cx));
        self.apply_filter(cx);
        cx.notify();
    }

    /// `/`: focuses the filter box, its text selected.
    fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_input
            .update(cx, |input, cx| input.select_all(cx));
        window.focus(&self.filter_input.focus_handle(cx), cx);
        cx.notify();
    }

    /// Whether the filter box has focus.
    fn filter_focused(&self, window: &Window, cx: &App) -> bool {
        self.filter_input.focus_handle(cx).is_focused(window)
    }

    /// Enter in the filter box reads the list at once and focuses it;
    /// Escape empties the box, lists every commit and focuses the list.
    fn on_filter_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        match keystroke.key.as_str() {
            "enter" => self.apply_filter(cx),
            "escape" => self.clear_filter(cx),
            _ => return,
        }
        cx.stop_propagation();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The filter box's editing commands; others go on.
    fn on_filter_command(&mut self, run: &RunCommand, _: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = picker::op(run, cx).and_then(PaletteOp::edit) else {
            return cx.propagate();
        };
        self.filter_input.update(cx, edit);
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
    /// Filtered, the whole graph is laid out again over the rows listed, a
    /// commit's lanes running to its parents listed alone: one in a later
    /// page joins up once it's read.
    fn append(&mut self, page: Vec<LogCommit>) {
        self.more = page.len() >= PAGE;
        let from = if self.filter.is_empty() {
            self.commits.len()
        } else {
            self.rows.clear();
            self.layout = GraphLayout::new();
            self.lanes = 0;
            0
        };
        self.commits.extend(page);
        let listed: Option<HashSet<&str>> = (!self.filter.is_empty()).then(|| {
            (self.commits.iter())
                .map(|entry| entry.commit.id.as_str())
                .collect()
        });
        for entry in &self.commits[from..] {
            let row = match &listed {
                Some(listed) => {
                    let parents: Vec<String> = (entry.parents.iter())
                        .filter(|parent| listed.contains(parent.as_str()))
                        .cloned()
                        .collect();
                    self.layout.push(&entry.commit.id, &parents)
                }
                None => self.layout.push(&entry.commit.id, &entry.parents),
            };
            self.lanes = self.lanes.max(lanes(&row));
            self.rows.push(row);
        }
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

    /// File `file` of commit `commit` as its row shows it: the file's name,
    /// its directory (a rename's old path after it), its line counts once
    /// they're read and the change's letter.
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
        let counts = match self.counts.get(&commit) {
            Some(Counts::Read(counts)) => counts.get(&file.path).copied(),
            _ => None,
        };
        Some(FileParts {
            kind,
            name,
            dir,
            counts,
        })
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

    /// Expands commit `ix`, reading its files and their line counts on the
    /// background executor, or collapses it.
    fn toggle(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.expanded.remove(&ix).is_some() {
            self.counts.remove(&ix);
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
        self.counts.insert(ix, Counts::Loading(request));
        self.relayout();
        let stats = cx.background_spawn({
            let (repo, id) = (repo.clone(), id.clone());
            async move { repo.commit_stats(&id).ok() }
        });
        cx.spawn(async move |panel, cx| {
            let stats = stats.await;
            (panel.update(cx, |panel, cx| {
                panel.land_counts(read, (ix, request), stats, cx)
            }))
            .ok();
        })
        .detach();
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

    /// Notes the line counts read for commit `ix`, if it's still expanded by
    /// the request they were read for in the list they were read for.
    fn land_counts(
        &mut self,
        read: u64,
        (ix, request): (usize, u64),
        stats: Option<CommitStats>,
        cx: &mut Context<Self>,
    ) {
        if read != self.read || self.counts.get(&ix) != Some(&Counts::Loading(request)) {
            return;
        }
        let counts = (stats.into_iter().flat_map(|stats| stats.files))
            .filter_map(|file| Some((file.path, (file.added?, file.removed?))))
            .collect();
        self.counts.insert(ix, Counts::Read(counts));
        cx.notify();
    }

    /// Opens commit `commit`'s change to all its files in one tab
    /// ([`commit_diff_view`]), or shows the one open.
    pub(crate) fn open_commit_diff(&mut self, commit: usize, window: &mut Window, cx: &mut App) {
        let (Some(repo), Some(entry)) = (self.repo.clone(), self.commits.get(commit).cloned())
        else {
            return;
        };
        open_commit_diff(self.workbench.clone(), repo, entry, window, cx);
    }

    /// Shows the open view of commit `commit`'s change to all its files
    /// scrolled to file `file`'s section: whether one is open.
    fn reveal_file(
        &mut self,
        commit: usize,
        file: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let (Some(entry), Some(Listing::Files(files))) =
            (self.commits.get(commit), self.expanded.get(&commit))
        else {
            return false;
        };
        let (Some(file), Some(workbench), Some(repo)) = (
            files.get(file),
            self.workbench.upgrade(),
            self.repo.as_ref(),
        ) else {
            return false;
        };
        let (id, workdir) = (entry.commit.id.clone(), repo.workdir().to_owned());
        if (workbench.read(cx).commit_diff_tab(&id, &workdir, cx)).is_none() {
            return false;
        }
        let path = file.path.clone();
        // Once this update is over: the workbench moves focus.
        window.defer(cx, move |window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.reveal_in_commit_diff((&id, &workdir), &path, window, cx)
            });
        });
        true
    }

    /// Opens file `file` of commit `commit`'s change: in the pane's preview
    /// tab if `preview`, else to stay.
    fn open_file(
        &mut self,
        commit: usize,
        file: usize,
        preview: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
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
                workbench.open_commit(repo, commit, preview, window, cx)
            };
            workbench.update(cx, open).ok();
        });
    }

    /// The `clicks`th click of a run on `line`, wherever its row is by now:
    /// selects it. A first click expands or collapses a commit and opens a
    /// file in the preview tab; a second keeps that tab.
    fn click(&mut self, line: Line, clicks: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.lines.iter().position(|other| *other == line) else {
            return;
        };
        self.select(ix, cx);
        match line {
            Line::Commit(commit) if clicks == 1 => self.toggle(commit, cx),
            Line::File(commit, file) if self.reveal_file(commit, file, window, cx) => {}
            Line::File(commit, file) => self.open_file(commit, file, clicks == 1, window, cx),
            _ => {}
        }
    }

    /// Enter, `o` or a click on row `ix`: expands or collapses a commit,
    /// opens a file; nothing on a note row.
    fn open(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        match self.lines.get(ix).copied() {
            Some(Line::Commit(commit)) => self.toggle(commit, cx),
            Some(Line::File(commit, file)) if self.reveal_file(commit, file, window, cx) => {}
            Some(Line::File(commit, file)) => self.open_file(commit, file, false, window, cx),
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
        let filter = self.filter.clone();
        let page = cx.background_spawn(async move {
            (repo.log_from_matching(&tips, &filter, skip, PAGE)).map_err(|error| error.to_string())
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
        } else if self.commits.is_empty() && self.tail == Tail::None && !self.filter.is_empty() {
            Some(NO_MATCHES)
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
        if self.filter_focused(window, cx) {
            return;
        }
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" && !keystroke.modifiers.modified() && self.menu.is_some() {
            cx.stop_propagation();
            return self.close_menu(cx);
        }
        if keystroke.key == "/" && !keystroke.modifiers.modified() && fe_gpui::vim_enabled(cx) {
            cx.stop_propagation();
            return self.focus_filter(window, cx);
        }
        let Some(key) = self.keys.key(event, cx) else {
            return;
        };
        match key {
            // Hides the bottom panel it's in, as `q` hides the sidebar
            // from the History panel.
            Key::Quit => {
                let workbench = self.workbench.clone();
                window.defer(cx, move |window, cx| {
                    (workbench.update(cx, |workbench, cx| {
                        workbench.hide_bottom_panel(cx);
                        window.focus(&workbench.focus_handle(cx), cx);
                    }))
                    .ok();
                });
            }
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
        let colors = FileColors::new(&ui, cx);
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
                    Line::File(commit, file) => {
                        let parts = self.file_parts(commit, file)?;
                        return Some(
                            row.when(selected, |row| row.bg(selected_bg))
                                .hover(move |row| row.bg(ui.row_hover))
                                .on_click(cx.listener(
                                    move |panel, event: &ClickEvent, window, cx| {
                                        panel.click(line, event.click_count(), window, cx)
                                    },
                                ))
                                .on_mouse_down(MouseButton::Right, menu_on(line, cx))
                                .child(file_cell(rail(commit), parts, &colors, ix, seen.clone()))
                                .into_any_element(),
                        );
                    }
                    Line::Note(commit) => {
                        let (text, color) = match self.expanded.get(&commit) {
                            Some(Listing::Failed(error)) => (error.clone(), ui.error_text),
                            _ => (LOADING.to_owned(), ui.muted_text),
                        };
                        return Some(
                            lettered(
                                row.when(selected, |row| row.bg(selected_bg))
                                    .text_color(color)
                                    .on_click(cx.listener(move |panel, _, window, cx| {
                                        panel.click(line, 1, window, cx)
                                    })),
                                lead().child(rail(commit)).child(ellipsized(text)),
                                ix,
                                seen.clone(),
                            )
                            .into_any_element(),
                        );
                    }
                };
                let Line::Commit(ix_commit) = line else {
                    return None;
                };
                let entry = self.commits.get(ix_commit)?;
                let merge = entry.parents.len() > 1;
                let lane = self.rows[ix_commit].color;
                let graph = graph_cell(
                    self.rows[ix_commit].clone(),
                    merge,
                    self.lanes,
                    ix,
                    seen.clone(),
                );
                let [summary, _] = commit_rows::texts(&entry.commit, self.now);
                seen.texts(ix, &[&summary]);
                let lead = (lead().child(graph))
                    .children(ref_pills(&entry.refs, lane, &ui))
                    .child(ellipsized(summary));
                let shown_card = match &mut self.shown_card {
                    Some((shown, card)) if *shown == ix_commit => Some(
                        (card.get_or_insert_with(|| {
                            let (now, repo) = (self.now, self.repo.clone());
                            let workbench = self.workbench.clone();
                            cx.new(|cx| CommitCard::new(entry, lane, now, repo, workbench, cx))
                        }))
                        .clone(),
                    ),
                    _ => None,
                };
                let (hovered, now, repo) = (entry.clone(), self.now, self.repo.clone());
                let workbench = self.workbench.clone();
                #[cfg(test)]
                let probe = self.card.clone();
                let row = row
                    .when(selected, |row| row.bg(selected_bg))
                    .hover(move |row| row.bg(ui.row_hover))
                    .on_click(cx.listener(move |panel, event: &ClickEvent, window, cx| {
                        panel.click(line, event.click_count(), window, cx)
                    }))
                    .on_mouse_down(MouseButton::Right, menu_on(line, cx))
                    // Where a tooltip shows, one pixel past the pointer.
                    .when_some(shown_card, |row, card| {
                        let at = anchored().offset(point(px(1.), px(1.)));
                        row.child(deferred(at.child(card)))
                    })
                    .hoverable_tooltip(move |_, cx| {
                        let (repo, workbench) = (repo.clone(), workbench.clone());
                        let card = cx.new(|cx| {
                            let card = CommitCard::new(&hovered, lane, now, repo, workbench, cx);
                            #[cfg(test)]
                            let card = card.probed(&probe);
                            card
                        });
                        card.into()
                    });
                Some(lettered(row, lead, ix, seen).into_any_element())
            })
            .collect()
    }
}

/// Opens `entry`'s change to all its files, read from `repo`, in
/// `workbench`, once this update is over: the workbench moves focus.
fn open_commit_diff(
    workbench: WeakEntity<Workbench>,
    repo: Arc<Repo>,
    entry: LogCommit,
    window: &mut Window,
    cx: &mut App,
) {
    window.defer(cx, move |window, cx| {
        (workbench.update(cx, |workbench, cx| {
            workbench.open_commit_diff(repo, entry, window, cx);
        }))
        .ok();
    });
}

/// What a right click on `line`'s row does: opens its menu.
fn menu_on(
    line: Line,
    cx: &mut Context<GitLogPanel>,
) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |panel, event: &MouseDownEvent, window, cx| {
        cx.stop_propagation();
        panel.open_menu(line, event.position, window, cx);
    })
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

/// What a row with a graph cell or a rail shows before its letter: the
/// cell, then the rest, clipped together.
fn lead() -> Div {
    (div().flex_1().min_w_0().flex().items_center().gap_2()).overflow_hidden()
}

/// `row` with its `lead` and, after it, the letter's room left empty, so
/// a graph cell's or rail's lanes clip where a file row's do, before its
/// letter, and run on unbroken from row to row however narrow the list.
fn lettered(row: Stateful<Div>, lead: Div, ix: usize, probe: RowsProbe) -> Stateful<Div> {
    // As a file row's cell holds what's before its letter, then the letter.
    let cell = (div().flex_1().min_w_0().flex().items_center().gap_2())
        .child(lead)
        .child(div().flex_none().w(LETTER_WIDTH));
    #[cfg(test)]
    let cell = cell.on_children_prepainted(move |drawn, _, _| {
        probe.0.borrow_mut().before.insert(ix, drawn[0]);
    });
    #[cfg(not(test))]
    let _ = (ix, probe);
    row.child(cell)
}

/// A ref label's text: a tag's prefixed `tag`.
#[cfg(test)]
fn label_text(label: &RefLabel) -> String {
    match label.kind {
        RefKind::Tag => format!("tag {}", label.name),
        _ => label.name.clone(),
    }
}

/// How many ref pills a row shows before the rest collapse into `+N`.
const MAX_PILLS: usize = 3;

/// How a ref's pill looks, apart from its text.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PillStyle {
    /// The text and border colour: the commit's lane colour.
    color: Hsla,
    /// The fill: solid for HEAD's pill, a tint of `color` for the others.
    fill: Hsla,
    /// The text colour: the theme's background on HEAD's solid fill,
    /// `color` itself on a tint.
    text: Hsla,
    /// A remote-tracking branch's cloud, a tag's tag.
    icon: Option<Icon>,
}

/// `label`'s pill on a commit of lane colour `lane`.
fn pill_style(label: &RefLabel, lane: usize, ui: &UiTheme) -> PillStyle {
    let color = lane_color(lane);
    let head = label.is_head || label.kind == RefKind::Head;
    PillStyle {
        color,
        fill: if head { color } else { color.opacity(0.18) },
        text: if head { ui.background } else { color },
        icon: match label.kind {
            RefKind::Remote => Some(Icon::Cloud),
            RefKind::Tag => Some(Icon::Tag),
            RefKind::Head | RefKind::Branch => None,
        },
    }
}

/// A ref's text in its pill: the name, with a tag's `tag` left to its icon.
fn pill_text(label: &RefLabel) -> &str {
    &label.name
}

/// The first [`MAX_PILLS`] of `refs`, and how many more there are.
fn visible_pills(refs: &[RefLabel]) -> (&[RefLabel], usize) {
    let shown = refs.len().min(MAX_PILLS);
    (&refs[..shown], refs.len() - shown)
}

/// A row's ref pills: the first few, then a muted `+N` for the rest, which
/// the commit's card lists.
fn ref_pills(refs: &[RefLabel], lane: usize, ui: &UiTheme) -> Vec<AnyElement> {
    let (shown, more) = visible_pills(refs);
    let mut pills: Vec<AnyElement> = (shown.iter())
        .map(|label| ref_label(label, lane, ui).into_any_element())
        .collect();
    if more > 0 {
        let rest = div()
            .flex_none()
            .text_size(px(11.))
            .text_color(ui.muted_text)
            .child(format!("+{more}"));
        pills.push(rest.into_any_element());
    }
    pills
}

/// A ref pointing at a commit, as a small pill in its commit's lane colour:
/// HEAD's filled and bold, the others tinted, a remote's after a cloud and a
/// tag's after a tag.
fn ref_label(label: &RefLabel, lane: usize, ui: &UiTheme) -> impl IntoElement {
    let style = pill_style(label, lane, ui);
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_0p5()
        .px_1()
        .rounded_sm()
        .border_1()
        .border_color(style.color)
        .bg(style.fill)
        .text_color(style.text)
        .text_size(px(11.))
        .when(label.is_head, |label| label.font_weight(FontWeight::BOLD))
        .children(style.icon.map(|icon| {
            svg()
                .path(icon.path())
                .size(px(11.))
                .flex_none()
                .text_color(style.text)
        }))
        .child(pill_text(label).to_owned())
}

/// A file row after its commit's `rail`: the file's icon from the installed
/// icon set, its name, then its directory dimmed; at the right edge its line
/// counts, muted, and the change's letter. The directory shrinks first, then
/// the name, then the icon and the counts, by their weights; the letter
/// never does.
fn file_cell(
    rail: impl IntoElement,
    parts: FileParts,
    colors: &FileColors,
    ix: usize,
    probe: RowsProbe,
) -> impl IntoElement {
    let icon = file_icon_ref(&parts.name);
    let icon_cell = (div().flex_shrink(LAST).min_w_0().overflow_hidden())
        .flex()
        .items_center()
        .child(icon.element(px(14.), colors.icon));
    let name = (div().flex_shrink(LAST * 100.).min_w_0().overflow_hidden()).text_ellipsis();
    let dir = (div()
        .flex_1()
        .flex_shrink(LAST * 10_000.)
        .min_w_0()
        .overflow_hidden())
    .text_ellipsis()
    .text_color(colors.muted);
    let count = |color: Hsla| {
        (div().flex_shrink(LAST).min_w_0().overflow_hidden())
            .text_ellipsis()
            .text_color(muted(color))
    };
    let (added, removed) = parts.counts.unwrap_or_default();
    let counts = [
        (added > 0).then(|| (format!("+{added}"), count(colors.added))),
        (removed > 0).then(|| (format!("-{removed}"), count(colors.deleted))),
    ];
    let mut letter =
        (div().flex_none().w(LETTER_WIDTH).text_right()).text_color(colors.letter(parts.kind));
    // The icon's cell is noted by its icon's path, and shows no text.
    let mut cells: Vec<_> = [
        (icon.path().to_string(), icon_cell),
        (parts.name, name),
        (parts.dir, dir),
    ]
    .into_iter()
    .chain(counts.into_iter().flatten())
    .collect();
    let kind = parts.kind.to_string();
    probe.cells(
        ix,
        (cells.iter_mut().map(|(text, cell)| (text.clone(), cell)))
            .chain([(kind.clone(), &mut letter)]),
    );
    #[cfg(test)]
    let at_letter = cells.len();
    let cells = (cells.into_iter().enumerate())
        .map(|(at, (text, cell))| if at == 0 { cell } else { cell.child(text) });
    // The rail and everything before the letter shrink and clip together,
    // however many lanes the rail has, so the letter beside them keeps its
    // room.
    let before = (div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .overflow_hidden())
    .child(rail)
    .children(cells);
    #[cfg(test)]
    let before = before.on_children_prepainted({
        let probe = probe.clone();
        move |drawn, _, _| probe.bounds(ix, 0, &drawn[1..])
    });
    let cell = (div().flex_1().min_w_0().flex().items_center().gap_2())
        .child(before)
        .child(letter.child(kind));
    #[cfg(test)]
    let cell = cell.on_children_prepainted(move |drawn, _, _| {
        probe.bounds(ix, at_letter, &drawn[1..]);
        probe.0.borrow_mut().before.insert(ix, drawn[0]);
    });
    cell
}

/// The flex shrink weight of what in a file row shrinks last: its icon and
/// its counts, once its name and directory have gone. At least 1, so it
/// takes the whole of what's left to shrink.
const LAST: f32 = 1.;

/// `color` muted, for a file's line counts.
fn muted(color: Hsla) -> Hsla {
    Hsla {
        a: color.a * 0.75,
        ..color
    }
}

/// A file row's colours, from the theme: its directory's and icon's from
/// the chrome's, its letter's and counts' from the editor's palette, a
/// modification amber (a warning's), an addition green (an added row's),
/// a deletion red (an error's) and a rename blue (information's).
struct FileColors {
    muted: Hsla,
    icon: Hsla,
    modified: Hsla,
    added: Hsla,
    deleted: Hsla,
    renamed: Hsla,
}

impl FileColors {
    fn new(ui: &UiTheme, cx: &App) -> Self {
        let palette = fe_gpui::editor_palette(cx);
        let editor = &palette.editor;
        let hsla = |color: fe_config::theme::Color| rgba(color.to_rgba_u32()).into();
        Self {
            muted: ui.muted_text,
            icon: ui.icon,
            modified: hsla(editor.warning),
            added: hsla(editor.change_added),
            deleted: hsla(editor.error),
            renamed: hsla(editor.information),
        }
    }

    /// The colour of a change's letter.
    fn letter(&self, kind: char) -> Hsla {
        match kind {
            'A' => self.added,
            'D' => self.deleted,
            'R' => self.renamed,
            _ => self.modified,
        }
    }
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
    /// A commit row's texts after its ref labels.
    pub(crate) texts: BTreeMap<usize, Vec<String>>,
    /// The bounds a row's graph cell or rail and what follows it clip to,
    /// before the letter.
    pub(crate) before: BTreeMap<usize, Bounds<Pixels>>,
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
    /// Whether it neither grows nor shrinks.
    pub(crate) flex_none: bool,
    /// Whether its text is aligned right.
    pub(crate) right: bool,
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
                flex_none: cell.style().flex_grow == Some(0.)
                    && cell.style().flex_shrink == Some(0.),
                right: cell.style().text.text_align == Some(gpui::TextAlign::Right),
                bounds: None,
            });
            self.0.borrow_mut().cells.insert(_ix, cells.collect());
        }
    }

    /// Notes commit row `ix`'s texts after its ref labels.
    fn texts(&self, _ix: usize, _texts: &[&str]) {
        #[cfg(test)]
        {
            let texts = _texts.iter().map(|text| text.to_string()).collect();
            self.0.borrow_mut().texts.insert(_ix, texts);
        }
    }

    /// Notes row `ix`'s cells' bounds, from its cell `from` on.
    #[cfg(test)]
    fn bounds(&self, ix: usize, from: usize, drawn: &[Bounds<Pixels>]) {
        if let Some(cells) = self.0.borrow_mut().cells.get_mut(&ix) {
            for (cell, bounds) in cells.iter_mut().skip(from).zip(drawn) {
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
    /// Lines added and removed, once read; none for a binary file.
    counts: Option<(u32, u32)>,
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
        // While the box has focus, without the `vim` flag, so no vim key
        // of the list's takes what's typed.
        let key_context = if self.filter_focused(window, cx) {
            let mut key_context = KeyContext::default();
            key_context.add(KEY_CONTEXT);
            key_context
        } else {
            commit_rows::key_context(KEY_CONTEXT, cx)
        };
        let filtering = !self.filter_input.read(cx).text().is_empty();
        let filter_row = (div()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1())
        .on_key_down(cx.listener(Self::on_filter_key))
        .on_action(cx.listener(Self::on_filter_command))
        .child(div().flex_1().min_w_0().child(self.filter_input.clone()))
        .when(filtering, |row| {
            row.child(
                IconButton::new(FILTER_CLEAR, Icon::X, &ui)
                    .name("Clear Filter")
                    .on_click(cx.listener(|panel, _, _, cx| panel.clear_filter(cx))),
            )
        });
        let menu = self.render_menu(cx);
        <Self as kiln::ViewCommands>::attach(div().track_focus(&self.focus), cx)
            .key_context(key_context)
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
            .child(filter_row)
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
            .children(menu)
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
            (labels, vec![summary])
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

    /// The muted text colour the last rows drawn used.
    pub(crate) fn muted_text(&self) -> Hsla {
        self.rows_seen.0.borrow().muted
    }

    /// What the tests see of the rows drawn.
    pub(crate) fn rows_seen(&self) -> Rc<RefCell<RowsSeen>> {
        self.rows_seen.0.clone()
    }

    /// What the tests see of the last commit card built.
    pub(crate) fn card_probe(&self) -> Rc<RefCell<CardProbe>> {
        self.card.clone()
    }
}

#[path = "git_log_menu.rs"]
mod git_log_menu;

#[path = "commit_diff_view.rs"]
pub(crate) mod commit_diff_view;

#[path = "commit_diff.rs"]
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "the view uses part of the model's API")
)]
pub(crate) mod commit_diff;

#[cfg(test)]
#[path = "git_log_tests.rs"]
mod git_log_tests;

#[cfg(test)]
#[path = "git_log_open_tests.rs"]
mod git_log_open_tests;

#[cfg(test)]
#[path = "git_log_select_tests.rs"]
mod git_log_select_tests;

#[cfg(test)]
#[path = "git_log_file_rows_tests.rs"]
mod git_log_file_rows_tests;

#[cfg(test)]
#[path = "git_log_expanded_tests.rs"]
mod git_log_expanded_tests;

#[cfg(test)]
#[path = "git_log_ref_pills_tests.rs"]
mod git_log_ref_pills_tests;

#[cfg(test)]
#[path = "git_log_filter_tests.rs"]
mod git_log_filter_tests;
