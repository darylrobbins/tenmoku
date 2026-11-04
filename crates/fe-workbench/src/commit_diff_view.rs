//! A Graph commit's change to all its files as one scrolling tab, as VS
//! Code's multi-file diff editor shows a commit: [`CommitDiffView`], over
//! the [`CommitDiff`] model, titled `<short> - <summary> (N files)`.
//! `Open Changes` on a commit row (its menu, or `git_log.open_changes` with
//! it selected) and the files-changed count on its card open it; asking
//! again for the same commit shows the tab open (`Workbench::open_commit_diff`).
//!
//! A section a file, each under a header: a chevron, the file's icon from
//! the installed icon set, its name and, dimmed, its directory (a rename's
//! old path after it), then at the right its lines added and removed and
//! its change's letter, coloured as the Graph's file rows colour them. A
//! click on the header or its chevron collapses the section, or expands
//! it. Under it the section's diff side by side, drawn here read-only
//! rather than by embedding `DiffView`'s editors, whose own scrolling
//! can't sit in one list: the left side's rows the parent's, the right's
//! the commit's, each after its line number, a hunk's removed and added
//! rows paired across, the shorter side hatched; the removed rows tinted
//! removed, the added added, and, as `DiffView` tints them, the changed
//! words of a modified pair darker ([`Diffed::words`]) and a row with no
//! partner darker throughout. Unchanged rows away from the hunks are
//! folded into `N hidden lines` rows, which a click shows.
//!
//! The whole view scrolls vertically, one list of rows, telling the model
//! which rows it shows so it reads the sections near them
//! ([`CommitDiff::set_viewport`]); a horizontal scroll over a section moves
//! both its sides together, until its widest row's end shows (its rows'
//! widths shaped in the font they're drawn in, wide characters and all,
//! against its own line numbers' width). The toolbar's previous and next change buttons go from
//! hunk to hunk across the sections in order, wrapping at the ends,
//! expanding a collapsed section they land in (one not read yet is read,
//! then its first hunk, or its last going back, is gone to, unless it's
//! collapsed meanwhile); its other two collapse and expand every section.
//! With the view open, a click (or Enter) on one of the commit's files in
//! the Graph shows the tab scrolled to the file's section, expanded
//! ([`CommitDiffView::reveal`]), once the files are read if they aren't
//! yet. What navigation or a reveal goes to stays in view as sections
//! above it land, until the wheel scrolls the view or a section is
//! collapsed or expanded, which also cancels a landing still waiting for
//! a section's texts. A tab is a commit's as read from one working
//! tree: the same commit from another worktree is another tab.

#[cfg(test)]
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use std::{
    collections::HashSet,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_git::{ChangeKind, LogCommit, Repo};
use fe_gpui::EditorTheme;
use fe_ui::{Icon, IconButton, IconRef, ROW_HEIGHT, Tooltip, file_icon_ref};
use gpui::{
    AnyElement, App, AppContext as _, Context, Div, ElementId, Entity, FocusHandle, Focusable,
    FontWeight, HighlightStyle, Hsla, InteractiveElement as _, IntoElement, KeyContext,
    ParentElement as _, Pixels, Render, ScrollStrategy, ScrollWheelEvent, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledText, Subscription,
    UniformListScrollHandle, WeakEntity, Window, div, font, pattern_slash,
    prelude::FluentBuilder as _, px, svg, uniform_list,
};
use kiln_shell::{PaneItem, PaneView as _};

use super::{
    FileColors, LETTER_WIDTH,
    commit_diff::{CommitDiff, Diffed, Line, Row, Section, Texts},
    muted,
};
use crate::{Pane, Workbench};

/// The key context the view's element sets.
const KEY_CONTEXT: &str = "CommitDiffView";

/// The columns a tab takes in a row.
const TAB_WIDTH: usize = 4;

/// A row of the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewRow {
    /// Section `0`'s header.
    Header(usize),
    /// Section `0`'s `Loading…` or error row.
    Note(usize),
    /// A row of a section's diff side by side: its left and right rows,
    /// `None` for a side's filler; `changed` in a hunk.
    Pair {
        section: usize,
        left: Option<u32>,
        right: Option<u32>,
        changed: bool,
    },
    /// Line `line` of a section's diff, a fold of unchanged rows.
    Hidden { section: usize, line: usize },
}

impl ViewRow {
    fn section(self) -> usize {
        match self {
            ViewRow::Header(section) | ViewRow::Note(section) => section,
            ViewRow::Pair { section, .. } | ViewRow::Hidden { section, .. } => section,
        }
    }
}

/// Where previous and next change go: a section's hunk, by its index
/// there, or a section whose hunks aren't known yet (`hunk` `None`), and
/// the row it shows at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stop {
    section: usize,
    hunk: Option<usize>,
    row: usize,
}

impl Stop {
    /// Its place in the order the stops are gone through.
    fn key(self) -> (usize, i64) {
        (self.section, self.hunk.map_or(-1, |hunk| hunk as i64))
    }
}

/// Where navigation or a reveal put the view: a section, by path, and its
/// hunk or else its header, kept in view as the rows above it change (a
/// section landing, say) until the user scrolls or collapses or expands
/// something.
#[derive(Clone, Debug, PartialEq)]
struct Anchor {
    path: PathBuf,
    hunk: Option<usize>,
    strategy: ScrollStrategy,
    /// Whether it's still to be expanded once its section is listed: a
    /// reveal's, which may come before the files are read.
    expand: bool,
}

/// What the tests see of the rows drawn, by row.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Seen {
    pub(crate) rows: BTreeMap<usize, Drawn>,
    /// The room past the end of each side's text (0 the left, 1 the right)
    /// in its clip, by row, as last drawn: negative where it's cut off.
    pub(crate) room: BTreeMap<(usize, usize), Pixels>,
}

/// A row as last drawn.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Drawn {
    Header(HeaderSeen),
    Note(String),
    Pair([Option<CellSeen>; 2]),
    Hidden(String),
}

/// A section header as drawn.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HeaderSeen {
    pub(crate) collapsed: bool,
    /// The file icon's asset path.
    pub(crate) icon: SharedString,
    pub(crate) name: String,
    pub(crate) dir: String,
    pub(crate) dir_color: Hsla,
    /// Why there's no diff, if there isn't one.
    pub(crate) note: Option<String>,
    /// Each count's text and colour.
    pub(crate) counts: Vec<(String, Hsla)>,
    pub(crate) letter: char,
    pub(crate) letter_color: Hsla,
}

/// One side of a row as drawn: `None` for a filler.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CellSeen {
    pub(crate) number: u32,
    pub(crate) text: String,
    /// The row's tint, if it has one.
    pub(crate) tint: Option<Hsla>,
    /// The bytes of `text` tinted as changed words, and their colour.
    pub(crate) words: Vec<Range<usize>>,
    pub(crate) word_tint: Option<Hsla>,
    /// How far its text is scrolled left.
    pub(crate) offset: Pixels,
    /// Its text's left edge as last drawn, less its clip's.
    pub(crate) text_x: Option<Pixels>,
}

/// A commit's change to all its files: see the module docs.
pub(crate) struct CommitDiffView {
    workbench: WeakEntity<Workbench>,
    diff: Entity<CommitDiff>,
    rows: Vec<ViewRow>,
    /// The model row each row starts at.
    model_rows: Vec<usize>,
    /// Each section's header's row.
    headers: Vec<usize>,
    stops: Vec<Stop>,
    /// The stop navigation last went to, by its key.
    at: Option<(usize, i64)>,
    /// A section navigation landed on before its texts were read, and
    /// whether it went forward: its first or last hunk is gone to once
    /// they are.
    pending: Option<(usize, bool)>,
    anchor: Option<Anchor>,
    /// The folds shown, by section path and line.
    revealed: HashSet<(PathBuf, usize)>,
    /// Each section's horizontal scroll, by path.
    offsets: Vec<(PathBuf, Pixels)>,
    /// The rows last drawn.
    viewport: Range<usize>,
    /// A character's width in the font last drawn with.
    em: Pixels,
    /// The list's width as last drawn.
    list_width: Pixels,
    /// Each section's widest row as drawn, by path, and the font family and
    /// size it was measured in.
    widths: Vec<(PathBuf, (SharedString, Pixels), Pixels)>,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    _diff: Subscription,
    #[cfg(test)]
    seen: Rc<RefCell<Seen>>,
}

impl CommitDiffView {
    pub(crate) fn new(
        workbench: WeakEntity<Workbench>,
        diff: Entity<CommitDiff>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&diff, |view, _, cx| {
            view.relayout(cx);
            cx.notify();
        });
        let mut view = Self {
            workbench,
            diff,
            rows: Vec::new(),
            model_rows: Vec::new(),
            headers: Vec::new(),
            stops: Vec::new(),
            at: None,
            pending: None,
            anchor: None,
            revealed: HashSet::new(),
            offsets: Vec::new(),
            viewport: 0..0,
            em: px(8.),
            list_width: px(0.),
            widths: Vec::new(),
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            _diff: subscription,
            #[cfg(test)]
            seen: Rc::default(),
        };
        view.relayout(cx);
        view
    }

    /// Whether it's of commit `id` read from the repository whose working
    /// tree is `workdir`.
    pub(crate) fn is_of(&self, id: &str, workdir: &Path, cx: &App) -> bool {
        let diff = self.diff.read(cx);
        diff.commit().commit.id == id && diff.workdir() == workdir
    }

    /// Lays the model's rows out, goes on from a landing that waited for
    /// its texts and keeps the anchor in view.
    fn relayout(&mut self, cx: &mut Context<Self>) {
        self.layout_rows(cx);
        self.land_pending(cx);
        self.follow_anchor(cx);
    }

    /// Lays the model's rows out side by side, and where navigation stops.
    fn layout_rows(&mut self, cx: &mut Context<Self>) {
        let diff = self.diff.read(cx);
        let (sections, rows) = (diff.sections(), diff.rows());
        self.rows.clear();
        self.model_rows.clear();
        self.headers.clear();
        self.stops.clear();
        let mut hunk = 0;
        let mut ix = 0;
        while ix < rows.len() {
            let row = rows[ix];
            let start = ix;
            ix += 1;
            let line = match row {
                Row::Header(section) => {
                    hunk = 0;
                    let at = self.rows.len();
                    self.headers.push(at);
                    self.push(ViewRow::Header(section), start);
                    let collapsed = diff.is_collapsed(section);
                    match &sections[section].texts {
                        Texts::Read(diffed) if collapsed => {
                            let hunks = hunk_count(&diffed.lines);
                            self.stops.extend((0..hunks).map(|hunk| Stop {
                                section,
                                hunk: Some(hunk),
                                row: at,
                            }));
                        }
                        Texts::Unread | Texts::Loading(_) => self.stops.push(Stop {
                            section,
                            hunk: None,
                            row: at,
                        }),
                        _ => {}
                    }
                    continue;
                }
                Row::Loading(section) | Row::Failed(section) => {
                    self.push(ViewRow::Note(section), start);
                    continue;
                }
                Row::Line(section, line) => (section, line),
            };
            let (section, line) = line;
            let Texts::Read(diffed) = &sections[section].texts else {
                continue;
            };
            match &diffed.lines[line] {
                Line::Same { left, right } => self.push(
                    ViewRow::Pair {
                        section,
                        left: Some(*left),
                        right: Some(*right),
                        changed: false,
                    },
                    start,
                ),
                Line::Hidden { left, right } => {
                    let path = sections[section].path.clone();
                    if self.revealed.contains(&(path, line)) {
                        for (left, right) in left.clone().zip(right.clone()) {
                            let pair = ViewRow::Pair {
                                section,
                                left: Some(left),
                                right: Some(right),
                                changed: false,
                            };
                            self.push(pair, start);
                        }
                    } else {
                        self.push(ViewRow::Hidden { section, line }, start);
                    }
                }
                Line::Removed(_) | Line::Added(_) => {
                    // The hunk: its removed rows then its added rows, paired.
                    let (mut removed, mut added) = (Vec::new(), Vec::new());
                    let mut at = start;
                    while let Some(Row::Line(other, line)) = rows.get(at)
                        && *other == section
                    {
                        match diffed.lines[*line] {
                            Line::Removed(row) => removed.push((row, at)),
                            Line::Added(row) => added.push((row, at)),
                            _ => break,
                        }
                        at += 1;
                    }
                    ix = at;
                    self.stops.push(Stop {
                        section,
                        hunk: Some(hunk),
                        row: self.rows.len(),
                    });
                    hunk += 1;
                    for pair in 0..removed.len().max(added.len()) {
                        let (left, right) = (removed.get(pair), added.get(pair));
                        let model_row = left.or(right).map_or(start, |(_, at)| *at);
                        let row = ViewRow::Pair {
                            section,
                            left: left.map(|(row, _)| *row),
                            right: right.map(|(row, _)| *row),
                            changed: true,
                        };
                        self.push(row, model_row);
                    }
                }
            }
        }
    }

    fn push(&mut self, row: ViewRow, model_row: usize) {
        self.rows.push(row);
        self.model_rows.push(model_row);
    }

    /// Scrolls the anchor's row into view, once its section is listed,
    /// expanding a reveal's.
    fn follow_anchor(&mut self, cx: &mut Context<Self>) {
        let Some(anchor) = self.anchor.clone() else {
            return;
        };
        let diff = self.diff.read(cx);
        let Some(section) = (diff.sections().iter()).position(|at| at.path == anchor.path) else {
            return;
        };
        if anchor.expand {
            // Once: a collapse after it stands.
            if let Some(anchor) = self.anchor.as_mut() {
                anchor.expand = false;
            }
            self.at = Some((section, -1));
            if diff.is_collapsed(section) {
                (self.diff).update(cx, |diff, cx| diff.set_collapsed(section, false, cx));
                self.layout_rows(cx);
            }
        }
        let hunk = (anchor.hunk).and_then(|hunk| {
            let stop = self
                .stops
                .iter()
                .find(|stop| stop.key() == (section, hunk as i64));
            Some(stop?.row)
        });
        let row = hunk.unwrap_or(self.headers[section]);
        self.scroll_to(row, anchor.strategy, cx);
    }

    /// Lets the view scroll as the user takes it: what navigation or a
    /// reveal last went to is no longer held in view.
    fn let_go(&mut self) {
        self.anchor = None;
        // Nor is a landing waiting for its section's texts.
        self.pending = None;
    }

    /// Goes on from a section navigation landed on before its texts were
    /// read, once they are: to its first hunk, or last going back, else on
    /// past it.
    fn land_pending(&mut self, cx: &mut Context<Self>) {
        let Some((section, forward)) = self.pending else {
            return;
        };
        let diff = self.diff.read(cx);
        let Some(texts) = diff.sections().get(section).map(|section| &section.texts) else {
            self.pending = None;
            return;
        };
        if matches!(texts, Texts::Unread | Texts::Loading(_)) {
            return;
        }
        self.pending = None;
        let hunks = self.stops.iter().filter(|stop| stop.section == section);
        let hunks: Vec<usize> = hunks.filter_map(|stop| stop.hunk).collect();
        let hunk = match forward {
            true => hunks.first(),
            false => hunks.last(),
        };
        match hunk {
            Some(&hunk) => {
                let target = (self.stops.iter())
                    .position(|stop| stop.key() == (section, hunk as i64))
                    .expect("its stop is listed");
                self.land(target, forward, cx);
            }
            None => {
                // Past it.
                let key = (section, if forward { i64::MAX } else { -2 });
                if let Some(target) = self.after(key, forward) {
                    self.land(target, forward, cx);
                }
            }
        }
    }

    /// The stop after the one at `key`, or before it, wrapping.
    fn after(&self, key: (usize, i64), forward: bool) -> Option<usize> {
        let count = self.stops.len();
        if count == 0 {
            return None;
        }
        Some(match forward {
            true => (self.stops.iter())
                .position(|stop| stop.key() > key)
                .unwrap_or(0),
            false => (self.stops.iter())
                .rposition(|stop| stop.key() < key)
                .unwrap_or(count - 1),
        })
    }

    /// The toolbar's next change, or previous: see the module docs.
    pub(crate) fn go_to_change(&mut self, forward: bool, cx: &mut Context<Self>) {
        let count = self.stops.len();
        if count == 0 {
            return;
        }
        let target = match self.at {
            Some(key) => self.after(key, forward),
            // From the rows in view.
            None => {
                let top = self.viewport.start;
                Some(match forward {
                    true => (self.stops.iter())
                        .position(|stop| stop.row >= top)
                        .unwrap_or(0),
                    false => (self.stops.iter())
                        .rposition(|stop| stop.row < top)
                        .unwrap_or(count - 1),
                })
            }
        };
        if let Some(target) = target {
            self.land(target, forward, cx);
        }
    }

    /// Goes to stop `target`, expanding its section.
    fn land(&mut self, target: usize, forward: bool, cx: &mut Context<Self>) {
        let Stop { section, hunk, .. } = self.stops[target];
        // This goes in place of one still waiting for its texts.
        self.pending = None;
        if self.diff.read(cx).is_collapsed(section) {
            (self.diff).update(cx, |diff, cx| diff.set_collapsed(section, false, cx));
            self.layout_rows(cx);
        }
        let strategy = match hunk {
            Some(hunk) => {
                self.at = Some((section, hunk as i64));
                ScrollStrategy::Center
            }
            None => {
                self.at = Some((section, -1));
                self.pending = Some((section, forward));
                ScrollStrategy::Top
            }
        };
        let path = self
            .section(section, cx)
            .map(|section| section.path.clone());
        self.anchor = path.map(|path| Anchor {
            path,
            hunk,
            strategy,
            expand: false,
        });
        self.follow_anchor(cx);
        cx.notify();
    }

    /// Scrolls row `row` into view as `strategy` says, telling the model
    /// it's shown so its section is read.
    fn scroll_to(&mut self, row: usize, strategy: ScrollStrategy, cx: &mut Context<Self>) {
        self.scroll.scroll_to_item(row, strategy);
        let shown = row..row + 1;
        self.show_rows(shown, cx);
    }

    /// Notes that the rows `rows` show.
    fn show_rows(&mut self, rows: Range<usize>, cx: &mut Context<Self>) {
        let start = self.model_rows.get(rows.start).copied().unwrap_or(0);
        let last = rows
            .end
            .checked_sub(1)
            .and_then(|row| self.model_rows.get(row));
        let end = last.map_or(start, |last| last + 1);
        self.viewport = rows;
        (self.diff).update(cx, |diff, cx| diff.set_viewport(start..end, cx));
    }

    /// Scrolls to the section of `path`, expanding it: a click on the file
    /// in the Graph. Before the files are read, once they are.
    pub(crate) fn reveal(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.pending = None;
        self.at = None;
        self.anchor = Some(Anchor {
            path: path.to_owned(),
            hunk: None,
            strategy: ScrollStrategy::Top,
            expand: true,
        });
        self.follow_anchor(cx);
        cx.notify();
    }

    /// Collapses section `section`, or expands it.
    fn toggle(&mut self, section: usize, cx: &mut Context<Self>) {
        self.let_go();
        (self.diff).update(cx, |diff, cx| diff.toggle(section, cx));
        self.relayout(cx);
        cx.notify();
    }

    fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.let_go();
        (self.diff).update(cx, |diff, cx| diff.collapse_all(cx));
        self.relayout(cx);
        cx.notify();
    }

    fn expand_all(&mut self, cx: &mut Context<Self>) {
        self.let_go();
        (self.diff).update(cx, |diff, cx| diff.expand_all(cx));
        self.relayout(cx);
        cx.notify();
    }

    /// Shows the rows the fold at line `line` of section `section` hides.
    fn unfold(&mut self, section: usize, line: usize, cx: &mut Context<Self>) {
        let Some(path) = self
            .section(section, cx)
            .map(|section| section.path.clone())
        else {
            return;
        };
        self.let_go();
        self.revealed.insert((path, line));
        self.relayout(cx);
        cx.notify();
    }

    fn section<'a>(&self, section: usize, cx: &'a App) -> Option<&'a Section> {
        self.diff.read(cx).sections().get(section)
    }

    /// Section `section`'s horizontal scroll.
    fn offset(&self, path: &Path) -> Pixels {
        (self.offsets.iter())
            .find(|(at, _)| at == path)
            .map_or(px(0.), |(_, offset)| *offset)
    }

    fn set_offset(&mut self, path: &Path, offset: Pixels) {
        match self.offsets.iter_mut().find(|(at, _)| at == path) {
            Some((_, at)) => *at = offset,
            None => self.offsets.push((path.to_owned(), offset)),
        }
    }

    /// The wheel over a row of section `section`: a sideways turn scrolls
    /// both its sides, up to its widest row's end.
    fn on_row_wheel(
        &mut self,
        section: usize,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(ROW_HEIGHT);
        let dx = match (delta.x, delta.y) {
            (x, y) if x.abs() > y.abs() => x,
            (_, y) if event.modifiers.shift => y,
            _ => return,
        };
        cx.stop_propagation();
        let theme = self.theme(cx);
        let Some(section) = self.diff.read(cx).sections().get(section) else {
            return;
        };
        let path = section.path.clone();
        let most = match &section.texts {
            Texts::Read(diffed) => self.most(&path, diffed, &theme, window),
            _ => px(0.),
        };
        let offset = (self.offset(&path).min(most) - dx).clamp(px(0.), most);
        self.set_offset(&path, offset);
        cx.notify();
    }

    /// The width of a section's line numbers, for its widest.
    fn gutter(&self, diffed: &Diffed) -> Pixels {
        let digits = diffed.left.row_count().max(diffed.right.row_count());
        self.em * (digits.to_string().len() + 2) as f32
    }

    /// How far a section's sides scroll sideways: until its widest row's
    /// end is in view on both sides, a few pixels from the narrower's edge
    /// (the left, by its border) whichever way the layout rounds their
    /// widths.
    fn most(
        &mut self,
        path: &Path,
        diffed: &Diffed,
        theme: &EditorTheme,
        window: &Window,
    ) -> Pixels {
        let text = self.list_width / 2. - self.gutter(diffed) - px(3.);
        (self.widest(path, diffed, theme, window) - text.max(px(0.))).max(px(0.))
    }

    /// The width of a section's widest row as drawn, every row of both its
    /// sides shaped in the font it's drawn in (so wide characters, fallback
    /// fonts and a proportional font's letters count), once a font.
    pub(crate) fn widest(
        &mut self,
        path: &Path,
        diffed: &Diffed,
        theme: &EditorTheme,
        window: &Window,
    ) -> Pixels {
        let font = (theme.font_family.clone(), theme.font_size);
        let known = (self.widths.iter()).find(|(at, at_font, _)| at == path && *at_font == font);
        if let Some((.., width)) = known {
            return *width;
        }
        let mut style = window.text_style();
        style.font_family = font.0.clone();
        style.font_size = font.1.into();
        let mut widest = px(0.);
        for side in [&diffed.left, &diffed.right] {
            for row in 0..side.row_count() {
                let (text, _) = detabbed(side.row(row), &[]);
                let run = style.to_run(text.len());
                let shaped = (window.text_system()).shape_line(text.into(), font.1, &[run], None);
                widest = widest.max(shaped.width);
            }
        }
        self.widths.retain(|(at, ..)| at != path);
        self.widths.push((path.to_owned(), font, widest));
        widest
    }

    /// The font the widths of the section of `path` were measured in.
    #[cfg(test)]
    pub(crate) fn measured(&self, path: &Path) -> Option<(SharedString, Pixels)> {
        let found = self.widths.iter().find(|(at, ..)| at == path);
        found.map(|(_, font, _)| font.clone())
    }

    /// The tab's title: `<short> - <summary> (N files)`, the count once the
    /// files are read.
    fn title(&self, cx: &App) -> String {
        let diff = self.diff.read(cx);
        let commit = &diff.commit().commit;
        let head = format!("{} - {}", commit.short, commit.summary);
        match (diff.message(), diff.sections().len()) {
            (Some(_), _) => head,
            (None, 1) => format!("{head} (1 file)"),
            (None, files) => format!("{head} ({files} files)"),
        }
    }

    fn theme(&self, cx: &App) -> EditorTheme {
        match self.workbench.upgrade() {
            Some(workbench) => workbench.read(cx).pane().read(cx).theme().clone(),
            None => EditorTheme::default_dark(cx),
        }
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ui = fe_ui::ui_theme(cx);
        let commit = self.diff.read(cx).commit().commit.clone();
        let none = self.stops.is_empty();
        let button = |id: &'static str, icon, title: &'static str| {
            IconButton::new(id, icon, &ui).tooltip(move |_, cx| {
                let tooltip = Tooltip::new(title, None::<String>, &fe_ui::ui_theme(cx));
                cx.new(|_| tooltip).into()
            })
        };
        let previous = button("commit-diff-previous", Icon::ArrowLeft, "Previous Change")
            .disabled(none)
            .on_click(cx.listener(|view, _, _, cx| view.go_to_change(false, cx)));
        let next = button("commit-diff-next", Icon::ArrowRight, "Next Change")
            .disabled(none)
            .on_click(cx.listener(|view, _, _, cx| view.go_to_change(true, cx)));
        let collapse = button(
            "commit-diff-collapse-all",
            Icon::FoldVertical,
            "Collapse All",
        )
        .on_click(cx.listener(|view, _, _, cx| view.collapse_all(cx)));
        let expand = button("commit-diff-expand-all", Icon::UnfoldVertical, "Expand All")
            .on_click(cx.listener(|view, _, _, cx| view.expand_all(cx)));
        (div()
            .flex_none()
            .h(ROW_HEIGHT)
            .px_2()
            .flex()
            .items_center()
            .gap_2())
        .border_b_1()
        .border_color(ui.border)
        .bg(ui.panel_background)
        .text_color(ui.text)
        .whitespace_nowrap()
        .child(
            div()
                .flex_none()
                .text_color(ui.muted_text)
                .child(commit.short.clone()),
        )
        .child(
            (div().flex_1().min_w_0().overflow_hidden())
                .text_ellipsis()
                .child(commit.summary.clone()),
        )
        .child(previous)
        .child(next)
        .child(collapse)
        .child(expand)
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        // The list also renders one row to measure it: that's not showing.
        if range.len() > 1 && range != self.viewport {
            self.show_rows(range.clone(), cx);
        }
        let theme = self.theme(cx);
        let ui = fe_ui::ui_theme(cx);
        let colors = FileColors::new(&ui, cx);
        let font_id = window
            .text_system()
            .resolve_font(&font(theme.font_family.clone()));
        self.em = (window.text_system().em_advance(font_id, theme.font_size)).unwrap_or(px(8.));
        self.list_width = self.scroll.0.borrow().base_handle.bounds().size.width;
        #[cfg(test)]
        let seen = self.seen.clone();
        let diff = self.diff.clone();
        let diff = diff.read(cx);
        let mut elements = Vec::new();
        for ix in range {
            let Some(&row) = self.rows.get(ix) else {
                continue;
            };
            let Some(section) = diff.sections().get(row.section()) else {
                continue;
            };
            let base = div()
                .id(ElementId::NamedInteger("commit-diff-row".into(), ix as u64))
                .h(ROW_HEIGHT)
                .w_full()
                .flex()
                .items_center()
                .overflow_hidden()
                .whitespace_nowrap();
            let element = match row {
                ViewRow::Header(at) => {
                    let header = header_parts(diff, at, section);
                    #[cfg(test)]
                    seen.borrow_mut()
                        .rows
                        .insert(ix, Drawn::Header(header.seen(&colors)));
                    let header = header.render(at, &ui, &colors);
                    (base.px_2().gap_2())
                        .debug_selector(move || format!("commit-diff-header:{at}"))
                        .cursor_pointer()
                        .bg(ui.panel_background)
                        .border_t_1()
                        .border_color(ui.border)
                        .text_color(ui.text)
                        .hover(move |row| row.bg(ui.row_hover))
                        .on_click(cx.listener(move |view, _, _, cx| view.toggle(at, cx)))
                        .children(header)
                        .into_any_element()
                }
                ViewRow::Note(_) => {
                    let (text, color) = match &section.texts {
                        Texts::Failed(error) => (error.clone(), ui.error_text),
                        _ => (super::commit_diff::LOADING.to_owned(), ui.muted_text),
                    };
                    #[cfg(test)]
                    seen.borrow_mut().rows.insert(ix, Drawn::Note(text.clone()));
                    (base.px_4().text_color(color).child(text)).into_any_element()
                }
                ViewRow::Hidden { section: at, line } => {
                    let Texts::Read(diffed) = &section.texts else {
                        continue;
                    };
                    let text = diffed.line_text(&diffed.lines[line]);
                    #[cfg(test)]
                    seen.borrow_mut()
                        .rows
                        .insert(ix, Drawn::Hidden(text.clone()));
                    (base.px_4().gap_2())
                        .debug_selector(move || format!("commit-diff-hidden:{ix}"))
                        .cursor_pointer()
                        .bg(theme.background.blend(theme.diff_filler))
                        .text_color(theme.gutter_foreground)
                        .text_size(theme.font_size)
                        .font_family(theme.font_family.clone())
                        .hover(move |row| row.text_color(theme.foreground))
                        .on_click(cx.listener(move |view, _, _, cx| view.unfold(at, line, cx)))
                        .child(
                            svg()
                                .path(Icon::UnfoldVertical.path())
                                .size(theme.font_size)
                                .flex_none()
                                .text_color(theme.gutter_foreground),
                        )
                        .child(text)
                        .into_any_element()
                }
                ViewRow::Pair {
                    section: at,
                    left,
                    right,
                    changed,
                } => {
                    let Texts::Read(diffed) = &section.texts else {
                        continue;
                    };
                    // Within what the section's widest row allows now, the
                    // list's width or the font having changed.
                    // Kept so, so the next scroll moves from where it shows.
                    let mut offset = self.offset(&section.path);
                    if offset > px(0.) {
                        let most = self.most(&section.path, diffed, &theme, window);
                        if offset > most {
                            offset = most;
                            self.set_offset(&section.path, most);
                        }
                    }
                    let gutter = self.gutter(diffed);
                    let paired = left.is_some() && right.is_some();
                    let cell = |side: usize, row: Option<u32>| Cell {
                        side,
                        row,
                        changed,
                        paired,
                        offset,
                        gutter,
                    };
                    let cells = [cell(0, left), cell(1, right)];
                    #[cfg(test)]
                    let seen_cells = Rc::new(RefCell::new(
                        cells.each_ref().map(|cell| cell.seen(diffed, &theme)),
                    ));
                    let [left, right] = cells.map(|cell| {
                        #[cfg(test)]
                        let clip = {
                            let (side, seen_cells, seen) =
                                (cell.side, seen_cells.clone(), seen.clone());
                            move |clip: Div| {
                                clip.on_children_prepainted(move |drawn, _, _| {
                                    // Where the text starts unscrolled, then
                                    // the text.
                                    let [start, text] = [drawn[0], drawn[1]];
                                    let mut cells = seen_cells.borrow_mut();
                                    if let Some(cell) = cells[side].as_mut() {
                                        cell.text_x = Some(text.left() - start.left());
                                    }
                                    let row = Drawn::Pair(cells.clone());
                                    let mut seen = seen.borrow_mut();
                                    seen.rows.insert(ix, row);
                                    seen.room.insert((ix, side), start.right() - text.right());
                                })
                            }
                        };
                        #[cfg(not(test))]
                        let clip = |clip: Div| clip;
                        cell.render(diffed, &theme, clip)
                    });
                    #[cfg(test)]
                    {
                        let row = Drawn::Pair(seen_cells.borrow().clone());
                        seen.borrow_mut().rows.insert(ix, row);
                    }
                    let _ = at;
                    base.bg(theme.background)
                        .text_size(theme.font_size)
                        .font_family(theme.font_family.clone())
                        .text_color(theme.foreground)
                        .on_scroll_wheel(cx.listener(move |view, event, window, cx| {
                            view.on_row_wheel(at, event, window, cx)
                        }))
                        .child(left.border_r_1().border_color(theme.popover_border))
                        .child(right)
                        .into_any_element()
                }
            };
            elements.push(element);
        }
        elements
    }
}

/// `text` with its tabs as spaces, and `spans`, bytes of it, as bytes of
/// what it becomes.
fn detabbed(text: &str, spans: &[Range<usize>]) -> (String, Vec<Range<usize>>) {
    if !text.contains('\t') {
        return (text.to_owned(), spans.to_vec());
    }
    let mut out = String::with_capacity(text.len());
    // Where each byte of `text` starts in `out`, and its end.
    let mut at = Vec::with_capacity(text.len() + 1);
    for ch in text.chars() {
        let start = out.len();
        at.extend(std::iter::repeat_n(start, ch.len_utf8()));
        match ch {
            '\t' => out.push_str(&" ".repeat(TAB_WIDTH)),
            _ => out.push(ch),
        }
    }
    at.push(out.len());
    let spans = (spans.iter())
        .map(|span| at[span.start.min(text.len())]..at[span.end.min(text.len())])
        .collect();
    (out, spans)
}

/// One side of a row of a section's diff.
struct Cell {
    /// 0 the left side, 1 the right.
    side: usize,
    /// Its row in the side's text; `None` for a filler.
    row: Option<u32>,
    /// In a hunk.
    changed: bool,
    /// The other side has a row beside it.
    paired: bool,
    offset: Pixels,
    gutter: Pixels,
}

impl Cell {
    /// Its text, tabs as spaces, and the bytes of it to tint as changed
    /// words: a modified pair's changed words, all of a changed row with no
    /// partner.
    fn text(&self, row: u32, diffed: &Diffed) -> (String, Vec<Range<usize>>) {
        let side = [&diffed.left, &diffed.right][self.side];
        let text = side.row(row);
        let words = match (self.changed, self.paired) {
            (false, _) => Vec::new(),
            (true, true) => (diffed.words[self.side].get(&row))
                .cloned()
                .unwrap_or_default(),
            (true, false) => std::iter::once(0..text.len()).collect(),
        };
        detabbed(text, &words)
    }

    /// Its row's tint and its changed words'.
    fn tints(&self, theme: &EditorTheme) -> Option<(Hsla, Hsla)> {
        self.changed.then_some(match self.side {
            0 => (theme.diff_removed_row, theme.diff_removed_word),
            _ => (theme.diff_added_row, theme.diff_added_word),
        })
    }

    /// Its line number, then its text clipped and scrolled by its offset;
    /// a filler hatched.
    /// `clip` is handed its text's clip.
    fn render(&self, diffed: &Diffed, theme: &EditorTheme, clip: impl FnOnce(Div) -> Div) -> Div {
        let cell = div().flex_1().min_w_0().h_full().flex().items_center();
        let Some(row) = self.row else {
            return cell.bg(pattern_slash(theme.diff_filler, 1., 6.));
        };
        let (text, words) = self.text(row, diffed);
        let tints = self.tints(theme);
        let highlights = (words.into_iter())
            .filter(|span| !span.is_empty())
            .map(|span| {
                let style = HighlightStyle {
                    background_color: tints.map(|(_, word)| word),
                    ..HighlightStyle::default()
                };
                (span, style)
            })
            .collect::<Vec<_>>();
        let text = StyledText::new(text).with_highlights(highlights);
        cell.when_some(tints, |cell, (row, _)| cell.bg(row))
            .child(
                (div().flex_none().w(self.gutter).pr_2().text_right())
                    .text_color(theme.gutter_foreground)
                    .child((row + 1).to_string()),
            )
            .child(clip(
                (div().flex_1().min_w_0().h_full().overflow_hidden())
                    .relative()
                    .flex()
                    .items_center()
                    // Where the text starts unscrolled, as wide as the clip.
                    .child(div().absolute().left_0().top_0().h_0().w_full())
                    .child(div().flex_none().relative().left(-self.offset).child(text)),
            ))
    }

    /// What the tests see of it.
    #[cfg(test)]
    fn seen(&self, diffed: &Diffed, theme: &EditorTheme) -> Option<CellSeen> {
        let row = self.row?;
        let (text, words) = self.text(row, diffed);
        let tints = self.tints(theme);
        Some(CellSeen {
            number: row + 1,
            text,
            tint: tints.map(|(row, _)| row),
            words: words.into_iter().filter(|span| !span.is_empty()).collect(),
            word_tint: tints.map(|(_, word)| word),
            offset: self.offset,
            text_x: None,
        })
    }
}

/// What a section's header shows.
struct HeaderParts {
    collapsed: bool,
    icon: IconRef,
    name: String,
    dir: String,
    note: Option<&'static str>,
    counts: Option<(u32, u32)>,
    letter: char,
}

/// Section `ix`'s header: its file's name, its directory from the working
/// tree (a rename's old path after it), why it shows no diff, its counts
/// and its change's letter.
fn header_parts(diff: &CommitDiff, ix: usize, section: &Section) -> HeaderParts {
    let path = diff.display_path(&section.path);
    let (dir, name) = match path.rsplit_once('/') {
        Some((dir, name)) => (dir.to_owned(), name.to_owned()),
        None => (String::new(), path),
    };
    let dir = match &section.from {
        Some(from) if dir.is_empty() => format!("← {}", diff.display_path(from)),
        Some(from) => format!("{dir} ← {}", diff.display_path(from)),
        None => dir,
    };
    let letter = match (&section.from, section.kind) {
        (Some(_), _) => 'R',
        (None, ChangeKind::Added) => 'A',
        (None, ChangeKind::Deleted) => 'D',
        (None, _) => 'M',
    };
    HeaderParts {
        collapsed: diff.is_collapsed(ix),
        icon: file_icon_ref(&name),
        name,
        dir,
        note: match section.texts {
            Texts::NotShown(why) => Some(why.text()),
            _ => None,
        },
        counts: section.counts,
        letter,
    }
}

impl HeaderParts {
    /// Its counts' texts and colours: those that aren't 0.
    fn counts(&self, colors: &FileColors) -> Vec<(String, Hsla)> {
        let (added, removed) = self.counts.unwrap_or_default();
        [
            (added > 0).then(|| (format!("+{added}"), muted(colors.added))),
            (removed > 0).then(|| (format!("-{removed}"), muted(colors.deleted))),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    #[cfg(test)]
    fn seen(&self, colors: &FileColors) -> HeaderSeen {
        HeaderSeen {
            collapsed: self.collapsed,
            icon: self.icon.path(),
            name: self.name.clone(),
            dir: self.dir.clone(),
            dir_color: colors.muted,
            note: self.note.map(str::to_owned),
            counts: self.counts(colors),
            letter: self.letter,
            letter_color: colors.letter(self.letter),
        }
    }

    fn render(self, ix: usize, ui: &fe_ui::UiTheme, colors: &FileColors) -> Vec<AnyElement> {
        let chevron = match self.collapsed {
            true => Icon::ChevronRight,
            false => Icon::ChevronDown,
        };
        let chevron = div()
            .id(ElementId::NamedInteger(
                "commit-diff-chevron".into(),
                ix as u64,
            ))
            .debug_selector(move || format!("commit-diff-chevron:{ix}"))
            .flex_none()
            .child(svg().path(chevron.path()).size(px(14.)).text_color(ui.icon));
        let icon = div()
            .size(px(14.))
            .flex_none()
            .child(self.icon.element(px(14.), colors.icon));
        let name = (div().flex_none().max_w_1_2().overflow_hidden())
            .text_ellipsis()
            .font_weight(FontWeight::MEDIUM)
            .child(self.name.clone());
        let dir = (div().flex_1().min_w_0().overflow_hidden())
            .text_ellipsis()
            .text_color(colors.muted)
            .child(self.dir.clone());
        let note = (self.note).map(|note| div().flex_none().text_color(colors.muted).child(note));
        let counts = (self.counts(colors).into_iter())
            .map(|(text, color)| div().flex_none().text_color(color).child(text));
        let letter = (div().flex_none().w(LETTER_WIDTH).text_right())
            .text_color(colors.letter(self.letter))
            .child(self.letter.to_string());
        let mut parts = vec![
            chevron.into_any_element(),
            icon.into_any_element(),
            name.into_any_element(),
            dir.into_any_element(),
        ];
        parts.extend(note.map(IntoElement::into_any_element));
        parts.extend(counts.map(IntoElement::into_any_element));
        parts.push(letter.into_any_element());
        parts
    }
}

impl Render for CommitDiffView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            let mut seen = self.seen.borrow_mut();
            seen.rows.clear();
            seen.room.clear();
        }
        let ui = fe_ui::ui_theme(cx);
        let theme = self.theme(cx);
        let mut key_context = KeyContext::default();
        key_context.add(KEY_CONTEXT);
        let toolbar = self.toolbar(cx);
        let body = match self.diff.read(cx).message() {
            Some(message) => (div().flex_1().p_4().text_color(ui.muted_text))
                .child(message.to_owned())
                .into_any_element(),
            None => uniform_list(
                "commit-diff-rows",
                self.rows.len(),
                cx.processor(|view, range, window, cx| view.render_rows(range, window, cx)),
            )
            .track_scroll(&self.scroll)
            .flex_1()
            // A scroll by hand: what navigation went to is no longer held.
            .on_scroll_wheel(cx.listener(|view, _: &ScrollWheelEvent, _, _| view.let_go()))
            .into_any_element(),
        };
        div()
            .key_context(key_context)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(ui.text)
            .child(toolbar)
            .child(body)
    }
}

impl Focusable for CommitDiffView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PaneItem for CommitDiffView {
    const KIND: &'static str = "fastedit.commit_diff_view";

    fn tab_title(&self, cx: &App) -> SharedString {
        self.title(cx).into()
    }

    fn window_title(&self, cx: &App) -> SharedString {
        format!("{} — fastedit", self.title(cx)).into()
    }

    // Not split or saved in a layout: the Graph opens it.
    fn serialize(&self, _: &App) -> Option<fe_command::Value> {
        None
    }
}

/// The number of hunks in `lines`: its runs of removed and added rows.
fn hunk_count(lines: &[Line]) -> usize {
    let changed = |line: &Line| matches!(line, Line::Removed(_) | Line::Added(_));
    (lines.iter().enumerate())
        .filter(|&(ix, line)| changed(line) && (ix == 0 || !changed(&lines[ix - 1])))
        .count()
}

impl Workbench {
    /// This window's view of commit `id`'s change, read from the
    /// repository whose working tree is `workdir`, if one is open: it, its
    /// pane and its tab's index there.
    pub(crate) fn commit_diff_tab(
        &self,
        id: &str,
        workdir: &Path,
        cx: &App,
    ) -> Option<(Entity<CommitDiffView>, Entity<Pane>, usize)> {
        self.panes(cx).into_iter().find_map(|pane| {
            let tabs = pane.read(cx).tabs();
            tabs.iter().enumerate().find_map(|(ix, tab)| {
                let view = tab.item().downcast_ref::<CommitDiffView>()?.clone();
                (view.read(cx).is_of(id, workdir, cx)).then(|| (view, pane.clone(), ix))
            })
        })
    }

    /// Opens `commit`'s change to all its files, read from `repo`, in a tab
    /// of the active pane, focused, or shows the one open.
    pub(crate) fn open_commit_diff(
        &mut self,
        repo: Arc<Repo>,
        commit: LogCommit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<CommitDiffView> {
        let workdir = repo.workdir().to_owned();
        if let Some((view, pane, ix)) = self.commit_diff_tab(&commit.commit.id, &workdir, cx) {
            self.show_commit_diff_tab(pane, ix, window, cx);
            return view;
        }
        let workbench = cx.weak_entity();
        let diff = cx.new(|cx| CommitDiff::new(repo, commit, cx));
        let view = cx.new(|cx| CommitDiffView::new(workbench, diff, cx));
        let pane = self.active.clone();
        let ix = pane.update(cx, |pane, cx| {
            let ix = pane.pane_mut().push(Box::new(view.clone()), cx);
            pane.items_changed(cx);
            cx.notify();
            ix
        });
        self.show_commit_diff_tab(pane, ix, window, cx);
        view
    }

    /// Shows the view of commit `id` read from the repository whose working
    /// tree is `workdir`, if one is open, scrolled to the section of `path`:
    /// whether it was.
    pub(crate) fn reveal_in_commit_diff(
        &mut self,
        (id, workdir): (&str, &Path),
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((view, pane, ix)) = self.commit_diff_tab(id, workdir, cx) else {
            return false;
        };
        self.show_commit_diff_tab(pane, ix, window, cx);
        view.update(cx, |view, cx| view.reveal(path, cx));
        true
    }

    /// Shows tab `ix` of `pane`, focused.
    fn show_commit_diff_tab(
        &mut self,
        pane: Entity<Pane>,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate(pane.clone(), window, cx);
        pane.update(cx, |pane, cx| pane.activate(ix, window, cx));
    }
}

/// What the tests see of the view.
#[cfg(test)]
impl CommitDiffView {
    pub(crate) fn diff(&self) -> &Entity<CommitDiff> {
        &self.diff
    }

    pub(crate) fn seen(&self) -> Rc<RefCell<Seen>> {
        self.seen.clone()
    }

    pub(crate) fn viewport(&self) -> Range<usize> {
        self.viewport.clone()
    }

    pub(crate) fn header_row(&self, section: usize) -> Option<usize> {
        self.headers.get(section).copied()
    }

    /// The change navigation last went to: its section and hunk, `None`
    /// for a section's header.
    pub(crate) fn current_change(&self) -> Option<(usize, Option<usize>)> {
        let (section, hunk) = self.at?;
        Some((section, usize::try_from(hunk).ok()))
    }

    /// The row a section's hunk starts at.
    pub(crate) fn hunk_row(&self, section: usize, hunk: usize) -> Option<usize> {
        let stop = (self.stops.iter()).find(|stop| stop.key() == (section, hunk as i64))?;
        Some(stop.row)
    }

    pub(crate) fn list_bounds(&self) -> gpui::Bounds<Pixels> {
        self.scroll.0.borrow().base_handle.bounds()
    }
}

#[cfg(test)]
#[path = "commit_diff_view_tests.rs"]
mod commit_diff_view_tests;
