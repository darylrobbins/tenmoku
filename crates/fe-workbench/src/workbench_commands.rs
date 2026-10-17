//! The workbench's typed commands: one `#[kiln::command]` block on
//! [`Workbench`], in its key context, whose handlers [`Workbench`]'s root
//! element attaches. Each method keeps its command's ID, title and context
//! from the `fe-command` registry it moved from, so keymaps and menus still
//! name it.

use fe_command::ContextPredicate;
use gpui::{App, Context, Window};
use kiln::Cx;

use kiln_shell::DockPosition;

use crate::{
    Axis, Direction, Pane, Workbench, dock_layout, nav::NavOp, open::OpenOp, persist,
    workbench::recent_picker::RecentOp,
};

#[kiln::command(context = "workbench")]
impl Workbench {
    /// Close Tab
    #[command(id = "workbench.close_tab")]
    fn close_tab(&mut self, cx: &mut Cx<Self>) {
        self.on_active_pane(cx, |pane, window, cx| pane.close_active(window, cx));
    }

    /// Next Tab
    #[command(id = "workbench.next_tab")]
    fn next_tab(&mut self, cx: &mut Cx<Self>) {
        self.on_active_pane(cx, |pane, window, cx| pane.cycle(false, window, cx));
    }

    /// Previous Tab
    #[command(id = "workbench.previous_tab")]
    fn previous_tab(&mut self, cx: &mut Cx<Self>) {
        self.on_active_pane(cx, |pane, window, cx| pane.cycle(true, window, cx));
    }

    /// Split Right
    #[command(id = "pane.split_right")]
    fn split_right(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.split(Direction::Right, cx.window, cx.cx);
    }

    /// Split Down
    #[command(id = "pane.split_down")]
    fn split_down(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.split(Direction::Down, cx.window, cx.cx);
    }

    /// Focus Left Pane
    #[command(id = "pane.focus_left")]
    fn focus_left(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_pane(Direction::Left, cx.window, cx.cx);
    }

    /// Focus Right Pane
    #[command(id = "pane.focus_right")]
    fn focus_right(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_pane(Direction::Right, cx.window, cx.cx);
    }

    /// Focus Pane Above
    #[command(id = "pane.focus_up")]
    fn focus_up(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_pane(Direction::Up, cx.window, cx.cx);
    }

    /// Focus Pane Below
    #[command(id = "pane.focus_down")]
    fn focus_down(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_pane(Direction::Down, cx.window, cx.cx);
    }

    /// Move Tab Left
    #[command(id = "pane.move_tab_left")]
    fn move_tab_left(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_tab(Direction::Left, cx.window, cx.cx);
    }

    /// Move Tab Right
    #[command(id = "pane.move_tab_right")]
    fn move_tab_right(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_tab(Direction::Right, cx.window, cx.cx);
    }

    /// Move Tab Up
    #[command(id = "pane.move_tab_up")]
    fn move_tab_up(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_tab(Direction::Up, cx.window, cx.cx);
    }

    /// Move Tab Down
    #[command(id = "pane.move_tab_down")]
    fn move_tab_down(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_tab(Direction::Down, cx.window, cx.cx);
    }

    /// Close Pane
    #[command(id = "pane.close")]
    fn close_pane(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.close_active_pane(cx.window, cx.cx);
    }

    /// Grow Pane Width
    #[command(id = "pane.grow_width")]
    fn grow_pane_width(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.resize_active_pane(Axis::Horizontal, true, cx);
    }

    /// Shrink Pane Width
    #[command(id = "pane.shrink_width")]
    fn shrink_pane_width(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.resize_active_pane(Axis::Horizontal, false, cx);
    }

    /// Grow Pane Height
    #[command(id = "pane.grow_height")]
    fn grow_pane_height(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.resize_active_pane(Axis::Vertical, true, cx);
    }

    /// Shrink Pane Height
    #[command(id = "pane.shrink_height")]
    fn shrink_pane_height(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.resize_active_pane(Axis::Vertical, false, cx);
    }

    /// Equalize Panes
    #[command(id = "pane.equalize")]
    fn equalize(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.equalize_panes(cx);
    }

    /// Focus Next Pane
    #[command(id = "pane.focus_next")]
    fn focus_next(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_next_pane(true, cx.window, cx.cx);
    }

    /// Focus Previous Pane
    #[command(id = "pane.focus_previous")]
    fn focus_previous(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.focus_next_pane(false, cx.window, cx.cx);
    }

    /// Grow Dock
    #[command(id = "dock.grow")]
    fn grow_dock(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.resize_focused_dock(dock_layout::STEP, cx.window, cx.cx);
    }

    /// Shrink Dock
    #[command(id = "dock.shrink")]
    fn shrink_dock(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.resize_focused_dock(-dock_layout::STEP, cx.window, cx.cx);
    }

    /// Move Terminal to Bottom
    #[command(id = "terminal.move_to_bottom")]
    fn move_terminal_to_bottom(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_terminal(DockPosition::Bottom, cx.cx);
    }

    /// Move Terminal to Left
    #[command(id = "terminal.move_to_left")]
    fn move_terminal_to_left(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_terminal(DockPosition::Left, cx.cx);
    }

    /// Move Terminal to Right
    #[command(id = "terminal.move_to_right")]
    fn move_terminal_to_right(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_terminal(DockPosition::Right, cx.cx);
    }

    /// Move Sidebar to Left
    #[command(id = "dock.move_sidebar_left")]
    fn move_sidebar_left(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_sidebar(DockPosition::Left, cx.cx);
    }

    /// Move Sidebar to Right
    #[command(id = "dock.move_sidebar_right")]
    fn move_sidebar_right(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.move_sidebar(DockPosition::Right, cx.cx);
    }

    /// Toggle Status Bar
    #[command(id = "view.toggle_status_bar")]
    fn toggle_status_bar(&mut self, cx: &mut Cx<Self>) {
        self.set_status_bar_visible(!self.status_bar_visible(), cx.cx);
        persist::save_status_bar(self.status_bar_visible(), cx.cx);
    }

    /// Change Language Mode…
    #[command(id = "language.select")]
    fn select_language_mode(&mut self, cx: &mut Cx<Self>) {
        self.select_language(cx.window, cx.cx);
    }

    /// Go to Definition
    #[command(id = "nav.goto_definition")]
    fn go_to_definition(&mut self, cx: &mut Cx<Self>) {
        self.nav_command(NavOp::GotoDefinition, cx);
    }

    /// Go to Symbol in Workspace…
    #[command(id = "nav.workspace_symbols")]
    fn workspace_symbols(&mut self, cx: &mut Cx<Self>) {
        self.nav_command(NavOp::WorkspaceSymbols, cx);
    }

    /// Go Back
    #[command(id = "nav.back")]
    fn go_back(&mut self, cx: &mut Cx<Self>) {
        self.nav_command(NavOp::Back, cx);
    }

    /// Go Forward
    #[command(id = "nav.forward")]
    fn go_forward(&mut self, cx: &mut Cx<Self>) {
        self.nav_command(NavOp::Forward, cx);
    }

    /// Open Recent…
    #[command(id = "workbench.open_recent")]
    fn open_recent(&mut self, cx: &mut Cx<Self>) {
        self.recent_command(RecentOp::Open, cx);
    }

    /// Clear Recently Opened
    #[command(id = "workbench.clear_recent")]
    fn clear_recent(&mut self, cx: &mut Cx<Self>) {
        self.recent_command(RecentOp::Clear, cx);
    }

    /// Open Recent in New Window
    #[command(id = "recent.open_in_new_window", context = "picker", palette = false)]
    fn open_recent_in_new_window(&mut self, cx: &mut Cx<Self>) {
        self.recent_command(RecentOp::OpenInNewWindow, cx);
    }

    /// Restart Language Server
    #[command(id = "lsp.restart")]
    fn restart_language_server(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.run_server_op(false, cx.window, cx.cx);
    }

    /// Language Server Log
    #[command(id = "lsp.show_log")]
    fn show_language_server_log(&mut self, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.run_server_op(true, cx.window, cx.cx);
    }

    /// Rename Symbol…
    #[command(id = "lsp.rename", context = "editor")]
    fn rename_symbol_command(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| this.rename_symbol(window, cx));
    }

    /// Code Actions…
    #[command(id = "lsp.code_actions", context = "editor")]
    fn code_actions(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| this.show_code_actions(window, cx));
    }

    /// Format Document
    #[command(id = "lsp.format_document", context = "editor")]
    fn format_document(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| this.format_active(false, window, cx));
    }

    /// Format Selection
    #[command(id = "lsp.format_selection", context = "editor")]
    fn format_selection(&mut self, cx: &mut Cx<Self>) {
        self.in_editor(cx, |this, window, cx| this.format_active(true, window, cx));
    }

    /// Open Folder…
    #[command(id = "workbench.open_folder")]
    fn open_folder(&mut self, cx: &mut Cx<Self>) {
        self.open_command(OpenOp::Folder, cx);
    }

    /// Open File…
    #[command(id = "workbench.open_file")]
    fn open_file(&mut self, cx: &mut Cx<Self>) {
        self.open_command(OpenOp::File, cx);
    }

    /// Settings…
    #[command(id = "workbench.open_settings")]
    fn open_settings_command(&mut self, cx: &mut Cx<Self>) {
        self.open_command(OpenOp::Settings, cx);
    }

    /// New Window
    #[command(id = "window.new")]
    fn new_window(&mut self, cx: &mut Cx<Self>) {
        self.open_command(OpenOp::NewWindow, cx);
    }

    /// Open Folder in New Window…
    #[command(id = "workbench.open_folder_in_new_window")]
    fn open_folder_in_new_window(&mut self, cx: &mut Cx<Self>) {
        self.open_command(OpenOp::FolderInNewWindow, cx);
    }

    /// New File
    #[command(id = "workbench.new_file")]
    fn new_file_command(&mut self, cx: &mut Cx<Self>) {
        self.open_command(OpenOp::NewFile, cx);
    }

    /// Save As…
    #[command(id = "workbench.save_as")]
    fn save_as(&mut self, cx: &mut Cx<Self>) {
        self.open_command(OpenOp::SaveAs, cx);
    }

    /// Trust This Folder
    #[command(id = "workbench.trust_folder")]
    fn trust_folder(&mut self, cx: &mut Cx<Self>) {
        self.trust_command(true, cx);
    }

    /// Untrust This Folder
    #[command(id = "workbench.untrust_folder")]
    fn untrust_folder(&mut self, cx: &mut Cx<Self>) {
        self.trust_command(false, cx);
    }
}

impl Workbench {
    /// Runs `run` on the active pane, then redraws it.
    fn on_active_pane(
        &mut self,
        cx: &mut Cx<Self>,
        run: impl FnOnce(&mut Pane, &mut Window, &mut App),
    ) {
        self.ran(cx);
        let Cx { window, cx } = cx;
        self.pane().clone().update(*cx, |pane, cx| {
            run(pane, window, cx);
            cx.notify();
        });
    }

    /// Runs the nav command `op`.
    fn nav_command(&mut self, op: NavOp, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.run_nav_op(op, cx.window, cx.cx);
    }

    /// Runs the recent command `op`.
    fn recent_command(&mut self, op: RecentOp, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.run_recent_op(op, cx.window, cx.cx);
    }

    /// Runs `run`, an editor command's, if the command was invoked where
    /// the editor's context holds, as its `fe-command` registry checked:
    /// kiln runs a typed command's handler wherever it's dispatched, so a
    /// menu pick or a `RunCommand` with the project panel or terminal
    /// focused would act on the active editor. One run with no invocation,
    /// as a raw action, whose key binding checked its context, runs.
    fn in_editor(
        &mut self,
        cx: &mut Cx<Self>,
        run: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>),
    ) {
        self.ran(cx);
        let editor = ContextPredicate::identifier(fe_editor::commands::CONTEXT);
        if let Some(invocation) = kiln::current_invocation(cx.cx)
            && !editor.eval(&invocation.context)
        {
            log::warn!("{} doesn't apply here: not in {editor}", invocation.command);
            return;
        }
        run(self, cx.window, cx.cx);
    }

    /// Runs the open command `op`.
    fn open_command(&mut self, op: OpenOp, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.prompt_open(op, cx.window, cx.cx);
    }

    /// Trusts the root, if `trusted`, or untrusts it.
    fn trust_command(&mut self, trusted: bool, cx: &mut Cx<Self>) {
        self.ran(cx);
        self.set_root_trust(trusted, cx.cx);
    }

    /// Notes the command running in `commands_run`, for tests.
    fn ran(&mut self, cx: &Cx<Self>) {
        #[cfg(test)]
        self.commands_run.extend(kiln::current_invocation(cx.cx));
        #[cfg(not(test))]
        let _ = (self, cx);
    }
}
