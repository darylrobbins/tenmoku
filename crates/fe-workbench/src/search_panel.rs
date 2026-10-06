//! Project search on the right dock: a query, three options, and the
//! matching lines grouped by file, which open at the match when clicked.
//!
//! Enter runs the query (`text_input.submit`, as typing never searches);
//! flipping an option reruns it. Each run has a generation and a cancel
//! flag: starting one sets the previous run's flag, and batches of another
//! generation than the current are dropped. `fe_project::search` runs in
//! the background, its batches coming over a channel to a foreground task
//! that appends them, in arrival order, so rows never move while results
//! stream in. Dropping the panel cancels the run.

use std::{
    ops::Range,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_gpui::commands::RunCommand;
use fe_project::{FileMatches, LineMatch, SearchError, SearchOptions, SearchQuery, SearchSummary};
use fe_ui::{HighlightedLabel, ROW_HEIGHT, TextInput, TextInputEvent, ToggleButton, UiTheme};
use gpui::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, UniformListScrollHandle, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, uniform_list,
};

use crate::{
    PANEL_WIDTH, Pane,
    dock::{self, DockOp},
    pane::Reveal,
    picker::{self, PaletteOp},
};

/// The key context the search panel's element sets.
pub(crate) const KEY_CONTEXT: &str = "search_panel";

/// A search option, with a toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchOption {
    Regex,
    MatchCase,
    WholeWord,
}

enum Status {
    /// No search has run, or the query is empty.
    Idle,
    NoFolder,
    Searching,
    Done(SearchSummary),
    Failed(SearchError),
}

/// A result row: a file's header, or one of its matching lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResultRow {
    File(usize),
    Line(usize, usize),
}

pub struct SearchPanel {
    root: Option<PathBuf>,
    pane: WeakEntity<Pane>,
    input: Entity<TextInput>,
    regex: bool,
    match_case: bool,
    whole_word: bool,
    options: SearchOptions,
    /// The current run's, which only its batches carry.
    generation: u64,
    /// The current run's: set when another starts, or the panel goes.
    cancel: Arc<AtomicBool>,
    running: Option<Task<()>>,
    status: Status,
    /// In arrival order.
    files: Vec<FileMatches>,
    rows: Vec<ResultRow>,
    scroll: UniformListScrollHandle,
    toggles: ScrollHandle,
    ui: UiTheme,
    /// The row ranges the last frame built.
    #[cfg(test)]
    built: Vec<Range<usize>>,
    _input: Subscription,
}

impl SearchPanel {
    /// Searches under `root`, opening matches in `pane`. Touches nothing
    /// until a query runs.
    pub fn new(
        root: Option<PathBuf>,
        pane: WeakEntity<Pane>,
        ui: UiTheme,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| TextInput::new("Search", &ui, cx));
        let submitted = cx.subscribe(&input, |panel, _, event, cx| {
            if *event == TextInputEvent::Submitted {
                panel.run(cx);
            }
        });
        Self {
            root,
            pane,
            input,
            regex: false,
            match_case: false,
            whole_word: false,
            options: SearchOptions::default(),
            generation: 0,
            cancel: Arc::default(),
            running: None,
            status: Status::Idle,
            files: Vec::new(),
            rows: Vec::new(),
            scroll: UniformListScrollHandle::new(),
            toggles: ScrollHandle::new(),
            ui,
            #[cfg(test)]
            built: Vec::new(),
            _input: submitted,
        }
    }

    pub fn input(&self) -> &Entity<TextInput> {
        &self.input
    }

    /// The query the input and options make.
    pub fn query(&self, cx: &gpui::App) -> SearchQuery {
        SearchQuery {
            pattern: self.input.read(cx).text().to_owned(),
            regex: self.regex,
            case_sensitive: self.match_case,
            whole_word: self.whole_word,
        }
    }

    /// Cancels the running search and starts a new one for the query,
    /// unless it's empty: that clears the results.
    pub fn run(&mut self, cx: &mut Context<Self>) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::default();
        self.generation += 1;
        self.running = None;
        self.files.clear();
        self.rows.clear();
        self.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
        let query = self.query(cx);
        if query.pattern.is_empty() {
            self.status = Status::Idle;
            return;
        }
        let Some(root) = self.root.clone() else {
            self.status = Status::NoFolder;
            return;
        };
        self.status = Status::Searching;
        let generation = self.generation;
        let options = self.options.clone();
        let cancel = self.cancel.clone();
        let (sender, batches) = async_channel::unbounded();
        let search = cx.background_spawn(async move {
            fe_project::search(&root, &query, &options, cancel, |batch| {
                sender.send_blocking((generation, batch)).ok();
            })
        });
        self.running = Some(cx.spawn(async move |panel, cx| {
            // The channel closes once the search returns.
            while let Ok((generation, batch)) = batches.recv().await {
                let appended = panel.update(cx, |panel, cx| panel.append(generation, batch, cx));
                if appended.is_err() {
                    return;
                }
            }
            let result = search.await;
            panel
                .update(cx, |panel, cx| panel.finished(generation, result, cx))
                .ok();
        }));
    }

    fn append(&mut self, generation: u64, batch: Vec<FileMatches>, cx: &mut Context<Self>) {
        if generation != self.generation {
            return;
        }
        for file in batch {
            let ix = self.files.len();
            self.rows.push(ResultRow::File(ix));
            self.rows
                .extend((0..file.lines.len()).map(|line| ResultRow::Line(ix, line)));
            self.files.push(file);
        }
        cx.notify();
    }

    fn finished(
        &mut self,
        generation: u64,
        result: Result<SearchSummary, SearchError>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        self.running = None;
        self.status = match result {
            Ok(summary) => {
                for (path, error) in &summary.errors {
                    log::warn!("cannot search {}: {error}", path.display());
                }
                Status::Done(summary)
            }
            Err(error) => {
                self.files.clear();
                self.rows.clear();
                Status::Failed(error)
            }
        };
        cx.notify();
    }

    /// Flips `option`, and reruns the query unless it's empty.
    fn toggle(&mut self, option: SearchOption, cx: &mut Context<Self>) {
        let flag = match option {
            SearchOption::Regex => &mut self.regex,
            SearchOption::MatchCase => &mut self.match_case,
            SearchOption::WholeWord => &mut self.whole_word,
        };
        *flag = !*flag;
        if !self.input.read(cx).text().is_empty() {
            self.run(cx);
        }
        cx.notify();
    }

    /// The status line, and whether it's an error.
    pub fn status(&self) -> Option<(SharedString, bool)> {
        let text = match &self.status {
            Status::Idle => return None,
            Status::NoFolder => "No folder open".into(),
            Status::Searching => "Searching…".into(),
            Status::Failed(error) => return Some((error.to_string().into(), true)),
            Status::Done(summary) if summary.truncated => {
                format!("Showing the first {} results", self.options.max_matches)
            }
            Status::Done(summary) if summary.matches == 0 => "No results".into(),
            Status::Done(summary) => format!(
                "{} in {}",
                count(summary.matches, "result"),
                count(summary.files_matched, "file")
            ),
        };
        Some((text.into(), false))
    }

    /// Opens the file of line row `ix`, if it's still `generation`'s, with
    /// its first match selected.
    fn open(&mut self, generation: u64, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(&ResultRow::Line(file, line)) = self.rows.get(ix) else {
            return;
        };
        if generation != self.generation {
            return;
        }
        let file = &self.files[file];
        let line = &file.lines[line];
        let reveal = Reveal {
            row: line
                .line_number
                .saturating_sub(1)
                .try_into()
                .unwrap_or(u32::MAX),
            columns: line.ranges.first().cloned().unwrap_or_default(),
        };
        let path = file.path.clone();
        self.pane
            .update(cx, |pane, cx| pane.open_at(path, Some(reveal), window, cx))
            .ok();
    }

    /// Runs a search option, Enter or an input edit; others go on to the
    /// workbench.
    fn on_run_command(&mut self, run: &RunCommand, _: &mut Window, cx: &mut Context<Self>) {
        let option = match dock::op(run, cx) {
            Some(DockOp::ToggleRegex) => Some(SearchOption::Regex),
            Some(DockOp::ToggleCase) => Some(SearchOption::MatchCase),
            Some(DockOp::ToggleWholeWord) => Some(SearchOption::WholeWord),
            Some(DockOp::Submit) => return self.input.update(cx, TextInput::submit),
            _ => None,
        };
        if let Some(option) = option {
            return self.toggle(option, cx);
        }
        let edit: fn(&mut TextInput, &mut Context<TextInput>) = match picker::op(run, cx) {
            Some(PaletteOp::Backspace) => TextInput::backspace,
            Some(PaletteOp::DeleteWordLeft) => TextInput::delete_word_left,
            Some(PaletteOp::Clear) => TextInput::clear,
            Some(PaletteOp::Paste) => TextInput::paste,
            _ => return cx.propagate(),
        };
        self.input.update(cx, edit);
    }

    fn render_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        #[cfg(test)]
        self.built.push(range.clone());
        let ui = self.ui;
        let generation = self.generation;
        range
            .map(|ix| {
                let row = div()
                    .id(("result", ix))
                    .h(ROW_HEIGHT)
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .whitespace_nowrap();
                match self.rows[ix] {
                    ResultRow::File(file) => {
                        let file = &self.files[file];
                        let matches: usize = file.lines.iter().map(|line| line.ranges.len()).sum();
                        row.text_color(ui.tab_active_text)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .child(file.rel.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(ui.muted_text)
                                    .child(matches.to_string()),
                            )
                    }
                    ResultRow::Line(file, line) => {
                        let line = &self.files[file].lines[line];
                        row.pl_5()
                            .hover(move |row| row.bg(ui.row_hover))
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                panel.open(generation, ix, window, cx)
                            }))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(ui.muted_text)
                                    .child(line.line_number.to_string()),
                            )
                            .child(line_label(line, &ui))
                    }
                }
                .into_any_element()
            })
            .collect()
    }
}

/// `n` and `noun`, plural unless `n` is 1.
fn count(n: usize, noun: &str) -> String {
    let s = if n == 1 { "" } else { "s" };
    format!("{n} {noun}{s}")
}

/// A line's preview, its matches highlighted: shifted by `preview_start`,
/// clipped to the preview and widened to its char boundaries.
fn line_label(line: &LineMatch, ui: &UiTheme) -> HighlightedLabel {
    let preview = &line.preview;
    let shift = |offset: usize| offset.saturating_sub(line.preview_start).min(preview.len());
    let highlights = line
        .ranges
        .iter()
        .filter_map(|range| {
            let start = preview.floor_char_boundary(shift(range.start));
            let end = preview.ceil_char_boundary(shift(range.end));
            (start < end).then_some(start..end)
        })
        .collect();
    HighlightedLabel::new(preview.clone(), highlights, ui)
}

impl Drop for SearchPanel {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Render for SearchPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        self.built.clear();
        let ui = self.ui;
        let toggles = [
            (SearchOption::Regex, ".*", self.regex),
            (SearchOption::MatchCase, "Aa", self.match_case),
            (SearchOption::WholeWord, "ab", self.whole_word),
        ]
        .map(|(option, label, on)| {
            ToggleButton::new(("search-option", option as usize), label, &ui)
                .on(on)
                .on_click(cx.listener(move |panel, _, _, cx| panel.toggle(option, cx)))
        });
        let list = uniform_list(
            "search-results",
            self.rows.len(),
            cx.processor(|panel, range, _, cx| panel.render_rows(range, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1();
        div()
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
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .pr_2()
                    .child(div().flex_1().min_w_0().child(self.input.clone()))
                    .child(
                        div()
                            .id("search-options")
                            .flex()
                            .flex_none()
                            .gap_0p5()
                            .track_scroll(&self.toggles)
                            .children(toggles),
                    ),
            )
            .when_some(self.status(), |panel, (status, error)| {
                let color = if error { ui.error_text } else { ui.muted_text };
                panel.child(
                    div()
                        .flex_none()
                        .px_3()
                        .pb_1()
                        .text_color(color)
                        .child(status),
                )
            })
            .child(list)
    }
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShownRow {
    /// A file's relative path and match count.
    File(String, usize),
    /// A line's number, preview and highlighted ranges in it.
    Line(u64, String, Vec<Range<usize>>),
}

#[cfg(test)]
impl SearchPanel {
    /// Sets how much a search searches: walker threads, and the match cap.
    pub(crate) fn set_options(&mut self, options: SearchOptions) {
        self.options = options;
    }

    /// The current run's cancel flag.
    pub(crate) fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub(crate) fn shown_rows(&self) -> Vec<ShownRow> {
        let ui = self.ui;
        self.rows
            .iter()
            .map(|row| match *row {
                ResultRow::File(file) => {
                    let file = &self.files[file];
                    let matches = file.lines.iter().map(|line| line.ranges.len()).sum();
                    ShownRow::File(file.rel.clone(), matches)
                }
                ResultRow::Line(file, line) => {
                    let line = &self.files[file].lines[line];
                    let label = line_label(line, &ui);
                    ShownRow::Line(line.line_number, label.text.to_string(), label.highlights)
                }
            })
            .collect()
    }

    pub(crate) fn status_text(&self) -> Option<String> {
        self.status().map(|(text, _)| text.to_string())
    }

    /// The row ranges the last frame built.
    pub(crate) fn built(&self) -> Vec<Range<usize>> {
        self.built.clone()
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

    /// Where the regex (0), match case (1) or whole word (2) toggle was laid
    /// out in the last frame.
    pub(crate) fn toggle_bounds(&self, ix: usize) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.toggles.bounds_for_item(ix)
    }
}
