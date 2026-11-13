//! The project panel, the right dock's Explorer's Folders view: the lazy
//! file tree, listed off the main thread and kept current by a file
//! watcher.
//!
//! One row is selected, kept by node so it survives rows above it changing.
//! Clicks and the keys of [`init`]'s registry (in the panel's
//! [`KEY_CONTEXT`]) move it; [`ProjectPanel::reveal`] selects a file's row,
//! expanding its collapsed ancestors.
//!
//! Several rows can be selected, as VS Code's explorer and JetBrains'
//! project view select them: shift-click selects the rows the list shows
//! from the anchor (the row last clicked alone or cmd-clicked) to the one
//! clicked, cmd-click (ctrl-click off macOS) selects one more or one fewer,
//! and shift-up and shift-down extend from the anchor a row at a time,
//! each saying how many are selected to a screen reader. A plain click, or
//! any other move, selects one again, and so does a filter. One of them is
//! the primary, the row last clicked or moved to: the keys move from it,
//! and what names one entry (rename, new file or folder, reveal) acts on it
//! alone. Delete, cut, copy, duplicate and copy path act on every selected
//! row the list shows ([`ProjectPanel::selection`]).
//!
//! The tree's first row is the root, the folder's name in its own case;
//! rows are 24 px, in UI body. The dock's header names the view
//! ("Project") and carries its actions ([`pane_actions`]).
//!
//! Rows show git's status as the Changes panel lists it
//! ([`ProjectPanel::set_git_statuses`]), in the theme's `vcs.*` colours
//! ([`vcs_color`]): a changed file's name in its status's colour with its
//! letter at the row's end, and a folder holding changes, expanded or
//! collapsed, in the colour of the highest-precedence status beneath it
//! with a 6 px dot centred where a letter goes.
//!
//! The file operations on the selected row (new file and folder, rename,
//! move to the Trash, duplicate, copy path) are in [`ops`]; expanding and
//! collapsing every folder, toggling ignored entries and refreshing are in
//! [`nav`]; cutting, copying and pasting entries are in [`clipboard`];
//! filtering the rows by name is in [`filter`]; the right-click menu that
//! offers them is in [`menu`]; the header's New menu, with typed files and
//! their [`templates`], is in [`new_menu`]; dragging rows onto a folder to
//! move or copy them is in [`drag`].

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
use fe_ui::{
    ActiveTheme as _, Icon, ROW_HEIGHT, Role, TreeRow, UI_BODY, file_icon_ref,
    folder_icon_ref_named,
};
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    Global, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    ParentElement as _, Render, ScrollStrategy, SharedString, Styled as _, Task,
    UniformListScrollHandle, Window, div, prelude::FluentBuilder as _, px, uniform_list,
};
use kiln_l10n::{Text, text, tr, verbatim};
use kiln_shell::PaneAction;

use crate::git_panel;
use crate::workbench::toast::{self, Toast};
use ops::EditKind;

/// The sidebar's Explorer, whose views are the project tree ([`FOLDERS`])
/// over the outline ([`crate::outline_panel::VIEW`]). Its id is the
/// project panel's of before, which layouts saved then name.
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

/// The project tree's view in the Explorer.
pub(crate) const FOLDERS: kiln_shell::ViewId = kiln_shell::ViewId("folders");

/// The key context the panel's element sets.
pub(crate) const KEY_CONTEXT: &str = "project_panel";

/// [`KEY_CONTEXT`] with vim on, for its netrw and nvim-tree keys, unless
/// the name field or the filter's box is open.
const VIM_KEY_CONTEXT: &str = verbatim!("project_panel vim");

/// The workbench's command that selects the active editor's file.
const REVEAL_ACTIVE: &str = verbatim!("project_panel.reveal_active");

/// The root row's text's inset: where a depth-0 row's chevron starts, its
/// selection box's inset and the row's padding.
const ROOT_INSET: gpui::Pixels = px(10.);

/// The colour of a row of git status `kind`: the theme's `vcs.*` token for
/// it, never the accent, which is the user's focus.
pub(crate) fn vcs_color(kind: ChangeKind, cx: &App) -> Hsla {
    let vcs = &cx.theme().colors.vcs;
    match kind {
        ChangeKind::Added => vcs.added,
        ChangeKind::Modified => vcs.modified,
        ChangeKind::Deleted => vcs.deleted,
        ChangeKind::Untracked => vcs.untracked,
        ChangeKind::Conflicted => vcs.conflict,
    }
}

/// A git status `kind` as a screen reader says it after a row's name: the
/// word for the letter (or a folder's dot) the row shows.
fn status_word(kind: ChangeKind) -> SharedString {
    match kind {
        ChangeKind::Added => tr!("workbench-project-status-added"),
        ChangeKind::Modified => tr!("workbench-project-status-modified"),
        ChangeKind::Deleted => tr!("workbench-project-status-deleted"),
        ChangeKind::Untracked => tr!("workbench-project-status-untracked"),
        ChangeKind::Conflicted => tr!("workbench-project-status-conflicted"),
    }
}

/// Node `id`'s place among its folder's children in `tree`, from 1, and
/// how many children that folder has: the model's, whatever rows the list
/// draws or the filter hides. `hints` keeps, per folder, where the last
/// node looked up was, so a draw's rows, which run in order, scan each
/// folder's children about once.
fn place_in_folder(
    tree: &FileTree,
    id: NodeId,
    hints: &mut HashMap<NodeId, usize>,
) -> Option<(usize, usize)> {
    let path = tree.path(id)?;
    let folder = tree.find(path.parent()?)?;
    let siblings = tree.children(folder)?;
    let hint = hints.get(&folder).map_or(0, |&at| at.min(siblings.len()));
    let at = (siblings[hint..].iter().position(|&node| node == id))
        .map(|ix| hint + ix)
        .or_else(|| siblings[..hint].iter().position(|&node| node == id))?;
    hints.insert(folder, at);
    Some((at + 1, siblings.len()))
}

/// The project tree's actions, in the dock's header before ⋯ and —: `+`,
/// leftmost, opening the New menu under it ([`new_menu`]); reveal the
/// active file; and collapse every folder.
pub(crate) fn pane_actions(panel: &Entity<ProjectPanel>) -> Vec<PaneAction> {
    let reveal = REVEAL_ACTIVE;
    let collapse = "project_panel.collapse_all";
    let weak = panel.downgrade();
    let (new, new_at) = (panel.downgrade(), panel.downgrade());
    vec![
        PaneAction {
            id: NEW_BUTTON,
            icon: Icon::Plus,
            title: tr!("workbench-project-new"),
            command: None,
            // With no button to put the menu under: its first entry.
            run: Rc::new(move |window, cx| {
                (new.update(cx, |panel, cx| panel.run_op(PanelOp::NewFile, window, cx))).ok();
            }),
            anchored: Some(Rc::new(move |anchor, window, cx| {
                (new_at.update(cx, |panel, cx| panel.toggle_new_menu(anchor, window, cx))).ok();
            })),
        },
        PaneAction {
            id: "project-reveal",
            icon: Icon::Target,
            title: fe_command::l10n::command_label(reveal, ""),
            command: Some(reveal),
            run: Rc::new(move |window, cx| {
                crate::pane_toolbar::dispatch(reveal, fe_command::Source::Mouse, window, cx)
            }),
            anchored: None,
        },
        PaneAction {
            id: "project-collapse",
            icon: Icon::FoldVertical,
            title: fe_command::l10n::command_label(collapse, ""),
            command: Some(collapse),
            // As its key does, dropping a reveal still waiting for a listing.
            run: Rc::new(move |window, cx| {
                (weak.update(cx, |panel, cx| {
                    panel.run_op(PanelOp::CollapseAll, window, cx)
                }))
                .ok();
            }),
            anchored: None,
        },
    ]
}

/// The `+`'s id among the header's buttons: `icon-button:project-new`.
pub(crate) const NEW_BUTTON: &str = "project-new";

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
    /// The panel renamed `from`, a file or a folder, to `to`, or moved it
    /// there (a paste of a cut entry): the editors
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

// One per panel, so its size is no concern; over the lint's limit only on
// Windows, where std's paths are a few bytes bigger.
#[cfg_attr(windows, allow(clippy::large_enum_variant))]
enum State {
    NoFolder,
    /// The root is being opened and listed in the background.
    Opening,
    Open(FileTree),
    /// The root couldn't be opened.
    /// Why, resolved as it's drawn.
    Failed(Text),
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
    /// The primary selected row.
    selected: Option<NodeId>,
    /// The rows selected with the primary, by shift- and cmd-clicks and
    /// shift-arrows: they count only while they hold the primary and
    /// another ([`Self::several`]), so whatever selects a row of its own
    /// (the right-click menu, say) selects it alone.
    multi: HashSet<NodeId>,
    /// The row shift-clicks and shift-arrows extend the selection from,
    /// while several are selected; the primary otherwise.
    anchor: Option<NodeId>,
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
    /// Every row the last frame drew selected, the primary's included.
    #[cfg(test)]
    drawn_selection: Vec<usize>,
    /// What the panel last said to a screen reader of the selection.
    #[cfg(test)]
    announced: Option<String>,
    /// Each row the last frame built, by name, and the status it drew.
    #[cfg(test)]
    drawn_status: Vec<(String, DrawnStatus)>,
    /// The root row's text the last frame drew, if it drew one.
    #[cfg(test)]
    drawn_root: Option<String>,
    /// Each row the last frame drew tinted as where a drop would go, by
    /// name, and whether it drew the root so.
    #[cfg(test)]
    drawn_drop: Vec<String>,
    #[cfg(test)]
    drawn_root_drop: bool,
    /// Whether the tree lists ignored entries.
    show_ignored: bool,
    /// Folders being expanded as their parents' listings land: expand
    /// all's, or those expanded before the tree was opened again.
    expansion: Option<nav::Expansion>,
    /// The panel's last word on an operation, under its rows, resolved as
    /// it's drawn.
    status: Option<Text>,
    /// The entry cut or copied, to paste.
    clipboard: clipboard::Clipboard,
    /// The file operations to undo and redo.
    history: undo::History,
    /// Each row the last frame drew dimmed as cut, by name.
    #[cfg(test)]
    drawn_cut: Vec<String>,
    /// The filter by name, while it's set.
    filter: Option<filter::Filter>,
    /// The right-click menu, while it's open.
    menu: Option<menu::TreeMenu>,
    /// The header's New menu, while it's open.
    new_menu: Option<new_menu::NewMenu>,
    /// A drag of the tree's rows, while it's under way.
    drag: Option<drag::Drag>,
    _watch: Option<Task<()>>,
    /// Its text, and its open fields' placeholders, in a new language.
    _locale: gpui::Subscription,
}

impl EventEmitter<PanelEvent> for ProjectPanel {}

impl ProjectPanel {
    /// A panel for `root`, or saying no folder is open. The root is opened,
    /// listed and watched in the background; nothing touches the file system
    /// on this thread.
    pub fn new(root: Option<PathBuf>, show_ignored: bool, cx: &mut Context<Self>) -> Self {
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
            multi: HashSet::new(),
            anchor: None,
            selected_hint: 0,
            reveal: None,
            revealing: None,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
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
            drawn_selection: Vec::new(),
            #[cfg(test)]
            announced: None,
            #[cfg(test)]
            drawn_status: Vec::new(),
            #[cfg(test)]
            drawn_root: None,
            #[cfg(test)]
            drawn_drop: Vec::new(),
            #[cfg(test)]
            drawn_root_drop: false,
            show_ignored,
            expansion: None,
            status: None,
            clipboard: clipboard::Clipboard::default(),
            history: undo::History::default(),
            #[cfg(test)]
            drawn_cut: Vec::new(),
            filter: None,
            menu: None,
            new_menu: None,
            drag: None,
            _watch: None,
            _locale: cx.observe_global::<kiln_platform::locale::ActiveLocale>(Self::follow_locale),
        };
        panel.start(show_ignored, cx);
        panel
    }

    /// Opens, lists and watches the root in the background, listing ignored
    /// entries too if `show_ignored`; the tree it replaces, if any, goes.
    fn start(&mut self, show_ignored: bool, cx: &mut Context<Self>) {
        let options = TreeOptions { show_ignored };
        // `cx.is_test()` also covers other crates' tests, where `cfg!(test)` is off.
        let in_tests = cfg!(test) || cx.is_test();
        let watch = self.root.clone().map(|root| {
            let (sender, batches) = async_channel::unbounded::<Vec<FsChange>>();
            let opened = cx.background_spawn(async move {
                // Watching first, so changes made while the root lists still
                // arrive. Not in tests, which hand batches to
                // `apply_fs_changes`: GPUI's test scheduler rejects wakeups
                // from the watcher's thread.
                let watcher = if in_tests {
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

    /// Draws its text, resolved as it's drawn, in the language just chosen,
    /// with the placeholders of the name field and the filter's box.
    fn follow_locale(&mut self, cx: &mut Context<Self>) {
        let inputs = (self.edit.as_ref().map(ops::NameEdit::input))
            .into_iter()
            .chain(self.filter_input());
        for input in inputs.collect::<Vec<_>>() {
            input.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    /// The words it shows now, in the active locale, by what shows them:
    /// `message` in place of the rows, `status` under them, and while open
    /// the name field's `name-placeholder` and `name-error` and the filter
    /// box's `filter-placeholder`. For tests outside the crate.
    #[doc(hidden)]
    pub fn shown_words(&self, cx: &App) -> Vec<(&'static str, String)> {
        let message = match &self.state {
            State::NoFolder => Some(tr!("workbench-project-no-folder")),
            State::Failed(message) => Some(message.resolve()),
            State::Open(_) if self.filtered().is_some_and(|view| view.is_empty()) => {
                Some(filter::no_matches())
            }
            State::Open(_) | State::Opening => None,
        };
        let edit = self.edit.as_ref();
        let words = [
            ("message", message),
            ("status", self.status.as_ref().map(Text::resolve)),
            (
                "name-placeholder",
                edit.map(|edit| edit.input().read(cx).placeholder_text()),
            ),
            ("name-error", edit.and_then(ops::NameEdit::error)),
            (
                "filter-placeholder",
                (self.filter_input()).map(|input| input.read(cx).placeholder_text()),
            ),
        ];
        (words.into_iter())
            .filter_map(|(what, words)| Some((what, words?.to_string())))
            .collect()
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
        self.reopen(cx);
    }

    /// Opens the tree again, as [`Self::new`] first did.
    fn reopen(&mut self, cx: &mut Context<Self>) {
        self.expansion = None;
        self.status = None;
        // The new tree numbers its nodes afresh, so what the old ids named
        // goes; a pending reveal is by path and carries over.
        self.generation += 1;
        self.expanded.clear();
        self.expanding.clear();
        self.listing.clear();
        self.failed.clear();
        self.selected = None;
        self.select_one();
        self.selected_hint = 0;
        self.revealing = None;
        self.edit = None;
        self.filter = None;
        self.menu = None;
        self.new_menu = None;
        self.clip_reopened();
        self.start(self.show_ignored, cx);
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
                let path = root.display().to_string();
                let toast = tr!("workbench-project-cant-open-folder", path = &path);
                toast::show(cx, Toast::error(toast).detail(error.to_string()));
                let error = error.to_string();
                State::Failed(text!(
                    "workbench-project-cannot-open",
                    path = path,
                    error = error
                ))
            }
        };
        // The entry to paste may be gone from the tree just opened.
        self.drop_gone_clip(cx);
        self.advance_reveal(cx);
        self.advance_expansion(cx);
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
        self.drop_gone_clip(cx);
        // Something changed, so failed listings may succeed now.
        self.epoch += 1;
        if changed {
            cx.notify();
        }
        self.reconcile(cx);
        self.advance_reveal(cx);
        self.advance_expansion(cx);
        self.refilter(cx);
    }

    /// Forgets nodes the tree removed (a watcher batch or a listing can).
    /// If the selected one went, selects the row now at its index,
    /// `selected`, clamped to the last row: in a filtered list the nearest
    /// match, or none with none left.
    fn removed(&mut self, selected: Option<usize>, cx: &mut Context<Self>) {
        // The menu goes with its row.
        self.close_gone_menu(cx);
        let State::Open(tree) = &self.state else {
            return;
        };
        let live = |id: &NodeId| tree.row(*id).is_some() || *id == tree.root();
        self.expanded.retain(live);
        self.expanding.retain(live);
        self.multi.retain(live);
        if self.anchor.is_some_and(|id| !live(&id)) {
            self.anchor = None;
        }
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
        let near = selected
            .zip(last)
            .and_then(|(ix, last)| self.nearest_match(ix.min(last)));
        if let Some(ix) = near {
            self.select_row(ix, cx);
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

    /// The selected row's index, if it's shown, and the filter doesn't
    /// hide it: what acts on the selection acts on a row the list shows.
    fn selected_index(&self) -> Option<usize> {
        let ix = Self::index_of(self.tree()?, self.selected, self.selected_hint)?;
        self.shows(ix).then_some(ix)
    }

    /// Selects row `ix` alone and scrolls it into view.
    fn select_row(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.make_primary(ix, cx) {
            self.select_one();
        }
    }

    /// Makes row `ix` the primary selected row, keeping the others, and
    /// scrolls it into view. Whether there's such a row.
    fn make_primary(&mut self, ix: usize, cx: &mut Context<Self>) -> bool {
        let Some(row) = self.tree().and_then(|tree| tree.rows(ix..ix + 1).next()) else {
            return false;
        };
        self.selected = Some(row.id);
        self.selected_hint = ix;
        self.scroll
            .scroll_to_item(self.list_pos(ix), ScrollStrategy::Nearest);
        cx.notify();
        true
    }

    /// Selects the first row, or the last, for vim's `gg` and `G`: the
    /// first or last match in a filtered list.
    fn select_end(&mut self, last: bool, cx: &mut Context<Self>) {
        if let Some(found) = self.end_match(last) {
            if let Some(ix) = found {
                self.select_row(ix, cx);
            }
            return;
        }
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
    /// selects the first row if none is. In a filtered list it moves to
    /// the next or previous match ([`Self::step_match`]).
    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(found) = self.step_match(delta) {
            if let Some(ix) = found {
                self.select_row(ix, cx);
            }
            return;
        }
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
    /// does, and otherwise selects its first child the list shows, if it
    /// has one.
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
            let depth = row.depth;
            let child = self.next_shown(ix).filter(|&child| {
                (tree.rows(child..child + 1).next()).is_some_and(|child| child.depth > depth)
            });
            if let Some(child) = child {
                self.select_row(child, cx);
            }
        } else if !self.expanding.contains(&id) {
            self.click(id, cx);
        }
    }

    /// Enter, and vim's `l` and `o`: opens the selected file, or toggles
    /// the selected directory, as a click does.
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
        self.select_one();
        match self.selected_index() {
            Some(ix) => self.select_row(ix, cx),
            // The filter hides it: the line above the rows says so.
            None => self.revealed_hidden(file),
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
    /// With shift held it extends the selection to the row instead, and
    /// with cmd (ctrl off macOS) it selects the row too, or no longer
    /// ([`Self::extend_to`], [`Self::toggle_selected`]): neither opens a
    /// file or toggles a folder.
    fn clicked(&mut self, id: NodeId, event: &gpui::ClickEvent, cx: &mut Context<Self>) {
        let modifiers = event.modifiers();
        if modifiers.shift || modifiers.secondary() {
            self.cancel_reveal();
            let Some(ix) = self
                .tree()
                .and_then(|tree| Self::index_of(tree, Some(id), 0))
            else {
                return;
            };
            if modifiers.shift {
                return self.extend_to(ix, cx);
            }
            return self.toggle_selected(ix, cx);
        }
        // A plain click selects its row alone.
        self.multi.clear();
        self.anchor = Some(id);
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
        self.drop_gone_clip(cx);
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
        self.advance_expansion(cx);
        self.refilter(cx);
    }

    /// Runs a panel command; others go on to the workbench. While the name
    /// field or the filter's box has focus, it takes the keys
    /// ([`Self::run_in_field`], [`Self::run_in_filter`]).
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        // In the name field and the filter's box, undo and redo are the
        // box's own, of its text: never the tree's file operations.
        if self.field_focused(window, cx) || self.filter_focused(window, cx) {
            let action: Option<Box<dyn gpui::Action>> = match op(run, cx) {
                Some(PanelOp::Undo) => Some(Box::new(fe_ui::text_field::Undo)),
                Some(PanelOp::Redo) => Some(Box::new(fe_ui::text_field::Redo)),
                _ => None,
            };
            if let Some(action) = action {
                return window.dispatch_action(action, cx);
            }
        }
        if self.field_focused(window, cx) {
            return self.run_in_field(run, window, cx);
        }
        if self.filter_focused(window, cx) {
            return self.run_in_filter(run, window, cx);
        }
        match op(run, cx) {
            Some(op) => self.run_op(op, window, cx),
            None => cx.propagate(),
        }
    }

    /// Does `op`, from a key, a command or a button in the dock's header.
    pub(crate) fn run_op(&mut self, op: PanelOp, window: &mut Window, cx: &mut Context<Self>) {
        // The user moved on. Toggling ignored entries carries a pending
        // reveal over to the new tree.
        if op != PanelOp::ToggleIgnored {
            self.cancel_reveal();
        }
        self.close_menu(cx);
        // Moving the selection, or opening or toggling the primary's row
        // as a click does, selects it alone first.
        use PanelOp::*;
        let moves = matches!(
            op,
            SelectPrev
                | SelectNext
                | SelectFirst
                | SelectLast
                | CollapseOrParent
                | ExpandOrFirstChild
                | Open
                | Toggle
        );
        if moves && self.several() {
            // Redrawn even if the move goes nowhere (Left on a root-level
            // row, say): the other rows are no longer selected.
            self.select_one();
            cx.notify();
        } else if moves {
            self.select_one();
        }
        match op {
            PanelOp::SelectPrev => self.move_selection(-1, cx),
            PanelOp::SelectNext => self.move_selection(1, cx),
            PanelOp::CollapseOrParent => self.collapse_or_parent(cx),
            PanelOp::ExpandOrFirstChild => self.expand_or_first_child(cx),
            PanelOp::Open | PanelOp::Toggle => self.open_selected(cx),
            PanelOp::SelectFirst => self.select_end(false, cx),
            PanelOp::SelectLast => self.select_end(true, cx),
            PanelOp::NewFile => self.start_edit(EditKind::NewFile, window, cx),
            PanelOp::NewFolder => self.start_edit(EditKind::NewFolder, window, cx),
            PanelOp::Rename => self.start_edit(EditKind::Rename, window, cx),
            PanelOp::Delete => self.delete_selected(window, cx),
            PanelOp::Duplicate => self.duplicate_selected(window, cx),
            PanelOp::CopyPath => self.copy_path(false, cx),
            PanelOp::CopyRelativePath => self.copy_path(true, cx),
            PanelOp::ExpandAll => self.expand_all(cx),
            PanelOp::CollapseAll => self.collapse_all(cx),
            PanelOp::ToggleIgnored => {
                // The new tree's rows are other nodes: the filter goes
                // first, so what's expanded carries over as it was.
                self.clear_filter(window, cx);
                self.toggle_ignored(cx)
            }
            PanelOp::Refresh => self.refresh(cx),
            PanelOp::Cut => self.clip(clipboard::ClipKind::Cut, cx),
            PanelOp::Copy => self.clip(clipboard::ClipKind::Copy, cx),
            PanelOp::Paste => self.paste(window, cx),
            PanelOp::Filter => self.start_filter(window, cx),
            PanelOp::ClearFilter => self.clear_filter(window, cx),
            PanelOp::Undo => self.revert(undo::Way::Undo, window, cx),
            PanelOp::Redo => self.revert(undo::Way::Redo, window, cx),
        }
        if matches!(op, PanelOp::NewFile | PanelOp::NewFolder | PanelOp::Rename) {
            self.scroll_to_field();
        }
    }

    /// The list's rows in `range`: the tree's, or those the filter shows,
    /// with the name field in place of the row being renamed or as a row of
    /// its own for a new entry ([`Self::list_field`]).
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
        let field = self.list_field();
        let renaming = (self.edit.as_ref())
            .filter(|edit| edit.kind == EditKind::Rename)
            .map(|edit| edit.node);
        // The rows the list's range shows: those after a new entry's field
        // are one further down the list.
        let row_ix = |ix: usize| match field {
            Some((at, _)) if ix > at => ix - 1,
            _ => ix,
        };
        let shown = self.list_rows(row_ix(range.start)..row_ix(range.end));
        #[cfg(test)]
        let mut drawn = Vec::new();
        let mut seen = None;
        let cut = self.cut_nodes();
        #[cfg(test)]
        let mut drawn_cut = Vec::new();
        let theme = fe_ui::ui_theme(cx);
        let mut places = HashMap::new();
        let weak = cx.entity().downgrade();
        let several = self.several();
        // What dragging one of the selected rows carries.
        let selection = several.then(|| self.dragged_selection());
        // The folder a drop would go in, whose rows are tinted: the root's
        // is the whole list's.
        let drop_target = (self.drop_target())
            .filter(|&dir| dir != tree.root())
            .and_then(|dir| tree.path(dir));
        #[cfg(test)]
        let mut drawn_selection = Vec::new();
        #[cfg(test)]
        let mut drawn_drop = Vec::new();
        let mut rows: Vec<gpui::AnyElement> = (shown.into_iter())
            .filter_map(|ix| Some((tree.rows(ix..ix + 1).next()?, ix)))
            .map(|(row, ix)| {
                let id = row.id;
                let primary = self.selected == Some(id);
                if primary {
                    seen = Some(ix);
                }
                let selected = primary || several && self.multi.contains(&id);
                #[cfg(test)]
                if selected {
                    drawn_selection.push(ix);
                }
                let name = row.name.to_string_lossy();
                let icon = match row.kind {
                    EntryKind::Dir => folder_icon_ref_named(&name, row.expanded),
                    EntryKind::Symlink => Icon::FileSymlink.into(),
                    EntryKind::File => file_icon_ref(&name),
                };
                if renaming == Some(id) {
                    return self.render_field(row.depth, icon, cx);
                }
                let mut element = TreeRow::new(("row", ix), name.into_owned(), &theme)
                    .depth(row.depth)
                    .icon(icon)
                    .focused(focused)
                    // A cut row is dimmed as an ignored one is.
                    .ignored(row.ignored || cut.contains(&id))
                    .error(row.error.is_some())
                    .selected(selected)
                    .active(primary)
                    .drop_target(
                        (drop_target.as_ref()).is_some_and(|dir| {
                            tree.path(id).is_some_and(|path| path.starts_with(dir))
                        }),
                    )
                    .on_click(cx.listener(move |panel, event, _, cx| panel.clicked(id, event, cx)))
                    .on_secondary_click({
                        let panel = weak.clone();
                        move |position, window, cx| {
                            (panel.update(cx, |panel, cx| {
                                panel.open_menu(Some(id), position, window, cx)
                            }))
                            .ok();
                        }
                    });
                if row.kind == EntryKind::Dir {
                    element = element.directory(row.expanded);
                }
                if let Some((dragged, ghost)) = self.dragged(id, selected, selection.as_ref(), cx) {
                    let panel = weak.clone();
                    element = element
                        .on_drag(dragged, ghost, move |dragged, window, cx| {
                            (panel.update(cx, |panel, cx| panel.drag_started(dragged, window, cx)))
                                .ok();
                        })
                        .on_drop(cx.listener(move |panel, dragged, window, cx| {
                            panel.dropped(dragged, drag::Over::Row(id), window, cx)
                        }));
                }
                #[cfg(test)]
                if element.is_drop_target() {
                    drawn_drop.push(row.name.to_string_lossy().into_owned());
                }
                if let Some((position, count)) = place_in_folder(tree, id, &mut places) {
                    element = element.in_set(position, count);
                }
                // An ignored row is left as it is.
                let status = (git && !row.ignored)
                    .then(|| self.git_status(tree, id, row.kind))
                    .flatten();
                if let Some((kind, badge)) = status {
                    let color = vcs_color(kind, cx);
                    element = element
                        .label_color(color)
                        .a11y_description(status_word(kind));
                    element = match badge {
                        Some(letter) => element.badge(letter, color),
                        None => element.dot(color),
                    };
                }
                #[cfg(test)]
                if cut.contains(&id) {
                    drawn_cut.push(row.name.to_string_lossy().into_owned());
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
                // A folder or file with no name yet.
                Some(EditKind::NewFolder) => folder_icon_ref_named("", false),
                _ => file_icon_ref(""),
            };
            rows.insert(at - range.start, self.render_field(depth, icon, cx));
        }
        #[cfg(test)]
        {
            self.drawn_status.extend(drawn);
            self.drawn_cut.extend(drawn_cut);
            self.drawn_selection.extend(drawn_selection);
            self.drawn_drop.extend(drawn_drop);
        }
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
    /// letter, or none for a folder's dot.
    fn git_status(
        &self,
        tree: &FileTree,
        id: NodeId,
        kind: EntryKind,
    ) -> Option<(ChangeKind, Option<&'static str>)> {
        let path = tree.path(id)?;
        let relative = path.strip_prefix(tree.root_path()).ok()?;
        match kind {
            EntryKind::Dir => Some((*self.git_dirs.get(relative)?, None)),
            EntryKind::File | EntryKind::Symlink => {
                let kind = *self.git_files.get(relative)?;
                Some((kind, Some(git_panel::letter(kind))))
            }
        }
    }

    fn message(&self, text: String, cx: &App) -> gpui::AnyElement {
        div()
            .px_3()
            .py_2()
            .text_color(fe_ui::ui_theme(cx).muted_text)
            .child(text)
            .into_any_element()
    }
}

/// Selecting several rows: see the module's docs.
impl ProjectPanel {
    /// Whether several rows are selected: those of `multi`, which holds the
    /// primary and another.
    fn several(&self) -> bool {
        self.multi.len() > 1 && self.selected.is_some_and(|id| self.multi.contains(&id))
    }

    /// Selects the primary alone, which the selection extends from next.
    fn select_one(&mut self) {
        self.multi.clear();
        self.anchor = self.selected;
    }

    /// The selected rows the list shows, in its order, as their tree
    /// indices and nodes: the primary alone unless several are selected.
    /// What acts on the selection acts on these.
    pub(super) fn selection(&self) -> Vec<(usize, NodeId)> {
        if !self.several() {
            return (self.selected_index().zip(self.selected).into_iter()).collect();
        }
        let Some(tree) = self.tree() else {
            return Vec::new();
        };
        (self.list_rows(0..self.list_count()).into_iter())
            .filter_map(|ix| {
                let id = tree.rows(ix..ix + 1).next()?.id;
                self.multi.contains(&id).then_some((ix, id))
            })
            .collect()
    }

    /// The row selections extend from, if the list shows it: the anchor
    /// while several rows are selected, else the primary.
    fn anchor_index(&self) -> Option<usize> {
        let tree = self.tree()?;
        let anchor = if self.several() {
            self.anchor
        } else {
            self.selected
        };
        let ix = Self::index_of(tree, anchor, 0)?;
        self.shows(ix).then_some(ix)
    }

    /// Shift-click on tree row `ix`, and shift-up and shift-down: selects
    /// the rows the list shows from the anchor to it, it the primary. With
    /// no row selected it's selected alone.
    fn extend_to(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(from) = self.anchor_index() else {
            self.select_row(ix, cx);
            return self.selected_count_changed(cx);
        };
        let Some(tree) = self.tree() else {
            return;
        };
        let anchor = tree.rows(from..from + 1).next().map(|row| row.id);
        let (low, high) = (from.min(ix), from.max(ix));
        let rows = self.list_rows(0..self.list_count());
        let multi = (rows.into_iter())
            .filter(|row| (low..=high).contains(row))
            .filter_map(|row| Some(tree.rows(row..row + 1).next()?.id))
            .collect();
        if !self.make_primary(ix, cx) {
            return;
        }
        self.multi = multi;
        self.anchor = anchor;
        self.selected_count_changed(cx);
    }

    /// Cmd-click (ctrl-click off macOS) on tree row `ix`: selects it too,
    /// the primary and the anchor, or, if it's selected, no longer, the
    /// primary moving to the nearest still selected below it, or else
    /// above, or with none left none selected.
    fn toggle_selected(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(id) = self
            .tree()
            .and_then(|tree| tree.rows(ix..ix + 1).next().map(|row| row.id))
        else {
            return;
        };
        let mut multi: HashSet<NodeId> = (self.selection().into_iter()).map(|(_, id)| id).collect();
        if multi.is_empty() {
            self.select_row(ix, cx);
            return self.selected_count_changed(cx);
        }
        if !multi.remove(&id) {
            multi.insert(id);
            self.make_primary(ix, cx);
            self.multi = multi;
            self.anchor = Some(id);
            return self.selected_count_changed(cx);
        }
        if multi.is_empty() {
            self.selected = None;
            self.select_one();
            cx.notify();
            return self.selected_count_changed(cx);
        }
        if self.selected == Some(id) {
            let left = self.selection();
            let left = left.iter().filter(|(_, row)| multi.contains(row));
            let below = left.clone().find(|(row, _)| *row > ix);
            let Some(&(primary, _)) = below.or(left.clone().next_back()) else {
                return;
            };
            self.make_primary(primary, cx);
        }
        self.multi = multi;
        self.anchor = self.selected;
        if self.multi.len() == 1 {
            self.select_one();
        }
        cx.notify();
        self.selected_count_changed(cx);
    }

    /// Shift-up and shift-down, `delta` -1 or 1: extends the selection from
    /// the anchor to the row the list shows above or below the primary.
    /// With no row selected, it moves the selection as up and down do.
    fn extend_by(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(at) = self.selected_index() else {
            self.move_selection(delta, cx);
            if self.selected_index().is_some() {
                self.selected_count_changed(cx);
            }
            return;
        };
        let rows = self.list_rows(0..self.list_count());
        let Ok(pos) = rows.binary_search(&at) else {
            return;
        };
        let Some(&next) = pos.checked_add_signed(delta).and_then(|pos| rows.get(pos)) else {
            return;
        };
        self.extend_to(next, cx);
    }

    /// Shift-up and shift-down, which no binding names: they extend the
    /// selection ([`Self::extend_by`]), unless the name field or the
    /// filter's box has the keys, or the right-click menu is open.
    fn on_extend_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let only_shift = modifiers.shift
            && !(modifiers.control || modifiers.alt || modifiers.platform || modifiers.function);
        let delta = match keystroke.key.as_str() {
            "up" if only_shift => -1,
            "down" if only_shift => 1,
            _ => return,
        };
        if self.menu_open() || self.field_focused(window, cx) || self.filter_focused(window, cx) {
            return;
        }
        self.cancel_reveal();
        self.extend_by(delta, cx);
        cx.stop_propagation();
    }

    /// Whether a menu is open, the right-click one or the New menu: the
    /// panel's keys are then the menu's.
    fn menu_open(&self) -> bool {
        self.menu.is_some() || self.new_menu.is_some()
    }

    /// Says how many rows are selected to a screen reader, as the
    /// selection grows or shrinks by a click or a key.
    fn selected_count_changed(&mut self, cx: &mut Context<Self>) {
        let count = self.selection().len();
        let said = tr!("workbench-project-selected-count", count = count);
        #[cfg(test)]
        {
            self.announced = Some(said.to_string());
        }
        fe_ui::announce(cx, said, fe_ui::Politeness::Polite);
    }
}

impl Render for ProjectPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.built.clear();
            self.drawn_selected = None;
            self.drawn_selection.clear();
            self.drawn_status.clear();
            self.drawn_cut.clear();
            self.drawn_drop.clear();
        }
        // A drag dropped elsewhere, or stopped; or rows moved under the
        // pointer.
        self.end_drag(cx);
        self.settle_drag_drawn(window, cx);
        // Something selected a row of its own (the right-click menu, say):
        // the rows selected with the one before go.
        if !self.several() && !self.multi.is_empty() {
            self.multi.clear();
        }
        // The root's row: the folder's name, as it's spelled.
        let root = match &self.state {
            State::Open(tree) => tree.root_path().file_name(),
            _ => self.root.as_deref().and_then(Path::file_name),
        }
        .map(|name| name.to_string_lossy().into_owned());
        // A drop in the root tints its row and the list.
        let root_drop = (self.drop_target())
            .is_some_and(|dir| self.tree().is_some_and(|tree| tree.root() == dir));
        let root_tint = root_drop.then(|| TreeRow::drop_color(cx));
        #[cfg(test)]
        {
            self.drawn_root = root.clone();
            self.drawn_root_drop = root_drop;
        }
        let root = root.map(|name| {
            div()
                .debug_selector(|| "project-root".into())
                .when_some(root_tint, |root, tint| root.bg(tint))
                .flex_none()
                .h(ROW_HEIGHT)
                .flex()
                .items_center()
                .pl(ROOT_INSET)
                .pr_2()
                .font_weight(FontWeight::MEDIUM)
                .child(div().min_w_0().truncate().child(name))
        });
        let field = self.list_field().is_some();
        let body = match &self.state {
            State::NoFolder => self.message(tr!("workbench-project-no-folder").into(), cx),
            State::Opening => div().into_any_element(),
            State::Failed(message) => self.message(message.resolve().into(), cx),
            State::Open(_) => {
                let rows = if self.filtered().is_some_and(|view| view.is_empty()) && !field {
                    self.message(filter::no_matches().into(), cx)
                } else {
                    uniform_list(
                        "project-rows",
                        self.list_count() + usize::from(field),
                        cx.processor(|panel, range, window, cx| {
                            panel.render_rows(range, window, cx)
                        }),
                    )
                    .track_scroll(&self.scroll)
                    .flex_1()
                    .into_any_element()
                };
                // The tree's empty space, below its rows or the line saying
                // none match: a row's own right click takes its press.
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .when_some(root_tint, |body, tint| body.bg(tint))
                    .child(rows)
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|panel, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            panel.open_menu(None, event.position, window, cx);
                        }),
                    )
                    .into_any_element()
            }
        };
        let filter = self.render_filter(cx);
        // Vim's tree keys are off while the name field or the filter's box
        // takes typing.
        let typing = self.edit.is_some() || self.filter_typing();
        let context = if self.menu_open() {
            menu::MENU_KEY_CONTEXT
        } else if self.dragging() {
            drag::DRAG_KEY_CONTEXT
        } else if fe_gpui::vim_enabled(cx) && !typing {
            VIM_KEY_CONTEXT
        } else {
            KEY_CONTEXT
        };
        let theme = fe_ui::ui_theme(cx);
        let panel = div()
            .id("project-tree")
            .track_focus(&self.focus)
            .key_context(context)
            .on_action(cx.listener(Self::on_run_command))
            .on_key_down(cx.listener(Self::on_menu_key))
            .on_key_down(cx.listener(Self::on_new_menu_key))
            .on_key_down(cx.listener(Self::on_extend_key))
            .on_key_down(cx.listener(Self::on_drag_key))
            // Rows dragged over the panel, and dropped where no row takes
            // them: in the root.
            .on_drag_move(cx.listener(Self::drag_moved))
            .on_drop(cx.listener(|panel, dragged, window, cx| {
                panel.dropped(dragged, drag::Over::Root, window, cx)
            }))
            .debug_selector(|| "sidebar-panel".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.panel_background)
            .text_color(theme.text)
            .text_size(UI_BODY.size)
            .line_height(UI_BODY.line_height)
            .children(root)
            .children(filter)
            .child(body)
            .children(
                (self.status.as_ref()).map(|status| self.message(status.resolve().into(), cx)),
            )
            .children(self.render_menu(window, cx))
            .children(self.render_new_menu(window, cx));
        // To a screen reader the panel is the `Tree` "Files", its rows the
        // tree's items (the list holds no node of its own). It's what takes
        // focus, so the selected row can be its active descendant.
        fe_ui::node(panel, window, cx, Role::Tree, tr!("workbench-project-tree"))
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
    Toggle,
    ExpandAll,
    CollapseAll,
    ToggleIgnored,
    Refresh,
    Cut,
    Copy,
    Paste,
    Filter,
    ClearFilter,
    Undo,
    Redo,
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

/// The title command `id` registers with: its `command-<id>` message,
/// which `label()` resolves again in the active locale wherever it's shown.
fn command_title(id: &str) -> gpui::SharedString {
    kiln_l10n::message(&fe_command::l10n::command_key(id))
}

pub(crate) fn registry() -> Registry<PanelCommands> {
    use PanelOp::*;
    let commands = [
        ("project_panel.select_prev", KEY_CONTEXT, SelectPrev),
        ("project_panel.select_next", KEY_CONTEXT, SelectNext),
        (
            "project_panel.collapse_or_parent",
            KEY_CONTEXT,
            CollapseOrParent,
        ),
        (
            "project_panel.expand_or_first_child",
            KEY_CONTEXT,
            ExpandOrFirstChild,
        ),
        ("project_panel.open", KEY_CONTEXT, Open),
        ("project_panel.select_first", KEY_CONTEXT, SelectFirst),
        ("project_panel.select_last", KEY_CONTEXT, SelectLast),
        ("project_panel.new_file", KEY_CONTEXT, NewFile),
        ("project_panel.new_folder", KEY_CONTEXT, NewFolder),
        ("project_panel.rename", KEY_CONTEXT, Rename),
        ("project_panel.delete", KEY_CONTEXT, Delete),
        ("project_panel.duplicate", KEY_CONTEXT, Duplicate),
        ("project_panel.copy_path", KEY_CONTEXT, CopyPath),
        (
            "project_panel.copy_relative_path",
            KEY_CONTEXT,
            CopyRelativePath,
        ),
        ("project_panel.toggle", KEY_CONTEXT, Toggle),
        ("project_panel.expand_all", KEY_CONTEXT, ExpandAll),
        ("project_panel.collapse_all", KEY_CONTEXT, CollapseAll),
        ("project_panel.toggle_ignored", KEY_CONTEXT, ToggleIgnored),
        ("project_panel.refresh", KEY_CONTEXT, Refresh),
        ("project_panel.cut", KEY_CONTEXT, Cut),
        ("project_panel.copy", KEY_CONTEXT, Copy),
        ("project_panel.paste", KEY_CONTEXT, Paste),
        ("project_panel.filter", KEY_CONTEXT, Filter),
        ("project_panel.clear_filter", KEY_CONTEXT, ClearFilter),
        ("project_panel.undo", KEY_CONTEXT, Undo),
        ("project_panel.redo", KEY_CONTEXT, Redo),
    ];
    let mut registry = Registry::new();
    // Undo and redo only where what goes to the Trash can be put back.
    let commands =
        (commands.into_iter()).filter(|(_, _, op)| undo::supported() || !matches!(op, Undo | Redo));
    for (id, context, op) in commands {
        registry
            .register(
                id,
                &command_title(id),
                move |target: &mut Option<PanelOp>, _: &Actor| {
                    *target = Some(op);
                    Ok(Dispatched::new(Effect::Notify))
                },
            )
            .expect("project panel command IDs are valid and distinct")
            .context(ContextPredicate::identifier(context));
    }
    // The workbench's typed command, which knows the active file: listed
    // still, it names no op, so the panel passes it on.
    registry
        .register(
            REVEAL_ACTIVE,
            &command_title(REVEAL_ACTIVE),
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

    /// Every selected row the list shows, by index, in order.
    pub(crate) fn selected_rows(&self) -> Vec<usize> {
        self.selection().into_iter().map(|(ix, _)| ix).collect()
    }

    /// Every row the last frame drew selected, the primary's included,
    /// each once.
    pub(crate) fn drawn_selection(&self) -> Vec<usize> {
        let mut drawn = self.drawn_selection.clone();
        drawn.sort_unstable();
        drawn.dedup();
        drawn
    }

    /// What the panel last said to a screen reader of the selection.
    pub(crate) fn announced(&self) -> Option<String> {
        self.announced.clone()
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
        self.tree()?;
        let count = self.list_count() + usize::from(self.field_row().is_some());
        let ix = self.list_pos(ix);
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

    /// Each row the last frame drew dimmed as cut, by name.
    pub(crate) fn drawn_cut(&self) -> Vec<String> {
        self.drawn_cut.clone()
    }

    /// The root row's text the last frame drew, if it drew one.
    pub(crate) fn drawn_root(&self) -> Option<String> {
        self.drawn_root.clone()
    }

    /// Each row the last frame drew tinted as where a drop would go, by
    /// name, and whether it tinted the root's row and the list so.
    /// Each once: the list measures its first row as well as building it.
    pub(crate) fn drawn_drop(&self) -> (Vec<String>, bool) {
        let mut drawn: Vec<String> = Vec::new();
        for name in &self.drawn_drop {
            if !drawn.contains(name) {
                drawn.push(name.clone());
            }
        }
        (drawn, self.drawn_root_drop)
    }
}

#[path = "project_panel_ops.rs"]
mod ops;

pub(crate) use ops::running;

#[path = "project_panel_nav.rs"]
mod nav;

#[cfg(test)]
#[path = "project_panel_nav_tests.rs"]
mod nav_tests;

#[path = "project_panel_clipboard.rs"]
mod clipboard;

#[cfg(test)]
#[path = "project_panel_clipboard_tests.rs"]
mod project_panel_clipboard_tests;

#[path = "project_panel_filter.rs"]
mod filter;

#[path = "project_panel_menu.rs"]
mod menu;

#[path = "project_panel_new_menu.rs"]
pub(crate) mod new_menu;

#[path = "project_new_templates.rs"]
pub(crate) mod templates;

#[path = "project_panel_drag.rs"]
mod drag;

#[path = "project_panel_undo.rs"]
mod undo;

#[cfg(test)]
#[path = "project_panel_filter_tests.rs"]
mod project_panel_filter_tests;

#[cfg(test)]
#[path = "project_panel_hover_tests.rs"]
mod hover_tests;

#[cfg(test)]
#[path = "project_panel_git_status_tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "project_ignored_tests.rs"]
mod ignored_tests;
