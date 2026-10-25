//! The window around the editors: the title bar, menu bar, window frame
//! and status bar; the sidebar and the terminal's dock; the panes and
//! their splits; and the window's saved layout.

use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::Buffer;
use fe_config::TerminalSettings;
use fe_editor::EditorModel;
use gpui::{AnyView, App, AppContext as _, Context, Entity, Focusable, Subscription, Window, px};

#[cfg(test)]
use super::toast;
use super::{Launch, LayoutKeys, TERMINAL_BUTTON, WELCOME, Workbench, active_diff_view};
#[cfg(test)]
use crate::title_bar;
use crate::{
    Dock, MenuBarView, OutlinePanel, Pane, PaneGroup, ProjectPanel, StatusBar,
    dock::DockOp,
    dock_layout,
    find_bar::FindOp,
    pane_group::{Axis, Destination, Direction, RESIZE_STEP},
    pane_toolbar::ToolbarRequest,
    picker::Picker,
    terminal_panel::{RenameTerminal, TERMINAL, TerminalPanel, TerminalPanelEvent},
    workbench::Palette,
};
use fe_command::Value;
use kiln_shell::{
    CloseKind, CloseRequest, Closing, DockPosition, DockState, DockView as _, ItemHandle,
    PaneView as _, Shell, Workspace, WorkspaceEvent,
    close::{self, Items, LiveItems},
    layout::{self, WindowLayout},
};

impl Workbench {
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
        self.note_zen_status_bar();
        self.show_status_bar(cx);
        cx.notify();
    }

    /// Has the shell show the status bar if it's visible, or if vim's
    /// command line, which shows in its place, is open.
    pub(super) fn show_status_bar(&mut self, cx: &mut Context<Self>) {
        let shown = self.status_bar_visible || self.status_bar.read(cx).line_open();
        self.shell
            .update(cx, |shell, cx| shell.set_status_bar_visible(shown, cx));
    }

    /// Applies the `terminal` settings to the terminal panel.
    pub fn set_terminal_settings(&mut self, settings: &TerminalSettings, cx: &mut Context<Self>) {
        self.terminal
            .update(cx, |terminal, cx| terminal.set_settings(settings, cx));
    }

    /// The terminal's dock, at the bottom unless moved.
    pub fn terminal_dock(&self) -> &Entity<kiln_shell::Dock> {
        &self.terminal_dock
    }

    pub fn terminal(&self) -> &Entity<TerminalPanel> {
        &self.terminal
    }

    /// Subscribes to the terminal: when its shell exits the dock hides and
    /// the active editor takes focus, as it does when the bottom dock's rail
    /// hides the terminal with focus in it.
    pub(super) fn follow_terminal(
        terminal: &Entity<TerminalPanel>,
        dock: &Entity<kiln_shell::Dock>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> [Subscription; 3] {
        let exited =
            cx.subscribe_in(
                terminal,
                window,
                |workbench, _, event, window, cx| match *event {
                    TerminalPanelEvent::Exited => {
                        workbench.terminal_dock.update(cx, |dock, cx| {
                            dock.dock_mut().set_visible(false);
                            cx.notify();
                        });
                        window.focus(&workbench.focus_handle(cx), cx);
                    }
                    TerminalPanelEvent::RenameRequested(id) => {
                        workbench.ask_terminal_name(Some(id), window, cx)
                    }
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

    /// `terminal.rename`: asks for a name for the active session.
    pub(crate) fn rename_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_terminal_name(None, window, cx);
    }

    /// Asks for a name for session `id`, the active one if `None`, with its
    /// label to edit. Enter names it, Escape leaves it, and an empty name
    /// clears the custom one.
    pub(crate) fn ask_terminal_name(
        &mut self,
        id: Option<gpui::EntityId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.terminal.read_with(cx, |terminal, cx| match id {
            Some(id) => terminal.label_of(id, cx).map(|label| (id, label)),
            None => terminal.active_label(cx),
        });
        let Some((id, label)) = current else {
            return;
        };
        self.dismiss_palette(window, cx);
        let delegate = RenameTerminal::new(self.terminal.downgrade(), id, &label);
        let picker = cx.new(|cx| Picker::new(delegate, "Terminal name…", fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| {
            picker.show(window, cx);
            let input = picker.input().clone();
            input.update(cx, |input, cx| input.set_text(label.to_string(), cx));
            // `show` reset the name with the empty query.
            picker.delegate.typed(&label);
        });
        self.show_palette(Palette::Rename, picker, window, cx);
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

    pub fn panel(&self) -> &Entity<ProjectPanel> {
        &self.panel
    }

    /// The sidebar's dock, on the right unless moved, which shows the
    /// project panel, search, the outline and the rest.
    pub fn dock(&self) -> &Entity<Dock> {
        &self.dock
    }

    /// Does a dock op, for a key or a rail click, with the active editor as
    /// the fallback focus: showing the project panel focuses it, showing
    /// search focuses its input, and showing the outline leaves focus where
    /// it is unless on a hidden panel.
    pub(crate) fn run_dock_op(&mut self, op: DockOp, window: &mut Window, cx: &mut Context<Self>) {
        self.dock.update(cx, |dock, cx| dock.run(op, window, cx));
        if op == DockOp::ShowProject {
            window.focus(&self.panel.focus_handle(cx), cx);
        }
        self.leave_hidden_panel(window, cx);
    }

    /// Focuses the active editor if a dock panel has focus but isn't
    /// shown, the dock being hidden or showing another panel.
    pub(super) fn leave_hidden_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dock.read(cx).hidden_panel_focused(window, cx) {
            window.focus(&self.focus_handle(cx), cx);
        }
    }

    /// Has the project panel select the active editor's file, if it has
    /// one, expanding the directories above it; else drops a reveal still
    /// pending for another tab. Focus stays where it is.
    pub(crate) fn reveal_active(&mut self, cx: &mut Context<Self>) {
        let path = self
            .active_editor(cx)
            .and_then(|editor| editor.read(cx).model().path().map(Path::to_owned));
        self.panel.update(cx, |panel, cx| match path {
            Some(path) => panel.reveal(path, cx),
            None => panel.cancel_reveal(),
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

    /// The window's toasts.
    #[cfg(test)]
    pub(crate) fn toasts(&self) -> &Entity<toast::ToastLayer> {
        &self.toasts
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
    pub(super) fn pane_added(
        &mut self,
        pane: &Entity<Pane>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        self.follow_zen_tabs(pane, cx);
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

    pub(super) fn workspace_event(
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
            // it then ([`Workbench::follow_workspace`]), which sees only
            // where it ended up: each change is counted here, so a round
            // trip through another pane within one update still counts.
            WorkspaceEvent::ActiveChanged => self.activations += 1,
        }
    }

    /// The workspace changed: a tab opened, closed or changed, a pane came
    /// or went, or another is active, by a click, a close or the workspace
    /// activating one itself. The workbench follows its active pane and its
    /// editors, and once a restored layout's files are in, takes up their
    /// editors; a window with no root that's left with nothing open shows
    /// the welcome text, as a launch with nothing to open does.
    pub(super) fn follow_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            // A Git Log drawn meanwhile waited for the active file.
            if let Some(panel) = self.dock.read(cx).git_log().cloned() {
                panel.update(cx, |_, cx| cx.notify());
            }
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
        self.activations += 1;
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

    /// Saves the window's layout from now on, in `layout`, through the
    /// workspace ([`Workspace::set_layout`]): as its shape changes (a
    /// split, a close, a move, a dock toggled, moved or resized, a tab
    /// opening or closing, a divider's drag), in full as a buffer's backup
    /// is written or removed, and as the app quits, once its buffers are
    /// backed up. As the window closes, its entry is kept if it's the last
    /// window, else deleted ([`kiln_shell::layout`]). Its unsaved buffers
    /// are backed up beside it from now on, for hot exit ([`super::hot_exit`]).
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
    pub(super) fn layout_keys_changed(&mut self, cx: &mut Context<Self>) {
        // Either dock notified: Zen Mode notes one shown or hidden.
        self.note_zen_chrome(cx);
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
    pub(super) fn follow_active_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
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

impl LayoutKeys {
    pub(super) fn save(&self, cx: &App) -> Vec<(&'static str, Value)> {
        let sidebar = self.sidebar.read(cx).state();
        let terminal = self.terminal.read(cx).dock();
        let docks = Value::from_iter([
            ("sidebar", dock_layout::position_name(sidebar.position())),
            ("terminal", dock_layout::position_name(terminal.position())),
        ]);
        let mut dock = sidebar.save(cx);
        // As it was before Zen Mode hid it, unless shown or hidden since.
        if let Some(before) = self.zen_sidebar.get() {
            dock["visible"] = Value::from(before);
        }
        let mut keys = vec![
            ("docks", docks),
            ("dock", dock),
            ("terminal_dock", terminal_dock_sizes(&terminal.save(cx))),
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
    pub(super) fn restore(&self, entry: &Value, cx: &mut App) {
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

/// Where the terminal starts: the launch root, else the launch file's
/// directory, else the home directory.
pub(super) fn terminal_dir(launch: &Launch) -> PathBuf {
    let file_dir = || launch.file.as_ref()?.0.parent().map(Path::to_owned);
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    (launch.root.clone().or_else(file_dir).or_else(home)).unwrap_or_else(|| PathBuf::from("/"))
}
