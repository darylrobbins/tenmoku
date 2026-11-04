//! The window shell: the editor panes, with the dock (the project panel,
//! search, the outline, the git changes, references or the problems) on the
//! right and the terminal below.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_editor::EditorModel;
use fe_git::ChangeKind;
use fe_gpui::{
    DiffView, EditorSettings, EditorTheme, EditorView,
    commands::{EditorViewEvent, RunCommand},
};
use fe_ui::ROW_HEIGHT;
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, EventEmitter, ExternalPaths, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, WeakEntity, WeakFocusHandle, Window, WindowBounds, div, px,
};

use self::recent_picker::RecentPicker;
use self::workbench_chrome::terminal_dir;
use self::workbench_files::DetachedSaves;
pub(crate) use self::workbench_files::{Busy, resolved};
pub(crate) use self::workbench_palettes::Palette;
use crate::{
    ChatPanel, Dock, GitLogPanel, GitPanel, KEY_CONTEXT, MenuBarView, OutlinePanel, Pane, Picker,
    ProjectPanel, QuickOpen, ReferencesPanel, SearchPanel, StatusBar,
    auto_save::AutoSave,
    config_problems::ConfigProblemsPicker,
    disk_sync::DiskSync,
    dock::{DockEvent, ProblemsPanel},
    extensions_picker::ExtensionsPicker,
    git::GitState,
    git_panel,
    go_to_line::GoToLine,
    language_picker::LanguagePicker,
    loader::{self, Opened},
    lsp::Lsp,
    nav::Nav,
    open,
    pane_toolbar::NavButtons,
    project_panel,
    setup_wizard::SetupWizard,
    terminal_panel::TerminalPanel,
    theme_picker::ThemePicker,
    title_bar,
    toolbar_picker::ToolbarPicker,
    trust::{self, TrustChanged},
};
use fe_ui::Icon;
use kiln::ViewCommands as _;
use kiln_shell::{
    DockPosition, DockState, DockView as _, ModalEvent, ModalLayer, PaneView as _, Shell,
    Workspace, layout::LayoutStore,
};

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
/// the root. No argument is no root and no tab.
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
    /// here, its notifications, passed on to the editors that share its
    /// buffer, and its jumps, recorded in the navigation history.
    editors: HashMap<EntityId, (Entity<EditorView>, [Subscription; 2])>,
    /// Per pane, the file the project panel last opened there, until its
    /// editor shows (an open can wait behind a Save As): for vim's normal
    /// mode then ([`Workbench::open_from_tree`]).
    tree_opens: HashMap<EntityId, (PathBuf, Subscription)>,
    /// `workbench.preview_tabs`: whether a file clicked once in the
    /// project panel opens in a preview tab.
    preview_tabs: bool,
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
    /// The color themes, shown by `theme.select`.
    theme_picker: Entity<Picker<ThemePicker>>,
    /// The installed extensions, shown by `workbench.show_extensions`.
    extensions_picker: Entity<Picker<ExtensionsPicker>>,
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
    /// The follow of the extension language servers waiting on consent,
    /// which asks about them.
    _consent_prompt: crate::lsp_consent_prompt::ConsentPrompt,
    /// The follow of the plugins waiting on a grant, which asks about them.
    _plugin_prompt: crate::plugin_host::PluginPrompt,
    /// The root's workspace trust.
    pub(crate) trust: kiln_shell::trust::RootTrust,
    /// The root's language servers.
    pub(crate) lsp: Lsp,
    /// What runs the external formatters ([`crate::external_format`]).
    pub(crate) format_runner: Rc<dyn crate::external_format::Runner>,
    /// To the project panel's events: replaced with it on a new root.
    root_subscription: Subscription,
    /// To the Changes panel's events: replaced with it on a new root.
    changes_subscription: Subscription,
    /// The config service's watch on the root's settings file: replaced
    /// on a new root.
    root_settings: Option<kiln::platform::config::FileWatch>,
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
    /// The AI turn's changeset the AI Changes panel lists, one at a time.
    pub(crate) ai_changes: Option<crate::ai_changes_panel::AiChanges>,
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
    /// How many times a pane has been made the active one, even the same
    /// pane again: what an ask that a round trip through another pane
    /// supersedes compares.
    pub(crate) activations: u64,
    /// Like [`Self::activations`], but advanced only where the workbench
    /// makes a pane active, synchronously, never by the workspace's queued
    /// [`WorkspaceEvent::ActiveChanged`]: what a link open captured in the
    /// same update as an activation compares.
    pub(crate) link_epoch: u64,
    /// The spelling squiggles in its editors.
    pub(crate) spelling: crate::spell_squiggles::Spelling,
    /// The checks of the open ignore files.
    pub(crate) ignore_diagnostics: crate::ignore_diagnostics::IgnoreDiagnostics,
    /// Zen Mode: see [`zen_mode`].
    pub(crate) zen: zen_mode::ZenMode,
    /// The setup wizard's sheet over the window, while it shows.
    setup: Option<Entity<SetupWizard>>,
}

impl EventEmitter<RootChanged> for Workbench {}
impl EventEmitter<TrustChanged> for Workbench {}

/// What runs Install/Uninstall 'tenmoku' Command in PATH: the app sets it
/// ([`Workbench::set_shell_command_handler`]), as the work is the binary's.
/// The argument is whether to install.
#[derive(Clone, Copy)]
struct ShellCommandHandler(fn(bool, &mut App));

impl gpui::Global for ShellCommandHandler {}

pub(crate) fn run_shell_command(install: bool, cx: &mut App) {
    if let Some(handler) = cx.try_global::<ShellCommandHandler>().copied() {
        (handler.0)(install, cx);
    }
}

impl Workbench {
    /// Sets what the Install and Uninstall 'tenmoku' Command in PATH
    /// commands run: `handler(install, cx)`.
    pub fn set_shell_command_handler(cx: &mut App, handler: fn(bool, &mut App)) {
        cx.set_global(ShellCommandHandler(handler));
    }

    /// Shows a toast in the active window, else the first open, from
    /// anywhere: `level` picks info, warning or error (`Debug` and `Trace`
    /// show as info); `detail` is a second, quieter line.
    pub fn notify(cx: &mut App, level: log::Level, message: String, detail: Option<String>) {
        let level = match level {
            log::Level::Error => toast::Level::Error,
            log::Level::Warn => toast::Level::Warning,
            _ => toast::Level::Info,
        };
        let mut toast = toast::Toast::new(level, message);
        toast.detail = detail;
        toast::show(cx, toast);
    }

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
        let code_actions_on_save = settings.code_actions_on_save.clone();
        let inlay_hints = settings.inlay_hints;
        let code_lens = settings.code_lens;
        let links = settings.links;
        let color_decorators = settings.color_decorators;
        let lightbulb = settings.lightbulb;
        let linked_editing = settings.linked_editing;
        let inline_blame = settings.inline_blame;
        let spelling = crate::spell_squiggles::Spelling::new(&settings);
        let auto_save = AutoSave::new(&settings, window, cx);
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
        let panel =
            cx.new(|cx| ProjectPanel::new(launch.root, project_panel::show_ignored(cx), cx));
        let problems = cx.weak_entity();
        let problems = cx.new(|cx| ProblemsPanel::new(problems, &theme, cx));
        let owner = cx.entity_id();
        let pane = cx.new(|cx| {
            let mut pane = Pane::new(theme, settings, cx);
            pane.owned_by(owner);
            match launch.file {
                None => {}
                Some((path, Some(Opened::Text(buffer)))) => pane.add(
                    EditorModel::new(loader::buffer_or_new(buffer), Some(path)),
                    cx,
                ),
                Some((_, Some(Opened::Large { view, index }))) => pane.add_viewer(view, index, cx),
                Some((path, Some(Opened::Image))) => pane.add_image_viewer(path, cx),
                Some((path, None)) => pane.opening(path, cx),
            }
            pane
        });
        let outline = cx.new(|cx| OutlinePanel::new(cx));
        let changes = cx.new(|cx| GitPanel::new(root.clone(), cx));
        let references = cx.weak_entity();
        let references = cx.new(|cx| ReferencesPanel::new(references, cx));
        let workbench = cx.weak_entity();
        let git_log = cx.new(|cx| GitLogPanel::new(workbench, cx));
        // Its key is read when it first draws, not now.
        let chat = cx.new(|cx| ChatPanel::new(window, cx));
        let dock = cx.new(|cx| {
            let search = cx.new(|cx| SearchPanel::new(root.clone(), pane.downgrade(), cx));
            Dock::new(
                panel.clone(),
                search,
                outline.clone(),
                changes.clone(),
                git_log,
                chat,
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
        let quick_open = Self::new_quick_open(root, pane.downgrade(), cx);
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
        let language_picker = cx.new(|cx| {
            Picker::new(
                LanguagePicker::new(),
                "Select a language…",
                fe_ui::ui_theme(cx),
                cx,
            )
        });
        let config_problems = cx.weak_entity();
        let config_problems = cx.new(|cx| {
            let delegate = ConfigProblemsPicker::new(config_problems);
            Picker::new(delegate, "Filter config problems…", fe_ui::ui_theme(cx), cx)
        });
        let go_to_line = cx.new(|cx| {
            let delegate = GoToLine::new();
            Picker::new(delegate, "Line or line:column…", fe_ui::ui_theme(cx), cx)
        });
        let toolbar_picker = cx.new(|cx| {
            let delegate = ToolbarPicker::new(outline.downgrade());
            Picker::new(delegate, "Filter…", fe_ui::ui_theme(cx), cx)
        });
        let recent = cx.weak_entity();
        let recent_picker = cx.new(|cx| {
            let delegate = RecentPicker::new(recent);
            Picker::new(
                delegate,
                "Open recent folder or file…",
                fe_ui::ui_theme(cx),
                cx,
            )
        });
        let theme_picker = cx.new(|cx| {
            Picker::new(
                ThemePicker::new(),
                "Select a color theme…",
                fe_ui::ui_theme(cx),
                cx,
            )
        });
        let extensions_picker = cx.new(|cx| {
            Picker::new(
                ExtensionsPicker::new(),
                "Filter installed extensions…",
                fe_ui::ui_theme(cx),
                cx,
            )
        });
        // A file's folder is only the root it opens in, not one opened.
        if let Some(root) = launch_root.clone().filter(|_| as_folder) {
            recent_picker::record(root, true, cx);
        }
        let nav_state = Nav::new(launch_root.clone(), cx);
        let status_bar = cx.new(|_| StatusBar::new());
        let toasts = cx.new(|cx| toast::ToastLayer::new(window, cx));
        // A restored layout's panes are made like a pane of the workbench's.
        let template = Rc::new(RefCell::new(pane.downgrade()));
        let workspace = cx.new(|cx| {
            let template = template.clone();
            let new_pane = move |_: &mut Window, cx: &mut App| {
                let template = (template.borrow().upgrade()).expect("a pane to restore beside");
                template.update(cx, |pane, cx| cx.new(|cx| pane.new_peer(cx)))
            };
            Workspace::new(KEY_CONTEXT, pane.clone(), new_pane, window, cx)
                .title(fe_config::APP_NAME)
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
            zen_sidebar: Cell::new(None),
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
            preview_tabs: (cx.try_global::<kiln::GlobalSettings<fe_config::Settings>>())
                .is_none_or(|global| global.settings().workbench.preview_tabs),
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
            theme_picker,
            extensions_picker,
            shell,
            modals,
            palette: None,
            nav_state,
            status_bar,
            status_bar_visible: true,
            menu_bar: None,
            #[cfg(test)]
            client_tiling: None,
            root: launch_root.clone(),
            revealed: None,
            launch_read: launch_read.map(|path| (path, pane.downgrade())),
            prompting: false,
            _consent_prompt: crate::lsp_consent_prompt::ConsentPrompt::new(window, cx),
            _plugin_prompt: crate::plugin_host::PluginPrompt::new(window, cx),
            trust: kiln_shell::trust::RootTrust::new(trust::prompt()),
            lsp: Lsp::new(),
            format_runner: Rc::new(crate::external_format::ProcessRunner),
            root_subscription,
            changes_subscription,
            root_settings: Self::watch_root_settings(launch_root.as_deref(), cx),
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
            ai_changes: None,
            owned_editors: Vec::new(),
            diff_left: None,
            vim_keys: None,
            #[cfg(test)]
            git_routed: Vec::new(),
            activations: 0,
            link_epoch: 0,
            spelling,
            ignore_diagnostics: Default::default(),
            zen: zen_mode::ZenMode::new(cx),
            setup: None,
        };
        // Under test, the chrome's window moves are recorded, not made.
        #[cfg(test)]
        title_bar::record_requests(cx);
        workbench.pane_added(&pane, window, cx);
        workbench.track_editors(cx);
        workbench.follow_trust(window, cx);
        workbench.resolve_trust(workbench.root.clone(), as_folder, window, cx);
        workbench.lsp.format_on_save.set(format_on_save);
        *workbench.lsp.code_actions_on_save.borrow_mut() = code_actions_on_save;
        workbench.lsp.inlay_hints_on.set(inlay_hints);
        workbench.lsp.code_lens_on.set(code_lens);
        workbench.lsp.links_on.set(links);
        workbench.lsp.color_decorators_on.set(color_decorators);
        workbench.lsp.lightbulb_on.set(lightbulb);
        workbench.lsp.linked_editing_on.set(linked_editing);
        workbench.start_lsp(window, cx);
        workbench.sync_config_problems(window, cx);
        if inline_blame {
            git_blame::set_line_blame(&mut workbench, true, cx);
        }
        workbench
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
            let jumps = cx.subscribe(editor, |workbench, editor, event, cx| {
                if let EditorViewEvent::Jumped { from } = event {
                    workbench.editor_jumped(&editor, *from, cx);
                }
            });
            let subscriptions = [observed, jumps];
            (self.editors).insert(editor.entity_id(), (editor.clone(), subscriptions));
            if let Some(path) = editor.read(cx).model().path().map(Path::to_owned) {
                recent_picker::record(path, false, cx);
            }
        }
        self.git.update(cx, |git, cx| git.track(&editors, cx));
        self.protect_ai_previews(cx);
        self.tint_tabs(cx);
        self.sync_ignore_files(cx);
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
        let ui = fe_ui::ui_theme(cx);
        for editor in self.open_editors(cx) {
            let tint = editor.read(cx).model().path().and_then(|path| {
                let kind = tints.get(&resolved(path))?;
                Some(git_panel::letter_color(*kind, &ui))
            });
            editor.update(cx, |editor, cx| editor.set_tab_tint(tint, cx));
        }
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
        // A save, a rename or another language notifies too.
        self.sync_ignore_editor(&editor, cx);
        // A diagnostic arriving or clearing notifies, as an edit does.
        self.sync_problems(cx);
        self.sync_backups(cx);
        self.sync_auto_save(cx);
        if self.ai_changes.is_some() {
            // Text edited under a hunk can make it stale.
            self.sync_ai_changes(cx);
        }
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

    /// Puts the setup wizard's sheet over the window, on its first page, with
    /// focus in it. Showing already, it stays as it is.
    pub fn show_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.setup.is_some() {
            return;
        }
        let workbench = cx.weak_entity();
        let setup = cx.new(|cx| SetupWizard::new(workbench, window, cx));
        window.focus(&setup.read(cx).focus_handle(), cx);
        self.setup = Some(setup);
        cx.notify();
    }

    /// Whether the setup wizard's sheet shows.
    pub fn is_setup_showing(&self) -> bool {
        self.setup.is_some()
    }

    /// The sheet that shows, if it does.
    #[cfg(test)]
    pub(crate) fn setup(&self) -> Option<&Entity<SetupWizard>> {
        self.setup.as_ref()
    }

    /// Takes the sheet away, giving focus back to `previous`, what had it, or
    /// if that's gone to the active item.
    pub(crate) fn close_setup(
        &mut self,
        previous: Option<WeakFocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.setup = None;
        let focus =
            (previous.and_then(|focus| focus.upgrade())).unwrap_or_else(|| self.focus_handle(cx));
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Runs a typed command (the `workbench.*` tab commands, the `pane.*`
    /// commands, the dock's and the panels') as its action, a git command,
    /// or a plugin's command, off the main thread; others go on to the app.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        // Under the sheet, only the app's and the window's own.
        let command = run.0.command.as_str();
        if self.setup.is_some() && !command.starts_with("app.") && !command.starts_with("window.") {
            return;
        }
        // A typed command, as its action, whose handler notes it ran: the
        // tab and pane commands.
        if kiln::commands::dispatch_typed(run, window, cx) {
            return;
        }
        if crate::plugin_host::dispatch(run, cx) {
            return;
        }
        #[cfg(test)]
        self.commands_run.push(run.0.clone());
        cx.propagate();
    }
}

impl Focusable for Workbench {
    /// The active editor's (a diff view's focused side), or the Markdown
    /// preview shown in its place, or else the active viewer's, or else the
    /// pane's, so the workbench's commands apply.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if let Some(setup) = &self.setup {
            return setup.read(cx).focus_handle();
        }
        let pane = self.active.read(cx);
        match (pane.shown_preview(), self.active_editor(cx)) {
            (Some(preview), _) => preview.focus_handle(cx),
            (None, Some(editor)) => editor.focus_handle(cx),
            (None, None) => pane.focus_target(cx),
        }
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&self.active.read(cx).title(cx));
        let followed = self.follow_active_editor(window, cx);
        self.land_problem(window, cx);
        // Once the frame says what's in view.
        if self.spelling.is_active() {
            cx.defer_in(window, |workbench, _, cx| {
                workbench.sync_spell_squiggles(cx)
            });
        }
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
        let drop_border = fe_ui::ui_theme(cx).accent;
        let drop_wash = drop_border.opacity(0.12);
        let bottom = if status_bar { ROW_HEIGHT } else { px(0.) } + px(8.);
        // The typed commands' handlers, in the workbench's key context, flagged
        // with the plugin loads whose commands the palette lists. Not under
        // the sheet: a menu pick of one comes straight here as its action, not
        // by `on_run_command`.
        let root = match self.setup {
            Some(_) => div().key_context(KEY_CONTEXT),
            None => Self::attach(div(), cx),
        };
        let root = match crate::plugin_host::key_context(cx) {
            Some(context) if self.setup.is_none() => root.key_context(context),
            _ => root,
        };
        root.on_action(cx.listener(Self::on_run_command))
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
            .children(self.setup.clone())
    }
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
    /// While Zen Mode is on, whether the sidebar showed before it, until
    /// it's shown or hidden meanwhile.
    zen_sidebar: Cell<Option<bool>>,
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

#[path = "workbench_root.rs"]
mod workbench_root;

#[path = "workbench_files.rs"]
mod workbench_files;

#[path = "workbench_palettes.rs"]
mod workbench_palettes;

#[path = "workbench_chrome.rs"]
mod workbench_chrome;

#[path = "vim_host.rs"]
pub(crate) mod vim_host;

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

#[path = "zen_mode.rs"]
pub(crate) mod zen_mode;

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
