//! Vim's window keys back out of a dock, the way [`super`]'s lead in: with
//! vim on, in the sidebar's lists (the project tree, the outline,
//! References, the Quickfix List, search's results and the Changes panel) and in
//! the bottom panel's tool windows (Problems, the terminal and the Git
//! log), `<C-w>h`, `j`, `k` and `l` run `dock.focus_left`,
//! `_down`, `_up` and `_right`, which focus the neighbour of the dock with
//! focus that way, if it has one: the editor, the other dock, or nothing.
//! `<C-w>p` and `<C-w>w`, and escape in a list, run `dock.focus_editor`:
//! the active pane's editor, the pane last active, as nvim's `<C-w>p` goes
//! back to the previous window.
//!
//! From a side dock, toward the panes is the editor. From the sidebar,
//! down is the terminal's dock, while it shows at the bottom. From there,
//! up goes back to the sidebar if a window key brought focus from it, else
//! to the editor, and left or right to the sidebar shown on that side.

use gpui::{App, Context, Focusable as _, Window};
use kiln_shell::{DockPosition, DockView as _};

use crate::{Direction, Workbench};

/// A dock with focus: the sidebar or the terminal's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focused {
    Sidebar,
    Terminal,
}

/// Where a window key leaves a dock for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Neighbour {
    Editor,
    Sidebar,
    Terminal,
    /// The shown tool window of the next (`1`) or previous (`-1`) bottom
    /// panel group.
    Group(isize),
    /// Up from the terminal's dock at the bottom: the sidebar if a window
    /// key brought focus from it, else the editor.
    Back,
}

impl Workbench {
    /// `dock.focus_left` and the rest: focuses the neighbour `direction`
    /// of the dock with focus. Nothing happens if no dock has focus or
    /// nothing lies that way.
    pub(crate) fn focus_from_dock(
        &mut self,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.focused_dock(window, cx) else {
            return;
        };
        match self.neighbour(from, direction, window, cx) {
            Some(Neighbour::Editor) => self.focus_editor_from_dock(window, cx),
            Some(Neighbour::Sidebar) => {
                self.focus_sidebar(window, cx);
            }
            Some(Neighbour::Terminal) => {
                self.focus_bottom_panel(window, cx);
                // Kept until focus leaves the tool window, whichever it is.
                self.bottom_from_sidebar = from == Focused::Sidebar;
            }
            Some(Neighbour::Group(by)) => {
                self.terminal_dock().clone().update(cx, |dock, cx| {
                    // Focus may have come to a group since the last draw.
                    dock.dock_mut().follow_focus(window, cx);
                    dock.dock_mut().focus_next_group(by, window, cx);
                });
            }
            Some(Neighbour::Back) => {
                let back = self.bottom_from_sidebar && self.focus_sidebar(window, cx);
                if !back {
                    self.focus_editor_from_dock(window, cx);
                }
            }
            None => {}
        }
    }

    /// `dock.focus_editor`: focuses the active pane's editor.
    pub(crate) fn focus_editor_from_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle(cx), cx);
    }

    /// Focuses the sidebar's shown view, true if it shows one.
    fn focus_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        (self.dock().clone()).update(cx, |dock, cx| dock.focus_active(window, cx))
    }

    /// The dock with focus, if one has.
    fn focused_dock(&self, window: &Window, cx: &App) -> Option<Focused> {
        // The bottom panel's tool windows: the terminal's dock.
        if self.bottom_panel_focused(window, cx) {
            return Some(Focused::Terminal);
        }
        let sidebar = self.dock().read(cx);
        let mut views = [
            sidebar.project().focus_handle(cx),
            sidebar.outline().focus_handle(cx),
            sidebar.search().focus_handle(cx),
            sidebar.changes().focus_handle(cx),
            sidebar.references().focus_handle(cx),
        ]
        .into_iter()
        .chain(sidebar.history().map(|panel| panel.focus_handle(cx)))
        .chain(sidebar.quickfix().map(|panel| panel.focus_handle(cx)))
        .chain(sidebar.chat().map(|panel| panel.focus_handle(cx)));
        (views.any(|focus| focus.contains_focused(window, cx))).then_some(Focused::Sidebar)
    }

    /// What lies `direction` of `from`, wherever the docks have been moved.
    fn neighbour(
        &self,
        from: Focused,
        direction: Direction,
        window: &Window,
        cx: &App,
    ) -> Option<Neighbour> {
        let sidebar = self.dock().read(cx);
        let terminal = self.terminal_dock().read(cx);
        let at = match from {
            Focused::Sidebar => sidebar.state().position(),
            Focused::Terminal => terminal.dock().position(),
        };
        if from == Focused::Terminal
            && let Some(by) = self.group_step(at, direction, window, cx)
        {
            return Some(Neighbour::Group(by));
        }
        let side = match direction {
            Direction::Left => DockPosition::Left,
            Direction::Right => DockPosition::Right,
            Direction::Up | Direction::Down => return vertical(from, at, direction, terminal),
        };
        if at != side && at != DockPosition::Bottom {
            // Toward the panes.
            return Some(Neighbour::Editor);
        }
        let sidebar_there = sidebar.is_visible() && sidebar.state().position() == side;
        (from == Focused::Terminal && at == DockPosition::Bottom && sidebar_there)
            .then_some(Neighbour::Sidebar)
    }
}

impl Workbench {
    /// Which way along the bottom panel's groups `direction` goes, if its
    /// groups lie along it and one is drawn that way: left and right at the
    /// bottom, up and down at a side.
    fn group_step(
        &self,
        at: DockPosition,
        direction: Direction,
        window: &Window,
        cx: &App,
    ) -> Option<isize> {
        let by = match (at == DockPosition::Bottom, direction) {
            (true, Direction::Left) | (false, Direction::Up) => -1,
            (true, Direction::Right) | (false, Direction::Down) => 1,
            _ => return None,
        };
        let panels = self.terminal_dock().read(cx).dock().shown_panels();
        let here = panels.iter().position(|&panel| {
            (self.bottom_tool_focus(panel, cx))
                .is_some_and(|focus| focus.contains_focused(window, cx))
        })?;
        let there = here.checked_add_signed(by)?;
        (there < panels.len()).then_some(by)
    }
}

/// What lies up or down from `from`, at `at`: the panes and the sidebar
/// above the terminal's dock at the bottom, which is below the sidebar
/// while it shows.
fn vertical(
    from: Focused,
    at: DockPosition,
    direction: Direction,
    terminal: &kiln_shell::Dock,
) -> Option<Neighbour> {
    match (from, direction) {
        (Focused::Terminal, Direction::Up) if at == DockPosition::Bottom => Some(Neighbour::Back),
        (Focused::Sidebar, Direction::Down)
            if terminal.is_visible() && terminal.dock().position() == DockPosition::Bottom =>
        {
            Some(Neighbour::Terminal)
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "dock_leave_tests.rs"]
mod dock_leave_tests;
