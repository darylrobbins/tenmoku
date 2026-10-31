//! A commit's change to all its files as one model, for the view that shows
//! it as one scrolling list, as VS Code's multi-file diff editor does: a
//! section a changed file, in the commit's file order ([`Repo::commit_files`],
//! against its first parent), each with its change's kind, its path (and a
//! rename's old one), its lines added and removed ([`Repo::commit_stats`]),
//! its two texts (the left empty for an added file, the right for a deleted
//! one), their row diff ([`TextDiff`]) and whether it's collapsed. The Graph
//! hands a commit to it ([`GitLogPanel::commit_diff_for`]).
//!
//! The model lays its sections out as rows ([`Row`]): each section's header,
//! then, unless it's collapsed, its diff in one column as `git diff` shows
//! it: each hunk's removed rows then its added rows, [`CONTEXT_ROWS`]
//! unchanged rows kept around each as the diff view keeps them, and the rest
//! of each unchanged run folded into one row saying how many it hides
//! ([`Line::Hidden`]). A text's empty last row, after its last line break,
//! isn't one.
//!
//! The files and counts are read on the background executor when the model
//! is made; until they land it has no rows and says `Loading…`, or the error
//! ([`CommitDiff::message`]). The texts are read and diffed a section at a
//! time on the background executor too, at most [`READS`] at once, and only
//! for sections within [`NEAR`] rows of the rows the view shows
//! ([`CommitDiff::set_viewport`]), nearest first, so a commit of hundreds of
//! files lays out without reading them all. Until its texts land a section
//! shows one `Loading…` row under its header; a failed read shows one muted
//! error row. A binary file (one git counts no lines of, or whose blob reads
//! as binary) or one over the diff view's limit ([`MAX_DIFF_BYTES`]) is its
//! header alone, saying it's not shown.
//!
//! Collapsing a section keeps its header and drops its rows; a collapsed
//! section's texts aren't read until it's expanded. Which sections are
//! collapsed is kept by path, through [`CommitDiff::reload`] and showing the
//! same commit again ([`CommitDiff::show`]).
//!
//! [`GitLogPanel::commit_diff_for`]: super::GitLogPanel::commit_diff_for

use std::{
    collections::HashSet,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_editor::text_diff::{MAX_DIFF_BYTES, TextDiff};
use fe_git::{ChangeKind, CommitFile, CommitStats, LogCommit, Repo};
use fe_gpui::diff_view::CONTEXT_ROWS;
use gpui::{AppContext as _, Context};

/// The sections' texts read at once.
pub(crate) const READS: usize = 4;

/// How many rows beyond the viewport's either end a section's texts are
/// read for.
pub(crate) const NEAR: usize = 40;

/// What a section, or the whole commit, shows until it's read.
pub(crate) const LOADING: &str = "Loading…";

/// Why a section shows no diff, after its header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotShown {
    /// Git counts no lines of it, or a side's blob reads as binary.
    Binary,
    /// A side is over [`MAX_DIFF_BYTES`].
    TooLarge,
}

impl NotShown {
    pub(crate) fn text(self) -> &'static str {
        match self {
            NotShown::Binary => "Binary file not shown",
            NotShown::TooLarge => "Large file not shown",
        }
    }
}

/// One text of a section, split into rows as [`TextDiff`] splits it, less
/// the empty row after a last line break.
#[derive(Debug, Default)]
pub(crate) struct SideText {
    text: String,
    /// Each row's bytes, its line break left out.
    rows: Vec<Range<usize>>,
}

impl SideText {
    fn new(text: String) -> Self {
        let bytes = text.as_bytes();
        let (mut rows, mut start, mut ix) = (Vec::new(), 0, 0);
        while ix < bytes.len() {
            let width = match (bytes[ix], bytes.get(ix + 1)) {
                (b'\r', Some(b'\n')) => 2,
                (b'\r' | b'\n', _) => 1,
                _ => 0,
            };
            if width == 0 {
                ix += 1;
                continue;
            }
            rows.push(start..ix);
            ix += width;
            start = ix;
        }
        if start < bytes.len() {
            rows.push(start..bytes.len());
        }
        Self { text, rows }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// How many rows it shows.
    pub(crate) fn row_count(&self) -> u32 {
        self.rows.len() as u32
    }

    /// Row `row`'s text, without its line break.
    pub(crate) fn row(&self, row: u32) -> &str {
        (self.rows.get(row as usize)).map_or("", |bytes| &self.text[bytes.clone()])
    }
}

/// A row of a section's diff, in rows of each side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// A row both sides have.
    Same { left: u32, right: u32 },
    /// A left row the commit removed.
    Removed(u32),
    /// A right row the commit added.
    Added(u32),
    /// Unchanged rows folded away, as many on each side.
    Hidden { left: Range<u32>, right: Range<u32> },
}

/// A section's texts and the rows of their diff.
#[derive(Debug)]
pub(crate) struct Diffed {
    pub(crate) left: SideText,
    pub(crate) right: SideText,
    pub(crate) lines: Vec<Line>,
}

impl Diffed {
    fn new(left: String, right: String) -> Self {
        let diff = TextDiff::new(&left, &right);
        let (left, right) = (SideText::new(left), SideText::new(right));
        let lines = lines(&diff, [left.row_count(), right.row_count()]);
        Self { left, right, lines }
    }

    /// Line `line`'s text: a row's, or how many rows it hides.
    pub(crate) fn line_text(&self, line: &Line) -> String {
        match line {
            Line::Same { left, .. } | Line::Removed(left) => self.left.row(*left).to_owned(),
            Line::Added(right) => self.right.row(*right).to_owned(),
            Line::Hidden { left, .. } => match left.len() {
                1 => "1 hidden line".to_owned(),
                rows => format!("{rows} hidden lines"),
            },
        }
    }
}

/// The rows of the diff `diff` of texts of `rows` rows (left, right): see
/// the module docs.
fn lines(diff: &TextDiff, rows: [u32; 2]) -> Vec<Line> {
    let mut lines = Vec::new();
    let (mut left, mut right) = (0, 0);
    let hunks = diff.hunks();
    for (ix, hunk) in hunks.iter().enumerate() {
        let len = hunk.left.start.min(rows[0]).saturating_sub(left);
        unchanged(&mut lines, (left, right), len, [ix > 0, true]);
        let removed = hunk.left.start..hunk.left.end.min(rows[0]);
        lines.extend(removed.map(Line::Removed));
        let added = hunk.right.start..hunk.right.end.min(rows[1]);
        lines.extend(added.map(Line::Added));
        (left, right) = (hunk.left.end, hunk.right.end);
    }
    let tail = rows[0]
        .saturating_sub(left)
        .min(rows[1].saturating_sub(right));
    unchanged(&mut lines, (left, right), tail, [!hunks.is_empty(), false]);
    lines
}

/// Adds the rows of the `len` unchanged rows from `(left, right)`, a hunk
/// above and below them as `hunks` says: those next to a hunk, the rest
/// folded.
fn unchanged(lines: &mut Vec<Line>, (left, right): (u32, u32), len: u32, hunks: [bool; 2]) {
    let [head, foot] = hunks.map(|hunk| if hunk { CONTEXT_ROWS } else { 0 });
    let right_of = |row: u32| row - left + right;
    let same = |rows: Range<u32>| {
        rows.map(move |row| Line::Same {
            left: row,
            right: right_of(row),
        })
    };
    if len <= head + foot {
        return lines.extend(same(left..left + len));
    }
    lines.extend(same(left..left + head));
    let hidden = left + head..left + len - foot;
    lines.push(Line::Hidden {
        right: right_of(hidden.start)..right_of(hidden.end),
        left: hidden,
    });
    lines.extend(same(left + len - foot..left + len));
}

/// What a section's texts are.
#[derive(Debug)]
pub(crate) enum Texts {
    /// Not asked for yet.
    Unread,
    /// Being read by the request numbered so.
    Loading(u64),
    Failed(String),
    NotShown(NotShown),
    Read(Arc<Diffed>),
}

/// One changed file's section.
#[derive(Debug)]
pub(crate) struct Section {
    /// Absolute: the file's path in the commit (for a deletion, the path it
    /// had).
    pub(crate) path: PathBuf,
    /// Absolute: its path in the first parent, when the commit renamed it.
    pub(crate) from: Option<PathBuf>,
    pub(crate) kind: ChangeKind,
    /// Lines added and removed, once read; `None` for a binary file.
    pub(crate) counts: Option<(u32, u32)>,
    pub(crate) texts: Texts,
}

/// A row of the layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Row {
    /// Section `0`'s header.
    Header(usize),
    /// Section `0`'s texts aren't read yet.
    Loading(usize),
    /// Section `0`'s texts couldn't be read: its muted error row.
    Failed(usize),
    /// Line `1` of section `0`'s [`Diffed::lines`].
    Line(usize, usize),
}

/// Where the commit's files are.
#[derive(Debug, PartialEq, Eq)]
enum Listing {
    /// Being read by the request numbered so.
    Loading(u64),
    Failed(String),
    Read,
}

/// What a section's read found.
enum Read {
    Texts(Diffed),
    NotShown(NotShown),
}

/// A commit's multi-file diff: see the module docs.
pub(crate) struct CommitDiff {
    repo: Arc<Repo>,
    commit: LogCommit,
    listing: Listing,
    sections: Vec<Section>,
    /// The collapsed sections' paths.
    collapsed: HashSet<PathBuf>,
    rows: Vec<Row>,
    /// Each section's rows in `rows`.
    spans: Vec<Range<usize>>,
    /// The rows the view shows.
    viewport: Range<usize>,
    /// The sections' reads under way.
    reading: usize,
    /// The last read's number: only the one a `Loading` names lands.
    requests: u64,
    /// The sections whose reads were started, in order.
    #[cfg(test)]
    started: Vec<usize>,
}

impl CommitDiff {
    /// The diff of `commit`, read from `repo`.
    pub(crate) fn new(repo: Arc<Repo>, commit: LogCommit, cx: &mut Context<Self>) -> Self {
        let mut diff = Self {
            repo,
            commit,
            listing: Listing::Read,
            sections: Vec::new(),
            collapsed: HashSet::new(),
            rows: Vec::new(),
            spans: Vec::new(),
            viewport: 0..0,
            reading: 0,
            requests: 0,
            #[cfg(test)]
            started: Vec::new(),
        };
        diff.load(cx);
        diff
    }

    /// Shows `commit` instead, read again; the sections collapsed stay so
    /// if it's the same commit.
    pub(crate) fn show(&mut self, commit: LogCommit, cx: &mut Context<Self>) {
        if commit.commit.id != self.commit.commit.id {
            self.collapsed.clear();
        }
        self.commit = commit;
        self.load(cx);
    }

    /// Reads the commit's files and texts again, the sections collapsed
    /// staying so.
    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        self.load(cx);
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.requests += 1;
        let request = self.requests;
        self.listing = Listing::Loading(request);
        self.sections.clear();
        self.reading = 0;
        self.relayout();
        let (repo, id) = (self.repo.clone(), self.commit.commit.id.clone());
        let read = cx.background_spawn(async move {
            let files = (repo.commit_files(&id)).map_err(|error| error.to_string());
            (files, repo.commit_stats(&id).ok())
        });
        cx.spawn(async move |diff, cx| {
            let (files, stats) = read.await;
            (diff.update(cx, |diff, cx| diff.land_files(request, files, stats, cx))).ok();
        })
        .detach();
        cx.notify();
    }

    /// Makes a section of each file read by request `request`, if it's
    /// still the last, with its counts in `stats`.
    fn land_files(
        &mut self,
        request: u64,
        files: Result<Vec<CommitFile>, String>,
        stats: Option<CommitStats>,
        cx: &mut Context<Self>,
    ) {
        if self.listing != Listing::Loading(request) {
            return;
        }
        let files = match files {
            Ok(files) => files,
            Err(error) => {
                self.listing = Listing::Failed(error);
                return cx.notify();
            }
        };
        let stats = stats.map(|stats| stats.files).unwrap_or_default();
        self.sections = (files.into_iter())
            .map(|file| {
                let stat = stats.iter().find(|stat| stat.path == file.path);
                let counts = stat.and_then(|stat| Some((stat.added?, stat.removed?)));
                let binary = stat.is_some() && counts.is_none();
                Section {
                    texts: match binary {
                        true => Texts::NotShown(NotShown::Binary),
                        false => Texts::Unread,
                    },
                    path: file.path,
                    from: file.from,
                    kind: file.kind,
                    counts,
                }
            })
            .collect();
        self.listing = Listing::Read;
        self.relayout();
        self.schedule(cx);
        cx.notify();
    }

    /// Notes that the view shows `rows` of the layout, reading the texts of
    /// the sections near them.
    pub(crate) fn set_viewport(&mut self, rows: Range<usize>, cx: &mut Context<Self>) {
        self.viewport = rows;
        self.schedule(cx);
    }

    /// Starts reading the texts of the unread, expanded sections within
    /// [`NEAR`] rows of the viewport, nearest first, while fewer than
    /// [`READS`] are under way.
    fn schedule(&mut self, cx: &mut Context<Self>) {
        if self.reading >= READS {
            return;
        }
        let (top, bottom) = (
            self.viewport.start,
            self.viewport.end.max(self.viewport.start + 1),
        );
        let mut near: Vec<(usize, usize)> = (self.spans.iter().enumerate())
            .filter(|&(ix, _)| {
                matches!(self.sections[ix].texts, Texts::Unread) && !self.is_collapsed(ix)
            })
            .map(|(ix, span)| {
                let distance =
                    (top.saturating_sub(span.end - 1)).max(span.start.saturating_sub(bottom - 1));
                (distance, ix)
            })
            .filter(|&(distance, _)| distance <= NEAR)
            .collect();
        near.sort_unstable();
        for (_, ix) in near.into_iter().take(READS - self.reading) {
            self.read_texts(ix, cx);
        }
    }

    /// Reads and diffs section `ix`'s texts on the background executor.
    fn read_texts(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.requests += 1;
        let request = self.requests;
        self.reading += 1;
        #[cfg(test)]
        self.started.push(ix);
        let section = &mut self.sections[ix];
        section.texts = Texts::Loading(request);
        let file = CommitFile {
            path: section.path.clone(),
            from: section.from.clone(),
            kind: section.kind,
        };
        let repo = self.repo.clone();
        let id = self.commit.commit.id.clone();
        let parent = self.commit.parents.first().cloned();
        let read = cx.background_spawn(async move { read(&repo, &id, parent.as_deref(), &file) });
        cx.spawn(async move |diff, cx| {
            let read = read.await;
            (diff.update(cx, |diff, cx| diff.land_texts(ix, request, read, cx))).ok();
        })
        .detach();
    }

    /// Notes section `ix`'s texts as request `request` read them, if it's
    /// still the one reading them.
    fn land_texts(
        &mut self,
        ix: usize,
        request: u64,
        read: Result<Read, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(section) = self.sections.get_mut(ix) else {
            return;
        };
        if !matches!(section.texts, Texts::Loading(at) if at == request) {
            return;
        }
        self.reading -= 1;
        section.texts = match read {
            Ok(Read::Texts(diffed)) => Texts::Read(Arc::new(diffed)),
            Ok(Read::NotShown(why)) => Texts::NotShown(why),
            Err(error) => Texts::Failed(error),
        };
        self.relayout();
        self.schedule(cx);
        cx.notify();
    }

    /// Lays the sections out as rows: see the module docs.
    fn relayout(&mut self) {
        self.rows.clear();
        self.spans.clear();
        for (ix, section) in self.sections.iter().enumerate() {
            let start = self.rows.len();
            self.rows.push(Row::Header(ix));
            if !self.collapsed.contains(&section.path) {
                match &section.texts {
                    Texts::Unread | Texts::Loading(_) => self.rows.push(Row::Loading(ix)),
                    Texts::Failed(_) => self.rows.push(Row::Failed(ix)),
                    Texts::NotShown(_) => {}
                    Texts::Read(diffed) => {
                        (self.rows).extend((0..diffed.lines.len()).map(|line| Row::Line(ix, line)))
                    }
                }
            }
            self.spans.push(start..self.rows.len());
        }
    }

    /// Collapses section `ix`, or expands it.
    pub(crate) fn toggle(&mut self, ix: usize, cx: &mut Context<Self>) {
        let collapsed = !self.is_collapsed(ix);
        self.set_collapsed(ix, collapsed, cx);
    }

    /// Collapses section `ix` if `collapsed`, else expands it.
    pub(crate) fn set_collapsed(&mut self, ix: usize, collapsed: bool, cx: &mut Context<Self>) {
        let Some(section) = self.sections.get(ix) else {
            return;
        };
        match collapsed {
            true => self.collapsed.insert(section.path.clone()),
            false => self.collapsed.remove(&section.path),
        };
        self.collapsed_changed(cx);
    }

    /// Collapses every section.
    pub(crate) fn collapse_all(&mut self, cx: &mut Context<Self>) {
        let paths = self.sections.iter().map(|section| section.path.clone());
        self.collapsed.extend(paths);
        self.collapsed_changed(cx);
    }

    /// Expands every section.
    pub(crate) fn expand_all(&mut self, cx: &mut Context<Self>) {
        self.collapsed.clear();
        self.collapsed_changed(cx);
    }

    fn collapsed_changed(&mut self, cx: &mut Context<Self>) {
        self.relayout();
        self.schedule(cx);
        cx.notify();
    }

    pub(crate) fn is_collapsed(&self, ix: usize) -> bool {
        (self.sections.get(ix)).is_some_and(|section| self.collapsed.contains(&section.path))
    }

    pub(crate) fn commit(&self) -> &LogCommit {
        &self.commit
    }

    pub(crate) fn sections(&self) -> &[Section] {
        &self.sections
    }

    pub(crate) fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// What shows in place of the rows: `Loading…` until the files are
    /// read, or why they couldn't be.
    pub(crate) fn message(&self) -> Option<&str> {
        match &self.listing {
            Listing::Loading(_) => Some(LOADING),
            Listing::Failed(error) => Some(error),
            Listing::Read => None,
        }
    }

    /// `path` from the repository's working tree.
    fn display_path(&self, path: &Path) -> String {
        let relative = path.strip_prefix(self.repo.workdir()).unwrap_or(path);
        relative.display().to_string()
    }

    /// Section `ix`'s header's text: its path, a rename's old path before
    /// it, then why it's not shown if it isn't.
    pub(crate) fn header_text(&self, ix: usize) -> Option<String> {
        let section = self.sections.get(ix)?;
        let mut text = self.display_path(&section.path);
        if let Some(from) = &section.from {
            text = format!("{} → {text}", self.display_path(from));
        }
        if let Texts::NotShown(why) = section.texts {
            text = format!("{text} · {}", why.text());
        }
        Some(text)
    }

    /// The text of row `row` of the layout.
    pub(crate) fn row_text(&self, row: usize) -> Option<String> {
        match *self.rows.get(row)? {
            Row::Header(ix) => self.header_text(ix),
            Row::Loading(_) => Some(LOADING.to_owned()),
            Row::Failed(ix) => match &self.sections[ix].texts {
                Texts::Failed(error) => Some(error.clone()),
                _ => None,
            },
            Row::Line(ix, line) => match &self.sections[ix].texts {
                Texts::Read(diffed) => Some(diffed.line_text(&diffed.lines[line])),
                _ => None,
            },
        }
    }

    /// The sections whose reads were started, in order.
    #[cfg(test)]
    pub(crate) fn started(&self) -> &[usize] {
        &self.started
    }
}

/// `file`'s texts at `commit` and its first parent `parent`, and their
/// diff, or why they aren't shown. Runs on the background executor.
fn read(
    repo: &Repo,
    commit: &str,
    parent: Option<&str>,
    file: &CommitFile,
) -> Result<Read, String> {
    let text_at = |at: &str, path: &Path| repo.text_at(at, path).map_err(|error| error.to_string());
    let before = file.from.as_deref().unwrap_or(&file.path);
    let left = match (file.kind, parent) {
        (ChangeKind::Added, _) | (_, None) => Some(String::new()),
        (_, Some(parent)) => text_at(parent, before)?,
    };
    let right = match file.kind {
        ChangeKind::Deleted => Some(String::new()),
        _ => text_at(commit, &file.path)?,
    };
    // Where a side should have a text, none is a binary blob's.
    let (Some(left), Some(right)) = (left, right) else {
        return Ok(Read::NotShown(NotShown::Binary));
    };
    if left.len() > MAX_DIFF_BYTES || right.len() > MAX_DIFF_BYTES {
        return Ok(Read::NotShown(NotShown::TooLarge));
    }
    Ok(Read::Texts(Diffed::new(left, right)))
}

#[cfg(test)]
#[path = "commit_diff_tests.rs"]
mod commit_diff_tests;
