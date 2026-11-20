//! The window around the editors: the title bar, menu bar, window frame
//! and status bar; the sidebar and the bottom panel; the panes and
//! their splits; and the window's saved layout.

use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_config::TerminalSettings;
use gpui::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable, Subscription, Window,
    px,
};

#[cfg(test)]
use super::toast;
use super::{Launch, LayoutKeys, Workbench, active_diff_view};

use crate::run_widget_debug::RunChoice;
use crate::title_bar;
#[path = "run_widget.rs"]
mod run_widget;
use crate::{
    Dock, MenuBarView, OutlinePanel, Pane, PaneGroup, ProjectPanel, StatusBar,
    dock::{BOTTOM_TABS, DockOp, Place, ProblemsOp},
    dock_layout,
    find_bar::FindOp,
    pane_group::{Axis, Destination, Direction, RESIZE_STEP},
    pane_toolbar::ToolbarRequest,
    picker::Picker,
    terminal_panel::{PickProfile, RenameTerminal, TERMINAL, TerminalPanel, TerminalPanelEvent},
    workbench::Palette,
};
use fe_command::Value;
use kiln_l10n::text;
use kiln_shell::{
    CloseKind, CloseRequest, Closing, DockPosition, DockState, DockView as _, ItemHandle,
    PaneView as _, PanelId, Shell, Workspace, WorkspaceEvent,
    close::{self, Items, LiveItems},
    dock::{DockDrop, DockEvent, DockedPanel, MoveTo},
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
    /// transparent macOS title bar, with the workbench's items in it (see
    /// [`title_bar`]): see [`crate::titlebar_options`]. Off by default.
    pub fn set_title_bar(&mut self, shown: bool, cx: &mut Context<Self>) {
        let mut items = title_bar::items(cx.entity().downgrade());
        let terminal = self.terminal().clone();
        run_widget::install(&mut items, &cx.entity(), &terminal, cx);
        self.shell.update(cx, |shell, cx| {
            shell.set_title_bar_items(items, cx);
            shell.set_title_bar(shown, cx);
        });
        self.own_title_bar = shown;
        self.sync_agents_status(cx);
        // The debug toolbar floats only without it.
        cx.notify();
    }

    /// Sets or removes the `agents` status item to follow the lanes
    /// Working: only where the platform draws the title bar, as the
    /// unified one has the chip.
    pub(crate) fn sync_agents_status(&mut self, cx: &mut Context<Self>) {
        let working = self.lanes.read(cx).counts().working;
        let shown = if self.own_title_bar { 0 } else { working };
        self.status_bar()
            .clone()
            .update(cx, |bar, cx| bar.set_agents(shown, cx));
    }

    /// Presses the run widget's play, without the read it waits for.
    #[cfg(test)]
    pub(crate) fn press_run_play(window: &mut Window, cx: &mut App) {
        run_widget::press_play(window, cx);
    }

    /// What the title bar shows: whether back and forward can go, the
    /// root's name and HEAD's branch.
    pub(crate) fn title_bar_state(&self) -> title_bar::State {
        title_bar::State {
            nav: self.nav,
            project: title_bar::name(self.root.as_deref()),
            branch: self.title_branch.clone(),
            tracking: self.title_upstream.clone(),
            // Read from the lanes as the bar draws ([`title_bar::items`]).
            working: 0,
        }
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

    /// The bottom panel, the terminal's dock, which holds Problems and the
    /// Git log too: at the bottom unless moved.
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
                    // Its last shell exited: its tab closes, another tool
                    // window open in the bottom panel showing in its place
                    // if it was shown, else the panel hiding. Focus moves
                    // to the editor only from what the close hid.
                    // Moved to the sidebar, the sidebar hides if it shows it.
                    TerminalPanelEvent::Exited => {
                        match workbench.place_of(TERMINAL, cx) {
                            Place::Bottom => workbench.terminal_dock.update(cx, |dock, cx| {
                                dock.dock_mut().close_tab(TERMINAL);
                                cx.notify();
                            }),
                            Place::Sidebar => _ = workbench.hide_sidebar_tool(TERMINAL, cx),
                        }
                        workbench.leave_hidden_panel(window, cx);
                    }
                    TerminalPanelEvent::RenameRequested(id) => {
                        workbench.ask_terminal_name(Some(id), window, cx)
                    }
                    TerminalPanelEvent::ShowRequested => workbench.show_terminal_dock(window, cx),
                    // For an open picker of them.
                    TerminalPanelEvent::ProfilesChanged => {}
                },
            );
        // A tool window dropped on it, from the sidebar's rail, moves.
        let hid = cx.subscribe_in(
            dock,
            window,
            |workbench, _, event, window, cx| match *event {
                DockEvent::HidFocus => window.focus(&workbench.focus_handle(cx), cx),
                DockEvent::PanelDropped {
                    panel,
                    from,
                    target,
                } => workbench.tool_window_dropped(panel, from, target, window, cx),
            },
        );
        // Each foot button is lit while the bottom dock shows its tool
        // window, in any of its groups.
        let shown = cx.observe(dock, |workbench, dock, cx| {
            let shown = dock.read(cx).dock().shown_panels();
            workbench
                .dock
                .update(cx, |dock, cx| dock.light_foot(shown, cx));
        });
        [exited, hid, shown]
    }

    /// Shows the tool window `panel` in whichever dock holds it, focused
    /// if it focuses on show: the bottom dock unless it was moved to the
    /// sidebar ([`Place`]).
    pub(crate) fn show_bottom_panel(
        &mut self,
        panel: PanelId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settle_placements(cx);
        match self.place_of(panel, cx) {
            Place::Bottom => {
                self.terminal_dock
                    .update(cx, |dock, cx| dock.show(panel, window, cx));
            }
            Place::Sidebar => {
                self.dock
                    .update(cx, |dock, cx| dock.show_tool(panel, window, cx));
            }
        }
    }

    /// Which dock holds the tool window `panel`.
    pub(crate) fn place_of(&self, panel: PanelId, cx: &App) -> Place {
        self.dock.read(cx).place_of(panel)
    }

    /// Whether the tool window `panel` shows, in whichever dock holds it.
    pub(crate) fn tool_shown(&self, panel: PanelId, cx: &App) -> bool {
        match self.place_of(panel, cx) {
            Place::Bottom => self.terminal_dock.read(cx).dock().is_shown(panel),
            Place::Sidebar => self.dock.read(cx).state().rail_highlighted() == Some(panel),
        }
    }

    /// Hides the sidebar if it shows the tool window `panel`, as its
    /// lit button does. Focus in it stays: the caller moves it.
    fn hide_sidebar_tool(&mut self, panel: PanelId, cx: &mut Context<Self>) -> bool {
        self.dock.update(cx, |dock, cx| {
            let shown = dock.state().rail_highlighted() == Some(panel);
            if shown {
                dock.dock_mut().set_visible(false);
                cx.notify();
            }
            shown
        })
    }

    /// Moves each tool window registered in the dock its place isn't to
    /// the one it is, unshown: a restored layout's, and a panel made since,
    /// as History is on first use.
    pub(crate) fn settle_placements(&mut self, cx: &mut Context<Self>) {
        settle_placements((&self.dock, &self.terminal_dock), cx);
    }

    /// Moves the tool window `panel` to the dock `to`: takes it from the
    /// one holding it, puts it in `to` (in the bottom dock, where `at`
    /// says, else in its focused group), notes its place and shows it
    /// there, focused. False, doing nothing, if it doesn't move or is
    /// there already.
    pub(crate) fn move_tool_window(
        &mut self,
        panel: PanelId,
        to: Place,
        at: Option<DockDrop>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(panel) = crate::dock::movable(panel.0) else {
            return false;
        };
        if self.place_of(panel, cx) == to {
            return false;
        }
        let docks = (&self.dock, &self.terminal_dock);
        if !move_between(docks, panel, to, at, cx) {
            return false;
        }
        // Where focus came from isn't where it goes back to: the tool
        // window is in the other dock, and its focus-out no longer ends it.
        self.bottom_from_sidebar = false;
        self.bottom_watch = None;
        self.show_bottom_panel(panel, window, cx);
        if let Some(focus) = self.bottom_tool_focus(panel, cx) {
            window.focus(&focus, cx);
        }
        true
    }

    /// The tool window `panel`, of the dock view `from`, dropped on a dock
    /// at `target`: from the bottom dock into the sidebar, or from the
    /// sidebar's rail into the bottom dock's groups or onto its foot.
    pub(crate) fn tool_window_dropped(
        &mut self,
        panel: PanelId,
        from: gpui::EntityId,
        target: DockDrop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (sidebar, bottom) = (self.dock.entity_id(), self.terminal_dock.entity_id());
        let to = match target {
            DockDrop::Into if from == bottom => Place::Sidebar,
            DockDrop::Foot if from == sidebar => Place::Bottom,
            _ if from == sidebar && target.move_to().is_some() => Place::Bottom,
            _ => return,
        };
        let at = (target != DockDrop::Foot).then_some(target);
        self.move_tool_window(panel, to, at, window, cx);
    }

    /// `dock.move_tool_window_to_sidebar` (`to` the sidebar) and
    /// `dock.move_tool_window_to_bottom_panel`: moves the tool window
    /// holding focus, else the bottom panel's focused group's (the
    /// sidebar's shown panel), to `to`, if it can go there.
    pub(crate) fn move_tool_window_to(
        &mut self,
        to: Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focused = self.focused_tool(window, cx);
        let panel = match focused {
            Some(panel) => Some(panel),
            None => match to {
                Place::Sidebar => self.terminal_dock.read(cx).dock().rail_highlighted(),
                Place::Bottom => self.dock.read(cx).state().rail_highlighted(),
            },
        };
        if let Some(panel) = panel {
            self.move_tool_window(panel, to, None, window, cx);
        }
    }

    /// The tool window holding focus, in either dock.
    fn focused_tool(&self, window: &Window, cx: &App) -> Option<PanelId> {
        let bottom = self.terminal_dock.read(cx).dock().panel_ids();
        let sidebar = self.dock.read(cx).state().panel_ids();
        (bottom.into_iter().chain(sidebar)).find(|&panel| {
            (self.bottom_tool_focus(panel, cx))
                .is_some_and(|focus| focus.contains_focused(window, cx))
        })
    }

    /// Focuses the tool window the bottom dock shows, its focused group's,
    /// wherever it is, true if it shows one.
    pub(crate) fn focus_bottom_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let dock = self.terminal_dock.read(cx);
        let Some(panel) = dock.dock().rail_highlighted() else {
            return false;
        };
        let Some(focus) = self.bottom_tool_focus(panel, cx) else {
            return false;
        };
        window.focus(&focus, cx);
        true
    }

    /// The focus of the tool window `panel`, wherever it is, if it
    /// exists: a bottom one's, or what focusing a sidebar panel focuses
    /// ([`Dock::panel_focus`]).
    pub(crate) fn bottom_tool_focus(&self, panel: PanelId, cx: &App) -> Option<FocusHandle> {
        Some(match panel.0 {
            id if id == crate::dock::PROBLEMS_BUTTON => {
                self.dock.read(cx).problems().focus_handle(cx)
            }
            id if id == crate::dock::RUNS_BUTTON => self.runs_panel.focus_handle(cx),
            id if id == crate::dock::DEBUG_BUTTON => self.debug_panel.focus_handle(cx),
            id if id == crate::dock::GIT_LOG_BUTTON => {
                self.dock.read(cx).git_log()?.focus_handle(cx)
            }
            id if id == TERMINAL.0 => self.terminal.focus_handle(cx),
            _ => {
                let sidebar = self.dock.read(cx);
                let panel = crate::DockPanel::of(panel)?;
                sidebar.panel_focus(panel, cx)?
            }
        })
    }

    /// The bottom tool window the panel-group commands act on: the shown
    /// one that holds focus, else the one the bottom dock's focused group
    /// shows. None while the dock is hidden.
    fn bottom_tool_in_play(&self, window: &Window, cx: &App) -> Option<PanelId> {
        let dock = self.terminal_dock.read(cx).dock();
        let focused = (dock.shown_panels().into_iter()).find(|&panel| {
            (self.bottom_tool_focus(panel, cx))
                .is_some_and(|focus| focus.contains_focused(window, cx))
        });
        focused.or(dock.rail_highlighted())
    }

    /// Hides the bottom dock's tool window `panel` if it shows: its group
    /// alone collapses if another group shows, else the whole dock hides
    /// ([`DockState::hide_panel`]). Focus that was in what it hid goes to
    /// the active editor. False, doing nothing, if it isn't shown.
    pub(crate) fn hide_bottom_tool(
        &mut self,
        panel: PanelId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // Moved to the sidebar, the sidebar hides if it shows it.
        if self.place_of(panel, cx) == Place::Sidebar {
            let hid = self.hide_sidebar_tool(panel, cx);
            if hid && self.dock.read(cx).hidden_panel_focused(window, cx) {
                window.focus(&self.focus_handle(cx), cx);
            }
            return hid;
        }
        let focused = self.bottom_panel_focused(window, cx);
        let hid = self.terminal_dock.update(cx, |dock, cx| {
            let hid = dock.dock_mut().hide_panel(panel);
            cx.notify();
            hid
        });
        if hid
            && focused
            && self
                .terminal_dock
                .read(cx)
                .dock()
                .hidden_panel_focused(window, cx)
        {
            window.focus(&self.focus_handle(cx), cx);
        }
        hid
    }

    /// `dock.split_panel_right`: moves the bottom tool window in play
    /// ([`Workbench::bottom_tool_in_play`]) into a new group just after its
    /// own, if its group keeps another tab, focused.
    pub(crate) fn split_panel_right(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.move_bottom_tool(window, cx, |dock, panel| {
            let group = dock.group_of(panel)?;
            (dock.open_panels_in(group).len() > 1).then_some(MoveTo::After(group))
        });
    }

    /// `dock.move_panel_to_next_group` (`by` 1) and
    /// `dock.move_panel_to_previous_group` (-1): moves the bottom tool
    /// window in play to the end of the drawn group after (before) its own,
    /// active and focused there, a group it empties going away.
    pub(crate) fn move_panel_to_group(
        &mut self,
        by: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_bottom_tool(window, cx, |dock, panel| {
            let drawn = drawn_groups(dock);
            let at = drawn
                .iter()
                .position(|&group| dock.group_of(panel) == Some(group))?;
            let to = at.checked_add_signed(by).and_then(|ix| drawn.get(ix))?;
            Some(MoveTo::Group {
                group: *to,
                index: usize::MAX,
            })
        });
    }

    /// Moves the bottom tool window in play where `to` says, if it says,
    /// keeping the dock shown and focusing it.
    fn move_bottom_tool(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        to: impl FnOnce(&DockState, PanelId) -> Option<MoveTo>,
    ) {
        let Some(panel) = self.bottom_tool_in_play(window, cx) else {
            return;
        };
        let Some(to) = to(self.terminal_dock.read(cx).dock(), panel) else {
            return;
        };
        let moved = self.terminal_dock.update(cx, |dock, cx| {
            let moved = dock.dock_mut().move_panel(panel, to);
            cx.notify();
            moved
        });
        if moved && let Some(focus) = self.bottom_tool_focus(panel, cx) {
            window.focus(&focus, cx);
        }
    }

    /// `dock.join_panel_groups`: puts the bottom dock's tool windows back
    /// in one group, the one in play shown and focused. Nothing with one
    /// group.
    pub(crate) fn join_panel_groups(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel = self.bottom_tool_in_play(window, cx);
        let dock = self.terminal_dock.read(cx).dock();
        let Some(panel) = panel.filter(|_| dock.group_count() > 1) else {
            return;
        };
        self.terminal_dock.update(cx, |dock, cx| {
            dock.dock_mut().join_groups();
            dock.show(panel, window, cx);
        });
        if let Some(focus) = self.bottom_tool_focus(panel, cx) {
            window.focus(&focus, cx);
        }
    }

    /// `dock.focus_next_panel_group`: focuses the tool window of the drawn
    /// group after the one in play's, wrapping. Nothing with one drawn.
    pub(crate) fn focus_next_panel_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.bottom_tool_in_play(window, cx) else {
            return;
        };
        let dock = self.terminal_dock.read(cx).dock();
        let drawn = drawn_groups(dock);
        let Some(at) = drawn
            .iter()
            .position(|&group| dock.group_of(panel) == Some(group))
        else {
            return;
        };
        let Some(next) = (drawn.len() > 1).then(|| drawn[(at + 1) % drawn.len()]) else {
            return;
        };
        let Some(shown) =
            (dock.shown_panels().into_iter()).find(|&id| dock.group_of(id) == Some(next))
        else {
            return;
        };
        self.terminal_dock.update(cx, |dock, cx| {
            dock.show(shown, window, cx);
        });
        if let Some(focus) = self.bottom_tool_focus(shown, cx) {
            window.focus(&focus, cx);
        }
    }

    /// Hides the bottom dock. Focus in it stays: the caller moves it.
    #[cfg(test)]
    pub(crate) fn hide_bottom_panel(&mut self, cx: &mut Context<Self>) {
        self.terminal_dock.update(cx, |dock, cx| {
            dock.dock_mut().set_visible(false);
            cx.notify();
        });
    }

    /// Whether a tool window of the bottom dock has focus: those placed
    /// there ([`Place`]).
    pub(crate) fn bottom_panel_focused(&self, window: &Window, cx: &App) -> bool {
        let panels = self.terminal_dock.read(cx).dock().panel_ids();
        panels.into_iter().any(|panel| match panel == TERMINAL {
            true => self.terminal.read(cx).is_focused(window, cx),
            false => (self.bottom_tool_focus(panel, cx))
                .is_some_and(|focus| focus.contains_focused(window, cx)),
        })
    }

    /// A click on the rail's foot button `id`, which runs `command`: hides
    /// that tool window if the bottom dock shows it, in any group
    /// ([`Workbench::hide_bottom_tool`]), the active editor taking focus if
    /// it was in it, else runs the command, as its key would.
    pub(crate) fn bottom_button_clicked(
        &mut self,
        id: &'static str,
        command: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.hide_bottom_tool(PanelId(id), window, cx) {
            // Once this update is over: the command is the workbench's.
            window.defer(cx, move |window, cx| {
                kiln_shell::dock::run_action(command, window, cx)
            });
        }
    }

    /// `terminal.toggle`: shows the bottom dock with the terminal, focused,
    /// starting a session if none runs; focuses the terminal if the dock
    /// shows but focus is elsewhere; hides the terminal and focuses the
    /// active editor if the terminal has focus: its group alone if another
    /// shows beside it, else the dock.
    pub(crate) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Moved to the sidebar, the sidebar hides.
        if self.place_of(TERMINAL, cx) == Place::Sidebar {
            if self.tool_shown(TERMINAL, cx) && self.terminal.read(cx).is_focused(window, cx) {
                self.hide_sidebar_tool(TERMINAL, cx);
                window.focus(&self.focus_handle(cx), cx);
                return;
            }
            self.terminal
                .update(cx, |terminal, cx| terminal.start(window, cx));
            return self.show_terminal_dock(window, cx);
        }
        let shown = self.terminal_dock.read(cx).is_visible();
        if shown && self.terminal.read(cx).is_focused(window, cx) {
            self.terminal_dock.update(cx, |dock, cx| {
                if !dock.dock_mut().hide_panel(TERMINAL) {
                    dock.dock_mut().set_visible(false);
                }
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

    /// `terminal.new_with_profile`: with `profile`, shows the bottom dock with
    /// a new session of the profile it names, focused, as `terminal.new`
    /// does with the default one; an unknown name is a toast and starts
    /// nothing. Without, asks which profile.
    pub(crate) fn new_terminal_with_profile(
        &mut self,
        profile: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(profile) = profile else {
            return self.ask_terminal_profile(window, cx);
        };
        let started = self.terminal.update(cx, |terminal, cx| {
            terminal.add_profile(&profile, window, cx)
        });
        if started {
            self.show_terminal_dock(window, cx);
        }
    }

    /// Asks which profile to start a terminal from: Enter starts it, as
    /// `terminal.new_with_profile` with its name does, and Escape starts
    /// nothing.
    fn ask_terminal_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_palette(window, cx);
        let (workbench, terminal) = (cx.entity().downgrade(), self.terminal.clone());
        let picker = cx.new(|cx| {
            let delegate = PickProfile::new(workbench, &terminal, cx);
            Picker::new(
                delegate,
                text!("workbench-terminal-profile-placeholder"),
                fe_ui::ui_theme(cx),
                cx,
            )
        });
        picker.update(cx, |picker, cx| picker.show(window, cx));
        self.show_palette(Palette::Rename, picker, window, cx);
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
        let picker = cx.new(|cx| {
            Picker::new(
                delegate,
                text!("workbench-chrome-terminal-name-placeholder"),
                fe_ui::ui_theme(cx),
                cx,
            )
        });
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

    /// Shows the bottom dock with the terminal, or the sidebar if it was
    /// moved there.
    pub(crate) fn show_terminal_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_bottom_panel(TERMINAL, window, cx);
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
    /// it is unless on a hidden panel. `problems.show`, `runs.show` and the
    /// Git log's show op show their tool windows in the bottom dock: each
    /// show op in whichever dock holds its tool window ([`Place`]), first,
    /// so what the sidebar does after (search's replace field, Changes'
    /// focus) stands.
    pub(crate) fn run_dock_op(&mut self, op: DockOp, window: &mut Window, cx: &mut Context<Self>) {
        self.settle_placements(cx);
        let tool = match op {
            DockOp::Problems(ProblemsOp::Show) => Some(crate::DockPanel::Problems.id()),
            DockOp::ShowGitLog => Some(PanelId(crate::dock::GIT_LOG_BUTTON)),
            DockOp::ShowRuns => Some(PanelId(crate::dock::RUNS_BUTTON)),
            // A sidebar panel moved to the bottom panel.
            _ => (crate::dock::shown_by(op))
                .filter(|&panel| self.place_of(panel, cx) == Place::Bottom),
        };
        if let Some(panel) = tool {
            self.show_bottom_panel(panel, window, cx);
        }
        self.dock.update(cx, |dock, cx| dock.run(op, window, cx));
        if op == DockOp::ShowProject {
            window.focus(&self.panel.focus_handle(cx), cx);
        }
        self.leave_hidden_panel(window, cx);
    }

    /// Focuses the active editor if a panel of the sidebar or a tool
    /// window of the bottom panel has focus but isn't shown, its dock being
    /// hidden or showing another.
    pub(crate) fn leave_hidden_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bottom = self.terminal_dock.read(cx).dock();
        if self.dock.read(cx).hidden_panel_focused(window, cx)
            || bottom.hidden_panel_focused(window, cx)
        {
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

    /// `dock.grow` or `dock.shrink`: resizes the bottom dock by `by` while
    /// one of its tool windows has focus, those placed there, else the
    /// sidebar.
    pub(crate) fn resize_focused_dock(
        &mut self,
        by: gpui::Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.bottom_panel_focused(window, cx) {
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

    /// The editor area as the last frame laid it out: the panes' content,
    /// below their tab bars, between the rail, the docks and the bottom
    /// panel. None before there is one.
    pub(crate) fn editor_area(&self, cx: &App) -> Option<gpui::Bounds<gpui::Pixels>> {
        let areas = (self.panes(cx).into_iter())
            .map(|pane| kiln_shell::PaneView::pane(pane.read(cx)).content_bounds())
            .filter(|bounds| bounds.size.width > gpui::px(0.) && bounds.size.height > gpui::px(0.));
        areas.reduce(|a, b| a.union(&b))
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
        let renames = cx.subscribe_in(
            pane,
            window,
            |workbench, pane, request: &crate::find_bar::FindRename, window, cx| {
                workbench.find_rename(pane, request, window, cx)
            },
        );
        self.find_renames.insert(pane.entity_id(), renames);
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
                self.find_renames.remove(id);
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
    /// editors.
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
        self.link_epoch += 1;
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

    /// Activates the pane in `direction` of the active one, if there's one;
    /// else, with vim on, focuses the dock shown on that side
    /// ([`Workbench::focus_dock_beyond`]).
    pub(crate) fn focus_pane(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = self.pane_group(cx).neighbour(&self.active, direction, cx) {
            self.activate(pane, window, cx);
        } else if fe_gpui::vim_enabled(cx) {
            self.focus_dock_beyond(direction, window, cx);
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

    /// What the run widget shows in `root`, as chosen there: a task or a
    /// launch configuration.
    pub(crate) fn run_task_choice(&self, root: &Path) -> Option<RunChoice> {
        let root = root.to_str()?;
        self.layout_keys.run_tasks.borrow().get(root).cloned()
    }

    /// Remembers `choice` as what the run widget shows in `root`, in the
    /// window's saved layout.
    pub(crate) fn choose_run_task(
        &mut self,
        root: &Path,
        choice: RunChoice,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = root.to_str() else {
            return;
        };
        (self.layout_keys.run_tasks.borrow_mut()).insert(root.to_owned(), choice);
        self.layout_keys_changed(cx);
        cx.notify();
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
        self.note_restored_paths(entry);
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
        self.follow_problems_file(editor.as_ref(), cx);
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

/// The groups of the bottom dock drawn, in order: those not collapsed
/// with a tab, while it shows.
fn drawn_groups(dock: &DockState) -> Vec<usize> {
    (0..dock.group_count())
        .filter(|&group| dock.is_visible() && !dock.is_collapsed(group))
        .filter(|&group| !dock.open_panels_in(group).is_empty())
        .collect()
}

/// The bottom panel's sizes in its saved state ([`DockState::save`]),
/// and, as `"shown"`, the tool window it shows if that's Problems, Runs or
/// the Git log: the terminal comes back hidden, with no session. Split
/// into groups, its `"groups"` too, as saved.
fn terminal_dock_sizes(dock: &DockState, cx: &App) -> Value {
    let saved = dock.save(cx);
    let size = |key: &'static str| Some((key, saved.get(key)?.clone()));
    let mut kept = Value::from_iter(
        ["size", "side_size", "bottom_size", "groups"]
            .into_iter()
            .filter_map(size),
    );
    if let Some(shown) = dock.rail_highlighted().and_then(restorable) {
        kept["shown"] = Value::from(shown.0);
    }
    kept
}

/// What [`DockState::restore`] takes to put back the bottom panel's saved
/// `groups` ([`terminal_dock_sizes`]): each group's active tool window
/// only if it's [`restorable`], so a group the terminal showed comes back
/// with no tab, in its place; and, if a group not collapsed then shows
/// one, the dock shown with `shown` (else that first one) active.
fn restored_groups(groups: &Value, shown: Option<PanelId>) -> Value {
    let mut groups = groups.clone();
    let mut first = None;
    for group in groups.as_array_mut().into_iter().flatten() {
        let active = (group.get("active").and_then(Value::as_str))
            .and_then(|id| crate::dock::movable(id).and_then(restorable))
            .map(|panel| panel.0);
        group["active"] = active.map_or(Value::Null, Value::from);
        let collapsed = (group.get("collapsed").and_then(Value::as_bool)).unwrap_or(false);
        if !collapsed && first.is_none() {
            first = active;
        }
    }
    let mut restored = Value::from_iter([("groups", groups)]);
    if let Some(active) = shown.map(|id| id.0).or(first) {
        restored["visible"] = Value::from(true);
        restored["active"] = Value::from(active);
    }
    restored
}

/// `panel` if the bottom panel shows it again on a restore: Problems, Runs
/// or the Git log, or a sidebar panel moved there, not the terminal, whose
/// sessions end with the window.
fn restorable(panel: PanelId) -> Option<PanelId> {
    let moved = !crate::dock::BOTTOM_TOOLS.contains(&panel.0);
    (moved || RESTORABLE.contains(&panel.0)).then_some(panel)
}

/// The bottom panel's tool windows a restore shows again: see
/// [`restorable`].
const RESTORABLE: [&str; 3] = [
    crate::dock::PROBLEMS_BUTTON,
    crate::dock::RUNS_BUTTON,
    crate::dock::GIT_LOG_BUTTON,
];

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
            ("terminal_dock", terminal_dock_sizes(terminal, cx)),
            ("breadcrumbs", Value::from(self.breadcrumbs.get())),
        ];
        // The tool windows moved from their places, alone.
        let moved = self.sidebar.read(cx).placements();
        if !moved.is_empty() {
            let moved =
                (moved.into_iter()).map(|(id, at)| (id.0.to_owned(), Value::from(at.name())));
            keys.push(("placements", Value::Object(moved.collect())));
        }
        if let Some(bounds) = self.bounds.get() {
            keys.push(("bounds", layout::save_bounds(bounds)));
        }
        // A window with no root says so, else it would reopen on its
        // store's root (`entry_root`): its launch folder, after Close Folder.
        match self.root.borrow().as_deref() {
            Some(root) => {
                if let Some(root) = root.to_str() {
                    keys.push(("root", Value::from(root)));
                }
            }
            None => keys.push(("root", Value::from(""))),
        }
        let run_tasks = self.run_tasks.borrow();
        if !run_tasks.is_empty() {
            let tasks = run_tasks
                .iter()
                .map(|(root, choice)| (root.clone(), choice.to_value()));
            keys.push(("run_tasks", Value::Object(tasks.collect())));
        }
        keys
    }

    /// Puts back the terminal dock's place, if the saved places are two
    /// edges with the sidebar at a side, then the sidebar's state and the
    /// bottom panel's sizes (it stays hidden, with no session, unless it
    /// was saved showing Problems, Runs or the Git log, or a layout saved before
    /// it held them had one shown in the sidebar: it shows that), and notes
    /// the breadcrumbs. The sidebar stays on the rail's side, as
    /// `workbench.rail` put it: the terminal's dock gives way if it was
    /// saved there ([`dock_layout::displaced`]).
    pub(super) fn restore(&self, entry: &Value, cx: &mut App) {
        // An earlier layout's groups don't apply to this one's tool windows.
        self.sidebar
            .update(cx, |dock, _| dock.keep_pending_groups(None));
        // The tool windows' places first, so the docks' state finds them:
        // without, each in its default place.
        let saved = entry.get("placements").and_then(Value::as_object);
        let places: Vec<(PanelId, Place)> = (saved.into_iter().flatten())
            .filter_map(|(id, at)| Some((crate::dock::movable(id)?, Place::named(at.as_str()?)?)))
            .collect();
        self.sidebar.update(cx, |dock, cx| {
            for (panel, _) in dock.placements() {
                dock.set_place(panel, Place::default_of(panel), cx);
            }
            for (panel, at) in places {
                dock.set_place(panel, at, cx);
            }
        });
        settle_placements((&self.sidebar, &self.terminal), cx);
        let position = |dock| {
            let name = entry.get("docks")?.get(dock)?.as_str()?;
            dock_layout::position_named(name)
        };
        if let (Some(saved), Some(terminal)) = (position("sidebar"), position("terminal"))
            && saved != terminal
            && saved != DockPosition::Bottom
        {
            let sidebar = self.sidebar.read(cx).state().position();
            let terminal = dock_layout::displaced(saved, sidebar, terminal);
            let docks = (&self.sidebar, &self.terminal, &self.shell);
            place_docks(docks, sidebar, terminal, cx);
        }
        // Saved before the bottom panel held them, Problems or the Git log
        // shown in the sidebar shows in the bottom panel, unfocused, as
        // does the one the bottom panel saved shown.
        let mut bottom = (entry.get("dock"))
            .and_then(|saved| self.sidebar.update(cx, |dock, cx| dock.restore(saved, cx)));
        if let Some(saved) = entry.get("terminal_dock") {
            self.terminal.update(cx, |dock, cx| {
                restore_terminal_dock_sizes(dock.dock_mut(), saved);
                cx.notify();
            });
            let shown = saved.get("shown").and_then(Value::as_str);
            let here = self.terminal.read(cx).dock().panel_ids();
            let shown = (shown.and_then(crate::dock::movable))
                .and_then(restorable)
                .filter(|panel| here.contains(panel))
                .map(|panel| panel.0);
            bottom = shown.map(PanelId).or(bottom);
            // Split into groups, each comes back in its place, showing
            // what it showed if that comes back.
            if let Some(groups) = saved.get("groups") {
                let restored = restored_groups(groups, shown.map(PanelId));
                self.terminal.update(cx, |dock, cx| {
                    dock.dock_mut().restore(&restored, cx);
                    cx.notify();
                });
                // A tool window made on first use, placed there, joins its
                // group when it's made.
                let waiting = (crate::dock::LAZY_TOOLS.iter()).any(|panel| {
                    let panel = panel.id();
                    self.sidebar.read(cx).place_of(panel) == Place::Bottom
                        && !self.terminal.read(cx).dock().panel_ids().contains(&panel)
                });
                let pending = waiting.then(|| crate::dock::PendingGroups {
                    saved: Value::from_iter([("groups", restored["groups"].clone())]),
                    left: self
                        .terminal
                        .read(cx)
                        .dock()
                        .save(cx)
                        .get("groups")
                        .cloned(),
                });
                self.sidebar
                    .update(cx, |dock, _| dock.keep_pending_groups(pending));
                bottom = None;
            }
        }
        if let Some(panel) = bottom {
            self.terminal.update(cx, |dock, cx| {
                let shown = Value::from_iter([
                    ("visible", Value::from(true)),
                    ("active", Value::from(panel.0)),
                ]);
                dock.dock_mut().restore(&shown, cx);
                cx.notify();
            });
        }
        if let Some(shown) = entry.get("breadcrumbs").and_then(Value::as_bool) {
            self.breadcrumbs.set(shown);
        }
        if let Some(saved) = entry.get("run_tasks").and_then(Value::as_object) {
            let tasks = saved
                .iter()
                .filter_map(|(root, choice)| Some((root.clone(), RunChoice::from_value(choice)?)));
            *self.run_tasks.borrow_mut() = tasks.collect();
        }
    }
}

/// Moves each tool window registered in the dock its place isn't to the
/// one it is, unshown ([`Workbench::settle_placements`]).
fn settle_placements((sidebar, bottom): (&Entity<Dock>, &Entity<kiln_shell::Dock>), cx: &mut App) {
    let dock = sidebar.read(cx);
    let to_bottom: Vec<PanelId> = (dock.state().panel_ids().into_iter())
        .filter(|&panel| dock.place_of(panel) == Place::Bottom)
        .collect();
    let to_sidebar: Vec<PanelId> = (bottom.read(cx).dock().panel_ids().into_iter())
        .filter(|&panel| dock.place_of(panel) == Place::Sidebar)
        .collect();
    let moves = (to_bottom.into_iter().map(|panel| (panel, Place::Bottom)))
        .chain(to_sidebar.into_iter().map(|panel| (panel, Place::Sidebar)));
    for (panel, to) in moves.collect::<Vec<_>>() {
        move_between((sidebar, bottom), panel, to, None, cx);
    }
}

/// Takes the tool window `panel` from the dock it isn't placed `to` and
/// puts it in the one it is, notes its place, unshown: in the bottom dock
/// where `at` says, else its focused group. False if it wasn't there.
fn move_between(
    (sidebar, bottom): (&Entity<Dock>, &Entity<kiln_shell::Dock>),
    panel: PanelId,
    to: Place,
    at: Option<DockDrop>,
    cx: &mut App,
) -> bool {
    let taken: Option<DockedPanel> = match to {
        Place::Sidebar => bottom.update(cx, |dock, cx| {
            let taken = dock.dock_mut().take_panel(panel);
            cx.notify();
            taken
        }),
        Place::Bottom => sidebar.update(cx, |dock, cx| dock.take(panel, cx)),
    };
    let Some(taken) = taken else {
        return false;
    };
    match to {
        Place::Sidebar => sidebar.update(cx, |dock, cx| dock.insert(taken, cx)),
        Place::Bottom => bottom.update(cx, |dock, cx| {
            insert_in_bottom(dock.dock_mut(), taken, at);
            cx.notify();
        }),
    }
    sidebar.update(cx, |dock, cx| dock.set_place(panel, to, cx));
    if to == Place::Bottom && at.is_none() {
        rejoin_saved_group((sidebar, bottom), cx);
    }
    true
}

/// A tool window made on first use has come to the bottom dock: puts the
/// groups as a restore saved them ([`crate::dock::PendingGroups`]) back
/// for it, unless they've been changed since, the saved layout is done
/// with once every such tool window has come.
fn rejoin_saved_group((sidebar, bottom): (&Entity<Dock>, &Entity<kiln_shell::Dock>), cx: &mut App) {
    let Some(pending) = sidebar.read(cx).pending_groups().cloned() else {
        return;
    };
    let groups = |dock: &kiln_shell::Dock, cx: &App| dock.dock().save(cx).get("groups").cloned();
    if groups(bottom.read(cx), cx) == pending.left {
        bottom.update(cx, |dock, cx| {
            dock.dock_mut().restore(&pending.saved, cx);
            cx.notify();
        });
    }
    let left = groups(bottom.read(cx), cx);
    let here = bottom.read(cx).dock().panel_ids();
    let waiting = crate::dock::LAZY_TOOLS.iter().any(|panel| {
        let panel = panel.id();
        sidebar.read(cx).place_of(panel) == Place::Bottom && !here.contains(&panel)
    });
    let pending = waiting.then_some(crate::dock::PendingGroups { left, ..pending });
    sidebar.update(cx, |dock, _| dock.keep_pending_groups(pending));
}

/// Puts `panel` in the bottom dock: a bottom tool window in its place among
/// the tabs, a sidebar panel after them; then in the group `at` names, or
/// else in the focused group, if that isn't the first.
fn insert_in_bottom(dock: &mut DockState, panel: DockedPanel, at: Option<DockDrop>) {
    let id = panel.id();
    let rank = |id: PanelId| BOTTOM_TABS.iter().position(|tab| *tab == id.0);
    let own = rank(id).unwrap_or(BOTTOM_TABS.len());
    let at_rail = (dock.panel_ids().into_iter())
        .position(|other| rank(other).unwrap_or(BOTTOM_TABS.len()) > own)
        .unwrap_or(usize::MAX);
    let focused = dock.active().and_then(|active| dock.group_of(active));
    dock.insert_panel(panel, at_rail);
    let to = match at.and_then(DockDrop::move_to) {
        Some(to) => Some(to),
        None => focused
            .filter(|&group| group > 0)
            .map(|group| MoveTo::Group {
                group,
                index: usize::MAX,
            }),
    };
    if let Some(to) = to {
        dock.move_panel(id, to);
    }
}

/// Where the terminal starts: the launch root, else the launch file's
/// directory, else the home directory.
pub(super) fn terminal_dir(launch: &Launch) -> PathBuf {
    let file_dir = || launch.file.as_ref()?.0.parent().map(Path::to_owned);
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    (launch.root.clone().or_else(file_dir).or_else(home)).unwrap_or_else(|| PathBuf::from("/"))
}
