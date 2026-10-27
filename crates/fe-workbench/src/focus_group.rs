//! Focus Editor Group 1-4: `cmd-1` .. `cmd-4`, as VS Code's
//! `workbench.action.focusFirstEditorGroup` .. `focusFourthEditorGroup`.

use gpui::{Context, Window};

use crate::Workbench;

impl Workbench {
    /// Activates the `n`th pane (1-based) in layout order, left to right
    /// then top to bottom, and focuses its active tab. Past the last pane,
    /// does nothing.
    pub(crate) fn focus_group(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = n.checked_sub(1).and_then(|index| {
            self.pane_group(cx)
                .panes()
                .get(index)
                .map(|pane| (*pane).clone())
        }) else {
            return;
        };
        self.activate(pane, window, cx);
    }
}
