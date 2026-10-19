//! The project panel on the right dock: the lazy file tree, listed off the
//! main thread and kept current by a file watcher.
//!
//! One row is selected, kept by node so it survives rows above it changing.
//! Clicks and the keys of [`init`]'s registry (in the panel's
//! [`KEY_CONTEXT`]) move it; [`ProjectPanel::reveal`] selects a file's row,
//! expanding its collapsed ancestors.
//!
//! Rows show git's status as the Changes panel lists it
//! ([`ProjectPanel::set_git_statuses`]): a changed file's name in its
//! status letter's colour with the letter at the row's end, and a folder
//! holding changes, expanded or collapsed, in the colour of the
//! highest-precedence status beneath it with a dot.
//!
//! The file operations on the selected row (new file and folder, rename,
//! move to the Trash, duplicate, copy path) are in [`ops`].

use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    io,
    ops::Range,
    path::{Component, Path, PathBuf},
    rc::Rc,
};

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_git::ChangeKind;
use fe_gpui::commands::RunCommand;
use fe_project::{EntryKind, FileTree, FsChange, Listed, NodeId, ProjectWatcher, TreeOptions};
use fe_ui::{Icon, ROW_HEIGHT, TreeRow, UiTheme, file_icon, folder_icon};
use gpui::{
    App, AppContext as _, Context, EventEmitter, FocusHandle, Focusable, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollStrategy, Styled as _,
    Task, UniformListScrollHandle, Window, div, px, uniform_list,
};

use crate::git_panel;
use crate::workbench::toast::{self, Toast};
use ops::EditKind;

/// The project tree in the sidebar.
pub(crate) const SIDEBAR: crate::dock::SidebarPanel = crate::dock::SidebarPanel {
    panel: crate::DockPanel::new("project"),
    name: "Project",
    icon: fe_ui::Icon::Files,
    show_command: "dock.show_project",
    show_op: crate::dock::DockOp::ShowProject,
    order: 10,
    always_on_rail: true,
    focus_on_show: false,
};

/// The key context the panel's element sets.
pub(crate) const KEY_CONTEXT: &str = "project_panel";

/// [`KEY_CONTEXT`] with vim on, for its netrw and nvim-tree keys, unless
/// the name field is open.
const VIM_KEY_CONTEXT: &str = "project_panel vim";

/// The sidebar's width as built. Its panels fill its column, however
/// wide it's dragged.
pub const PANEL_WIDTH: gpui::Pixels = px(240.);

/// What the panel asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelEvent {
    /// A file row was double-clicked, or Enter pressed on it: it opens
    /// to stay.
    OpenFile(PathBuf),
    /// A file row was clicked once: it opens for a look, in a preview tab
    /// that the next one replaces, if `workbench.preview_tabs` is on.
    PreviewFile(PathBuf),
    /// A watcher batch came, for others that follow the disk too.
    FsChanged(Vec<FsChange>),
    /// The root opened: its tree shows.
    Opened,
    /// The panel renamed `from`, a file or a folder, to `to`: the editors
    /// of it, or of a file in it, are to take the paths `moves` planned for
    /// them before the rename ([`crate::Workbench::plan_rename`]).
    Renamed {
        from: PathBuf,
        to: PathBuf,
        moves: Vec<(gpui::Entity<fe_gpui::EditorView>, PathBuf)>,
    },
    /// The panel made these, each with whether it's a folder: a new file or
    /// folder, or a duplicate. Language servers are told
    /// (`workspace/didCreateFiles`). A failed operation sends none.
    Created(Vec<(PathBuf, bool)>),
    /// The panel moved these, each with whether it's a folder, to the
    /// Trash. Language servers are told (`workspace/didDeleteFiles`).
    Deleted(Vec<(PathBuf, bool)>),
}

enum State {
    NoFolder,
    /// The root is being opened and listed in the background.
    Opening,
    Open(FileTree),
    /// The root couldn't be opened.
    Failed(String),
}

pub struct ProjectPanel {
    /// The root as the workbench was given it. Paths the panel hands out are
    /// under it, so they match the launch file's path.
    root: Option<PathBuf>,
    state: State,
    /// Directories the panel shows expanded, to list again when the tree
    /// says a listing may have missed something.
    expanded: HashSet<NodeId>,
    /// Clicked directories waiting for their listing, to expand once it
    /// lands.
    expanding: HashSet<NodeId>,
    /// Directories with a listing running, and the epoch it started in.
    listing: HashMap<NodeId, u64>,
    /// Directories whose last listing failed, and the epoch it failed in.
    /// They aren't listed again until the epoch moves on, so a lasting error
    /// doesn't loop.
    failed: HashMap<NodeId, u64>,
    /// Moves on with each successful listing (it may have made others
    /// reachable or owed), watcher batch and directory click.
    epoch: u64,
    /// Moves on when the tree is opened again, so a listing that started
    /// on the old one is dropped when it lands.
    generation: u64,
    selected: Option<NodeId>,
    /// Where the selected row was last seen, checked before searching the
    /// rows for it.
    selected_hint: usize,
    /// The file the last reveal asked for, until its row is selected or
    /// it's found not to have one.
    reveal: Option<PathBuf>,
    /// The ancestor the reveal waits to have listed.
    revealing: Option<NodeId>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    theme: UiTheme,
    /// Changed files' statuses, by path from the root.
    git_files: HashMap<PathBuf, ChangeKind>,
    /// Each folder holding changed files, by path from the root (the root
    /// itself not included), and the status it shows: the one of highest
    /// precedence beneath it.
    git_dirs: HashMap<PathBuf, ChangeKind>,
    /// The name field, while a new entry or a rename is being named.
    edit: Option<ops::NameEdit>,
    /// Counts the name fields opened, so an operation's outcome finds the
    /// field it came from.
    edits: u64,
    /// Where a new entry's folder was last seen in the rows, checked
    /// before searching them for it.
    field_hint: Cell<usize>,
    /// The row ranges the last frame built: the list builds the visible
    /// rows and measures its first row twice.
    #[cfg(test)]
    built: Vec<Range<usize>>,
    /// The row the last frame drew selected.
    #[cfg(test)]
    drawn_selected: Option<usize>,
    /// Each row the last frame built, by name, and the status it drew.
    #[cfg(test)]
    drawn_status: Vec<(String, DrawnStatus)>,
    /// Whether the tree lists ignored entries.
    show_ignored: bool,
    _watch: Option<Task<()>>,
}

impl EventEmitter<PanelEvent> for ProjectPanel {}

impl ProjectPanel {
    /// A panel for `root`, or saying no folder is open. The root is opened,
    /// listed and watched in the background; nothing touches the file system
    /// on this thread.
    pub fn new(
        root: Option<PathBuf>,
        theme: UiTheme,
        show_ignored: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self {
            state: if root.is_some() {
                State::Opening
            } else {
                State::NoFolder
            },
            root,
            expanded: HashSet::new(),
            expanding: HashSet::new(),
            listing: HashMap::new(),
            failed: HashMap::new(),
            epoch: 0,
            generation: 0,
            selected: None,
            selected_hint: 0,
            reveal: None,
            revealing: None,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            theme,
            git_files: HashMap::new(),
            git_dirs: HashMap::new(),
            edit: None,
            edits: 0,
            field_hint: Cell::new(0),
            #[cfg(test)]
            built: Vec::new(),
            #[cfg(test)]
            drawn_selected: None,
            #[cfg(test)]
            drawn_status: Vec::new(),
            show_ignored,
            _watch: None,
        };
        panel.start(show_ignored, cx);
        panel
    }

    /// Opens, lists and watches the root in the background, listing ignored
    /// entries too if `show_ignored`; the tree it replaces, if any, goes.
    fn start(&mut self, show_ignored: bool, cx: &mut Context<Self>) {
        let options = TreeOptions { show_ignored };
        let watch = self.root.clone().map(|root| {
            let (sender, batches) = async_channel::unbounded::<Vec<FsChange>>();
            let opened = cx.background_spawn(async move {
                // Watching first, so changes made while the root lists still
                // arrive. Not in tests, which hand batches to
                // `apply_fs_changes`: GPUI's test scheduler rejects wakeups
                // from the watcher's thread.
                let watcher = if cfg!(test) {
                    drop(sender);
                    None
                } else {
                    ProjectWatcher::new(&root, move |batch| {
                        sender.send_blocking(batch).ok();
                    })
                    .inspect_err(|error| {
                        log::warn!("{} won't update on changes: {error}", root.display())
                    })
                    .ok()
                };
                (FileTree::open(&root, options), watcher)
            });
            cx.spawn(async move |this, cx| {
                let (tree, _watcher) = opened.await;
                if this.update(cx, |panel, cx| panel.opened(tree, cx)).is_err() {
                    return;
                }
                while let Ok(mut changes) = batches.recv().await {
                    while let Ok(more) = batches.try_recv() {
                        changes.extend(more);
                    }
                    let applied = this.update(cx, |panel, cx| panel.apply_fs_changes(&changes, cx));
                    if applied.is_err() {
                        break;
                    }
                }
            })
        });
        self.state = if self.root.is_some() {
            State::Opening
        } else {
            State::NoFolder
        };
        self._watch = watch;
    }

    /// Whether the tree lists ignored entries.
    pub(crate) fn lists_ignored(&self) -> bool {
        self.show_ignored
    }

    /// Lists ignored entries too, or stops, by opening the tree again.
    pub(crate) fn set_show_ignored(&mut self, show_ignored: bool, cx: &mut Context<Self>) {
        if self.show_ignored == show_ignored {
            return;
        }
        self.show_ignored = show_ignored;
        // The new tree numbers its nodes afresh, so what the old ids named
        // goes; a pending reveal is by path and carries over.
        self.generation += 1;
        self.expanded.clear();
        self.expanding.clear();
        self.listing.clear();
        self.failed.clear();
        self.selected = None;
        self.selected_hint = 0;
        self.revealing = None;
        self.edit = None;
        self.start(show_ignored, cx);
        cx.notify();
    }

    pub fn tree(&self) -> Option<&FileTree> {
        match &self.state {
            State::Open(tree) => Some(tree),
            _ => None,
        }
    }

    fn opened(&mut self, tree: io::Result<FileTree>, cx: &mut Context<Self>) {
        self.state = match tree {
            Ok(tree) => {
                cx.emit(PanelEvent::Opened);
                State::Open(tree)
            }
            Err(error) => {
                let root = self.root.as_deref().unwrap_or(Path::new(""));
                log::error!("cannot open {}: {error}", root.display());
                toast::show(
                    cx,
                    Toast::error(format!("Can't open folder {}", root.display()))
                        .detail(error.to_string()),
                );
                State::Failed(format!("Cannot open {}: {error}", root.display()))
            }
        };
        self.advance_reveal(cx);
        cx.notify();
    }

    /// Shows `statuses`, each a changed file's path from the root and its
    /// status, in place of those before: on the file's row, and on each
    /// folder above it, which shows the status of highest precedence
    /// beneath it (conflicted, modified, deleted, added, then untracked).
    /// A deleted file has no row, but its folders show it.
    pub fn set_git_statuses(
        &mut self,
        statuses: impl IntoIterator<Item = (PathBuf, ChangeKind)>,
        cx: &mut Context<Self>,
    ) {
        let mut files = HashMap::new();
        let mut dirs: HashMap<PathBuf, ChangeKind> = HashMap::new();
        for (path, kind) in statuses {
            for dir in path.ancestors().skip(1) {
                if dir.as_os_str().is_empty() {
                    break;
                }
                let shown = dirs.entry(dir.to_owned()).or_insert(kind);
                if precedence(*shown) > precedence(kind) {
                    // Its folders show this or higher already.
                    break;
                }
                *shown = kind;
            }
            let shown = files.entry(path).or_insert(kind);
            *shown = std::cmp::max_by_key(*shown, kind, |kind| precedence(*kind));
        }
        if files != self.git_files || dirs != self.git_dirs {
            self.git_files = files;
            self.git_dirs = dirs;
            cx.notify();
        }
    }

    /// Brings the tree up to date with a batch from the file watcher, then
    /// lists again any shown directory it says may have missed something.
    pub fn apply_fs_changes(&mut self, changes: &[FsChange], cx: &mut Context<Self>) {
        cx.emit(PanelEvent::FsChanged(changes.to_vec()));
        let State::Open(tree) = &mut self.state else {
            return;
        };
        let version = tree.version();
        let selected = Self::index_of(tree, self.selected, self.selected_hint);
        tree.apply(changes);
        let changed = tree.version() != version;
        self.removed(selected, cx);
        // Something changed, so failed listings may succeed now.
        self.epoch += 1;
        if changed {
            cx.notify();
        }
        self.reconcile(cx);
        self.advance_reveal(cx);
    }

    /// Forgets nodes the tree removed (a watcher batch or a listing can).
    /// If the selected one went, selects the row now at its index,
    /// `selected`, clamped to the last row.
    fn removed(&mut self, selected: Option<usize>, cx: &mut Context<Self>) {
        let State::Open(tree) = &self.state else {
            return;
        };
        let live = |id: &NodeId| tree.row(*id).is_some() || *id == tree.root();
        self.expanded.retain(live);
        self.expanding.retain(live);
        if self.edit.as_ref().is_some_and(|edit| !live(&edit.node)) {
            // What it named went: so does the field.
            self.edit = None;
            cx.notify();
        }
        if !self.selected.is_some_and(|id| !live(&id)) {
            return;
        }
        let last = tree.row_count().checked_sub(1);
        self.selected = None;
        if let (Some(ix), Some(last)) = (selected, last) {
            self.select_row(ix.min(last), cx);
        }
    }

    /// Row `id`'s index, if it's shown: `hint` if it's there.
    fn index_of(tree: &FileTree, id: Option<NodeId>, hint: usize) -> Option<usize> {
        let id = id?;
        if tree
            .rows(hint..hint + 1)
            .next()
            .is_some_and(|row| row.id == id)
        {
            return Some(hint);
        }
        tree.rows(0..tree.row_count()).position(|row| row.id == id)
    }

    /// The selected row's index, if it's shown.
    fn selected_index(&self) -> Option<usize> {
        Self::index_of(self.tree()?, self.selected, self.selected_hint)
    }

    /// Selects row `ix` and scrolls it into view.
    fn select_row(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(row) = self.tree().and_then(|tree| tree.rows(ix..ix + 1).next()) else {
            return;
        };
        self.selected = Some(row.id);
        self.selected_hint = ix;
        self.scroll
            .scroll_to_item(self.list_ix(ix), ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Selects the first row, or the last, for vim's `gg` and `G`.
    fn select_end(&mut self, last: bool, cx: &mut Context<Self>) {
        let Some(count) = self
            .tree()
            .map(FileTree::row_count)
            .filter(|&count| count > 0)
        else {
            return;
        };
        self.select_row(if last { count - 1 } else { 0 }, cx);
    }

    /// Up and Down: moves the selection by `delta` rows, clamped, or
    /// selects the first row if none is.
    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(last) = self.tree().and_then(|tree| tree.row_count().checked_sub(1)) else {
            return;
        };
        let ix = match self.selected_index() {
            Some(ix) => ix.saturating_add_signed(delta).min(last),
            None => 0,
        };
        self.select_row(ix, cx);
    }

    /// Left: collapses the selected directory if it's expanded, and
    /// otherwise selects its parent, if that's a row, no longer expanding
    /// the directory once listed.
    fn collapse_or_parent(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.selected_index() else {
            return;
        };
        let State::Open(tree) = &mut self.state else {
            return;
        };
        let row = tree
            .rows(ix..ix + 1)
            .next()
            .expect("the selection is shown");
        let (id, depth) = (row.id, row.depth);
        if row.expanded {
            tree.collapse(id);
            self.expanded.remove(&id);
            cx.notify();
        } else if depth > 0 {
            self.expanding.remove(&id);
            // The parent is the nearest row above that is less deep.
            let parent = (0..ix)
                .rev()
                .find(|&above| tree.rows(above..above + 1).any(|row| row.depth < depth));
            if let Some(parent) = parent {
                self.select_row(parent, cx);
            }
        }
    }

    /// Right: expands the selected directory if it's collapsed, as a click
    /// does, and otherwise selects its first child, if it has one.
    fn expand_or_first_child(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.selected_index() else {
            return;
        };
        let Some(tree) = self.tree() else {
            return;
        };
        let row = tree
            .rows(ix..ix + 1)
            .next()
            .expect("the selection is shown");
        if row.kind != EntryKind::Dir {
            return;
        }
        let id = row.id;
        if row.expanded {
            let child = tree.rows(ix + 1..ix + 2).next();
            if child.is_some_and(|child| child.depth > row.depth) {
                self.select_row(ix + 1, cx);
            }
        } else if !self.expanding.contains(&id) {
            self.click(id, cx);
        }
    }

    /// Enter: opens the selected file, or toggles the selected directory,
    /// as a click does.
    fn open_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_index().and(self.selected) {
            self.click(id, cx);
        }
    }

    /// Selects `path`'s row, expanding each collapsed ancestor from the root
    /// down, without moving focus. Unlisted ancestors are listed off the
    /// main thread, one at a time; none is expanded until the file's row is
    /// found, so a file outside the root, or one the tree hides (it's
    /// ignored, say), expands nothing. A file with no row yet (not created,
    /// or its watcher event not in) reveals nothing either. A later reveal
    /// replaces this one, and moving the selection drops it.
    pub fn reveal(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.reveal = Some(path);
        self.revealing = None;
        self.advance_reveal(cx);
    }

    /// Takes the pending reveal as far as what's listed allows: down to the
    /// first ancestor that wants a listing, which it starts, or to the row,
    /// which it shows and selects.
    fn advance_reveal(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.reveal.clone() else {
            return;
        };
        let State::Open(tree) = &mut self.state else {
            return;
        };
        let rel = self
            .root
            .as_deref()
            .and_then(|root| path.strip_prefix(root).ok())
            .or_else(|| path.strip_prefix(tree.root_path()).ok());
        let Some(rel) = rel.filter(|rel| !rel.as_os_str().is_empty()) else {
            return self.cancel_reveal();
        };
        let mut chain = Vec::new();
        let mut at = tree.root();
        for component in rel.components() {
            let Component::Normal(name) = component else {
                return self.cancel_reveal();
            };
            if tree.listing(at).is_some() {
                // Unlisted, or owing a listing: wait for it, unless the last
                // failed and nothing changed since.
                if self.failed.get(&at) == Some(&self.epoch) {
                    return self.cancel_reveal();
                }
                self.revealing = Some(at);
                return self.reconcile(cx);
            }
            let child = tree.children(at).and_then(|children| {
                children
                    .iter()
                    .copied()
                    .find(|&child| tree.row(child).is_some_and(|row| row.name == name))
            });
            let Some(child) = child else {
                return self.cancel_reveal();
            };
            chain.push(child);
            at = child;
        }
        let file = chain.pop().expect("the path has a component");
        for dir in chain {
            // Listed, so this does no IO.
            tree.expand(dir);
            self.expanded.insert(dir);
            self.expanding.remove(&dir);
        }
        self.cancel_reveal();
        self.selected = Some(file);
        if let Some(ix) = self.selected_index() {
            self.select_row(ix, cx);
        }
        cx.notify();
    }

    /// Drops the pending reveal, if any.
    pub(crate) fn cancel_reveal(&mut self) {
        self.reveal = None;
        self.revealing = None;
    }

    /// What clicking a row does: it's selected, and a directory expands
    /// once listed (or collapses, or stops waiting to expand); a file opens.
    fn click(&mut self, id: NodeId, cx: &mut Context<Self>) {
        self.click_row(id, false, cx);
    }

    /// A mouse click on a row: [`ProjectPanel::click`], but a file's first
    /// click opens it for a look, in a preview tab, and the second of a
    /// double-click keeps it.
    fn clicked(&mut self, id: NodeId, event: &gpui::ClickEvent, cx: &mut Context<Self>) {
        self.click_row(id, event.click_count() == 1, cx);
    }

    fn click_row(&mut self, id: NodeId, preview: bool, cx: &mut Context<Self>) {
        self.cancel_reveal();
        let State::Open(tree) = &mut self.state else {
            return;
        };
        let Some(row) = tree.row(id) else {
            return;
        };
        self.selected = Some(id);
        if row.kind == EntryKind::Dir {
            if row.expanded {
                tree.collapse(id);
                self.expanded.remove(&id);
            } else if !self.expanding.remove(&id) {
                self.expanding.insert(id);
                // Retries failed listings, the root's included.
                self.epoch += 1;
                self.reconcile(cx);
            }
            cx.notify();
            return;
        }
        if let Some(path) = self.shown_path(id) {
            cx.emit(if preview {
                PanelEvent::PreviewFile(path)
            } else {
                PanelEvent::OpenFile(path)
            });
        }
        cx.notify();
    }

    /// Node `id`'s path as the panel hands paths out: under the root as the
    /// workbench gave it. `None` for the root itself.
    fn shown_path(&self, id: NodeId) -> Option<PathBuf> {
        let tree = self.tree()?;
        let path = tree.path(id)?;
        let rel = path.strip_prefix(tree.root_path()).ok()?;
        if rel.as_os_str().is_empty() {
            return None;
        }
        Some(self.root.as_ref()?.join(rel))
    }

    /// Lists in the background each directory shown, waiting to expand or
    /// waited on by a reveal that the tree wants listed (a result can make
    /// others than its own directory owe a listing), unless one is running
    /// or its last failed.
    /// Expands those waiting that need none.
    fn reconcile(&mut self, cx: &mut Context<Self>) {
        let State::Open(tree) = &mut self.state else {
            return;
        };
        let dirs: Vec<NodeId> = [tree.root()]
            .into_iter()
            .chain(self.expanded.iter().copied())
            .chain(self.expanding.iter().copied())
            .chain(self.revealing)
            .collect();
        for dir in dirs {
            let Some(listing) = tree.listing(dir) else {
                if self.expanding.remove(&dir) {
                    // Listed already, so this does no IO.
                    tree.expand(dir);
                    self.expanded.insert(dir);
                    cx.notify();
                }
                continue;
            };
            if self.failed.get(&dir) == Some(&self.epoch) || self.listing.contains_key(&dir) {
                continue;
            }
            self.listing.insert(dir, self.epoch);
            let listed = cx.background_spawn(async move { listing.run() });
            let generation = self.generation;
            cx.spawn(async move |this, cx| {
                let listed = listed.await;
                this.update(cx, |panel, cx| {
                    if panel.generation == generation {
                        panel.listed(dir, listed, cx);
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    /// Stores a listing's result, then reconciles.
    pub(crate) fn listed(&mut self, dir: NodeId, listed: Listed, cx: &mut Context<Self>) {
        let started = self.listing.remove(&dir);
        let State::Open(tree) = &mut self.state else {
            return;
        };
        let failed = listed.result().is_err();
        let selected = Self::index_of(tree, self.selected, self.selected_hint);
        tree.fill(dir, listed);
        self.removed(selected, cx);
        if !failed {
            self.epoch += 1;
        } else if started == Some(self.epoch) {
            // Nothing changed while it ran, so the failure is current: the
            // row shows it until a change or a click retries.
            self.failed.insert(dir, self.epoch);
            self.expanding.remove(&dir);
        }
        // Otherwise it may be stale: reconciling retries it.
        cx.notify();
        self.reconcile(cx);
        self.advance_reveal(cx);
    }

    /// Runs a panel command; others go on to the workbench. While the name
    /// field has focus, the field takes the keys ([`Self::run_in_field`]).
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        if self.field_focused(window, cx) {
            return self.run_in_field(run, window, cx);
        }
        let op = op(run, cx);
        if op.is_some() {
            // The user moved on.
            self.cancel_reveal();
        }
        match op {
            Some(PanelOp::SelectPrev) => self.move_selection(-1, cx),
            Some(PanelOp::SelectNext) => self.move_selection(1, cx),
            Some(PanelOp::CollapseOrParent) => self.collapse_or_parent(cx),
            Some(PanelOp::ExpandOrFirstChild) => self.expand_or_first_child(cx),
            Some(PanelOp::Open) => self.open_selected(cx),
            Some(PanelOp::SelectFirst) => self.select_end(false, cx),
            Some(PanelOp::SelectLast) => self.select_end(true, cx),
            Some(PanelOp::NewFile) => self.start_edit(EditKind::NewFile, window, cx),
            Some(PanelOp::NewFolder) => self.start_edit(EditKind::NewFolder, window, cx),
            Some(PanelOp::Rename) => self.start_edit(EditKind::Rename, window, cx),
            Some(PanelOp::Delete) => self.delete_selected(window, cx),
            Some(PanelOp::Duplicate) => self.duplicate_selected(window, cx),
            Some(PanelOp::CopyPath) => self.copy_path(false, cx),
            Some(PanelOp::CopyRelativePath) => self.copy_path(true, cx),
            None => cx.propagate(),
        }
    }

    /// The list's rows in `range`: the tree's, with the name field in
    /// place of the row being renamed or as a row of its own for a new
    /// entry ([`Self::field_row`]).
    fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let State::Open(tree) = &self.state else {
            return Vec::new();
        };
        let focused = self.focus.contains_focused(window, cx);
        #[cfg(test)]
        self.built.push(range.clone());
        let git = !self.git_files.is_empty() || !self.git_dirs.is_empty();
        let field = self.field_row();
        let renaming = (self.edit.as_ref())
            .filter(|edit| edit.kind == EditKind::Rename)
            .map(|edit| edit.node);
        // The tree's rows the list's range shows: those after a new entry's
        // field are one further down the list.
        let tree_ix = |ix: usize| match field {
            Some((at, _)) if ix > at => ix - 1,
            _ => ix,
        };
        let shown = tree_ix(range.start)..tree_ix(range.end);
        #[cfg(test)]
        let mut drawn = Vec::new();
        let mut seen = None;
        let mut rows: Vec<gpui::AnyElement> = tree
            .rows(shown.clone())
            .zip(shown)
            .map(|(row, ix)| {
                let id = row.id;
                let selected = self.selected == Some(id);
                if selected {
                    seen = Some(ix);
                }
                let name = row.name.to_string_lossy();
                let icon = match row.kind {
                    EntryKind::Dir => folder_icon(row.expanded),
                    EntryKind::Symlink => Icon::FileSymlink,
                    EntryKind::File => file_icon(&name),
                };
                if renaming == Some(id) {
                    return self.render_field(row.depth, icon, cx);
                }
                let mut element = TreeRow::new(("row", ix), name.into_owned(), &self.theme)
                    .depth(row.depth)
                    .icon(icon)
                    .focused(focused)
                    .ignored(row.ignored)
                    .error(row.error.is_some())
                    .selected(selected)
                    .on_click(cx.listener(move |panel, event, _, cx| panel.clicked(id, event, cx)));
                if row.kind == EntryKind::Dir {
                    element = element.directory(row.expanded);
                }
                // An ignored row is left as it is.
                let status = (git && !row.ignored)
                    .then(|| self.git_status(tree, id, row.kind))
                    .flatten();
                if let Some((kind, badge)) = status {
                    let color = git_panel::letter_color(kind, &self.theme);
                    element = element.label_color(color).badge(badge, color);
                }
                #[cfg(test)]
                drawn.push((row.name.to_string_lossy().into_owned(), {
                    let (label, badge) = element.status();
                    (label, badge.map(|(text, color)| (text.to_string(), color)))
                }));
                element.into_any_element()
            })
            .collect();
        if let Some((at, depth)) = field.filter(|(at, _)| range.contains(at)) {
            let icon = match self.edit.as_ref().map(|edit| edit.kind) {
                Some(EditKind::NewFolder) => folder_icon(false),
                _ => Icon::File,
            };
            rows.insert(at - range.start, self.render_field(depth, icon, cx));
        }
        #[cfg(test)]
        self.drawn_status.extend(drawn);
        if let Some(ix) = seen {
            self.selected_hint = ix;
            #[cfg(test)]
            {
                self.drawn_selected = Some(ix);
            }
        }
        rows
    }

    /// Node `id`'s git status, if it's a changed file or a folder holding
    /// changes (`kind` says which it is), and its badge: the file's status
    /// letter, or a dot.
    fn git_status(
        &self,
        tree: &FileTree,
        id: NodeId,
        kind: EntryKind,
    ) -> Option<(ChangeKind, &'static str)> {
        let path = tree.path(id)?;
        let relative = path.strip_prefix(tree.root_path()).ok()?;
        match kind {
            EntryKind::Dir => Some((*self.git_dirs.get(relative)?, "•")),
            EntryKind::File | EntryKind::Symlink => {
                let kind = *self.git_files.get(relative)?;
                Some((kind, git_panel::letter(kind)))
            }
        }
    }

    fn message(&self, text: String) -> gpui::AnyElement {
        div()
            .px_3()
            .py_2()
            .text_color(self.theme.muted_text)
            .child(text)
            .into_any_element()
    }
}

impl Render for ProjectPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.built.clear();
            self.drawn_selected = None;
            self.drawn_status.clear();
        }
        let title = match &self.state {
            State::Open(tree) => tree.root_path().file_name(),
            _ => self.root.as_deref().and_then(Path::file_name),
        }
        .map_or("PROJECT".into(), |name| {
            name.to_string_lossy().to_uppercase()
        });
        let body = match &self.state {
            State::NoFolder => self.message("No folder open".into()),
            State::Opening => div().into_any_element(),
            State::Failed(message) => self.message(message.clone()),
            State::Open(tree) => uniform_list(
                "project-rows",
                tree.row_count() + usize::from(self.field_row().is_some()),
                cx.processor(|panel, range, window, cx| panel.render_rows(range, window, cx)),
            )
            .track_scroll(&self.scroll)
            .flex_1()
            .into_any_element(),
        };
        // Vim's tree keys are off while the name field takes typing.
        let context = if fe_gpui::vim_enabled(cx) && self.edit.is_none() {
            VIM_KEY_CONTEXT
        } else {
            KEY_CONTEXT
        };
        div()
            .track_focus(&self.focus)
            .key_context(context)
            .on_action(cx.listener(Self::on_run_command))
            .debug_selector(|| "sidebar-panel".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(self.theme.border)
            .bg(self.theme.panel_background)
            .text_color(self.theme.text)
            .text_size(px(12.))
            .child(div().flex_none().px_3().py_2().child(title))
            .child(body)
    }
}

impl Focusable for ProjectPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Whether new trees list ignored entries: unless `tree.show_ignored` is
/// off in the app's settings. Tests with no settings global list them only
/// if they ask.
pub(crate) fn show_ignored(cx: &App) -> bool {
    let settings = cx.try_global::<kiln::GlobalSettings<fe_config::Settings>>();
    #[cfg(test)]
    let default = tests::SHOW_IGNORED.get();
    #[cfg(not(test))]
    let default = true;
    settings.map_or(default, |global| global.settings().tree.show_ignored)
}

/// Which status a folder holding several shows: the highest.
fn precedence(kind: ChangeKind) -> u8 {
    match kind {
        ChangeKind::Untracked => 0,
        ChangeKind::Added => 1,
        ChangeKind::Deleted => 2,
        ChangeKind::Modified => 3,
        ChangeKind::Conflicted => 4,
    }
}

/// What a panel command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanelOp {
    SelectPrev,
    SelectNext,
    CollapseOrParent,
    ExpandOrFirstChild,
    Open,
    SelectFirst,
    SelectLast,
    NewFile,
    NewFolder,
    Rename,
    Delete,
    Duplicate,
    CopyPath,
    CopyRelativePath,
}

/// The [`CommandTarget`] of panel commands: the op to do.
pub(crate) struct PanelCommands;

impl CommandTarget for PanelCommands {
    type Tx = ();
    type Target<'a> = Option<PanelOp>;
}

struct Commands(Rc<Registry<PanelCommands>>);

impl Global for Commands {}

/// Registers the panel's commands, and the keys of its name field
/// ([`fe_ui::TextField`]) in that field's own key context.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
    // The name field's caret, selection and clipboard keys.
    cx.bind_keys(fe_ui::TextField::key_bindings(cfg!(target_os = "macos")));
}

pub(crate) fn registry() -> Registry<PanelCommands> {
    use PanelOp::*;
    let commands = [
        (
            "project_panel.select_prev",
            "Select Previous",
            KEY_CONTEXT,
            SelectPrev,
        ),
        (
            "project_panel.select_next",
            "Select Next",
            KEY_CONTEXT,
            SelectNext,
        ),
        (
            "project_panel.collapse_or_parent",
            "Collapse or Select Parent",
            KEY_CONTEXT,
            CollapseOrParent,
        ),
        (
            "project_panel.expand_or_first_child",
            "Expand or Select First Child",
            KEY_CONTEXT,
            ExpandOrFirstChild,
        ),
        ("project_panel.open", "Open", KEY_CONTEXT, Open),
        (
            "project_panel.select_first",
            "Select First",
            KEY_CONTEXT,
            SelectFirst,
        ),
        (
            "project_panel.select_last",
            "Select Last",
            KEY_CONTEXT,
            SelectLast,
        ),
        ("project_panel.new_file", "New File…", KEY_CONTEXT, NewFile),
        (
            "project_panel.new_folder",
            "New Folder…",
            KEY_CONTEXT,
            NewFolder,
        ),
        ("project_panel.rename", "Rename…", KEY_CONTEXT, Rename),
        ("project_panel.delete", "Move to Trash", KEY_CONTEXT, Delete),
        (
            "project_panel.duplicate",
            "Duplicate",
            KEY_CONTEXT,
            Duplicate,
        ),
        (
            "project_panel.copy_path",
            "Copy Path",
            KEY_CONTEXT,
            CopyPath,
        ),
        (
            "project_panel.copy_relative_path",
            "Copy Relative Path",
            KEY_CONTEXT,
            CopyRelativePath,
        ),
    ];
    let mut registry = Registry::new();
    for (id, title, context, op) in commands {
        registry
            .register(id, title, move |target: &mut Option<PanelOp>, _: &Actor| {
                *target = Some(op);
                Ok(Dispatched::new(Effect::Notify))
            })
            .expect("project panel command IDs are valid and distinct")
            .context(ContextPredicate::identifier(context));
    }
    // The workbench's typed command, which knows the active file: listed
    // still, it names no op, so the panel passes it on.
    registry
        .register(
            "project_panel.reveal_active",
            "Reveal Active File in Project",
            |_: &mut Option<PanelOp>, _: &Actor| Ok(Dispatched::new(Effect::Notify)),
        )
        .expect("project panel command IDs are valid and distinct")
        .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    registry
}

/// The op of `run`, if it's a panel command that applies in its context.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<PanelOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}

#[cfg(test)]
impl ProjectPanel {
    /// The row the last frame drew selected.
    pub(crate) fn drawn_selected(&self) -> Option<usize> {
        self.drawn_selected
    }

    /// The selected row's index, if it's shown.
    pub(crate) fn selected_row(&self) -> Option<usize> {
        self.selected_index()
    }

    /// The row ranges the last frame built.
    pub(crate) fn built(&self) -> Vec<Range<usize>> {
        self.built.clone()
    }

    /// Whether `dir`'s failed listing waits for a change to be retried.
    pub(crate) fn waits_to_retry(&self, dir: NodeId) -> bool {
        self.failed.get(&dir) == Some(&self.epoch)
    }

    /// Where the list was laid out in the last frame.
    pub(crate) fn list_bounds(&self) -> gpui::Bounds<gpui::Pixels> {
        self.scroll.0.borrow().base_handle.bounds()
    }

    /// Where row `ix` was laid out in the last frame, from the list's bounds,
    /// scroll offset and measured row height.
    pub(crate) fn row_bounds(&self, ix: usize) -> Option<gpui::Bounds<gpui::Pixels>> {
        let count = self.tree()?.row_count() + usize::from(self.field_row().is_some());
        let ix = self.list_ix(ix);
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

/// What a row was built with: its label's colour, if it set one, and its
/// badge and the badge's colour, if any.
#[cfg(test)]
pub(crate) type DrawnStatus = (Option<gpui::Hsla>, Option<(String, gpui::Hsla)>);

#[cfg(test)]
impl ProjectPanel {
    /// Each row the last frame built, by name, and the status it drew.
    pub(crate) fn drawn_status(&self) -> Vec<(String, DrawnStatus)> {
        self.drawn_status.clone()
    }
}

#[path = "project_panel_ops.rs"]
mod ops;

pub(crate) use ops::running;

#[cfg(test)]
#[path = "project_panel_hover_tests.rs"]
mod hover_tests;

#[cfg(test)]
#[path = "project_panel_git_status_tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "project_ignored_tests.rs"]
mod ignored_tests;
