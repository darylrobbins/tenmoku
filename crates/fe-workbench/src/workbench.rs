//! The window shell: the editor panes, with the dock (the project panel,
//! search, the outline, the git changes, references or the problems) on the
//! right and the terminal below.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::Buffer;
use fe_config::TerminalSettings;
use fe_editor::{EditorModel, save_target};
use fe_git::ChangeKind;
use fe_gpui::{
    DiffView, EditorSettings, EditorTheme, EditorView, SettingsEvent, commands::RunCommand,
};
use fe_ui::{ROW_HEIGHT, UiTheme};
use gpui::{
    AnyView, App, AppContext as _, Context, Entity, EntityId, EventEmitter, ExternalPaths,
    FocusHandle, Focusable, Global, InteractiveElement as _, IntoElement, ParentElement as _,
    PromptButton, PromptLevel, Render, Styled as _, Subscription, WeakEntity, Window, WindowBounds,
    div, px,
};

use self::recent_picker::{RecentOp, RecentPicker};
use self::toast::Toast;
use crate::{
    Dock, GitPanel, GitPanelEvent, KEY_CONTEXT, MenuBarView, OutlinePanel, Pane, PaneGroup, Picker,
    ProjectPanel, QuickOpen, ReferencesPanel, SearchPanel, StatusBar,
    auto_save::AutoSave,
    config_problems::ConfigProblemsPicker,
    disk_sync::DiskSync,
    dock::{self, DockEvent, DockOp, ProblemsOp, ProblemsPanel},
    dock_layout,
    find_bar::FindOp,
    git::GitState,
    git_panel,
    go_to_line::GoToLine,
    language_picker::LanguagePicker,
    loader::{self, Opened},
    lsp::Lsp,
    nav::Nav,
    open::{self, OpenOp},
    outline_panel,
    pane_group::{Axis, Destination, Direction, RESIZE_STEP},
    pane_toolbar::{self, NavButtons, SymbolChain, ToolbarRequest},
    project_panel::{self, PanelEvent, PanelOp},
    terminal_panel::{TERMINAL, TerminalPanel, TerminalPanelEvent},
    title_bar,
    toolbar_picker::ToolbarPicker,
    trust::{self, TrustChanged},
};
use fe_command::Value;
use fe_ui::Icon;
use kiln::ViewCommands as _;
use kiln_shell::{
    CloseKind, CloseRequest, Closing, DockPosition, DockState, DockView as _, ItemHandle,
    ModalEvent, ModalId, ModalLayer, PaneView as _, Shell, Workspace, WorkspaceEvent,
    close::{self, Items, LiveItems},
    layout::{self, LayoutStore, WindowLayout},
};

/// Shown in a tab when fastedit starts with nothing to open.
pub const WELCOME: &str = "\
fastedit

Open a folder or a file with:  cargo run -p fastedit -- path

Keys: arrows, alt-arrows (words), cmd-arrows (line/document),
shift to select, double/triple-click, cmd-c/x/v, cmd-z/shift-cmd-z, cmd-s.
Tabs: cmd-shift-[ and cmd-shift-] to switch, cmd-w to close.
";

/// What the workbench opens with.
pub struct Launch {
    /// The project panel's folder.
    pub root: Option<PathBuf>,
    /// A file to open in a tab: its path, and what [`loader::open`] made of
    /// it if it's done by now, a buffer or a large file's view. Without it,
    /// the tab shows the file loading until the host hands the read to
    /// [`Workbench::file_read`].
    pub file: Option<(PathBuf, Option<Opened>)>,
}

/// The launch for the command-line argument `arg`, made absolute: a
/// directory is the root, with no file; a file, or a missing path (a new
/// file, which saving creates), opens in a tab with its parent directory as
/// the root. No argument is no root, and the welcome text in a tab.
pub fn resolve_launch(arg: Option<PathBuf>) -> Launch {
    let Some(arg) = arg else {
        return Launch {
            root: None,
            file: None,
        };
    };
    let path = std::path::absolute(&arg).unwrap_or(arg);
    if path.is_dir() {
        return Launch {
            root: Some(path),
            file: None,
        };
    }
    Launch {
        root: path.parent().map(Path::to_owned),
        file: Some((path, None)),
    }
}

/// The saved layouts, in `dir` (`<state>/layouts`), of the windows on the
/// canonical root folder `root`, or with no root, those of the windows a
/// launch with no argument opens: theirs are kept under the key of the
/// empty path, which no canonical root has, so their backups too are their
/// own ([`hot_exit`]).
pub fn window_layouts(dir: impl Into<PathBuf>, root: Option<&Path>) -> LayoutStore {
    LayoutStore::new(dir, root.unwrap_or(Path::new("")))
}

/// The palette shown over the window, one at a time, in kiln-shell's
/// modal layer: fastedit's names for its [`ModalId`]s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Palette {
    QuickOpen,
    Language,
    Toolbar,
    Definitions,
    Symbols,
    ConfigProblems,
    GoToLine,
    /// The new name for `lsp.rename`.
    Rename,
    /// `lsp.code_actions`' list.
    CodeActions,
    Recent,
    /// `git.switch_branch`'s list, or the new branch's name.
    Branch,
}

impl Palette {
    const ALL: [Palette; 11] = [
        Palette::QuickOpen,
        Palette::Language,
        Palette::Toolbar,
        Palette::Definitions,
        Palette::Symbols,
        Palette::ConfigProblems,
        Palette::GoToLine,
        Palette::Rename,
        Palette::CodeActions,
        Palette::Recent,
        Palette::Branch,
    ];

    /// Its id in the modal layer.
    pub(crate) fn id(self) -> ModalId {
        ModalId(match self {
            Palette::QuickOpen => "quick_open",
            Palette::Language => "language",
            Palette::Toolbar => "toolbar",
            Palette::Definitions => "definitions",
            Palette::Symbols => "symbols",
            Palette::ConfigProblems => "config_problems",
            Palette::GoToLine => "go_to_line",
            Palette::Rename => "rename",
            Palette::CodeActions => "code_actions",
            Palette::Recent => "recent",
            Palette::Branch => "git_branch",
        })
    }

    /// The palette shown as `id`.
    fn of(id: ModalId) -> Option<Self> {
        Self::ALL.into_iter().find(|palette| palette.id() == id)
    }
}

/// The workbench was re-rooted at the folder it carries, by
/// [`Workbench::set_root`], for whatever indexes the root to follow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootChanged(pub PathBuf);

/// The sidebar rail's Terminal button.
const TERMINAL_BUTTON: &str = "terminal";

pub struct Workbench {
    panel: Entity<ProjectPanel>,
    dock: Entity<Dock>,
    /// The terminal's dock, at the bottom unless moved, hidden at launch.
    terminal_dock: Entity<kiln_shell::Dock>,
    terminal: Entity<TerminalPanel>,
    /// To the terminal's shell exiting, to the bottom dock hiding it with
    /// focus in it, and to the bottom dock showing or hiding, which the
    /// sidebar's Terminal button follows.
    _terminal: [Subscription; 3],
    /// The window's shell around the panes, which it keeps in step: their
    /// group, the active one, a pane emptied closing, tabs dropped moving.
    workspace: Entity<Workspace<Pane>>,
    /// The workspace's active pane, which files open in, as last followed;
    /// its active tab is the active editor.
    active: Entity<Pane>,
    /// The pane the workspace makes a restored layout's panes like.
    template: Rc<RefCell<WeakEntity<Pane>>>,
    /// To the workspace's events and its notifications.
    _workspace: [Subscription; 2],
    /// Per pane: its toolbar's requests.
    toolbar_requests: HashMap<EntityId, Subscription>,
    /// Calls of [`Workbench::move_tab`] and tabs dropped on a pane, which
    /// the workspace moves.
    #[cfg(test)]
    pub(crate) moves: usize,
    /// Per pane: tabs dropped on it, counted in `moves`.
    #[cfg(test)]
    drops: HashMap<EntityId, Subscription>,
    /// Per open editor: the editor, held until its tab's close is seen
    /// here, and its notifications, passed on to the editors that share its
    /// buffer.
    editors: HashMap<EntityId, (Entity<EditorView>, Subscription)>,
    /// Per pane, the file the project panel last opened there, until its
    /// editor shows (an open can wait behind a Save As): for vim's normal
    /// mode then ([`Workbench::open_from_tree`]).
    tree_opens: HashMap<EntityId, (PathBuf, Subscription)>,
    /// Hands the editors still saving to [`DetachedSaves`] as the
    /// workbench goes, with its window.
    _release: Subscription,
    /// Editors notified because an editor sharing their buffer was, so their
    /// own notification isn't passed back.
    echoes: HashSet<EntityId>,
    quick_open: Entity<Picker<QuickOpen>>,
    language_picker: Entity<Picker<LanguagePicker>>,
    /// The config files' problems, shown by `workbench.show_config_problems`.
    pub(crate) config_problems: Entity<Picker<ConfigProblemsPicker>>,
    /// To kiln's config diagnostics, which the status bar shows.
    _config_problems: Subscription,
    go_to_line: Entity<Picker<GoToLine>>,
    /// The recent folders and files, shown by `workbench.open_recent`.
    recent_picker: Entity<Picker<RecentPicker>>,
    /// The workspace's shell: the chrome, the dock, the status bar and the
    /// palettes' modal layer, around the panes.
    shell: Entity<Shell>,
    /// The shell's modal layer, which shows the palettes.
    modals: Entity<ModalLayer>,
    /// The palette that shows, if one does: the modal layer's, as of its
    /// last change.
    pub(crate) palette: Option<Palette>,
    /// Go to definition, workspace symbols, back and forward.
    pub(crate) nav_state: Nav,
    status_bar: Entity<StatusBar>,
    status_bar_visible: bool,
    /// The menu bar drawn in the window, where the platform has no native
    /// one: see [`Workbench::set_menu_bar`].
    menu_bar: Option<Entity<MenuBarView>>,
    /// Stands in for the platform's client-side decorations: the test
    /// platform's windows only have server-side ones.
    #[cfg(test)]
    pub(crate) client_tiling: Option<gpui::Tiling>,
    /// The folder the workbench is rooted at, if any.
    pub(crate) root: Option<PathBuf>,
    /// The editor whose file the project panel last revealed.
    revealed: Option<EntityId>,
    /// The launch file the host is reading, and the pane it opens in, until
    /// [`Workbench::file_read`] hands it over.
    launch_read: Option<(PathBuf, WeakEntity<Pane>)>,
    /// Whether the open panel shows: it doesn't block the window, and
    /// another isn't shown until it's answered.
    prompting: bool,
    /// The root's workspace trust.
    pub(crate) trust: kiln_shell::trust::RootTrust,
    /// The root's language servers.
    pub(crate) lsp: Lsp,
    ui: UiTheme,
    /// To the project panel's events: replaced with it on a new root.
    root_subscription: Subscription,
    /// To the Changes panel's events: replaced with it on a new root.
    changes_subscription: Subscription,
    /// The root's git status, kept live in the Changes panel and the open
    /// editors' diff bases: replaced on a new root.
    git: Entity<GitState>,
    _rail: Subscription,
    /// To the sidebar's dock and the terminal's: their changes (a toggle, a
    /// drag of an edge, another panel shown) ask the workspace to save the
    /// layout, as its own docks do.
    _docks: [Subscription; 2],
    /// To the modal layer's palettes coming and going.
    _modals: Subscription,
    /// To the status bar, whose vim line shows in its place even while
    /// it's hidden, and gives the host commands typed on it.
    _status_bar: [Subscription; 2],
    /// The window's toasts, over the panes at the bottom right.
    pub(crate) toasts: Entity<toast::ToastLayer>,
    /// What the panes' toolbar crumbs and "…" list.
    toolbar_picker: Entity<Picker<ToolbarPicker>>,
    /// Whether the panes show their toolbars.
    breadcrumbs: bool,
    nav: NavButtons,
    /// What the active pane's symbol chain was last worked out for: the
    /// pane, the editor, its caret and the outline: `None` if none can come
    /// (plain text, an unsupported language, a file too large), `Some(None)`
    /// while it's still to land, else its generation.
    chain_key: Option<(EntityId, EntityId, usize, Option<Option<u64>>)>,
    /// To the outline, so a landed outline updates the crumbs.
    _outline: Subscription,
    #[cfg(test)]
    pub(crate) chains_computed: usize,
    /// Times the nav buttons' state was pushed into the panes.
    #[cfg(test)]
    pub(crate) nav_pushes: usize,
    /// Every command the workbench was dispatched, oldest first.
    #[cfg(test)]
    pub(crate) commands_run: Vec<fe_command::Invocation>,
    /// The id of the window's saved layout, once the host gives it one:
    /// the workspace saves it ([`Workspace::set_layout`]).
    layout_id: Option<u64>,
    /// A restored layout's items are still being read, as the workspace's
    /// are ([`Workspace::restoring`]): the workbench takes up its editors
    /// once they're in.
    restoring: bool,
    /// fastedit's own keys in the saved layout.
    layout_keys: Rc<LayoutKeys>,
    /// Keeps the window's bounds in [`LayoutKeys`].
    _bounds: Subscription,
    /// The open buffers' backups, once the window has a layout.
    hot_exit: hot_exit::HotExit,
    /// Keeps the open buffers in step with their files on disk.
    pub(crate) disk_sync: DiskSync,
    /// Saves changed buffers as `editor.auto_save` says.
    pub(crate) auto_save: AutoSave,
    /// An editor a problem opened in, left in vim's insert or replace mode,
    /// and the problem's start: see [`Workbench::land_problem`].
    pub(crate) problem_landing: Option<(WeakEntity<EditorView>, usize)>,
    /// The editors shown outside the tabs, and what shows each: see
    /// [`Workbench::own_editor`].
    owned_editors: Vec<(WeakEntity<EditorView>, git_commands::Owner)>,
    /// The diff view whose left side had focus last: [`Self::active_editor`]
    /// is that side while it's the active item, else the right one.
    diff_left: Option<EntityId>,
    /// Runs the host commands vim's keys leave in the active editor:
    /// see [`Workbench::follow_vim_keys`].
    vim_keys: Option<(EntityId, Subscription)>,
    /// Every git op run and owned editor's host command, oldest first.
    #[cfg(test)]
    pub(crate) git_routed: Vec<git_commands::Routed>,
}

impl EventEmitter<RootChanged> for Workbench {}
impl EventEmitter<TrustChanged> for Workbench {}

impl Workbench {
    /// Opens `launch`. `settings` configure the editors it opens (tab size,
    /// soft wrap); `window` is where file rows open their tabs. Nothing
    /// touches the file system on this thread: the project panel lists
    /// in the background.
    pub fn new(
        launch: Launch,
        theme: EditorTheme,
        settings: EditorSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let format_on_save = settings.format_on_save;
        let auto_save = AutoSave::new(&settings, window, cx);
        let ui = UiTheme::default_dark();
        let welcome = launch.root.is_none() && launch.file.is_none();
        let root = launch.root.clone();
        let launch_root = launch.root.clone();
        let as_folder = launch.file.is_none();
        let launch_read = match &launch.file {
            Some((path, None)) => Some(path.clone()),
            _ => None,
        };
        let terminal = cx.new(|cx| TerminalPanel::new(terminal_dir(&launch), &theme, cx));
        let terminal_dock = cx.new(|cx| {
            // Shown by the sidebar's Terminal action and `terminal.toggle`,
            // so it draws no rail of its own.
            let state = DockState::new(DockPosition::Bottom)
                .without_rail()
                .shortcut_contexts([KEY_CONTEXT, fe_gpui::KEY_CONTEXT]);
            let mut dock = kiln_shell::Dock::with_state(state);
            dock.add_panel(terminal.clone(), cx);
            dock.dock_mut().set_visible(false);
            dock
        });
        let terminal_subscriptions = Self::follow_terminal(&terminal, &terminal_dock, window, cx);
        let panel = cx.new(|cx| ProjectPanel::new(launch.root, ui, cx));
        let problems = cx.weak_entity();
        let problems = cx.new(|cx| ProblemsPanel::new(problems, &theme, ui, cx));
        let pane = cx.new(|cx| {
            let mut pane = Pane::new(theme, settings, ui, cx);
            match launch.file {
                None if welcome => pane.add(EditorModel::new(Buffer::new(WELCOME), None), cx),
                None => {}
                Some((path, Some(Opened::Text(buffer)))) => pane.add(
                    EditorModel::new(loader::buffer_or_new(buffer), Some(path)),
                    cx,
                ),
                Some((_, Some(Opened::Large { view, index }))) => pane.add_viewer(view, index, cx),
                Some((path, None)) => pane.opening(path, cx),
            }
            pane
        });
        let outline = cx.new(|cx| OutlinePanel::new(ui, cx));
        let changes = cx.new(|cx| GitPanel::new(root.clone(), ui, cx));
        let references = cx.weak_entity();
        let references = cx.new(|cx| ReferencesPanel::new(references, ui, cx));
        let dock = cx.new(|cx| {
            let search = cx.new(|cx| SearchPanel::new(root.clone(), pane.downgrade(), ui, cx));
            Dock::new(
                panel.clone(),
                search,
                outline.clone(),
                changes.clone(),
                references,
                problems,
                cx,
            )
        });
        // Opens the terminal from the sidebar, whose button follows the
        // bottom dock (`follow_terminal`).
        dock.update(cx, |dock, cx| {
            dock.add_action(
                kiln_shell::RailAction {
                    id: TERMINAL_BUTTON,
                    icon: Icon::FileTerminal,
                    title: "Terminal".into(),
                    command: "terminal.toggle",
                },
                cx,
            )
        });
        let git = Self::new_git(root.clone(), &changes, cx);
        let quick_open = Self::new_quick_open(root, pane.downgrade(), ui, cx);
        // A rail click runs the op its panel's key would, or hides the dock.
        let rail = cx.subscribe_in(
            &dock,
            window,
            |workbench, dock, DockEvent::Rail(panel), window, cx| {
                let op = dock.read(cx).rail_op(*panel);
                workbench.run_dock_op(op, window, cx);
            },
        );
        let root_subscription = Self::follow_root(&panel, window, cx);
        let changes_subscription = Self::follow_changes(&changes, window, cx);
        let language_picker =
            cx.new(|cx| Picker::new(LanguagePicker::new(ui), "Select a language…", ui, cx));
        let config_problems = cx.weak_entity();
        let config_problems = cx.new(|cx| {
            let delegate = ConfigProblemsPicker::new(config_problems, ui);
            Picker::new(delegate, "Filter config problems…", ui, cx)
        });
        let go_to_line = cx.new(|cx| {
            let delegate = GoToLine::new();
            Picker::new(delegate, "Line or line:column…", ui, cx)
        });
        let toolbar_picker = cx.new(|cx| {
            let delegate = ToolbarPicker::new(outline.downgrade(), ui);
            Picker::new(delegate, "Filter…", ui, cx)
        });
        let recent = cx.weak_entity();
        let recent_picker = cx.new(|cx| {
            let delegate = RecentPicker::new(recent, ui);
            Picker::new(delegate, "Open recent folder or file…", ui, cx)
        });
        // A file's folder is only the root it opens in, not one opened.
        if let Some(root) = launch_root.clone().filter(|_| as_folder) {
            recent_picker::record(root, true, cx);
        }
        let nav_state = Nav::new(launch_root.clone(), ui, cx);
        let status_bar = cx.new(|_| StatusBar::new(ui));
        let toasts = cx.new(|cx| toast::ToastLayer::new(ui, window, cx));
        // A restored layout's panes are made like a pane of the workbench's.
        let template = Rc::new(RefCell::new(pane.downgrade()));
        let workspace = cx.new(|cx| {
            let template = template.clone();
            let new_pane = move |_: &mut Window, cx: &mut App| {
                let template = (template.borrow().upgrade()).expect("a pane to restore beside");
                template.update(cx, |pane, cx| cx.new(|cx| pane.new_peer(cx)))
            };
            Workspace::new(KEY_CONTEXT, pane.clone(), new_pane, window, cx).title("fastedit")
        });
        let shell = workspace.read(cx).shell().clone();
        shell.update(cx, |shell, cx| {
            // The chrome is the host's to turn on (`set_title_bar` and the
            // like); the text size is set around the workspace as it draws.
            shell.set_title_bar(false, cx);
            shell.set_menu_bar(None, cx);
            shell.set_window_frame(false, cx);
            shell.set_title(title_bar::name(launch_root.as_deref()), cx);
            shell.set_dock(DockPosition::Right, Some(dock.clone().into()), cx);
            shell.set_dock(DockPosition::Bottom, Some(terminal_dock.clone().into()), cx);
            shell.set_status_bar(Some(status_bar.clone().into()), cx);
        });
        let workspace_events = cx.subscribe_in(&workspace, window, Self::workspace_event);
        let workspace_changed = cx.observe_in(&workspace, window, |workbench, _, window, cx| {
            workbench.follow_workspace(window, cx)
        });
        let modals = shell.read(cx).modals().clone();
        let status_bar_changed = cx.observe(&status_bar, |workbench, _, cx| {
            workbench.show_status_bar(cx);
        });
        // `:w`, `:q` and the like, typed on the vim line.
        let vim_host_commands =
            cx.subscribe_in(&status_bar, window, |workbench, _, event, window, cx| {
                let command = event.command.clone();
                workbench.run_vim_host_command(&event.editor, command, window, cx);
            });
        // The layer hides a palette that's dismissed or left.
        let modals_changed = cx.subscribe(&modals, |workbench, _, event, cx| {
            workbench.palette = match *event {
                ModalEvent::Shown(id) => Palette::of(id),
                ModalEvent::Dismissed(id) if workbench.palette == Palette::of(id) => None,
                ModalEvent::Dismissed(_) => workbench.palette,
            };
            cx.notify();
        });
        let docks = [
            cx.observe(&dock, |workbench, _, cx| workbench.layout_keys_changed(cx)),
            cx.observe(&terminal_dock, |workbench, _, cx| {
                workbench.layout_keys_changed(cx)
            }),
        ];
        let layout_keys = Rc::new(LayoutKeys {
            sidebar: dock.clone(),
            terminal: terminal_dock.clone(),
            shell: shell.clone(),
            breadcrumbs: Cell::new(true),
            bounds: Cell::new(Some(window.window_bounds())),
        });
        workspace.update(cx, |workspace, _| {
            let keys = layout_keys.clone();
            workspace.on_save(move |cx| keys.save(cx));
            let keys = layout_keys.clone();
            workspace.on_restore(move |entry, _, cx| keys.restore(entry, cx));
        });
        let bounds = cx.observe_window_bounds(window, |workbench, window, _| {
            (workbench.layout_keys.bounds).set(Some(window.window_bounds()))
        });
        let mut workbench = Self {
            panel,
            dock,
            terminal_dock,
            terminal,
            _terminal: terminal_subscriptions,
            workspace,
            active: pane.clone(),
            template,
            _workspace: [workspace_events, workspace_changed],
            toolbar_requests: HashMap::new(),
            #[cfg(test)]
            moves: 0,
            #[cfg(test)]
            drops: HashMap::new(),
            editors: HashMap::new(),
            tree_opens: HashMap::new(),
            _release: cx.on_release(|workbench, cx| {
                let editors = workbench.editors.drain().map(|(_, (editor, _))| editor);
                DetachedSaves::keep(editors, cx);
            }),
            echoes: HashSet::new(),
            quick_open,
            language_picker,
            config_problems,
            _config_problems: Self::follow_config_problems(window, cx),
            go_to_line,
            recent_picker,
            shell,
            modals,
            palette: None,
            nav_state,
            status_bar,
            status_bar_visible: true,
            menu_bar: None,
            #[cfg(test)]
            client_tiling: None,
            root: launch_root,
            revealed: None,
            launch_read: launch_read.map(|path| (path, pane.downgrade())),
            prompting: false,
            trust: kiln_shell::trust::RootTrust::new(trust::prompt()),
            lsp: Lsp::new(),
            ui,
            root_subscription,
            changes_subscription,
            git,
            _rail: rail,
            _docks: docks,
            _modals: modals_changed,
            _status_bar: [status_bar_changed, vim_host_commands],
            toasts,
            toolbar_picker,
            breadcrumbs: true,
            nav: NavButtons::default(),
            chain_key: None,
            _outline: cx.observe(&outline, |workbench, _, cx| {
                if workbench.palette == Some(Palette::Toolbar) {
                    let picker = workbench.toolbar_picker.clone();
                    picker.update(cx, |picker, cx| picker.delegate.outline_changed(cx));
                }
                cx.notify()
            }),
            #[cfg(test)]
            chains_computed: 0,
            #[cfg(test)]
            nav_pushes: 0,
            #[cfg(test)]
            commands_run: Vec::new(),
            layout_id: None,
            restoring: false,
            layout_keys,
            _bounds: bounds,
            hot_exit: Default::default(),
            disk_sync: DiskSync::new(window, cx),
            auto_save,
            problem_landing: None,
            owned_editors: Vec::new(),
            diff_left: None,
            vim_keys: None,
            #[cfg(test)]
            git_routed: Vec::new(),
        };
        // Under test, the chrome's window moves are recorded, not made.
        #[cfg(test)]
        title_bar::record_requests(cx);
        workbench.pane_added(&pane, window, cx);
        workbench.track_editors(cx);
        workbench.follow_trust(window, cx);
        workbench.resolve_trust(workbench.root.clone(), as_folder, window, cx);
        workbench.lsp.format_on_save.set(format_on_save);
        workbench.start_lsp(window, cx);
        workbench.sync_config_problems(window, cx);
        workbench
    }

    /// The quick-open palette for `root`, opening files in `pane`.
    fn new_quick_open(
        root: Option<PathBuf>,
        pane: WeakEntity<Pane>,
        ui: UiTheme,
        cx: &mut Context<Self>,
    ) -> Entity<Picker<QuickOpen>> {
        cx.new(|cx| Picker::new(QuickOpen::new(root, pane, ui), "Go to file…", ui, cx))
    }

    /// The git state for `root`, which `changes` reads through.
    fn new_git(
        root: Option<PathBuf>,
        changes: &Entity<GitPanel>,
        cx: &mut Context<Self>,
    ) -> Entity<GitState> {
        let workbench = cx.weak_entity();
        let git = cx.new(|cx| GitState::new(root, changes, workbench, cx));
        changes.update(cx, |changes, _| changes.connect(git.downgrade()));
        git
    }

    /// Subscribes to a root's project panel. Its tree opening reads the git
    /// status, so the tree shows it without the Changes panel drawn.
    fn follow_root(
        panel: &Entity<ProjectPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(
            panel,
            window,
            |workbench, _, event, window, cx| match event {
                PanelEvent::OpenFile(path) => workbench.open_from_tree(path.clone(), window, cx),
                PanelEvent::FsChanged(changes) => {
                    workbench
                        .quick_open
                        .update(cx, |picker, cx| picker.delegate.fs_changed(changes, cx));
                    workbench.nav_state.fs_changed(changes, cx);
                    workbench
                        .git
                        .update(cx, |git, cx| git.fs_changed(changes, cx));
                    workbench.disk_fs_changed(changes, window, cx);
                }
                PanelEvent::Opened => workbench.git.update(cx, GitState::refresh_once),
                PanelEvent::Renamed { moves, .. } => workbench.panel_renamed(moves, window, cx),
            },
        )
    }

    /// Subscribes to the terminal: when its shell exits the dock hides and
    /// the active editor takes focus, as it does when the bottom dock's rail
    /// hides the terminal with focus in it.
    fn follow_terminal(
        terminal: &Entity<TerminalPanel>,
        dock: &Entity<kiln_shell::Dock>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> [Subscription; 3] {
        let exited = cx.subscribe_in(
            terminal,
            window,
            |workbench, _, TerminalPanelEvent::Exited, window, cx| {
                workbench.terminal_dock.update(cx, |dock, cx| {
                    dock.dock_mut().set_visible(false);
                    cx.notify();
                });
                window.focus(&workbench.focus_handle(cx), cx);
            },
        );
        let hid = cx.subscribe_in(
            dock,
            window,
            |workbench, _, kiln_shell::dock::DockEvent::HidFocus, window, cx| {
                window.focus(&workbench.focus_handle(cx), cx);
            },
        );
        let shown = cx.observe(dock, |workbench, dock, cx| {
            let shown = dock.read(cx).is_visible();
            workbench.dock.update(cx, |dock, cx| {
                dock.set_action_highlighted(TERMINAL_BUTTON, shown, cx)
            });
        });
        [exited, hid, shown]
    }

    /// `terminal.toggle`: shows the bottom dock with the terminal, focused,
    /// starting a session if none runs; focuses the terminal if the dock
    /// shows but focus is elsewhere; hides the dock and focuses the active
    /// editor if the terminal has focus.
    pub(crate) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.terminal_dock.read(cx).is_visible();
        if shown && self.terminal.read(cx).is_focused(window, cx) {
            self.terminal_dock.update(cx, |dock, cx| {
                dock.dock_mut().set_visible(false);
                cx.notify();
            });
            window.focus(&self.focus_handle(cx), cx);
            return;
        }
        self.terminal
            .update(cx, |terminal, cx| terminal.start(window, cx));
        self.terminal_dock
            .update(cx, |dock, cx| dock.show(TERMINAL, window, cx));
    }

    /// `terminal.new`: shows the bottom dock with a new session, focused.
    pub(crate) fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal
            .update(cx, |terminal, cx| terminal.add(window, cx));
        self.show_terminal_dock(window, cx);
    }

    /// `terminal.close`: drops the active session.
    pub(crate) fn close_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal
            .update(cx, |terminal, cx| terminal.close_active(window, cx));
    }

    /// `terminal.next` and `terminal.previous`: shows and focuses the next
    /// session, `by` 1, or the previous one, -1, if there's another.
    pub(crate) fn cycle_terminal(
        &mut self,
        by: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if (self.terminal).update(cx, |terminal, cx| terminal.cycle(by, window, cx)) {
            self.show_terminal_dock(window, cx);
        }
    }

    /// `terminal.find`: shows the dock with the active session's find row
    /// open, focused.
    pub(crate) fn find_in_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let terminal = self.terminal.clone();
        terminal.update(cx, |terminal, cx| terminal.start(window, cx));
        self.show_terminal_dock(window, cx);
        terminal.update(cx, |terminal, cx| {
            terminal.run_find(FindOp::Show, window, cx)
        });
    }

    /// `terminal.previous_prompt` and `terminal.next_prompt`: scrolls the
    /// active session to its shell's previous prompt or, if `forward`, its
    /// next.
    pub(crate) fn scroll_terminal_to_prompt(&mut self, forward: bool, cx: &mut Context<Self>) {
        self.terminal
            .update(cx, |terminal, cx| terminal.scroll_to_prompt(forward, cx));
    }

    /// `terminal.vi_mode`: shows the dock and turns the active session's vi
    /// copy mode on or off, focused.
    pub(crate) fn toggle_terminal_vi_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_terminal_dock(window, cx);
        self.terminal
            .update(cx, |terminal, cx| terminal.toggle_vi_mode(window, cx));
    }

    /// Shows the bottom dock with the terminal.
    fn show_terminal_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal_dock
            .update(cx, |dock, cx| dock.show(TERMINAL, window, cx));
    }

    /// Subscribes to a root's Changes panel: its rows open as the project
    /// panel's do, and the project panel shows the statuses it lists.
    fn follow_changes(
        changes: &Entity<GitPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(
            changes,
            window,
            |workbench, changes, event, window, cx| match event {
                GitPanelEvent::OpenFile(path) => workbench
                    .active
                    .update(cx, |pane, cx| pane.open(path.clone(), window, cx)),
                GitPanelEvent::Listed => {
                    let statuses: Vec<_> = (changes.read(cx).statuses())
                        .map(|(path, kind)| (path.to_owned(), kind))
                        .collect();
                    (workbench.panel).update(cx, |panel, cx| panel.set_git_statuses(statuses, cx));
                    workbench.tint_tabs(cx);
                }
            },
        )
    }

    /// Shows `picker` as `palette` in the modal layer, in place of any
    /// other. The layer hides it once it's picked from, dismissed or left.
    pub(crate) fn show_palette<D: crate::PickerDelegate>(
        &mut self,
        palette: Palette,
        picker: Entity<Picker<D>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.modals.update(cx, |layer, cx| {
            layer.show_modal(palette.id(), picker, window, cx)
        });
        self.palette = Some(palette);
        cx.notify();
    }

    /// Re-roots the workbench at `root`, in this window: a new project
    /// panel lists it in the background, and a new quick-open palette
    /// indexes it once opened. The old ones are dropped, and with them the
    /// old tree, its watcher, and the old index's walk; their batches land
    /// nowhere. A new Changes panel reads `root`'s repository, the old
    /// one's status landing nowhere, and a new git state gives the open
    /// editors their bases in `root`'s repository, the old one's refresh
    /// landing nowhere. Search searches `root` from now on,
    /// its running search cancelled and its results cleared, the query and
    /// options kept. The
    /// dock keeps its visibility and active panel, and open tabs, dirty
    /// ones included, stay open. Emits [`RootChanged`].
    ///
    /// The host's settings watcher keeps the launch root's
    /// `.fastedit/settings.toml`: a new root's project settings aren't
    /// loaded.
    pub fn set_root(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.palette,
            Some(Palette::Toolbar | Palette::Definitions | Palette::Symbols)
        ) {
            // Their paths are relative to the old root.
            self.dismiss_palette(window, cx);
        }
        if self.palette == Some(Palette::QuickOpen) {
            // Dismissed as Escape would be, as the palette goes.
            let picker = self.quick_open.clone();
            self.dismiss_palette(window, cx);
            if picker.focus_handle(cx).contains_focused(window, cx) {
                window.focus(&self.focus_handle(cx), cx);
            }
        }
        let panel_focused = self.panel.focus_handle(cx).contains_focused(window, cx);
        let changes_focused =
            (self.dock.read(cx).changes().focus_handle(cx)).contains_focused(window, cx);
        let ui = self.ui;
        let panel = cx.new(|cx| ProjectPanel::new(Some(root.clone()), ui, cx));
        let changes = cx.new(|cx| GitPanel::new(Some(root.clone()), ui, cx));
        let quick_open = Self::new_quick_open(Some(root.clone()), self.active.downgrade(), ui, cx);
        // The next session starts in the new root; one running stays.
        (self.terminal).update(cx, |terminal, _| terminal.set_cwd(root.clone()));
        self.root_subscription = Self::follow_root(&panel, window, cx);
        self.changes_subscription = Self::follow_changes(&changes, window, cx);
        self.git.update(cx, |git, _| git.close());
        self.git = Self::new_git(Some(root.clone()), &changes, cx);
        let search = self.dock.update(cx, |dock, cx| {
            dock.set_project(panel.clone(), cx);
            dock.set_changes(changes.clone(), cx);
            dock.search().clone()
        });
        search.update(cx, |search, cx| search.set_root(root.clone(), cx));
        let references = self.dock.read(cx).references().clone();
        references.update(cx, |references, cx| references.clear(cx));
        self.nav_state.set_root(root.clone(), cx);
        self.panel = panel;
        self.quick_open = quick_open;
        if panel_focused {
            window.focus(&self.panel.focus_handle(cx), cx);
        }
        if changes_focused {
            window.focus(&changes.focus_handle(cx), cx);
        }
        // So the new tree reveals the active file, if it's under the root.
        self.revealed = None;
        self.root = Some(root.clone());
        for pane in self.panes(cx) {
            let root = Some(root.clone());
            pane.update(cx, |pane, cx| {
                pane.update_toolbar(|toolbar| toolbar.root = root, cx)
            });
        }
        self.resolve_trust(Some(root.clone()), true, window, cx);
        self.track_editors(cx);
        recent_picker::record(root.clone(), true, cx);
        let name = title_bar::name(Some(&root));
        self.shell.update(cx, |shell, cx| shell.set_title(name, cx));
        cx.emit(RootChanged(root));
        cx.notify();
    }

    /// Shows the open panel for `op`, then opens what's picked: a folder
    /// re-roots the workbench, and a file opens in the active pane as a
    /// tree click does, the root staying. Cancelling does nothing, and an
    /// error from the panel is logged and shown in an error toast
    /// ([`open_panel_failed`]). Asked again while the panel shows, it does
    /// nothing.
    pub(crate) fn prompt_open(&mut self, op: OpenOp, window: &mut Window, cx: &mut Context<Self>) {
        if op == OpenOp::Settings {
            return self.open_settings(window, cx);
        }
        if op == OpenOp::NewWindow {
            return open::open_window(self.root.clone(), cx);
        }
        if op == OpenOp::NewFile {
            return self.new_file(window, cx);
        }
        if op == OpenOp::SaveAs {
            return self.prompt_save_as(window, cx);
        }
        if self.prompting {
            return;
        }
        self.prompting = true;
        let answer = cx.prompt_for_paths(op.prompt());
        #[cfg(test)]
        let answer = match OPEN_PANEL_FAILS.take() {
            Some(error) => {
                let (failed, answer) = futures::channel::oneshot::channel();
                failed.send(Err(std::io::Error::other(error).into())).ok();
                answer
            }
            None => answer,
        };
        cx.spawn_in(window, async move |workbench, cx| {
            let mut failed = None;
            let path = match answer.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => None,
                Ok(Err(error)) => {
                    log::warn!("the open panel failed: {error}");
                    failed = Some(error.to_string());
                    None
                }
                Err(_) => {
                    log::warn!("the open panel went without an answer");
                    None
                }
            };
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench.prompting = false;
                    if let Some(error) = failed {
                        let (message, detail) = open_panel_failed(&error);
                        toast::show(cx, Toast::error(message).detail(detail));
                    }
                    match (op, path) {
                        (_, None)
                        | (
                            OpenOp::Settings | OpenOp::NewWindow | OpenOp::NewFile | OpenOp::SaveAs,
                            Some(_),
                        ) => {}
                        (OpenOp::FolderInNewWindow, Some(path)) => {
                            open::open_window(Some(path), cx)
                        }
                        (OpenOp::Folder, Some(path)) => workbench.set_root(path, window, cx),
                        (OpenOp::File, Some(path)) => workbench
                            .active
                            .update(cx, |pane, cx| pane.open(path, window, cx)),
                    }
                })
                .ok();
        })
        .detach();
    }

    /// Opens an empty untitled tab in the active pane, focused.
    fn new_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = EditorModel::new(Buffer::new(""), None);
        self.active.update(cx, |pane, cx| {
            pane.add(model, cx);
            pane.activate(pane.tabs().len() - 1, window, cx);
        });
    }

    /// Shows the save panel for the active editor, in its file's folder
    /// (else the root, else home) with its file name (else `untitled`)
    /// suggested, then saves the editor where it answers
    /// ([`Workbench::save_editor_as`]). Cancelling changes nothing, and an
    /// error from the panel is shown as the open panel's is. Asked again
    /// while a panel shows, it does nothing.
    fn prompt_save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active.read(cx).active_editor().cloned() else {
            return;
        };
        if self.prompting {
            return;
        }
        self.prompting = true;
        let path = editor.read(cx).model().path().map(Path::to_owned);
        let dir = (path.as_deref().and_then(Path::parent).map(Path::to_owned))
            .or_else(|| self.root.clone())
            .or_else(std::env::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));
        let name = fe_gpui::pane_item::file_name(path.as_deref());
        let answer = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn_in(window, async move |workbench, cx| {
            let mut failed = None;
            let path = match answer.await {
                Ok(Ok(path)) => path,
                Ok(Err(error)) => {
                    log::warn!("the save panel failed: {error}");
                    failed = Some(error.to_string());
                    None
                }
                Err(_) => {
                    log::warn!("the save panel went without an answer");
                    None
                }
            };
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench.prompting = false;
                    // As the open panel's (see `prompt_open`).
                    if let Some(error) = failed {
                        let (message, detail) = open_panel_failed(&error);
                        toast::show(cx, Toast::error(message).detail(detail));
                    }
                    if let Some(path) = path {
                        workbench.save_editor_as(&editor, path, window, cx);
                    }
                })
                .ok();
        })
        .detach();
    }

    /// Saves `editor` to `path` ([`EditorView::save_as`]), the tabs and the
    /// editors over its buffer following it there once it's written, or
    /// shows why it couldn't be.
    ///
    /// A file another buffer might also save to is refused: one another
    /// tab shows, a pane is reading, in this window or another, or another
    /// Save As holds, by any path to it ([`open::file_identity`]). The destination is held from here
    /// until the outcome is taken in ([`open::reserve`]), so nothing else
    /// takes it meanwhile, even if the tab closes. The destination's
    /// `.editorconfig` line ending, read now, goes with the save: it's
    /// written with it, and the buffer takes it only if the save goes
    /// through.
    pub(crate) fn save_editor_as(
        &mut self,
        editor: &Entity<EditorView>,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save_editor_as_with(editor, path, true, window, cx);
    }

    /// [`Workbench::save_editor_as`], refusing a file that exists unless
    /// `overwrite` (a vim `:sav` without `!`): that is the vim message
    /// `E13`, not a prompt.
    pub(crate) fn save_editor_as_with(
        &mut self,
        editor: &Entity<EditorView>,
        path: PathBuf,
        overwrite: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = fe_gpui::pane_item::file_name(Some(&path));
        let identity = open::file_identity(&path);
        let same = |other: &Path| {
            other == path || identity.is_some() && open::file_identity(other) == identity
        };
        let model = editor.read(cx).model();
        let buffer = model.buffer().id();
        // This window, which is being updated, then every other one.
        let this_window = window.window_handle().window_id();
        let others = (cx.windows().into_iter())
            .filter(|other| other.window_id() != this_window)
            .filter_map(|other| other.downcast::<Workbench>())
            .filter_map(|other| other.read(cx).ok());
        let in_use = self.uses_file(&same, model, cx)
            || others
                .into_iter()
                .any(|other| other.uses_file(&same, model, cx));
        let held =
            open::reserved(&path, identity.as_deref()).is_some_and(|(holder, _)| holder != buffer);
        if in_use || held {
            drop(window.prompt(
                PromptLevel::Critical,
                &format!("“{name}” is open in another tab."),
                Some("Close it first, or save under another name."),
                &[PromptButton::ok("OK")],
                cx,
            ));
            return;
        }
        let reservation = open::reserve(buffer, path.clone(), identity);
        // Read here, so Save As requests keep their order.
        let config = fe_config::editorconfig_for(&path);
        let line_ending = crate::pane::config_line_ending(&config);
        let fixes = fe_gpui::SaveFixes {
            trim_trailing_whitespace: config.trim_trailing_whitespace == Some(true),
            insert_final_newline: config.insert_final_newline == Some(true),
        };
        let workbench = cx.weak_entity();
        let release = move |cx: &mut App| {
            (workbench.update(cx, |w, cx| w.lsp_release(buffer, cx))).ok();
        };
        let saved = editor.update(cx, |editor, cx| {
            editor.save_as_with(path, line_ending, fixes, overwrite, release, window, cx)
        });
        let editor = editor.clone();
        cx.spawn_in(window, async move |workbench, cx| {
            let result = saved.await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    if result.is_ok() {
                        workbench.editor_moved(&editor, cx);
                    }
                    drop(reservation);
                    let Err(error) = result else {
                        return;
                    };
                    let exists = (error.downcast_ref::<std::io::Error>())
                        .and_then(|error| error.get_ref())
                        .is_some_and(|inner| inner.is::<fe_editor::SaveError>());
                    if exists && !overwrite {
                        editor.update(cx, |editor, cx| editor.set_vim_message(vim_host::E13, cx));
                        return;
                    }
                    let detail = format!("{error:#}");
                    drop(window.prompt(
                        PromptLevel::Critical,
                        &format!("“{name}” couldn’t be saved."),
                        Some(&detail),
                        &[PromptButton::ok("OK")],
                        cx,
                    ));
                })
                .ok();
        })
        .detach();
    }

    /// Whether a file `same` matches is in use here other than by `model`'s
    /// buffer: shown in a tab (a read-only viewer's too), being read into
    /// one, or the launch file the host is reading.
    fn uses_file(&self, same: &dyn Fn(&Path) -> bool, model: &EditorModel, cx: &App) -> bool {
        let panes = self.pane_group(cx).panes();
        let shown = (panes.iter())
            .flat_map(|pane| pane.read(cx).tabs())
            .any(|tab| {
                let ours = (tab.editor())
                    .is_some_and(|other| other.read(cx).model().shares_buffer_with(model));
                !ours && tab.item().path(cx).is_some_and(|other| same(&other))
            });
        let reading = (panes.iter()).any(|pane| pane.read(cx).reading().any(same));
        shown || reading || self.launch_reading(cx).is_some_and(same)
    }

    /// After a Save As of `editor`'s buffer: the tabs take its path as it
    /// is now, and its other editors detect their language from it.
    fn editor_moved(&mut self, editor: &Entity<EditorView>, cx: &mut Context<Self>) {
        for other in self.open_editors(cx) {
            let shares = other != *editor
                && (other.read(cx).model()).shares_buffer_with(editor.read(cx).model());
            if shares {
                other.update(cx, |other, cx| other.detect_language(cx));
            }
        }
        for pane in self.panes(cx) {
            pane.update(cx, |pane, cx| pane.sync_paths(cx));
        }
        self.track_editors(cx);
        self.sync_lsp(cx);
        cx.notify();
    }

    /// Opens `path` from the project panel in the active pane. With vim on,
    /// its editor is in normal mode once it shows, whatever mode an editor
    /// of it already open was left in (a new one starts there): at once, or
    /// once an open waiting (behind a Save As of that file, say) lands, in
    /// each pane. A later open from the panel in the same pane replaces the
    /// one waiting there.
    fn open_from_tree(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.active.clone();
        pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx));
        self.tree_opens.remove(&pane.entity_id());
        if self.tree_opened(&pane, &path, window, cx) {
            return;
        }
        let waiting = cx.observe_in(&pane, window, {
            let path = path.clone();
            move |workbench, pane, window, cx| {
                let current = (workbench.tree_opens.get(&pane.entity_id()))
                    .is_some_and(|(waiting, _)| *waiting == path);
                if current && workbench.tree_opened(&pane, &path, window, cx) {
                    workbench.tree_opens.remove(&pane.entity_id());
                }
            }
        });
        self.tree_opens.insert(pane.entity_id(), (path, waiting));
    }

    /// Whether the open of `path` from the tree is over in `pane`: its
    /// editor shows, which is put in vim's normal mode, or `path` is no
    /// longer being read (it failed, or another file was opened instead).
    fn tree_opened(
        &mut self,
        pane: &Entity<Pane>,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let pane = pane.read(cx);
        let editor = (pane.active_editor().cloned())
            .filter(|editor| editor.read(cx).model().path() == Some(path));
        let Some(editor) = editor else {
            return !pane.reading().any(|reading| reading == path);
        };
        let normal = |editor: &Entity<EditorView>, cx: &App| {
            (editor.read(cx).vim_mode()).is_none_or(|mode| mode == "normal")
        };
        if normal(&editor, cx) {
            return true;
        }
        // Outside this update: the key runs the editor's own.
        window.defer(cx, move |window, cx| {
            window.focus(&editor.focus_handle(cx), cx);
            // One escape leaves insert, a visual mode or a pending command;
            // a second, should the first only drop a count.
            for _ in 0..2 {
                if normal(&editor, cx) {
                    break;
                }
                window.dispatch_keystroke(gpui::Keystroke::parse("escape").unwrap(), cx);
            }
        });
        true
    }

    /// Why a rename of the file or folder at `path` must wait, if it must:
    /// an editor saving to it or under it, by its path or the file a save
    /// writes through a symlink ([`fe_editor::save_target`]), whether its
    /// tab and window are open or closed since ([`DetachedSaves`]); a Save
    /// As holding it as its destination; a pane reading it, in `window`'s
    /// workbench or any other window's; or the project panel creating,
    /// duplicating or trashing where it overlaps, from any window, open or
    /// closed since ([`project_panel::running`]). Paths are matched by the
    /// folders they're in, resolved ([`under`]), so another spelling of the
    /// root matches too.
    pub(crate) fn busy_under(path: &Path, window: &Window, cx: &App) -> Option<Busy> {
        let from = resolved(path);
        let under = |other: &Path| under(other, path, &from);
        if open::reserved_paths().iter().any(|held| under(held)) {
            return Some(Busy::Saving);
        }
        let saving_under = |editor: &Entity<EditorView>| {
            let editor = editor.read(cx);
            editor.is_saving()
                && editor.model().path().is_some_and(|saved| {
                    under(saved) || save_target(saved).is_ok_and(|target| under(&target))
                })
        };
        let detached = cx.try_global::<DetachedSaves>();
        if detached.is_some_and(|saves| saves.0.iter().any(saving_under)) {
            return Some(Busy::Saving);
        }
        let this_window = window.window_handle().window_id();
        let this = window.root::<Workbench>().flatten();
        let others = (cx.windows().into_iter())
            .filter(|other| other.window_id() != this_window)
            .filter_map(|other| other.downcast::<Workbench>())
            .filter_map(|other| other.entity(cx).ok());
        for workbench in this.into_iter().chain(others) {
            let workbench = workbench.read(cx);
            if workbench.open_editors(cx).iter().any(saving_under) {
                return Some(Busy::Saving);
            }
            let panes = workbench.pane_group(cx).panes();
            let reading = (panes.iter()).any(|pane| pane.read(cx).reading().any(under));
            if reading || workbench.launch_reading(cx).is_some_and(under) {
                return Some(Busy::Opening);
            }
        }
        if project_panel::running()
            .iter()
            .any(|running| overlap(running, path))
        {
            return Some(Busy::Working);
        }
        None
    }

    /// Where every editor of `from`, or of a file under it, in `window`'s
    /// workbench and every other window's, will be once `from` is renamed
    /// `to` ([`moved_to`]). Taken before the rename, while every spelling
    /// of `from` still resolves: a symlink into a renamed folder dangles
    /// after it.
    pub(crate) fn plan_rename(from: &Path, to: &Path, window: &Window, cx: &App) -> Moves {
        let this_window = window.window_handle().window_id();
        let this = window.root::<Workbench>().flatten();
        let others = (cx.windows().into_iter())
            .filter(|other| other.window_id() != this_window)
            .filter_map(|other| other.downcast::<Workbench>())
            .filter_map(|other| other.entity(cx).ok());
        (this.into_iter().chain(others))
            .flat_map(|workbench| workbench.read(cx).moves(from, to, cx))
            .collect()
    }

    /// [`Workbench::plan_rename`]'s moves of this workbench's editors.
    fn moves(&self, from: &Path, to: &Path, cx: &App) -> Moves {
        let (from_resolved, to_resolved) = (resolved(from), resolved(to));
        (self.open_editors(cx).into_iter())
            .filter_map(|editor| {
                let path = editor.read(cx).model().path()?;
                let path = moved_to(path, from, to, &from_resolved, &to_resolved)?;
                Some((editor, path))
            })
            .collect()
    }

    /// After the project panel renamed a file or folder: the editors of it,
    /// or of the files in it, take the paths `moves` planned for them
    /// ([`Workbench::plan_rename`]), in this window and every other
    /// ([`Workbench::follow_moves`]).
    fn panel_renamed(&mut self, moves: &Moves, window: &Window, cx: &mut Context<Self>) {
        self.follow_moves(moves, cx);
        // This window, which is being updated, then every other one.
        let this_window = window.window_handle().window_id();
        let others = (cx.windows().into_iter())
            .filter(|other| other.window_id() != this_window)
            .filter_map(|other| other.downcast::<Workbench>());
        for other in others {
            (other.update(cx, |other, _, cx| other.follow_moves(moves, cx))).ok();
        }
    }

    /// Has the editors of `from`, or of files under it, follow its rename to
    /// `to`, as [`Workbench::follow_moves`] does, their paths resolved now:
    /// for tests that rename behind the panel's back.
    #[cfg(test)]
    pub(crate) fn follow_rename(&mut self, from: &Path, to: &Path, cx: &mut Context<Self>) {
        let moves = self.moves(from, to, cx);
        self.follow_moves(&moves, cx);
    }

    /// Has each of this workbench's editors in `moves` save to its new path
    /// from now on ([`EditorModel::move_path`]: the stamp it knows moves
    /// too, so a save still refuses the file if it changed), once per
    /// buffer, as the path set through one editor is every split's: its
    /// tabs, title and breadcrumbs follow, it detects its language again,
    /// and its language server document is closed and opened again under
    /// the new path. Its text, vim mode and selections stay as they are.
    pub(crate) fn follow_moves(&mut self, moves: &Moves, cx: &mut Context<Self>) {
        let open: HashSet<EntityId> = (self.open_editors(cx).iter())
            .map(Entity::entity_id)
            .collect();
        let moved: Vec<_> = (moves.iter())
            .filter(|(editor, _)| open.contains(&editor.entity_id()))
            .collect();
        if moved.is_empty() {
            return;
        }
        let mut buffers = HashSet::new();
        for (editor, path) in &moved {
            let buffer = editor.read(cx).model().buffer().id();
            if buffers.insert(buffer) {
                self.lsp_release(buffer, cx);
                let path = path.clone();
                editor.update(cx, |editor, _| editor.model_mut().move_path(path));
            }
        }
        for (editor, _) in &moved {
            editor.update(cx, |editor, cx| editor.detect_language(cx));
        }
        for pane in self.panes(cx) {
            pane.update(cx, |pane, cx| pane.sync_paths(cx));
        }
        self.track_editors(cx);
        self.sync_lsp(cx);
        cx.notify();
    }

    /// Opens the user's `settings.toml` in the active pane, creating it from
    /// the template first, in the background, if it's missing.
    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = open::settings_path(cx) else {
            log::error!("can't open settings: there's no config directory");
            toast::show(
                cx,
                Toast::error("Can't open settings").detail("There's no config directory"),
            );
            return;
        };
        let created = cx.background_spawn({
            let path = path.clone();
            async move { open::create_settings(&path) }
        });
        cx.spawn_in(window, async move |workbench, cx| {
            if let Err(error) = created.await {
                log::error!("can't create {}: {error}", path.display());
                cx.update(|_, cx| {
                    toast::show(
                        cx,
                        Toast::error(format!("Can't create {}", path.display()))
                            .detail(error.to_string()),
                    )
                })
                .ok();
                return;
            }
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench
                        .active
                        .update(cx, |pane, cx| pane.open(path, window, cx))
                })
                .ok();
        })
        .detach();
    }

    /// The folder the workbench is rooted at, if any.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn panel(&self) -> &Entity<ProjectPanel> {
        &self.panel
    }

    /// The terminal's dock, at the bottom unless moved.
    pub fn terminal_dock(&self) -> &Entity<kiln_shell::Dock> {
        &self.terminal_dock
    }

    pub fn terminal(&self) -> &Entity<TerminalPanel> {
        &self.terminal
    }

    /// The sidebar's dock, on the right unless moved, which shows the
    /// project panel, search, the outline and the rest.
    pub fn dock(&self) -> &Entity<Dock> {
        &self.dock
    }

    /// The active pane, which files open in.
    pub fn pane(&self) -> &Entity<Pane> {
        &self.active
    }

    /// Every pane, as laid out: the workspace's group.
    pub fn pane_group<'a>(&self, cx: &'a App) -> &'a PaneGroup {
        self.workspace.read(cx).group()
    }

    /// Every pane, as laid out, to update in turn.
    pub(crate) fn panes(&self, cx: &App) -> Vec<Entity<Pane>> {
        self.pane_group(cx).panes().into_iter().cloned().collect()
    }

    /// The window's workspace, around the panes.
    pub fn workspace(&self) -> &Entity<Workspace<Pane>> {
        &self.workspace
    }

    /// A pane joined the workspace, the first or split off since: its
    /// toolbar shows as the others' do, and its requests are heard.
    fn pane_added(&mut self, pane: &Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) {
        pane.update(cx, |pane, cx| {
            pane.update_toolbar(
                |toolbar| {
                    toolbar.visible = self.breadcrumbs;
                    toolbar.nav = self.nav;
                    toolbar.root = self.root.clone();
                },
                cx,
            );
        });
        let requested = cx.subscribe_in(
            pane,
            window,
            |workbench, pane, request: &ToolbarRequest, window, cx| {
                workbench.open_toolbar_picker(pane, request.clone(), window, cx)
            },
        );
        self.toolbar_requests.insert(pane.entity_id(), requested);
        #[cfg(test)]
        {
            let dropped = cx
                .subscribe(pane, |workbench, _, _: &crate::pane_drag::TabDropped, _| {
                    workbench.moves += 1
                });
            self.drops.insert(pane.entity_id(), dropped);
        }
    }

    fn workspace_event(
        &mut self,
        _: &Entity<Workspace<Pane>>,
        event: &WorkspaceEvent<Pane>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            WorkspaceEvent::PaneAdded(pane) => self.pane_added(pane, window, cx),
            WorkspaceEvent::PaneRemoved(id) => {
                self.toolbar_requests.remove(id);
                #[cfg(test)]
                self.drops.remove(id);
            }
            // The workspace notifies as it changes, so the workbench follows
            // it then ([`Workbench::follow_workspace`]).
            WorkspaceEvent::ActiveChanged => {}
        }
    }

    /// The workspace changed: a tab opened, closed or changed, a pane came
    /// or went, or another is active, by a click, a close or the workspace
    /// activating one itself. The workbench follows its active pane and its
    /// editors, and once a restored layout's files are in, takes up their
    /// editors; a window with no root that's left with nothing open shows
    /// the welcome text, as a launch with nothing to open does.
    fn follow_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if *self.workspace.read(cx).active_pane() != self.active {
            // The workspace made it active itself (a click on it, a close,
            // a file read into it): focus goes where the workbench's own
            // activation puts it ([`Pane::focus_target`]).
            self.follow_active_pane(cx);
            let focus = self.active.read(cx).focus_target(cx);
            window.focus(&focus, cx);
        }
        if self.restoring && !self.workspace.read(cx).restoring() {
            self.restoring = false;
            let active = self.workspace.read(cx).active_pane().clone();
            self.activate(active, window, cx);
            self.adopt_orphans(cx);
            let panes = self.pane_group(cx).panes();
            if self.root.is_none() && panes.iter().all(|pane| pane.read(cx).tabs().is_empty()) {
                let welcome = EditorModel::new(Buffer::new(WELCOME), None);
                self.active.update(cx, |pane, cx| pane.add(welcome, cx));
            }
        }
        self.track_editors(cx);
        cx.notify();
    }

    /// Takes the workspace's active pane as the one files open in, quick
    /// open's and search's.
    fn follow_active_pane(&mut self, cx: &mut Context<Self>) {
        let pane = self.workspace.read(cx).active_pane().clone();
        let weak = pane.downgrade();
        self.quick_open
            .update(cx, |picker, _| picker.delegate.set_pane(weak.clone()));
        let search = self.dock.read(cx).search().clone();
        search.update(cx, |search, _| search.set_pane(weak));
        self.active = pane;
    }

    /// Makes `pane` the active pane, which files open in, and focuses it.
    pub(crate) fn activate(
        &mut self,
        pane: Entity<Pane>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        (self.workspace).update(cx, |workspace, cx| workspace.activate(pane, window, cx));
        self.follow_active_pane(cx);
        let focus = self.active.read(cx).focus_target(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Activates the pane in `direction` of the active one, if there's one.
    pub(crate) fn focus_pane(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = self.pane_group(cx).neighbour(&self.active, direction, cx) {
            self.activate(pane, window, cx);
        }
    }

    /// Activates the next (or previous) pane, wrapping.
    pub(crate) fn focus_next_pane(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = self.pane_group(cx).next(&self.active, forward) {
            self.activate(pane, window, cx);
        }
    }

    /// Grows or shrinks the active pane by [`RESIZE_STEP`] along `axis`.
    pub(crate) fn resize_active_pane(&mut self, axis: Axis, grow: bool, cx: &mut Context<Self>) {
        let delta = if grow { RESIZE_STEP } else { -RESIZE_STEP };
        let active = self.active.clone();
        self.workspace.update(cx, |workspace, cx| {
            if workspace.group_mut().resize(&active, axis, delta) {
                cx.notify();
            }
        });
    }

    /// Evens out every axis's panes.
    pub(crate) fn equalize_panes(&mut self, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.group_mut().equalize();
            cx.notify();
        });
    }

    /// `dock.grow` or `dock.shrink`: resizes the terminal's dock by `by`
    /// while the terminal has focus, else the sidebar.
    pub(crate) fn resize_focused_dock(
        &mut self,
        by: gpui::Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal.read(cx).is_focused(window, cx) {
            self.terminal_dock.update(cx, |dock, cx| {
                dock_layout::resize(dock.dock_mut(), by, window, cx);
                cx.notify();
            });
        } else {
            self.dock.update(cx, |dock, cx| {
                dock_layout::resize(dock.dock_mut(), by, window, cx);
                cx.notify();
            });
        }
    }

    /// The sidebar's position and the terminal dock's.
    fn dock_positions(&self, cx: &App) -> (DockPosition, DockPosition) {
        let sidebar = self.dock.read(cx).state().position();
        (sidebar, self.terminal_dock.read(cx).dock().position())
    }

    /// Moves the terminal's dock to `to`, keeping its visibility, its
    /// sessions and focus, the sidebar giving way if there
    /// ([`dock_layout::displaced`]).
    pub(crate) fn move_terminal(&mut self, to: DockPosition, cx: &mut Context<Self>) {
        let (sidebar, terminal) = self.dock_positions(cx);
        let sidebar = dock_layout::displaced(terminal, to, sidebar);
        self.place_docks(sidebar, to, cx);
    }

    /// Moves the sidebar to the side `to`, the terminal's dock giving way
    /// if there ([`dock_layout::displaced`]).
    pub(crate) fn move_sidebar(&mut self, to: DockPosition, cx: &mut Context<Self>) {
        let (sidebar, terminal) = self.dock_positions(cx);
        let terminal = dock_layout::displaced(sidebar, to, terminal);
        self.place_docks(to, terminal, cx);
    }

    /// Puts the sidebar at `sidebar` and the terminal's dock at `terminal`
    /// ([`place_docks`]).
    fn place_docks(
        &mut self,
        sidebar: DockPosition,
        terminal: DockPosition,
        cx: &mut Context<Self>,
    ) {
        let docks = (&self.dock, &self.terminal_dock, &self.shell);
        place_docks(docks, sidebar, terminal, cx);
    }

    /// Opens a pane in `direction` of the active one and activates it. If
    /// the active pane shows a tab, the new pane gets one for the same file,
    /// sharing its buffer.
    pub(crate) fn split(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new =
            (self.workspace).update(cx, |workspace, cx| workspace.split(direction, window, cx));
        self.activate(new, window, cx);
        self.track_editors(cx);
    }

    /// Moves the active tab to the pane in `direction`, or to a pane split
    /// that way if there's none ([`Workspace::move_tab`]), then activates
    /// the pane it went to; the workspace closes the active pane if that
    /// emptied it.
    pub(crate) fn move_tab(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = self.active.clone();
        let pane = source.read(cx);
        let Some(ix) = pane.active() else {
            return;
        };
        if pane.active_item().is_none() {
            // A file being read, or a read error, shows in the tab's place.
            return;
        }
        let tab = pane.tabs()[ix].id();
        let destination = match self.pane_group(cx).neighbour(&source, direction, cx) {
            Some(pane) => Destination::Pane { pane, index: None },
            None => Destination::Split {
                pane: source.clone(),
                direction,
            },
        };
        #[cfg(test)]
        {
            self.moves += 1;
        }
        let target = self.workspace.update(cx, |workspace, cx| {
            workspace.move_tab(&source, tab, destination, window, cx)
        });
        if let Some(target) = target {
            self.activate(target, window, cx);
        }
        self.track_editors(cx);
    }

    /// Closes the active pane with its tabs, unless it's the last, once its
    /// tabs agree ([`kiln_shell::close`]): each is asked unless another
    /// pane shows its file, and a dirty one asks whether to discard its
    /// changes.
    pub(crate) fn close_active_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pane_group(cx).panes().len() < 2 {
            return;
        }
        let pane = self.active.clone();
        let now = Items {
            closing: pane.read(cx).pane().items().to_vec(),
            open: self.pane_group(cx).items(cx),
        };
        let (weak, workbench) = (pane.downgrade(), cx.weak_entity());
        let live: LiveItems = Rc::new(move |cx| {
            let pane = weak.upgrade()?;
            let workbench = workbench.upgrade()?;
            Some(Items {
                closing: pane.read(cx).pane().items().to_vec(),
                open: workbench.read(cx).pane_group(cx).items(cx),
            })
        });
        let request =
            CloseRequest::new(CloseKind::Pane).scope(window.window_handle(), Some(now), live);
        let (closed, workspace) = (pane.clone(), self.workspace.downgrade());
        let commit = move |window: Option<&mut Window>, cx: &mut App| {
            if let Some(window) = window {
                (workspace.update(cx, |workspace, cx| {
                    workspace.close_pane(&closed, window, cx)
                }))
                .ok();
            }
        };
        match close::confirm(request, commit, Some(window), cx) {
            Closing::Now => {
                (self.workspace).update(cx, |workspace, cx| workspace.close_pane(&pane, window, cx))
            }
            Closing::Refused => {}
            Closing::Later(closing) => closing.detach(),
        }
    }

    /// Observes each open editor, and stops observing closed ones. A newly
    /// observed editor gets its diff base, and its file, if it has one, is
    /// recorded as opened recently. The problems are listed again.
    fn track_editors(&mut self, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        let open: HashSet<EntityId> = editors.iter().map(Entity::entity_id).collect();
        let closed: Vec<_> = (self.editors)
            .extract_if(|id, _| !open.contains(id))
            .map(|(_, (editor, _))| editor)
            .collect();
        DetachedSaves::keep(closed.into_iter(), cx);
        self.echoes.retain(|id| open.contains(id));
        for editor in &editors {
            if self.editors.contains_key(&editor.entity_id()) {
                continue;
            }
            let observed = cx.observe(editor, Self::editor_changed);
            (self.editors).insert(editor.entity_id(), (editor.clone(), observed));
            if let Some(path) = editor.read(cx).model().path().map(Path::to_owned) {
                recent_picker::record(path, false, cx);
            }
        }
        self.git.update(cx, |git, cx| git.track(&editors, cx));
        self.tint_tabs(cx);
        self.sync_problems(cx);
        self.sync_backups(cx);
        self.sync_auto_save(cx);
    }

    /// Tints each open editor's tab title in its file's status colour from
    /// the Changes panel's list, and clears the tint of a file not in it.
    fn tint_tabs(&mut self, cx: &mut Context<Self>) {
        let changes = self.dock.read(cx).changes().clone();
        let tints: HashMap<PathBuf, ChangeKind> = (changes.read(cx).changed_paths())
            .map(|(path, kind)| (resolved(path), kind))
            .collect();
        let ui = self.ui;
        for editor in self.open_editors(cx) {
            let tint = editor.read(cx).model().path().and_then(|path| {
                let kind = tints.get(&resolved(path))?;
                Some(git_panel::letter_color(*kind, &ui))
            });
            editor.update(cx, |editor, cx| editor.set_tab_tint(tint, cx));
        }
    }

    #[cfg(test)]
    pub(crate) fn git(&self) -> &Entity<GitState> {
        &self.git
    }

    /// The window's toasts.
    #[cfg(test)]
    pub(crate) fn toasts(&self) -> &Entity<toast::ToastLayer> {
        &self.toasts
    }

    pub(crate) fn open_editors(&self, cx: &App) -> Vec<Entity<EditorView>> {
        self.pane_group(cx)
            .panes()
            .into_iter()
            .flat_map(|pane| {
                (pane.read(cx).tabs().iter())
                    .filter_map(|tab| tab.editor().cloned())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Lists the problems again, and notifies every other editor sharing
    /// `editor`'s buffer, so an edit in one pane shows in the others on the
    /// next frame. Each keeps its own selections, scroll and folds.
    fn editor_changed(&mut self, editor: Entity<EditorView>, cx: &mut Context<Self>) {
        // A diagnostic arriving or clearing notifies, as an edit does.
        self.sync_problems(cx);
        self.sync_backups(cx);
        self.sync_auto_save(cx);
        if self.echoes.remove(&editor.entity_id()) {
            return;
        }
        let model = editor.read(cx).model();
        let sharing: Vec<_> = self
            .open_editors(cx)
            .into_iter()
            .filter(|other| *other != editor && other.read(cx).model().shares_buffer_with(model))
            .collect();
        for other in sharing {
            // Only an observed editor's notification consumes its echo.
            if self.editors.contains_key(&other.entity_id()) {
                self.echoes.insert(other.entity_id());
            }
            // Caught up now, so it reparses for the edit as it renders.
            other.update(cx, |other, cx| {
                other.model_mut().sync_display();
                cx.notify();
            });
        }
    }

    /// Saves the window's layout from now on, in `layout`, through the
    /// workspace ([`Workspace::set_layout`]): as its shape changes (a
    /// split, a close, a move, a dock toggled, moved or resized, a tab
    /// opening or closing, a divider's drag), in full as a buffer's backup
    /// is written or removed, and as the app quits, once its buffers are
    /// backed up. As the window closes, its entry is kept if it's the last
    /// window, else deleted ([`kiln_shell::layout`]). Its unsaved buffers
    /// are backed up beside it from now on, for hot exit ([`hot_exit`]).
    pub fn set_layout(&mut self, layout: WindowLayout, window: &Window, cx: &mut Context<Self>) {
        // First, so the backups are written before the workspace saves the
        // layout at quit.
        self.start_hot_exit(layout.store(), window, cx);
        self.layout_id = layout.id();
        (self.workspace).update(cx, |workspace, cx| workspace.set_layout(layout, window, cx));
    }

    /// Saves the layout as the app quits, once every buffer's backup is
    /// written, and resolves once it's written: what quitting does, through
    /// the hooks of hot exit and the workspace.
    #[cfg(test)]
    pub(crate) fn save_layout_at_quit(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl Future<Output = ()> + use<> {
        self.flush_backups(cx);
        (self.workspace).update(cx, |workspace, cx| workspace.save_at_quit(cx))
    }

    /// Has the workspace save the layout if its shape changed: fastedit's
    /// keys changed ([`LayoutKeys`]).
    fn layout_keys_changed(&mut self, cx: &mut Context<Self>) {
        self.workspace.update(cx, |_, cx| cx.notify());
    }

    /// Has the workspace save the layout now, in full: a buffer's backup
    /// was written or removed, which the entry's shape doesn't show.
    pub(crate) fn save_layout_in_full(&mut self, cx: &mut Context<Self>) {
        self.workspace
            .update(cx, |workspace, cx| workspace.save(cx));
    }

    /// The window's layout id, once it has a layout.
    pub fn layout_id(&self) -> Option<u64> {
        self.layout_id
    }

    /// Whether a restored layout's files are still being read.
    pub fn restoring(&self) -> bool {
        self.restoring
    }

    /// The window's layout, as saved ([`Workspace::entry`]): its panes and
    /// their files, and fastedit's keys ([`LayoutKeys`]).
    pub fn layout_entry(&self, cx: &App) -> Value {
        self.workspace.read(cx).entry(cx)
    }

    /// Puts the window back as `entry` saved it, through the workspace
    /// ([`Workspace::restore`]): the panes, split as they were, with their
    /// files read in the background, the views of one file sharing its
    /// buffer, each with its caret, and fastedit's keys ([`LayoutKeys`])
    /// at once. A file that's gone is left out, and a pane left empty
    /// closes, unless it's the only one. An entry without panes puts back
    /// fastedit's keys alone.
    pub fn restore_layout(&mut self, entry: &Value, window: &mut Window, cx: &mut Context<Self>) {
        // The workspace restores the panes, made like the active one; once
        // their files are in, the workbench follows it
        // ([`Workbench::follow_workspace`]).
        *self.template.borrow_mut() = self.active.downgrade();
        let restored =
            (self.workspace).update(cx, |workspace, cx| workspace.restore(entry, window, cx));
        if !restored {
            // Without panes, the workspace hands back none of fastedit's
            // keys, but an entry saved before it kept both the terminal
            // dock's sizes still puts them back.
            self.layout_keys.restore(entry, cx);
        }
        if self.layout_keys.breadcrumbs.get() != self.breadcrumbs {
            self.toggle_breadcrumbs(cx);
        }
        if !restored {
            return;
        }
        let active = self.workspace.read(cx).active_pane().clone();
        self.activate(active, window, cx);
        self.restoring = true;
        self.track_editors(cx);
        cx.notify();
    }

    /// The quick-open palette, shown while [`Workbench::palette_open`].
    pub fn quick_open(&self) -> &Entity<Picker<QuickOpen>> {
        &self.quick_open
    }

    /// Whether the quick-open palette shows.
    pub fn palette_open(&self) -> bool {
        self.palette == Some(Palette::QuickOpen)
    }

    /// Shows the quick-open palette, focused, in place of any other, or
    /// dismisses it.
    pub(crate) fn toggle_quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let show = !self.palette_open();
        if show {
            self.dismiss_palette(window, cx);
        }
        let previous = window.focused(cx);
        let picker = self.quick_open.clone();
        if show {
            picker.update(cx, |picker, cx| {
                picker.delegate.opened(previous, cx);
                picker.show(window, cx);
            });
            self.show_palette(Palette::QuickOpen, picker, window, cx);
        } else {
            self.dismiss_palette(window, cx);
        }
        cx.notify();
    }

    /// The language picker, shown by `language.select`.
    #[cfg(test)]
    pub(crate) fn language_picker(&self) -> &Entity<Picker<LanguagePicker>> {
        &self.language_picker
    }

    /// Whether the language picker shows.
    #[cfg(test)]
    pub(crate) fn language_picker_open(&self) -> bool {
        self.palette == Some(Palette::Language)
    }

    /// Shows the language picker, focused, for the active editor, in place
    /// of any other palette, or afresh if it shows. Nothing without an
    /// active editor.
    pub(crate) fn select_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        // Dismissing it would hide it again once its dismissal lands.
        if self.palette != Some(Palette::Language) {
            self.dismiss_palette(window, cx);
        }
        let picker = self.language_picker.clone();
        picker.update(cx, |picker, cx| {
            picker.delegate.opened(&editor, cx);
            picker.show(window, cx);
        });
        self.show_palette(Palette::Language, picker, window, cx);
    }

    /// The go-to-line picker, shown by `go_to_line.toggle`.
    #[cfg(test)]
    pub(crate) fn go_to_line_picker(&self) -> &Entity<Picker<GoToLine>> {
        &self.go_to_line
    }

    /// Whether the go-to-line picker shows.
    #[cfg(test)]
    pub(crate) fn go_to_line_open(&self) -> bool {
        self.palette == Some(Palette::GoToLine)
    }

    /// Shows the go-to-line picker, focused, for the active editor, in
    /// place of any other palette, or dismisses it if it shows. Nothing
    /// without an active editor.
    pub(crate) fn toggle_go_to_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette == Some(Palette::GoToLine) {
            return self.dismiss_palette(window, cx);
        }
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        self.dismiss_palette(window, cx);
        let workbench = cx.entity().downgrade();
        let picker = self.go_to_line.clone();
        picker.update(cx, |picker, cx| {
            picker.delegate.opened(&editor, workbench, cx);
            picker.show(window, cx);
        });
        self.show_palette(Palette::GoToLine, picker, window, cx);
    }

    /// The recent folders and files picker, shown by `workbench.open_recent`.
    #[cfg(test)]
    pub(crate) fn recent_picker(&self) -> &Entity<Picker<RecentPicker>> {
        &self.recent_picker
    }

    /// Does a recent command: shows the picker, focused, in place of any
    /// other palette, or afresh if it shows; empties the list; or, while
    /// the picker shows, opens its selected folder in a new window, or its
    /// selected file as confirming it would.
    pub(crate) fn run_recent_op(
        &mut self,
        op: RecentOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picker = self.recent_picker.clone();
        match op {
            RecentOp::Open => {
                // Dismissing it would hide it again once its dismissal lands.
                if self.palette != Some(Palette::Recent) {
                    self.dismiss_palette(window, cx);
                }
                let input = picker.focus_handle(cx);
                let previous = window.focused(cx).filter(|focused| *focused != input);
                picker.update(cx, |picker, cx| {
                    picker.delegate.opened(previous, cx);
                    picker.show(window, cx);
                });
                self.show_palette(Palette::Recent, picker, window, cx);
            }
            RecentOp::Clear => {
                recent_picker::clear(cx);
                picker.update(cx, |picker, cx| picker.delegate.cleared(cx));
            }
            RecentOp::OpenInNewWindow if self.palette == Some(Palette::Recent) => {
                // After this update: confirming a file reads the workbench.
                window.defer(cx, move |window, cx| {
                    picker.update(cx, |picker, cx| {
                        let selected = picker.selected();
                        picker.delegate.open_in_new_window(selected, window, cx)
                    })
                });
            }
            RecentOp::OpenInNewWindow => cx.propagate(),
        }
    }

    /// Dismisses the palette that shows, as Escape would.
    pub(crate) fn dismiss_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            (self.modals.clone()).update(cx, |layer, cx| layer.dismiss_modal(window, cx));
        }
    }

    /// Shows the toolbar picker, focused, for `request` from `pane`, in
    /// place of any other palette, or afresh if it shows.
    fn open_toolbar_picker(
        &mut self,
        pane: &Entity<Pane>,
        request: ToolbarRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette != Some(Palette::Toolbar) {
            self.dismiss_palette(window, cx);
        }
        let input = self.toolbar_picker.focus_handle(cx);
        let previous = window.focused(cx).filter(|focused| *focused != input);
        let (pane, root) = (pane.downgrade(), self.root.clone());
        let workbench = cx.entity().downgrade();
        let picker = self.toolbar_picker.clone();
        picker.update(cx, |picker, cx| {
            picker
                .delegate
                .open(request, pane, workbench, root, previous, cx);
            picker.show(window, cx);
        });
        self.show_palette(Palette::Toolbar, picker, window, cx);
    }

    /// The toolbar picker, shown by a crumb or "…".
    #[cfg(test)]
    pub(crate) fn toolbar_picker(&self) -> Option<&Entity<Picker<ToolbarPicker>>> {
        (self.palette == Some(Palette::Toolbar)).then_some(&self.toolbar_picker)
    }

    /// Shows or hides every pane's toolbar, as `view.toggle_breadcrumbs`
    /// does.
    pub(crate) fn toggle_breadcrumbs(&mut self, cx: &mut Context<Self>) {
        self.breadcrumbs = !self.breadcrumbs;
        self.layout_keys.breadcrumbs.set(self.breadcrumbs);
        self.layout_keys_changed(cx);
        self.chain_key = None;
        let visible = self.breadcrumbs;
        for pane in self.panes(cx) {
            pane.update(cx, |pane, cx| {
                pane.update_toolbar(|toolbar| toolbar.visible = visible, cx)
            });
        }
        cx.notify();
    }

    /// Enables back and forward in every pane, as the navigation history
    /// allows.
    pub(crate) fn set_nav_buttons(&mut self, nav: NavButtons, cx: &mut Context<Self>) {
        self.nav = nav;
        #[cfg(test)]
        {
            self.nav_pushes += 1;
        }
        for pane in self.panes(cx) {
            pane.update(cx, |pane, cx| {
                pane.update_toolbar(|toolbar| toolbar.nav = nav, cx)
            });
        }
    }

    /// Pushes the symbols enclosing `editor`'s caret into the active pane,
    /// from the outline shown for it, if the toolbars show and the pane (a
    /// tab moved keeps its editor), the editor, its caret or the outline
    /// moved since the last time.
    fn push_symbols(&mut self, editor: Option<&Entity<EditorView>>, cx: &mut Context<Self>) {
        let Some(editor) = editor.filter(|_| self.breadcrumbs) else {
            return;
        };
        let caret = pane_toolbar::caret(editor.read(cx));
        let outline = self.dock.read(cx).outline().read(cx);
        let followed = outline.shown_for(editor.entity_id());
        let shown = followed.flatten();
        let key = (
            self.active.entity_id(),
            editor.entity_id(),
            caret,
            followed.map(|shown| shown.map(|(_, generation)| generation)),
        );
        if self.chain_key == Some(key) {
            return;
        }
        self.chain_key = Some(key);
        // A pane made active again keeps its chain until the outline
        // following it there lands, unless its caret moved: so a click on
        // one of its crumbs, which activates it on the way down, still lands.
        let kept = &self.active.read(cx).toolbar().symbols;
        if key.3 == Some(None) && kept.editor == Some(key.1) && kept.caret == caret {
            return;
        }
        #[cfg(test)]
        {
            self.chains_computed += 1;
        }
        let chain = SymbolChain {
            editor: Some(editor.entity_id()),
            caret,
            generation: key.3.flatten().unwrap_or_default(),
            items: shown.map_or_else(Vec::new, |(outline, _)| {
                pane_toolbar::symbol_chain(outline, caret)
            }),
        };
        self.active.update(cx, |pane, cx| {
            pane.update_toolbar(|toolbar| toolbar.symbols = chain, cx)
        });
    }

    /// The status bar, for a host to set its own items on.
    pub fn status_bar(&self) -> &Entity<StatusBar> {
        &self.status_bar
    }

    /// The window's shell, around the panes.
    pub fn shell(&self) -> &Entity<Shell> {
        &self.shell
    }

    /// Shows or hides the title bar the standalone app draws under its
    /// transparent macOS title bar: see [`crate::titlebar_options`]. Off by
    /// default.
    pub fn set_title_bar(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.shell
            .update(cx, |shell, cx| shell.set_title_bar(shown, cx));
    }

    /// Shows or hides the menu bar drawn in the window (kiln-shell's), from
    /// the menus [`fe_gpui::menu::install`] set: for platforms with no
    /// native menu bar (Linux). Off by default.
    pub fn set_menu_bar(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.menu_bar = shown.then(|| cx.new(MenuBarView::new));
        let bar = self.menu_bar.clone();
        self.shell
            .update(cx, |shell, cx| shell.set_menu_bar(bar, cx));
    }

    /// Draws the window's frame (title, buttons, resize edges) whenever the
    /// platform asks the app to, as GNOME on Wayland does; the window manager
    /// draws it otherwise. For a standalone app on Linux. Off by default.
    pub fn set_window_frame(&mut self, drawn: bool, cx: &mut Context<Self>) {
        self.shell
            .update(cx, |shell, cx| shell.set_window_frame(drawn, cx));
    }

    /// The menu bar, while it's shown.
    pub fn menu_bar(&self) -> Option<&Entity<MenuBarView>> {
        self.menu_bar.as_ref()
    }

    /// The name the title bar reads.
    #[cfg(test)]
    pub(crate) fn title_bar_name(&self) -> gpui::SharedString {
        title_bar::name(self.root.as_deref())
    }

    pub fn status_bar_visible(&self) -> bool {
        self.status_bar_visible
    }

    /// Applies the `terminal` settings to the terminal panel.
    pub fn set_terminal_settings(&mut self, settings: &TerminalSettings, cx: &mut Context<Self>) {
        self.terminal
            .update(cx, |terminal, cx| terminal.set_settings(settings, cx));
    }

    /// Shows or hides the status bar, as `view.toggle_status_bar` does,
    /// though without saving it (see [`crate::save_toggles_to`]).
    pub fn set_status_bar_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.status_bar_visible = visible;
        self.show_status_bar(cx);
        cx.notify();
    }

    /// Has the shell show the status bar if it's visible, or if vim's
    /// command line, which shows in its place, is open.
    fn show_status_bar(&mut self, cx: &mut Context<Self>) {
        let shown = self.status_bar_visible || self.status_bar.read(cx).line_open();
        self.shell
            .update(cx, |shell, cx| shell.set_status_bar_visible(shown, cx));
    }

    /// The active tab's editor, unless a file being read or a read error
    /// shows in its place; for a [`DiffView`], the side focused last, so
    /// the vim line, the status bar and the editor commands follow it.
    pub fn active_editor<'a>(&self, cx: &'a App) -> Option<&'a Entity<EditorView>> {
        let pane = self.active.read(cx);
        pane.active_editor().or_else(|| {
            let diff = active_diff_view(pane)?;
            let view = diff.read(cx);
            let left = !view.is_inline() && self.diff_left == Some(diff.entity_id());
            Some(if left { view.left() } else { view.right() })
        })
    }

    /// Notes which side of the active diff view has focus, if one does.
    fn follow_diff_side(&mut self, window: &Window, cx: &App) {
        let Some(diff) = active_diff_view(self.active.read(cx)) else {
            return;
        };
        let view = diff.read(cx);
        if view.left().focus_handle(cx).is_focused(window) {
            self.diff_left = Some(diff.entity_id());
        } else if view.right().focus_handle(cx).is_focused(window) {
            self.diff_left = None;
        }
    }

    /// Tells the outline and the status bar which editor is active, if that
    /// changed, and has the project panel reveal its file. Every tab change
    /// redraws, though not every one notifies (focusing an open tab only
    /// focuses it), so the workbench calls it as it renders. With splits,
    /// it's the active pane's active editor. Whether the status bar's editor
    /// changed.
    fn follow_active_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.follow_diff_side(window, cx);
        let editor = self.active_editor(cx).cloned();
        let outline = self.dock.read(cx).outline().clone();
        outline.update(cx, |outline, cx| outline.set_editor(editor.as_ref(), cx));
        self.push_symbols(editor.as_ref(), cx);
        let id = editor.as_ref().map(Entity::entity_id);
        self.follow_vim_keys(editor.as_ref(), window, cx);
        // The pane's active item, or a diff view's side.
        let item = match editor.clone() {
            Some(editor) => Some(Box::new(editor) as Box<dyn ItemHandle>),
            None => (self.active.read(cx).active_item()).map(ItemHandle::boxed_clone),
        };
        let followed = self.status_bar.update(cx, |bar, cx| {
            let followed = bar.item_id() != item.as_ref().map(|item| item.id());
            bar.set_active_item(item.as_deref(), window, cx);
            followed
        });
        let reveal = id != self.revealed;
        self.revealed = id;
        // The editor reparses as it renders, which notifies no one.
        let status_bar = self.status_bar.clone();
        cx.defer_in(window, move |workbench, window, cx| {
            outline.update(cx, OutlinePanel::frame_drawn);
            if followed {
                // Drawn uncached, it's rendered again for the cache: now,
                // rather than in whichever frame comes next.
                status_bar.update(cx, |_, cx| cx.notify());
            }
            if reveal {
                workbench.reveal_active(cx);
            }
            workbench.leave_hidden_panel(window, cx);
            workbench.sync_lsp(cx);
        });
        followed
    }

    /// Has the project panel select the active editor's file, if it has
    /// one, expanding the directories above it; else drops a reveal still
    /// pending for another tab. Focus stays where it is.
    fn reveal_active(&mut self, cx: &mut Context<Self>) {
        let path = self
            .active_editor(cx)
            .and_then(|editor| editor.read(cx).model().path().map(Path::to_owned));
        self.panel.update(cx, |panel, cx| match path {
            Some(path) => panel.reveal(path, cx),
            None => panel.cancel_reveal(),
        });
    }

    /// Focuses the active editor if a dock panel has focus but isn't
    /// shown, the dock being hidden or showing another panel.
    fn leave_hidden_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dock.read(cx).hidden_panel_focused(window, cx) {
            window.focus(&self.focus_handle(cx), cx);
        }
    }

    /// Does a dock op, for a key or a rail click, with the active editor as
    /// the fallback focus: showing the project panel focuses it, showing
    /// search focuses its input, and showing the outline leaves focus where
    /// it is unless on a hidden panel. False if the op is the search
    /// panel's to do.
    fn run_dock_op(&mut self, op: DockOp, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let done = self.dock.update(cx, |dock, cx| dock.run(op, window, cx));
        if op == DockOp::ShowProject {
            window.focus(&self.panel.focus_handle(cx), cx);
        }
        self.leave_hidden_panel(window, cx);
        done
    }

    /// The launch file the host is reading, until it lands: it opens in a
    /// tab then, whatever the pane has shown since.
    /// Nothing once its pane has closed: [`Workbench::file_read`] drops it
    /// then.
    pub(crate) fn launch_reading(&self, cx: &App) -> Option<&Path> {
        let (path, pane) = self.launch_read.as_ref()?;
        let pane = pane.upgrade()?;
        self.pane_group(cx)
            .panes()
            .contains(&&pane)
            .then_some(path.as_path())
    }

    /// Opens the host's read of the launch file, `Ok(Opened::Text(None))`
    /// being a new file, in the pane it was launched in, whatever that pane has shown
    /// since; see [`Launch::file`]. If that pane has closed, the read is
    /// dropped. Any other read opens in the pane showing it being read, else
    /// the active pane.
    pub fn file_read(
        &mut self,
        path: PathBuf,
        result: std::io::Result<Opened>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = match self.launch_read.take_if(|(launched, _)| *launched == path) {
            Some((_, pane)) => {
                let open = self.pane_group(cx).panes();
                let pane = pane.upgrade().filter(|pane| open.contains(&pane));
                let Some(pane) = pane else {
                    log::info!("not opening {}: its pane closed", path.display());
                    return;
                };
                pane
            }
            None => self
                .pane_group(cx)
                .panes()
                .into_iter()
                .find(|pane| pane.read(cx).is_opening(&path))
                .unwrap_or(&self.active)
                .clone(),
        };
        pane.update(cx, |pane, cx| pane.file_read(path, result, window, cx));
    }

    /// Applies reloaded settings to the open editors and to editors yet to
    /// open, and `status_bar.visible` to the status bar if the reload
    /// changed it, so reloading for any other key keeps a toggle.
    pub fn reload_settings(&mut self, event: &SettingsEvent, cx: &mut Context<Self>) {
        let visible = event.settings.status_bar.visible;
        if visible != event.previous.status_bar.visible {
            self.set_status_bar_visible(visible, cx);
        }
        if event.settings.terminal != event.previous.terminal {
            self.set_terminal_settings(&event.settings.terminal, cx);
        }
        for pane in self.panes(cx) {
            pane.update(cx, |pane, cx| pane.reload_settings(event, cx));
        }
        // Editors restored from a saved layout open as the panes' do.
        let theme = self.active.read(cx).theme().clone();
        self.terminal
            .update(cx, |terminal, cx| terminal.set_theme(&theme, cx));
        fe_gpui::set_restore_style(cx, theme, event.settings.editor.clone());
        self.lsp
            .format_on_save
            .set(event.settings.editor.format_on_save);
        self.sync_save_hooks(cx);
        self.reload_lsp_settings(&event.settings.lsp, cx);
        self.reload_auto_save(&event.settings.editor, cx);
    }

    /// Runs a typed command (the `workbench.*` tab commands and the
    /// `pane.*` commands) as its action, or a palette toggle, a status bar command or a dock
    /// command; others go on to the app.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        // A typed command, as its action, whose handler notes it ran: the
        // tab and pane commands.
        if kiln::commands::dispatch_typed(run, window, cx) {
            return;
        }
        #[cfg(test)]
        self.commands_run.push(run.0.clone());
        if self.run_references_command(run, window, cx) {
            return;
        }
        if let Some(command) = git_commands::op(run, cx) {
            return self.run_git_command(command, window, cx);
        }
        if project_panel::op(run, cx) == Some(PanelOp::RevealActive) {
            return self.reveal_active(cx);
        }
        let op = if outline_panel::shows(run, cx) {
            Some(DockOp::ShowOutline)
        } else {
            dock::op(run, cx)
        };
        // `problems.show` is the dock's; the other problems commands
        // the workbench's.
        if let Some(DockOp::Problems(op)) = op
            && op != ProblemsOp::Show
        {
            return self.run_problems_op(op, window, cx);
        }
        let done = op.is_some_and(|op| self.run_dock_op(op, window, cx));
        if !done {
            cx.propagate();
        }
    }
}

impl Focusable for Workbench {
    /// The active editor's (a diff view's focused side), or else the active
    /// viewer's, or else the pane's, so the workbench's commands apply.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.active_editor(cx) {
            Some(editor) => editor.focus_handle(cx),
            None => self.active.read(cx).focus_target(cx),
        }
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&self.active.read(cx).title(cx));
        let followed = self.follow_active_editor(window, cx);
        self.land_problem(window, cx);
        let name = title_bar::name(self.root.as_deref());
        self.shell.update(cx, |shell, cx| {
            // Not from its cache in a frame that gave it another editor: a
            // notification during a draw doesn't reach this frame's cache,
            // nor ask for another frame.
            if followed {
                shell.redraw_status_bar();
            }
            // The chrome names the root, not the active item, as the
            // workspace would have it.
            if *shell.title() != name {
                shell.set_title(name, cx);
            }
            #[cfg(test)]
            shell.set_client_tiling(self.client_tiling);
        });
        // Above the status bar, or where it would be.
        let status_bar = self.status_bar_visible || self.status_bar.read(cx).line_open();
        let drop_border = self.ui.accent;
        let drop_wash = drop_border.opacity(0.12);
        let bottom = if status_bar { ROW_HEIGHT } else { px(0.) } + px(8.);
        // The typed commands' handlers, in the workbench's key context.
        Self::attach(div(), cx)
            .on_action(cx.listener(Self::on_run_command))
            // So a modifier pressed while a panel has focus reaches the
            // editors, for their cmd-click hover.
            .on_modifiers_changed(EditorView::modifiers_changed)
            .size_full()
            .child(
                div()
                    .size_full()
                    .text_size(px(12.))
                    .child(self.workspace.clone()),
            )
            .child(
                // Over the panes, so an editor's mouse-up doesn't swallow the
                // drop; without occluding, so nothing else loses the mouse.
                // A translucent wash while files are dragged over the window.
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .on_drop(cx.listener(|workbench, paths: &ExternalPaths, window, cx| {
                        workbench.drop_external(paths, window.mouse_position(), window, cx)
                    }))
                    .drag_over::<ExternalPaths>(move |style, _, _, _| {
                        style.bg(drop_wash).border_2().border_color(drop_border)
                    }),
            )
            .child(
                // The window's height, for toasts to fit in, bottom first.
                div()
                    .absolute()
                    .top(px(8.))
                    .right(px(12.))
                    .bottom(bottom)
                    .flex()
                    .flex_col()
                    .justify_end()
                    .items_end()
                    .child(self.toasts.clone()),
            )
    }
}

/// Where the terminal starts: the launch root, else the launch file's
/// directory, else the home directory.
fn terminal_dir(launch: &Launch) -> PathBuf {
    let file_dir = || launch.file.as_ref()?.0.parent().map(Path::to_owned);
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    (launch.root.clone().or_else(file_dir).or_else(home)).unwrap_or_else(|| PathBuf::from("/"))
}

/// The terminal's dock's sizes in its saved state ([`DockState::save`]),
/// and nothing else: the dock comes back hidden, with no session.
fn terminal_dock_sizes(saved: &Value) -> Value {
    let size = |key: &'static str| Some((key, saved.get(key)?.clone()));
    Value::from_iter(
        ["size", "side_size", "bottom_size"]
            .into_iter()
            .filter_map(size),
    )
}

/// Puts back the terminal dock's sizes as [`terminal_dock_sizes`] kept
/// them, as [`DockState::restore`] does, but leaving its panel,
/// visibility and rail as they are: `restore` puts the active panel on the
/// rail, as if it had shown, and a terminal never shown takes no room.
fn restore_terminal_dock_sizes(dock: &mut DockState, saved: &Value) {
    let size = |key: &str| {
        let size = saved.get(key)?.as_f64()?;
        Some(px(size as f32))
    };
    let position = dock.position();
    // Saved before the dock kept both: its size at its position.
    if let Some(size) = size("size") {
        dock.set_size(size);
    }
    let sizes = [
        ("side_size", DockPosition::Right),
        ("bottom_size", DockPosition::Bottom),
    ];
    for (key, at) in sizes {
        if let Some(size) = size(key) {
            dock.set_position(at);
            dock.set_size(size);
        }
    }
    dock.set_position(position);
}

/// Puts the sidebar at `sidebar` and the terminal's dock at `terminal`,
/// two edges, the sidebar's a side: each dock's state moves, and its view
/// moves in the shell.
fn place_docks(
    (dock, terminal_dock, shell): (&Entity<Dock>, &Entity<kiln_shell::Dock>, &Entity<Shell>),
    sidebar: DockPosition,
    terminal: DockPosition,
    cx: &mut App,
) {
    debug_assert!(sidebar != terminal && sidebar != DockPosition::Bottom);
    let was_sidebar = dock.read(cx).state().position();
    let was_terminal = terminal_dock.read(cx).dock().position();
    if (was_sidebar, was_terminal) == (sidebar, terminal) {
        return;
    }
    dock.update(cx, |dock, cx| {
        dock.dock_mut().set_position(sidebar);
        cx.notify();
    });
    terminal_dock.update(cx, |dock, cx| {
        dock.dock_mut().set_position(terminal);
        cx.notify();
    });
    let views: [(DockPosition, AnyView); 2] = [
        (sidebar, dock.clone().into()),
        (terminal, terminal_dock.clone().into()),
    ];
    shell.update(cx, |shell, cx| {
        shell.set_dock(was_sidebar, None, cx);
        shell.set_dock(was_terminal, None, cx);
        for (position, view) in views {
            shell.set_dock(position, Some(view), cx);
        }
    });
}

/// fastedit's own keys in the window's saved layout, beside the
/// workspace's panes ([`Workspace::on_save`]): the docks' places under
/// `docks` (`sidebar` and `terminal`, where a layout saved before the
/// workspace saved it kept them too), the sidebar's state (`dock`), the
/// terminal dock's sizes (`terminal_dock`), whether the breadcrumbs show
/// and the window's `bounds`.
struct LayoutKeys {
    sidebar: Entity<Dock>,
    terminal: Entity<kiln_shell::Dock>,
    shell: Entity<Shell>,
    /// Whether the breadcrumbs show, as [`Workbench::breadcrumbs`]; or as
    /// a restored entry has them, for the workbench to follow.
    breadcrumbs: Cell<bool>,
    /// The window's bounds, as they last changed.
    bounds: Cell<Option<WindowBounds>>,
}

impl LayoutKeys {
    fn save(&self, cx: &App) -> Vec<(&'static str, Value)> {
        let sidebar = self.sidebar.read(cx).state();
        let terminal = self.terminal.read(cx).dock();
        let docks = Value::from_iter([
            ("sidebar", dock_layout::position_name(sidebar.position())),
            ("terminal", dock_layout::position_name(terminal.position())),
        ]);
        let mut keys = vec![
            ("docks", docks),
            ("dock", sidebar.save()),
            ("terminal_dock", terminal_dock_sizes(&terminal.save())),
            ("breadcrumbs", Value::from(self.breadcrumbs.get())),
        ];
        if let Some(bounds) = self.bounds.get() {
            keys.push(("bounds", layout::save_bounds(bounds)));
        }
        keys
    }

    /// Puts back the docks' places, if two edges with the sidebar at a
    /// side, then the sidebar's state and the terminal's dock's sizes (it
    /// stays hidden, with no session), and notes the breadcrumbs.
    fn restore(&self, entry: &Value, cx: &mut App) {
        let position = |dock| {
            let name = entry.get("docks")?.get(dock)?.as_str()?;
            dock_layout::position_named(name)
        };
        if let (Some(sidebar), Some(terminal)) = (position("sidebar"), position("terminal"))
            && sidebar != terminal
            && sidebar != DockPosition::Bottom
        {
            let docks = (&self.sidebar, &self.terminal, &self.shell);
            place_docks(docks, sidebar, terminal, cx);
        }
        if let Some(saved) = entry.get("dock") {
            self.sidebar.update(cx, |dock, cx| dock.restore(saved, cx));
        }
        if let Some(saved) = entry.get("terminal_dock") {
            self.terminal.update(cx, |dock, cx| {
                restore_terminal_dock_sizes(dock.dock_mut(), saved);
                cx.notify();
            });
        }
        if let Some(shown) = entry.get("breadcrumbs").and_then(Value::as_bool) {
            self.breadcrumbs.set(shown);
        }
    }
}

/// The active item of `pane`, if it's a diff view.
fn active_diff_view(pane: &Pane) -> Option<&Entity<DiffView>> {
    pane.pane().active_item()?.downcast_ref::<DiffView>()
}

/// The prompt shown when the open panel fails with `error`: see
/// [`kiln_platform::dialogs::open_panel_failed`].
pub(crate) fn open_panel_failed(error: &str) -> (&'static str, String) {
    kiln_platform::dialogs::open_panel_failed(error, kiln_platform::Os::current())
}

#[cfg(test)]
thread_local! {
    /// The open panel's next answer is this error, in tests: the test
    /// platform's panel can't fail.
    pub(crate) static OPEN_PANEL_FAILS: std::cell::Cell<Option<&'static str>> =
        const { std::cell::Cell::new(None) };
}

#[path = "vim_host.rs"]
mod vim_host;

#[path = "git_commands.rs"]
pub(crate) mod git_commands;

#[path = "git_diff.rs"]
mod git_diff;

#[path = "git_stage.rs"]
mod git_stage;

#[path = "git_commit.rs"]
mod git_commit;

#[path = "git_blame.rs"]
mod git_blame;

#[path = "git_branch.rs"]
mod git_branch;

#[path = "external_drop.rs"]
mod external_drop;

#[path = "recent_picker.rs"]
pub(crate) mod recent_picker;

#[path = "hot_exit.rs"]
mod hot_exit;

#[path = "toast.rs"]
pub(crate) mod toast;

#[cfg(test)]
#[path = "terminal_no_rail_tests.rs"]
mod terminal_no_rail_tests;
#[cfg(test)]
#[path = "terminal_rail_button_tests.rs"]
mod terminal_rail_button_tests;

#[cfg(test)]
#[path = "close_tests.rs"]
mod close_tests;

#[cfg(test)]
#[path = "vim_host_tests.rs"]
mod vim_host_tests;

#[cfg(test)]
#[path = "palette_tests.rs"]
mod palette_tests;

#[cfg(test)]
#[path = "layout_tests.rs"]
mod layout_tests;

#[cfg(test)]
#[path = "dock_resize_tests.rs"]
mod dock_resize_tests;

#[cfg(test)]
#[path = "dock_position_tests.rs"]
mod dock_position_tests;

/// Editors and the paths they're to take: [`Workbench::plan_rename`]'s.
pub(crate) type Moves = Vec<(Entity<EditorView>, PathBuf)>;

/// Editors still saving after their tab or window closed, until the save is
/// over, held so [`Workbench::busy_under`] sees the write coming.
#[derive(Default)]
struct DetachedSaves(Vec<Entity<EditorView>>);

impl Global for DetachedSaves {}

impl DetachedSaves {
    /// Keeps those of `editors` saving, and lets go of the saves over.
    fn keep(editors: impl Iterator<Item = Entity<EditorView>>, cx: &mut App) {
        let mut kept = (cx.try_global::<DetachedSaves>())
            .map(|saves| saves.0.clone())
            .unwrap_or_default();
        kept.extend(editors);
        kept.retain(|editor| editor.read(cx).is_saving());
        cx.set_global(DetachedSaves(kept));
    }
}

/// Whether `a` and `b` overlap: one is the other, or under it, as [`under`]
/// matches.
pub(crate) fn overlap(a: &Path, b: &Path) -> bool {
    under(a, b, &resolved(b)) || under(b, a, &resolved(a))
}

/// Why a rename waits ([`Workbench::busy_under`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Busy {
    /// A save or a Save As would write there.
    Saving,
    /// A file there is being read into a tab.
    Opening,
    /// The project panel is creating, duplicating or trashing there.
    Working,
}

/// Whether `path` is `from`, or under it, spelled as it is or as another
/// spelling of the same folders: `from_resolved` is `from`'s
/// [`resolved`].
pub(crate) fn under(path: &Path, from: &Path, from_resolved: &Path) -> bool {
    path.starts_with(from) || resolved(path).starts_with(from_resolved)
}

/// Where the file at `path` is once `from` is renamed `to`, if it's `from`
/// or under it: under `to` as `path` spells it, or, for a path spelled
/// through other folders (the root opened through a symlink, say), under
/// `to`'s resolved spelling. `from_resolved` and `to_resolved` are their
/// [`resolved`]s, taken after the rename. The entry itself is never
/// dereferenced, so a symlinked file keeps its link.
fn moved_to(
    path: &Path,
    from: &Path,
    to: &Path,
    from_resolved: &Path,
    to_resolved: &Path,
) -> Option<PathBuf> {
    let join = |base: &Path, rest: &Path| {
        if rest.as_os_str().is_empty() {
            base.to_owned()
        } else {
            base.join(rest)
        }
    };
    if let Ok(rest) = path.strip_prefix(from) {
        return Some(join(to, rest));
    }
    let resolved = resolved(path);
    let rest = resolved.strip_prefix(from_resolved).ok()?;
    Some(join(to_resolved, rest))
}

/// One identity for the spellings of `path`: its folder with the longest
/// existing ancestor resolved through symlinks and `..` (the rest, which
/// may be deleted, kept), then its own name as is. The entry itself is
/// never dereferenced, so a symlink keeps its own identity apart from its
/// target's.
pub(crate) fn resolved(path: &Path) -> PathBuf {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return path.to_owned();
    };
    let mut rest = Vec::new();
    let mut ancestor = parent;
    let base = loop {
        if let Ok(real) = ancestor.canonicalize() {
            break real;
        }
        match (ancestor.file_name(), ancestor.parent()) {
            (Some(name), Some(up)) => {
                rest.push(name);
                ancestor = up;
            }
            _ => break ancestor.to_owned(),
        }
    };
    rest.iter()
        .rev()
        .fold(base, |base, part| base.join(part))
        .join(name)
}

#[cfg(test)]
#[path = "tab_git_status_tests.rs"]
mod tab_git_status_tests;

#[cfg(test)]
#[path = "windows_tests.rs"]
mod windows_tests;

#[cfg(test)]
#[path = "workspace_embed_tests.rs"]
mod workspace_embed_tests;

#[cfg(test)]
mod focus_tests {
    use std::{sync::Arc, time::Duration};

    use fe_fileview::{FileView, IndexState, LineIndex};
    use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

    use crate::{Launch, Workbench, loader::Opened};

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    fn focused(handle: &impl gpui::Focusable, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| handle.focus_handle(cx).is_focused(window))
    }

    /// A launch file the host opened as Large shows in a viewer that the
    /// workbench's focus handle focuses, as the app does at launch, and
    /// again once a hidden dock gives focus back: its movement keys reach
    /// it.
    #[gpui::test]
    fn a_large_launch_file_takes_the_keys(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let big = root.join("big.log");
        let text: String = (0..100).map(|row| format!("line {row:02}\n")).collect();
        std::fs::write(&big, text).unwrap();
        let view = Arc::new(FileView::open(&big).unwrap());
        let index = Arc::new(LineIndex::build(view.clone()));
        while index.progress().state == IndexState::Building {
            std::thread::sleep(Duration::from_millis(1));
        }
        let launch = Launch {
            root: Some(root),
            file: Some((big, Some(Opened::Large { view, index }))),
        };
        let (workbench, cx, _) = crate::tests::open(cx, launch);
        draw(cx);
        cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
        draw(cx);
        let viewer = workbench.read_with(cx, |workbench, cx| {
            let pane = workbench.pane().read(cx);
            pane.tabs()[pane.active().unwrap()].viewer().cloned()
        });
        let viewer = viewer.expect("the launch file shows in a viewer");
        assert!(focused(&viewer, cx));
        let head = |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.model().head());
        assert_eq!(head(cx), 0);
        cx.simulate_keystrokes("down");
        assert_eq!(head(cx), "line 00\n".len());

        // The project panel takes focus, then its dock hides: focus goes
        // back to the viewer.
        cx.simulate_keystrokes("cmd-shift-e");
        let panel: Entity<crate::ProjectPanel> =
            workbench.read_with(cx, |workbench: &Workbench, _| workbench.panel().clone());
        assert!(focused(&panel, cx));
        cx.simulate_keystrokes("cmd-b");
        draw(cx);
        assert!(focused(&viewer, cx));
        cx.simulate_keystrokes("down");
        assert_eq!(head(cx), 2 * "line 00\n".len());
    }
}

#[cfg(test)]
#[path = "layout_compat_tests.rs"]
mod layout_compat_tests;
