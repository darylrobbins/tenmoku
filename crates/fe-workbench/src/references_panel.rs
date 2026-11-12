//! Find All References (shift-alt-F12) on the right dock: the uses of the
//! word at the active editor's caret, grouped by file, which open at the use
//! when clicked, or with Enter on the selected one. With vim on, the panel's
//! key context has the `vim` flag, for the list keys bound in vim's style.
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
//! shift-alt-F12, or until the workbench is re-rooted: their paths are
//! relative to the root they were found in.
//!
//! Peek References (shift-F12) runs a panel of its own in the peek's list
//! ([`ReferencesPanel::peeked`]): there a click selects a reference, and a
//! double-click or Enter asks the peek to open it ([`OpenPeeked`]).

mod landing;

use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use async_channel::Receiver;
use fe_buffer::{Bias, Text};
use fe_nav::{
    Answer, FileReferences, NavError, NavRequest, Navigation, Reference, ReferencesSummary,
};
use fe_ui::{HighlightedLabel, ROW_HEIGHT, UiTheme};
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, ElementId, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyContext, ParentElement as _, Render,
    ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _, Task,
    UniformListScrollHandle, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    uniform_list,
};
use kiln_l10n::{tr, verbatim};

use crate::{
    DockPanel, Workbench,
    workbench_commands::{
        OpenReference, SelectFirstReference, SelectLastReference, SelectNextReference,
        SelectPreviousReference, invoked_in,
    },
};

/// Find references' results in the sidebar: on the rail once it has shown.
pub(crate) const SIDEBAR: crate::dock::SidebarPanel = crate::dock::SidebarPanel {
    panel: crate::DockPanel::new("references"),
    name: "References",
    icon: fe_ui::Icon::FileSymlink,
    show_command: "nav.find_references",
    show_op: crate::dock::DockOp::ShowReferences,
    order: 50,
    always_on_rail: false,
    focus_on_show: false,
};

/// The key context the panel's element sets.
pub(crate) const KEY_CONTEXT: &str = verbatim!("ReferencesPanel");

/// The flag the panel's key context has while vim is on.
const VIM_FLAG: &str = "vim";

/// A function that decodes a host's locations of type `L`.
type Decode<L> = fn(
    &fe_lsp::LspNavigation,
    &Path,
    &fe_lsp::Admitted,
    L,
    &AtomicBool,
) -> Result<Vec<FileReferences>, NavError>;

enum Status {
    /// Nothing has run.
    Idle,
    NoFolder,
    NoSymbol,
    Running,
    /// The summary, and the provider's name for a search's answer.
    Done(ReferencesSummary, Option<String>),
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
    /// Where this run's opens took each pane's caret.
    landings: landing::Landings,
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
    /// In a peek's list: see [`ReferencesPanel::peeked`].
    peeked: bool,
    #[cfg(test)]
    pub(crate) test: TestHooks,
}

/// A peek's panel asks it to open the selected reference.
pub(crate) struct OpenPeeked;

impl EventEmitter<OpenPeeked> for ReferencesPanel {}

impl ReferencesPanel {
    /// A panel with no results, opening them through `workbench`.
    pub(crate) fn new(workbench: WeakEntity<Workbench>, cx: &mut App) -> Self {
        Self {
            workbench,
            focus: cx.focus_handle(),
            word: None,
            root: None,
            landings: landing::Landings::default(),
            generation: 0,
            cancel: Arc::default(),
            running: None,
            status: Status::Idle,
            files: Vec::new(),
            rows: Vec::new(),
            references: 0,
            selected: None,
            scroll: UniformListScrollHandle::new(),
            peeked: false,
            #[cfg(test)]
            test: TestHooks::default(),
        }
    }

    /// A panel for a peek's list: a click on a reference selects it, and a
    /// double-click or Enter emits [`OpenPeeked`] rather than opening it.
    pub(crate) fn peeked(workbench: WeakEntity<Workbench>, cx: &mut App) -> Self {
        let mut panel = Self::new(workbench, cx);
        panel.peeked = true;
        panel
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
        self.landings.clear();
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
    /// `request`'s caret, once `ready` closes.
    pub(crate) fn run(
        &mut self,
        request: NavRequest,
        word: String,
        ready: Receiver<()>,
        navigation: Arc<Navigation>,
        cx: &mut Context<Self>,
    ) {
        let generation = self.restart(cx);
        self.word = Some(word);
        self.root = Some(request.root.clone());
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

    /// Replaces the results with `files`, an answer the host already has,
    /// as a finished search would, with `word` in the header.
    pub(crate) fn show_locations(
        &mut self,
        word: String,
        files: Vec<FileReferences>,
        cx: &mut Context<Self>,
    ) {
        let generation = self.restart(cx);
        self.word = Some(word.clone());
        self.append(generation, files, cx);
        self.status = Status::Done(
            ReferencesSummary {
                word: Some(word),
                files_searched: self.files.len(),
                files_verified: self.files.len(),
                ..ReferencesSummary::default()
            },
            None,
        );
        cx.notify();
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
                let by = (answer.precision == fe_nav::Precision::Search)
                    .then(|| answer.provider.clone());
                self.word = answer.value.word.clone();
                Status::Done(answer.value, by)
            }
            Err(error) => Status::Failed(error),
        };
        cx.notify();
    }

    /// The references' locations, in the order the rows list them.
    pub(crate) fn found(&self) -> impl Iterator<Item = &fe_nav::Location> {
        (self.files.iter()).flat_map(|file| file.refs.iter().map(|reference| &reference.location))
    }

    /// Which of [`ReferencesPanel::found`] is selected.
    pub(crate) fn selected_reference(&self) -> Option<usize> {
        let selected = self.selected?;
        let refs = self.rows[..selected].iter();
        Some(refs.filter(|row| matches!(row, Row::Ref(..))).count())
    }

    /// What the status says, once the run is over with no reference found.
    pub(crate) fn found_none(&self) -> Option<String> {
        let over = !matches!(self.status, Status::Idle | Status::Running);
        let (text, _) = self.status().filter(|_| over && self.references == 0)?;
        Some(text)
    }

    /// The line above the status: the word the results are of.
    fn header(&self) -> Option<String> {
        let word = self.word.as_ref()?;
        Some(tr!("workbench-lsp-references-header", word = word).into())
    }

    /// The status line, and whether it's an error.
    fn status(&self) -> Option<(String, bool)> {
        let word = self.word.as_deref().unwrap_or_default();
        // A search's answer names its provider.
        let source = |by: &Option<String>| match by {
            Some(_) => "search",
            None => "exact",
        };
        let provider = |by: &Option<String>| by.clone().unwrap_or_default();
        let text = match &self.status {
            Status::Idle => return None,
            Status::NoFolder => tr!("workbench-lsp-references-no-folder"),
            Status::NoSymbol => tr!("workbench-lsp-references-no-symbol"),
            Status::Running => tr!("workbench-lsp-references-finding", word = word),
            Status::Failed(error) => {
                let text = tr!("workbench-lsp-references-failed", error = error.to_string());
                return Some((text.into(), true));
            }
            Status::Done(summary, by) if summary.word.is_none() => tr!(
                "workbench-lsp-references-no-symbol-by",
                source = source(by),
                provider = provider(by)
            ),
            Status::Done(summary, by) if summary.truncated => tr!(
                "workbench-lsp-references-first",
                count = self.references,
                source = source(by),
                provider = provider(by)
            ),
            Status::Done(_, by) if self.references == 0 => tr!(
                "workbench-lsp-references-none",
                word = word,
                source = source(by),
                provider = provider(by)
            ),
            Status::Done(_, by) => tr!(
                "workbench-lsp-references-found",
                count = self.references,
                files = self.files.len(),
                source = source(by),
                provider = provider(by)
            ),
        };
        Some((text.into(), false))
    }

    /// Moves the selection `delta` references on, clamped to the first and
    /// last, skipping file headers, and scrolls it into view.
    pub(crate) fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
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

    /// Selects the last reference, if `last`, or else the first, and
    /// scrolls it into view.
    pub(crate) fn select_end(&mut self, last: bool, cx: &mut Context<Self>) {
        let mut refs = (self.rows.iter().enumerate())
            .filter(|(_, row)| matches!(row, Row::Ref(..)))
            .map(|(ix, _)| ix);
        let end = if last { refs.next_back() } else { refs.next() };
        if let Some(ix) = end {
            self.select(ix, cx);
        }
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Opens reference row `ix`, if it's still `generation`'s, in the active
    /// pane with its word selected (where it is now, if a language server
    /// found it in an open buffer's earlier version: see
    /// [`Workbench::target_of`]), recording where the caret is unless the
    /// panel's last open in the pane took it there and it hasn't moved since:
    /// so going back after going through the list returns to where the caret
    /// was before.
    fn open(&mut self, generation: u64, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if generation != self.generation {
            return;
        }
        let Some(&Row::Ref(file, reference)) = self.rows.get(ix) else {
            return;
        };
        self.select(ix, cx);
        if self.peeked {
            return cx.emit(OpenPeeked);
        }
        let location = self.files[file].refs[reference].location.clone();
        let Some(workbench) = self.workbench.upgrade() else {
            return;
        };
        let pane = workbench.read(cx).pane().clone();
        let record = !(self.landings).holds_caret(&pane, workbench.read(cx), cx);
        let before = pane.read(cx).open_serial();
        // The file the jump opens, as the pane will have it.
        let target = workbench.read(cx).target_of(&location, cx);
        workbench.update(cx, |workbench, cx| {
            workbench.list_jump(record, &location, window, cx)
        });
        if let Some((path, _)) = target {
            self.landings.opened(&pane, before, path, cx);
        }
    }

    /// Opens the selected reference, if any.
    pub(crate) fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.selected {
            self.open(self.generation, ix, window, cx);
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

    fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        #[cfg(test)]
        self.test.built.push(range.clone());
        let ui = fe_ui::ui_theme(cx);
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
                            .on_click(cx.listener(move |panel, event: &ClickEvent, window, cx| {
                                // In a peek, a click shows the reference.
                                if panel.peeked && event.click_count() < 2 {
                                    if generation == panel.generation {
                                        panel.select(ix, cx);
                                    }
                                    return;
                                }
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
                                        .child(tr!("workbench-lsp-references-definition")),
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
    HighlightedLabel::new(reference.preview.clone(), highlights(reference), ui)
}

/// A reference's word in its preview.
fn highlights(reference: &Reference) -> Vec<Range<usize>> {
    let preview = &reference.preview;
    // On char boundaries, as the label wants them.
    (reference.preview_ranges.iter())
        .map(|range| {
            let end = preview.ceil_char_boundary(range.end.min(preview.len()));
            preview.floor_char_boundary(range.start.min(end))..end
        })
        .filter(|range| !range.is_empty())
        .collect()
}

/// The word at `offset` in `text`: the longest run of word chars touching
/// it, as the search provider takes it (fe-nav's `word_at`), for the status
/// to name while the run goes.
pub(crate) fn word_at(text: &Text, offset: usize, language: Option<&str>) -> Option<String> {
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
        let ui = fe_ui::ui_theme(cx);
        let list = uniform_list(
            "reference-rows",
            self.rows.len(),
            cx.processor(|panel, range, window, cx| panel.render_rows(range, window, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1();
        div()
            .track_focus(&self.focus)
            .key_context(self.key_context(cx))
            // Its commands, the workbench's typed commands, which the panel
            // does itself while focused, where their context holds.
            .on_action(cx.listener(|panel, _: &SelectPreviousReference, _, cx| {
                if invoked_in(KEY_CONTEXT, cx) {
                    panel.move_selection(-1, cx)
                }
            }))
            .on_action(cx.listener(|panel, _: &SelectNextReference, _, cx| {
                if invoked_in(KEY_CONTEXT, cx) {
                    panel.move_selection(1, cx)
                }
            }))
            .on_action(cx.listener(|panel, _: &SelectFirstReference, _, cx| {
                if invoked_in(KEY_CONTEXT, cx) {
                    panel.select_end(false, cx)
                }
            }))
            .on_action(cx.listener(|panel, _: &SelectLastReference, _, cx| {
                if invoked_in(KEY_CONTEXT, cx) {
                    panel.select_end(true, cx)
                }
            }))
            .on_action(cx.listener(|panel, _: &OpenReference, window, cx| {
                if invoked_in(KEY_CONTEXT, cx) {
                    panel.open_selected(window, cx)
                }
            }))
            .when(!self.peeked, |panel| {
                panel.debug_selector(|| "sidebar-panel".into()).border_l_1()
            })
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
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
    pub(crate) fn find_references(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A go-to-definition still running would take focus from the panel.
        self.nav_state.goto.next();
        let dock = self.dock().clone();
        let panel = dock.update(cx, |dock, cx| {
            dock.show(DockPanel::References, window, cx);
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
        let navigation = self.navigation().clone();
        panel.update(cx, |panel, cx| {
            panel.run(request, word, ready, navigation, cx)
        });
    }

    /// Shows the dock with References, focused, listing `locations`, a
    /// server's, in place of the last results: decoded off the UI thread.
    ///
    /// `locations` stay as the server's type, which this crate doesn't
    /// name: `decode` is the function that takes them.
    pub(crate) fn show_references<L: Send + 'static>(
        &mut self,
        word: String,
        origin: fe_lsp::Admitted,
        locations: L,
        decode: Decode<L>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A go-to-definition still running would take focus from the panel.
        self.nav_state.goto.next();
        let dock = self.dock().clone();
        let panel = dock.update(cx, |dock, cx| {
            dock.show(DockPanel::References, window, cx);
            dock.references().clone()
        });
        window.focus(&panel.focus_handle(cx), cx);
        let (Some(root), Some(manager)) = (self.lsp.root.clone(), self.lsp.manager().clone())
        else {
            return panel.update(cx, |panel, cx| panel.reset(Status::NoFolder, cx));
        };
        let navigation = fe_lsp::LspNavigation::new(Arc::downgrade(&manager), &root);
        let (cancel, generation) = panel.update(cx, |panel, cx| {
            let generation = panel.restart(cx);
            panel.word = Some(word.clone());
            panel.status = Status::Running;
            (panel.cancel.clone(), generation)
        });
        let decoding =
            cx.background_spawn(
                async move { decode(&navigation, &root, &origin, locations, &cancel) },
            );
        let panel = panel.downgrade();
        let task = cx.spawn(async move |_, cx| {
            let answer = decoding.await;
            panel
                .update(cx, |panel, cx| {
                    if generation != panel.generation {
                        return;
                    }
                    match answer {
                        Ok(files) => panel.show_locations(word, files, cx),
                        Err(error) => {
                            panel.running = None;
                            panel.status = Status::Failed(error);
                            cx.notify();
                        }
                    }
                })
                .ok();
        });
        task.detach();
    }

    /// Runs `run` on the References panel, once the workbench's update is
    /// over: opening a reference updates the workbench.
    pub(crate) fn on_references_panel(
        &mut self,
        run: impl FnOnce(&mut ReferencesPanel, &mut Window, &mut Context<ReferencesPanel>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panel = self.dock().read(cx).references().clone();
        window.defer(cx, move |window, cx| {
            panel.update(cx, |panel, cx| run(panel, window, cx))
        });
    }
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
        (self.rows.iter())
            .map(|row| match *row {
                Row::File(file) => {
                    let file = &self.files[file];
                    let shown = crate::tests::slashed(file.rel.display().to_string());
                    ShownRow::File(shown, file.refs.len())
                }
                Row::Ref(file, reference) => {
                    let reference = &self.files[file].refs[reference];
                    ShownRow::Ref {
                        line: reference.line_number,
                        preview: reference.preview.to_string(),
                        highlights: highlights(reference),
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
