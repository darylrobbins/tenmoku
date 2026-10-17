//! The workbench's typed commands: one `#[kiln::command]` block on
//! [`Workbench`], in its key context, whose handlers [`Workbench`]'s root
//! element attaches. Each method keeps its command's ID, title and context
//! from the `fe-command` registry it moved from, so keymaps and menus still
//! name it.

use gpui::{App, Window};
use kiln::Cx;

use crate::{Pane, Workbench};

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
}

impl Workbench {
    /// Runs `run` on the active pane, then redraws it.
    fn on_active_pane(
        &self,
        cx: &mut Cx<Self>,
        run: impl FnOnce(&mut Pane, &mut Window, &mut App),
    ) {
        let Cx { window, cx } = cx;
        self.pane().clone().update(*cx, |pane, cx| {
            run(pane, window, cx);
            cx.notify();
        });
    }
}
