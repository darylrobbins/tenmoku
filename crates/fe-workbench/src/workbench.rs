//! The window shell: the editor panes, with the dock (the project panel,
//! search or the outline) on the right.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, SettingsEvent, commands::RunCommand};
use fe_ui::UiTheme;
use gpui::{
    App, AppContext as _, Context, DismissEvent, Entity, EntityId, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, Subscription,
    WeakEntity, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    Dock, DockPanel, KEY_CONTEXT, OutlinePanel, Pane, PaneGroup, Picker, ProjectPanel, QuickOpen,
    SearchPanel, dock, outline_panel,
    pane::{PaneEvent, Peers, file_name},
    pane_group::{self, Direction, PaneOp},
    picker::{self, PaletteOp},
    project_panel::{self, PanelEvent, PanelOp},
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

pub struct Workbench {
    panel: Entity<ProjectPanel>,
    dock: Entity<Dock>,
    panes: PaneGroup,
    /// The pane files open in; its active tab is the active editor.
    active: Entity<Pane>,
    /// `panes`' panes, which each of them sees.
    peers: Peers,
    /// Per pane: its notifications and its events.
    pane_subscriptions: HashMap<EntityId, [Subscription; 2]>,
    /// Per open editor: its notifications, passed on to the editors that
    /// share its buffer.
    editors: HashMap<EntityId, Subscription>,
    /// Editors notified because an editor sharing their buffer was, so their
    /// own notification isn't passed back.
    echoes: HashSet<EntityId>,
    quick_open: Entity<Picker<QuickOpen>>,
    /// Whether the quick-open palette shows.
    palette: bool,
    /// The editor whose file the project panel last revealed.
    revealed: Option<EntityId>,
    /// The launch file the host is reading, and the pane it opens in, until
    /// [`Workbench::file_read`] hands it over.
    launch_read: Option<(PathBuf, WeakEntity<Pane>)>,
    ui: UiTheme,
    _subscriptions: [Subscription; 3],
}

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
        let dock = cx.new(|cx| {
            let search = cx.new(|cx| SearchPanel::new(root.clone(), pane.downgrade(), ui, cx));
            let outline = cx.new(|_| OutlinePanel::new(ui));
            Dock::new(panel.clone(), search, outline, ui)
        });
        let quick_open = cx.new(|cx| {
            let delegate = QuickOpen::new(root, pane.downgrade(), ui);
            Picker::new(delegate, "Go to file…", ui, cx)
        });
        let open = cx.subscribe_in(
            &panel,
            window,
            |workbench, _, event, window, cx| match event {
                PanelEvent::OpenFile(path) => workbench
                    .active
                    .update(cx, |pane, cx| pane.open(path.clone(), window, cx)),
                PanelEvent::FsChanged(changes) => workbench
                    .quick_open
                    .update(cx, |picker, cx| picker.delegate.fs_changed(changes, cx)),
            },
        );
        // Hidden once picked from, dismissed or left: focus is elsewhere by
        // then, unless nothing was focused before it opened.
        let dismissed = cx.subscribe_in(
            &quick_open,
            window,
            |workbench, picker, _: &DismissEvent, window, cx| {
                if picker.focus_handle(cx).is_focused(window) {
                    window.focus(&workbench.focus_handle(cx), cx);
                }
                workbench.palette = false;
                cx.notify();
            },
        );
        // Left for another element, or the window deactivated: GPUI blurs
        // then too, focus staying on the input. That's dismissed as Escape
        // would be, so focus doesn't stay on an input no longer drawn.
        let left = cx.on_blur(
            &quick_open.focus_handle(cx),
            window,
            |workbench, window, cx| {
                if !workbench.palette {
                    return;
                }
                let picker = workbench.quick_open.clone();
                if picker.focus_handle(cx).is_focused(window) {
                    picker.update(cx, |picker, cx| picker.dismiss(window, cx));
                }
                workbench.palette = false;
                cx.notify();
            },
        );
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
            palette: false,
            revealed: None,
            launch_read: launch_read.map(|path| (path, pane.downgrade())),
            ui,
            _subscriptions: [open, dismissed, left],
        };
        workbench.watch(&pane, window, cx);
        workbench.track_editors(cx);
        workbench
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

    /// Joins `pane` to the workbench's: it sees the others, and retitles
    /// the window, closes once emptied and becomes active on a click.
    fn watch(&mut self, pane: &Entity<Pane>, window: &mut Window, cx: &mut Context<Self>) {
        pane.update(cx, |pane, _| pane.set_peers(self.peers.clone()));
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
        self.pane_subscriptions
            .insert(pane.entity_id(), [changed, events]);
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

    /// Moves the active tab to the pane in `direction`, opening one there
    /// if there's none. If that pane shows the file already, its tab is
    /// activated and the moved one closes, as long as its unsaved changes
    /// live on in another tab. A pane the move empties closes.
    fn move_tab(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let source = self.active.clone();
        let Some(ix) = source.read(cx).active() else {
            return;
        };
        if source.read(cx).active_editor().is_none() {
            // A file being read, or a read error, shows in the tab's place.
            return;
        }
        let target = match self.panes.neighbour(&source, direction, cx) {
            Some(target) => target,
            None => self.new_pane(direction, window, cx),
        };
        let tab = &source.read(cx).tabs()[ix];
        let shown = tab
            .path
            .as_deref()
            .and_then(|path| target.read(cx).position(path));
        if let Some(shown) = shown {
            let source_pane = source.read(cx);
            let model = tab.editor.read(cx).model();
            if model.is_dirty() && !source_pane.shown_elsewhere(&tab.editor, cx) {
                log::warn!(
                    "not moving {}: it has unsaved changes, and {direction:?} shows another copy",
                    file_name(model.path())
                );
                return;
            }
            source.update(cx, |pane, cx| {
                pane.take_active(window, cx);
                cx.notify();
            });
            target.update(cx, |pane, cx| {
                pane.activate(shown, window, cx);
                cx.notify();
            });
        } else {
            let tab = source.update(cx, |pane, cx| {
                cx.notify();
                pane.take_active(window, cx)
            });
            if let Some(tab) = tab {
                target.update(cx, |pane, cx| pane.insert(tab, window, cx));
            }
        }
        self.activate(target, window, cx);
        if source.update(cx, |pane, _| pane.take_emptied()) {
            self.close_pane(&source, window, cx);
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

    fn open_editors(&self, cx: &App) -> Vec<Entity<EditorView>> {
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

    pub fn palette_open(&self) -> bool {
        self.palette
    }

    /// Shows the quick-open palette, focused, or dismisses it.
    fn toggle_quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let show = !self.palette;
        let previous = window.focused(cx);
        self.quick_open.update(cx, |picker, cx| {
            if show {
                picker.delegate.opened(previous, cx);
                picker.show(window, cx);
            } else {
                picker.dismiss(window, cx);
            }
        });
        self.palette = show;
        cx.notify();
    }

    /// The active tab's editor, unless a file being read or a read error
    /// shows in its place.
    pub fn active_editor<'a>(&self, cx: &'a App) -> Option<&'a Entity<EditorView>> {
        self.active.read(cx).active_editor()
    }

    /// Tells the outline which editor is active, if that changed, and has
    /// the project panel reveal its file. Every tab change redraws, though
    /// not every one notifies (focusing an open tab only focuses it), so the
    /// workbench calls it as it renders. With splits, it's the active pane's
    /// active editor.
    fn follow_active_editor(&mut self, window: &Window, cx: &mut Context<Self>) {
        let editor = self.active_editor(cx).cloned();
        let outline = self.dock.read(cx).outline().clone();
        outline.update(cx, |outline, cx| outline.set_editor(editor.as_ref(), cx));
        let id = editor.as_ref().map(Entity::entity_id);
        let reveal = id != self.revealed;
        self.revealed = id;
        // The editor reparses as it renders, which notifies no one.
        cx.defer_in(window, move |workbench, window, cx| {
            outline.update(cx, OutlinePanel::frame_drawn);
            if reveal {
                workbench.reveal_active(cx);
            }
            workbench.leave_hidden_panel(window, cx);
        });
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

    /// Focuses the active editor if the project panel has focus but isn't
    /// shown, the dock being hidden or showing another panel.
    fn leave_hidden_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dock = self.dock.read(cx);
        let shown = dock.is_visible() && dock.active() == DockPanel::Project;
        if !shown && self.panel.focus_handle(cx).contains_focused(window, cx) {
            window.focus(&self.focus_handle(cx), cx);
        }
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
    /// open.
    pub fn reload_settings(&mut self, event: &SettingsEvent, cx: &mut Context<Self>) {
        for pane in self.panes.panes() {
            pane.update(cx, |pane, cx| pane.reload_settings(event, cx));
        }
    }

    /// Runs a `workbench.*` command on the active pane, a pane command, a
    /// palette toggle or a dock command; others go on to the app.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        let Some(registry) =
            crate::registry(cx).filter(|registry| registry.get(&run.0.command).is_some())
        else {
            if picker::op(run, cx) == Some(PaletteOp::ToggleQuickOpen) {
                return self.toggle_quick_open(window, cx);
            }
            if let Some(op) = pane_group::op(run, cx) {
                return self.run_pane_op(op, window, cx);
            }
            if project_panel::op(run, cx) == Some(PanelOp::RevealActive) {
                return self.reveal_active(cx);
            }
            let fallback = self.focus_handle(cx);
            if outline_panel::shows(run, cx) {
                // Focus stays where it is, unless on search's hidden input.
                self.dock.update(cx, |dock, cx| {
                    dock.show(DockPanel::Outline, cx);
                    dock.leave_search(&fallback, window, cx);
                });
                return self.leave_hidden_panel(window, cx);
            }
            let op = dock::op(run, cx);
            let done = op.is_some_and(|op| {
                self.dock
                    .update(cx, |dock, cx| dock.run(op, &fallback, window, cx))
            });
            if op == Some(dock::DockOp::ShowProject) {
                window.focus(&self.panel.focus_handle(cx), cx);
            }
            self.leave_hidden_panel(window, cx);
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
        self.follow_active_editor(window, cx);
        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_run_command))
            .relative()
            .size_full()
            .flex()
            .bg(self.ui.background)
            .text_color(self.ui.text)
            .text_size(px(12.))
            .child(self.panes.render(&self.ui))
            .child(self.dock.clone())
            .when(self.palette, |root| root.child(self.quick_open.clone()))
    }
}
