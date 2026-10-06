//! The project panel on the right dock: the lazy file tree, listed off the
//! main thread and kept current by a file watcher.

use std::{
    collections::{HashMap, HashSet},
    io,
    ops::Range,
    path::{Path, PathBuf},
};

use fe_project::{EntryKind, FileTree, FsChange, Listed, NodeId, ProjectWatcher, TreeOptions};
use fe_ui::{TreeRow, UiTheme};
use gpui::{
    AppContext as _, Context, EventEmitter, IntoElement, ParentElement as _, Render, Styled as _,
    Task, UniformListScrollHandle, Window, div, px, uniform_list,
};

/// The dock's width, fixed until docks can be resized.
pub const PANEL_WIDTH: gpui::Pixels = px(240.);

/// What the panel asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelEvent {
    /// A file row was clicked.
    OpenFile(PathBuf),
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
    selected: Option<NodeId>,
    scroll: UniformListScrollHandle,
    theme: UiTheme,
    /// The row ranges the last frame built: the list builds the visible
    /// rows and measures its first row twice.
    #[cfg(test)]
    built: Vec<Range<usize>>,
    _watch: Option<Task<()>>,
}

impl EventEmitter<PanelEvent> for ProjectPanel {}

impl ProjectPanel {
    /// A panel for `root`, or saying no folder is open. The root is opened,
    /// listed and watched in the background; nothing touches the file system
    /// on this thread.
    pub fn new(root: Option<PathBuf>, theme: UiTheme, cx: &mut Context<Self>) -> Self {
        let watch = root.clone().map(|root| {
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
                (FileTree::open(&root, TreeOptions::default()), watcher)
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
        Self {
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
            selected: None,
            scroll: UniformListScrollHandle::new(),
            theme,
            #[cfg(test)]
            built: Vec::new(),
            _watch: watch,
        }
    }

    pub fn tree(&self) -> Option<&FileTree> {
        match &self.state {
            State::Open(tree) => Some(tree),
            _ => None,
        }
    }

    fn opened(&mut self, tree: io::Result<FileTree>, cx: &mut Context<Self>) {
        self.state = match tree {
            Ok(tree) => State::Open(tree),
            Err(error) => {
                let root = self.root.as_deref().unwrap_or(Path::new(""));
                log::error!("cannot open {}: {error}", root.display());
                State::Failed(format!("Cannot open {}: {error}", root.display()))
            }
        };
        cx.notify();
    }

    /// Brings the tree up to date with a batch from the file watcher, then
    /// lists again any shown directory it says may have missed something.
    pub fn apply_fs_changes(&mut self, changes: &[FsChange], cx: &mut Context<Self>) {
        let State::Open(tree) = &mut self.state else {
            return;
        };
        let version = tree.version();
        tree.apply(changes);
        let tree = &*tree;
        let live = |id: &NodeId| tree.row(*id).is_some();
        self.expanded.retain(live);
        self.expanding.retain(live);
        if self.selected.is_some_and(|id| !live(&id)) {
            self.selected = None;
        }
        // Something changed, so failed listings may succeed now.
        self.epoch += 1;
        if tree.version() != version {
            cx.notify();
        }
        self.reconcile(cx);
    }

    /// What clicking a row does: a directory expands once listed (or
    /// collapses, or stops waiting to expand); a file opens.
    fn click(&mut self, id: NodeId, cx: &mut Context<Self>) {
        let State::Open(tree) = &mut self.state else {
            return;
        };
        let Some(row) = tree.row(id) else {
            return;
        };
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
        let path = tree.path(id).and_then(|path| {
            let rel = path.strip_prefix(tree.root_path()).ok()?;
            Some(self.root.as_ref()?.join(rel))
        });
        if let Some(path) = path {
            self.selected = Some(id);
            cx.emit(PanelEvent::OpenFile(path));
            cx.notify();
        }
    }

    /// Lists in the background each directory shown or waiting to expand
    /// that the tree wants listed (a result can make others than its own
    /// directory owe a listing), unless one is running or its last failed.
    /// Expands those waiting that need none.
    fn reconcile(&mut self, cx: &mut Context<Self>) {
        let State::Open(tree) = &mut self.state else {
            return;
        };
        let dirs: Vec<NodeId> = [tree.root()]
            .into_iter()
            .chain(self.expanded.iter().copied())
            .chain(self.expanding.iter().copied())
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
            cx.spawn(async move |this, cx| {
                let listed = listed.await;
                this.update(cx, |panel, cx| panel.listed(dir, listed, cx))
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
        tree.fill(dir, listed);
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
    }

    fn render_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<TreeRow> {
        let State::Open(tree) = &self.state else {
            return Vec::new();
        };
        #[cfg(test)]
        self.built.push(range.clone());
        tree.rows(range.clone())
            .zip(range)
            .map(|(row, ix)| {
                let id = row.id;
                let mut element = TreeRow::new(
                    ("row", ix),
                    row.name.to_string_lossy().into_owned(),
                    &self.theme,
                )
                .depth(row.depth)
                .ignored(row.ignored)
                .error(row.error.is_some())
                .selected(self.selected == Some(id))
                .on_click(cx.listener(move |panel, _, _, cx| panel.click(id, cx)));
                if row.kind == EntryKind::Dir {
                    element = element.directory(row.expanded);
                }
                element
            })
            .collect()
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
        self.built.clear();
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
                tree.row_count(),
                cx.processor(|panel, range, _, cx| panel.render_rows(range, cx)),
            )
            .track_scroll(&self.scroll)
            .flex_1()
            .into_any_element(),
        };
        div()
            .w(PANEL_WIDTH)
            .h_full()
            .flex_none()
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

#[cfg(test)]
impl ProjectPanel {
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
        let count = self.tree()?.row_count();
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
