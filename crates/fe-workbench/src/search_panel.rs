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
//!
//! A replace field under the query (the chevron, `search.toggle_replace`
//! or `search.show_replace` shows it) replaces the results' matches: all of
//! them, after asking with the counts (`search.replace_all`), or one file's
//! (its row's Replace button). Dismissing a file drops it from the results,
//! so Replace All skips it. The files are matched again as they are then:
//! see [`search_replace`]. Afterwards the status line says what was
//! replaced, and the search runs again.
//!
//! The results list takes focus (`search.focus_results`, or down from the
//! query) and moves through the match rows with `search.select_next` and
//! `search.select_previous`; `search.open_selected` opens the selected
//! match. Its element's key context is [`RESULTS_KEY_CONTEXT`], with `vim`
//! too while vim is on. The fields are `text_input`s, so vim never sees the
//! keys typed in them.
//!
//! The panel keeps each line's ranges and preview start as offsets in the
//! line's lossy decoding, which is what the preview and the editor's text
//! are made of, rather than in its bytes on disk as the search gives them:
//! see [`decode_offsets`].

use std::{
    fs::{self, File},
    io,
    ops::Range,
    os::unix::fs::FileExt as _,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_buffer::FileEncoding;
use fe_gpui::commands::RunCommand;
use fe_project::{
    FileMatches, LineMatch, PathFilter, Replacer, SearchError, SearchOptions, SearchQuery,
    SearchSummary,
};
use fe_ui::{HighlightedLabel, ROW_HEIGHT, TextInput, TextInputEvent, ToggleButton, UiTheme};
use gpui::{
    AnyElement, AppContext as _, Context, ElementId, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, PromptButton, PromptLevel, Render, ScrollHandle,
    ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    UniformListScrollHandle, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    uniform_list,
};

use crate::{
    Pane,
    dock::{self, DockOp},
    pane::Reveal,
    picker::{self, PaletteOp},
};
use search_replace::{Boundary, Target, count};

/// The key context the search panel's element sets.
pub(crate) const KEY_CONTEXT: &str = "search_panel";

/// The key context the results list's element sets, within the panel's.
pub(crate) const RESULTS_KEY_CONTEXT: &str = "search_results";

/// Replace All's prompt's buttons; Replace is the default.
const REPLACE_BUTTONS: [PromptButton; 2] = [
    PromptButton::Ok(SharedString::new_static("Replace")),
    PromptButton::Cancel(SharedString::new_static("Cancel")),
];

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
    /// A glob in the include or exclude field doesn't parse.
    BadGlob(String),
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
    /// The panel's focus: the query's, or either filter's within it.
    focus: FocusHandle,
    input: Entity<TextInput>,
    /// The files to include and to exclude: comma-separated globs.
    include: Entity<TextInput>,
    exclude: Entity<TextInput>,
    /// The replacement, under the query while `replace_shown`.
    replace: Entity<TextInput>,
    replace_shown: bool,
    /// The results list's, which moves through the match rows.
    results_focus: FocusHandle,
    /// The selected line row, in `rows`.
    selected: Option<usize>,
    regex: bool,
    match_case: bool,
    whole_word: bool,
    options: SearchOptions,
    /// The current run's, which only its batches carry.
    generation: u64,
    /// The query the results are of, as it ran.
    searched: Option<SearchQuery>,
    /// The folder it searched, as it was then, unless it couldn't be read.
    boundary: Option<Boundary>,
    /// Moves on with each run and each dismissed file, as the rows do, so
    /// a row pressed before isn't clicked as the row now in its place.
    layout: u64,
    /// The current run's: set when another starts, or the panel goes.
    cancel: Arc<AtomicBool>,
    running: Option<Task<()>>,
    status: Status,
    /// In arrival order.
    files: Vec<FileMatches>,
    rows: Vec<ResultRow>,
    /// The last replacement's outcome, and whether it's an error: the
    /// status line until the next search the user runs.
    notice: Option<(String, bool)>,
    replacing: Option<Task<()>>,
    scroll: UniformListScrollHandle,
    toggles: ScrollHandle,
    ui: UiTheme,
    /// The row ranges the last frame built.
    #[cfg(test)]
    built: Vec<Range<usize>>,
    _inputs: [Subscription; 4],
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
        let include = cx.new(|cx| TextInput::new("files to include", &ui, cx));
        let exclude = cx.new(|cx| TextInput::new("files to exclude", &ui, cx));
        let replace = cx.new(|cx| TextInput::new("Replace", &ui, cx));
        let fields = [
            (&input, false),
            (&include, true),
            (&exclude, true),
            (&replace, false),
        ];
        let submitted = fields.map(|(input, filter)| {
            cx.subscribe(input, move |panel, _, event, cx| match event {
                TextInputEvent::Submitted => panel.run(cx),
                // Typing never searches the query, but a filter reruns.
                TextInputEvent::Changed if filter => panel.run(cx),
                TextInputEvent::Changed => {}
            })
        });
        Self {
            root,
            pane,
            focus: cx.focus_handle(),
            input,
            include,
            exclude,
            replace,
            replace_shown: false,
            results_focus: cx.focus_handle(),
            selected: None,
            regex: false,
            match_case: false,
            whole_word: false,
            options: SearchOptions::default(),
            generation: 0,
            searched: None,
            boundary: None,
            layout: 0,
            cancel: Arc::default(),
            running: None,
            status: Status::Idle,
            files: Vec::new(),
            rows: Vec::new(),
            notice: None,
            replacing: None,
            scroll: UniformListScrollHandle::new(),
            toggles: ScrollHandle::new(),
            ui,
            #[cfg(test)]
            built: Vec::new(),
            _inputs: submitted,
        }
    }

    pub fn input(&self) -> &Entity<TextInput> {
        &self.input
    }
}

/// The panel's focus is its input's.
impl gpui::Focusable for SearchPanel {
    fn focus_handle(&self, _: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl SearchPanel {
    /// Opens matches in `pane` from now on: the active one, as it changes.
    pub(crate) fn set_pane(&mut self, pane: WeakEntity<Pane>) {
        self.pane = pane;
    }

    /// Searches under `root` from now on: cancels the running search and
    /// clears its results. The query and options stay, to run again.
    pub(crate) fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.restart(cx);
        self.root = Some(root);
        self.status = Status::Idle;
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

    /// The path filter the include and exclude fields make.
    fn filter(&self, root: &Path, cx: &gpui::App) -> Result<PathFilter, fe_project::GlobError> {
        let globs = |input: &Entity<TextInput>| -> Vec<String> {
            input
                .read(cx)
                .text()
                .split(',')
                .map(str::trim)
                .filter(|glob| !glob.is_empty())
                .map(str::to_owned)
                .collect()
        };
        let (include, exclude) = (globs(&self.include), globs(&self.exclude));
        let include: Vec<&str> = include.iter().map(String::as_str).collect();
        let exclude: Vec<&str> = exclude.iter().map(String::as_str).collect();
        PathFilter::new(root, &include, &exclude)
    }

    /// Cancels the running search and starts a new one for the query,
    /// unless it's empty: that clears the results.
    pub fn run(&mut self, cx: &mut Context<Self>) {
        let generation = self.restart(cx);
        self.notice = None;
        let query = self.query(cx);
        if query.pattern.is_empty() {
            self.status = Status::Idle;
            return;
        }
        let Some(root) = self.root.clone() else {
            self.status = Status::NoFolder;
            return;
        };
        let filter = match self.filter(&root, cx) {
            Ok(filter) => filter,
            Err(error) => {
                self.status =
                    Status::BadGlob(format!("Invalid glob {}: {}", error.glob, error.message));
                return;
            }
        };
        self.status = Status::Searching;
        self.searched = Some(query.clone());
        self.boundary = Boundary::of(&root).ok();
        let options = self.options.clone();
        let cancel = self.cancel.clone();
        let (sender, batches) = async_channel::unbounded();
        let search = cx.background_spawn(async move {
            let flag = cancel.clone();
            let cancelled = move || flag.load(Ordering::Relaxed);
            fe_project::search_filtered(&root, &query, &options, &filter, cancel, |batch| {
                publish(batch, &cancelled, |batch| {
                    sender.send_blocking((generation, batch)).ok();
                });
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

    /// Cancels the running search and clears its results, for a new
    /// generation, which it returns.
    fn restart(&mut self, cx: &mut Context<Self>) -> u64 {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::default();
        self.generation += 1;
        self.layout += 1;
        self.running = None;
        self.searched = None;
        self.boundary = None;
        self.files.clear();
        self.rows.clear();
        self.selected = None;
        self.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
        self.generation
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
                self.selected = None;
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
        if let Some((notice, error)) = &self.notice {
            return Some((notice.clone().into(), *error));
        }
        let text = match &self.status {
            Status::Idle => return None,
            Status::NoFolder => "No folder open".into(),
            Status::Searching => "Searching…".into(),
            Status::Failed(error) => return Some((error.to_string().into(), true)),
            Status::BadGlob(message) => return Some((message.clone().into(), true)),
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

    /// Opens the file of line row `ix`, if the rows are still `layout`'s,
    /// with its first match selected.
    fn open(&mut self, layout: u64, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(&ResultRow::Line(file, line)) = self.rows.get(ix) else {
            return;
        };
        if layout != self.layout {
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
            within_row: false,
        };
        let path = file.path.clone();
        self.pane
            .update(cx, |pane, cx| pane.open_at(path, Some(reveal), window, cx))
            .ok();
    }

    /// Selects and opens line row `ix`, clicked in a frame of `layout`'s
    /// rows, if they're still those.
    fn click_line(&mut self, layout: u64, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if layout == self.layout {
            self.selected = Some(ix);
            self.open(layout, ix, window, cx);
        }
    }

    /// Runs a search option, a replace or results command, Enter or an
    /// input edit; others go on to the workbench.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        let option = match dock::op(run, cx) {
            Some(DockOp::ToggleRegex) => Some(SearchOption::Regex),
            Some(DockOp::ToggleCase) => Some(SearchOption::MatchCase),
            Some(DockOp::ToggleWholeWord) => Some(SearchOption::WholeWord),
            Some(DockOp::Submit) => {
                return self
                    .focused_input(window, cx)
                    .clone()
                    .update(cx, TextInput::submit);
            }
            Some(DockOp::ShowReplace) => return self.show_replace(false, window, cx),
            Some(DockOp::ToggleReplace) => return self.toggle_replace(window, cx),
            Some(DockOp::ReplaceAll) => return self.confirm_replace_all(window, cx),
            Some(DockOp::FocusResults) => return self.focus_results(window, cx),
            Some(DockOp::SelectNext) => return self.select_next(true, cx),
            Some(DockOp::SelectPrevious) => return self.select_next(false, cx),
            Some(DockOp::OpenSelected) => {
                if let Some(ix) = self.selected {
                    self.open(self.layout, ix, window, cx);
                }
                return;
            }
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
        self.focused_input(window, cx).update(cx, edit);
    }

    /// The field with focus: the query's unless another has it.
    fn focused_input(&self, window: &Window, cx: &gpui::App) -> &Entity<TextInput> {
        [&self.include, &self.exclude, &self.replace]
            .into_iter()
            .find(|input| gpui::Focusable::focus_handle(*input, cx).is_focused(window))
            .unwrap_or(&self.input)
    }

    /// Shows the replace field, focused. If `seed`, from outside the panel,
    /// a nonempty selection within one row in the active editor becomes the
    /// query, and runs.
    pub(crate) fn show_replace(&mut self, seed: bool, window: &mut Window, cx: &mut Context<Self>) {
        let seeded = seed.then(|| self.selected_text(cx)).flatten();
        if let Some(text) = seeded {
            self.input.update(cx, |input, cx| input.set_text(text, cx));
            self.run(cx);
        }
        self.replace_shown = true;
        window.focus(&gpui::Focusable::focus_handle(&self.replace, cx), cx);
        cx.notify();
    }

    /// The active editor's selection, if it's its only one, nonempty and
    /// within one row.
    fn selected_text(&self, cx: &gpui::App) -> Option<String> {
        let pane = self.pane.upgrade()?;
        let editor = pane.read(cx).active_editor()?;
        let model = editor.read(cx).model();
        let selections = model.selections();
        let [selection] = &selections[..] else {
            return None;
        };
        let text = model.text().slice_to_string(selection.range.clone());
        (!text.is_empty() && !text.contains('\n')).then_some(text)
    }

    /// Shows the replace field, focused, or hides it, focusing the query if
    /// it had focus.
    fn toggle_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.replace_shown {
            return self.show_replace(false, window, cx);
        }
        self.replace_shown = false;
        if gpui::Focusable::focus_handle(&self.replace, cx).is_focused(window) {
            window.focus(&gpui::Focusable::focus_handle(&self.input, cx), cx);
        }
        cx.notify();
    }

    /// The matches in the results, and their files.
    fn counts(&self) -> (usize, usize) {
        let matches = (self.files.iter())
            .flat_map(|file| &file.lines)
            .map(|line| line.ranges.len())
            .sum();
        (matches, self.files.len())
    }

    /// The results' files, or only the `only`th's.
    fn targets(&self, only: Option<usize>) -> Vec<Target> {
        (self.files.iter().enumerate())
            .filter(|&(ix, _)| only.is_none_or(|only| only == ix))
            .map(|(_, file)| Target {
                path: file.path.clone(),
                rel: file.rel.clone(),
            })
            .collect()
    }

    /// Asks whether to replace every match in the results, with the counts,
    /// and replaces them if so. Nothing while a replacement runs.
    pub(crate) fn confirm_replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.replacing.is_some() {
            return;
        }
        let Some(query) = self.searched.clone() else {
            return;
        };
        let (matches, files) = self.counts();
        if files == 0 {
            return;
        }
        let template = self.replace.read(cx).text().to_owned();
        let question = format!(
            "Replace {} across {} with “{template}”?",
            count(matches, "occurrence"),
            count(files, "file")
        );
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            Some("Files that aren't open are saved, and can't be undone."),
            &REPLACE_BUTTONS,
            cx,
        );
        let targets = self.targets(None);
        let boundary = self.boundary.clone();
        cx.spawn(async move |panel, cx| {
            if answer.await != Ok(0) {
                return;
            }
            panel
                .update(cx, |panel, cx| {
                    panel.replace(boundary, targets, query, template, cx)
                })
                .ok();
        })
        .detach();
    }

    /// Replaces the matches in the `file`th file of the results, if the
    /// rows are still `layout`'s, without asking.
    fn replace_in_file(&mut self, layout: u64, file: usize, cx: &mut Context<Self>) {
        if layout != self.layout || self.replacing.is_some() || file >= self.files.len() {
            return;
        }
        let Some(query) = self.searched.clone() else {
            return;
        };
        let template = self.replace.read(cx).text().to_owned();
        let targets = self.targets(Some(file));
        self.replace(self.boundary.clone(), targets, query, template, cx);
    }

    /// Replaces `query`'s matches in `targets` with `template`, then says
    /// what it did and searches again.
    fn replace(
        &mut self,
        boundary: Option<Boundary>,
        targets: Vec<Target>,
        query: SearchQuery,
        template: String,
        cx: &mut Context<Self>,
    ) {
        let Some(boundary) = boundary else {
            self.notice = Some(("Can't read the project folder".into(), true));
            cx.notify();
            return;
        };
        let replacer = match Replacer::new(&query, &template) {
            Ok(replacer) => Arc::new(replacer),
            Err(error) => {
                self.notice = Some((error.to_string(), true));
                cx.notify();
                return;
            }
        };
        self.notice = Some(("Replacing…".into(), false));
        self.replacing = Some(cx.spawn(async move |panel, cx| {
            let outcome = search_replace::replace(boundary, targets, replacer, cx).await;
            panel
                .update(cx, |panel, cx| {
                    panel.replacing = None;
                    panel.run(cx);
                    panel.notice = Some(outcome.message());
                    cx.notify();
                })
                .ok();
        }));
        cx.notify();
    }

    /// Drops the `file`th file from the results, if the rows are still
    /// `layout`'s, so Replace All skips it.
    fn dismiss(&mut self, layout: u64, file: usize, cx: &mut Context<Self>) {
        if layout != self.layout || file >= self.files.len() {
            return;
        }
        self.files.remove(file);
        self.layout += 1;
        self.selected = None;
        self.rows.clear();
        for (ix, file) in self.files.iter().enumerate() {
            self.rows.push(ResultRow::File(ix));
            (self.rows).extend((0..file.lines.len()).map(|line| ResultRow::Line(ix, line)));
        }
        cx.notify();
    }

    /// Focuses the results list, selecting its first match row if none is.
    fn focus_results(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_none() {
            self.select_next(true, cx);
        }
        window.focus(&self.results_focus, cx);
        cx.notify();
    }

    /// Selects the next match row after the selected one, or the one
    /// before it if not `forward`; the first with none selected.
    fn select_next(&mut self, forward: bool, cx: &mut Context<Self>) {
        let is_line = |ix: &usize| matches!(self.rows[*ix], ResultRow::Line(..));
        let selected = self.selected.filter(|&ix| ix < self.rows.len());
        let next = match selected {
            None => (0..self.rows.len()).find(is_line),
            Some(at) if forward => (at + 1..self.rows.len()).find(is_line),
            Some(at) => (0..at).rev().find(is_line),
        };
        if let Some(ix) = next {
            self.selected = Some(ix);
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    /// Down in the query moves to the results.
    fn on_query_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "down" && !keystroke.modifiers.modified() && !self.rows.is_empty() {
            self.focus_results(window, cx);
            cx.stop_propagation();
        }
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        #[cfg(test)]
        self.built.push(range.clone());
        let ui = self.ui;
        let layout = self.layout;
        let selected_bg = if self.results_focus.is_focused(window) {
            ui.row_selected
        } else {
            ui.row_selected_inactive
        };
        // A row's identity is its layout's too, so one pressed before a
        // rerun or a dismissal isn't clicked as the row now in its place.
        let name = SharedString::from(format!("result-{layout}"));
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
                let row = row.when(self.selected == Some(ix), |row| row.bg(selected_bg));
                match self.rows[ix] {
                    ResultRow::File(file_ix) => {
                        let file = &self.files[file_ix];
                        let matches: usize = file.lines.iter().map(|line| line.ranges.len()).sum();
                        let replace = self.replace_shown.then(|| {
                            div()
                                .flex_none()
                                .debug_selector(move || format!("search-replace-file:{ix}"))
                                .child(
                                    ToggleButton::new(("search-replace-file", ix), "Replace", &ui)
                                        .on_click(cx.listener(move |panel, _, _, cx| {
                                            panel.replace_in_file(layout, file_ix, cx)
                                        })),
                                )
                        });
                        let dismiss = div()
                            .flex_none()
                            .debug_selector(move || format!("search-dismiss:{ix}"))
                            .child(
                                ToggleButton::new(("search-dismiss", ix), "×", &ui).on_click(
                                    cx.listener(move |panel, _, _, cx| {
                                        panel.dismiss(layout, file_ix, cx)
                                    }),
                                ),
                            );
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
                            .children(replace)
                            .child(dismiss)
                    }
                    ResultRow::Line(file, line) => {
                        let line = &self.files[file].lines[line];
                        row.pl_5()
                            .hover(move |row| row.bg(ui.row_hover))
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                panel.click_line(layout, ix, window, cx)
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

/// A line's preview, its matches highlighted: shifted by `preview_start`,
/// clipped to the preview and widened to its char boundaries, and then
/// merged where they overlap, as two byte matches in one char do.
fn line_label(line: &LineMatch, ui: &UiTheme) -> HighlightedLabel {
    let preview = &line.preview;
    let shift = |offset: usize| offset.saturating_sub(line.preview_start).min(preview.len());
    let mut highlights: Vec<Range<usize>> = Vec::new();
    for range in &line.ranges {
        let start = preview.floor_char_boundary(shift(range.start));
        let end = preview.ceil_char_boundary(shift(range.end));
        if start >= end {
            continue;
        }
        // The ranges are in order, so only the last can overlap.
        match highlights.last_mut() {
            Some(last) if start < last.end => last.end = last.end.max(end),
            _ => highlights.push(start..end),
        }
    }
    HighlightedLabel::new(preview.clone(), highlights, ui)
}

/// How many bytes of a line are read, or decoded, between checks of a run's
/// cancel flag.
pub(crate) const CHECK_BYTES: usize = 64 * 1024;

/// A run's cancel flag was found set.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Cancelled;

fn check(cancelled: &dyn Fn() -> bool) -> Result<(), Cancelled> {
    if cancelled() { Err(Cancelled) } else { Ok(()) }
}

/// Decodes `batch`'s offsets, then passes it to `send`, unless `cancelled`
/// is true at a check while decoding or just before sending: an obsolete
/// batch is neither decoded further nor published. One whose flag is set
/// after that last check is still dropped by [`SearchPanel::append`], as
/// another generation's.
pub(crate) fn publish(
    mut batch: Vec<FileMatches>,
    cancelled: &dyn Fn() -> bool,
    send: impl FnOnce(Vec<FileMatches>),
) {
    if decode_batch(&mut batch, cancelled).is_ok() && check(cancelled).is_ok() {
        send(batch);
    }
}

/// Runs [`decode_offsets`] on each of `batch`'s files, checking `cancelled`
/// before each file, and stops at the first check that's true. A line is
/// rewritten whole or not at all, so the files and lines after the one it
/// stopped in keep their offsets from the search.
pub(crate) fn decode_batch(
    batch: &mut [FileMatches],
    cancelled: &dyn Fn() -> bool,
) -> Result<(), Cancelled> {
    for file in batch {
        check(cancelled)?;
        decode_offsets(file, cancelled)?;
    }
    Ok(())
}

/// Rewrites the ranges and preview starts of `file`'s lines from byte
/// offsets in the lines on disk to offsets in their lossy decoding, which
/// the previews and the editor's text are made of: there, each invalid
/// sequence (1 to 3 bytes) is a 3-byte U+FFFD. An offset inside a char or an
/// invalid sequence goes to its start, or to its end for a range's end.
///
/// The lines are read again for it, though only those whose offsets could
/// differ: the ones whose preview has a U+FFFD, or doesn't hold the line
/// from its start to its last match's end (which must then be valid UTF-8).
/// In a file in a legacy encoding that's every line but those of ASCII, as
/// their bytes decode to other text whether or not they're valid UTF-8.
/// A line whose bytes no longer decode to its preview, the file having
/// changed since, is left as the search gave it, as is a UTF-16 file, whose
/// offsets are in the UTF-8 it was searched as.
///
/// That's one more read of each such line, up to its last match, from a file
/// just searched, on the search's thread. `cancelled` is checked before each
/// line and every [`CHECK_BYTES`] bytes read or decoded in one, so a rerun
/// or a dropped panel stops it within that much work.
fn decode_offsets(file: &mut FileMatches, cancelled: &dyn Fn() -> bool) -> Result<(), Cancelled> {
    let mut lines = file
        .lines
        .iter_mut()
        .filter(|line| may_shift(line) || may_differ_legacy(line))
        .peekable();
    if lines.peek().is_none() {
        return Ok(());
    }
    let Ok(source) = open_regular(&file.path) else {
        return Ok(());
    };
    let Ok(file_encoding) = FileEncoding::detect(&source) else {
        return Ok(());
    };
    let encoding = file_encoding;
    if matches!(encoding.name(), "UTF-16LE" | "UTF-16BE") {
        return Ok(());
    }
    let legacy = encoding.encoding != FileEncoding::UTF8.encoding;
    let bom_len = if file_encoding.bom && !legacy { 3 } else { 0 };
    for line in lines {
        check(cancelled)?;
        if !legacy && !may_shift(line) {
            continue;
        }
        let end = line.ranges.iter().map(|range| range.end).max().unwrap_or(0);
        let preview_end = line.preview_start + line.preview.len();
        // A decoded unit is never smaller than its bytes, so the preview's
        // bytes end by `preview_end`; and 3 more bytes settle the unit at
        // the end, as no unit is longer than 4.
        let len = end.max(preview_end) + 3;
        if let Ok((bytes, terminated)) = read_line(&source, line.line_offset, len, cancelled)? {
            if legacy {
                let ends = if terminated {
                    LineEnd::Newline
                } else if bytes.len() < len {
                    LineEnd::Eof
                } else {
                    LineEnd::Cut
                };
                decode_legacy_line(&bytes, ends, line, encoding, cancelled)?;
            } else {
                let moved = decode_line(&bytes, line, cancelled)?;
                if moved && line.line_offset == 0 && bom_len > 0 {
                    drop_bom(line, bom_len);
                }
            }
        }
    }
    Ok(())
}

/// Whether `line`'s offsets could be other than its decoding's.
fn may_shift(line: &LineMatch) -> bool {
    let end = line.ranges.iter().map(|range| range.end).max().unwrap_or(0);
    line.preview_start > 0
        || (line.line_offset == 0 && line.preview.starts_with('\u{FEFF}'))
        || end > line.preview.len()
        || line.preview.contains(char::REPLACEMENT_CHARACTER)
}

/// Whether `line`'s offsets could be other than its decoding's if its file
/// is in a legacy encoding: unless its bytes up to its last match are ASCII,
/// bar the ESC that ISO-2022-JP shifts with.
fn may_differ_legacy(line: &LineMatch) -> bool {
    !line.preview.is_ascii() || line.preview.contains('\u{1B}')
}

/// Opens the regular file at `path` for reading, without following a link
/// or waiting on a FIFO that has taken its place since it was searched.
#[cfg(unix)]
fn open_regular(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(file)
}

/// Elsewhere nothing: the offsets stay as the search gave them.
#[cfg(not(unix))]
fn open_regular(_: &Path) -> io::Result<File> {
    Err(io::ErrorKind::Unsupported.into())
}

/// Up to `len` bytes of the line at `offset` in `source`, short of its
/// `\n`, read [`CHECK_BYTES`] at a time with `cancelled` checked before
/// each, and whether they stopped at a `\n`.
fn read_line(
    source: &File,
    offset: u64,
    len: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<io::Result<(Vec<u8>, bool)>, Cancelled> {
    let mut bytes = Vec::new();
    let mut terminated = false;
    while bytes.len() < len {
        check(cancelled)?;
        let start = bytes.len();
        let end = len.min(start + CHECK_BYTES);
        bytes.resize(end, 0);
        let mut read = start;
        while read < end {
            match source.read_at(&mut bytes[read..end], offset + read as u64) {
                Ok(0) => break,
                Ok(n) => read += n,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Ok(Err(error)),
            }
        }
        bytes.truncate(read);
        if let Some(newline) = bytes[start..].iter().position(|&byte| byte == b'\n') {
            bytes.truncate(start + newline);
            terminated = true;
            break;
        }
        if read < end {
            break;
        }
    }
    Ok(Ok((bytes, terminated)))
}

/// Moves `line`'s offsets from `bytes`, the start of the line, to their
/// lossy decoding, and says whether it did, if `bytes` from the preview's start decode to its
/// preview and hold its matches.
fn decode_line(
    bytes: &[u8],
    line: &mut LineMatch,
    cancelled: &dyn Fn() -> bool,
) -> Result<bool, Cancelled> {
    let end = line.ranges.iter().map(|range| range.end).max().unwrap_or(0);
    let start = line.preview_start;
    if bytes.len() < end || bytes.len() < start {
        return Ok(false);
    }
    let window = &bytes[start..bytes.len().min(start + line.preview.len() + 3)];
    if !String::from_utf8_lossy(window).starts_with(&line.preview) {
        return Ok(false);
    }
    // Each offset, and whether it rounds up: the preview's start both ways,
    // as it must be where a unit starts, then each range's ends.
    let mut offsets = vec![(start, false), (start, true)];
    offsets.extend(
        line.ranges
            .iter()
            .flat_map(|range| [(range.start, false), (range.end, true)]),
    );
    let decoded = decode_checked(bytes, &offsets, cancelled)?;
    if decoded[0] != decoded[1] {
        return Ok(false);
    }
    line.preview_start = decoded[0];
    let (ends, _) = decoded[2..].as_chunks::<2>();
    for (range, &[start, end]) in line.ranges.iter_mut().zip(ends) {
        *range = start..end;
    }
    Ok(true)
}

/// Moves a BOM-prefixed file's first line, its offsets already in the lossy
/// decoding, to the editor's text, which has no BOM: back by its `bom_len`
/// bytes, and without the U+FEFF in the preview.
fn drop_bom(line: &mut LineMatch, bom_len: usize) {
    if line.preview_start == 0 && line.preview.starts_with('\u{FEFF}') {
        line.preview.remove(0);
    } else {
        line.preview_start = line.preview_start.saturating_sub(bom_len);
    }
    for range in &mut line.ranges {
        let start = range.start.saturating_sub(bom_len);
        let end = range.end.saturating_sub(bom_len);
        *range = start..end;
    }
}

/// [`decode_line`] for a file in a legacy `encoding`: the offsets and the
/// preview move to the text `encoding` decodes the bytes to, if the bytes
/// from the preview's start still read as the preview in lossy UTF-8. The
/// preview is that text over the bytes it covered, widened to whole decoded
/// units at both ends. `ends` says what `bytes` end at: the editor decodes
/// through a line's `\n`, which settles the bytes before it as it does, so
/// that is decoded too and left out of the preview.
pub(crate) fn decode_legacy_line(
    bytes: &[u8],
    ends: LineEnd,
    line: &mut LineMatch,
    encoding: FileEncoding,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), Cancelled> {
    let end = line.ranges.iter().map(|range| range.end).max().unwrap_or(0);
    let start = line.preview_start;
    if bytes.len() < end || bytes.len() < start {
        return Ok(());
    }
    let window = &bytes[start..bytes.len().min(start + line.preview.len() + 3)];
    if !String::from_utf8_lossy(window).starts_with(&line.preview) {
        return Ok(());
    }
    // The bytes the preview was made of: the lossy units from `start` up to
    // the preview's length.
    let mut preview_bytes = 0;
    let mut decoded_len = 0;
    for chunk in bytes[start..].utf8_chunks() {
        for c in chunk.valid().chars() {
            if decoded_len >= line.preview.len() {
                break;
            }
            preview_bytes += c.len_utf8();
            decoded_len += c.len_utf8();
        }
        if decoded_len < line.preview.len() && !chunk.invalid().is_empty() {
            preview_bytes += chunk.invalid().len();
            decoded_len += char::REPLACEMENT_CHARACTER.len_utf8();
        }
        if decoded_len >= line.preview.len() {
            break;
        }
    }
    let preview_bytes = start..start + preview_bytes;
    let offsets: Vec<_> = line
        .ranges
        .iter()
        .flat_map(|range| [(range.start, false), (range.end, true)])
        .collect();
    let mut order: Vec<usize> = (0..offsets.len()).collect();
    order.sort_by_key(|&ix| offsets[ix].0);
    let mut order = order.into_iter().peekable();
    let mut decoded = vec![0; offsets.len()];
    let mut preview = String::new();
    let mut preview_start = None;
    let mut out = 0;
    let mut block = 0;
    let mut fed = bytes.to_vec();
    if ends == LineEnd::Newline {
        fed.push(b'\n');
    }
    encoding.for_each_unit(&fed, ends == LineEnd::Eof, |unit, text| {
        // The `\n` is no part of the line.
        let text = if ends == LineEnd::Newline && unit.end == fed.len() {
            text.strip_suffix('\n').unwrap_or(text)
        } else {
            text
        };
        if unit.start / CHECK_BYTES != block {
            block = unit.start / CHECK_BYTES;
            check(cancelled)?;
        }
        let unit_out = out..out + text.len();
        // The offsets in this unit are at its start, or inside it and go to
        // its start, or to its end if they round up.
        while let Some(&ix) = order.peek() {
            let (offset, up) = offsets[ix];
            if offset >= unit.end {
                break;
            }
            decoded[ix] = if up && offset > unit.start {
                unit_out.end
            } else {
                unit_out.start
            };
            order.next();
        }
        if unit.end > preview_bytes.start && unit.start < preview_bytes.end {
            preview_start.get_or_insert(unit_out.start);
            preview.push_str(text);
        }
        out = unit_out.end;
        Ok(order.peek().is_none() && unit.end >= preview_bytes.end)
    })?;
    // The rest are at or past the end.
    for ix in order {
        decoded[ix] = out;
    }
    line.preview = preview;
    line.preview_start = preview_start.unwrap_or(out);
    let (ends, _) = decoded.as_chunks::<2>();
    for (range, &[start, end]) in line.ranges.iter_mut().zip(ends) {
        *range = start..end;
    }
    Ok(())
}

/// What the bytes read of a line end at.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum LineEnd {
    /// The end of the file.
    Eof,
    /// The line's `\n`, not among the bytes.
    Newline,
    /// The limit on the bytes read, in the line.
    Cut,
}

/// `offsets`, byte offsets in `bytes` and whether each rounds up, as offsets
/// in `bytes`' lossy decoding: one inside a char or an invalid sequence goes
/// to the start of its decoding, or its end if it rounds up.
#[cfg(test)]
pub(crate) fn decode(bytes: &[u8], offsets: &[(usize, bool)]) -> Vec<usize> {
    decode_checked(bytes, offsets, &|| false).expect("never cancelled")
}

/// [`decode`], a window of [`CHECK_BYTES`] at a time with `cancelled`
/// checked before each, stopping once every offset is placed.
fn decode_checked(
    bytes: &[u8],
    offsets: &[(usize, bool)],
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<usize>, Cancelled> {
    let mut order: Vec<usize> = (0..offsets.len()).collect();
    order.sort_by_key(|&ix| offsets[ix].0);
    let mut order = order.into_iter().peekable();
    let mut decoded = vec![0; offsets.len()];
    // Where the next unit starts, in `bytes` and decoded.
    let (mut at, mut out) = (0, 0);
    while at < bytes.len() && order.peek().is_some() {
        check(cancelled)?;
        let window_end = bytes.len().min(at + CHECK_BYTES);
        // A unit is settled by at most 4 bytes from its start, so where the
        // window cuts the line, the units starting in its last 3 bytes are
        // left to the next window, which starts with them.
        let settled = if window_end == bytes.len() {
            window_end
        } else {
            window_end - 3
        };
        // Each unit's length in `bytes`, then decoded: a char, or an
        // invalid sequence that decodes to U+FFFD.
        let units = bytes[at..window_end].utf8_chunks().flat_map(|chunk| {
            let chars = chunk.valid().chars().map(|c| (c.len_utf8(), c.len_utf8()));
            let invalid = chunk.invalid().len();
            let replaced =
                (invalid > 0).then_some((invalid, char::REPLACEMENT_CHARACTER.len_utf8()));
            chars.chain(replaced)
        });
        for (len, size) in units {
            if at >= settled {
                break;
            }
            // The offsets before this unit's end are in it, or at its start.
            while let Some(&ix) = order.peek() {
                let (offset, up) = offsets[ix];
                if at + len <= offset {
                    break;
                }
                decoded[ix] = if up && at < offset { out + size } else { out };
                order.next();
            }
            at += len;
            out += size;
        }
    }
    // The rest are at or past the end.
    for ix in order {
        decoded[ix] = out;
    }
    Ok(decoded)
}

impl Drop for SearchPanel {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Render for SearchPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        self.built.clear();
        // Focusing the panel as a whole focuses its query.
        if self.focus.is_focused(window) {
            window.focus(&gpui::Focusable::focus_handle(&self.input, cx), cx);
        }
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
            cx.processor(|panel, range, window, cx| panel.render_rows(range, window, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1();
        let results_context = if fe_gpui::vim_enabled(cx) {
            "search_results vim"
        } else {
            RESULTS_KEY_CONTEXT
        };
        let results = div()
            .key_context(results_context)
            .track_focus(&self.results_focus)
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(list);
        let replace_toggle = div()
            .flex_none()
            .pl_1()
            .debug_selector(|| "search-replace-toggle".into())
            .child(
                ToggleButton::new(
                    "search-replace-toggle",
                    if self.replace_shown { "▾" } else { "▸" },
                    &ui,
                )
                .on(self.replace_shown)
                .on_click(cx.listener(|panel, _, window, cx| panel.toggle_replace(window, cx))),
            );
        let replace = self.replace_shown.then(|| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .pr_2()
                .child(div().flex_1().min_w_0().child(self.replace.clone()))
                .child(
                    div()
                        .flex_none()
                        .debug_selector(|| "search-replace-all".into())
                        .child(
                            ToggleButton::new("search-replace-all", "Replace All", &ui).on_click(
                                cx.listener(|panel, _, window, cx| {
                                    panel.confirm_replace_all(window, cx)
                                }),
                            ),
                        ),
                )
        });
        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_run_command))
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
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .pr_2()
                    .child(replace_toggle)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .on_key_down(cx.listener(Self::on_query_key))
                            .child(self.input.clone()),
                    )
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
            .children(replace)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .child(self.include.clone())
                    .child(self.exclude.clone()),
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
            .child(results)
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
    /// Starts a generation whose results are `files` at once, as a search
    /// that finishes before the next frame would.
    pub(crate) fn rerun_with(&mut self, files: Vec<FileMatches>, cx: &mut Context<Self>) {
        let generation = self.restart(cx);
        self.append(generation, files, cx);
        self.status = Status::Done(SearchSummary::default());
    }

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

    pub(crate) fn include_input(&self) -> &Entity<TextInput> {
        &self.include
    }

    pub(crate) fn exclude_input(&self) -> &Entity<TextInput> {
        &self.exclude
    }

    pub(crate) fn replace_input(&self) -> &Entity<TextInput> {
        &self.replace
    }

    /// The selected row, in [`SearchPanel::shown_rows`].
    pub(crate) fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub(crate) fn results_focused(&self, window: &Window) -> bool {
        self.results_focus.is_focused(window)
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

#[path = "search_replace.rs"]
mod search_replace;

#[cfg(test)]
#[path = "search_panel_filter_tests.rs"]
mod filter_tests;
