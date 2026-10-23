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
//! query) and moves through the file and match rows with
//! `search.select_next` and `search.select_previous`;
//! `search.open_selected` opens the selected match, or a file's first.
//! Clicking a file's row collapses its group, or expands it, as
//! `search.collapse_or_parent` and `search.expand_or_first_child` do, which
//! otherwise move to the parent file or to its first line, as VS Code's tree
//! does. A collapsed file's lines are hidden but still in the results, so
//! Replace All and the counts include them. Its element's key context is
//! [`RESULTS_KEY_CONTEXT`], with `vim` too while vim is on. The fields are
//! `text_input`s, so vim never sees the keys typed in them.
//!
//! A toolbar above the query, as VS Code's Search view has, reruns the query
//! (`search.refresh`), clears it and the results (`search.clear_results`),
//! and collapses every group, or expands them all once all are collapsed
//! (`search.collapse_all`, `search.expand_all`). A new run's groups are
//! expanded.
//!
//! View as Tree (`search.view_as_tree`, or the toolbar's button, which is
//! View as List in the tree) groups the files under folder rows by their
//! paths from the search root, as VS Code's tree does: a chain of folders
//! that each hold only one folder is one row, `a/b/c`, and each folder's
//! row counts the matches under it, collapses like a file's, and replaces
//! in or dismisses every file under it. A folder's collapse is kept by its
//! path, across switches back to the list, which shows every file.
//!
//! The panel keeps each line's ranges and preview start as offsets in the
//! line's lossy decoding, which is what the preview and the editor's text
//! are made of, rather than in its bytes on disk as the search gives them:
//! see [`decode_offsets`].

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs::{self, File},
    io,
    ops::Range,
    os::unix::fs::FileExt as _,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_buffer::FileEncoding;
use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use fe_project::{
    FileMatches, LineMatch, PathFilter, Replacer, SearchError, SearchOptions, SearchQuery,
    SearchSummary,
};
use fe_ui::{
    HighlightedLabel, Icon, IconButton, ROW_HEIGHT, TREE_INDENT, TextInput, TextInputEvent,
    ToggleButton, Tooltip, UiTheme,
};
use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, FocusHandle, Global,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, PromptButton,
    PromptLevel, Render, ScrollHandle, ScrollStrategy, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, UniformListScrollHandle,
    WeakEntity, Window, div, prelude::FluentBuilder as _, px, svg, uniform_list,
};

use crate::{
    Pane,
    pane::Reveal,
    picker::{self, PaletteOp},
    workbench_commands::{ReplaceAllInFiles, ReplaceInFiles, invoked_in},
};
use search_replace::{Boundary, Target, count};

/// Project search in the sidebar: focused when shown, for its input.
pub(crate) const SIDEBAR: crate::dock::SidebarPanel = crate::dock::SidebarPanel {
    panel: crate::DockPanel::new("search"),
    name: "Search",
    icon: fe_ui::Icon::Search,
    show_command: "search.show",
    show_op: crate::dock::DockOp::ShowSearch,
    order: 20,
    always_on_rail: true,
    focus_on_show: true,
};

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

/// What a folder or file row's Replace or dismiss button does.
type RowAction = Box<dyn Fn(&mut SearchPanel, &mut Context<SearchPanel>)>;

/// A result row: a folder's, in the tree, by index in the build's
/// folders; a file's header; or one of its matching lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResultRow {
    Folder(usize),
    File(usize),
    Line(usize, usize),
}

/// A folder row of the tree.
struct Folder {
    /// From the search root, the first and the last of the folders
    /// compacted into the row.
    top: PathBuf,
    path: PathBuf,
    /// Their names joined: `a/b/c`.
    label: String,
    /// The matches in the files under it, at any depth.
    matches: usize,
}

impl Folder {
    /// The folders compacted into the row, from the last.
    fn chain(&self) -> impl Iterator<Item = &Path> {
        (self.path.ancestors()).take_while(|dir| dir.starts_with(&self.top))
    }
}

/// A row as a rebuild keeps it selected: a folder by its path, as the
/// folders' indices are the build's.
enum Kept {
    Folder(PathBuf),
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
    /// Whether the include and exclude fields are shown. Hidden globs still
    /// filter every search.
    details_shown: bool,
    /// The results list's, which moves through the rows.
    results_focus: FocusHandle,
    /// The selected row, in `rows`.
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
    /// The files, by index in `files`, whose line rows are hidden. Each run
    /// starts with none.
    collapsed: BTreeSet<usize>,
    /// The folders, by path from the search root, whose rows' descendants
    /// the tree hides: a row of compacted folders is collapsed if any of
    /// them is. Each run starts with none.
    collapsed_folders: BTreeSet<PathBuf>,
    /// Whether the results are a tree of folders rather than a list.
    view_as_tree: bool,
    /// Each file's row, then its lines' unless it's collapsed, under their
    /// folders' in the tree: see [`SearchPanel::rebuild_rows`].
    rows: Vec<ResultRow>,
    /// Each row's depth: a list's files are at 0 and their lines at 1.
    depths: Vec<usize>,
    /// The tree's folder rows', in the order built.
    folders: Vec<Folder>,
    /// The last replacement's outcome, and whether it's an error: the
    /// status line until the next search the user runs.
    notice: Option<(String, bool)>,
    replacing: Option<Task<()>>,
    /// Moves on with each Clear Search Results, after which a replacement
    /// already running finishes its writes but neither searches again nor
    /// says what it did.
    clears: u64,
    scroll: UniformListScrollHandle,
    toggles: ScrollHandle,
    /// The row ranges the last frame built.
    #[cfg(test)]
    built: Vec<Range<usize>>,
    _inputs: [Subscription; 4],
}

impl SearchPanel {
    /// Searches under `root`, opening matches in `pane`. Touches nothing
    /// until a query runs.
    pub fn new(root: Option<PathBuf>, pane: WeakEntity<Pane>, cx: &mut Context<Self>) -> Self {
        let ui = fe_ui::ui_theme(cx);
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
            details_shown: false,
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
            collapsed: BTreeSet::new(),
            collapsed_folders: BTreeSet::new(),
            view_as_tree: false,
            rows: Vec::new(),
            depths: Vec::new(),
            folders: Vec::new(),
            notice: None,
            replacing: None,
            clears: 0,
            scroll: UniformListScrollHandle::new(),
            toggles: ScrollHandle::new(),
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
        self.running = None;
        self.searched = None;
        self.boundary = None;
        self.files.clear();
        self.collapsed.clear();
        self.collapsed_folders.clear();
        self.selected = None;
        self.rebuild_rows();
        self.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
        self.generation
    }

    fn append(&mut self, generation: u64, batch: Vec<FileMatches>, cx: &mut Context<Self>) {
        if generation != self.generation {
            return;
        }
        let from = self.files.len();
        self.files.extend(batch);
        // The tree's rows move as files come in under its folders.
        if self.view_as_tree {
            self.rebuild_rows();
        } else {
            self.push_rows(from);
        }
        cx.notify();
    }

    /// Adds the list's rows of the files from the `from`th on: each file's,
    /// then its lines' unless it's collapsed.
    fn push_rows(&mut self, from: usize) {
        for ix in from..self.files.len() {
            self.push_file(ix, self.files[ix].lines.len(), 0);
        }
    }

    /// Adds the `file`th file's row at `depth`, then its `lines` lines'
    /// unless it's collapsed.
    fn push_file(&mut self, file: usize, lines: usize, depth: usize) {
        self.rows.push(ResultRow::File(file));
        self.depths.push(depth);
        if !self.collapsed.contains(&file) {
            (self.rows).extend((0..lines).map(|line| ResultRow::Line(file, line)));
            (self.depths).extend(std::iter::repeat_n(depth + 1, lines));
        }
    }

    /// Adds the tree's rows of `files`: [`Dir`]'s folders, compacted, before
    /// its files, each by name, and a folder's descendants after it unless
    /// it's collapsed.
    fn push_tree(&mut self, files: &[FileMatches], dir: &Dir, path: &Path, depth: usize) {
        for (folder, child) in dir.folders(path) {
            self.rows.push(ResultRow::Folder(self.folders.len()));
            self.depths.push(depth);
            let collapsed = (folder.chain()).any(|dir| self.collapsed_folders.contains(dir));
            let path = folder.path.clone();
            self.folders.push(folder);
            if !collapsed {
                self.push_tree(files, child, &path, depth + 1);
            }
        }
        for &(_, file) in &dir.files {
            self.push_file(file, files[file].lines.len(), depth);
        }
    }

    /// Builds the rows again, as a new layout, with the selected row still
    /// selected, or the nearest shown row that holds it if a collapse or a
    /// switch of view hid it.
    fn rebuild_rows(&mut self) {
        let kept = self.selected_row().map(|(_, row)| match row {
            ResultRow::Folder(folder) => Kept::Folder(self.folders[folder].path.clone()),
            ResultRow::File(file) => Kept::File(file),
            ResultRow::Line(file, line) => Kept::Line(file, line),
        });
        self.rows.clear();
        self.depths.clear();
        self.folders.clear();
        if self.view_as_tree {
            // Out of the panel while the tree, which borrows them, is read.
            let files = std::mem::take(&mut self.files);
            let root = self.root.clone();
            self.push_tree(&files, &Dir::of(&files, root.as_deref()), Path::new(""), 0);
            self.files = files;
        } else {
            self.push_rows(0);
        }
        self.layout += 1;
        self.selected = kept.and_then(|kept| self.position_of(&kept));
    }

    /// Where `kept` is in the rows: its own row, or else its line's file's,
    /// or else the deepest shown folder holding it, or else, for a folder
    /// in the list, its first file's.
    fn position_of(&self, kept: &Kept) -> Option<usize> {
        let shown = |row| self.rows.iter().position(|&shown| shown == row);
        let (own, path) = match *kept {
            Kept::Folder(ref path) => {
                let own = self.rows.iter().position(|&row| {
                    matches!(row, ResultRow::Folder(folder) if self.folders[folder].path == *path)
                });
                (own, path.as_path())
            }
            Kept::File(file) => (
                shown(ResultRow::File(file)),
                self.rel_path(self.files.get(file)?),
            ),
            Kept::Line(file, line) => (
                shown(ResultRow::Line(file, line)).or_else(|| shown(ResultRow::File(file))),
                self.rel_path(self.files.get(file)?),
            ),
        };
        let folder = || {
            (self.rows.iter().enumerate())
                .filter_map(|(ix, &row)| match row {
                    ResultRow::Folder(folder) => Some((ix, &self.folders[folder].path)),
                    _ => None,
                })
                .filter(|(_, folder)| under(path, folder))
                .max_by_key(|(_, folder)| folder.components().count())
                .map(|(ix, _)| ix)
        };
        let file = || {
            self.rows.iter().position(|&row| {
                matches!(row, ResultRow::File(file) if under(self.rel_path(&self.files[file]), path))
            })
        };
        own.or_else(folder).or_else(file)
    }

    /// `file`'s path from the search root, as its folder rows group it.
    fn rel_path<'a>(&self, file: &'a FileMatches) -> &'a Path {
        (self.root.as_deref())
            .and_then(|root| file.path.strip_prefix(root).ok())
            .unwrap_or(Path::new(&file.rel))
    }

    /// Whether `row`'s descendants are hidden, unless it's a line's.
    fn is_collapsed(&self, row: ResultRow) -> Option<bool> {
        match row {
            ResultRow::Folder(folder) => {
                Some((self.folders[folder].chain()).any(|dir| self.collapsed_folders.contains(dir)))
            }
            ResultRow::File(file) => Some(self.collapsed.contains(&file)),
            ResultRow::Line(..) => None,
        }
    }

    /// Hides `row`'s descendants, or shows them if not `collapse`: a
    /// file's lines, or a folder's folders and files.
    /// A folder row expands by expanding every folder compacted into it.
    fn set_collapsed(&mut self, row: ResultRow, collapse: bool, cx: &mut Context<Self>) {
        if self.is_collapsed(row) != Some(!collapse) {
            return;
        }
        match row {
            ResultRow::Folder(folder) if collapse => {
                (self.collapsed_folders).insert(self.folders[folder].path.clone());
            }
            ResultRow::Folder(folder) => {
                for dir in self.folders[folder].chain() {
                    self.collapsed_folders.remove(dir);
                }
            }
            ResultRow::File(file) if collapse => {
                self.collapsed.insert(file);
            }
            ResultRow::File(file) => {
                self.collapsed.remove(&file);
            }
            ResultRow::Line(..) => {}
        }
        self.rebuild_rows();
        cx.notify();
    }

    /// Whether there are results and every folder and file shown is
    /// collapsed: the toolbar then offers Expand All rather than Collapse
    /// All.
    fn all_collapsed(&self) -> bool {
        !self.files.is_empty()
            && (self.rows.iter()).all(|&row| self.is_collapsed(row).is_some_and(|shut| shut))
    }

    /// Collapses every file, and in the tree every folder, or expands them
    /// all if not `collapse`.
    fn collapse_all(&mut self, collapse: bool, cx: &mut Context<Self>) {
        let (files, folders) = if collapse {
            let folders = if self.view_as_tree {
                let mut all = BTreeSet::new();
                let tree = Dir::of(&self.files, self.root.as_deref());
                tree.all_folders(Path::new(""), &mut all);
                all
            } else {
                self.collapsed_folders.clone()
            };
            ((0..self.files.len()).collect(), folders)
        } else if self.view_as_tree {
            (BTreeSet::new(), BTreeSet::new())
        } else {
            (BTreeSet::new(), self.collapsed_folders.clone())
        };
        if files != self.collapsed || folders != self.collapsed_folders {
            self.collapsed = files;
            self.collapsed_folders = folders;
            self.rebuild_rows();
            cx.notify();
        }
    }

    /// Shows the results as a tree of folders, or as a list if not
    /// `tree`, with the same results, collapses and selected row: the tree
    /// expands the folders a selected file or match is in, to show it.
    fn set_view_as_tree(&mut self, tree: bool, cx: &mut Context<Self>) {
        if self.view_as_tree == tree {
            return;
        }
        self.view_as_tree = tree;
        if let Some((_, ResultRow::File(file) | ResultRow::Line(file, _))) = self.selected_row()
            && tree
        {
            let rel = self.rel_path(&self.files[file]).to_owned();
            self.collapsed_folders.retain(|dir| !under(&rel, dir));
        }
        self.rebuild_rows();
        if let Some(ix) = self.selected {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    /// Whether Refresh applies: unless the query is empty.
    fn can_refresh(&self, cx: &App) -> bool {
        !self.input.read(cx).text().is_empty()
    }

    /// Reruns the query, with the options and filters as they are, unless
    /// it's empty.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.can_refresh(cx) {
            self.run(cx);
        }
    }

    /// Whether Clear Search Results applies: unless the fields are all
    /// empty and there are no results.
    fn can_clear(&self, cx: &App) -> bool {
        let fields = [&self.input, &self.replace, &self.include, &self.exclude];
        !self.files.is_empty() || fields.iter().any(|input| !input.read(cx).text().is_empty())
    }

    /// Cancels the running search and empties the query, the replacement,
    /// the filters, the results and the status, then focuses the query. The
    /// options stay, as do whether replace and the filters are shown.
    fn clear_results(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restart(cx);
        self.clears += 1;
        self.notice = None;
        self.status = Status::Idle;
        for input in [&self.input, &self.replace, &self.include, &self.exclude] {
            input.update(cx, |input, cx| input.set_text("", cx));
        }
        window.focus(&gpui::Focusable::focus_handle(&self.input, cx), cx);
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
                self.collapsed.clear();
                self.collapsed_folders.clear();
                self.selected = None;
                self.rebuild_rows();
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

    /// Opens the file of row `ix`, if the rows are still `layout`'s, with
    /// its line's first match selected, or the file's first for a file row.
    /// A folder's row collapses, or expands.
    fn open(&mut self, layout: u64, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (file, line) = match self.rows.get(ix) {
            Some(&ResultRow::Line(file, line)) => (file, line),
            Some(&ResultRow::File(file)) => (file, 0),
            Some(&row @ ResultRow::Folder(_)) => {
                if layout == self.layout {
                    let collapsed = self.is_collapsed(row) == Some(true);
                    self.set_collapsed(row, !collapsed, cx);
                }
                return;
            }
            None => return,
        };
        if layout != self.layout {
            return;
        }
        let file = &self.files[file];
        let Some(line) = file.lines.get(line) else {
            return;
        };
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

    /// Selects folder or file row `ix` and collapses it, or expands it if
    /// it's collapsed, if the rows are still `layout`'s.
    fn click_node(&mut self, layout: u64, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if layout != self.layout {
            return;
        }
        let Some(&row) = self.rows.get(ix) else {
            return;
        };
        let Some(collapsed) = self.is_collapsed(row) else {
            return;
        };
        self.selected = Some(ix);
        window.focus(&self.results_focus, cx);
        self.set_collapsed(row, !collapsed, cx);
        cx.notify();
    }

    /// Runs a search option, a replace, toolbar or results command, Enter
    /// or an input edit; others go on to the workbench.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        let option = match op(run, cx) {
            Some(SearchOp::ToggleRegex) => Some(SearchOption::Regex),
            Some(SearchOp::ToggleCase) => Some(SearchOption::MatchCase),
            Some(SearchOp::ToggleWholeWord) => Some(SearchOption::WholeWord),
            Some(SearchOp::Submit) => {
                return self
                    .focused_input(window, cx)
                    .clone()
                    .update(cx, TextInput::submit);
            }
            Some(SearchOp::ToggleReplace) => return self.toggle_replace(window, cx),
            Some(SearchOp::ToggleDetails) => return self.toggle_details(window, cx),
            Some(SearchOp::FocusResults) => return self.focus_results(window, cx),
            Some(SearchOp::SelectNext) => return self.select_next(true, cx),
            Some(SearchOp::SelectPrevious) => return self.select_next(false, cx),
            Some(SearchOp::OpenSelected) => {
                if let Some(ix) = self.selected {
                    self.open(self.layout, ix, window, cx);
                }
                return;
            }
            Some(SearchOp::CollapseOrParent) => return self.collapse_or_parent(cx),
            Some(SearchOp::ExpandOrFirstChild) => return self.expand_or_first_child(cx),
            Some(SearchOp::Refresh) => return self.refresh(cx),
            Some(SearchOp::ClearResults) => return self.clear_results(window, cx),
            Some(SearchOp::CollapseAll) => return self.collapse_all(true, cx),
            Some(SearchOp::ExpandAll) => return self.collapse_all(false, cx),
            Some(SearchOp::ViewAsTree) => return self.set_view_as_tree(true, cx),
            Some(SearchOp::ViewAsList) => return self.set_view_as_tree(false, cx),
            None => None,
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

    /// Shows or hides the include and exclude fields, focusing the query if
    /// one of them had focus.
    fn toggle_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.details_shown = !self.details_shown;
        if !self.details_shown {
            let filter_focused = [&self.include, &self.exclude]
                .into_iter()
                .any(|input| gpui::Focusable::focus_handle(input, cx).is_focused(window));
            if filter_focused {
                window.focus(&gpui::Focusable::focus_handle(&self.input, cx), cx);
            }
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

    /// The results' `files`, by index.
    fn targets(&self, files: impl IntoIterator<Item = usize>) -> Vec<Target> {
        (files.into_iter())
            .map(|ix| &self.files[ix])
            .map(|file| Target {
                path: file.path.clone(),
                rel: file.rel.clone(),
            })
            .collect()
    }

    /// The results' files under the `folder`th folder row, by index.
    fn files_under(&self, folder: usize) -> Vec<usize> {
        let path = &self.folders[folder].path;
        (self.files.iter().enumerate())
            .filter(|(_, file)| under(self.rel_path(file), path))
            .map(|(ix, _)| ix)
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
        let targets = self.targets(0..self.files.len());
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

    /// Puts `template` in the replace field, shown, then asks and replaces
    /// as [`SearchPanel::confirm_replace_all`] does: vim's `:Replace`.
    /// Nothing while a replacement runs.
    pub(crate) fn confirm_replace_all_with(
        &mut self,
        template: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.replacing.is_some() {
            return;
        }
        self.replace
            .update(cx, |input, cx| input.set_text(template, cx));
        self.replace_shown = true;
        self.confirm_replace_all(window, cx);
    }

    /// Replaces the matches in the `file`th file of the results, if the
    /// rows are still `layout`'s, without asking.
    fn replace_in_file(&mut self, layout: u64, file: usize, cx: &mut Context<Self>) {
        if layout == self.layout && file < self.files.len() {
            self.replace_in(vec![file], cx);
        }
    }

    /// Replaces the matches in every file under the `folder`th folder row,
    /// if the rows are still `layout`'s, without asking.
    fn replace_in_folder(&mut self, layout: u64, folder: usize, cx: &mut Context<Self>) {
        if layout == self.layout && folder < self.folders.len() {
            self.replace_in(self.files_under(folder), cx);
        }
    }

    /// Replaces the matches in the results' `files`, by index, without
    /// asking. Nothing while a replacement runs.
    fn replace_in(&mut self, files: Vec<usize>, cx: &mut Context<Self>) {
        if self.replacing.is_some() {
            return;
        }
        let Some(query) = self.searched.clone() else {
            return;
        };
        let template = self.replace.read(cx).text().to_owned();
        let targets = self.targets(files);
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
        let clears = self.clears;
        self.replacing = Some(cx.spawn(async move |panel, cx| {
            let outcome = search_replace::replace(boundary, targets, replacer, cx).await;
            panel
                .update(cx, |panel, cx| {
                    panel.replacing = None;
                    if panel.clears == clears {
                        panel.run(cx);
                        panel.notice = Some(outcome.message());
                    }
                    cx.notify();
                })
                .ok();
        }));
        cx.notify();
    }

    /// Drops the `file`th file from the results, if the rows are still
    /// `layout`'s, so Replace All skips it.
    fn dismiss(&mut self, layout: u64, file: usize, cx: &mut Context<Self>) {
        if layout == self.layout && file < self.files.len() {
            self.drop_files(BTreeSet::from([file]), cx);
        }
    }

    /// Drops every file under the `folder`th folder row from the results,
    /// if the rows are still `layout`'s, so Replace All skips them.
    fn dismiss_folder(&mut self, layout: u64, folder: usize, cx: &mut Context<Self>) {
        if layout == self.layout && folder < self.folders.len() {
            let files = self.files_under(folder).into_iter().collect();
            self.drop_files(files, cx);
        }
    }

    /// Drops the results' `files`, by index.
    fn drop_files(&mut self, files: BTreeSet<usize>, cx: &mut Context<Self>) {
        let mut ix = 0;
        self.files.retain(|_| {
            ix += 1;
            !files.contains(&(ix - 1))
        });
        // The files after them move down, collapsed or not.
        self.collapsed = (self.collapsed.iter())
            .filter(|ix| !files.contains(ix))
            .map(|&ix| ix - files.range(..ix).count())
            .collect();
        self.selected = None;
        self.rebuild_rows();
        cx.notify();
    }

    /// Focuses the results list, selecting its first match row, or its
    /// first row if every file is collapsed, if none is selected.
    fn focus_results(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_none() {
            let line = (self.rows.iter()).position(|row| matches!(row, ResultRow::Line(..)));
            if let Some(ix) = line.or((!self.rows.is_empty()).then_some(0)) {
                self.select(ix, cx);
            }
        }
        window.focus(&self.results_focus, cx);
        cx.notify();
    }

    /// Selects row `ix`, scrolled into view.
    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = Some(ix);
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// The selected row, and where it is, if it's one of the rows.
    fn selected_row(&self) -> Option<(usize, ResultRow)> {
        let ix = self.selected?;
        Some((ix, *self.rows.get(ix)?))
    }

    /// Selects the row after the selected one, file or line, or the one
    /// before it if not `forward`; the first with none selected.
    fn select_next(&mut self, forward: bool, cx: &mut Context<Self>) {
        let next = match self.selected_row() {
            None => (!self.rows.is_empty()).then_some(0),
            Some((at, _)) if forward => (at + 1 < self.rows.len()).then_some(at + 1),
            Some((at, _)) => at.checked_sub(1),
        };
        if let Some(ix) = next {
            self.select(ix, cx);
        }
    }

    /// Left: collapses the selected folder or file if it's expanded, or
    /// else selects the row it's in: a line's file, or a folder.
    fn collapse_or_parent(&mut self, cx: &mut Context<Self>) {
        let Some((at, row)) = self.selected_row() else {
            return;
        };
        if self.is_collapsed(row) == Some(false) {
            return self.set_collapsed(row, true, cx);
        }
        let depth = self.depths[at];
        if let Some(ix) = self.depths[..at].iter().rposition(|&above| above < depth) {
            self.select(ix, cx);
        }
    }

    /// Right: expands the selected folder or file if it's collapsed, or
    /// else selects its first child.
    fn expand_or_first_child(&mut self, cx: &mut Context<Self>) {
        let Some((at, row)) = self.selected_row() else {
            return;
        };
        match self.is_collapsed(row) {
            Some(true) => self.set_collapsed(row, false, cx),
            Some(false) if self.depths.get(at + 1) > Some(&self.depths[at]) => {
                self.select(at + 1, cx)
            }
            _ => {}
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
        let ui = fe_ui::ui_theme(cx);
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
                let depth = self.depths[ix];
                // The list's rows keep their padding; the tree's are indented
                // one step per depth.
                let tree = self.view_as_tree;
                let node = |row: gpui::Stateful<gpui::Div>,
                            label: SharedString,
                            matches: usize,
                            expanded: bool,
                            on_replace: RowAction,
                            on_dismiss: RowAction,
                            cx: &mut Context<Self>| {
                    let replace = self.replace_shown.then(|| {
                        div()
                            .flex_none()
                            .debug_selector(move || format!("search-replace-file:{ix}"))
                            .child(
                                ToggleButton::new(("search-replace-file", ix), "Replace", &ui)
                                    .on_click(cx.listener(move |panel, _, _, cx| {
                                        cx.stop_propagation();
                                        on_replace(panel, cx)
                                    })),
                            )
                    });
                    let dismiss = div()
                        .flex_none()
                        .debug_selector(move || format!("search-dismiss:{ix}"))
                        .child(
                            ToggleButton::new(("search-dismiss", ix), "×", &ui).on_click(
                                cx.listener(move |panel, _, _, cx| {
                                    cx.stop_propagation();
                                    on_dismiss(panel, cx)
                                }),
                            ),
                        );
                    let chevron = svg()
                        .path(if expanded {
                            Icon::ChevronDown.path()
                        } else {
                            Icon::ChevronRight.path()
                        })
                        .size(px(14.))
                        .flex_none()
                        .text_color(ui.muted_text)
                        .debug_selector(move || {
                            let state = if expanded { "open" } else { "closed" };
                            format!("search-chevron:{ix}:{state}")
                        });
                    row.pl_1()
                        .when(tree, |row| row.pl(px(4.) + TREE_INDENT * depth as f32))
                        .gap_1()
                        .text_color(ui.tab_active_text)
                        .hover(move |row| row.bg(ui.row_hover))
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            panel.click_node(layout, ix, window, cx)
                        }))
                        .child(chevron)
                        .child(div().flex_1().min_w_0().overflow_hidden().child(label))
                        .child(
                            div()
                                .flex_none()
                                .text_color(ui.muted_text)
                                .child(matches.to_string()),
                        )
                        .children(replace)
                        .child(dismiss)
                };
                match self.rows[ix] {
                    ResultRow::Folder(folder_ix) => {
                        let folder = &self.folders[folder_ix];
                        node(
                            row,
                            folder.label.clone().into(),
                            folder.matches,
                            self.is_collapsed(ResultRow::Folder(folder_ix)) == Some(false),
                            Box::new(move |panel, cx| {
                                panel.replace_in_folder(layout, folder_ix, cx)
                            }),
                            Box::new(move |panel, cx| panel.dismiss_folder(layout, folder_ix, cx)),
                            cx,
                        )
                    }
                    ResultRow::File(file_ix) => {
                        let file = &self.files[file_ix];
                        let matches = file.lines.iter().map(|line| line.ranges.len()).sum();
                        node(
                            row,
                            self.file_label(file_ix),
                            matches,
                            self.is_collapsed(ResultRow::File(file_ix)) == Some(false),
                            Box::new(move |panel, cx| panel.replace_in_file(layout, file_ix, cx)),
                            Box::new(move |panel, cx| panel.dismiss(layout, file_ix, cx)),
                            cx,
                        )
                    }
                    ResultRow::Line(file, line) => {
                        let line = &self.files[file].lines[line];
                        let indent = TREE_INDENT * depth.saturating_sub(1) as f32;
                        row.pl_6()
                            .when(tree, |row| row.pl(px(24.) + indent))
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

impl SearchPanel {
    /// What the `file`th file's row shows: its name in the tree, or else its
    /// path from the search root.
    fn file_label(&self, file: usize) -> SharedString {
        let rel = &self.files[file].rel;
        let name = rel.rsplit_once('/').map_or(rel.as_str(), |(_, name)| name);
        if self.view_as_tree { name } else { rel }.to_owned().into()
    }
}

/// A line's preview, its matches highlighted ([`line_highlights`]).
fn line_label(line: &LineMatch, ui: &UiTheme) -> HighlightedLabel {
    HighlightedLabel::new(line.preview.clone(), line_highlights(line), ui)
}

/// A line's matches in its preview: shifted by `preview_start`, clipped to
/// the preview and widened to its char boundaries, and then merged where
/// they overlap, as two byte matches in one char do.
fn line_highlights(line: &LineMatch) -> Vec<Range<usize>> {
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
    highlights
}

/// The results' folders, as a tree from the search root, for the tree's
/// rows: by their names as they are, not as they're shown, so two that
/// aren't UTF-8 are two folders though they read the same.
#[derive(Default)]
struct Dir<'a> {
    /// By name.
    dirs: BTreeMap<&'a OsStr, Dir<'a>>,
    /// Each file's name and index in the results, by name.
    files: Vec<(&'a OsStr, usize)>,
    /// The matches in the files under it, at any depth.
    matches: usize,
}

impl<'a> Dir<'a> {
    /// The root of `files`' folders, by their paths from `root`.
    fn of(files: &'a [FileMatches], root: Option<&Path>) -> Self {
        let mut tree = Dir::default();
        for (ix, file) in files.iter().enumerate() {
            let matches: usize = file.lines.iter().map(|line| line.ranges.len()).sum();
            let rel = (root.and_then(|root| file.path.strip_prefix(root).ok()))
                .unwrap_or(Path::new(&file.rel));
            let mut names = rel.iter();
            let name = names.next_back().unwrap_or_default();
            let mut dir = &mut tree;
            dir.matches += matches;
            for folder in names {
                dir = dir.dirs.entry(folder).or_default();
                dir.matches += matches;
            }
            dir.files.push((name, ix));
        }
        tree.sort_files();
        tree
    }

    fn sort_files(&mut self) {
        self.files.sort_unstable();
        self.dirs.values_mut().for_each(Dir::sort_files);
    }

    /// The folder rows under this folder, at `path`, by name, each with the
    /// folder whose contents it shows. A chain of folders that each hold
    /// only one folder, and no files, is one row, labelled with their names
    /// joined.
    fn folders(&self, path: &Path) -> Vec<(Folder, &Dir<'a>)> {
        (self.dirs.iter())
            .map(|(&name, mut dir)| {
                let top = path.join(name);
                let (mut path, mut label) = (top.clone(), name.to_string_lossy().into_owned());
                while dir.files.is_empty()
                    && dir.dirs.len() == 1
                    && let Some((&name, only)) = dir.dirs.first_key_value()
                {
                    path.push(name);
                    label = format!("{label}/{}", name.to_string_lossy());
                    dir = only;
                }
                let matches = dir.matches;
                (
                    Folder {
                        top,
                        path,
                        label,
                        matches,
                    },
                    dir,
                )
            })
            .collect()
    }

    /// Every folder row's path under this folder, at `path`, at any depth.
    fn all_folders(&self, path: &Path, all: &mut BTreeSet<PathBuf>) {
        for (folder, dir) in self.folders(path) {
            dir.all_folders(&folder.path, all);
            all.insert(folder.path);
        }
    }
}

/// Whether `path` is in the folder at `folder`, at any depth.
fn under(path: &Path, folder: &Path) -> bool {
    path != folder && path.starts_with(folder)
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

/// What a results toolbar button does.
type ToolbarAction = fn(&mut SearchPanel, &mut Window, &mut Context<SearchPanel>);

impl SearchPanel {
    /// The results toolbar's buttons: each one's debug selector and ID,
    /// icon, tooltip, whether it's enabled, and what it does. View as Tree
    /// is View as List in the tree, and Collapse All is Expand All once
    /// every folder and file shown is collapsed.
    fn toolbar_buttons(
        &self,
        cx: &App,
    ) -> [(&'static str, Icon, &'static str, bool, ToolbarAction); 4] {
        let view: (Icon, &str, ToolbarAction) = if self.view_as_tree {
            (Icon::List, "View as List", |panel, _, cx| {
                panel.set_view_as_tree(false, cx)
            })
        } else {
            (Icon::ListTree, "View as Tree", |panel, _, cx| {
                panel.set_view_as_tree(true, cx)
            })
        };
        let collapse: (Icon, &str, ToolbarAction) = if self.all_collapsed() {
            (Icon::UnfoldVertical, "Expand All", |panel, _, cx| {
                panel.collapse_all(false, cx)
            })
        } else {
            (Icon::FoldVertical, "Collapse All", |panel, _, cx| {
                panel.collapse_all(true, cx)
            })
        };
        [
            (
                "search-toolbar-refresh",
                Icon::RefreshCw,
                "Refresh",
                self.can_refresh(cx),
                |panel, _, cx| panel.refresh(cx),
            ),
            (
                "search-toolbar-clear",
                Icon::ListX,
                "Clear Search Results",
                self.can_clear(cx),
                Self::clear_results,
            ),
            ("search-toolbar-view-mode", view.0, view.1, true, view.2),
            (
                "search-toolbar-collapse",
                collapse.0,
                collapse.1,
                !self.files.is_empty(),
                collapse.2,
            ),
        ]
    }

    /// The results toolbar, right-aligned above the query, as
    /// [`crate::git_panel`] draws its buttons.
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ui = fe_ui::ui_theme(cx);
        let buttons = self
            .toolbar_buttons(cx)
            .map(|(id, icon, tooltip, enabled, act)| {
                div().flex_none().debug_selector(move || id.into()).child(
                    IconButton::new(id, icon, &ui)
                        .disabled(!enabled)
                        .tooltip(move |_, cx| {
                            cx.new(|_| Tooltip::new(tooltip, None::<&str>, &ui)).into()
                        })
                        .on_click(cx.listener(move |panel, _, window, cx| act(panel, window, cx))),
                )
            });
        div()
            .flex_none()
            .flex()
            .justify_end()
            .items_center()
            .gap_0p5()
            .px_2()
            .pt_1()
            .children(buttons)
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
        let ui = fe_ui::ui_theme(cx);
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
        let details_toggle = div()
            .debug_selector(|| "search-details-toggle".into())
            .child(
                ToggleButton::new("search-details-toggle", "...", &ui)
                    .on(self.details_shown)
                    .on_click(cx.listener(|panel, _, window, cx| panel.toggle_details(window, cx))),
            );
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
        let toolbar = self.render_toolbar(cx);
        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_run_command))
            // The workbench's replace commands, which the panel does itself
            // while focused, where the workbench's context holds: showing
            // replace seeds nothing from the editor.
            .on_action(cx.listener(|panel, _: &ReplaceInFiles, window, cx| {
                if invoked_in(crate::KEY_CONTEXT, cx) {
                    panel.show_replace(false, window, cx)
                }
            }))
            .on_action(cx.listener(|panel, _: &ReplaceAllInFiles, window, cx| {
                if invoked_in(crate::KEY_CONTEXT, cx) {
                    panel.confirm_replace_all(window, cx)
                }
            }))
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
            .child(toolbar)
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
                            .children(toggles)
                            .child(details_toggle),
                    ),
            )
            .children(replace)
            .when(self.details_shown, |panel| {
                panel.child(
                    div()
                        .flex_none()
                        .flex()
                        .flex_col()
                        .child(self.include.clone())
                        .child(self.exclude.clone()),
                )
            })
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

/// What a search panel command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SearchOp {
    ToggleRegex,
    ToggleCase,
    ToggleWholeWord,
    /// Enter in the search panel's input: [`TextInput::submit`].
    Submit,
    ToggleReplace,
    ToggleDetails,
    FocusResults,
    SelectNext,
    SelectPrevious,
    OpenSelected,
    /// Left in the results.
    CollapseOrParent,
    /// Right in the results.
    ExpandOrFirstChild,
    Refresh,
    ClearResults,
    CollapseAll,
    ExpandAll,
    ViewAsTree,
    ViewAsList,
}

/// The [`CommandTarget`] of search panel commands: the op to do.
pub(crate) struct SearchCommands;

impl CommandTarget for SearchCommands {
    type Tx = ();
    type Target<'a> = Option<SearchOp>;
}

struct Commands(Rc<Registry<SearchCommands>>);

impl Global for Commands {}

/// Registers the search panel's commands.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<SearchCommands> {
    use SearchOp::*;
    let search = KEY_CONTEXT;
    let results = RESULTS_KEY_CONTEXT;
    let commands = [
        ("search.toggle_regex", "Toggle Regex", search, ToggleRegex),
        (
            "search.toggle_case",
            "Toggle Match Case",
            search,
            ToggleCase,
        ),
        (
            "search.toggle_whole_word",
            "Toggle Whole Word",
            search,
            ToggleWholeWord,
        ),
        (
            "search.toggle_replace",
            "Toggle Replace",
            search,
            ToggleReplace,
        ),
        (
            "search.toggle_details",
            "Toggle Search Details",
            search,
            ToggleDetails,
        ),
        (
            "search.focus_results",
            "Focus Search Results",
            search,
            FocusResults,
        ),
        (
            "search.select_next",
            "Select Next Result",
            results,
            SelectNext,
        ),
        (
            "search.select_previous",
            "Select Previous Result",
            results,
            SelectPrevious,
        ),
        (
            "search.open_selected",
            "Open Selected Result",
            results,
            OpenSelected,
        ),
        (
            "search.collapse_or_parent",
            "Collapse Result or Select Its File",
            results,
            CollapseOrParent,
        ),
        (
            "search.expand_or_first_child",
            "Expand Result or Select Its First Match",
            results,
            ExpandOrFirstChild,
        ),
        ("search.refresh", "Refresh", search, Refresh),
        (
            "search.clear_results",
            "Clear Search Results",
            search,
            ClearResults,
        ),
        ("search.collapse_all", "Collapse All", search, CollapseAll),
        ("search.expand_all", "Expand All", search, ExpandAll),
        ("search.view_as_tree", "View as Tree", search, ViewAsTree),
        ("search.view_as_list", "View as List", search, ViewAsList),
        (
            "text_input.submit",
            "Submit",
            TextInput::KEY_CONTEXT,
            Submit,
        ),
    ];
    let mut registry = Registry::new();
    for (id, title, context, op) in commands {
        registry
            .register(
                id,
                title,
                move |target: &mut Option<SearchOp>, _: &Actor| {
                    *target = Some(op);
                    Ok(Dispatched::new(Effect::Notify))
                },
            )
            .expect("search command IDs are valid and distinct")
            .context(ContextPredicate::identifier(context));
    }
    registry
}

/// The op of `run`, if it's a search panel command that applies in its
/// context.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<SearchOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShownRow {
    /// A folder's label and the matches under it.
    Folder(String, usize),
    /// A file's label (its relative path, or its name in the tree) and
    /// match count.
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
        self.rows
            .iter()
            .map(|row| match *row {
                ResultRow::Folder(folder) => {
                    let folder = &self.folders[folder];
                    ShownRow::Folder(folder.label.clone(), folder.matches)
                }
                ResultRow::File(file) => {
                    let matches = (self.files[file].lines.iter())
                        .map(|line| line.ranges.len())
                        .sum();
                    ShownRow::File(self.file_label(file).to_string(), matches)
                }
                ResultRow::Line(file, line) => {
                    let line = &self.files[file].lines[line];
                    let preview = line.preview.clone();
                    ShownRow::Line(line.line_number, preview, line_highlights(line))
                }
            })
            .collect()
    }

    /// Each row's depth, in [`SearchPanel::shown_rows`]. Only the macOS-only tree tests read it.
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn depths(&self) -> Vec<usize> {
        self.depths.clone()
    }

    pub(crate) fn details_shown(&self) -> bool {
        self.details_shown
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

#[cfg(all(test, target_os = "macos"))]
#[path = "search_results_toolbar_tests.rs"]
mod results_toolbar_tests;

#[cfg(all(test, target_os = "macos"))]
#[path = "search_results_tree_tests.rs"]
mod results_tree_tests;

#[cfg(test)]
#[path = "search_panel_details_tests.rs"]
mod details_tests;
