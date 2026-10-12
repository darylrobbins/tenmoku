//! The window shell: the editor panes, with the dock (the project panel,
//! search, the outline or the git changes) on the right.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, SettingsEvent, commands::RunCommand};
use fe_ui::UiTheme;
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, PromptButton, PromptLevel, Render,
    Styled as _, Subscription, WeakEntity, Window, WindowBounds, div, px,
};

use crate::{
    Dock, GitPanel, GitPanelEvent, KEY_CONTEXT, MenuBarView, OutlinePanel, Pane, PaneGroup, Picker,
    ProjectPanel, QuickOpen, ReferencesPanel, SearchPanel, StatusBar,
    config_problems::{self, ConfigProblemsPicker},
    dock::{self, DockEvent, DockOp},
    git::GitState,
    go_to_line::GoToLine,
    language_picker::LanguagePicker,
    lsp::{self, Lsp},
    nav::{self, Nav},
    open::{self, OpenOp},
    outline_panel,
    pane::{PaneEvent, Peers, TabId},
    pane_drag::TabDropped,
    pane_group::{self, Destination, Direction, PaneOp},
    pane_toolbar::{self, NavButtons, SymbolChain, ToolbarRequest},
    persist,
    picker::{self, Toggle},
    project_panel::{self, PanelEvent, PanelOp},
    status_bar::{self, StatusOp},
    title_bar,
    toolbar_picker::ToolbarPicker,
    trust::{self, TrustChanged},
};
use fe_command::Value;
use kiln_shell::{
    CloseKind, CloseRequest, Closing, DockPosition, ItemHandle, ModalEvent, ModalId, ModalLayer,
    PaneView as _, Shell,
    close::{self, Items, LiveItems},
    layout::{self, WindowLayout},
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
    /// A file to open in a tab: its path, and its buffer if it's read by
    /// now. Without the buffer, the tab shows it loading until the host
    /// hands the read to [`Workbench::file_read`].
    pub file: Option<(PathBuf, Option<Buffer>)>,
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
}

impl Palette {
    const ALL: [Palette; 7] = [
        Palette::QuickOpen,
        Palette::Language,
        Palette::Toolbar,
        Palette::Definitions,
        Palette::Symbols,
        Palette::ConfigProblems,
        Palette::GoToLine,
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

pub struct Workbench {
    panel: Entity<ProjectPanel>,
    dock: Entity<Dock>,
    panes: PaneGroup,
    /// The pane files open in; its active tab is the active editor.
    active: Entity<Pane>,
    /// `panes`' panes, which each of them sees.
    peers: Peers,
    /// Per pane: its notifications, its events, tabs dropped on it and its
    /// toolbar's requests.
    pane_subscriptions: HashMap<EntityId, [Subscription; 4]>,
    /// Per open editor: its notifications, passed on to the editors that
    /// share its buffer.
    editors: HashMap<EntityId, Subscription>,
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
    /// The window's shell: the chrome, the dock, the status bar and the
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
    /// To the modal layer's palettes coming and going.
    _modals: Subscription,
    /// To the status bar, whose vim line shows in its place even while
    /// it's hidden, and gives the host commands typed on it.
    _status_bar: [Subscription; 2],
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
    /// The window's saved layout, once the host gives it one.
    layout: Option<WindowLayout>,
    /// A saved layout's items are still being restored: nothing is saved
    /// until they're in.
    restoring: bool,
    /// The window's bounds as last drawn, for the layout saved at quit.
    bounds: Option<WindowBounds>,
    /// Saves the layout at quit, and keeps or deletes it as the window
    /// closes.
    _layout_hooks: Vec<Subscription>,
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
        let ui = UiTheme::default_dark();
        let welcome = launch.root.is_none() && launch.file.is_none();
        let root = launch.root.clone();
        let launch_root = launch.root.clone();
        let as_folder = launch.file.is_none();
        let launch_read = match &launch.file {
            Some((path, None)) => Some(path.clone()),
            _ => None,
        };
        let panel = cx.new(|cx| ProjectPanel::new(launch.root, ui, cx));
        let pane = cx.new(|cx| {
            let mut pane = Pane::new(theme, settings, ui, cx);
            match launch.file {
                None if welcome => pane.add(EditorModel::new(Buffer::new(WELCOME), None), cx),
                None => {}
                Some((path, Some(buffer))) => pane.add(EditorModel::new(buffer, Some(path)), cx),
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
                ui,
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
        let nav_state = Nav::new(launch_root.clone(), ui, cx);
        let status_bar = cx.new(|_| StatusBar::new(ui));
        let shell = cx.new(|cx| {
            let mut shell = Shell::new(KEY_CONTEXT, ui, window, cx).text_size(px(12.));
            shell.set_title(title_bar::name(launch_root.as_deref()), cx);
            shell.set_dock(DockPosition::Right, Some(dock.clone().into()), cx);
            shell.set_status_bar(Some(status_bar.clone().into()), cx);
            shell
        });
        let centre = cx.weak_entity();
        let centre = cx.new(|cx| PaneArea {
            workbench: centre,
            focus: cx.focus_handle(),
        });
        shell.update(cx, |shell, cx| shell.set_center(centre, cx));
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
        let mut workbench = Self {
            panel,
            dock,
            panes: PaneGroup::new(pane.clone()),
            active: pane.clone(),
            peers: Peers::default(),
            pane_subscriptions: HashMap::new(),
            editors: HashMap::new(),
            echoes: HashSet::new(),
            quick_open,
            language_picker,
            config_problems,
            _config_problems: Self::follow_config_problems(window, cx),
            go_to_line,
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
            _modals: modals_changed,
            _status_bar: [status_bar_changed, vim_host_commands],
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
            layout: None,
            restoring: false,
            bounds: None,
            _layout_hooks: Vec::new(),
        };
        // Under test, the chrome's window moves are recorded, not made.
        #[cfg(test)]
        title_bar::record_requests(cx);
        workbench.watch(&pane, window, cx);
        workbench.track_editors(cx);
        Self::follow_window_items(window, cx);
        workbench.follow_trust(window, cx);
        workbench.resolve_trust(workbench.root.clone(), as_folder, window, cx);
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

    /// Subscribes to a root's project panel.
    fn follow_root(
        panel: &Entity<ProjectPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(
            panel,
            window,
            |workbench, _, event, window, cx| match event {
                PanelEvent::OpenFile(path) => workbench
                    .active
                    .update(cx, |pane, cx| pane.open(path.clone(), window, cx)),
                PanelEvent::FsChanged(changes) => {
                    workbench
                        .quick_open
                        .update(cx, |picker, cx| picker.delegate.fs_changed(changes, cx));
                    workbench.nav_state.fs_changed(changes, cx);
                    workbench
                        .git
                        .update(cx, |git, cx| git.fs_changed(changes, cx));
                }
            },
        )
    }

    /// Subscribes to a root's Changes panel: its rows open as the project
    /// panel's do.
    fn follow_changes(
        changes: &Entity<GitPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(
            changes,
            window,
            |workbench, _, GitPanelEvent::OpenFile(path), window, cx| {
                workbench
                    .active
                    .update(cx, |pane, cx| pane.open(path.clone(), window, cx))
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
        for pane in self.panes.panes() {
            let root = Some(root.clone());
            pane.update(cx, |pane, cx| {
                pane.update_toolbar(|toolbar| toolbar.root = root, cx)
            });
        }
        self.resolve_trust(Some(root.clone()), true, window, cx);
        self.track_editors(cx);
        let name = title_bar::name(Some(&root));
        self.shell.update(cx, |shell, cx| shell.set_title(name, cx));
        cx.emit(RootChanged(root));
        cx.notify();
    }

    /// Shows the open panel for `op`, then opens what's picked: a folder
    /// re-roots the workbench, and a file opens in the active pane as a
    /// tree click does, the root staying. Cancelling does nothing, and an
    /// error from the panel is logged and, off macOS, shown in a prompt
    /// ([`open_panel_failed`]). Asked again while the panel shows, it does
    /// nothing.
    fn prompt_open(&mut self, op: OpenOp, window: &mut Window, cx: &mut Context<Self>) {
        if op == OpenOp::Settings {
            return self.open_settings(window, cx);
        }
        if op == OpenOp::NewWindow {
            return open::open_window(self.root.clone(), cx);
        }
        if self.prompting {
            return;
        }
        self.prompting = true;
        let answer = cx.prompt_for_paths(op.prompt());
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
                    // Off macOS only, for now: there the panel is
                    // xdg-desktop-portal's, which a minimal desktop may lack.
                    // Whether macOS should show it too is still open.
                    if let Some(error) = failed.filter(|_| !cfg!(target_os = "macos")) {
                        let (message, detail) = open_panel_failed(&error);
                        // Answered by its one button; nothing waits on it.
                        drop(window.prompt(
                            PromptLevel::Critical,
                            message,
                            Some(&detail),
                            &[PromptButton::ok("OK")],
                            cx,
                        ));
                    }
                    match (op, path) {
                        (_, None) | (OpenOp::Settings | OpenOp::NewWindow, Some(_)) => {}
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

    /// Opens the user's `settings.toml` in the active pane, creating it from
    /// the template first, in the background, if it's missing.
    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = open::settings_path(cx) else {
            log::error!("can't open settings: there's no config directory");
            return;
        };
        let created = cx.background_spawn({
            let path = path.clone();
            async move { open::create_settings(&path) }
        });
        cx.spawn_in(window, async move |workbench, cx| {
            if let Err(error) = created.await {
                log::error!("can't create {}: {error}", path.display());
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

    /// The right dock, which shows the project panel, search or the outline.
    pub fn dock(&self) -> &Entity<Dock> {
        &self.dock
    }

    /// The active pane, which files open in.
    pub fn pane(&self) -> &Entity<Pane> {
        &self.active
    }

    /// Every pane, as laid out.
    pub fn pane_group(&self) -> &PaneGroup {
        &self.panes
    }

    pub(crate) fn pane_group_mut(&mut self) -> &mut PaneGroup {
        &mut self.panes
    }

    /// Joins `pane` to the workbench's: it sees the others, and retitles
    /// the window, closes once emptied and becomes active on a click.
    fn watch(&mut self, pane: &Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) {
        pane.update(cx, |pane, cx| {
            pane.set_peers(self.peers.clone());
            pane.update_toolbar(
                |toolbar| {
                    toolbar.visible = self.breadcrumbs;
                    toolbar.nav = self.nav;
                    toolbar.root = self.root.clone();
                },
                cx,
            );
        });
        let changed = cx.observe_in(pane, window, Self::pane_changed);
        let events = cx.subscribe_in(
            pane,
            window,
            |workbench, pane, event, window, cx| match event {
                PaneEvent::Activated if *pane != workbench.active => {
                    workbench.activate(pane.clone(), window, cx)
                }
                PaneEvent::Activated => {}
            },
        );
        let dropped = cx.subscribe_in(
            pane,
            window,
            |workbench, _, dropped: &TabDropped, window, cx| {
                let Some(source) = dropped.dragged.pane.upgrade() else {
                    return;
                };
                let (tab, destination) = (dropped.dragged.item, dropped.destination.clone());
                workbench.move_tab_to(&source, tab, destination, window, cx);
            },
        );
        let requested = cx.subscribe_in(
            pane,
            window,
            |workbench, pane, request: &ToolbarRequest, window, cx| {
                workbench.open_toolbar_picker(pane, request.clone(), window, cx)
            },
        );
        self.pane_subscriptions
            .insert(pane.entity_id(), [changed, events, dropped, requested]);
        self.sync_peers();
    }

    fn sync_peers(&self) {
        *self.peers.borrow_mut() = self
            .panes
            .panes()
            .into_iter()
            .map(Entity::downgrade)
            .collect();
    }

    /// A tab opened, closed or changed in `pane`: retitles the window, and
    /// closes the pane if its last tab closed.
    fn pane_changed(&mut self, pane: Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) {
        if pane.update(cx, |pane, _| pane.take_emptied()) {
            self.close_pane(&pane, window, cx);
        }
        self.track_editors(cx);
        cx.notify();
    }

    /// Makes `pane` the active pane, which files open in, and focuses it.
    fn activate(&mut self, pane: Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) {
        let weak = pane.downgrade();
        self.quick_open
            .update(cx, |picker, _| picker.delegate.set_pane(weak.clone()));
        let search = self.dock.read(cx).search().clone();
        search.update(cx, |search, _| search.set_pane(weak));
        let focus = pane.read(cx).focus_target(cx);
        self.active = pane;
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Does a pane command.
    fn run_pane_op(&mut self, op: PaneOp, window: &mut Window, cx: &mut Context<Self>) {
        match op {
            PaneOp::Split(direction) => self.split(direction, window, cx),
            PaneOp::Focus(direction) => {
                if let Some(pane) = self.panes.neighbour(&self.active, direction, cx) {
                    self.activate(pane, window, cx);
                }
            }
            PaneOp::MoveTab(direction) => self.move_tab(direction, window, cx),
            PaneOp::Close => self.close_active_pane(window, cx),
        }
    }

    /// Opens a pane in `direction` of the active one and activates it. If
    /// the active pane shows a tab, the new pane gets one for the same file,
    /// sharing its buffer.
    fn split(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let item = (self.active.read(cx).active_item()).map(ItemHandle::boxed_clone);
        let split = item.and_then(|item| item.split(window, cx));
        let new = self.new_pane(direction, window, cx);
        if let Some(split) = split {
            new.update(cx, |pane, cx| pane.add_item(split, cx));
        }
        self.activate(new, window, cx);
        self.track_editors(cx);
    }

    /// An empty pane in `direction` of the active one.
    fn new_pane(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Pane> {
        let active = self.active.clone();
        let new = active.update(cx, |pane, cx| cx.new(|cx| pane.new_peer(cx)));
        self.panes.split(&active, new.clone(), direction);
        self.watch(&new, window, cx);
        new
    }

    /// Moves the active tab to the pane in `direction`, or to a pane split
    /// that way if there's none: see [`PaneGroup::move_tab`].
    fn move_tab(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let source = self.active.clone();
        let pane = source.read(cx);
        let Some(ix) = pane.active() else {
            return;
        };
        if pane.active_editor().is_none() {
            // A file being read, or a read error, shows in the tab's place.
            return;
        }
        let tab = pane.tabs()[ix].id();
        let destination = match self.panes.neighbour(&source, direction, cx) {
            Some(pane) => Destination::Pane { pane, index: None },
            None => Destination::Split {
                pane: source.clone(),
                direction,
            },
        };
        self.move_tab_to(&source, tab, destination, window, cx);
    }

    /// Moves `tab` from `source` to `destination`, by key or drop, then
    /// activates the pane it went to and closes `source` if that emptied it.
    fn move_tab_to(
        &mut self,
        source: &Entity<Pane>,
        tab: TabId,
        destination: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((target, new)) = self.panes.move_tab(source, tab, destination, window, cx) else {
            return;
        };
        if new {
            self.watch(&target, window, cx);
        }
        self.activate(target, window, cx);
        if source.update(cx, |pane, _| pane.take_emptied()) {
            self.close_pane(source, window, cx);
        }
        self.track_editors(cx);
    }

    /// Closes the active pane with its tabs, unless it's the last, once its
    /// tabs agree ([`kiln_shell::close`]): each is asked unless another
    /// pane shows its file, and a dirty one asks whether to discard its
    /// changes.
    fn close_active_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panes.panes().len() < 2 {
            return;
        }
        let pane = self.active.clone();
        let now = Items {
            closing: pane.read(cx).pane().items().to_vec(),
            open: self.panes.items(cx),
        };
        let (weak, workbench) = (pane.downgrade(), cx.weak_entity());
        let live: LiveItems = Rc::new(move |cx| {
            let pane = weak.upgrade()?;
            let workbench = workbench.upgrade()?;
            Some(Items {
                closing: pane.read(cx).pane().items().to_vec(),
                open: workbench.read(cx).panes.items(cx),
            })
        });
        let request =
            CloseRequest::new(CloseKind::Pane).scope(window.window_handle(), Some(now), live);
        let (closed, workbench) = (pane.clone(), cx.weak_entity());
        let commit = move |window: Option<&mut Window>, cx: &mut App| {
            if let Some(window) = window {
                (workbench.update(cx, |workbench, cx| {
                    workbench.close_pane(&closed, window, cx)
                }))
                .ok();
            }
        };
        match close::confirm(request, commit, Some(window), cx) {
            Closing::Now => self.close_pane(&pane, window, cx),
            Closing::Refused => {}
            Closing::Later(closing) => closing.detach(),
        }
    }

    /// Every editor of every pane, for the window's close and quitting to
    /// ask.
    fn follow_window_items(window: &mut Window, cx: &mut Context<Self>) {
        let workbench = cx.weak_entity();
        let items: kiln_shell::close::WindowItems = Rc::new(move |cx| {
            workbench
                .upgrade()
                .map(|workbench| workbench.read(cx).panes.items(cx))
                .unwrap_or_default()
        });
        close::set_window_items(window, items, cx);
    }

    /// Closes `pane`, unless it's the last, activating the nearest pane if it
    /// was active.
    fn close_pane(&mut self, pane: &Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) {
        if self.panes.panes().len() < 2 {
            return;
        }
        let nearest = self.panes.nearest(pane, cx);
        self.panes.remove(pane);
        self.pane_subscriptions.remove(&pane.entity_id());
        self.sync_peers();
        if *pane == self.active
            && let Some(nearest) = nearest
        {
            self.activate(nearest, window, cx);
        }
        self.track_editors(cx);
        cx.notify();
    }

    /// Observes each open editor, and stops observing closed ones. A newly
    /// observed editor gets its diff base.
    fn track_editors(&mut self, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        let open: HashSet<EntityId> = editors.iter().map(Entity::entity_id).collect();
        self.editors.retain(|id, _| open.contains(id));
        self.echoes.retain(|id| open.contains(id));
        for editor in &editors {
            self.editors
                .entry(editor.entity_id())
                .or_insert_with(|| cx.observe(editor, Self::editor_changed));
        }
        self.git.update(cx, |git, cx| git.track(&editors, cx));
    }

    #[cfg(test)]
    pub(crate) fn git(&self) -> &Entity<GitState> {
        &self.git
    }

    pub(crate) fn open_editors(&self, cx: &App) -> Vec<Entity<EditorView>> {
        self.panes
            .panes()
            .into_iter()
            .flat_map(|pane| pane.read(cx).tabs().iter().map(|tab| tab.editor.clone()))
            .collect()
    }

    /// Notifies every other editor sharing `editor`'s buffer, so an edit in
    /// one pane shows in the others on the next frame. Each keeps its own
    /// selections, scroll and folds.
    fn editor_changed(&mut self, editor: Entity<EditorView>, cx: &mut Context<Self>) {
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

    /// Saves the window's layout from now on, in `layout`, as it changes
    /// (a split, a close, a move, a dock toggle, a tab opening or
    /// closing, a divider's drag), and in full as the app quits. As the
    /// window closes, its entry is kept if it's the last window, else
    /// deleted ([`kiln_shell::layout`]).
    pub fn set_layout(&mut self, layout: WindowLayout, window: &Window, cx: &mut Context<Self>) {
        self.layout = Some(layout);
        let at_quit = cx.on_app_quit(|workbench, cx| {
            let entry = workbench.layout_entry(cx);
            let saved = workbench
                .layout
                .as_mut()
                .map(|layout| layout.save_at_quit(entry, cx));
            async move {
                if let Some(saved) = saved {
                    saved.await;
                }
            }
        });
        // The window closes, or the workbench goes: whichever is first.
        let ours = window.window_handle().window_id();
        let workbench = cx.weak_entity();
        let closed = cx.on_window_closed(move |cx, closed| {
            if closed == ours {
                workbench
                    .update(cx, |workbench, cx| workbench.close_layout(cx))
                    .ok();
            }
        });
        let released = cx.on_release(|workbench, cx| workbench.close_layout(cx));
        self._layout_hooks = vec![at_quit, closed, released];
    }

    /// The window has closed: its entry is kept if no other window is open,
    /// else deleted.
    fn close_layout(&mut self, cx: &mut App) {
        if let Some(layout) = self.layout.take() {
            let entry = self.layout_entry(cx);
            let others_open = !cx.windows().is_empty();
            layout.close(entry, others_open, cx);
        }
    }

    /// The window's layout id, once it has a layout.
    pub fn layout_id(&self) -> Option<u64> {
        self.layout.as_ref().and_then(WindowLayout::id)
    }

    /// Whether a restored layout's files are still being read.
    pub fn restoring(&self) -> bool {
        self.restoring
    }

    /// The window's layout, as saved: its bounds, its panes and their
    /// files, the dock and whether the breadcrumbs show.
    pub fn layout_entry(&self, cx: &App) -> Value {
        let mut entry = Value::from_iter([
            ("panes", layout::save_panes(&self.panes, &self.active, cx)),
            ("dock", self.dock.read(cx).state().save()),
            ("breadcrumbs", Value::from(self.breadcrumbs)),
        ]);
        if let (Some(bounds), Some(object)) = (self.bounds, entry.as_object_mut()) {
            object.insert("bounds".into(), layout::save_bounds(bounds));
        }
        entry
    }

    /// Puts the window back as `entry` saved it: the dock and the
    /// breadcrumbs at once; the panes, split as they were, with their
    /// files read in the background, the views of one file sharing its
    /// buffer, each with its caret. A file that's gone is left out, and a
    /// pane left empty closes, unless it's the only one.
    pub fn restore_layout(&mut self, entry: &Value, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(saved) = entry.get("dock") {
            self.dock.update(cx, |dock, cx| dock.restore(saved, cx));
        }
        let breadcrumbs = entry.get("breadcrumbs").and_then(Value::as_bool);
        if breadcrumbs.is_some_and(|shown| shown != self.breadcrumbs) {
            self.toggle_breadcrumbs(cx);
        }
        let Some(saved) = entry.get("panes") else {
            return;
        };
        let template = self.active.clone();
        let mut new_pane = |_: &mut Window, cx: &mut App| {
            template.update(cx, |pane, cx| cx.new(|cx| pane.new_peer(cx)))
        };
        let Some(restored) = layout::restore_panes(saved, &mut new_pane, window, cx) else {
            return;
        };
        for pane in self.panes.panes() {
            self.pane_subscriptions.remove(&pane.entity_id());
        }
        self.panes.set_root(restored.root);
        let panes: Vec<_> = self.panes.panes().into_iter().cloned().collect();
        for pane in &panes {
            self.watch(pane, window, cx);
        }
        let active = restored.active;
        self.activate(active.clone(), window, cx);
        self.restoring = true;
        let filled = restored.filled;
        cx.spawn_in(window, async move |workbench, cx| {
            let empty = filled.await;
            workbench
                .update_in(cx, |workbench, window, cx| {
                    workbench.restoring = false;
                    for pane in &empty {
                        workbench.close_pane(pane, window, cx);
                    }
                    let open = workbench.panes.panes();
                    let active = match open.contains(&&active) {
                        true => active,
                        false => open[0].clone(),
                    };
                    workbench.activate(active, window, cx);
                    workbench.track_editors(cx);
                })
                .ok();
        })
        .detach();
        self.track_editors(cx);
        cx.notify();
    }

    /// Saves the layout if its shape changed, unless it's still being
    /// restored.
    fn save_layout(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.bounds = Some(window.window_bounds());
        if self.layout.is_none() || self.restoring {
            return;
        }
        let entry = self.layout_entry(cx);
        if let Some(layout) = self.layout.as_mut() {
            layout.save_if_changed(entry, cx);
        }
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
    fn toggle_quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    fn select_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    fn toggle_go_to_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    fn toggle_breadcrumbs(&mut self, cx: &mut Context<Self>) {
        self.breadcrumbs = !self.breadcrumbs;
        self.chain_key = None;
        let visible = self.breadcrumbs;
        for pane in self.panes.panes() {
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
        for pane in self.panes.panes() {
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

    fn run_status_op(&mut self, op: StatusOp, window: &mut Window, cx: &mut Context<Self>) {
        match op {
            StatusOp::ToggleStatusBar => {
                self.set_status_bar_visible(!self.status_bar_visible, cx);
                persist::save_status_bar(self.status_bar_visible, cx);
            }
            StatusOp::SelectLanguage => self.select_language(window, cx),
            StatusOp::CancelVimLine => {}
        }
    }

    /// The active tab's editor, unless a file being read or a read error
    /// shows in its place.
    pub fn active_editor<'a>(&self, cx: &'a App) -> Option<&'a Entity<EditorView>> {
        self.active.read(cx).active_editor()
    }

    /// Tells the outline and the status bar which editor is active, if that
    /// changed, and has the project panel reveal its file. Every tab change
    /// redraws, though not every one notifies (focusing an open tab only
    /// focuses it), so the workbench calls it as it renders. With splits,
    /// it's the active pane's active editor. Whether the status bar's editor
    /// changed.
    fn follow_active_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let editor = self.active_editor(cx).cloned();
        let outline = self.dock.read(cx).outline().clone();
        outline.update(cx, |outline, cx| outline.set_editor(editor.as_ref(), cx));
        self.push_symbols(editor.as_ref(), cx);
        let id = editor.as_ref().map(Entity::entity_id);
        let item = self
            .active
            .read(cx)
            .active_item()
            .map(ItemHandle::boxed_clone);
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

    /// Opens the host's read of the launch file, `Ok(None)` being a new
    /// file, in the pane it was launched in, whatever that pane has shown
    /// since; see [`Launch::file`]. If that pane has closed, the read is
    /// dropped. Any other read opens in the pane showing it being read, else
    /// the active pane.
    pub fn file_read(
        &mut self,
        path: PathBuf,
        result: std::io::Result<Option<Buffer>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = match self.launch_read.take_if(|(launched, _)| *launched == path) {
            Some((_, pane)) => {
                let open = self.panes.panes();
                let pane = pane.upgrade().filter(|pane| open.contains(&pane));
                let Some(pane) = pane else {
                    log::info!("not opening {}: its pane closed", path.display());
                    return;
                };
                pane
            }
            None => self
                .panes
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
        for pane in self.panes.panes() {
            pane.update(cx, |pane, cx| pane.reload_settings(event, cx));
        }
        // Editors restored from a saved layout open as the panes' do.
        let theme = self.active.read(cx).theme().clone();
        fe_gpui::set_restore_style(cx, theme, event.settings.editor.clone());
        self.reload_lsp_settings(&event.settings.lsp, cx);
    }

    /// Runs a `workbench.*` command on the active pane, a pane command, a
    /// palette toggle, a status bar command or a dock command; others go on
    /// to the app.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(test)]
        self.commands_run.push(run.0.clone());
        let Some(registry) =
            crate::registry(cx).filter(|registry| registry.get(&run.0.command).is_some())
        else {
            if let Some(op) = open::op(run, cx) {
                return self.prompt_open(op, window, cx);
            }
            if let Some(trusted) = trust::op(run, cx) {
                return self.set_root_trust(trusted, cx);
            }
            match picker::toggled(run, cx) {
                Some(Toggle::QuickOpen) => return self.toggle_quick_open(window, cx),
                Some(Toggle::GoToLine) => return self.toggle_go_to_line(window, cx),
                None => {}
            }
            if config_problems::shows(run, cx) {
                return self.show_config_problems(window, cx);
            }
            if let Some(op) = nav::op(run, cx) {
                return self.run_nav_op(op, window, cx);
            }
            if self.run_references_command(run, window, cx) {
                return;
            }
            if let Some(op) = status_bar::op(run, cx) {
                return self.run_status_op(op, window, cx);
            }
            if let Some(op) = lsp::op(run, cx) {
                return self.run_lsp_op(op, window, cx);
            }
            if let Some(op) = pane_group::op(run, cx) {
                return self.run_pane_op(op, window, cx);
            }
            if project_panel::op(run, cx) == Some(PanelOp::RevealActive) {
                return self.reveal_active(cx);
            }
            if pane_toolbar::toggles(run, cx) {
                return self.toggle_breadcrumbs(cx);
            }
            let op = if outline_panel::shows(run, cx) {
                Some(DockOp::ShowOutline)
            } else {
                dock::op(run, cx)
            };
            let done = op.is_some_and(|op| self.run_dock_op(op, window, cx));
            if !done {
                cx.propagate();
            }
            return;
        };
        let result = self.active.update(cx, |pane, cx| {
            let result = registry.dispatch(&mut (pane, window, &mut **cx), &run.0);
            cx.notify();
            result
        });
        if let Err(error) = result {
            log::warn!("{error}");
        }
    }
}

impl Focusable for Workbench {
    /// The active editor's, or else the pane's, so the workbench's commands
    /// apply.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.active_editor(cx) {
            Some(editor) => editor.focus_handle(cx),
            None => self.active.focus_handle(cx),
        }
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&self.active.read(cx).title(cx));
        let followed = self.follow_active_editor(window, cx);
        self.save_layout(window, cx);
        self.shell.update(cx, |shell, _| {
            // Not from its cache in a frame that gave it another editor: a
            // notification during a draw doesn't reach this frame's cache,
            // nor ask for another frame.
            if followed {
                shell.redraw_status_bar();
            }
            #[cfg(test)]
            shell.set_client_tiling(self.client_tiling);
        });
        div()
            .on_action(cx.listener(Self::on_run_command))
            // So a modifier pressed while a panel has focus reaches the
            // editors, for their cmd-click hover.
            .on_modifiers_changed(EditorView::modifiers_changed)
            .size_full()
            .child(self.shell.clone())
    }
}

/// The shell's centre: the panes, which the workbench holds and lays out.
/// Its focus is the workbench's, the active editor's.
struct PaneArea {
    workbench: WeakEntity<Workbench>,
    /// Focused only once the workbench has gone.
    focus: FocusHandle,
}

impl Render for PaneArea {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(workbench) = self.workbench.upgrade() else {
            return div().into_any_element();
        };
        workbench.update(cx, |workbench, cx| {
            workbench.panes.render(&workbench.ui, cx)
        })
    }
}

impl Focusable for PaneArea {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.workbench.upgrade() {
            Some(workbench) => workbench.read(cx).focus_handle(cx),
            None => self.focus.clone(),
        }
    }
}

/// The prompt shown when the open panel fails with `error`: see
/// [`kiln_platform::dialogs::open_panel_failed`].
pub(crate) fn open_panel_failed(error: &str) -> (&'static str, String) {
    kiln_platform::dialogs::open_panel_failed(error, kiln_platform::Os::current())
}

#[path = "vim_host.rs"]
mod vim_host;

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
#[path = "windows_tests.rs"]
mod windows_tests;
