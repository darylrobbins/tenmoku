//! The window shell: the editor panes, with the dock (the project panel,
//! search or the outline) on the right.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, SettingsEvent, commands::RunCommand};
use fe_ui::{ROW_HEIGHT, UiTheme};
use gpui::{
    App, AppContext as _, Context, DismissEvent, Entity, EntityId, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement as _, PromptButton, PromptLevel,
    Render, StyleRefinement, Styled as _, Subscription, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    Dock, DockPanel, KEY_CONTEXT, MenuBarView, OutlinePanel, Pane, PaneGroup, Picker, ProjectPanel,
    QuickOpen, ReferencesPanel, SearchPanel, StatusBar,
    dock::{self, DockEvent, DockOp},
    language_picker::LanguagePicker,
    lsp::{self, Lsp},
    nav::{self, Nav},
    open::{self, OpenOp},
    outline_panel,
    pane::{PaneEvent, Peers, TabId, file_name},
    pane_drag::TabDropped,
    pane_group::{self, Destination, Direction, PaneOp},
    pane_toolbar::{self, NavButtons, SymbolChain, ToolbarRequest},
    persist, picker,
    project_panel::{self, PanelEvent, PanelOp},
    status_bar::{self, StatusOp},
    title_bar,
    toolbar_picker::ToolbarPicker,
    trust::{self, RootTrust, TrustChanged},
};
use kiln_shell::{
    ItemHandle, TitleBar,
    window_frame::{self, TopRow},
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

/// The palette shown over the window: one at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Palette {
    QuickOpen,
    Language,
    Toolbar,
    Definitions,
    Symbols,
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
    /// The palette that shows, if one does.
    pub(crate) palette: Option<Palette>,
    /// Go to definition, workspace symbols, back and forward.
    pub(crate) nav_state: Nav,
    status_bar: Entity<StatusBar>,
    status_bar_visible: bool,
    title_bar: bool,
    /// The menu bar drawn in the window, where the platform has no native
    /// one: see [`Workbench::set_menu_bar`].
    menu_bar: Option<Entity<MenuBarView>>,
    /// Whether the workbench draws the window's frame when the platform
    /// asks the app to: see [`Workbench::set_window_frame`].
    window_frame: bool,
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
    pub(crate) trust: RootTrust,
    /// The root's language servers.
    pub(crate) lsp: Lsp,
    ui: UiTheme,
    /// To the project panel's and the palette's events: replaced with them
    /// on a new root.
    root_subscriptions: [Subscription; 3],
    _rail: Subscription,
    /// To the language picker's dismissal and blur.
    _language_picker: [Subscription; 2],
    /// What the panes' toolbar crumbs and "…" list.
    toolbar_picker: Entity<Picker<ToolbarPicker>>,
    _toolbar_picker: [Subscription; 2],
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
        let outline = cx.new(|_| OutlinePanel::new(ui));
        let references = cx.weak_entity();
        let references = cx.new(|cx| ReferencesPanel::new(references, ui, cx));
        let dock = cx.new(|cx| {
            let search = cx.new(|cx| SearchPanel::new(root.clone(), pane.downgrade(), ui, cx));
            Dock::new(panel.clone(), search, outline.clone(), references, ui)
        });
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
        let root_subscriptions = Self::follow_root(&panel, &quick_open, window, cx);
        let language_picker =
            cx.new(|cx| Picker::new(LanguagePicker::new(ui), "Select a language…", ui, cx));
        let language_subscriptions =
            Self::follow_palette(&language_picker, Palette::Language, window, cx);
        let toolbar_picker = cx.new(|cx| {
            let delegate = ToolbarPicker::new(outline.downgrade(), ui);
            Picker::new(delegate, "Filter…", ui, cx)
        });
        let toolbar_subscriptions =
            Self::follow_palette(&toolbar_picker, Palette::Toolbar, window, cx);
        let nav_state = Nav::new(launch_root.clone(), ui, window, cx);
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
            palette: None,
            nav_state,
            status_bar: cx.new(|_| StatusBar::new(ui)),
            status_bar_visible: true,
            title_bar: false,
            menu_bar: None,
            window_frame: false,
            #[cfg(test)]
            client_tiling: None,
            root: launch_root,
            revealed: None,
            launch_read: launch_read.map(|path| (path, pane.downgrade())),
            prompting: false,
            trust: RootTrust::default(),
            lsp: Lsp::new(),
            ui,
            root_subscriptions,
            _rail: rail,
            _language_picker: language_subscriptions,
            toolbar_picker,
            _toolbar_picker: toolbar_subscriptions,
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
        };
        // Under test, the chrome's window moves are recorded, not made.
        #[cfg(test)]
        title_bar::record_requests(cx);
        workbench.watch(&pane, window, cx);
        workbench.track_editors(cx);
        workbench.follow_trust(window, cx);
        workbench.resolve_trust(workbench.root.clone(), as_folder, window, cx);
        workbench.start_lsp(window, cx);
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

    /// Subscribes to a root's project panel and quick-open palette.
    fn follow_root(
        panel: &Entity<ProjectPanel>,
        quick_open: &Entity<Picker<QuickOpen>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> [Subscription; 3] {
        let open = cx.subscribe_in(
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
                }
            },
        );
        let [dismissed, left] = Self::follow_palette(quick_open, Palette::QuickOpen, window, cx);
        [open, dismissed, left]
    }

    /// Hides `picker`, shown as `palette`, once it's picked from, dismissed
    /// or left. Its events once another palette shows instead are ignored.
    pub(crate) fn follow_palette<D: crate::PickerDelegate>(
        picker: &Entity<Picker<D>>,
        palette: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> [Subscription; 2] {
        // Focus is elsewhere by then, unless nothing was focused before it
        // opened.
        let dismissed = cx.subscribe_in(
            picker,
            window,
            move |workbench, picker, _: &DismissEvent, window, cx| {
                if picker.focus_handle(cx).is_focused(window) {
                    window.focus(&workbench.focus_handle(cx), cx);
                }
                // Not once another palette shows instead, its input focused.
                if workbench.palette == Some(palette) {
                    workbench.palette = None;
                }
                cx.notify();
            },
        );
        // Left for another element, or the window deactivated: GPUI blurs
        // then too, focus staying on the input. That's dismissed as Escape
        // would be, so focus doesn't stay on an input no longer drawn.
        let weak = picker.downgrade();
        let left = cx.on_blur(
            &picker.focus_handle(cx),
            window,
            move |workbench, window, cx| {
                if workbench.palette != Some(palette) {
                    return;
                }
                let Some(picker) = weak.upgrade() else {
                    return;
                };
                if picker.focus_handle(cx).is_focused(window) {
                    picker.update(cx, |picker, cx| picker.dismiss(window, cx));
                }
                workbench.palette = None;
                cx.notify();
            },
        );
        [dismissed, left]
    }

    /// Re-roots the workbench at `root`, in this window: a new project
    /// panel lists it in the background, and a new quick-open palette
    /// indexes it once opened. The old ones are dropped, and with them the
    /// old tree, its watcher, and the old index's walk; their batches land
    /// nowhere. Search searches `root` from now on, its running search
    /// cancelled and its results cleared, the query and options kept. The
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
            // Dismissed as Escape would be, as the palette's subscriptions
            // go with it.
            let picker = self.quick_open.clone();
            picker.update(cx, |picker, cx| picker.dismiss(window, cx));
            if picker.focus_handle(cx).contains_focused(window, cx) {
                window.focus(&self.focus_handle(cx), cx);
            }
            self.palette = None;
        }
        let panel_focused = self.panel.focus_handle(cx).contains_focused(window, cx);
        let ui = self.ui;
        let panel = cx.new(|cx| ProjectPanel::new(Some(root.clone()), ui, cx));
        let quick_open = Self::new_quick_open(Some(root.clone()), self.active.downgrade(), ui, cx);
        self.root_subscriptions = Self::follow_root(&panel, &quick_open, window, cx);
        let search = self.dock.update(cx, |dock, cx| {
            dock.set_project(panel.clone(), cx);
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
                        (_, None) | (OpenOp::Settings, Some(_)) => {}
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
                let (tab, destination) = (dropped.dragged.tab, dropped.destination.clone());
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
        let model = self
            .active
            .read(cx)
            .active_editor()
            .map(|editor| editor.read(cx).model().split());
        let new = self.new_pane(direction, window, cx);
        if let Some(model) = model {
            new.update(cx, |pane, cx| pane.add(model, cx));
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

    /// Closes the active pane with its tabs, unless it's the last, or one of
    /// its tabs has unsaved changes no other pane shows.
    fn close_active_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.active.clone();
        let unsaved = {
            let pane = pane.read(cx);
            pane.tabs().iter().find_map(|tab| {
                let model = tab.editor.read(cx).model();
                (model.is_dirty() && !pane.shown_in_peers(model, cx)).then(|| model.path())
            })
        };
        if let Some(path) = unsaved {
            log::warn!(
                "not closing the pane: {} has unsaved changes",
                file_name(path)
            );
            return;
        }
        self.close_pane(&pane, window, cx);
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

    /// Observes each open editor, and stops observing closed ones.
    fn track_editors(&mut self, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        let open: HashSet<EntityId> = editors.iter().map(Entity::entity_id).collect();
        self.editors.retain(|id, _| open.contains(id));
        self.echoes.retain(|id| open.contains(id));
        for editor in editors {
            self.editors
                .entry(editor.entity_id())
                .or_insert_with(|| cx.observe(&editor, Self::editor_changed));
        }
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
        self.quick_open.update(cx, |picker, cx| {
            if show {
                picker.delegate.opened(previous, cx);
                picker.show(window, cx);
            } else {
                picker.dismiss(window, cx);
            }
        });
        self.palette = show.then_some(Palette::QuickOpen);
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
        self.language_picker.update(cx, |picker, cx| {
            picker.delegate.opened(&editor, cx);
            picker.show(window, cx);
        });
        self.palette = Some(Palette::Language);
        cx.notify();
    }

    /// Dismisses the palette that shows, as Escape would.
    pub(crate) fn dismiss_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.palette.take() {
            Some(Palette::QuickOpen) => self
                .quick_open
                .update(cx, |picker, cx| picker.dismiss(window, cx)),
            Some(Palette::Language) => self
                .language_picker
                .update(cx, |picker, cx| picker.dismiss(window, cx)),
            Some(Palette::Toolbar) => self
                .toolbar_picker
                .update(cx, |picker, cx| picker.dismiss(window, cx)),
            Some(Palette::Definitions) => (self.nav_state.definitions.clone())
                .update(cx, |picker, cx| picker.dismiss(window, cx)),
            Some(Palette::Symbols) => {
                (self.nav_state.symbols.clone()).update(cx, |picker, cx| picker.dismiss(window, cx))
            }
            None => {}
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
        self.toolbar_picker.update(cx, |picker, cx| {
            picker
                .delegate
                .open(request, pane, workbench, root, previous, cx);
            picker.show(window, cx);
        });
        self.palette = Some(Palette::Toolbar);
        cx.notify();
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

    /// Shows or hides the title bar the standalone app draws under its
    /// transparent macOS title bar: see [`crate::titlebar_options`]. Off by
    /// default.
    pub fn set_title_bar(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.title_bar = shown;
        cx.notify();
    }

    /// Shows or hides the menu bar drawn in the window (kiln-shell's), from
    /// the menus [`fe_gpui::menu::install`] set: for platforms with no
    /// native menu bar (Linux). Off by default.
    pub fn set_menu_bar(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.menu_bar = shown.then(|| cx.new(MenuBarView::new));
        cx.notify();
    }

    /// Draws the window's frame (title, buttons, resize edges) whenever the
    /// platform asks the app to, as GNOME on Wayland does; the window manager
    /// draws it otherwise. For a standalone app on Linux. Off by default.
    pub fn set_window_frame(&mut self, drawn: bool, cx: &mut Context<Self>) {
        self.window_frame = drawn;
        cx.notify();
    }

    /// The menu bar, while it's shown.
    pub fn menu_bar(&self) -> Option<&Entity<MenuBarView>> {
        self.menu_bar.as_ref()
    }

    /// `menu_bar.open`: opens the first menu of the menu bar, if it's shown.
    fn open_menu_bar(
        &mut self,
        _: &kiln_shell::menu_bar::Open,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(bar) = &self.menu_bar {
            bar.update(cx, |bar, cx| bar.open(0, true, window, cx));
        }
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
        cx.notify();
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

    /// Focuses the active editor if the project panel or References has
    /// focus but isn't shown, the dock being hidden or showing another
    /// panel.
    fn leave_hidden_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dock = self.dock.read(cx);
        let shown = |panel| dock.is_visible() && dock.active() == panel;
        let hidden = [
            (DockPanel::Project, self.panel.focus_handle(cx)),
            (DockPanel::References, dock.references().focus_handle(cx)),
        ]
        .into_iter()
        .any(|(panel, focus)| !shown(panel) && focus.contains_focused(window, cx));
        if hidden {
            window.focus(&self.focus_handle(cx), cx);
        }
    }

    /// Does a dock op, for a key or a rail click, with the active editor as
    /// the fallback focus: showing the project panel focuses it, and
    /// showing the outline leaves focus where it is unless on a hidden
    /// panel. False if the op is the search panel's to do.
    fn run_dock_op(&mut self, op: DockOp, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let fallback = self.focus_handle(cx);
        let done = self
            .dock
            .update(cx, |dock, cx| dock.run(op, &fallback, window, cx));
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
            if picker::toggles_quick_open(run, cx) {
                return self.toggle_quick_open(window, cx);
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
        let main = div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .child(self.panes.render(&self.ui, cx))
            .child(self.dock.clone());
        // Cached: frames the workbench redraws for other reasons reuse the
        // bar's last render. Not in a frame that gave it another editor: a
        // notification during a draw doesn't reach this frame's cache, nor
        // ask for another frame.
        let mut style = StyleRefinement::default().w_full().h(ROW_HEIGHT);
        style.flex_shrink = Some(0.);
        // Vim's command line shows in the bar's place even with it hidden.
        let line_open = self.status_bar.read(cx).line_open();
        let status_bar = (self.status_bar_visible || line_open).then(|| {
            let bar = self.status_bar.clone();
            if followed {
                bar.into_any_element()
            } else {
                bar.cached(style).into_any_element()
            }
        });
        #[cfg(test)]
        let client_tiling = self
            .client_tiling
            .or_else(|| window_frame::client_tiling(window, cx));
        #[cfg(not(test))]
        let client_tiling = window_frame::client_tiling(window, cx);
        let tiling = client_tiling.filter(|_| self.window_frame);
        let top_row = (self.menu_bar.is_some() || tiling.is_some()).then(|| {
            let row = TopRow::new(title_bar::name(self.root.as_deref()), tiling.is_some())
                .key_context(KEY_CONTEXT);
            match &self.menu_bar {
                Some(bar) => row.menu_bar(bar.clone()),
                None => row,
            }
        });
        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_run_command))
            .on_action(cx.listener(Self::open_menu_bar))
            // So a modifier pressed while a panel has focus reaches the
            // editors, for their cmd-click hover.
            .on_modifiers_changed(EditorView::modifiers_changed)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(self.ui.background)
            .text_color(self.ui.text)
            .text_size(px(12.))
            .children(
                self.title_bar
                    .then(|| TitleBar::new(title_bar::name(self.root.as_deref()))),
            )
            .children(top_row)
            .child(main)
            .children(status_bar)
            .map(|root| match self.palette {
                Some(Palette::QuickOpen) => root.child(self.quick_open.clone()),
                Some(Palette::Language) => root.child(self.language_picker.clone()),
                Some(Palette::Toolbar) => root.child(self.toolbar_picker.clone()),
                Some(Palette::Definitions) => root.child(self.nav_state.definitions.clone()),
                Some(Palette::Symbols) => root.child(self.nav_state.symbols.clone()),
                None => root,
            })
            // The frame's border and resize edges, over everything, unless
            // the window fills the screen.
            .when_some(tiling.filter(|tiling| !tiling.is_tiled()), |root, _| {
                root.border_1().border_color(self.ui.border)
            })
            .children(tiling.into_iter().flat_map(window_frame::resize_handles))
    }
}

/// The prompt shown when the open panel fails with `error`: see
/// [`kiln_platform::dialogs::open_panel_failed`].
pub(crate) fn open_panel_failed(error: &str) -> (&'static str, String) {
    kiln_platform::dialogs::open_panel_failed(error, kiln_platform::Os::current())
}
