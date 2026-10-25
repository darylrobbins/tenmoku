//! The Call Hierarchy panel, as VS Code's: `nav.show_call_hierarchy`
//! (shift-alt-h) asks the active editor's language server for the call
//! hierarchy item at the caret ([`LspManager::prepare_call_hierarchy`]; with
//! several, the first, as VS Code picks) and shows it, as the root of a tree
//! of calls, in the right dock's Call Hierarchy panel, made the first time
//! the command runs and on the rail from its first showing.
//!
//! - **Rows.** A row is an item (its kind's icon, name, dimmed detail) with
//!   the number of call sites when more than one. The root is open when it
//!   lands; any other row loads its calls the first time it's expanded,
//!   asking only that item's: `callHierarchy/incomingCalls` (its callers) or
//!   `outgoingCalls` (its callees), by the panel's mode, off the UI thread.
//!   A row loading has a child "Loading…", an empty one "No calls", a failed
//!   one the error's message. A row that is one of its ancestors is marked
//!   "recursive" and isn't expanded.
//! - **Opening.** A click or Enter opens the row's file at the item's
//!   selection. For an incoming call that is not the root, it opens the
//!   caller's call site instead (the first of its `fromRanges`, and the next
//!   with each Enter or click on the row again). A click also expands a
//!   collapsed row. Up and Down move, Right expands (or moves to the first
//!   call) and Left collapses (or moves to the parent).
//! - **Direction.** `hierarchy.toggle_direction`, and the panel's button,
//!   flips incoming and outgoing calls and loads the root's again.
//! - **Requests.** Each prepare and each toggle starts a new generation:
//!   the tasks of the one before are dropped, which cancels their requests,
//!   and an answer of an older generation that lands anyway is ignored.

use std::{
    ops::Range,
    sync::{Arc, Mutex},
};

use fe_gpui::EditorView;
use fe_lsp::{CallEdge, HierarchyItem, LspError, LspManager};
use fe_text::Text;
use fe_ui::{ActiveTheme as _, Icon, IconButton, TreeRow};
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, ScrollStrategy, Styled as _, Task,
    UniformListScrollHandle, WeakEntity, Window, div, px, uniform_list,
};

use crate::{
    Workbench,
    dock::{DockOp, SidebarPanel},
    outline_panel::symbol_icon,
    pane::Reveal,
    workbench::toast::{self, Toast},
};

/// The panel in the sidebar: on the rail once it has shown; focused when
/// shown, as its rows take keys.
pub(crate) const SIDEBAR: SidebarPanel = SidebarPanel {
    panel: crate::DockPanel::new("call_hierarchy"),
    name: "Call Hierarchy",
    icon: Icon::ListTree,
    show_command: "nav.show_call_hierarchy",
    show_op: DockOp::ShowHierarchy,
    order: 80,
    always_on_rail: false,
    focus_on_show: true,
};

/// What the toast says for a caret with no item, or a server that
/// doesn't provide call hierarchy.
const NONE_HERE: &str = "No call hierarchy available here";

/// What the toast says when no language server has the file.
const NO_SERVER: &str = "No language server for this file";

/// Whether the panel lists the calls into an item or out of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Incoming,
    Outgoing,
}

impl Mode {
    fn flipped(self) -> Self {
        match self {
            Mode::Incoming => Mode::Outgoing,
            Mode::Outgoing => Mode::Incoming,
        }
    }

    /// What the button that flips to the other mode says.
    fn other_label(self) -> &'static str {
        match self {
            Mode::Incoming => "Show Outgoing Calls",
            Mode::Outgoing => "Show Incoming Calls",
        }
    }
}

/// A row's calls.
#[derive(Debug)]
enum Children {
    NotLoaded,
    Loading,
    Loaded(Vec<Node>),
    Failed(String),
}

/// An item and the calls under it.
#[derive(Debug)]
struct Node {
    item: HierarchyItem,
    /// The call sites between the item and its parent.
    sites: Vec<lsp_types::Range>,
    children: Children,
    expanded: bool,
    /// The item is one of its ancestors (or the root): not expandable.
    recursive: bool,
    /// The call site the next open goes to.
    next_site: usize,
}

impl Node {
    fn new(item: HierarchyItem, sites: Vec<lsp_types::Range>, recursive: bool) -> Self {
        Self {
            item,
            sites,
            children: Children::NotLoaded,
            expanded: false,
            recursive,
            next_site: 0,
        }
    }

    /// Makes every Loading node below (and this one) loadable again.
    fn reset_loading(&mut self) {
        match &mut self.children {
            Children::Loading => self.children = Children::NotLoaded,
            Children::Loaded(children) => children.iter_mut().for_each(Node::reset_loading),
            _ => {}
        }
    }
}

/// A row as drawn: a node (by its path of child indices from the root) or
/// a message under one.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    Node(Vec<usize>),
    Message {
        depth: usize,
        text: String,
        error: bool,
    },
}

/// The Call Hierarchy panel: see the module docs.
pub(crate) struct HierarchyPanel {
    mode: Mode,
    root: Option<Node>,
    /// The buffer the root came from: the server of its file takes the
    /// follow-up requests.
    buffer: Option<fe_buffer::BufferId>,
    manager: Option<Arc<Mutex<LspManager>>>,
    /// The selected node's path.
    selected: Vec<usize>,
    /// Moves on with each prepare and each toggle: only the tasks and
    /// answers of the latest count.
    generation: u64,
    /// Moves on with each open, so a slow read of an older one is dropped.
    opened: u64,
    /// The prepare and the calls in flight, dropped (cancelled) when a new
    /// generation starts.
    tasks: Vec<Task<()>>,
    workbench: WeakEntity<Workbench>,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
}

impl HierarchyPanel {
    pub(crate) fn new(workbench: WeakEntity<Workbench>, cx: &mut Context<Self>) -> Self {
        Self {
            mode: Mode::Incoming,
            root: None,
            buffer: None,
            manager: None,
            selected: Vec::new(),
            generation: 0,
            opened: 0,
            tasks: Vec::new(),
            workbench,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
        }
    }

    /// The dock's title row: `Call Hierarchy: <name>`.
    pub(crate) fn title(&self, _: &App) -> gpui::SharedString {
        match &self.root {
            Some(root) => format!("Call Hierarchy: {}", root.item.name).into(),
            None => "Call Hierarchy".into(),
        }
    }

    /// Starts a new generation: older requests are dropped.
    fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        // A read of an older open is dropped too.
        self.opened += 1;
        self.tasks.clear();
        // Rows whose requests were just cancelled would stay Loading.
        if let Some(root) = &mut self.root {
            root.reset_loading();
        }
        self.generation
    }

    /// Asks for the items at the caret of `buffer` at `version`, and shows
    /// the first as the root when they land, showing the dock.
    fn prepare(
        &mut self,
        manager: Arc<Mutex<LspManager>>,
        buffer: fe_buffer::BufferId,
        version: fe_buffer::Version,
        offset: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let generation = self.next_generation();
        // The old tree goes: its rows' requests are cancelled, and an
        // answer for them mustn't land in the new one.
        self.root = None;
        self.selected = Vec::new();
        cx.notify();
        let asked = lock(&manager).prepare_call_hierarchy(buffer, version, offset);
        let task = cx.spawn_in(window, async move |panel, cx| {
            let answer = asked.await;
            let shown = panel.update_in(cx, |panel, window, cx| {
                panel.prepared(generation, manager, buffer, answer, window, cx)
            });
            if !shown.unwrap_or(false) {
                return;
            }
            let workbench = panel.read_with(cx, |panel, _| panel.workbench.clone());
            if let Ok(workbench) = workbench {
                workbench
                    .update_in(cx, |workbench, window, cx| {
                        workbench.run_dock_op(DockOp::ShowHierarchy, window, cx)
                    })
                    .ok();
            }
        });
        self.tasks.push(task);
    }

    /// Shows the answer to a prepare as the root, or toasts why not.
    /// Whether the dock is to show.
    fn prepared(
        &mut self,
        generation: u64,
        manager: Arc<Mutex<LspManager>>,
        buffer: fe_buffer::BufferId,
        answer: Result<Option<Vec<HierarchyItem>>, LspError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        let item = match answer {
            Ok(Some(items)) => items.into_iter().next(),
            Ok(None) => None,
            Err(error) => {
                toast::show(cx, Toast::error(format!("Call hierarchy: {error}")));
                return false;
            }
        };
        let Some(item) = item else {
            toast::show(cx, Toast::info(NONE_HERE));
            return false;
        };
        self.manager = Some(manager);
        self.buffer = Some(buffer);
        let mut root = Node::new(item, Vec::new(), false);
        root.expanded = true;
        self.root = Some(root);
        self.selected = Vec::new();
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.load(Vec::new(), window, cx);
        cx.notify();
        true
    }

    /// Flips incoming and outgoing calls, loading the root's again.
    pub(crate) fn toggle_direction(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = self.mode.flipped();
        self.next_generation();
        self.selected = Vec::new();
        if let Some(root) = &mut self.root {
            root.children = Children::NotLoaded;
            root.expanded = true;
            self.load(Vec::new(), window, cx);
        }
        cx.notify();
    }

    fn node(&self, path: &[usize]) -> Option<&Node> {
        let mut node = self.root.as_ref()?;
        for ix in path {
            match &node.children {
                Children::Loaded(children) => node = children.get(*ix)?,
                _ => return None,
            }
        }
        Some(node)
    }

    fn node_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        let mut node = self.root.as_mut()?;
        for ix in path {
            match &mut node.children {
                Children::Loaded(children) => node = children.get_mut(*ix)?,
                _ => return None,
            }
        }
        Some(node)
    }

    /// The items from the root down to the node at `path`.
    fn ancestors(&self, path: &[usize]) -> Vec<&HierarchyItem> {
        let mut items = Vec::new();
        let mut node = self.root.as_ref();
        for ix in path.iter().map(Some).chain([None]) {
            let Some(current) = node else { break };
            items.push(&current.item);
            node = ix.and_then(|ix| match &current.children {
                Children::Loaded(children) => children.get(*ix),
                _ => None,
            });
        }
        items
    }

    /// Asks for the calls of the node at `path`, whose children show
    /// "Loading…" until they land.
    fn load(&mut self, path: Vec<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(manager), Some(buffer)) = (self.manager.clone(), self.buffer) else {
            return;
        };
        let mode = self.mode;
        let generation = self.generation;
        let Some(node) = self.node_mut(&path) else {
            return;
        };
        node.children = Children::Loading;
        let item = node.item.clone();
        let asked: std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<CallEdge>, LspError>> + Send>,
        > = {
            let manager = lock(&manager);
            match mode {
                Mode::Incoming => Box::pin(manager.incoming_calls(buffer, &item)),
                Mode::Outgoing => Box::pin(manager.outgoing_calls(buffer, &item)),
            }
        };
        let task = cx.spawn_in(window, async move |panel, cx| {
            let answer = asked.await;
            panel
                .update(cx, |panel, cx| panel.loaded(generation, path, answer, cx))
                .ok();
        });
        self.tasks.push(task);
        cx.notify();
    }

    /// Puts the calls of the node at `path` under it, unless a newer
    /// generation started.
    fn loaded(
        &mut self,
        generation: u64,
        path: Vec<usize>,
        answer: Result<Vec<CallEdge>, LspError>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        let children = match answer {
            Ok(edges) => {
                let ancestors: Vec<HierarchyItem> =
                    self.ancestors(&path).into_iter().cloned().collect();
                Children::Loaded(
                    edges
                        .into_iter()
                        .map(|edge| {
                            let recursive = ancestors.iter().any(|a| {
                                a.path == edge.item.path
                                    && a.selection_range == edge.item.selection_range
                                    && a.name == edge.item.name
                            });
                            Node::new(edge.item, edge.from_ranges, recursive)
                        })
                        .collect(),
                )
            }
            Err(error) => Children::Failed(error.to_string()),
        };
        if let Some(node) = self.node_mut(&path) {
            node.children = children;
        }
        cx.notify();
    }

    /// Expands the node at `path`, loading its calls the first time.
    fn expand(&mut self, path: Vec<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = self.node_mut(&path) else {
            return;
        };
        if node.recursive || node.expanded {
            return;
        }
        node.expanded = true;
        if matches!(node.children, Children::NotLoaded) {
            self.load(path, window, cx);
        }
        cx.notify();
    }

    /// The rows shown, top to bottom.
    fn rows(&self) -> Vec<Row> {
        fn push(node: &Node, path: &mut Vec<usize>, rows: &mut Vec<Row>) {
            rows.push(Row::Node(path.clone()));
            if !node.expanded || node.recursive {
                return;
            }
            let depth = path.len() + 1;
            let message = |text: &str, error: bool| Row::Message {
                depth,
                text: text.to_owned(),
                error,
            };
            match &node.children {
                Children::NotLoaded => {}
                Children::Loading => rows.push(message("Loading…", false)),
                Children::Failed(error) => rows.push(message(error, true)),
                Children::Loaded(children) if children.is_empty() => {
                    rows.push(message("No calls", false))
                }
                Children::Loaded(children) => {
                    for (ix, child) in children.iter().enumerate() {
                        path.push(ix);
                        push(child, path, rows);
                        path.pop();
                    }
                }
            }
        }
        let mut rows = Vec::new();
        if let Some(root) = &self.root {
            push(root, &mut Vec::new(), &mut rows);
        }
        rows
    }

    /// The paths of the node rows, in order.
    fn node_paths(&self) -> Vec<Vec<usize>> {
        (self.rows().into_iter())
            .filter_map(|row| match row {
                Row::Node(path) => Some(path),
                Row::Message { .. } => None,
            })
            .collect()
    }

    fn select(&mut self, path: Vec<usize>, cx: &mut Context<Self>) {
        let ix = self
            .rows()
            .iter()
            .position(|row| *row == Row::Node(path.clone()));
        self.selected = path;
        if let Some(ix) = ix {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    /// Moves the selection by `by` node rows.
    fn move_selection(&mut self, by: isize, cx: &mut Context<Self>) {
        let paths = self.node_paths();
        let Some(at) = paths.iter().position(|path| *path == self.selected) else {
            return;
        };
        let to = (at as isize + by).clamp(0, paths.len() as isize - 1) as usize;
        self.select(paths[to].clone(), cx);
    }

    /// Selects the node at `path` and opens it: see the module docs.
    fn open(&mut self, path: Vec<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let incoming = self.mode == Mode::Incoming;
        let is_root = path.is_empty();
        let Some(node) = self.node_mut(&path) else {
            return;
        };
        let (item, range) = if incoming && !is_root && !node.sites.is_empty() {
            let site = node.next_site % node.sites.len();
            node.next_site = (site + 1) % node.sites.len();
            (node.item.clone(), node.sites[site])
        } else {
            (node.item.clone(), node.item.selection_range)
        };
        self.select(path, cx);
        // An older open still reading its file is dropped.
        self.opened += 1;
        let (token, focus, panel) = (self.opened, self.focus.clone(), cx.weak_entity());
        if let Some(workbench) = self.workbench.upgrade() {
            workbench.update(cx, |workbench, cx| {
                workbench.open_hierarchy_range(
                    item,
                    range,
                    (panel, token, focus.clone()),
                    window,
                    cx,
                )
            });
        }
        // The panel keeps focus, so Enter again goes to the next call site
        // and the arrows still move.
        window.focus(&focus, cx);
    }

    /// A click on the node at `path`: opens it, and expands it if collapsed.
    fn clicked(&mut self, path: Vec<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.open(path.clone(), window, cx);
        self.expand(path, window, cx);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let selected = self.selected.clone();
        match event.keystroke.key.as_str() {
            "up" => self.move_selection(-1, cx),
            "down" => self.move_selection(1, cx),
            "enter" => self.open(selected, window, cx),
            "right" => {
                let Some(node) = self.node(&selected) else {
                    return;
                };
                if node.recursive {
                    return;
                }
                if node.expanded {
                    // Only onto a first call: not past a node still loading,
                    // empty or failed.
                    if matches!(&node.children, Children::Loaded(c) if !c.is_empty()) {
                        self.move_selection(1, cx);
                    }
                } else {
                    self.expand(selected, window, cx);
                }
            }
            "left" => {
                let Some(node) = self.node(&selected) else {
                    return;
                };
                if node.expanded && !node.recursive {
                    if let Some(node) = self.node_mut(&selected) {
                        node.expanded = false;
                    }
                    cx.notify();
                } else if !selected.is_empty() {
                    self.select(selected[..selected.len() - 1].to_vec(), cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn render_row(
        &mut self,
        row: &Row,
        ix: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> TreeRow {
        let ui = fe_ui::ui_theme(cx);
        let dim = cx.theme().colors.text.muted;
        match row {
            Row::Message { depth, text, error } => {
                let row = TreeRow::new(("call-hierarchy-message", ix), text.clone(), &ui)
                    .depth(*depth)
                    .error(*error);
                if *error { row } else { row.label_color(dim) }
            }
            Row::Node(path) => {
                let node = self.node(path).expect("a drawn row has its node");
                let mut badge = Vec::new();
                if node.recursive {
                    badge.push("recursive".to_owned());
                }
                if node.sites.len() > 1 {
                    badge.push(node.sites.len().to_string());
                }
                let target = path.clone();
                let mut row = TreeRow::new(("call-hierarchy-row", ix), node.item.name.clone(), &ui)
                    .depth(path.len())
                    .icon(symbol_icon(node.item.kind))
                    .detail(node.item.detail.clone().unwrap_or_default(), dim)
                    .selected(*path == self.selected)
                    .focused(self.focus.contains_focused(window, cx))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.clicked(target.clone(), window, cx)
                    }));
                if !node.recursive {
                    row = row.directory(node.expanded);
                }
                if !badge.is_empty() {
                    row = row.badge(badge.join(" · "), dim);
                }
                row
            }
        }
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<TreeRow> {
        let rows = self.rows();
        range
            .filter_map(|ix| Some(self.render_row(rows.get(ix)?, ix, window, cx)))
            .collect()
    }
}

/// Locks the manager.
fn lock(manager: &Mutex<LspManager>) -> std::sync::MutexGuard<'_, LspManager> {
    manager
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Focusable for HierarchyPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for HierarchyPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = fe_ui::ui_theme(cx);
        let count = self.rows().len();
        let list = uniform_list(
            "call-hierarchy-rows",
            count,
            cx.processor(|panel, range, window, cx| panel.render_rows(range, window, cx)),
        )
        .track_scroll(&self.scroll)
        .flex_1();
        let flip = {
            let mode = self.mode;
            let icon = match mode {
                Mode::Incoming => Icon::ArrowRight,
                Mode::Outgoing => Icon::ArrowLeft,
            };
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .child(
                    IconButton::new("call-hierarchy-direction", icon, &ui).on_click(
                        cx.listener(|panel, _, window, cx| panel.toggle_direction(window, cx)),
                    ),
                )
                .child(div().text_color(ui.muted_text).child(mode.other_label()))
        };
        div()
            .track_focus(&self.focus)
            .key_context("call_hierarchy_panel")
            .on_key_down(cx.listener(Self::on_key_down))
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
            .child(flip)
            .child(list)
    }
}

impl Workbench {
    /// Shows the call hierarchy of the symbol at the active editor's caret
    /// in the Call Hierarchy panel, made now if it hasn't been.
    pub(crate) fn show_call_hierarchy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Any request still pending is for an earlier ask.
        if let Some(panel) = self.dock().read(cx).hierarchy().cloned() {
            panel.update(cx, |panel, _| {
                panel.next_generation();
            });
        }
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        let (buffer, version, caret) = {
            let model = editor.read(cx).model();
            let buffer = model.buffer();
            (
                buffer.id(),
                buffer.version(),
                model.primary_selection().head(),
            )
        };
        let manager = self
            .lsp
            .manager()
            .filter(|manager| lock(manager).server_of(buffer).is_some());
        let Some(manager) = manager else {
            toast::show(cx, Toast::info(NO_SERVER));
            return;
        };
        let panel = match self.dock().read(cx).hierarchy().cloned() {
            Some(panel) => panel,
            None => {
                let workbench = cx.weak_entity();
                let panel = cx.new(|cx| HierarchyPanel::new(workbench, cx));
                self.dock()
                    .update(cx, |dock, cx| dock.set_hierarchy(panel.clone(), cx));
                panel
            }
        };
        panel.update(cx, |panel, cx| {
            panel.prepare(manager, buffer, version, caret, window, cx)
        });
    }

    /// Flips the panel's mode, if it has been made.
    pub(crate) fn toggle_hierarchy_direction(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(panel) = self.dock().read(cx).hierarchy().cloned() {
            panel.update(cx, |panel, cx| panel.toggle_direction(window, cx));
        }
    }

    /// Opens `item`'s file with `range` (positions as the item's server
    /// counts them) selected, recording where the caret was.
    fn open_hierarchy_range(
        &mut self,
        item: HierarchyItem,
        range: lsp_types::Range,
        (panel, token, focus): (WeakEntity<HierarchyPanel>, u64, FocusHandle),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = self
            .open_editors(cx)
            .into_iter()
            .find_map(|editor: Entity<EditorView>| {
                let model = editor.read(cx).model();
                (model.path() == Some(item.path.as_path())).then(|| model.text().clone())
            });
        let path = item.path.clone();
        let keep = focus.clone();
        let jump = move |workbench: &mut Self,
                         text: &Text,
                         window: &mut Window,
                         cx: &mut Context<Self>| {
            let mut at = item.clone();
            at.selection_range = range;
            let bytes = at.selection_in(text);
            workbench.push_history(cx);
            // The pane gives the panel focus back once a file still to be
            // read lands.
            workbench.pane().clone().update(cx, |pane, cx| {
                pane.open_at_keeping_focus(
                    item.path.clone(),
                    Some(Reveal::bytes(bytes)),
                    keep.clone(),
                    window,
                    cx,
                )
            });
        };
        if let Some(text) = open {
            return jump(self, &text, window, cx);
        }
        // Not open: its text as the editor will load it, read off the UI
        // thread.
        let file = path.clone();
        let read = cx.background_spawn(async move {
            let text = std::fs::read_to_string(&file).ok()?;
            Some(Text::from_file_contents(&text).0)
        });
        let pane = self.pane().clone();
        let (opens, navigations) = {
            let pane = pane.read(cx);
            (pane.open_serial(), pane.navigations())
        };
        cx.spawn_in(window, async move |workbench, cx| {
            let Some(text) = read.await else { return };
            workbench
                .update_in(cx, |workbench, window, cx| {
                    // Another row or hierarchy was asked for, another pane is
                    // active, or the pane opened or showed a tab, meanwhile.
                    let current = panel.upgrade().is_some_and(|p| p.read(cx).opened == token);
                    let same = *workbench.pane() == pane && {
                        let pane = pane.read(cx);
                        pane.open_serial() == opens && pane.navigations() == navigations
                    };
                    if !current || !same {
                        return;
                    }
                    jump(workbench, &text, window, cx);
                    window.focus(&focus, cx);
                })
                .ok();
        })
        .detach();
    }
}

#[cfg(test)]
impl HierarchyPanel {
    /// The rows' texts, indented by depth, as the panel draws them.
    pub(crate) fn texts(&self) -> Vec<String> {
        self.rows()
            .iter()
            .map(|row| match row {
                Row::Message { depth, text, .. } => format!("{}{text}", "  ".repeat(*depth)),
                Row::Node(path) => {
                    let node = self.node(path).unwrap();
                    let mut text = format!("{}{}", "  ".repeat(path.len()), node.item.name);
                    if node.recursive {
                        text.push_str(" (recursive)");
                    } else if node.sites.len() > 1 {
                        text.push_str(&format!(" ×{}", node.sites.len()));
                    }
                    text
                }
            })
            .collect()
    }

    pub(crate) fn mode(&self) -> Mode {
        self.mode
    }

    pub(crate) fn selected_path(&self) -> &[usize] {
        &self.selected
    }

    /// Whether the row at `path` is expanded.
    pub(crate) fn is_expanded(&self, path: &[usize]) -> bool {
        self.node(path).is_some_and(|node| node.expanded)
    }

    pub(crate) fn root_name(&self) -> Option<&str> {
        self.root.as_ref().map(|root| root.item.name.as_str())
    }

    pub(crate) fn press(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let event = KeyDownEvent {
            keystroke: gpui::Keystroke::parse(key).unwrap(),
            is_held: false,
            prefer_character_input: false,
        };
        self.on_key_down(&event, window, cx);
    }
}

#[cfg(test)]
#[path = "hierarchy_panel_tests.rs"]
mod hierarchy_panel_tests;
