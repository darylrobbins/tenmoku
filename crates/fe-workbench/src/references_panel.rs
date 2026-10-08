//! Find references (shift-F12) on the right dock: the uses of the word at
//! the active editor's caret, grouped by file, which open at the use when
//! clicked, or with Enter on the selected one.
//!
//! `nav.find_references` builds a [`NavRequest`] for the active editor's
//! caret and runs [`Navigation::references`] on the background executor.
//! As in project search, each run has a generation and a cancel flag:
//! starting one sets the previous run's flag, and batches of another
//! generation than the current are dropped. The batches come over a channel
//! to a foreground task that appends them, in arrival order, so rows never
//! move while results stream in. Dropping the panel, as closing the
//! workbench does, cancels the run.
//!
//! The results stay, whatever the dock shows meanwhile, until the next
//! shift-F12, or until the workbench is re-rooted: their paths are relative
//! to the root they were found in.

use std::{
    ops::Range,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use async_channel::Receiver;
use fe_buffer::{Bias, Text};
use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use fe_nav::{
    Answer, FileReferences, NavError, NavRequest, Navigation, Reference, ReferencesSummary,
};
use fe_ui::{HighlightedLabel, ROW_HEIGHT, UiTheme};
use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, FocusHandle, Focusable, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollStrategy, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, UniformListScrollHandle, WeakEntity,
    Window, div, prelude::FluentBuilder as _, px, uniform_list,
};

use crate::{DockPanel, PANEL_WIDTH, Workbench, nav::Place};

/// The key context the panel's element sets.
pub(crate) const KEY_CONTEXT: &str = "ReferencesPanel";

enum Status {
    /// Nothing has run.
    Idle,
    NoFolder,
    NoSymbol,
    Running,
    /// The summary, and ` (by provider)` for a search answer.
    Done(ReferencesSummary, String),
    Failed(NavError),
}

/// A result row: a file's header, or one of its references.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    File(usize),
    Ref(usize, usize),
}

pub struct ReferencesPanel {
    workbench: WeakEntity<Workbench>,
    focus: FocusHandle,
    /// The word the results are of.
    word: Option<String>,
    /// The root the results' paths are relative to.
    root: Option<PathBuf>,
    /// Where the caret was when they were asked for.
    from: Option<Place>,
    /// The (path relative to the root, range end) of each reference opened
    /// from this run: where the caret may be after one lands, though other
    /// landings may be still loading.
    opened: Vec<(PathBuf, usize)>,
    /// The current run's, which only its batches carry.
    generation: u64,
    /// The current run's: set when another starts, or the panel goes.
    cancel: Arc<AtomicBool>,
    running: Option<Task<()>>,
    status: Status,
    /// In arrival order.
    files: Vec<FileReferences>,
    rows: Vec<Row>,
    /// The `Row::Ref`s in `rows`.
    references: usize,
    /// A `Row::Ref`'s index in `rows`.
    selected: Option<usize>,
    scroll: UniformListScrollHandle,
    ui: UiTheme,
    #[cfg(test)]
    pub(crate) test: TestHooks,
}

impl ReferencesPanel {
    /// A panel with no results, opening them through `workbench`.
    pub(crate) fn new(workbench: WeakEntity<Workbench>, ui: UiTheme, cx: &mut App) -> Self {
        Self {
            workbench,
            focus: cx.focus_handle(),
            word: None,
            root: None,
            from: None,
            opened: Vec::new(),
            generation: 0,
            cancel: Arc::default(),
            running: None,
            status: Status::Idle,
            files: Vec::new(),
            rows: Vec::new(),
            references: 0,
            selected: None,
            scroll: UniformListScrollHandle::new(),
            ui,
            #[cfg(test)]
            test: TestHooks::default(),
        }
    }

    /// Cancels the running search and clears its results, for a new
    /// generation, which it returns.
    fn restart(&mut self, cx: &mut Context<Self>) -> u64 {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::default();
        self.generation += 1;
        self.running = None;
        self.files.clear();
        self.rows.clear();
        self.references = 0;
        self.selected = None;
        self.word = None;
        self.root = None;
        self.from = None;
        self.opened.clear();
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
        self.generation
    }

    /// Cancels the running search and clears its results, saying `status`.
    fn reset(&mut self, status: Status, cx: &mut Context<Self>) {
        self.restart(cx);
        self.status = status;
    }

    /// Drops the results, as the workbench is re-rooted.
    pub(crate) fn clear(&mut self, cx: &mut Context<Self>) {
        self.reset(Status::Idle, cx);
    }

    /// Cancels the running search, and finds the uses of `word`, the word at
    /// `request`'s caret, once `ready` closes. `from` is where the caret is.
    fn run(
        &mut self,
        request: NavRequest,
        word: String,
        from: Option<Place>,
        ready: Receiver<()>,
        navigation: Arc<Navigation>,
        cx: &mut Context<Self>,
    ) {
        let generation = self.restart(cx);
        self.word = Some(word);
        self.root = Some(request.root.clone());
        self.from = from;
        self.status = Status::Running;
        let cancel = self.cancel.clone();
        #[cfg(test)]
        let hold = self.test.hold.take();
        self.running = Some(cx.spawn(async move |panel, cx| {
            ready.recv().await.ok();
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            #[cfg(test)]
            panel.update(cx, |panel, _| panel.test.runs += 1).ok();
            let (sender, batches) = async_channel::unbounded();
            let found = cx.background_spawn(async move {
                navigation.references(&request, &cancel, &mut |batch| {
                    sender.send_blocking(batch).ok();
                })
            });
            // The channel closes once the provider returns.
            while let Ok(batch) = batches.recv().await {
                #[cfg(test)]
                if let Some(hold) = &hold {
                    hold.recv().await.ok();
                }
                let appended = panel.update(cx, |panel, cx| panel.append(generation, batch, cx));
                if appended.is_err() {
                    return;
                }
            }
            let answer = found.await;
            #[cfg(test)]
            if let Some(hold) = &hold {
                hold.recv().await.ok();
            }
            panel
                .update(cx, |panel, cx| panel.finished(generation, answer, cx))
                .ok();
        }));
    }

    /// Appends `batch`'s files, if it's of the current generation, and
    /// selects the first reference if none is.
    fn append(&mut self, generation: u64, batch: Vec<FileReferences>, cx: &mut Context<Self>) {
        if generation != self.generation {
            return;
        }
        for file in batch {
            let ix = self.files.len();
            self.rows.push(Row::File(ix));
            self.rows
                .extend((0..file.refs.len()).map(|reference| Row::Ref(ix, reference)));
            self.references += file.refs.len();
            self.files.push(file);
        }
        if self.selected.is_none() {
            self.selected = self.rows.iter().position(|row| matches!(row, Row::Ref(..)));
        }
        cx.notify();
    }

    fn finished(
        &mut self,
        generation: u64,
        answer: Result<Answer<ReferencesSummary>, NavError>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        self.running = None;
        self.status = match answer {
            Ok(answer) => {
                let by = crate::nav::by(&answer.provider, answer.precision);
                self.word = answer.value.word.clone();
                Status::Done(answer.value, by)
            }
            Err(error) => Status::Failed(error),
        };
        cx.notify();
    }

    /// The line above the status: the word the results are of.
    fn header(&self) -> Option<String> {
        let word = self.word.as_ref()?;
        Some(format!("References to `{word}`"))
    }

    /// The status line, and whether it's an error.
    fn status(&self) -> Option<(String, bool)> {
        let word = self.word.as_deref().unwrap_or_default();
        let text = match &self.status {
            Status::Idle => return None,
            Status::NoFolder => "No folder open".into(),
            Status::NoSymbol => "No symbol at the caret".into(),
            Status::Running => format!("Finding references to `{word}`…"),
            Status::Failed(error) => {
                return Some((format!("Find references failed: {error}"), true));
            }
            Status::Done(summary, by) if summary.word.is_none() => {
                format!("No symbol at the caret{by}")
            }
            Status::Done(summary, by) if summary.truncated => {
                format!(
                    "Showing the first {}{by}",
                    count(self.references, "reference")
                )
            }
            Status::Done(_, by) if self.references == 0 => {
                format!("No references to `{word}`{by}")
            }
            Status::Done(_, by) => format!(
                "{} in {}{by}",
                count(self.references, "reference"),
                count(self.files.len(), "file")
            ),
        };
        Some((text, false))
    }

    /// Moves the selection `delta` references on, clamped to the first and
    /// last, skipping file headers, and scrolls it into view.
    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(selected) = self.selected else {
            return;
        };
        let refs = (self.rows.iter().enumerate())
            .filter(|(_, row)| matches!(row, Row::Ref(..)))
            .map(|(ix, _)| ix);
        let refs: Vec<usize> = refs.collect();
        let Ok(at) = refs.binary_search(&selected) else {
            return;
        };
        let to = at.saturating_add_signed(delta).min(refs.len() - 1);
        self.select(refs[to], cx);
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Opens reference row `ix`, if it's still `generation`'s, in the active
    /// pane with its word selected, recording where the caret is unless it's
    /// still where the last reference opened left it: so going back after
    /// going through the list returns to where the caret was before.
    fn open(&mut self, generation: u64, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if generation != self.generation {
            return;
        }
        let Some(&Row::Ref(file, reference)) = self.rows.get(ix) else {
            return;
        };
        self.select(ix, cx);
        let location = self.files[file].refs[reference].location.clone();
        let Some(workbench) = self.workbench.upgrade() else {
            return;
        };
        let record = workbench
            .read(cx)
            .caret_place(cx)
            .is_some_and(|here| !self.lands_at(&here, workbench.read(cx), cx));
        self.opened.push((location.rel.clone(), location.range.end));
        workbench.update(cx, |workbench, cx| {
            workbench.list_jump(record, &location, window, cx)
        });
    }

    /// Whether `here` is where a reference this panel opened left the caret,
    /// as the editor showing it has it now: the end of its range, or that end
    /// clamped to a file that has shrunk since it was found. Not where the
    /// references were asked for, though one ends there; nor the end of one
    /// not opened, which the caret was moved to.
    fn lands_at(&self, here: &Place, workbench: &Workbench, cx: &App) -> bool {
        if self.from.as_ref() == Some(here) {
            return false;
        }
        let Some(rel) = (self.root.as_deref()).and_then(|root| here.path.strip_prefix(root).ok())
        else {
            return false;
        };
        let editor = workbench
            .active_editor(cx)
            .map(|editor| editor.read(cx).model());
        let text = editor
            .filter(|model| model.path() == Some(here.path.as_path()))
            .map(|model| model.text());
        (self.opened.iter())
            .filter(|(opened, _)| opened == rel)
            .any(|&(_, end)| {
                here.offset == end
                    || text.is_some_and(|text| text.clip_offset(end, Bias::Right) == here.offset)
            })
    }

    /// Runs a panel command; others go on to the workbench.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        match op(run, cx) {
            Some(ReferencesOp::SelectPrev) => self.move_selection(-1, cx),
            Some(ReferencesOp::SelectNext) => self.move_selection(1, cx),
            Some(ReferencesOp::Open) => {
                if let Some(ix) = self.selected {
                    self.open(self.generation, ix, window, cx);
                }
            }
            Some(ReferencesOp::Find) | None => cx.propagate(),
        }
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        #[cfg(test)]
        self.test.built.push(range.clone());
        let ui = self.ui;
        let generation = self.generation;
        let selected_bg = if self.focus.contains_focused(window, cx) {
            ui.row_selected
        } else {
            ui.row_selected_inactive
        };
        // A row's identity is its generation's too, so one pressed before a
        // rerun isn't clicked as the row that replaced it.
        let name = SharedString::from(format!("reference-{generation}"));
        range
            .map(|ix| {
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
                match self.rows[ix] {
                    Row::File(file) => {
                        let file = &self.files[file];
                        row.text_color(ui.tab_active_text)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .child(file.rel.display().to_string()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(ui.muted_text)
                                    .child(file.refs.len().to_string()),
                            )
                    }
                    Row::Ref(file, reference) => {
                        let reference = &self.files[file].refs[reference];
                        let (dimmed, definition) = (!reference.verified, reference.is_definition);
                        #[cfg(test)]
                        self.test.drawn.push((ix, dimmed, definition));
                        row.pl_5()
                            .when(dimmed, |row| row.opacity(0.5))
                            .when(self.selected == Some(ix), |row| row.bg(selected_bg))
                            .hover(move |row| row.bg(ui.row_hover))
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                panel.open(generation, ix, window, cx)
                            }))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(ui.muted_text)
                                    .child(reference.line_number.to_string()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .child(label(reference, &ui)),
                            )
                            .when(definition, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .text_color(ui.muted_text)
                                        .child("definition"),
                                )
                            })
                    }
                }
                .into_any_element()
            })
            .collect()
    }
}

/// A reference's preview, its word highlighted.
fn label(reference: &Reference, ui: &UiTheme) -> HighlightedLabel {
    let preview = &reference.preview;
    // On char boundaries, as the label wants them.
    let highlights = (reference.preview_ranges.iter())
        .map(|range| {
            let end = preview.ceil_char_boundary(range.end.min(preview.len()));
            preview.floor_char_boundary(range.start.min(end))..end
        })
        .filter(|range| !range.is_empty())
        .collect();
    HighlightedLabel::new(preview.clone(), highlights, ui)
}

/// `n` and `noun`, plural unless `n` is 1.
fn count(n: usize, noun: &str) -> String {
    let s = if n == 1 { "" } else { "s" };
    format!("{n} {noun}{s}")
}

/// The word at `offset` in `text`: the longest run of word chars touching
/// it, as the search provider takes it (fe-nav's `word_at`), for the status
/// to name while the run goes.
fn word_at(text: &Text, offset: usize, language: Option<&str>) -> Option<String> {
    let dollar = matches!(language, Some("javascript" | "typescript" | "tsx"));
    let is_word = |ch: char| fe_project::is_word_char(ch) || (dollar && ch == '$');
    let offset = text.clip_offset(offset, Bias::Left);
    let row = text.offset_to_point(offset).row;
    let line = text.row_to_string(row);
    let column = offset - text.row_start(row);
    let start = (line[..column].char_indices().rev())
        .take_while(|&(_, ch)| is_word(ch))
        .last()
        .map_or(column, |(at, _)| at);
    let end = (line[column..].char_indices())
        .find(|&(_, ch)| !is_word(ch))
        .map_or(line.len(), |(at, _)| column + at);
    (start < end).then(|| line[start..end].to_owned())
}

impl Drop for ReferencesPanel {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Focusable for ReferencesPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ReferencesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.test.built.clear();
            self.test.drawn.clear();
        }
        let ui = self.ui;
        let list = uniform_list(
            "reference-rows",
            self.rows.len(),
            cx.processor(|panel, range, window, cx| panel.render_rows(range, window, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1();
        div()
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_run_command))
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
            .when_some(self.header(), |panel, header| {
                panel.child(div().flex_none().px_3().pt_2().child(header))
            })
            .when_some(self.status(), |panel, (status, error)| {
                let color = if error { ui.error_text } else { ui.muted_text };
                panel.child(
                    div()
                        .flex_none()
                        .px_3()
                        .py_1()
                        .text_color(color)
                        .child(status),
                )
            })
            .child(list)
    }
}

impl Workbench {
    /// `nav.find_references`: shows the dock with References, focused, and
    /// finds the uses of the word at the active editor's caret there, in
    /// place of the last results.
    fn find_references(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A go-to-definition still running would take focus from the panel.
        self.nav_state.goto.next();
        let fallback = self.focus_handle(cx);
        let dock = self.dock().clone();
        let panel = dock.update(cx, |dock, cx| {
            dock.show(DockPanel::References, cx);
            dock.leave_search(&fallback, window, cx);
            dock.references().clone()
        });
        window.focus(&panel.focus_handle(cx), cx);
        let Some(ready) = self.nav_state.start(cx) else {
            return panel.update(cx, |panel, cx| panel.reset(Status::NoFolder, cx));
        };
        let request = self.nav_request(cx);
        let word = (request.as_ref())
            .and_then(|request| word_at(&request.text, request.offset, request.language));
        let (Some(request), Some(word)) = (request, word) else {
            return panel.update(cx, |panel, cx| panel.reset(Status::NoSymbol, cx));
        };
        let from = self.caret_place(cx);
        let navigation = self.navigation().clone();
        panel.update(cx, |panel, cx| {
            panel.run(request, word, from, ready, navigation, cx)
        });
    }

    /// Runs `nav.find_references`, if `run` is it. Whether it was.
    pub(crate) fn run_references_command(
        &mut self,
        run: &RunCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let find = op(run, cx) == Some(ReferencesOp::Find);
        if find {
            self.find_references(window, cx);
        }
        find
    }
}

/// What a references command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReferencesOp {
    Find,
    SelectPrev,
    SelectNext,
    Open,
}

/// The [`CommandTarget`] of references commands: the op to do.
pub(crate) struct ReferencesCommands;

impl CommandTarget for ReferencesCommands {
    type Tx = ();
    type Target<'a> = Option<ReferencesOp>;
}

struct Commands(Rc<Registry<ReferencesCommands>>);

impl Global for Commands {}

/// Registers `nav.find_references` and the panel's commands.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<ReferencesCommands> {
    use ReferencesOp::*;
    let commands = [
        (
            "nav.find_references",
            "Find All References",
            crate::KEY_CONTEXT,
            Find,
        ),
        (
            "references_panel.select_prev",
            "Select Previous Reference",
            KEY_CONTEXT,
            SelectPrev,
        ),
        (
            "references_panel.select_next",
            "Select Next Reference",
            KEY_CONTEXT,
            SelectNext,
        ),
        ("references_panel.open", "Open Reference", KEY_CONTEXT, Open),
    ];
    let mut registry = Registry::new();
    for (id, title, context, op) in commands {
        registry
            .register(
                id,
                title,
                move |target: &mut Option<ReferencesOp>, _: &Actor| {
                    *target = Some(op);
                    Ok(Dispatched::new(Effect::Notify))
                },
            )
            .expect("references command IDs are valid and distinct")
            .context(ContextPredicate::identifier(context));
    }
    registry
}

/// The op of `run`, if it's a references command that applies in its
/// context.
fn op(run: &RunCommand, cx: &App) -> Option<ReferencesOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}

/// What the tests hold and count.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestHooks {
    /// Awaited before each batch of the next run lands, and before its
    /// summary does. It holds batches here rather than in a provider: a
    /// provider's `references` blocks, and GPUI's test executor runs
    /// background tasks on the test thread, so a provider waiting for the
    /// test would wait forever.
    pub(crate) hold: Option<Receiver<()>>,
    /// The row ranges the last frame built.
    built: Vec<Range<usize>>,
    /// The reference rows the last frame built: their index, and whether
    /// they were dimmed and tagged a definition.
    drawn: Vec<(usize, bool, bool)>,
    /// Runs that asked the provider.
    runs: usize,
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShownRow {
    /// A file's relative path and reference count.
    File(String, usize),
    /// A reference's line number, preview and highlighted ranges in it,
    /// whether it's dimmed (unverified) and tagged a definition.
    Ref {
        line: u64,
        preview: String,
        highlights: Vec<Range<usize>>,
        dimmed: bool,
        definition: bool,
    },
}

#[cfg(test)]
impl ReferencesPanel {
    pub(crate) fn shown_rows(&self) -> Vec<ShownRow> {
        let ui = self.ui;
        (self.rows.iter())
            .map(|row| match *row {
                Row::File(file) => {
                    let file = &self.files[file];
                    ShownRow::File(file.rel.display().to_string(), file.refs.len())
                }
                Row::Ref(file, reference) => {
                    let reference = &self.files[file].refs[reference];
                    let label = label(reference, &ui);
                    ShownRow::Ref {
                        line: reference.line_number,
                        preview: label.text.to_string(),
                        highlights: label.highlights,
                        dimmed: !reference.verified,
                        definition: reference.is_definition,
                    }
                }
            })
            .collect()
    }

    /// The reference rows' locations, in order.
    pub(crate) fn locations(&self) -> Vec<fe_nav::Location> {
        (self.rows.iter())
            .filter_map(|row| match *row {
                Row::Ref(file, reference) => {
                    Some(self.files[file].refs[reference].location.clone())
                }
                Row::File(_) => None,
            })
            .collect()
    }

    pub(crate) fn header_text(&self) -> Option<String> {
        self.header()
    }

    pub(crate) fn status_text(&self) -> Option<String> {
        self.status().map(|(text, _)| text)
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The current run's cancel flag.
    pub(crate) fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    /// Runs that asked the provider.
    pub(crate) fn runs(&self) -> usize {
        self.test.runs
    }

    /// The row ranges the last frame built.
    pub(crate) fn built(&self) -> Vec<Range<usize>> {
        self.test.built.clone()
    }

    /// The reference rows the last frame built: their index, and whether
    /// they were dimmed and tagged a definition.
    pub(crate) fn drawn(&self) -> Vec<(usize, bool, bool)> {
        self.test.drawn.clone()
    }

    /// Where the list was laid out in the last frame.
    pub(crate) fn list_bounds(&self) -> gpui::Bounds<gpui::Pixels> {
        self.scroll.0.borrow().base_handle.bounds()
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
