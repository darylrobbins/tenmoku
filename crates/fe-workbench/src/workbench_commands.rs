//! The workbench's typed commands: one `#[kiln::command]` block on
//! [`Workbench`], in its key context, whose handlers [`Workbench`]'s root
//! element attaches. Each method keeps its command's ID, title and context
//! from the `fe-command` registry it moved from, so keymaps and menus still
//! name it.

use gpui::{App, Window};
use kiln::Cx;

use crate::{Axis, Direction, Pane, Workbench};

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

    /// Notes the command running in `commands_run`, for tests.
    fn ran(&mut self, cx: &Cx<Self>) {
        #[cfg(test)]
        self.commands_run.extend(kiln::current_invocation(cx.cx));
        #[cfg(not(test))]
        let _ = (self, cx);
    }
}
