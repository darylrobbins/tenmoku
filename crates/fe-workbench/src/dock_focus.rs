//! Vim's window keys past the editor's edge: with vim on, `pane.focus_left`,
//! `_right`, `_up` and `_down` (`<C-w>h`, `l`, `k`, `j`) from a pane with
//! no neighbour that way focus the dock on that side, the sidebar or the
//! terminal's, wherever they have been moved, if it shows: the sidebar's
//! shown view ([`crate::Dock::focus_active`]) or the terminal. A hidden
//! dock stays hidden, its show commands' to open, and with vim off the
//! keys stop at the edge, as VS Code's do.

use gpui::{Context, Focusable as _, Window};
use kiln_shell::{DockPosition, DockView as _};

use crate::{Direction, Workbench};

impl Workbench {
    /// Focuses the dock shown on `direction`'s side of the panes, true if
    /// there's one to focus.
    pub(crate) fn focus_dock_beyond(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let position = match direction {
            Direction::Left => DockPosition::Left,
            Direction::Right => DockPosition::Right,
            Direction::Down => DockPosition::Bottom,
            Direction::Up => return false,
        };
        if self.dock().read(cx).state().position() == position {
            return (self.dock().clone()).update(cx, |dock, cx| dock.focus_active(window, cx));
        }
        let terminal = self.terminal_dock().read(cx);
        if terminal.dock().position() != position || !terminal.is_visible() {
            return false;
        }
        window.focus(&self.terminal().focus_handle(cx), cx);
        true
    }
}

#[cfg(test)]
#[path = "dock_focus_tests.rs"]
mod dock_focus_tests;
