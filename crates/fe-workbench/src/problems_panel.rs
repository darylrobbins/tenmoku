//! The Problems panel on the right dock: the language servers' diagnostics
//! of every buffer open in the window, once per file however many splits
//! show it, grouped by file. A file's row has its path, from the root if
//! it's under it, and how many problems it has; one row per problem
//! follows, with its severity, message, source and `row:col`. Clicking a
//! problem, or `problems.open` on the selected one, activates the file's
//! editor (the active pane's, else the first pane's showing it) and selects
//! the problem's range; `problems.select_next` and
//! `problems.select_previous` move the selection over the problems.
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
//! A known limit: fe-lsp delivers diagnostics per open document, so a file
//! that isn't open isn't listed, whatever a server knows of it.
//!
//! The commands are in the dock's registry ([`crate::dock`]), as
//! [`ProblemsOp`]s: `problems.show`, `diagnostics.next` and
//! `diagnostics.previous` in the workbench's context, the others in the
//! panel's [`KEY_CONTEXT`].

use std::{
    collections::HashSet,
    ops::Range,
    path::{Path, PathBuf},
};

use fe_command::Invocation;
use fe_editor::diagnostics::{DiagnosticCounts, FileProblems, Problem, Severity, group_problems};
use fe_gpui::{EditorTheme, EditorView, commands::RunCommand};
use fe_ui::{ROW_HEIGHT, UiTheme};
use gpui::{
    AnyElement, App, Context, ElementId, Entity, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, KeyContext, ParentElement as _, Render, ScrollStrategy,
    SharedString, StatefulInteractiveElement as _, Styled as _, UniformListScrollHandle,
    WeakEntity, Window, div, prelude::FluentBuilder as _, px, uniform_list,
};

use crate::{Pane, Workbench, pane::Reveal};
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
    /// Opens the selected problem.
    Open,
    /// `diagnostics.next`, on the active editor.
    Next,
    /// `diagnostics.previous`, on the active editor.
    Previous,
}

/// The problems commands, for the dock's registry: id, title, context and
/// op.
pub(crate) fn commands() -> [(&'static str, &'static str, &'static str, ProblemsOp); 6] {
    use ProblemsOp::*;
    let workbench = crate::KEY_CONTEXT;
    [
        (SHOW, "Show Problems", workbench, Show),
        (
            "problems.select_next",
            "Select Next Problem",
            KEY_CONTEXT,
            SelectNext,
        ),
        (
            "problems.select_previous",
            "Select Previous Problem",
            KEY_CONTEXT,
            SelectPrevious,
        ),
        ("problems.open", "Open Problem", KEY_CONTEXT, Open),
        ("diagnostics.next", "Go to Next Problem", workbench, Next),
        (
            "diagnostics.previous",
            "Go to Previous Problem",
            workbench,
            Previous,
        ),
    ]
}

/// A row: a file's, or one of its problems.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    File(usize),
    Problem(usize, usize),
}

pub struct ProblemsPanel {
    workbench: WeakEntity<Workbench>,
    focus: FocusHandle,
    /// What paths show relative to.
    root: Option<PathBuf>,
    /// By path; none without problems.
    files: Vec<FileProblems>,
    rows: Vec<Row>,
    /// A `Row::Problem`'s index in `rows`.
    selected: Option<usize>,
    /// Bumped as the list changes, for the rows' element ids.
    generation: u64,
    /// The counts the status item shows, once it's set.
    item: Option<DiagnosticCounts>,
    scroll: UniformListScrollHandle,
    ui: UiTheme,
    /// Each severity's colour, as the editor marks it.
    colors: [Hsla; 4],
}

impl ProblemsPanel {
    /// An empty panel, opening problems through `workbench`, its severities
    /// in `theme`'s colours.
    pub(crate) fn new(
        workbench: WeakEntity<Workbench>,
        theme: &EditorTheme,
        ui: UiTheme,
        cx: &mut App,
    ) -> Self {
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
            rows: Vec::new(),
            selected: None,
            generation: 0,
            item: None,
            scroll: UniformListScrollHandle::new(),
            ui,
            colors,
        }
    }

    /// Lists `files`, with paths relative to `root`, keeping the selected
    /// problem selected if it's still listed: the nearest with its message,
    /// severity and source. The window's counts, if the status item doesn't
    /// show them yet.
    fn set_files(
        &mut self,
        root: Option<PathBuf>,
        files: Vec<FileProblems>,
        cx: &mut Context<Self>,
    ) -> Option<DiagnosticCounts> {
        let counts = counts(&files);
        let item = (self.item != Some(counts)).then(|| {
            self.item = Some(counts);
            counts
        });
        if files == self.files && root == self.root {
            return item;
        }
        let selected = self.selected_problem().map(|(path, problem)| {
            let path = path.to_owned();
            (path, problem.clone())
        });
        self.root = root;
        self.files = files;
        self.rows.clear();
        for (ix, file) in self.files.iter().enumerate() {
            self.rows.push(Row::File(ix));
            (self.rows).extend((0..file.problems.len()).map(|problem| Row::Problem(ix, problem)));
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
            self.rows
                .iter()
                .position(|row| *row == Row::Problem(file, problem))
        });
        self.generation += 1;
        cx.notify();
        item
    }

    /// The selected problem, and its file's path.
    fn selected_problem(&self) -> Option<(&Path, &Problem)> {
        let Row::Problem(file, problem) = *self.rows.get(self.selected?)? else {
            return None;
        };
        let file = &self.files[file];
        Some((&file.path, &file.problems[problem]))
    }

    /// The problem rows' indices in `rows`.
    fn problem_rows(&self) -> Vec<usize> {
        (self.rows.iter().enumerate())
            .filter(|(_, row)| matches!(row, Row::Problem(..)))
            .map(|(ix, _)| ix)
            .collect()
    }

    /// Moves the selection to the next problem, or the previous, stopping
    /// at the last and the first; with none selected, selects the first,
    /// or the last.
    fn move_selection(&mut self, forward: bool, cx: &mut Context<Self>) {
        let problems = self.problem_rows();
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

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Selects row `ix`, if it's still `generation`'s and a problem's, and
    /// opens it.
    fn open(&mut self, generation: u64, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if generation != self.generation || !matches!(self.rows.get(ix), Some(Row::Problem(..))) {
            return;
        }
        self.select(ix, cx);
        self.open_selected(window, cx);
    }

    /// The selected problem's file and range.
    fn selected_target(&self) -> Option<(PathBuf, Range<usize>)> {
        let (path, problem) = self.selected_problem()?;
        Some((path.to_owned(), problem.range.clone()))
    }

    /// Opens the selected problem through the workbench, for a click.
    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, range)) = self.selected_target() else {
            return;
        };
        if let Some(workbench) = self.workbench.upgrade() {
            workbench.update(cx, |workbench, cx| {
                workbench.open_problem(&path, range, window, cx)
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
        let ui = self.ui;
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
                                    .child(position(problem)),
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

/// A problem's `row:col`, one-based.
fn position(problem: &Problem) -> String {
    format!("{}:{}", problem.row + 1, problem.column + 1)
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
        let ui = self.ui;
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
    /// file, and shows their errors and warnings on the status bar.
    pub(crate) fn sync_problems(&mut self, cx: &mut Context<Self>) {
        let mut listed = HashSet::new();
        let files = (self.open_editors(cx).into_iter())
            .filter_map(|editor| {
                let model = editor.read(cx).model();
                let path = model.path()?.to_owned();
                listed
                    .insert(path.clone())
                    .then(|| (path, model.problems()))
            })
            .collect();
        let files = group_problems(files);
        let root = self.root().map(Path::to_owned);
        let panel = self.dock().read(cx).problems().clone();
        let counts = panel.update(cx, |panel, cx| panel.set_files(root, files, cx));
        if let Some(counts) = counts {
            self.show_problem_counts(counts, cx);
        }
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
            ProblemsOp::Open => {
                if let Some((path, range)) = panel.read(cx).selected_target() {
                    self.open_problem(&path, range, window, cx);
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

    /// Activates `path`'s editor, the active pane's or else the first
    /// pane's showing it, focused, with `range` selected. With vim on, the
    /// caret goes to the range's start, in normal mode: an editor left in
    /// insert or replace mode leaves it once it's drawn
    /// ([`Workbench::land_problem`]).
    fn open_problem(
        &mut self,
        path: &Path,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shows = |pane: &Entity<Pane>, cx: &App| pane.read(cx).position(path).is_some();
        let pane = Some(self.pane().clone())
            .filter(|pane| shows(pane, cx))
            .or_else(|| {
                (self.pane_group(cx).panes().into_iter())
                    .find(|pane| shows(pane, cx))
                    .cloned()
            });
        let Some(pane) = pane else {
            return;
        };
        if pane != *self.pane() {
            self.activate(pane.clone(), window, cx);
        }
        let vim = fe_gpui::vim_enabled(cx);
        let range = if vim { range.start..range.start } else { range };
        pane.update(cx, |pane, cx| {
            pane.open_at(
                path.to_owned(),
                Some(Reveal::bytes(range.clone())),
                window,
                cx,
            )
        });
        let Some(editor) = pane.read(cx).active_editor().cloned() else {
            return;
        };
        window.focus(&editor.focus_handle(cx), cx);
        // Selecting left a visual mode and dropped a pending command.
        if vim && typing(&editor, cx) {
            self.problem_landing = Some((editor.downgrade(), range.start));
            cx.notify();
        }
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
                    let problem = &self.files[file].problems[problem];
                    ShownRow::Problem(
                        severity_icon(problem.severity),
                        first_line(&problem.message).to_owned(),
                        problem.source.clone(),
                        position(problem),
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
