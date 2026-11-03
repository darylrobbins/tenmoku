//! The Problems panel on the right dock: the language servers' diagnostics
//! of every buffer open in the window, once per file however many splits
//! show it, grouped by file. A file's row has its path, from the root if
//! it's under it, and how many problems it has; one row per problem
//! follows, with its severity, message, source and `row:col`, then an
//! indented row per location it's related to, with its message and
//! `file:row` (`row` alone in the problem's file). Clicking a problem, or
//! `problems.open` on the selected one, activates the file's editor (the
//! active pane's, else the first pane's showing it) and selects the
//! problem's range; a related location in the same file opens the same way,
//! and one in another file opens it, in the active pane if no pane shows
//! it, with the caret at its row's start. `problems.select_next` and
//! `problems.select_previous` move the selection over the problems and
//! their related rows, and `problems.select_first` and
//! `problems.select_last` go to the first and the last of them.
//!
//! The workbench lists the problems again whenever an open editor changes,
//! as a diagnostic arriving or clearing notifies it, and as tabs open and
//! close ([`Workbench::sync_problems`]). It keeps the window's error and
//! warning counts in a status bar item on the left, [`ITEM`], while there
//! are any; a click on it runs `problems.show`.
//!
//! `diagnostics.next` and `diagnostics.previous` put the active editor's
//! caret at the start of the next or previous diagnostic, wrapping
//! ([`fe_editor::EditorModel::next_diagnostic`]), and reveal it.
//!
//! With vim on, the panel's key context has the `vim` flag, for keys bound
//! to its commands in vim's style; opening a problem leaves the editor in
//! normal mode, the caret at the problem's start.
//!
//! A file no editor shows is listed too, as VS Code lists it, while a
//! server has diagnostics for it (`LspEvent::FileDiagnostics`, kept on the
//! workbench's LSP state until the server sends none for the file, restarts
//! or stops). Its rows are decoded against its text on disk, read off the
//! main thread with [`loader::read`] once per change to the server's
//! diagnostics for it or to the file (its editor closing): until the read
//! lands, they show their rows without a column, and a file that can't be
//! read (gone, binary, larger than [`fe_lsp::DEFAULT_MAX_FILE_BYTES`]) shows
//! each position as the server sent it. They count in the status items. A
//! file an editor shows is listed from its buffer alone, so no problem is
//! listed twice. Opening such a row opens the file in the active pane, as
//! quick open does, and selects the problem's range decoded against the
//! opened buffer's text.
//!
//! A finished task's problems are listed too ([`crate::task_run`]): a source
//! per task tab, each replaced by the tab's next run and gone with the tab
//! ([`ProblemsPanel::set_source`]). They're panel rows alone, merged by
//! position with the file's other problems, labelled with the task's name and
//! counted in the status items; no editor marks them. They show the row and
//! column the tool printed, and sort and open by where that is in the file's
//! text, in the unit the tool counts columns in ([`ColumnUnit`]).
//!
//! The commands are the workbench's typed commands
//! (`workbench_commands.rs`), which do a [`ProblemsOp`]: `problems.show`,
//! `diagnostics.next` and `diagnostics.previous` in the workbench's
//! context, the others in the panel's [`KEY_CONTEXT`].

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
};

use fe_command::Invocation;
use fe_editor::diagnostics::{
    DiagnosticCounts, FileProblems, Problem, RelatedInfo, Severity, group_problems,
};
use fe_gpui::{EditorTheme, EditorView, commands::RunCommand};
use fe_lsp::{FileDiagnostic, PositionEncoding, decode_file_diagnostics};
use fe_text::Text;
use fe_ui::ROW_HEIGHT;
use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, KeyContext, ParentElement as _, Render, ScrollStrategy,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    UniformListScrollHandle, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    uniform_list,
};

use crate::{Pane, Workbench, loader, pane::Reveal, task_run::TaskTabs};
use kiln_shell::{Icon, StatusItem, StatusSide, StatusTone};

/// The key context the panel's element sets.
pub(crate) const KEY_CONTEXT: &str = "problems_panel";

/// The flag the panel's key context has while vim is on.
const VIM_FLAG: &str = "vim";

/// The status item's id.
pub(crate) const ITEM: &str = "problems";

/// The warnings' status item's id, beside [`ITEM`], the errors'.
pub(crate) const WARNINGS_ITEM: &str = "problems.warnings";

/// The command that shows the panel, which the status item runs.
pub(crate) const SHOW: &str = "problems.show";

/// The problems in the sidebar: on the rail once it has shown; focused when
/// shown, for its commands. The icon bundle has no warning sign: an error's
/// cross.
pub(crate) const SIDEBAR: crate::dock::SidebarPanel = crate::dock::SidebarPanel {
    panel: crate::DockPanel::new("problems"),
    name: "Problems",
    icon: fe_ui::Icon::X,
    show_command: SHOW,
    show_op: crate::dock::DockOp::Problems(ProblemsOp::Show),
    order: 60,
    always_on_rail: false,
    focus_on_show: true,
};

/// The errors' item's priority: on the left, after the config problems'; the
/// warnings' is the next.
const PRIORITY: i32 = 60;

/// What a problems command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProblemsOp {
    /// `problems.show`: shows the dock with the panel, focused.
    Show,
    SelectNext,
    SelectPrevious,
    /// Selects the first problem or related row.
    SelectFirst,
    /// Selects the last problem or related row.
    SelectLast,
    /// Opens the selected problem.
    Open,
    /// `diagnostics.next`, on the active editor.
    Next,
    /// `diagnostics.previous`, on the active editor.
    Previous,
}

/// A row: a file's, one of its problems, or one of a problem's related
/// locations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    File(usize),
    Problem(usize, usize),
    Related(usize, usize, usize),
}

/// Where opening a row goes.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    /// A range of a file shown in a pane.
    Range(PathBuf, Range<usize>),
    /// A row's start, in a file that may not be open.
    Row(PathBuf, u32),
    /// A diagnostic as a server sent it, in a file no editor showed.
    Sent(PathBuf, FileDiagnostic, PositionEncoding),
    /// A row and column a task's output named, in a file that may not be open.
    Position(PathBuf, u32, u32, ColumnUnit),
}

/// What a tool's columns count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColumnUnit {
    Bytes,
    Utf16,
    /// Unicode scalar values.
    Chars,
    /// Terminal cells: wide characters take two, a tab runs to the next
    /// multiple of [`TAB_STOP`].
    Display,
}

/// Where a tool counting [`ColumnUnit::Display`] puts its tab stops.
const TAB_STOP: u32 = 8;

/// The offset in `text` of zero-based `column`, counted in `unit`, on `row`:
/// the text's end for a row past it, the row's end for a column past that.
pub(crate) fn column_offset(text: &Text, row: u32, column: u32, unit: ColumnUnit) -> usize {
    if row >= text.row_count() {
        return text.len();
    }
    let start = text.row_start(row);
    let line = text.row_to_string(row);
    let mut counted = 0;
    for (ix, ch) in line.char_indices() {
        let width = match unit {
            ColumnUnit::Bytes => ch.len_utf8() as u32,
            ColumnUnit::Utf16 => ch.len_utf16() as u32,
            ColumnUnit::Chars => 1,
            ColumnUnit::Display if ch == '\t' => TAB_STOP - counted % TAB_STOP,
            ColumnUnit::Display => unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1) as u32,
        };
        // A mark of no width belongs to the character before it.
        if counted >= column && width > 0 {
            return start + ix;
        }
        counted += width;
    }
    start + line.len()
}

/// Where the problems of a file go that aren't all its buffer's own: one no
/// editor shows, whose diagnostics are as the servers sent them, or one a
/// task's problems are listed in. In its problems' order; whether its text is
/// still being read, so its problems' columns aren't known.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Sent {
    targets: Vec<Target>,
    reading: bool,
}

/// A problem a task's output named, at the row and column the tool printed,
/// zero-based, the column counted in `unit`. `file_column` is that column in
/// bytes of the file's text as the task's output was read, the printed one if
/// the file couldn't be: what it sorts by while no editor has the file's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceProblem {
    pub(crate) path: PathBuf,
    pub(crate) problem: Problem,
    pub(crate) unit: ColumnUnit,
    pub(crate) file_column: u32,
}

/// Zero-based `column` of `row`, counted in `unit`, in bytes of `text`.
pub(crate) fn byte_column(text: &Text, row: u32, column: u32, unit: ColumnUnit) -> u32 {
    let offset = column_offset(text, row, column, unit);
    (offset - text.row_start(row).min(offset)) as u32
}

/// The files the servers have diagnostics for that no editor shows, by
/// resolved path: each one's text on disk and its rows, made once per change
/// to either. Forgotten as an editor shows the file, so it's read again
/// once none does: it may have been saved.
#[derive(Default)]
pub(crate) struct Unopened {
    files: HashMap<PathBuf, UnopenedFile>,
    /// Selects the problem a row opened, once its file is shown.
    opening: Option<Subscription>,
}

struct UnopenedFile {
    /// `None` while it's read; then `None` inside if it can't be.
    text: Option<Option<Text>>,
    /// The read: dropped, it's dropped too.
    _read: Task<()>,
    /// Its rows, decoded against `text`, as listed: `None` until made.
    rows: Option<(Vec<Problem>, Sent)>,
}

impl Unopened {
    /// Drops what's known of `path`'s file, for a change to its
    /// diagnostics.
    pub(crate) fn forget(&mut self, path: &Path) {
        self.files.remove(path);
    }
}

/// `path`'s text, as an editor would load it, unless it's gone, can't be
/// read, is larger than a server takes or is binary (has a NUL). Blocking.
pub(crate) fn read_text(path: PathBuf) -> Option<Text> {
    let len = std::fs::metadata(&path).ok()?.len();
    if len > fe_lsp::DEFAULT_MAX_FILE_BYTES as u64 {
        return None;
    }
    let buffer = loader::read(path).ok()??;
    let text = buffer.text().clone();
    let binary = text.chunks().any(|chunk| chunk.contains('\0'));
    (!binary).then_some(text)
}

/// `items` as problems of the file listed as `path`, decoded against `text`;
/// without it, at each position as sent, the line and character as the row and
/// column. By position, as an open file's.
fn sent_problems(
    path: &Path,
    text: Option<&Text>,
    items: Vec<(FileDiagnostic, PositionEncoding)>,
    reading: bool,
) -> (Vec<Problem>, Sent) {
    let mut problems: Vec<(Problem, Target)> = (items.into_iter())
        .map(|(item, encoding)| {
            let problem = match text {
                Some(text) => {
                    let decoded =
                        decode_file_diagnostics(text, std::slice::from_ref(&item), encoding);
                    let range = decoded.into_iter().next().map_or(0..0, |d| d.range);
                    let point = text.offset_to_point(range.start);
                    sent_problem(&item, range, point.row, point.column)
                }
                None => {
                    let start = item.range.start;
                    sent_problem(&item, 0..0, start.line, start.character)
                }
            };
            (problem, Target::Sent(path.to_owned(), item, encoding))
        })
        .collect();
    sort_by_position(&mut problems);
    let (problems, targets) = problems.into_iter().unzip();
    (problems, Sent { targets, reading })
}

/// In a file's listing order, the worst first at one position.
fn sort_by_position(problems: &mut [(Problem, Target)]) {
    problems.sort_by_key(|(p, _)| (p.row, p.column, std::cmp::Reverse(p.severity)));
}

/// Adds `sources`' problems to the `files` listed, among their rows by
/// position, or as files of their own. Other files stay as they were. Rows
/// count columns in bytes, so a task's sorts by where its column is in the
/// file's text: of `texts`, if it's there, else as it was read with the output.
fn merge_sources(
    files: &mut BTreeMap<PathBuf, (FileProblems, Option<Sent>)>,
    sources: &BTreeMap<u64, Vec<SourceProblem>>,
    texts: &HashMap<PathBuf, Text>,
) {
    let mut by_path: BTreeMap<&Path, Vec<&SourceProblem>> = BTreeMap::new();
    for problem in sources.values().flatten() {
        by_path.entry(&problem.path).or_default().push(problem);
    }
    for (path, listed) in by_path {
        let (file, sent) = files.entry(path.to_owned()).or_insert_with(|| {
            let path = path.to_owned();
            let problems = Vec::new();
            (FileProblems { path, problems }, None)
        });
        let sent = sent.get_or_insert_with(|| Sent {
            targets: (file.problems.iter())
                .map(|problem| Target::Range(path.to_owned(), problem.range.clone()))
                .collect(),
            reading: false,
        });
        let text = texts.get(path);
        let added = listed.into_iter().map(|source| {
            let problem = &source.problem;
            let (row, column) = (problem.row, problem.column);
            let target = Target::Position(path.to_owned(), row, column, source.unit);
            let bytes = text.map_or(source.file_column, |text| {
                byte_column(text, row, column, source.unit)
            });
            (bytes, problem.clone(), target)
        });
        let mut rows: Vec<_> = (std::mem::take(&mut file.problems).into_iter())
            .zip(std::mem::take(&mut sent.targets))
            .map(|(problem, target)| (problem.column, problem, target))
            .chain(added)
            .collect();
        rows.sort_by_key(|(bytes, problem, _)| {
            (problem.row, *bytes, std::cmp::Reverse(problem.severity))
        });
        (file.problems, sent.targets) = (rows.into_iter())
            .map(|(_, problem, target)| (problem, target))
            .unzip();
    }
}

/// `item` as a problem at `range`, starting at `row` and `column`: one with
/// no severity is an error, as for an open file.
fn sent_problem(item: &FileDiagnostic, range: Range<usize>, row: u32, column: u32) -> Problem {
    let severity = match item.severity {
        None | Some(fe_lsp::Severity::Error) => Severity::Error,
        Some(fe_lsp::Severity::Warning) => Severity::Warning,
        Some(fe_lsp::Severity::Information) => Severity::Information,
        Some(fe_lsp::Severity::Hint) => Severity::Hint,
    };
    Problem {
        range,
        row,
        column,
        severity,
        message: item.message.clone(),
        source: item.source.clone(),
        related: Vec::new(),
        code_href: None,
    }
}

pub struct ProblemsPanel {
    workbench: WeakEntity<Workbench>,
    focus: FocusHandle,
    /// What paths show relative to.
    root: Option<PathBuf>,
    /// By path; none without problems.
    files: Vec<FileProblems>,
    /// Each of `files`' diagnostics as sent, for one no editor shows.
    sent: Vec<Option<Sent>>,
    rows: Vec<Row>,
    /// A `Row::Problem`'s or a `Row::Related`'s index in `rows`.
    selected: Option<usize>,
    /// Bumped as the list changes, for the rows' element ids.
    generation: u64,
    /// The counts the status item shows, once it's set.
    item: Option<DiagnosticCounts>,
    scroll: UniformListScrollHandle,
    /// Each severity's colour, as the editor marks it.
    colors: [Hsla; 4],
    /// Each task tab's problems, by the key [`TaskTabs`] gave it.
    sources: BTreeMap<u64, Vec<SourceProblem>>,
    /// The task tabs those belong to.
    pub(crate) task_tabs: TaskTabs,
}

impl ProblemsPanel {
    /// An empty panel, opening problems through `workbench`, its severities
    /// in `theme`'s colours.
    pub(crate) fn new(workbench: WeakEntity<Workbench>, theme: &EditorTheme, cx: &mut App) -> Self {
        let colors = [
            Severity::Hint,
            Severity::Information,
            Severity::Warning,
            Severity::Error,
        ]
        .map(|severity| theme.diagnostic_color(severity));
        Self {
            workbench,
            focus: cx.focus_handle(),
            root: None,
            files: Vec::new(),
            sent: Vec::new(),
            rows: Vec::new(),
            selected: None,
            generation: 0,
            item: None,
            scroll: UniformListScrollHandle::new(),
            colors,
            sources: BTreeMap::new(),
            task_tabs: TaskTabs::default(),
        }
    }

    /// Replaces source `key`'s problems with `problems`, for the next
    /// [`Workbench::sync_problems`].
    pub(crate) fn set_source(&mut self, key: u64, problems: Vec<SourceProblem>) {
        self.sources.insert(key, problems);
    }

    /// Drops source `key`'s problems, for the next
    /// [`Workbench::sync_problems`].
    pub(crate) fn clear_source(&mut self, key: u64) {
        self.sources.remove(&key);
    }

    /// Lists `files`, with paths relative to `root`, each with its
    /// diagnostics as sent if no editor shows it, keeping the selected
    /// problem selected if it's still listed: the nearest with its message,
    /// severity and source (and the same related row of it, if it has
    /// one). The window's counts, if the status item doesn't show them yet.
    fn set_files(
        &mut self,
        root: Option<PathBuf>,
        files: Vec<(FileProblems, Option<Sent>)>,
        cx: &mut Context<Self>,
    ) -> Option<DiagnosticCounts> {
        let (files, sent): (Vec<_>, Vec<_>) = files.into_iter().unzip();
        let counts = counts(&files);
        let item = (self.item != Some(counts)).then(|| {
            self.item = Some(counts);
            counts
        });
        if files == self.files && sent == self.sent && root == self.root {
            return item;
        }
        let related = match self.selected.and_then(|ix| self.rows.get(ix)) {
            Some(Row::Related(_, _, related)) => Some(*related),
            _ => None,
        };
        let selected = self.selected_problem().map(|(path, problem)| {
            let path = path.to_owned();
            (path, problem.clone())
        });
        self.root = root;
        self.files = files;
        self.sent = sent;
        self.rows.clear();
        for (ix, file) in self.files.iter().enumerate() {
            self.rows.push(Row::File(ix));
            for (problem_ix, problem) in file.problems.iter().enumerate() {
                self.rows.push(Row::Problem(ix, problem_ix));
                (self.rows).extend(
                    (0..problem.related.len()).map(|related| Row::Related(ix, problem_ix, related)),
                );
            }
        }
        self.selected = selected.and_then(|(path, was)| {
            let file = self.files.iter().position(|file| file.path == path)?;
            let problems = &self.files[file].problems;
            // The same problem, unmoved; else the nearest saying the same.
            let same = |p: &Problem| {
                (p.severity, &p.message, &p.source) == (was.severity, &was.message, &was.source)
            };
            let problem = (problems.iter().position(|p| *p == was)).or_else(|| {
                let distance =
                    |p: &Problem| (p.row.abs_diff(was.row), p.column.abs_diff(was.column));
                (problems.iter().enumerate())
                    .filter(|(_, p)| same(p))
                    .min_by_key(|(_, p)| distance(p))
                    .map(|(ix, _)| ix)
            })?;
            let related = related.filter(|&ix| ix < problems[problem].related.len());
            let row = match related {
                Some(related) => Row::Related(file, problem, related),
                None => Row::Problem(file, problem),
            };
            self.rows.iter().position(|r| *r == row)
        });
        self.generation += 1;
        cx.notify();
        item
    }

    /// The selected problem, or the problem of the selected related row,
    /// and its file's path.
    fn selected_problem(&self) -> Option<(&Path, &Problem)> {
        let (Row::Problem(file, problem) | Row::Related(file, problem, _)) =
            *self.rows.get(self.selected?)?
        else {
            return None;
        };
        let file = &self.files[file];
        Some((&file.path, &file.problems[problem]))
    }

    /// The problem and related rows' indices in `rows`.
    fn selectable_rows(&self) -> Vec<usize> {
        (self.rows.iter().enumerate())
            .filter(|(_, row)| !matches!(row, Row::File(..)))
            .map(|(ix, _)| ix)
            .collect()
    }

    /// Moves the selection to the next problem or related row, or the
    /// previous, stopping at the last and the first; with none selected,
    /// selects the first, or the last.
    fn move_selection(&mut self, forward: bool, cx: &mut Context<Self>) {
        let problems = self.selectable_rows();
        let (Some(&first), Some(&last)) = (problems.first(), problems.last()) else {
            return;
        };
        let to = match self.selected {
            None if forward => first,
            None => last,
            Some(selected) if forward => (problems.iter().copied())
                .find(|&ix| ix > selected)
                .unwrap_or(last),
            Some(selected) => (problems.iter().rev().copied())
                .find(|&ix| ix < selected)
                .unwrap_or(first),
        };
        self.select(to, cx);
    }

    /// Selects the last problem or related row, if `last`, or else the
    /// first.
    fn select_end(&mut self, last: bool, cx: &mut Context<Self>) {
        let problems = self.selectable_rows();
        let end = if last {
            problems.last()
        } else {
            problems.first()
        };
        if let Some(&ix) = end {
            self.select(ix, cx);
        }
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Selects row `ix`, if it's still `generation`'s and a problem's or a
    /// related location's, and opens it.
    fn open(&mut self, generation: u64, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if generation != self.generation || matches!(self.rows.get(ix), None | Some(Row::File(_))) {
            return;
        }
        self.select(ix, cx);
        self.open_selected(window, cx);
    }

    /// Where the selected row goes: a problem's range in its file (as
    /// sent, in one no editor showed), a task's problem's position, a related
    /// location's in the same file, or another file's row. None for a location
    /// in no file.
    fn selected_target(&self) -> Option<Target> {
        let (path, problem) = self.selected_problem()?;
        let row = *self.rows.get(self.selected?)?;
        if let Row::Problem(file, problem) = row
            && let Some(sent) = &self.sent[file]
        {
            return Some(sent.targets[problem].clone());
        }
        let Row::Related(_, _, related) = row else {
            return Some(Target::Range(path.to_owned(), problem.range.clone()));
        };
        let related = &problem.related[related];
        match &related.range {
            Some(range) => Some(Target::Range(path.to_owned(), range.clone())),
            None => Some(Target::Row(related.path.clone()?, related.row)),
        }
    }

    /// Opens the selected row through the workbench, for a click.
    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.selected_target() else {
            return;
        };
        if let Some(workbench) = self.workbench.upgrade() {
            workbench.update(cx, |workbench, cx| {
                workbench.open_problem(target, window, cx)
            });
        }
    }

    /// Its element's key context: [`KEY_CONTEXT`], with the `vim` flag
    /// while vim is on.
    fn key_context(&self, cx: &App) -> KeyContext {
        let mut context = KeyContext::default();
        context.add(KEY_CONTEXT);
        if fe_gpui::vim_enabled(cx) {
            context.add(VIM_FLAG);
        }
        context
    }

    fn color(&self, severity: Severity) -> Hsla {
        self.colors[severity as usize]
    }

    /// `path` as a file row shows it: from the root, if it's under it.
    fn display_path(&self, path: &Path) -> String {
        let relative = (self.root.as_deref()).and_then(|root| path.strip_prefix(root).ok());
        relative.unwrap_or(path).display().to_string()
    }

    /// Whether file `ix`'s text is being read, so its columns aren't known.
    fn reading(&self, ix: usize) -> bool {
        self.sent[ix].as_ref().is_some_and(|sent| sent.reading)
    }

    /// What shows in place of the rows, if anything.
    fn message(&self) -> Option<&'static str> {
        self.files
            .is_empty()
            .then_some("No problems in the open files")
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let ui = fe_ui::ui_theme(cx);
        let generation = self.generation;
        let selected_bg = if self.focus.contains_focused(window, cx) {
            ui.row_selected
        } else {
            ui.row_selected_inactive
        };
        // A row's identity is its list's too, so one pressed before the
        // list changed isn't clicked as the row that replaced it.
        let name = SharedString::from(format!("problem-{generation}"));
        range
            .filter_map(|ix| {
                let row = div()
                    .id(ElementId::NamedInteger(name.clone(), ix as u64))
                    .h(ROW_HEIGHT)
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .whitespace_nowrap();
                let row = match *self.rows.get(ix)? {
                    Row::File(file) => {
                        let file = &self.files[file];
                        row.text_color(ui.tab_active_text)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(self.display_path(&file.path)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(ui.muted_text)
                                    .child(file.problems.len().to_string()),
                            )
                    }
                    Row::Problem(file, problem) => {
                        let problem = &self.files[file].problems[problem];
                        row.pl_5()
                            .when(self.selected == Some(ix), |row| row.bg(selected_bg))
                            .hover(move |row| row.bg(ui.row_hover))
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                panel.open(generation, ix, window, cx)
                            }))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(10.))
                                    .text_color(self.color(problem.severity))
                                    .child(severity_icon(problem.severity)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(first_line(&problem.message).to_owned()),
                            )
                            .when_some(problem.source.clone(), |row, source| {
                                row.child(div().flex_none().text_color(ui.muted_text).child(source))
                            })
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(ui.muted_text)
                                    .child(position(problem, self.reading(file))),
                            )
                    }
                    Row::Related(file, problem, related) => {
                        let related = &self.files[file].problems[problem].related[related];
                        row.pl(px(44.))
                            .when(self.selected == Some(ix), |row| row.bg(selected_bg))
                            .hover(move |row| row.bg(ui.row_hover))
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                panel.open(generation, ix, window, cx)
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(first_line(&related.message).to_owned()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(ui.muted_text)
                                    .child(related_position(related)),
                            )
                    }
                };
                Some(row.into_any_element())
            })
            .collect()
    }
}

/// A severity's mark in its row.
fn severity_icon(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "E",
        Severity::Warning => "W",
        Severity::Information => "I",
        Severity::Hint => "H",
    }
}

/// A message's first line: a row shows one.
fn first_line(message: &str) -> &str {
    message.lines().next().unwrap_or_default()
}

/// A problem's `row:col`, one-based; `row` alone while its file's text is
/// being read.
fn position(problem: &Problem, reading: bool) -> String {
    match reading {
        true => (problem.row + 1).to_string(),
        false => format!("{}:{}", problem.row + 1, problem.column + 1),
    }
}

/// A related location's `file:row`, one-based: `row` alone in the
/// problem's own file.
fn related_position(related: &RelatedInfo) -> String {
    let row = related.row + 1;
    let name = (related.range.is_none())
        .then(|| related.path.as_deref()?.file_name())
        .flatten();
    match name {
        Some(name) => format!("{}:{row}", name.to_string_lossy()),
        None => row.to_string(),
    }
}

/// How many problems of each severity `files` have.
fn counts(files: &[FileProblems]) -> DiagnosticCounts {
    let mut counts = DiagnosticCounts::default();
    for problem in files.iter().flat_map(|file| &file.problems) {
        match problem.severity {
            Severity::Error => counts.errors += 1,
            Severity::Warning => counts.warnings += 1,
            Severity::Information => counts.infos += 1,
            Severity::Hint => counts.hints += 1,
        }
    }
    counts
}

/// `n` and `noun`, plural unless `n` is 1.
fn count(n: usize, noun: &str) -> String {
    let s = if n == 1 { "" } else { "s" };
    format!("{n} {noun}{s}")
}

/// The status items for `counts`: the errors', then the warnings', none
/// without either. Each has both counts, as `1 error, 1 warning`, for a
/// tooltip.
fn status_items(counts: DiagnosticCounts) -> Vec<StatusItem> {
    if counts.errors + counts.warnings == 0 {
        return Vec::new();
    }
    let tooltip = format!(
        "{}, {}",
        count(counts.errors, "error"),
        count(counts.warnings, "warning")
    );
    let item = |id, priority, n: usize, icon, tone| {
        StatusItem::new(id, StatusSide::Left, priority, n.to_string())
            .icon(icon)
            .tone(if n > 0 { tone } else { StatusTone::Default })
            .tooltip(tooltip.clone())
            .command(Invocation::of(SHOW).expect("a valid command ID"))
    };
    vec![
        item(
            ITEM,
            PRIORITY,
            counts.errors,
            Icon::CircleX,
            StatusTone::Error,
        ),
        item(
            WARNINGS_ITEM,
            PRIORITY + 1,
            counts.warnings,
            Icon::TriangleAlert,
            StatusTone::Warning,
        ),
    ]
}

impl Focusable for ProblemsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ProblemsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = fe_ui::ui_theme(cx);
        let list = uniform_list(
            "problem-rows",
            self.rows.len(),
            cx.processor(|panel, range, window, cx| panel.render_rows(range, window, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1();
        div()
            .track_focus(&self.focus)
            .key_context(self.key_context(cx))
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

impl Workbench {
    /// Lists the open buffers' diagnostics in the Problems panel, once per
    /// file, with the servers' diagnostics for files no editor shows and the
    /// finished tasks' problems, and shows their errors and warnings on the
    /// status bar.
    pub(crate) fn sync_problems(&mut self, cx: &mut Context<Self>) {
        let mut listed = HashSet::new();
        // The open files' paths, as opened and as resolved.
        let mut shown = HashSet::new();
        // The text of each file listed that's known, for a task's columns.
        let mut texts = HashMap::new();
        let files = (self.open_editors(cx).into_iter())
            .filter_map(|editor| {
                let model = editor.read(cx).model();
                let path = model.path()?.to_owned();
                let resolved = self.lsp.resolved.get(&model.buffer().id());
                shown.extend(resolved.cloned().flatten());
                shown.insert(path.clone());
                listed.insert(path.clone()).then(|| {
                    texts.insert(path.clone(), model.buffer().text().clone());
                    (path, model.problems())
                })
            })
            .collect();
        let files = group_problems(files);
        let mut files: BTreeMap<PathBuf, (FileProblems, Option<Sent>)> = (files.into_iter())
            .map(|file| (file.path.clone(), (file, None)))
            .collect();
        for (path, problems, sent, text) in self.unopened_problems(&shown, cx) {
            if let Some(text) = text {
                texts.entry(path.clone()).or_insert(text);
            }
            let file = FileProblems {
                path: path.clone(),
                problems,
            };
            files.entry(path).or_insert((file, Some(sent)));
        }
        let panel = self.dock().read(cx).problems().clone();
        merge_sources(&mut files, &panel.read(cx).sources, &texts);
        let files = files.into_values().collect();
        let root = self.root().map(Path::to_owned);
        let counts = panel.update(cx, |panel, cx| panel.set_files(root, files, cx));
        if let Some(counts) = counts {
            self.show_problem_counts(counts, cx);
        }
    }

    /// The problems of each file the servers have diagnostics for that
    /// isn't `shown`, by the path it's listed and opened by: under the
    /// workbench's root as the root has it, as quick open opens it, with its
    /// text once it's read. Starts reading the files not read yet, and forgets
    /// the rest.
    fn unopened_problems(
        &mut self,
        shown: &HashSet<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Vec<(PathBuf, Vec<Problem>, Sent, Option<Text>)> {
        let root = self.root().map(Path::to_owned);
        let lsp = &mut self.lsp;
        let listed_path = |resolved: &Path| {
            let relative = resolved.strip_prefix(lsp.root.as_deref()?).ok()?;
            Some(root.as_deref()?.join(relative))
        };
        // By resolved path, each server's.
        let mut stored: BTreeMap<&Path, Vec<_>> = BTreeMap::new();
        for ((server, path), (encoding, items)) in &lsp.file_diagnostics {
            let servers = stored.entry(path).or_default();
            servers.push((*server, *encoding, items));
        }
        stored.retain(|resolved, _| {
            let path = listed_path(resolved);
            !shown.contains(*resolved) && !path.is_some_and(|path| shown.contains(&path))
        });
        let unopened = &mut lsp.unopened.files;
        unopened.retain(|path, _| stored.contains_key(path.as_path()));
        let mut problems = Vec::new();
        for (resolved, mut servers) in stored {
            let file = unopened.entry(resolved.to_owned()).or_insert_with(|| {
                let read = cx.background_spawn({
                    let resolved = resolved.to_owned();
                    async move { read_text(resolved) }
                });
                let resolved = resolved.to_owned();
                let task = cx.spawn(async move |workbench, cx| {
                    let text = read.await;
                    workbench
                        .update(cx, |workbench, cx| {
                            let files = &mut workbench.lsp.unopened.files;
                            if let Some(file) = files.get_mut(&resolved) {
                                file.text = Some(text);
                                file.rows = None;
                                workbench.sync_problems(cx);
                            }
                        })
                        .ok();
                });
                UnopenedFile {
                    text: None,
                    _read: task,
                    rows: None,
                }
            });
            let path = listed_path(resolved).unwrap_or_else(|| resolved.to_owned());
            let (rows, sent) = file.rows.get_or_insert_with(|| {
                servers.sort_by_key(|(server, ..)| *server);
                let items = (servers.into_iter())
                    .flat_map(|(_, encoding, items)| {
                        items.iter().map(move |item| (item.clone(), encoding))
                    })
                    .collect();
                let text = file.text.as_ref();
                sent_problems(&path, text.and_then(Option::as_ref), items, text.is_none())
            });
            let (rows, sent) = (rows.clone(), sent.clone());
            problems.push((path, rows, sent, file.text.clone().flatten()));
        }
        problems
    }

    /// Shows `counts` on the status bar: the pair of items, or neither.
    pub(crate) fn show_problem_counts(&mut self, counts: DiagnosticCounts, cx: &mut Context<Self>) {
        let bar = self.status_bar().clone();
        bar.update(cx, |bar, cx| {
            let items = status_items(counts);
            if items.is_empty() {
                bar.remove_item(ITEM, cx);
                bar.remove_item(WARNINGS_ITEM, cx);
            }
            for item in items {
                bar.set_item(item, cx);
            }
        });
    }

    /// Does a problems command but `problems.show`, which the dock does.
    pub(crate) fn run_problems_op(
        &mut self,
        op: ProblemsOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = self.dock().read(cx).problems().clone();
        match op {
            ProblemsOp::Show => {}
            ProblemsOp::SelectNext => panel.update(cx, |panel, cx| panel.move_selection(true, cx)),
            ProblemsOp::SelectPrevious => {
                panel.update(cx, |panel, cx| panel.move_selection(false, cx))
            }
            ProblemsOp::SelectFirst => panel.update(cx, |panel, cx| panel.select_end(false, cx)),
            ProblemsOp::SelectLast => panel.update(cx, |panel, cx| panel.select_end(true, cx)),
            ProblemsOp::Open => {
                if let Some(target) = panel.read(cx).selected_target() {
                    self.open_problem(target, window, cx);
                }
            }
            ProblemsOp::Next => self.go_to_diagnostic(true, cx),
            ProblemsOp::Previous => self.go_to_diagnostic(false, cx),
        }
    }

    /// Puts the active editor's caret at the start of the diagnostic after
    /// its caret, or before it, wrapping, and reveals it.
    fn go_to_diagnostic(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        let model = editor.read(cx).model();
        let from = model.primary_selection().head();
        let Some(range) = model.next_diagnostic(from, forward) else {
            return;
        };
        editor.update(cx, |editor, cx| {
            editor.select_and_reveal(range.start..range.start, cx)
        });
    }

    /// Activates the target's file's editor, the active pane's or else the
    /// first pane's showing it, focused, with the range selected or the
    /// caret at the row's start. A row's file that no pane shows opens in
    /// the active pane, which focuses it once it's read. With vim on, the
    /// caret goes to the range's start, in normal mode: an editor left in
    /// insert or replace mode leaves it once it's drawn
    /// ([`Workbench::land_problem`]). A diagnostic as sent, or a task's
    /// problem, opens as [`Workbench::open_decoded_problem`] opens it.
    fn open_problem(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        self.lsp.unopened.opening = None;
        let vim = fe_gpui::vim_enabled(cx);
        let (path, reveal) = match target {
            Target::Sent(path, item, encoding) => {
                let decode = move |text: &Text| {
                    let decoded =
                        decode_file_diagnostics(text, std::slice::from_ref(&item), encoding);
                    decoded.into_iter().next().map(|d| d.range)
                };
                return self.open_decoded_problem(path, decode, window, cx);
            }
            Target::Position(path, row, column, unit) => {
                let decode = move |text: &Text| {
                    let offset = column_offset(text, row, column, unit);
                    Some(offset..offset)
                };
                return self.open_decoded_problem(path, decode, window, cx);
            }
            Target::Range(path, range) => {
                let range = if vim { range.start..range.start } else { range };
                (path, Reveal::bytes(range))
            }
            Target::Row(path, row) => (
                path,
                Reveal {
                    row,
                    columns: 0..0,
                    within_row: true,
                },
            ),
        };
        let shows = |pane: &Entity<Pane>, cx: &App| pane.read(cx).position(&path).is_some();
        let pane = Some(self.pane().clone())
            .filter(|pane| shows(pane, cx))
            .or_else(|| {
                (self.pane_group(cx).panes().into_iter())
                    .find(|pane| shows(pane, cx))
                    .cloned()
            });
        let pane = match pane {
            Some(pane) => pane,
            None if reveal.within_row => self.pane().clone(),
            None => return,
        };
        if pane != *self.pane() {
            self.activate(pane.clone(), window, cx);
        }
        pane.update(cx, |pane, cx| {
            pane.open_at(path.clone(), Some(reveal), window, cx)
        });
        // Not shown yet: it's being read.
        if !shows(&pane, cx) {
            return;
        }
        let Some(editor) = pane.read(cx).active_editor().cloned() else {
            return;
        };
        self.focus_problem(&editor, window, cx);
    }

    /// Focuses `editor`, where a problem was just selected; with vim on, in
    /// normal mode once it's drawn.
    fn focus_problem(
        &mut self,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&editor.focus_handle(cx), cx);
        // Selecting left a visual mode and dropped a pending command.
        if fe_gpui::vim_enabled(cx) && typing(editor, cx) {
            let at = editor.read(cx).model().primary_selection().range.start;
            self.problem_landing = Some((editor.downgrade(), at));
            cx.notify();
        }
    }

    /// Opens `path`, a file whose problem's offsets aren't known until it's
    /// read, in the active pane if it shows it, else the first showing it by
    /// now, else the active pane, as quick open does, and once its editor is
    /// shown selects the range `decode`
    /// finds in its text (its start, with vim on), focused. Not if another
    /// file is opened in the pane first.
    fn open_decoded_problem(
        &mut self,
        path: PathBuf,
        decode: impl Fn(&Text) -> Option<Range<usize>> + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shows = |pane: &Entity<Pane>, cx: &App| pane.read(cx).position(&path).is_some();
        let pane = Some(self.pane().clone())
            .filter(|pane| shows(pane, cx))
            .or_else(|| {
                (self.pane_group(cx).panes().into_iter())
                    .find(|pane| shows(pane, cx))
                    .cloned()
            })
            .unwrap_or_else(|| self.pane().clone());
        if pane != *self.pane() {
            self.activate(pane.clone(), window, cx);
        }
        pane.update(cx, |pane, cx| pane.open_at(path.clone(), None, window, cx));
        let serial = pane.read(cx).open_serial();
        if !pane.read(cx).is_opening(&path) {
            return self.land_decoded_problem(&pane, &path, &decode, window, cx);
        }
        let opening = cx.observe_in(&pane, window, move |workbench, pane, window, cx| {
            let read = pane.read(cx);
            if read.open_serial() == serial && read.is_opening(&path) {
                return;
            }
            workbench.lsp.unopened.opening = None;
            if read.open_serial() == serial {
                workbench.land_decoded_problem(&pane, &path, &decode, window, cx);
            }
        });
        self.lsp.unopened.opening = Some(opening);
    }

    /// Selects the range `decode` finds in the text of `pane`'s active editor,
    /// if it shows `path`, and focuses it.
    fn land_decoded_problem(
        &mut self,
        pane: &Entity<Pane>,
        path: &Path,
        decode: &dyn Fn(&Text) -> Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = pane.read(cx).active_editor().cloned() else {
            return;
        };
        if editor.read(cx).model().path() != Some(path) {
            return;
        }
        let Some(mut range) = decode(editor.read(cx).model().buffer().text()) else {
            return;
        };
        if fe_gpui::vim_enabled(cx) {
            range = range.start..range.start;
        }
        editor.update(cx, |editor, cx| editor.select_and_reveal(range, cx));
        self.focus_problem(&editor, window, cx);
    }

    /// Takes the editor a problem opened in out of insert or replace mode,
    /// its caret back at the problem: after the frame being drawn, which
    /// draws the editor, so it takes the command. The workbench calls it as
    /// it renders. It's vim's command, not `escape`, so it doesn't depend
    /// on the keymap.
    pub(crate) fn land_problem(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((editor, at)) = self.problem_landing.take() else {
            return;
        };
        cx.defer_in(window, move |_, window, cx| {
            // Outside the workbench's update: the editor runs it.
            window.defer(cx, move |window, cx| {
                let Some(editor) = editor.upgrade() else {
                    return;
                };
                if typing(&editor, cx) {
                    let invocation = Invocation::of(NORMAL_MODE).expect("a valid command ID");
                    let run = RunCommand(invocation);
                    editor.focus_handle(cx).dispatch_action(&run, window, cx);
                }
                editor.update(cx, |editor, cx| editor.select_and_reveal(at..at, cx));
            });
        });
    }
}

/// Vim's command leaving insert mode, and replace mode.
const NORMAL_MODE: &str = "vim.normal_mode";

/// Whether `editor` is in vim's insert or replace mode.
fn typing(editor: &Entity<EditorView>, cx: &App) -> bool {
    matches!(editor.read(cx).vim_mode(), Some("insert" | "replace"))
}

/// A row as shown.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShownRow {
    /// A file's path, as shown, and its problem count.
    File(String, usize),
    /// A problem's severity mark, message, source and `row:col`.
    Problem(&'static str, String, Option<String>, String),
    /// A related location's message and `file:row`.
    Related(String, String),
}

#[cfg(test)]
impl ProblemsPanel {
    pub(crate) fn shown_rows(&self) -> Vec<ShownRow> {
        (self.rows.iter())
            .map(|row| match *row {
                Row::File(file) => {
                    let file = &self.files[file];
                    ShownRow::File(self.display_path(&file.path), file.problems.len())
                }
                Row::Problem(file, problem) => {
                    let reading = self.reading(file);
                    let problem = &self.files[file].problems[problem];
                    ShownRow::Problem(
                        severity_icon(problem.severity),
                        first_line(&problem.message).to_owned(),
                        problem.source.clone(),
                        position(problem, reading),
                    )
                }
                Row::Related(file, problem, related) => {
                    let related = &self.files[file].problems[problem].related[related];
                    ShownRow::Related(
                        first_line(&related.message).to_owned(),
                        related_position(related),
                    )
                }
            })
            .collect()
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub(crate) fn message_text(&self) -> Option<&'static str> {
        self.message()
    }

    /// The key context its element sets.
    pub(crate) fn test_key_context(&self, cx: &App) -> KeyContext {
        self.key_context(cx)
    }

    /// Where row `ix` was laid out in the last frame, from the list's bounds,
    /// scroll offset and measured row height.
    pub(crate) fn row_bounds(&self, ix: usize) -> Option<gpui::Bounds<gpui::Pixels>> {
        let count = self.rows.len();
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
#[path = "problems_panel_tests.rs"]
mod problems_panel_tests;
